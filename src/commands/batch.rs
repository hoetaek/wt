use crate::cli::BaseMode;
use crate::commands::issue;
use crate::commands::issue_selection;
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

pub fn issue(
    ctx: &Ctx,
    issues: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;

    let selected_issues = if issues.is_empty() {
        issue_selection::select_issues(ctx, "Select issues for batch")?
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
    let resolved_base = resolve_batch_base(ctx, base)?;
    let now = current_utc_timestamp();
    let batch = BatchMetadata {
        profile: profile.map(str::to_string),
        base_mode: "explicit".into(),
        base: Some(resolved_base),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        items: issue_snapshots
            .into_iter()
            .map(BatchItem::from_snapshot)
            .collect(),
    };
    let batch_path = write_new_batch_metadata(ctx, &batch)?;

    ctx.ui
        .print_step(&format!("Prepared batch: {}", batch_path.display()));
    Ok(())
}

fn resolve_batch_base(ctx: &Ctx, base: &Option<String>) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match BaseMode::from_raw(base) {
        BaseMode::Explicit(branch) => Ok(branch),
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            Ok(branches[idx].clone())
        }
        BaseMode::Current => git.current_branch(),
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))
        }
    }?;

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }

    Ok(base)
}

pub fn run(ctx: &Ctx, batch: &str) -> Result<()> {
    let batch_path = resolve_batch_path(ctx, batch)?;
    let mut metadata = read_batch_metadata(&batch_path)?;
    validate_profile(ctx, metadata.profile.as_deref())?;

    if metadata.items.is_empty() {
        bail!("Batch has no items: {}", batch_path.display());
    }

    let has_runnable_item = metadata
        .items
        .iter()
        .any(|item| is_runnable_status(&item.status));
    let base = if has_runnable_item {
        Some(batch_base_option(&metadata)?)
    } else {
        None
    };
    let mut ran_any = false;

    for idx in 0..metadata.items.len() {
        let current_status = metadata.items[idx].status.as_str();
        if !is_runnable_status(current_status) {
            ctx.ui.print_step(&format!(
                "Skipping {} ({current_status})",
                metadata.items[idx].label()
            ));
            continue;
        }

        let base = base
            .as_ref()
            .expect("batch base is validated before running an item");

        ran_any = true;
        metadata.status = STATUS_RUNNING.into();
        metadata.updated_at = current_utc_timestamp();
        metadata.items[idx].status = STATUS_RUNNING.into();
        metadata.items[idx].error.clear();
        write_batch_metadata(&batch_path, &metadata)?;

        let result = run_batch_item(ctx, &metadata.items[idx], base, metadata.profile.as_deref());

        match result {
            Ok(()) => {
                metadata.items[idx].status = STATUS_DONE.into();
                metadata.items[idx].error.clear();
            }
            Err(err) => {
                if err
                    .downcast_ref::<WtError>()
                    .is_some_and(|err| matches!(err, WtError::Cancelled))
                {
                    metadata.items[idx].status = STATUS_SKIPPED.into();
                    metadata.items[idx].error = "User cancelled".into();
                    metadata.status = summarize_batch_status(&metadata.items);
                    metadata.updated_at = current_utc_timestamp();
                    write_batch_metadata(&batch_path, &metadata)?;
                    return Ok(());
                }

                metadata.items[idx].status = STATUS_FAILED.into();
                metadata.items[idx].error = err.to_string();
            }
        }

        metadata.status = summarize_batch_status(&metadata.items);
        metadata.updated_at = current_utc_timestamp();
        write_batch_metadata(&batch_path, &metadata)?;
    }

    if !ran_any {
        ctx.ui
            .print_step("No prepared or failed items to run in this batch.");
    }

    metadata.status = summarize_batch_status(&metadata.items);
    metadata.updated_at = current_utc_timestamp();
    write_batch_metadata(&batch_path, &metadata)?;
    ctx.ui
        .print_step(&format!("Batch status: {}", metadata.status));

    if metadata.status == STATUS_FAILED {
        bail!("Batch failed: {}", batch_path.display());
    }

    Ok(())
}

