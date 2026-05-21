use crate::context::Ctx;
use crate::task::{self, TaskDocument};
use anyhow::{Context, Result};
use console::measure_text_width;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::Path;

const LIST_START: &str = "◆";
const BAR: &str = "│";
const FOOTER: &str = "└";
const BULLET: &str = "•";
const TITLE_COLUMN_MAX: usize = 56;
const SOURCE_COLUMN_MAX: usize = 18;
const TASK_COLUMN_MAX: usize = 34;
const BRANCH_COLUMN_MAX: usize = 48;

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

    for path in task::task_document_paths(ctx)? {
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
    ctx.storage_root.display_path(path)
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
        ctx.ui
            .print_step("No tasks found in <git-common-dir>/wt/tasks");
        return;
    }

    for line in render_text_lines(report) {
        ctx.ui.print_plain(&line);
    }

    for invalid in &report.invalid_tasks {
        ctx.ui.print_warning(&format!(
            "Invalid task {}: {}",
            invalid.path,
            one_line(&invalid.error)
        ));
    }
}

fn render_text_lines(report: &TaskListReport) -> Vec<String> {
    let mut lines = vec![format!("{LIST_START} Tasks"), BAR.to_string()];
    let mut emitted_group = false;
    for group in ["provider-origin", "local"] {
        let rows = report
            .tasks
            .iter()
            .filter(|row| row.source == group)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }

        if emitted_group {
            lines.push(BAR.to_string());
        }
        lines.push(format!("{BAR} {group}"));
        emitted_group = true;
        let widths = task_list_column_widths(&rows);
        for row in rows {
            lines.push(format!(
                "{BAR}  {BULLET}  {}",
                task_inventory_label(row, &widths)
            ));
        }
    }
    lines.push(FOOTER.to_string());
    lines
}

#[derive(Debug, Clone, Copy)]
struct TaskListColumnWidths {
    title: usize,
    source: usize,
    task: usize,
    branch: usize,
}

fn task_list_column_widths(rows: &[&TaskListRow]) -> TaskListColumnWidths {
    rows.iter().fold(
        TaskListColumnWidths {
            title: 0,
            source: 0,
            task: 0,
            branch: 0,
        },
        |widths, row| {
            let columns = task_inventory_columns(row);
            TaskListColumnWidths {
                title: capped_width(widths.title, &columns.title, TITLE_COLUMN_MAX),
                source: capped_width(widths.source, &columns.source, SOURCE_COLUMN_MAX),
                task: capped_width(widths.task, &columns.task, TASK_COLUMN_MAX),
                branch: columns.branch.as_deref().map_or(widths.branch, |branch| {
                    capped_width(widths.branch, branch, BRANCH_COLUMN_MAX)
                }),
            }
        },
    )
}

#[derive(Debug, Clone)]
struct TaskInventoryColumns {
    title: String,
    source: String,
    task: String,
    branch: Option<String>,
}

fn task_inventory_columns(row: &TaskListRow) -> TaskInventoryColumns {
    TaskInventoryColumns {
        title: row.display.label().to_string(),
        source: task_inventory_source(row),
        task: format!("task {}", row.key),
        branch: row.branch.as_ref().map(|branch| format!("branch {branch}")),
    }
}

fn task_inventory_source(row: &TaskListRow) -> String {
    row.origin
        .as_ref()
        .map(|origin| {
            format!(
                "{} {}",
                provider_display_label(&origin.provider),
                origin.id.trim()
            )
        })
        .unwrap_or_else(|| "not published".into())
}

fn provider_display_label(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "github" => "GitHub".into(),
        "linear" => "Linear".into(),
        "" => "external".into(),
        other => other.to_string(),
    }
}

fn task_inventory_label(row: &TaskListRow, widths: &TaskListColumnWidths) -> String {
    let columns = task_inventory_columns(row);
    let mut parts = vec![
        pad_column(&columns.title, widths.title),
        pad_column(&columns.source, widths.source),
        pad_column(&columns.task, widths.task),
    ];
    if let Some(branch) = columns.branch {
        parts.push(truncate_display_width(&branch, widths.branch));
    }
    parts.join("  ")
}

fn capped_width(current: usize, value: &str, max_width: usize) -> usize {
    current.max(measure_text_width(value).min(max_width))
}

fn pad_column(value: &str, width: usize) -> String {
    let value = truncate_display_width(value, width);
    let padding = width.saturating_sub(measure_text_width(&value));
    format!("{value}{}", " ".repeat(padding))
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if measure_text_width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut width = 0;
    let mut truncated = String::new();
    let target_width = max_width - 3;
    for ch in value.chars() {
        let ch_width = measure_text_width(&ch.to_string());
        if width + ch_width > target_width {
            break;
        }
        truncated.push(ch);
        width += ch_width;
    }
    truncated.push_str("...");
    truncated
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
        let tasks_dir = dir.path().join(".git/wt/tasks");
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
        assert_eq!(
            report.tasks[0].path,
            "<git-common-dir>/wt/tasks/PROJ-123.toml"
        );
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
        let tasks_dir = dir.path().join(".git/wt/tasks");
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
