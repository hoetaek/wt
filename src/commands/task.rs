use crate::commands::issue;
use crate::commands::new as new_command;
use crate::commands::task_run;
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

#[derive(Clone, Debug)]
pub(crate) struct SelectedTask {
    pub(crate) key: String,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) document: TaskDocument,
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

#[cfg(test)]
pub(crate) fn select_local_task(ctx: &Ctx) -> Result<SelectedTask> {
    let tasks = list_local_tasks(ctx)?;
    if tasks.is_empty() {
        bail!("No task files found in .local/tasks");
    }

    let items = tasks.iter().map(task_selection_label).collect::<Vec<_>>();
    let idx = ctx.ui.select("Select a local task", &items)?;
    let task = tasks
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {idx}"))?;
    Ok(task.clone())
}

pub(crate) fn select_local_tasks(ctx: &Ctx) -> Result<Vec<SelectedTask>> {
    let tasks = list_local_tasks(ctx)?;
    if tasks.is_empty() {
        bail!("No task files found in .local/tasks");
    }

    let items = tasks.iter().map(task_selection_label).collect::<Vec<_>>();
    let selections = ctx.ui.multi_select("Select local tasks to start", &items)?;
    let mut selected = Vec::new();
    for idx in selections {
        let task = tasks
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {idx}"))?;
        selected.push(task.clone());
    }
    Ok(selected)
}

pub(crate) fn select_local_task_by_key(ctx: &Ctx, key: &str) -> Result<SelectedTask> {
    let key = safe_task_key(key);
    let (document, path, content) = read_task_file(ctx, &key)?;
    Ok(SelectedTask {
        key,
        path,
        content,
        document,
    })
}

pub(crate) fn list_local_tasks(ctx: &Ctx) -> Result<Vec<SelectedTask>> {
    let tasks_dir = ctx.repo_root.join(".local/tasks");
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in
        fs::read_dir(&tasks_dir).with_context(|| "Failed to read task directory: .local/tasks")?
    {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut tasks = Vec::new();
    for path in paths {
        let task = read_selected_task(ctx, path)?;
        if task_run::task_is_selectable(ctx, &task.key)? {
            tasks.push(task);
        }
    }
    Ok(tasks)
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

pub(crate) fn prepared_branch_name(branch: &str) -> Option<&str> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "-" {
        None
    } else {
        Some(branch)
    }
}

fn task_path(ctx: &Ctx, key: &str) -> PathBuf {
    ctx.repo_root.join(task_relative_path(key))
}

fn task_key_from_text(value: &str) -> Result<String> {
    new_command::branch_name_from_words(&[value.to_string()])
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

fn read_selected_task(ctx: &Ctx, path: PathBuf) -> Result<SelectedTask> {
    let relative_path = path
        .strip_prefix(&ctx.repo_root)
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned();
    let key = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Task file is missing a key: {relative_path}"))?
        .to_string();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read task: {relative_path}"))?;
    let document: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {relative_path}"))?;
    Ok(SelectedTask {
        key,
        path: relative_path,
        content,
        document,
    })
}

fn task_selection_label(task: &SelectedTask) -> String {
    let title = task.document.title_or_key(&task.key);
    let branch = prepared_branch_name(&task.document.branch);
    match (title == task.key, branch) {
        (true, Some(branch)) => format!("{} ({branch})", task.key),
        (false, Some(branch)) => format!("{} - {} ({branch})", task.key, title),
        (true, None) => task.key.clone(),
        (false, None) => format!("{} - {}", task.key, title),
    }
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

pub(crate) fn workspace_run_label(idx: usize, total: usize, identifier: Option<&str>) -> String {
    let total = total.max(1);
    let mut label = format!("{}/{}", idx + 1, total);
    if let Some(identifier) = identifier
        .map(str::trim)
        .filter(|identifier| !identifier.is_empty())
    {
        label.push(' ');
        label.push_str(identifier);
    }
    label
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
    if value.starts_with(['\n', '\r']) {
        return toml_quote(value);
    }
    let escaped = value
        .replace("\\", "\\\\")
        .replace("\"\"\"", "\\\"\\\"\\\"");
    format!("\"\"\"{}\"\"\"", escaped)
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
    fn workspace_run_label_keeps_order_and_identifier_short() {
        assert_eq!(workspace_run_label(1, 5, Some("PROJ-123")), "2/5 PROJ-123");
        assert_eq!(workspace_run_label(0, 3, None), "1/3");
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

    #[test]
    fn select_local_task_errors_when_no_task_files_exist() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = select_local_task(&ctx);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No task files found in .local/tasks")
        );
    }

    #[test]
    fn select_local_task_reads_selected_task_document() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("a-first.toml"),
            "title = \"First\"\nbranch = \"first\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("b-second.toml"),
            "title = \"Second\"\nbranch = \"second\"\nbody = \"details\"\n",
        )
        .unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        let selected = select_local_task(&ctx).unwrap();

        assert_eq!(selected.key, "b-second");
        assert_eq!(selected.path, ".local/tasks/b-second.toml");
        assert_eq!(selected.document.title, "Second");
        assert_eq!(selected.document.branch, "second");
        assert!(selected.content.contains("body = \"details\""));
    }

    #[test]
    fn list_local_tasks_omits_tasks_with_completed_runs() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("a-first.toml"),
            "title = \"First\"\nbranch = \"first\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("b-second.toml"),
            "title = \"Second\"\nbranch = \"second\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        task_run::create(
            &ctx,
            "a-first",
            "first",
            task_run::SOURCE_NEW,
            None,
            task_run::STATUS_DONE,
        )
        .unwrap();

        let tasks = list_local_tasks(&ctx).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].key, "b-second");
    }

    #[test]
    fn list_local_tasks_rejects_unknown_task_fields() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("bad.toml"),
            "title = \"Bad\"\nbranch = \"bad\"\nextra = true\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = list_local_tasks(&ctx);

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("unknown field"));
    }
}
