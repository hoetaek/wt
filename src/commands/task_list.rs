use crate::context::{Ctx, PromptItem};
use crate::task::{self, TaskDocument};
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn run(ctx: &Ctx) -> Result<()> {
    let report = collect(ctx)?;
    if ctx.is_json() {
        write_json(&report)?;
    } else {
        print_text(ctx, &report);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct TaskListReport {
    tasks: Vec<TaskListRow>,
    invalid_tasks: Vec<InvalidTaskRow>,
}

#[derive(Debug, Serialize)]
struct TaskListRow {
    key: String,
    path: String,
    title: String,
    branch: Option<String>,
    origin: Option<TaskOriginSummary>,
    publish_state: String,
    source: String,
    body_summary: Option<String>,
    #[serde(skip_serializing)]
    display: task::TaskDocumentDisplay,
}

#[derive(Debug, Serialize)]
struct TaskOriginSummary {
    provider: String,
    id: String,
}

#[derive(Debug, Serialize)]
struct InvalidTaskRow {
    key: String,
    path: String,
    error: String,
}

fn collect(ctx: &Ctx) -> Result<TaskListReport> {
    let mut tasks = Vec::new();
    let mut invalid_tasks = Vec::new();

    for path in task_paths(ctx)? {
        let key = task_key_from_path(&path).unwrap_or_default();
        let relative_path = task_relative_path(ctx, &path);
        match read_task_row(ctx, &path) {
            Ok(row) => tasks.push(row),
            Err(err) => invalid_tasks.push(InvalidTaskRow {
                key,
                path: relative_path,
                error: format!("{err:#}"),
            }),
        }
    }

    Ok(TaskListReport {
        tasks,
        invalid_tasks,
    })
}

fn task_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
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
    Ok(paths)
}

fn read_task_row(ctx: &Ctx, path: &Path) -> Result<TaskListRow> {
    let key = task_key_from_path(path)?;
    let relative_path = task_relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read task: {relative_path}"))?;
    let document: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {relative_path}"))?;
    Ok(task_row(key, relative_path, document))
}

fn task_row(key: String, path: String, document: TaskDocument) -> TaskListRow {
    let display = task::TaskDocumentDisplay::for_document(&key, &document);
    let origin = document.origin.as_ref().map(|origin| TaskOriginSummary {
        provider: origin.provider.clone(),
        id: origin.id.clone(),
    });
    let publish_state = if origin.is_some() {
        "published"
    } else {
        "local"
    };
    let source = if origin.is_some() {
        "provider-origin"
    } else {
        "local"
    };

    TaskListRow {
        key,
        path,
        title: document.title,
        branch: task::prepared_branch_name(&document.branch).map(str::to_string),
        origin,
        publish_state: publish_state.into(),
        source: source.into(),
        body_summary: body_summary(&document.body),
        display,
    }
}

fn task_key_from_path(path: &Path) -> Result<String> {
    Ok(task::safe_task_key(
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("Task file is missing a key: {}", path.display()))?,
    ))
}

fn task_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn body_summary(body: &str) -> Option<String> {
    let summary = one_line(body);
    if summary.is_empty() {
        None
    } else {
        Some(truncate_chars(&summary, 120))
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn print_text(ctx: &Ctx, report: &TaskListReport) {
    if report.tasks.is_empty() && report.invalid_tasks.is_empty() {
        ctx.ui.print_step("No tasks found in .local/tasks");
        return;
    }

    for row in &report.tasks {
        ctx.ui.print_step(&task_inventory_label(row));
        ctx.ui.print_dim(&format!("  Path: {}", row.path));
        ctx.ui
            .print_dim(&format!("  Origin: {}", origin_label(row)));
        if let Some(summary) = row.body_summary.as_deref() {
            ctx.ui.print_dim(&format!("  Summary: {summary}"));
        }
    }

    for invalid in &report.invalid_tasks {
        ctx.ui.print_warning(&format!(
            "Invalid task {}: {}",
            invalid.path,
            one_line(&invalid.error)
        ));
    }
}

fn task_inventory_label(row: &TaskListRow) -> String {
    let mut hint_parts = row.display.inventory_hint_parts();
    hint_parts.push(format!("source {}", row.source));
    PromptItem::from_hint_parts(row.display.label().to_string(), hint_parts).render_plain()
}

fn origin_label(row: &TaskListRow) -> String {
    row.origin
        .as_ref()
        .map(|origin| format!("{}:{}", origin.provider, origin.id))
        .unwrap_or_else(|| "none".into())
}

fn write_json(report: &TaskListReport) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};

    fn ctx(root: &Path, output_mode: OutputMode) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                output_mode,
                ..CtxOptions::default()
            },
        )
    }

    #[test]
    fn collect_lists_valid_tasks_and_reports_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".local/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("local.toml"),
            r#"title = "Local task"
branch = "feature/local"
body = "Do local work"
"#,
        )
        .unwrap();
        fs::write(
            tasks_dir.join("PROJ-123.toml"),
            r#"title = "Provider task"
branch = "alice/proj-123-provider-task"
body = "Do provider work"

[origin]
provider = "linear"
id = "PROJ-123"
"#,
        )
        .unwrap();
        fs::write(tasks_dir.join("bad.toml"), "unknown = true\n").unwrap();

        let report = collect(&ctx).unwrap();

        assert_eq!(report.tasks.len(), 2);
        assert_eq!(report.invalid_tasks.len(), 1);
        assert_eq!(report.tasks[0].key, "PROJ-123");
        assert_eq!(report.tasks[0].path, ".local/tasks/PROJ-123.toml");
        assert_eq!(report.tasks[0].publish_state, "published");
        assert_eq!(report.tasks[0].source, "provider-origin");
        assert_eq!(report.tasks[0].origin.as_ref().unwrap().provider, "linear");
        assert_eq!(report.tasks[1].key, "local");
        assert_eq!(report.tasks[1].publish_state, "local");
        assert_eq!(report.tasks[1].source, "local");
        assert_eq!(report.invalid_tasks[0].key, "bad");
    }

    #[test]
    fn collect_does_not_apply_selector_visible_cap() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".local/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();

        for idx in 1..=11 {
            fs::write(
                tasks_dir.join(format!("task-{idx}.toml")),
                format!(
                    r#"title = "Task {idx}"
branch = "feature/task-{idx}"
"#
                ),
            )
            .unwrap();
        }

        let report = collect(&ctx).unwrap();

        assert_eq!(report.tasks.len(), 11);
        assert_eq!(report.invalid_tasks.len(), 0);
        assert_eq!(report.tasks[10].key, "task-9");
    }
}
