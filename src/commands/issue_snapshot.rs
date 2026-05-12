use crate::commands::issue;
use crate::context::Ctx;
use anyhow::Result;
use std::fs;

#[derive(Clone, Debug)]
pub(crate) struct IssueSnapshot {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) title: String,
    pub(crate) branch: String,
    pub(crate) snapshot: String,
}

pub(crate) fn snapshot_issues(ctx: &Ctx, issues: &[String]) -> Result<Vec<IssueSnapshot>> {
    let provider = issue::build_provider(ctx)?;
    let issues_dir = ctx.repo_root.join(".local/issues");
    fs::create_dir_all(&issues_dir)?;

    let mut snapshots = Vec::new();
    for source in issues {
        let issue = provider.get_issue(source.trim_start_matches('#'))?;
        let file_name = format!("{}.md", safe_file_stem(&issue.identifier));
        let relative_path = format!(".local/issues/{file_name}");
        let snapshot_path = ctx.repo_root.join(&relative_path);
        let branch = issue.branch_name.as_deref().unwrap_or("-").to_string();
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
            title: issue.title,
            branch,
            snapshot: relative_path,
        });
    }

    Ok(snapshots)
}

pub(crate) fn safe_file_stem(value: &str) -> String {
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
