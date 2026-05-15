use crate::commands::issue;
use crate::commands::new as new_command;
use crate::config::IssueProviderType;
use crate::context::Ctx;
use crate::worktree_naming;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskDocument {
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) branch: String,
    #[serde(default)]
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) origin: Option<TaskOrigin>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskOrigin {
    pub(crate) provider: String,
    pub(crate) id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedTask {
    pub(crate) key: String,
    pub(crate) branch: String,
}

impl TaskDocument {
    pub(crate) fn title_or_key(&self, key: &str) -> String {
        if self.title.trim().is_empty() {
            key.to_string()
        } else {
            self.title.clone()
        }
    }

    pub(crate) fn mode(&self) -> &'static str {
        if self.origin.is_some() {
            "issue"
        } else {
            "new"
        }
    }

    pub(crate) fn identifier_or_key(&self, key: &str) -> String {
        self.origin
            .as_ref()
            .map(|origin| origin.id.clone())
            .unwrap_or_else(|| key.to_string())
    }
}

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

        let path = task_path(ctx, &key);
        let doc = if path.exists() {
            read_task_document(ctx, &key)?
        } else {
            let branch = new_command::branch_name_from_words(&[title.to_string()])?;
            let doc = TaskDocument {
                title: title.to_string(),
                branch,
                body: String::new(),
                origin: None,
            };
            write_task_document(ctx, &key, &doc)?;
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
        let key = safe_task_key(&issue.identifier);
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
        write_task_document(ctx, &key, &doc)?;
        tasks.push(PreparedTask { key, branch });
    }

    Ok(tasks)
}

pub(crate) fn read_task_document(ctx: &Ctx, key: &str) -> Result<TaskDocument> {
    let content = fs::read_to_string(task_path(ctx, key))
        .with_context(|| format!("Failed to read task: {}", task_relative_path(key)))?;
    let task: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {}", task_relative_path(key)))?;
    Ok(task)
}

pub(crate) fn read_task_file(ctx: &Ctx, key: &str) -> Result<(TaskDocument, String, String)> {
    let path = task_relative_path(key);
    let content = fs::read_to_string(ctx.repo_root.join(&path))
        .with_context(|| format!("Failed to read task: {path}"))?;
    let task: TaskDocument =
        toml::from_str(&content).with_context(|| format!("Failed to parse task: {path}"))?;
    Ok((task, path, content))
}

pub(crate) fn write_task_document(ctx: &Ctx, key: &str, task: &TaskDocument) -> Result<()> {
    let tasks_dir = ctx.repo_root.join(".local/tasks");
    fs::create_dir_all(&tasks_dir)?;
    fs::write(task_path(ctx, key), render_task_document(task))?;
    Ok(())
}

pub(crate) fn write_task_branch(ctx: &Ctx, key: &str, branch: &str) -> Result<()> {
    let mut task = read_task_document(ctx, key)?;
    task.branch = branch.to_string();
    write_task_document(ctx, key, &task)
}

pub(crate) fn task_relative_path(key: &str) -> String {
    format!(".local/tasks/{}.toml", safe_task_key(key))
}

fn task_path(ctx: &Ctx, key: &str) -> PathBuf {
    ctx.repo_root.join(task_relative_path(key))
}

fn task_key_from_text(value: &str) -> Result<String> {
    new_command::branch_name_from_words(&[value.to_string()])
}

fn issue_provider_name(ctx: &Ctx) -> Result<String> {
    let issues = ctx
        .config
        .issues
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No [issues] section in .wt.toml"))?;
    Ok(match issues.provider {
        IssueProviderType::Github => "github",
        IssueProviderType::Linear => "linear",
    }
    .into())
}

pub(crate) fn safe_task_key(value: &str) -> String {
    let key = value
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

    if key.is_empty() { "task".into() } else { key }
}

fn render_task_document(task: &TaskDocument) -> String {
    let mut content = String::new();
    content.push_str(&format!("title = {}\n", toml_quote(&task.title)));
    if !task.branch.trim().is_empty() {
        content.push_str(&format!("branch = {}\n", toml_quote(&task.branch)));
    }
    if !task.body.trim().is_empty() {
        content.push_str(&format!("body = {}\n", toml_multiline_string(&task.body)));
    }
    if let Some(origin) = &task.origin {
        content.push_str("\n[origin]\n");
        content.push_str(&format!("provider = {}\n", toml_quote(&origin.provider)));
        content.push_str(&format!("id = {}\n", toml_quote(&origin.id)));
    }
    content
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
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};

    #[test]
    fn safe_task_key_replaces_unsafe_chars() {
        assert_eq!(safe_task_key("#42"), "42");
        assert_eq!(safe_task_key("PROJ-123"), "PROJ-123");
        assert_eq!(safe_task_key("bad/value"), "bad-value");
    }

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