pub fn show(ctx: &Ctx, batch: Option<&str>) -> Result<()> {
    let batch_path = match batch {
        Some(target) => resolve_batch_path(ctx, target)?,
        None => latest_batch_path(ctx)?,
    };
    let metadata = read_batch_metadata(&batch_path)?;

    ctx.ui
        .print_step(&format!("Batch: {}", batch_path.display()));
    ctx.ui.print_dim(&format!("  Status: {}", metadata.status));
    ctx.ui
        .print_dim(&format!("  Base: {}", describe_batch_base(&metadata)?));
    ctx.ui.print_dim(&format!(
        "  Profile: {}",
        metadata.profile.as_deref().unwrap_or("(effective config)")
    ));
    ctx.ui.print_dim(&format!(
        "  Items: {} ({})",
        metadata.items.len(),
        batch_status_counts(&metadata.items)
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
        if !item.branch.trim().is_empty() {
            ctx.ui.print_dim(&format!("     Branch: {}", item.branch));
        }
        ctx.ui
            .print_dim(&format!("     Snapshot: {}", item.snapshot));
        if !item.error.trim().is_empty() {
            ctx.ui.print_dim(&format!("     Error: {}", item.error));
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchMetadata {
    #[serde(default)]
    profile: Option<String>,
    base_mode: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(default = "default_batch_status")]
    status: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    items: Vec<BatchItem>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchItem {
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
    snapshot: String,
    #[serde(default = "default_issue_status")]
    status: String,
    #[serde(default)]
    error: String,
}

impl BatchItem {
    fn from_snapshot(snapshot: IssueSnapshot) -> Self {
        Self {
            kind: "issue".into(),
            id: snapshot.id,
            source: snapshot.source,
            title: snapshot.title,
            branch: snapshot.branch,
            snapshot: snapshot.snapshot,
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
        "batch-item".into()
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
            "issue"
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
            self.kind = "issue".into();
        }
    }
}

fn default_batch_status() -> String {
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

fn run_batch_item(
    ctx: &Ctx,
    batch_item: &BatchItem,
    base: &Option<String>,
    profile: Option<&str>,
) -> Result<()> {
    if batch_item.kind() != "issue" {
        bail!(
            "Batch item {} has unsupported kind: {}",
            batch_item.label(),
            batch_item.kind()
        );
    }

    let snapshot_path = batch_item.snapshot.clone();
    let content = fs::read_to_string(ctx.repo_root.join(&snapshot_path))
        .with_context(|| format!("Failed to read issue snapshot: {snapshot_path}"))?;
    let branch_name = prepared_branch_name(batch_item);

    issue::run_with_issue_snapshot(
        ctx,
        base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &batch_item.label(),
            title: &batch_item.title(),
            branch_name,
            mode: "issue",
            prompt_intro: "Use this issue snapshot before changing code.",
            snapshot: issue::IssueSnapshotContext {
                path_label: "Snapshot path",
                path: &snapshot_path,
                content: &content,
            },
        },
    )
    .map(|_| ())
}

fn prepared_branch_name(item: &BatchItem) -> Option<&str> {
    let branch = item.branch.trim();
    if branch.is_empty() || branch == "-" {
        None
    } else {
        Some(branch)
    }
}

fn write_new_batch_metadata(ctx: &Ctx, batch: &BatchMetadata) -> Result<PathBuf> {
    let batches_dir = ctx.repo_root.join(".local/batches");
    fs::create_dir_all(&batches_dir)?;

    let date = current_utc_date();
    let mut seq = 1;
    let path = loop {
        let candidate = batches_dir.join(format!("{date}-{seq:03}.toml"));
        if !candidate.exists() {
            break candidate;
        }
        seq += 1;
    };

    write_batch_metadata(&path, batch)?;
    Ok(path)
}

fn read_batch_metadata(path: &Path) -> Result<BatchMetadata> {
    let content = fs::read_to_string(path)?;
    let mut metadata: BatchMetadata = toml::from_str(&content)?;
    for item in &mut metadata.items {
        item.normalize();
    }
    Ok(metadata)
}

fn write_batch_metadata(path: &Path, batch: &BatchMetadata) -> Result<()> {
    let mut content = String::new();
    if let Some(profile) = batch.profile.as_deref() {
        content.push_str(&format!("profile = {}\n", toml_quote(profile)));
    }
    content.push_str(&format!("base_mode = {}\n", toml_quote(&batch.base_mode)));
    if let Some(base) = &batch.base {
        content.push_str(&format!("base = {}\n", toml_quote(base)));
    }
    content.push_str(&format!("status = {}\n", toml_quote(&batch.status)));
    content.push_str(&format!("created_at = {}\n", toml_quote(&batch.created_at)));
    content.push_str(&format!("updated_at = {}\n", toml_quote(&batch.updated_at)));

    for item in &batch.items {
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
        content.push_str(&format!("snapshot = {}\n", toml_quote(&item.snapshot)));
        content.push_str(&format!("status = {}\n", toml_quote(&item.status)));
        content.push_str(&format!("error = {}\n", toml_quote(&item.error)));
    }

    fs::write(path, content)?;
    Ok(())
}

fn resolve_batch_path(ctx: &Ctx, target: &str) -> Result<PathBuf> {
    if target == "latest" {
        return latest_batch_path(ctx);
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
            .join(".local/batches")
            .join(format!("{target}.toml"));
        if shorthand.exists() {
            return Ok(shorthand);
        }
    }

    bail!("Batch not found: {target}");
}

fn latest_batch_path(ctx: &Ctx) -> Result<PathBuf> {
    let batches_dir = ctx.repo_root.join(".local/batches");
    let mut paths = Vec::new();
    if batches_dir.exists() {
        for entry in fs::read_dir(&batches_dir)? {
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
        .ok_or_else(|| anyhow::anyhow!("No batch files found in .local/batches"))
}

fn describe_batch_base(batch: &BatchMetadata) -> Result<String> {
    match batch.base_mode.as_str() {
        "explicit" => batch
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Batch base_mode is explicit but base is missing")),
        "default" | "interactive" | "current" => {
            bail!("Batch base_mode must be explicit; recreate the batch with wt batch issue")
        }
        other => bail!("Unknown batch base_mode: {other}"),
    }
}

fn batch_status_counts(items: &[BatchItem]) -> String {
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

fn batch_base_option(batch: &BatchMetadata) -> Result<Option<String>> {
    match batch.base_mode.as_str() {
        "explicit" => batch
            .base
            .clone()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("Batch base_mode is explicit but base is missing")),
        "default" | "interactive" | "current" => bail!(
            "Batch base_mode must be explicit before run; recreate the batch with wt batch issue"
        ),
        other => bail!("Unknown batch base_mode: {other}"),
    }
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, STATUS_PREPARED | STATUS_FAILED)
}

fn summarize_batch_status(items: &[BatchItem]) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::issue_snapshot::safe_file_stem;
    use crate::config::{Config, IssueProviderType, IssuesConfig, WorktreeNamingConfig};
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};

    #[test]
    fn civil_date_matches_unix_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn safe_file_stem_replaces_unsafe_chars() {
        assert_eq!(safe_file_stem("#42"), "42");
        assert_eq!(safe_file_stem("PROJ-123"), "PROJ-123");
        assert_eq!(safe_file_stem("bad/value"), "bad-value");
    }

    #[test]
    fn snapshot_issues_writes_markdown_body_outside_batch_toml() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let snapshots = snapshot_issues(&ctx, &["PROJ-123".into()]).unwrap();

        assert_eq!(snapshots[0].snapshot, ".local/issues/PROJ-123.md");
        assert_eq!(snapshots[0].title, "Fix editor");
        assert_eq!(snapshots[0].branch, "alice/proj-123-fix-editor");
        let markdown =
            std::fs::read_to_string(dir.path().join(".local/issues/PROJ-123.md")).unwrap();
        assert!(markdown.contains("# PROJ-123: Fix editor"));
        assert!(markdown.contains("## Body"));
        assert!(markdown.contains("Long issue body"));
    }

    #[test]
    fn issue_omits_profile_when_default_behavior_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("main", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(!content.contains("profile ="));
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"main\""));
        assert!(content.contains("status = \"prepared\""));
        assert!(content.contains("[[items]]"));
        assert!(content.contains("kind = \"issue\""));
        assert!(!content.contains("[[issues]]"));
        assert!(content.contains("id = \"PROJ-123\""));
        assert!(content.contains("title = \"Fix editor\""));
        assert!(content.contains("branch = \"alice/proj-123-fix-editor\""));
        assert!(content.contains("snapshot = \".local/issues/PROJ-123.md\""));
    }

    #[test]
    fn issue_applies_worktree_naming_to_prepared_branch() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response(r#"{"english_slug":"repair-editor"}"#, true);
        runner.add_response("main", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
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

        issue(&ctx, &["PROJ-123".into()], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(&batch_path).unwrap();
        assert!(content.contains("branch = \"alice/proj-123-repair-editor\""));
        let markdown =
            std::fs::read_to_string(dir.path().join(".local/issues/PROJ-123.md")).unwrap();
        assert!(markdown.contains("- Branch: `alice/proj-123-repair-editor`"));
    }

    #[test]
    fn issue_resolves_current_base_for_dot_base() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("feature", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], None, &Some(".".into())).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"feature\""));
    }

    #[test]
    fn batch_base_option_rejects_non_explicit_base() {
        let batch = BatchMetadata {
            profile: None,
            base_mode: "current".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            items: Vec::new(),
        };

        let result = batch_base_option(&batch);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("base_mode must be explicit")
        );
    }

    #[test]
    fn issue_stores_default_base_prompt_result_at_prepare_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_input("develop");
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
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

        issue(&ctx, &["PROJ-123".into()], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"develop\""));
    }

    #[test]
    fn issue_records_explicit_named_profile() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("main", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], Some("codex"), &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("profile = \"codex\""));
    }

    #[test]
    fn show_prints_batch_metadata_and_items() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: Some("codex".into()),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            items: vec![BatchItem {
                kind: "issue".into(),
                id: "PROJ-123".into(),
                source: "123".into(),
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                snapshot: ".local/issues/PROJ-123.md".into(),
                status: STATUS_FAILED.into(),
                error: "missing snapshot".into(),
            }],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        show(&ctx, Some(batch_path.to_str().unwrap())).unwrap();

        let steps = ui.steps.lock().unwrap();
        assert!(steps[0].contains("Batch:"));
        let details = ui.dims.lock().unwrap().join("\n");
        assert!(details.contains("Status: partial"));
        assert!(details.contains("Base: main"));
        assert!(details.contains("Profile: codex"));
        assert!(details.contains("Items: 1 (failed=1)"));
        assert!(details.contains("PROJ-123 [failed] Fix editor"));
        assert!(details.contains("Branch: alice/proj-123-fix-editor"));
        assert!(details.contains("Error: missing snapshot"));
    }

    #[test]
    fn show_rejects_non_explicit_base_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "interactive".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            items: Vec::new(),
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = show(&ctx, Some(batch_path.to_str().unwrap()));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("base_mode must be explicit")
        );
    }

    #[test]
    fn issue_with_no_args_selects_issues_from_provider_list() {
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
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
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

        issue(&ctx, &[], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("id = \"PROJ-2\""));
        assert!(!content.contains("id = \"PROJ-1\""));
    }

    #[test]
    fn batch_metadata_round_trips_status_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: Some("codex-yolo".into()),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:01:00Z".into(),
            items: vec![BatchItem {
                kind: "issue".into(),
                id: "PROJ-123".into(),
                source: "123".into(),
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                snapshot: ".local/issues/PROJ-123.md".into(),
                status: STATUS_DONE.into(),
                error: String::new(),
            }],
        };

        write_batch_metadata(&path, &batch).unwrap();
        let parsed = read_batch_metadata(&path).unwrap();

        assert_eq!(parsed.profile.as_deref(), Some("codex-yolo"));
        assert_eq!(parsed.base.as_deref(), Some("main"));
        assert_eq!(parsed.status, STATUS_PARTIAL);
        assert_eq!(parsed.items[0].kind(), "issue");
        assert_eq!(parsed.items[0].status, STATUS_DONE);

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("[[items]]"));
        assert!(content.contains("kind = \"issue\""));
        assert!(!content.contains("[[issues]]"));
    }

    #[test]
    fn read_batch_metadata_rejects_issues_tables() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.toml");
        std::fs::write(
            &batch_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[issues]]
