use crate::cli::BaseMode;
use crate::commands::issue_selection::{self, SelectedIssue};
use crate::commands::issue_snapshot::{IssueSnapshot, snapshot_issues};
use crate::commands::{issue, new as new_command};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::git::GitService;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STATUS_PREPARED: &str = "prepared";
const STATUS_RUNNING: &str = "running";
const STATUS_DONE: &str = "done";
const STATUS_FAILED: &str = "failed";
const STATUS_SKIPPED: &str = "skipped";
const STATUS_PARTIAL: &str = "partial";

pub fn new(
    ctx: &Ctx,
    items: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;
    if items.is_empty() {
        bail!("Usage: wt stack new <item>...");
    }

    let now = current_utc_timestamp();
    let stack = StackMetadata {
        profile: profile.map(str::to_string),
        base_mode: base_mode_name(base).into(),
        base: explicit_base(base),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        items: stack_items_from_names(items, explicit_base(base))?,
    };
    let stack_path = write_new_stack_metadata(ctx, &stack)?;

    ctx.ui
        .print_step(&format!("Created stack: {}", stack_path.display()));
    Ok(())
}

pub fn issue(
    ctx: &Ctx,
    issues: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;

    let selected_issues = if issues.is_empty() {
        select_ordered_issues(ctx)?
            .into_iter()
            .map(|issue| issue.identifier)
            .collect::<Vec<_>>()
    } else {
        issues.to_vec()
    };

    if selected_issues.is_empty() {
        ctx.ui.print_warning("No issues selected");
        return Ok(());
    }

    let issue_snapshots = snapshot_issues(ctx, &selected_issues)?;
    let now = current_utc_timestamp();
    let stack = StackMetadata {
        profile: profile.map(str::to_string),
        base_mode: base_mode_name(base).into(),
        base: explicit_base(base),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        items: stack_items_from_snapshots(issue_snapshots, explicit_base(base)),
    };
    let stack_path = write_new_stack_metadata(ctx, &stack)?;

    ctx.ui
        .print_step(&format!("Created stack: {}", stack_path.display()));
    Ok(())
}

pub fn run(ctx: &Ctx, stack: &str) -> Result<()> {
    let stack_path = resolve_stack_path(ctx, stack)?;
    let mut metadata = read_stack_metadata(&stack_path)?;
    validate_profile(ctx, metadata.profile.as_deref())?;

    if metadata.items.is_empty() {
        bail!("Stack has no items: {}", stack_path.display());
    }

    if let Some(item) = metadata
        .items
        .iter()
        .find(|item| item.status == STATUS_RUNNING)
    {
        bail!(
            "Stack item {} is already running. Mark it complete with: wt stack complete {} {}",
            item.label(),
            stack_path.display(),
            item.label()
        );
    }

    let Some(idx) = next_runnable_item(&metadata.items) else {
        ctx.ui
            .print_step("No prepared or failed items to run in this stack.");
        metadata.status = summarize_stack_status(&metadata.items);
        metadata.updated_at = current_utc_timestamp();
        write_stack_metadata(&stack_path, &metadata)?;
        return Ok(());
    };

    let parent = parent_for_item(ctx, &metadata, idx)?;
    metadata.status = STATUS_RUNNING.into();
    metadata.updated_at = current_utc_timestamp();
    metadata.items[idx].status = STATUS_RUNNING.into();
    metadata.items[idx].parent = Some(parent.clone());
    metadata.items[idx].error.clear();
    write_stack_metadata(&stack_path, &metadata)?;

    let result = run_stack_item(
        ctx,
        &stack_path,
        &metadata.items[idx],
        &parent,
        metadata.profile.as_deref(),
    );

    match result {
        Ok(result) => {
            metadata.items[idx].branch = result.branch_name;
            metadata.items[idx].status = STATUS_RUNNING.into();
            metadata.items[idx].error.clear();
            ctx.ui.print_step(&format!(
                "Started stack item {}. Mark it complete with: wt stack complete {} {}",
                metadata.items[idx].label(),
                stack_path.display(),
                metadata.items[idx].label()
            ));
        }
        Err(err) => {
            if err
                .downcast_ref::<WtError>()
                .is_some_and(|err| matches!(err, WtError::Cancelled))
            {
                metadata.items[idx].status = STATUS_SKIPPED.into();
                metadata.items[idx].error = "User cancelled".into();
                metadata.status = summarize_stack_status(&metadata.items);
                metadata.updated_at = current_utc_timestamp();
                write_stack_metadata(&stack_path, &metadata)?;
                return Ok(());
            }

            metadata.items[idx].status = STATUS_FAILED.into();
            metadata.items[idx].error = err.to_string();
        }
    }

    metadata.status = summarize_stack_status(&metadata.items);
    metadata.updated_at = current_utc_timestamp();
    write_stack_metadata(&stack_path, &metadata)?;
    ctx.ui
        .print_step(&format!("Stack status: {}", metadata.status));

    if metadata.status == STATUS_FAILED {
        bail!("Stack failed: {}", stack_path.display());
    }

    Ok(())
}

