use crate::cli::BaseMode;
use crate::commands::issue;
use crate::config::Config;
use crate::context::Ctx;
use anyhow::{Result, bail};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(ctx: &Ctx, issues: &[String], profile: &str, base: &Option<String>) -> Result<()> {
    if issues.is_empty() {
        bail!("Usage: wt batch <issue>... --profile <name>");
    }

    if Config::load_profile(&ctx.repo_root, profile, &ctx.config)?.is_none() {
        bail!("Profile '{profile}' not found");
    }

    let issue_snapshots = snapshot_issues(ctx, issues)?;
    let batch_path = write_batch_metadata(ctx, profile, base, &issue_snapshots)?;

    ctx.ui
        .print_step(&format!("Batch metadata: {}", batch_path.display()));

    for (issue_target, snapshot) in issues.iter().zip(issue_snapshots.iter()) {
        let content = fs::read_to_string(ctx.repo_root.join(&snapshot.snapshot))?;
        issue::run_with_issue_snapshot(
            ctx,
            Some(issue_target.as_str()),
            base,
            Some(profile),
            false,
            issue::IssueSnapshotContext {
                path: &snapshot.snapshot,
                content: &content,
            },
        )?;
    }

    Ok(())
}

#[derive(Clone)]
struct IssueSnapshot {
    id: String,
    source: String,
    snapshot: String,
}

fn snapshot_issues(ctx: &Ctx, issues: &[String]) -> Result<Vec<IssueSnapshot>> {
    let provider = issue::build_provider(ctx)?;
    let issues_dir = ctx.repo_root.join(".local/issues");
    fs::create_dir_all(&issues_dir)?;

    let mut snapshots = Vec::new();
    for source in issues {
        let issue = provider.get_issue(source.trim_start_matches('#'))?;
        let file_name = format!("{}.md", safe_file_stem(&issue.identifier));
        let relative_path = format!(".local/issues/{file_name}");
        let snapshot_path = ctx.repo_root.join(&relative_path);
        let branch = issue.branch_name.as_deref().unwrap_or("-");
        let body = issue.body.as_deref().unwrap_or("").trim();
        let body_section = if body.is_empty() {
            String::new()
        } else {
            format!("\n## Body\n\n{body}\n")
        };
        fs::write(
            &snapshot_path,
            format!(
                "# {}: {}\n\n- Source: `{}`\n- Branch: `{}`\n{}",
                issue.identifier, issue.title, source, branch, body_section
            ),
        )?;
        snapshots.push(IssueSnapshot {
            id: issue.identifier,
            source: source.clone(),
            snapshot: relative_path,
        });
    }

    Ok(snapshots)
}

fn write_batch_metadata(
    ctx: &Ctx,
    profile: &str,
    base: &Option<String>,
    snapshots: &[IssueSnapshot],
) -> Result<std::path::PathBuf> {
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

    let mut content = String::new();
    content.push_str(&format!("profile = {}\n", toml_quote(profile)));
    match BaseMode::from_raw(base) {
        BaseMode::Default => content.push_str("base_mode = \"default\"\n"),
        BaseMode::Interactive => content.push_str("base_mode = \"interactive\"\n"),
        BaseMode::Explicit(branch) => {
            content.push_str("base_mode = \"explicit\"\n");
            content.push_str(&format!("base = {}\n", toml_quote(&branch)));
        }
    }
    content.push_str("\n[[issues]]\n");
    for (idx, issue) in snapshots.iter().enumerate() {
        if idx > 0 {
            content.push_str("\n[[issues]]\n");
        }
        content.push_str(&format!("id = {}\n", toml_quote(&issue.id)));
        content.push_str(&format!("source = {}\n", toml_quote(&issue.source)));
        content.push_str(&format!("snapshot = {}\n", toml_quote(&issue.snapshot)));
    }

    fs::write(&path, content)?;
    Ok(path)
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

fn safe_file_stem(value: &str) -> String {
    let stem = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if stem.is_empty() {
        "issue".into()
    } else {
        stem
    }
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
        let markdown =
            std::fs::read_to_string(dir.path().join(".local/issues/PROJ-123.md")).unwrap();
        assert!(markdown.contains("# PROJ-123: Fix editor"));
        assert!(markdown.contains("## Body"));
        assert!(markdown.contains("Long issue body"));
    }

    #[test]
    fn batch_metadata_references_snapshots_and_profile() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let snapshots = vec![IssueSnapshot {
            id: "PROJ-123".into(),
            source: "123".into(),
            snapshot: ".local/issues/PROJ-123.md".into(),
        }];

        let path = write_batch_metadata(&ctx, "codex-yolo", &None, &snapshots).unwrap();
        let content = std::fs::read_to_string(path).unwrap();

        assert!(content.contains("profile = \"codex-yolo\""));
        assert!(content.contains("id = \"PROJ-123\""));
        assert!(content.contains("snapshot = \".local/issues/PROJ-123.md\""));
    }
}