id = "PROJ-1"
source = "1"
title = "Fix editor"
branch = "alice/proj-1-fix-editor"
snapshot = ".local/issues/PROJ-1.md"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_batch_metadata(&batch_path).is_err());
    }

    #[test]
    fn latest_batch_path_uses_lexically_newest_batch_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batches_dir = dir.path().join(".local/batches");
        std::fs::create_dir_all(&batches_dir).unwrap();
        std::fs::write(batches_dir.join("2026-05-11-001.toml"), "").unwrap();
        std::fs::write(batches_dir.join("2026-05-11-002.toml"), "").unwrap();

        let latest = latest_batch_path(&ctx).unwrap();

        assert!(latest.ends_with("2026-05-11-002.toml"));
    }

    #[test]
    fn summarize_status_distinguishes_batch_and_item_state() {
        let item = |status: &str| BatchItem {
            kind: "issue".into(),
            id: "PROJ-123".into(),
            source: "123".into(),
            title: String::new(),
            branch: String::new(),
            snapshot: ".local/issues/PROJ-123.md".into(),
            status: status.into(),
            error: String::new(),
        };

        assert_eq!(
            summarize_batch_status(&[item(STATUS_PREPARED)]),
            STATUS_PREPARED
        );
        assert_eq!(
            summarize_batch_status(&[item(STATUS_DONE), item(STATUS_PREPARED)]),
            STATUS_PARTIAL
        );
        assert_eq!(
            summarize_batch_status(&[item(STATUS_DONE), item(STATUS_FAILED)]),
            STATUS_FAILED
        );
        assert_eq!(
            summarize_batch_status(&[item(STATUS_DONE), item(STATUS_SKIPPED)]),
            STATUS_DONE
        );
    }

    #[test]
    fn run_skips_done_items_without_touching_issue_provider() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "default".into(),
            base: None,
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            items: vec![BatchItem {
                kind: "issue".into(),
                id: "PROJ-123".into(),
                source: "123".into(),
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                snapshot: ".local/issues/PROJ-123.md".into(),
                status: STATUS_DONE.into(),
                error: String::new(),
            }],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        run(&ctx, batch_path.to_str().unwrap()).unwrap();

        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_DONE);
        assert_eq!(updated.items[0].status, STATUS_DONE);
    }

    #[test]
    fn run_marks_item_failed_and_errors_when_snapshot_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            items: vec![BatchItem {
                kind: "issue".into(),
                id: "PROJ-123".into(),
                source: "123".into(),
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                snapshot: ".local/issues/PROJ-123.md".into(),
                status: STATUS_PREPARED.into(),
                error: String::new(),
            }],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap());

        assert!(result.is_err());
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.base_mode, "explicit");
        assert_eq!(updated.base.as_deref(), Some("main"));
        assert_eq!(updated.status, STATUS_FAILED);
        assert_eq!(updated.items[0].status, STATUS_FAILED);
        assert!(
            updated.items[0]
                .error
                .contains("Failed to read issue snapshot")
        );
    }

    #[test]
    fn run_rejects_non_explicit_base_before_touching_items() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "interactive".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            items: vec![BatchItem {
                kind: "issue".into(),
                id: "PROJ-123".into(),
                source: "123".into(),
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                snapshot: ".local/issues/PROJ-123.md".into(),
                status: STATUS_PREPARED.into(),
                error: String::new(),
            }],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap());

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("base_mode must be explicit")
        );
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.base_mode, "interactive");
        assert_eq!(updated.status, STATUS_PREPARED);
        assert_eq!(updated.items[0].status, STATUS_PREPARED);
        assert!(updated.items[0].error.is_empty());
    }

    #[test]
    fn run_uses_snapshot_metadata_without_issue_provider_when_branch_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        let issues_dir = dir.path().join(".local/issues");
        std::fs::create_dir_all(&issues_dir).unwrap();
        std::fs::write(
            issues_dir.join("PROJ-123.md"),
            "# PROJ-123: Fix editor\n\nBody",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        ); // checked_out_path
        runner.add_response("", true); // fetch
        runner.add_response("", false); // branch does not exist locally
        runner.add_response("", false); // branch does not exist remotely
        runner.add_response("", true); // worktree add
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // branch parent config
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            items: vec![BatchItem {
                kind: "issue".into(),
                id: "PROJ-123".into(),
                source: "123".into(),
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                snapshot: ".local/issues/PROJ-123.md".into(),
                status: STATUS_PREPARED.into(),
                error: String::new(),
            }],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        run(&ctx, batch_path.to_str().unwrap()).unwrap();

        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.base_mode, "explicit");
        assert_eq!(updated.base.as_deref(), Some("main"));
        assert_eq!(updated.status, STATUS_DONE);
        assert_eq!(updated.items[0].status, STATUS_DONE);
    }
}
