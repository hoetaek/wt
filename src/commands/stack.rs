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

    let mut stack_items = stack_items_from_names(items, None)?;
    let resolved_base = resolve_stack_base(ctx, base)?;
    assign_stack_item_parents(&mut stack_items, &resolved_base);
    let now = current_utc_timestamp();
    let stack = StackMetadata {
        profile: profile.map(str::to_string),
        base_mode: "explicit".into(),
        base: Some(resolved_base),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        items: stack_items,
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
    let mut stack_items = stack_items_from_snapshots(issue_snapshots, None);
    let resolved_base = resolve_stack_base(ctx, base)?;
    assign_stack_item_parents(&mut stack_items, &resolved_base);
    let now = current_utc_timestamp();
    let stack = StackMetadata {
        profile: profile.map(str::to_string),
        base_mode: "explicit".into(),
        base: Some(resolved_base),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        items: stack_items,
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

pub fn show(ctx: &Ctx, stack: Option<&str>) -> Result<()> {
    let stack_path = match stack {
        Some(target) => resolve_stack_path(ctx, target)?,
        None => latest_stack_path(ctx)?,
    };
    let metadata = read_stack_metadata(&stack_path)?;

    ctx.ui
        .print_step(&format!("Stack: {}", stack_path.display()));
    ctx.ui.print_dim(&format!("  Status: {}", metadata.status));
    ctx.ui
        .print_dim(&format!("  Base: {}", describe_stack_base(&metadata)?));
    ctx.ui.print_dim(&format!(
        "  Profile: {}",
        metadata.profile.as_deref().unwrap_or("(effective config)")
    ));
    ctx.ui.print_dim(&format!(
        "  Items: {} ({})",
        metadata.items.len(),
        stack_status_counts(&metadata.items)
    ));

    for (idx, item) in metadata.items.iter().enumerate() {
        let title = item.title();
        let summary = if title.is_empty() {
            format!("  {}. {} [{}]", idx + 1, item.label(), item.status)
        } else {
            format!(
                "  {}. {} [{}] {}",
                idx + 1,
                item.label(),
                item.status,
                title
            )
        };
        ctx.ui.print_dim(&summary);
        ctx.ui.print_dim(&format!("     Kind: {}", item.kind()));
        if !item.branch.trim().is_empty() {
            ctx.ui.print_dim(&format!("     Branch: {}", item.branch));
        }
        if let Some(parent) = item.parent.as_deref() {
            ctx.ui.print_dim(&format!("     Parent: {parent}"));
        }
        if let Some(snapshot) = item.snapshot.as_deref() {
            ctx.ui.print_dim(&format!("     Snapshot: {snapshot}"));
        }
        if !item.error.trim().is_empty() {
            ctx.ui.print_dim(&format!("     Error: {}", item.error));
        }
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

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StackMetadata {
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
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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

fn assign_stack_item_parents(items: &mut [StackItem], initial_parent: &str) {
    let mut parent = Some(initial_parent.to_string());
    for item in items {
        item.parent = parent.clone();
        parent = prepared_branch_name(&item.branch).map(str::to_string);
    }
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

    for previous in stack.items[..idx].iter().rev() {
        match previous.status.as_str() {
            STATUS_DONE => {
                return prepared_branch_name(&previous.branch)
                    .map(str::to_string)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Previous stack item {} has no branch", previous.label())
                    });
            }
            STATUS_SKIPPED => continue,
            _ => bail!("Previous stack item {} is not done", previous.label()),
        }
    }

    resolve_initial_base(ctx, stack)
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
    let mut metadata: StackMetadata = toml::from_str(&content)?;
    for item in &mut metadata.items {
        item.normalize();
    }
    Ok(metadata)
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

fn resolve_stack_base(ctx: &Ctx, base: &Option<String>) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match BaseMode::from_raw(base) {
        BaseMode::Explicit(branch) => branch,
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            branches[idx].clone()
        }
        BaseMode::Current => git.current_branch()?,
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))?
        }
    };

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
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
        "current" => git.current_branch()?,
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

fn describe_stack_base(stack: &StackMetadata) -> Result<String> {
    match stack.base_mode.as_str() {
        "default" => Ok("prompt at run time".into()),
        "interactive" => Ok("branch selector at run time".into()),
        "current" => Ok("current branch at run time".into()),
        "explicit" => stack
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Stack base_mode is explicit but base is missing")),
        other => bail!("Unknown stack base_mode: {other}"),
    }
}

