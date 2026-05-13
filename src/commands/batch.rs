use crate::cli::BaseMode;
use crate::commands::issue;
use crate::commands::issue_selection;
use crate::commands::issue_snapshot::{IssueSnapshot, snapshot_issues};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::error::WtError;
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
    let now = current_utc_timestamp();
    let batch = BatchMetadata {
        profile: profile.map(str::to_string),
        base_mode: base_mode_name(base).into(),
        base: explicit_base(base),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        issues: issue_snapshots
            .into_iter()
            .map(BatchIssue::from_snapshot)
            .collect(),
    };
    let batch_path = write_new_batch_metadata(ctx, &batch)?;

    ctx.ui
        .print_step(&format!("Prepared batch: {}", batch_path.display()));
    Ok(())
}

pub fn run(ctx: &Ctx, batch: &str) -> Result<()> {
    let batch_path = resolve_batch_path(ctx, batch)?;
    let mut metadata = read_batch_metadata(&batch_path)?;
    validate_profile(ctx, metadata.profile.as_deref())?;

    if metadata.issues.is_empty() {
        bail!("Batch has no issues: {}", batch_path.display());
    }

    let base = batch_base_option(&metadata)?;
    let mut ran_any = false;

    for idx in 0..metadata.issues.len() {
        let current_status = metadata.issues[idx].status.as_str();
        if !is_runnable_status(current_status) {
            ctx.ui.print_step(&format!(
                "Skipping {} ({current_status})",
                metadata.issues[idx].id
            ));
            continue;
        }

        ran_any = true;
        metadata.status = STATUS_RUNNING.into();
        metadata.updated_at = current_utc_timestamp();
        metadata.issues[idx].status = STATUS_RUNNING.into();
        metadata.issues[idx].error.clear();
        write_batch_metadata(&batch_path, &metadata)?;

        let result = run_batch_issue(
            ctx,
            &metadata.issues[idx],
            &base,
            metadata.profile.as_deref(),
        );

        match result {
            Ok(()) => {
                metadata.issues[idx].status = STATUS_DONE.into();
                metadata.issues[idx].error.clear();
            }
            Err(err) => {
                if err
                    .downcast_ref::<WtError>()
                    .is_some_and(|err| matches!(err, WtError::Cancelled))
                {
                    metadata.issues[idx].status = STATUS_SKIPPED.into();
                    metadata.issues[idx].error = "User cancelled".into();
                    metadata.status = summarize_batch_status(&metadata.issues);
                    metadata.updated_at = current_utc_timestamp();
                    write_batch_metadata(&batch_path, &metadata)?;
                    return Ok(());
                }

                metadata.issues[idx].status = STATUS_FAILED.into();
                metadata.issues[idx].error = err.to_string();
            }
        }

        metadata.status = summarize_batch_status(&metadata.issues);
        metadata.updated_at = current_utc_timestamp();
        write_batch_metadata(&batch_path, &metadata)?;
    }

    if !ran_any {
        ctx.ui
            .print_step("No prepared or failed issues to run in this batch.");
    }

    metadata.status = summarize_batch_status(&metadata.issues);
    metadata.updated_at = current_utc_timestamp();
    write_batch_metadata(&batch_path, &metadata)?;
    ctx.ui
        .print_step(&format!("Batch status: {}", metadata.status));

    if metadata.status == STATUS_FAILED {
        bail!("Batch failed: {}", batch_path.display());
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
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
    issues: Vec<BatchIssue>,
}

#[derive(Clone, Debug, Deserialize)]
struct BatchIssue {
    id: String,
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

impl BatchIssue {
    fn from_snapshot(snapshot: IssueSnapshot) -> Self {
        Self {
            id: snapshot.id,
            source: snapshot.source,
            title: snapshot.title,
            branch: snapshot.branch,
            snapshot: snapshot.snapshot,
            status: STATUS_PREPARED.into(),
            error: String::new(),
        }
    }
}

fn default_batch_status() -> String {
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

fn run_batch_issue(
    ctx: &Ctx,
    batch_issue: &BatchIssue,
    base: &Option<String>,
    profile: Option<&str>,
) -> Result<()> {
    let snapshot_path = batch_issue.snapshot.clone();
    let content = fs::read_to_string(ctx.repo_root.join(&snapshot_path))
        .with_context(|| format!("Failed to read issue snapshot: {snapshot_path}"))?;
    let branch_name = prepared_branch_name(batch_issue);

    issue::run_with_issue_snapshot(
        ctx,
        base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &batch_issue.id,
            title: &batch_issue.title,
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

fn prepared_branch_name(issue: &BatchIssue) -> Option<&str> {
    let branch = issue.branch.trim();
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
    Ok(toml::from_str(&content)?)
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

    for issue in &batch.issues {
        content.push_str("\n[[issues]]\n");
        content.push_str(&format!("id = {}\n", toml_quote(&issue.id)));
        content.push_str(&format!("source = {}\n", toml_quote(&issue.source)));
        content.push_str(&format!("title = {}\n", toml_quote(&issue.title)));
        content.push_str(&format!("branch = {}\n", toml_quote(&issue.branch)));
        content.push_str(&format!("snapshot = {}\n", toml_quote(&issue.snapshot)));
        content.push_str(&format!("status = {}\n", toml_quote(&issue.status)));
        content.push_str(&format!("error = {}\n", toml_quote(&issue.error)));
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

fn batch_base_option(batch: &BatchMetadata) -> Result<Option<String>> {
    match batch.base_mode.as_str() {
        "default" => Ok(None),
        "interactive" => Ok(Some(String::new())),
        "explicit" => batch
            .base
            .clone()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("Batch base_mode is explicit but base is missing")),
        other => bail!("Unknown batch base_mode: {other}"),
    }
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, STATUS_PREPARED | STATUS_FAILED)
}

fn summarize_batch_status(issues: &[BatchIssue]) -> String {
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
    use crate::commands::issue_snapshot::safe_file_stem;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
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
        assert!(content.contains("status = \"prepared\""));
        assert!(content.contains("[[issues]]"));
        assert!(content.contains("id = \"PROJ-123\""));
        assert!(content.contains("title = \"Fix editor\""));
        assert!(content.contains("branch = \"alice/proj-123-fix-editor\""));
        assert!(content.contains("snapshot = \".local/issues/PROJ-123.md\""));
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
            issues: vec![BatchIssue {
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
        assert_eq!(parsed.issues[0].status, STATUS_DONE);
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
        let issue = |status: &str| BatchIssue {
            id: "PROJ-123".into(),
            source: "123".into(),
            title: String::new(),
            branch: String::new(),
            snapshot: ".local/issues/PROJ-123.md".into(),
            status: status.into(),
            error: String::new(),
        };

        assert_eq!(
            summarize_batch_status(&[issue(STATUS_PREPARED)]),
            STATUS_PREPARED
        );
        assert_eq!(
            summarize_batch_status(&[issue(STATUS_DONE), issue(STATUS_PREPARED)]),
            STATUS_PARTIAL
        );
        assert_eq!(
            summarize_batch_status(&[issue(STATUS_DONE), issue(STATUS_FAILED)]),
            STATUS_FAILED
        );
        assert_eq!(
            summarize_batch_status(&[issue(STATUS_DONE), issue(STATUS_SKIPPED)]),
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
            issues: vec![BatchIssue {
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
        assert_eq!(updated.issues[0].status, STATUS_DONE);
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
            base_mode: "default".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            issues: vec![BatchIssue {
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
        assert_eq!(updated.status, STATUS_FAILED);
        assert_eq!(updated.issues[0].status, STATUS_FAILED);
        assert!(
            updated.issues[0]
                .error
                .contains("Failed to read issue snapshot")
        );
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
        runner.add_response("main", true); // current branch for default base
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
            base_mode: "default".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            issues: vec![BatchIssue {
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
        assert_eq!(updated.status, STATUS_DONE);
        assert_eq!(updated.issues[0].status, STATUS_DONE);
    }
}