pub fn complete(ctx: &Ctx, stack: &str, item: Option<&str>, run_next: bool) -> Result<()> {
    let stack_path = resolve_stack_path(ctx, stack)?;
    let mut metadata = read_stack_metadata(&stack_path)?;

    let Some(idx) = metadata
        .items
        .iter()
        .position(|item| item.status == STATUS_RUNNING)
    else {
        ctx.ui.print_warning("No running stack item found");
        return Ok(());
    };

    if let Some(item) = item {
        let running = &metadata.items[idx];
        if !stack_item_matches(running, item) {
            bail!(
                "Running stack item is {}, but complete was requested for {item}",
                running.label()
            );
        }
    }

    validate_completable_stack_item(ctx, &metadata.items[idx])?;

    metadata.items[idx].status = STATUS_DONE.into();
    metadata.items[idx].error.clear();
    metadata.status = summarize_stack_status(&metadata.items);
    metadata.updated_at = current_utc_timestamp();
    write_stack_metadata(&stack_path, &metadata)?;

    ctx.ui
        .print_step(&format!("Marked {} done", metadata.items[idx].label()));
    if run_next {
        run(ctx, stack_path.to_string_lossy().as_ref())?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct StackMetadata {
    profile: Option<String>,
    base_mode: String,
    base: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
    items: Vec<StackItem>,
}

#[derive(Debug, Deserialize)]
struct RawStackMetadata {
    #[serde(default)]
    profile: Option<String>,
    base_mode: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(default = "default_stack_status")]
    status: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    items: Vec<StackItem>,
    #[serde(default)]
    issues: Vec<StackItem>,
}

impl RawStackMetadata {
    fn into_metadata(mut self) -> Result<StackMetadata> {
        if !self.items.is_empty() && !self.issues.is_empty() {
            bail!("Stack TOML cannot contain both [[items]] and legacy [[issues]]");
        }

        let mut items = if self.items.is_empty() {
            for item in &mut self.issues {
                if item.kind.trim().is_empty() || item.kind == "item" {
                    item.kind = "issue".into();
                }
            }
            self.issues
        } else {
            self.items
        };
        for item in &mut items {
            item.normalize();
        }

        Ok(StackMetadata {
            profile: self.profile,
            base_mode: self.base_mode,
            base: self.base,
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            items,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct StackItem {
    #[serde(default = "default_item_kind")]
    kind: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    snapshot: Option<String>,
    #[serde(default)]
    body: String,
    #[serde(default = "default_issue_status")]
    status: String,
    #[serde(default)]
    error: String,
}

impl StackItem {
    fn from_snapshot(snapshot: IssueSnapshot, parent: Option<String>) -> Self {
        Self {
            kind: "issue".into(),
            id: snapshot.id,
            source: snapshot.source,
            title: snapshot.title,
            branch: snapshot.branch,
            parent,
            snapshot: Some(snapshot.snapshot),
            body: String::new(),
            status: STATUS_PREPARED.into(),
            error: String::new(),
        }
    }

    fn label(&self) -> String {
        if !self.id.trim().is_empty() {
            return self.id.clone();
        }
        if !self.branch.trim().is_empty() {
            return self.branch.clone();
        }
        if !self.title.trim().is_empty() {
            return self.title.clone();
        }
        "stack-item".into()
    }

    fn title(&self) -> String {
        if !self.title.trim().is_empty() {
            self.title.clone()
        } else {
            self.label()
        }
    }

    fn kind(&self) -> &str {
        if self.kind.trim().is_empty() {
            if self.snapshot.is_some() {
                "issue"
            } else {
                "item"
            }
        } else {
            self.kind.as_str()
        }
    }

    fn normalize(&mut self) {
        if self.id.trim().is_empty() {
            self.id = if !self.source.trim().is_empty() {
                self.source.clone()
            } else if !self.branch.trim().is_empty() {
                self.branch.clone()
            } else {
                self.title.clone()
            };
        }
        if self.title.trim().is_empty() {
            self.title = self.label();
        }
        if self.kind.trim().is_empty() {
            self.kind = if self.snapshot.is_some() {
                "issue".into()
            } else {
                "item".into()
            };
        }
    }
}

fn select_ordered_issues(ctx: &Ctx) -> Result<Vec<SelectedIssue>> {
    let selected = issue_selection::select_issues(ctx, "Select issues for stack")?;
    if selected.len() <= 1 {
        return Ok(selected);
    }

    ctx.ui.print_step("Stack order (base -> top):");
    for (idx, issue) in selected.iter().enumerate() {
        ctx.ui
            .print_dim(&format!("  {}. {}", idx + 1, issue.display));
    }

    let default_order = (1..=selected.len())
        .map(|idx| idx.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let raw_order = ctx
        .ui
        .input("Stack order (base -> top)", Some(&default_order))?;
    let order = parse_order(&raw_order, selected.len())?;
    Ok(order.into_iter().map(|idx| selected[idx].clone()).collect())
}

fn parse_order(raw: &str, len: usize) -> Result<Vec<usize>> {
    let numbers = raw
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<usize>()
                .with_context(|| format!("Invalid stack order item: {part}"))
        })
        .collect::<Result<Vec<_>>>()?;

    if numbers.len() != len {
        bail!("Stack order must include each selected issue exactly once");
    }

    let mut seen = vec![false; len];
    let mut order = Vec::new();
    for number in numbers {
        if number == 0 || number > len {
            bail!("Stack order item out of range: {number}");
        }
        let idx = number - 1;
        if seen[idx] {
            bail!("Stack order includes duplicate item: {number}");
        }
        seen[idx] = true;
        order.push(idx);
    }

    Ok(order)
}

fn stack_items_from_snapshots(
    snapshots: Vec<IssueSnapshot>,
    initial_parent: Option<String>,
) -> Vec<StackItem> {
    let mut parent = initial_parent;
    snapshots
        .into_iter()
        .map(|snapshot| {
            let issue_parent = parent.clone();
            parent = prepared_branch_name(&snapshot.branch).map(str::to_string);
            StackItem::from_snapshot(snapshot, issue_parent)
        })
        .collect()
}

fn stack_items_from_names(
    names: &[String],
    initial_parent: Option<String>,
) -> Result<Vec<StackItem>> {
    let mut seen_branches = HashSet::new();
    let mut parent = initial_parent;
    let mut items = Vec::new();

    for name in names {
        let title = name.trim();
        let branch = new_command::branch_name_from_words(&[title.to_string()])?;
        if !seen_branches.insert(branch.clone()) {
            bail!("Duplicate stack item branch: {branch}");
        }

        let item_parent = parent.clone();
        parent = Some(branch.clone());
        items.push(StackItem {
            kind: "new".into(),
            id: branch.clone(),
            source: String::new(),
            title: title.to_string(),
            branch,
            parent: item_parent,
            snapshot: None,
            body: String::new(),
            status: STATUS_PREPARED.into(),
            error: String::new(),
        });
    }

    Ok(items)
}

fn default_stack_status() -> String {
    STATUS_PREPARED.into()
}

fn default_item_kind() -> String {
    "item".into()
}

fn default_issue_status() -> String {
    STATUS_PREPARED.into()
}

fn validate_profile(ctx: &Ctx, profile: Option<&str>) -> Result<()> {
    let Some(profile) = profile else {
        return Ok(());
    };

    validate_profile_name(profile)?;
    if Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?.is_none() {
        bail!("Profile '{profile}' not found");
    }

    Ok(())
}

fn run_stack_item(
    ctx: &Ctx,
    stack_path: &Path,
    stack_item: &StackItem,
    parent: &str,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let (snapshot_path, content) = stack_item_content(ctx, stack_item, parent)?;
    let content = format!(
        "{}\n\n## Stack Completion\n\nWhen this item is complete and committed, run:\n\n```bash\nwt stack complete {} {} --run-next\n```",
        content.trim_end(),
        stack_path.display(),
        stack_item.label()
    );
    let branch_name = prepared_branch_name(&stack_item.branch);
    if branch_name.is_none() {
        bail!("Stack item {} has no branch", stack_item.label());
    }
    let base = Some(parent.to_string());
    let identifier = stack_item.label();
    let title = stack_item.title();
    let mode = if stack_item.snapshot.is_some() {
        "issue"
    } else {
        "new"
    };
    let prompt_intro = if stack_item.snapshot.is_some() {
        "Use this issue snapshot before changing code."
    } else {
        "Use this stack item before changing code."
    };
    let path_label = if stack_item.snapshot.is_some() {
        "Snapshot path"
    } else {
        "Stack item"
    };

    issue::run_with_issue_snapshot(
        ctx,
        &base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &identifier,
            title: &title,
            branch_name,
            mode,
            prompt_intro,
            snapshot: issue::IssueSnapshotContext {
                path_label,
                path: &snapshot_path,
                content: &content,
            },
        },
    )
}

fn stack_item_content(ctx: &Ctx, item: &StackItem, parent: &str) -> Result<(String, String)> {
    if let Some(snapshot_path) = item.snapshot.as_deref() {
        let content = fs::read_to_string(ctx.repo_root.join(snapshot_path))
            .with_context(|| format!("Failed to read issue snapshot: {snapshot_path}"))?;
        return Ok((snapshot_path.to_string(), content));
    }

    let path = format!("stack:{}", item.label());
    let mut content = format!(
        "# {}\n\n- Kind: `{}`\n- Branch: `{}`\n- Parent: `{}`\n",
        item.title(),
        item.kind(),
        item.branch,
        parent
    );
    if !item.body.trim().is_empty() {
        content.push_str("\n## Body\n\n");
        content.push_str(item.body.trim());
        content.push('\n');
    }
    Ok((path, content))
}

fn stack_item_matches(item: &StackItem, target: &str) -> bool {
    item.id == target
        || item.source == target
        || item.title == target
        || prepared_branch_name(&item.branch) == Some(target)
        || item.branch.rsplit('/').next() == Some(target)
}

fn validate_completable_stack_item(ctx: &Ctx, item: &StackItem) -> Result<()> {
    let branch = prepared_branch_name(&item.branch)
        .ok_or_else(|| anyhow::anyhow!("Stack item {} has no branch", item.label()))?;
    let parent = item
        .parent
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Stack item {} has no parent", item.label()))?;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    if let Some(path) = git.checked_out_path(branch)? {
        let status = git.status_porcelain(&path)?;
        let relevant_status = relevant_worktree_status(ctx, &status);
        if !relevant_status.trim().is_empty() {
            bail!(
                "Stack item {} has uncommitted changes in {}. Commit or stash them before completing.\n{}",
                item.label(),
                path.display(),
                relevant_status.trim_end()
            );
        }
    }

    if !git.branch_has_commits_ahead(parent, branch)? {
        bail!(
            "Stack item {} has no commits ahead of parent {parent}. Commit the item work before completing.",
            item.label()
        );
    }

    Ok(())
}

fn relevant_worktree_status(ctx: &Ctx, status: &str) -> String {
    status
        .lines()
        .filter(|line| !is_configured_link_status_line(ctx, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_configured_link_status_line(ctx: &Ctx, line: &str) -> bool {
    let Some(path) = porcelain_status_path(line) else {
        return false;
    };

    ctx.config
        .worktree
        .link
        .iter()
        .map(|linked| linked.trim_end_matches('/'))
        .any(|linked| path == linked || path.starts_with(&format!("{linked}/")))
}

fn porcelain_status_path(line: &str) -> Option<&str> {
    let path = line.get(3..)?.trim();
    let path = path.rsplit(" -> ").next().unwrap_or(path);
    Some(path.trim_matches('"'))
}

fn next_runnable_item(items: &[StackItem]) -> Option<usize> {
    for (idx, item) in items.iter().enumerate() {
        match item.status.as_str() {
            STATUS_DONE | STATUS_SKIPPED => continue,
            status if is_runnable_status(status) => return Some(idx),
            _ => return None,
        }
    }
    None
}

fn parent_for_item(ctx: &Ctx, stack: &StackMetadata, idx: usize) -> Result<String> {
    if idx == 0 {
        return resolve_initial_base(ctx, stack);
    }

    let previous = &stack.items[idx - 1];
    if previous.status != STATUS_DONE && previous.status != STATUS_SKIPPED {
        bail!("Previous stack item {} is not done", previous.label());
    }

    prepared_branch_name(&previous.branch)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Previous stack item {} has no branch", previous.label()))
}

fn prepared_branch_name(branch: &str) -> Option<&str> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "-" {
        None
    } else {
        Some(branch)
    }
}

fn write_new_stack_metadata(ctx: &Ctx, stack: &StackMetadata) -> Result<PathBuf> {
    let stacks_dir = ctx.repo_root.join(".local/stacks");
    fs::create_dir_all(&stacks_dir)?;

    let date = current_utc_date();
    let mut seq = 1;
    let path = loop {
        let candidate = stacks_dir.join(format!("{date}-{seq:03}.toml"));
        if !candidate.exists() {
            break candidate;
        }
        seq += 1;
    };

    write_stack_metadata(&path, stack)?;
    Ok(path)
}

fn read_stack_metadata(path: &Path) -> Result<StackMetadata> {
    let content = fs::read_to_string(path)?;
    let raw: RawStackMetadata = toml::from_str(&content)?;
    raw.into_metadata()
}

fn write_stack_metadata(path: &Path, stack: &StackMetadata) -> Result<()> {
    let mut content = String::new();
    if let Some(profile) = stack.profile.as_deref() {
        content.push_str(&format!("profile = {}\n", toml_quote(profile)));
    }
    content.push_str(&format!("base_mode = {}\n", toml_quote(&stack.base_mode)));
    if let Some(base) = &stack.base {
        content.push_str(&format!("base = {}\n", toml_quote(base)));
    }
    content.push_str(&format!("status = {}\n", toml_quote(&stack.status)));
    content.push_str(&format!("created_at = {}\n", toml_quote(&stack.created_at)));
    content.push_str(&format!("updated_at = {}\n", toml_quote(&stack.updated_at)));

    for item in &stack.items {
        content.push_str("\n[[items]]\n");
        content.push_str(&format!("kind = {}\n", toml_quote(item.kind())));
        if !item.id.trim().is_empty() {
            content.push_str(&format!("id = {}\n", toml_quote(&item.id)));
        }
        if !item.source.trim().is_empty() {
            content.push_str(&format!("source = {}\n", toml_quote(&item.source)));
        }
        content.push_str(&format!("title = {}\n", toml_quote(&item.title())));
        content.push_str(&format!("branch = {}\n", toml_quote(&item.branch)));
        if let Some(parent) = item.parent.as_deref() {
            content.push_str(&format!("parent = {}\n", toml_quote(parent)));
        }
        if let Some(snapshot) = item.snapshot.as_deref() {
            content.push_str(&format!("snapshot = {}\n", toml_quote(snapshot)));
        }
        if !item.body.trim().is_empty() {
            content.push_str(&format!("body = {}\n", toml_multiline_string(&item.body)));
        }
        content.push_str(&format!("status = {}\n", toml_quote(&item.status)));
        content.push_str(&format!("error = {}\n", toml_quote(&item.error)));
    }

    fs::write(path, content)?;
    Ok(())
}

fn resolve_stack_path(ctx: &Ctx, target: &str) -> Result<PathBuf> {
    if target == "latest" {
        return latest_stack_path(ctx);
    }

    let path = PathBuf::from(target);
    if path.is_absolute() && path.exists() {
        return Ok(path);
    }

    let invocation_path = ctx.invocation_root.join(target);
    if invocation_path.exists() {
        return Ok(invocation_path);
    }

    let repo_path = ctx.repo_root.join(target);
    if repo_path.exists() {
        return Ok(repo_path);
    }

    if !target.ends_with(".toml") {
        let shorthand = ctx
            .repo_root
            .join(".local/stacks")
            .join(format!("{target}.toml"));
        if shorthand.exists() {
            return Ok(shorthand);
        }
    }

    bail!("Stack not found: {target}");
}

fn latest_stack_path(ctx: &Ctx) -> Result<PathBuf> {
    let stacks_dir = ctx.repo_root.join(".local/stacks");
    let mut paths = Vec::new();
    if stacks_dir.exists() {
        for entry in fs::read_dir(&stacks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .pop()
        .ok_or_else(|| anyhow::anyhow!("No stack files found in .local/stacks"))
}

fn resolve_initial_base(ctx: &Ctx, stack: &StackMetadata) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match stack.base_mode.as_str() {
        "default" => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))?
        }
        "interactive" => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            branches[idx].clone()
        }
        "explicit" => stack
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Stack base_mode is explicit but base is missing"))?,
        other => bail!("Unknown stack base_mode: {other}"),
    };

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
}

fn base_mode_name(base: &Option<String>) -> &'static str {
    match BaseMode::from_raw(base) {
        BaseMode::Default => "default",
        BaseMode::Interactive => "interactive",
        BaseMode::Explicit(_) => "explicit",
    }
}

