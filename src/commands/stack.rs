use crate::cli::BaseMode;
use crate::commands::issue;
use crate::commands::issue_selection::{self, SelectedIssue};
use crate::commands::issue_snapshot::{IssueSnapshot, snapshot_issues};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::git::GitService;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STATUS_PREPARED: &str = "prepared";
const STATUS_RUNNING: &str = "running";
const STATUS_DONE: &str = "done";
const STATUS_FAILED: &str = "failed";
const STATUS_SKIPPED: &str = "skipped";
const STATUS_PARTIAL: &str = "partial";

pub fn prepare(
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
        issues: stack_issues_from_snapshots(issue_snapshots, explicit_base(base)),
    };
    let stack_path = write_new_stack_metadata(ctx, &stack)?;

    ctx.ui
        .print_step(&format!("Prepared stack: {}", stack_path.display()));
    Ok(())
}

pub fn run(ctx: &Ctx, stack: &str) -> Result<()> {
    let stack_path = resolve_stack_path(ctx, stack)?;
    let mut metadata = read_stack_metadata(&stack_path)?;
    validate_profile(ctx, metadata.profile.as_deref())?;

    if metadata.issues.is_empty() {
        bail!("Stack has no issues: {}", stack_path.display());
    }

    if let Some(issue) = metadata
        .issues
        .iter()
        .find(|issue| issue.status == STATUS_RUNNING)
    {
        bail!(
            "Stack item {} is already running. Mark it complete with: wt stack complete {} {}",
            issue.id,
            stack_path.display(),
            issue.id
        );
    }

    let Some(idx) = next_runnable_issue(&metadata.issues) else {
        ctx.ui
            .print_step("No prepared or failed issues to run in this stack.");
        metadata.status = summarize_stack_status(&metadata.issues);
        metadata.updated_at = current_utc_timestamp();
        write_stack_metadata(&stack_path, &metadata)?;
        return Ok(());
    };

    let parent = parent_for_issue(ctx, &metadata, idx)?;
    metadata.status = STATUS_RUNNING.into();
    metadata.updated_at = current_utc_timestamp();
    metadata.issues[idx].status = STATUS_RUNNING.into();
    metadata.issues[idx].parent = Some(parent.clone());
    metadata.issues[idx].error.clear();
    write_stack_metadata(&stack_path, &metadata)?;

    let result = run_stack_issue(
        ctx,
        &stack_path,
        &metadata.issues[idx],
        &parent,
        metadata.profile.as_deref(),
    );

    match result {
        Ok(result) => {
            metadata.issues[idx].branch = result.branch_name;
            metadata.issues[idx].status = STATUS_RUNNING.into();
            metadata.issues[idx].error.clear();
            ctx.ui.print_step(&format!(
                "Started stack item {}. Mark it complete with: wt stack complete {} {}",
                metadata.issues[idx].id,
                stack_path.display(),
                metadata.issues[idx].id
            ));
        }
        Err(err) => {
            if err
                .downcast_ref::<WtError>()
                .is_some_and(|err| matches!(err, WtError::Cancelled))
            {
                metadata.issues[idx].status = STATUS_SKIPPED.into();
                metadata.issues[idx].error = "User cancelled".into();
                metadata.status = summarize_stack_status(&metadata.issues);
                metadata.updated_at = current_utc_timestamp();
                write_stack_metadata(&stack_path, &metadata)?;
                return Ok(());
            }

            metadata.issues[idx].status = STATUS_FAILED.into();
            metadata.issues[idx].error = err.to_string();
        }
    }

    metadata.status = summarize_stack_status(&metadata.issues);
    metadata.updated_at = current_utc_timestamp();
    write_stack_metadata(&stack_path, &metadata)?;
    ctx.ui
        .print_step(&format!("Stack status: {}", metadata.status));

    if metadata.status == STATUS_FAILED {
        bail!("Stack failed: {}", stack_path.display());
    }

    Ok(())
}