fn stack_status_counts(items: &[StackItem]) -> String {
    let statuses = [
        STATUS_PREPARED,
        STATUS_RUNNING,
        STATUS_DONE,
        STATUS_FAILED,
        STATUS_SKIPPED,
    ];
    let counts = statuses
        .iter()
        .filter_map(|status| {
            let count = items.iter().filter(|item| item.status == *status).count();
            (count > 0).then(|| format!("{status}={count}"))
        })
        .collect::<Vec<_>>();

    if counts.is_empty() {
        "none".into()
    } else {
        counts.join(", ")
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
    use crate::config::{Config, WorktreeNamingConfig};
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
    fn new_resolves_current_base_for_dot_base() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("feature/current", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let items = vec!["Add schema".into()];

        new(&ctx, &items, None, &Some(".".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let content = std::fs::read_to_string(stack_path).unwrap();
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"feature/current\""));
        assert!(content.contains("parent = \"feature/current\""));
    }

    #[test]
    fn new_stores_default_base_prompt_result_at_prepare_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_input("develop");
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );
        let items = vec!["Add schema".into()];

        new(&ctx, &items, None, &None).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base_mode, "explicit");
        assert_eq!(stack.base.as_deref(), Some("develop"));
        assert_eq!(stack.items[0].parent.as_deref(), Some("develop"));
    }

    #[test]
    fn resolve_initial_base_current_uses_current_branch_without_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("feature/current", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let stack = StackMetadata {
            profile: None,
            base_mode: "current".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            items: Vec::new(),
        };

        assert_eq!(
            resolve_initial_base(&ctx, &stack).unwrap(),
            "feature/current"
        );
    }

    #[test]
    fn parent_for_item_skips_skipped_items_when_finding_parent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            items: vec![
                StackItem {
                    kind: "new".into(),
                    id: "schema".into(),
                    source: "Schema".into(),
                    title: "Schema".into(),
                    branch: "schema".into(),
                    parent: Some("main".into()),
                    snapshot: None,
                    body: String::new(),
                    status: STATUS_DONE.into(),
                    error: String::new(),
                },
                StackItem {
                    kind: "new".into(),
                    id: "api".into(),
                    source: "API".into(),
                    title: "API".into(),
                    branch: "api".into(),
                    parent: Some("schema".into()),
                    snapshot: None,
                    body: String::new(),
                    status: STATUS_SKIPPED.into(),
                    error: "User cancelled".into(),
                },
                StackItem {
                    kind: "new".into(),
                    id: "ui".into(),
                    source: "UI".into(),
                    title: "UI".into(),
                    branch: "ui".into(),
                    parent: None,
                    snapshot: None,
                    body: String::new(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
            ],
        };

        assert_eq!(parent_for_item(&ctx, &stack, 2).unwrap(), "schema");
    }

    #[test]
    fn parent_for_item_uses_initial_base_when_previous_items_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            items: vec![
                StackItem {
                    kind: "new".into(),
                    id: "schema".into(),
                    source: "Schema".into(),
                    title: "Schema".into(),
                    branch: "schema".into(),
                    parent: Some("main".into()),
                    snapshot: None,
                    body: String::new(),
                    status: STATUS_SKIPPED.into(),
                    error: "User cancelled".into(),
                },
                StackItem {
                    kind: "new".into(),
                    id: "api".into(),
                    source: "API".into(),
                    title: "API".into(),
                    branch: "api".into(),
                    parent: None,
                    snapshot: None,
                    body: String::new(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
            ],
        };

        assert_eq!(parent_for_item(&ctx, &stack, 1).unwrap(), "main");
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
    fn issue_applies_worktree_naming_to_prepared_parent_chain() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Schema","branchName":"alice/proj-1-schema","description":"Schema body"}"#,
            true,
        );
        runner.add_response(r#"{"english_slug":"schema-layer"}"#, true);
        runner.add_response(
            r#"{"identifier":"PROJ-2","title":"API","branchName":"alice/proj-2-api","description":"API body"}"#,
            true,
        );
        runner.add_response(r#"{"english_slug":"api-layer"}"#, true);
        let config = crate::config::Config {
            issues: Some(crate::config::IssuesConfig {
                provider: crate::config::IssueProviderType::Linear,
                gh_user: None,
            }),
            worktree: crate::config::WorktreeConfig {
                naming: Some(WorktreeNamingConfig {
                    command: "namer".into(),
                    prompt: "{{issue_title}}".into(),
                    branch: Some("{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}".into()),
                    workspace: None,
                }),
                ..Default::default()
            },
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(
            &ctx,
            &["PROJ-1".into(), "PROJ-2".into()],
            None,
            &Some("main".into()),
        )
        .unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.items[0].branch, "alice/proj-1-schema-layer");
        assert_eq!(stack.items[0].parent.as_deref(), Some("main"));
        assert_eq!(stack.items[1].branch, "alice/proj-2-api-layer");
        assert_eq!(
            stack.items[1].parent.as_deref(),
            Some("alice/proj-1-schema-layer")
        );
    }

    #[test]
    fn show_prints_stack_metadata_and_items() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: Some("codex".into()),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            items: vec![StackItem {
                kind: "issue".into(),
                id: "PROJ-1".into(),
                source: "PROJ-1".into(),
                title: "Schema".into(),
                branch: "alice/proj-1-schema".into(),
                parent: Some("main".into()),
                snapshot: Some(".local/issues/PROJ-1.md".into()),
                body: String::new(),
                status: STATUS_FAILED.into(),
                error: "missing snapshot".into(),
            }],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        show(&ctx, Some(stack_path.to_str().unwrap())).unwrap();

        let steps = ui.steps.lock().unwrap();
        assert!(steps[0].contains("Stack:"));
        let details = ui.dims.lock().unwrap().join("\n");
        assert!(details.contains("Status: partial"));
        assert!(details.contains("Base: main"));
        assert!(details.contains("Profile: codex"));
        assert!(details.contains("Items: 1 (failed=1)"));
        assert!(details.contains("PROJ-1 [failed] Schema"));
        assert!(details.contains("Kind: issue"));
        assert!(details.contains("Branch: alice/proj-1-schema"));
        assert!(details.contains("Parent: main"));
        assert!(details.contains("Snapshot: .local/issues/PROJ-1.md"));
        assert!(details.contains("Error: missing snapshot"));
    }

    #[test]
    fn read_stack_metadata_rejects_issues_tables() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("stack.toml");
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