fn explicit_base(base: &Option<String>) -> Option<String> {
    match BaseMode::from_raw(base) {
        BaseMode::Explicit(branch) => Some(branch),
        BaseMode::Default | BaseMode::Interactive => None,
    }
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, STATUS_PREPARED | STATUS_FAILED)
}

fn summarize_stack_status(items: &[StackItem]) -> String {
    if items.is_empty() {
        return STATUS_DONE.into();
    }
    if items.iter().any(|item| item.status == STATUS_FAILED) {
        return STATUS_FAILED.into();
    }
    if items.iter().any(|item| item.status == STATUS_RUNNING) {
        return STATUS_RUNNING.into();
    }
    if items
        .iter()
        .all(|item| matches!(item.status.as_str(), STATUS_DONE | STATUS_SKIPPED))
    {
        return STATUS_DONE.into();
    }
    if items.iter().all(|item| item.status == STATUS_PREPARED) {
        return STATUS_PREPARED.into();
    }
    STATUS_PARTIAL.into()
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn current_utc_date() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_multiline_string(value: &str) -> String {
    let escaped = value
        .replace("\\", "\\\\")
        .replace("\"\"\"", "\\\"\\\"\\\"");
    format!("\"\"\"\n{}\n\"\"\"", escaped.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    #[test]
    fn parse_order_accepts_comma_or_space_separated_numbers() {
        assert_eq!(parse_order("2,1,3", 3).unwrap(), vec![1, 0, 2]);
        assert_eq!(parse_order("3 1 2", 3).unwrap(), vec![2, 0, 1]);
    }

    #[test]
    fn parse_order_rejects_missing_duplicate_or_out_of_range_items() {
        assert!(parse_order("1,2", 3).is_err());
        assert!(parse_order("1,1,2", 3).is_err());
        assert!(parse_order("1,2,4", 3).is_err());
    }

    #[test]
    fn relevant_worktree_status_ignores_configured_links() {
        let config = Config {
            worktree: crate::config::WorktreeConfig {
                link: vec![".local".into()],
                ..crate::config::WorktreeConfig::default()
            },
            ..Config::default()
        };
        let ctx = Ctx::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(
            relevant_worktree_status(&ctx, "?? .local\n M src/lib.rs"),
            " M src/lib.rs"
        );
        assert_eq!(relevant_worktree_status(&ctx, "?? .local"), "");
    }

    #[test]
    fn new_creates_manual_stack_items() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let items = vec!["Add schema".into(), "Wire API".into()];

        new(&ctx, &items, None, &Some("main".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base_mode, "explicit");
        assert_eq!(stack.base.as_deref(), Some("main"));
        assert_eq!(stack.items.len(), 2);
        assert_eq!(stack.items[0].kind(), "new");
        assert_eq!(stack.items[0].id, "add-schema");
        assert_eq!(stack.items[0].title, "Add schema");
        assert_eq!(stack.items[0].branch, "add-schema");
        assert_eq!(stack.items[0].parent.as_deref(), Some("main"));
        assert!(stack.items[0].snapshot.is_none());
        assert_eq!(stack.items[1].id, "wire-api");
        assert_eq!(stack.items[1].parent.as_deref(), Some("add-schema"));

        let content = std::fs::read_to_string(stack_path).unwrap();
        assert!(content.contains("[[items]]"));
        assert!(content.contains("kind = \"new\""));
        assert!(!content.contains("snapshot ="));
        assert!(!content.contains("[[issues]]"));
    }

    #[test]
    fn new_rejects_duplicate_item_branches() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let items = vec!["Add schema".into(), "add schema".into()];

        let err = new(&ctx, &items, None, &None).unwrap_err();
        assert!(err.to_string().contains("Duplicate stack item branch"));
    }

    #[test]
    fn issue_with_no_args_selects_and_reorders_issues() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"PROJ-1","title":"Schema","state":{"name":"Todo"}},{"identifier":"PROJ-2","title":"API","state":{"name":"Todo"}}]"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-2","title":"API","branchName":"alice/proj-2-api","description":"API body"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Schema","branchName":"alice/proj-1-schema","description":"Schema body"}"#,
            true,
        );
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 1]);
        ui.add_input("2,1");
        let config = crate::config::Config {
            issues: Some(crate::config::IssuesConfig {
                provider: crate::config::IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(ui),
        );

        issue(&ctx, &[], None, &Some("main".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base.as_deref(), Some("main"));
        assert_eq!(stack.items[0].id, "PROJ-2");
        assert_eq!(stack.items[0].parent.as_deref(), Some("main"));
        assert_eq!(stack.items[1].id, "PROJ-1");
        assert_eq!(stack.items[1].parent.as_deref(), Some("alice/proj-2-api"));
        let content = std::fs::read_to_string(stack_path).unwrap();
        assert!(content.contains("[[items]]"));
        assert!(content.contains("kind = \"issue\""));
        assert!(!content.contains("[[issues]]"));
    }

    #[test]
    fn read_stack_metadata_accepts_legacy_issues_tables() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("legacy.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[issues]]
id = "PROJ-1"
source = "PROJ-1"
title = "Schema"
branch = "alice/proj-1-schema"
snapshot = ".local/issues/PROJ-1.md"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        let stack = read_stack_metadata(&stack_path).unwrap();

        assert_eq!(stack.items.len(), 1);
        assert_eq!(stack.items[0].kind(), "issue");
        assert_eq!(stack.items[0].id, "PROJ-1");
        assert_eq!(
            stack.items[0].snapshot.as_deref(),
            Some(".local/issues/PROJ-1.md")
        );
    }

    #[test]
    fn read_stack_metadata_rejects_mixed_items_and_legacy_issues() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("mixed.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"

[[items]]
title = "Manual"
branch = "manual"

[[issues]]
id = "PROJ-1"
source = "PROJ-1"
title = "Issue"
branch = "alice/proj-1"
snapshot = ".local/issues/PROJ-1.md"
"#,
        )
        .unwrap();

        assert!(read_stack_metadata(&stack_path).is_err());
    }

    #[test]
    fn run_starts_one_item_and_complete_allows_next_parent() {
        let dir = tempfile::tempdir().unwrap();
        let issues_dir = dir.path().join(".local/issues");
        std::fs::create_dir_all(&issues_dir).unwrap();
        std::fs::write(issues_dir.join("PROJ-1.md"), "# PROJ-1: Schema\n").unwrap();
        std::fs::write(issues_dir.join("PROJ-2.md"), "# PROJ-2: API\n").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/alice/proj-1-schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            items: vec![
                StackItem {
                    kind: "issue".into(),
                    id: "PROJ-1".into(),
                    source: "PROJ-1".into(),
                    title: "Schema".into(),
                    branch: "alice/proj-1-schema".into(),
                    parent: None,
                    snapshot: Some(".local/issues/PROJ-1.md".into()),
                    body: String::new(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
                StackItem {
                    kind: "issue".into(),
                    id: "PROJ-2".into(),
                    source: "PROJ-2".into(),
                    title: "API".into(),
                    branch: "alice/proj-2-api".into(),
                    parent: None,
                    snapshot: Some(".local/issues/PROJ-2.md".into()),
                    body: String::new(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
            ],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        run(&ctx, stack_path.to_str().unwrap()).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.items[0].parent.as_deref(), Some("main"));
        assert_eq!(updated.items[0].status, STATUS_RUNNING);
        assert_eq!(updated.items[1].status, STATUS_PREPARED);

        complete(&ctx, stack_path.to_str().unwrap(), Some("PROJ-1"), false).unwrap();
        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_PARTIAL);
        assert_eq!(updated.items[0].status, STATUS_DONE);

        run(&ctx, stack_path.to_str().unwrap()).unwrap();
        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(
            updated.items[1].parent.as_deref(),
            Some("alice/proj-1-schema")
        );
        assert_eq!(updated.items[1].status, STATUS_RUNNING);

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(_, args, _)| {
            args.len() == 6
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b"
                && args[3] == "alice/proj-2-api"
                && args[5] == "alice/proj-1-schema"
        }));
    }

    #[test]
    fn complete_with_run_next_starts_next_item() {
        let dir = tempfile::tempdir().unwrap();
        let issues_dir = dir.path().join(".local/issues");
        std::fs::create_dir_all(&issues_dir).unwrap();
        std::fs::write(issues_dir.join("PROJ-1.md"), "# PROJ-1: Schema\n").unwrap();
        std::fs::write(issues_dir.join("PROJ-2.md"), "# PROJ-2: API\n").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/alice/proj-1-schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/alice/proj-1-schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            items: vec![
                StackItem {
                    kind: "issue".into(),
                    id: "PROJ-1".into(),
                    source: "PROJ-1".into(),
                    title: "Schema".into(),
                    branch: "alice/proj-1-schema".into(),
                    parent: None,
                    snapshot: Some(".local/issues/PROJ-1.md".into()),
                    body: String::new(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
                StackItem {
                    kind: "issue".into(),
                    id: "PROJ-2".into(),
                    source: "PROJ-2".into(),
                    title: "API".into(),
                    branch: "alice/proj-2-api".into(),
                    parent: None,
                    snapshot: Some(".local/issues/PROJ-2.md".into()),
                    body: String::new(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
            ],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        run(&ctx, stack_path.to_str().unwrap()).unwrap();
        complete(&ctx, stack_path.to_str().unwrap(), Some("PROJ-1"), true).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.items[0].status, STATUS_DONE);
        assert_eq!(updated.items[1].status, STATUS_RUNNING);
        assert_eq!(
            updated.items[1].parent.as_deref(),
            Some("alice/proj-1-schema")
        );
    }

    #[test]
    fn complete_rejects_dirty_stack_item_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response(" M src/lib.rs", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_RUNNING.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            items: vec![StackItem {
                kind: "new".into(),
                id: "feature".into(),
                source: String::new(),
                title: "Feature".into(),
                branch: "feature".into(),
                parent: Some("main".into()),
                snapshot: None,
                body: String::new(),
                status: STATUS_RUNNING.into(),
                error: String::new(),
            }],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        let err = complete(&ctx, stack_path.to_str().unwrap(), Some("feature"), false).unwrap_err();
        assert!(err.to_string().contains("uncommitted changes"));

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.items[0].status, STATUS_RUNNING);
    }

    #[test]
    fn complete_rejects_stack_item_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("0", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_RUNNING.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            items: vec![StackItem {
                kind: "new".into(),
                id: "feature".into(),
                source: String::new(),
                title: "Feature".into(),
                branch: "feature".into(),
                parent: Some("main".into()),
                snapshot: None,
                body: String::new(),
                status: STATUS_RUNNING.into(),
                error: String::new(),
            }],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        let err = complete(&ctx, stack_path.to_str().unwrap(), Some("feature"), false).unwrap_err();
        assert!(err.to_string().contains("no commits ahead"));

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.items[0].status, STATUS_RUNNING);
    }

    #[test]
    fn run_supports_manual_item_without_issue_provider_or_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("manual.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[items]]
kind = "new"
title = "Add schema"
branch = "add-schema"
body = """
Create the schema without an issue provider.
"""
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, stack_path.to_str().unwrap()).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.items[0].id, "add-schema");
        assert_eq!(updated.items[0].parent.as_deref(), Some("main"));
        assert_eq!(updated.items[0].status, STATUS_RUNNING);

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(_, args, _)| {
            args.len() == 6
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b"
                && args[3] == "add-schema"
                && args[5] == "main"
        }));
    }
}