pub fn complete(ctx: &Ctx, stack: &str, issue: Option<&str>) -> Result<()> {
    let stack_path = resolve_stack_path(ctx, stack)?;
    let mut metadata = read_stack_metadata(&stack_path)?;

    let Some(idx) = metadata
        .issues
        .iter()
        .position(|issue| issue.status == STATUS_RUNNING)
    else {
        ctx.ui.print_warning("No running stack item found");
        return Ok(());
    };

    if let Some(issue) = issue {
        let running = &metadata.issues[idx];
        if !stack_issue_matches(running, issue) {
            bail!(
                "Running stack item is {}, but complete was requested for {issue}",
                running.id
            );
        }
    }

    metadata.issues[idx].status = STATUS_DONE.into();
    metadata.issues[idx].error.clear();
    metadata.status = summarize_stack_status(&metadata.issues);
    metadata.updated_at = current_utc_timestamp();
    write_stack_metadata(&stack_path, &metadata)?;

    ctx.ui
        .print_step(&format!("Marked {} done", metadata.issues[idx].id));
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
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
    issues: Vec<StackIssue>,
}

#[derive(Clone, Debug, Deserialize)]
struct StackIssue {
    id: String,
    source: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    parent: Option<String>,
    snapshot: String,
    #[serde(default = "default_issue_status")]
    status: String,
    #[serde(default)]
    error: String,
}

