use crate::commands::issue;
use crate::commands::new as new_command;
use crate::config::IssueProviderType;
use crate::context::Ctx;
use crate::task::{self, PreparedTask, TaskDocument, TaskOrigin};
use crate::worktree_naming;
use anyhow::{Result, bail};
use std::collections::HashSet;

pub(crate) fn prepare_named_tasks(ctx: &Ctx, names: &[String]) -> Result<Vec<PreparedTask>> {
    if names.is_empty() {
        bail!("Usage: wt <batch|stack> task <task>...");
    }

    let mut seen = HashSet::new();
    let mut tasks = Vec::new();
    for name in names {
        let title = name.trim();
        if title.is_empty() {
            bail!("Task cannot be empty");
        }
        let key = task_key_from_text(title)?;
        if !seen.insert(key.clone()) {
            bail!("Duplicate task: {key}");
        }

        let doc = if task::task_exists(ctx, &key) {
            task::read_task_document(ctx, &key)?
        } else {
            let branch = new_command::branch_name_from_words(&[title.to_string()])?;
            let doc = TaskDocument {
                title: title.to_string(),
                branch,
                body: String::new(),
                origin: None,
            };
            task::write_task_document(ctx, &key, &doc)?;
            doc
        };

        tasks.push(PreparedTask {
            key,
            branch: doc.branch,
        });
    }

    Ok(tasks)
}

pub(crate) fn prepare_issue_tasks(ctx: &Ctx, issues: &[String]) -> Result<Vec<PreparedTask>> {
    let provider = issue::build_provider(ctx)?;
    let provider_name = issue_provider_name(ctx)?;
    let mut seen = HashSet::new();
    let mut tasks = Vec::new();

    for source in issues {
        let issue = provider.get_issue(source.trim_start_matches('#'))?;
        let naming = worktree_naming::generate(
            ctx,
            &issue.identifier,
            &issue.title,
            issue.branch_name.as_deref(),
        )?;
        let branch = naming
            .and_then(|naming| naming.branch)
            .or(issue.branch_name)
            .unwrap_or_default();
        let key = task::safe_task_key(&issue.identifier);
        if !seen.insert(key.clone()) {
            bail!("Duplicate task: {key}");
        }

        let doc = TaskDocument {
            title: issue.title,
            branch: branch.clone(),
            body: issue.body.unwrap_or_default(),
            origin: Some(TaskOrigin {
                provider: provider_name.clone(),
                id: issue.identifier,
            }),
        };
        task::write_task_document(ctx, &key, &doc)?;
        tasks.push(PreparedTask { key, branch });
    }

    Ok(tasks)
}

pub(crate) fn issue_provider_name(ctx: &Ctx) -> Result<String> {
    let issues = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    Ok(match issues.provider {
        IssueProviderType::Github => "github",
        IssueProviderType::Linear => "linear",
    }
    .into())
}

fn task_key_from_text(value: &str) -> Result<String> {
    new_command::branch_name_from_words(&[value.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};

    #[test]
    fn prepare_issue_tasks_writes_task_toml() {
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

        let tasks = prepare_issue_tasks(&ctx, &["PROJ-123".into()]).unwrap();

        assert_eq!(tasks[0].key, "PROJ-123");
        assert_eq!(tasks[0].branch, "alice/proj-123-fix-editor");
        let content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(content.contains("title = \"Fix editor\""));
        assert!(content.contains("branch = \"alice/proj-123-fix-editor\""));
        assert!(content.contains("body = \"\"\""));
        assert!(content.contains("Long issue body"));
        assert!(content.contains("[origin]"));
        assert!(content.contains("provider = \"linear\""));
        assert!(content.contains("id = \"PROJ-123\""));
    }
}