impl StackIssue {
    fn from_snapshot(snapshot: IssueSnapshot, parent: Option<String>) -> Self {
        Self {
            id: snapshot.id,
            source: snapshot.source,
            title: snapshot.title,
            branch: snapshot.branch,
            parent,
            snapshot: snapshot.snapshot,
            status: STATUS_PREPARED.into(),
            error: String::new(),
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

fn stack_issues_from_snapshots(
    snapshots: Vec<IssueSnapshot>,
    initial_parent: Option<String>,
) -> Vec<StackIssue> {
    let mut parent = initial_parent;
    snapshots
        .into_iter()
        .map(|snapshot| {
            let issue_parent = parent.clone();
            parent = prepared_branch_name(&snapshot.branch).map(str::to_string);
            StackIssue::from_snapshot(snapshot, issue_parent)
        })
        .collect()
}

fn default_stack_status() -> String {
    STATUS_PREPARED.into()
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

fn run_stack_issue(
    ctx: &Ctx,
    stack_path: &Path,
    stack_issue: &StackIssue,
    parent: &str,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let snapshot_path = stack_issue.snapshot.clone();
    let content = fs::read_to_string(ctx.repo_root.join(&snapshot_path))
        .with_context(|| format!("Failed to read issue snapshot: {snapshot_path}"))?;
    let content = format!(
        "{}\n\n## Stack Completion\n\nWhen this issue is complete, run:\n\n```bash\nwt stack complete {} {}\n```",
        content.trim_end(),
        stack_path.display(),
        stack_issue.id
    );
    let branch_name = prepared_branch_name(&stack_issue.branch);
    let base = Some(parent.to_string());

    issue::run_with_issue_snapshot(
        ctx,
        &base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &stack_issue.id,
            title: &stack_issue.title,
            branch_name,
            snapshot: issue::IssueSnapshotContext {
                path: &snapshot_path,
                content: &content,
            },
        },
    )
}

fn stack_issue_matches(issue: &StackIssue, target: &str) -> bool {
    issue.id == target
        || issue.source == target
        || prepared_branch_name(&issue.branch) == Some(target)
        || issue.branch.rsplit('/').next() == Some(target)
}

fn next_runnable_issue(issues: &[StackIssue]) -> Option<usize> {
    for (idx, issue) in issues.iter().enumerate() {
        match issue.status.as_str() {
            STATUS_DONE | STATUS_SKIPPED => continue,
            status if is_runnable_status(status) => return Some(idx),
            _ => return None,
        }
    }
    None
}

fn parent_for_issue(ctx: &Ctx, stack: &StackMetadata, idx: usize) -> Result<String> {
    if idx == 0 {
        return resolve_initial_base(ctx, stack);
    }

    let previous = &stack.issues[idx - 1];
    if previous.status != STATUS_DONE && previous.status != STATUS_SKIPPED {
        bail!("Previous stack item {} is not done", previous.id);
    }

    prepared_branch_name(&previous.branch)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Previous stack item {} has no branch", previous.id))
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
    Ok(toml::from_str(&content)?)
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

    for issue in &stack.issues {
        content.push_str("\n[[issues]]\n");
        content.push_str(&format!("id = {}\n", toml_quote(&issue.id)));
        content.push_str(&format!("source = {}\n", toml_quote(&issue.source)));
        content.push_str(&format!("title = {}\n", toml_quote(&issue.title)));
        content.push_str(&format!("branch = {}\n", toml_quote(&issue.branch)));
        if let Some(parent) = issue.parent.as_deref() {
            content.push_str(&format!("parent = {}\n", toml_quote(parent)));
        }
        content.push_str(&format!("snapshot = {}\n", toml_quote(&issue.snapshot)));
        content.push_str(&format!("status = {}\n", toml_quote(&issue.status)));
        content.push_str(&format!("error = {}\n", toml_quote(&issue.error)));
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

fn summarize_stack_status(issues: &[StackIssue]) -> String {
    if issues.is_empty() {
        return STATUS_DONE.into();
    }
    if issues.iter().any(|issue| issue.status == STATUS_FAILED) {
        return STATUS_FAILED.into();
    }
    if issues.iter().any(|issue| issue.status == STATUS_RUNNING) {
        return STATUS_RUNNING.into();
    }
    if issues
        .iter()
        .all(|issue| matches!(issue.status.as_str(), STATUS_DONE | STATUS_SKIPPED))
    {
        return STATUS_DONE.into();
    }
    if issues.iter().all(|issue| issue.status == STATUS_PREPARED) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx};
    use std::path::Path;
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
    fn prepare_with_no_args_selects_and_reorders_issues() {
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

        prepare(&ctx, &[], None, &Some("main".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base.as_deref(), Some("main"));
        assert_eq!(stack.issues[0].id, "PROJ-2");
        assert_eq!(stack.issues[0].parent.as_deref(), Some("main"));
        assert_eq!(stack.issues[1].id, "PROJ-1");
        assert_eq!(stack.issues[1].parent.as_deref(), Some("alice/proj-2-api"));
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
            issues: vec![
                StackIssue {
                    id: "PROJ-1".into(),
                    source: "PROJ-1".into(),
                    title: "Schema".into(),
                    branch: "alice/proj-1-schema".into(),
                    parent: None,
                    snapshot: ".local/issues/PROJ-1.md".into(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
                StackIssue {
                    id: "PROJ-2".into(),
                    source: "PROJ-2".into(),
                    title: "API".into(),
                    branch: "alice/proj-2-api".into(),
                    parent: None,
                    snapshot: ".local/issues/PROJ-2.md".into(),
                    status: STATUS_PREPARED.into(),
                    error: String::new(),
                },
            ],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        run(&ctx, stack_path.to_str().unwrap()).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.issues[0].parent.as_deref(), Some("main"));
        assert_eq!(updated.issues[0].status, STATUS_RUNNING);
        assert_eq!(updated.issues[1].status, STATUS_PREPARED);

        complete(&ctx, stack_path.to_str().unwrap(), Some("PROJ-1")).unwrap();
        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_PARTIAL);
        assert_eq!(updated.issues[0].status, STATUS_DONE);

        run(&ctx, stack_path.to_str().unwrap()).unwrap();
        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(
            updated.issues[1].parent.as_deref(),
            Some("alice/proj-1-schema")
        );
        assert_eq!(updated.issues[1].status, STATUS_RUNNING);

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
}
