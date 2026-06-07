use crate::context::Ctx;
use crate::origin_action_menu::{OriginActionMenu, OriginLabel};
use crate::origin_snapshot::{FieldSnapshot, OriginHealthSummary, read_task_snapshot};
use crate::task::{self, TaskDocument};
use crate::task_run;
use anyhow::{Context, Result};
use console::measure_text_width;
use serde::Serialize;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;

const LIST_START: &str = "◆";
const BAR: &str = "│";
const FOOTER: &str = "└";
const BULLET: &str = "•";
const STATUS_COLUMN_MAX: usize = 10;
const TITLE_COLUMN_MAX: usize = 56;
const SOURCE_COLUMN_MAX: usize = 18;
const TASK_COLUMN_MAX: usize = 34;
const NEXT_COLUMN_MAX: usize = 8;
const BRANCH_COLUMN_MAX: usize = 48;

pub(crate) fn run(ctx: &Ctx, all: bool) -> Result<()> {
    let report = collect(ctx, all)?;
    if should_open_browser(ctx) {
        if crate::tui::terminal_size_allows_task_browser() {
            return crate::tui::run_task_browser_with(browser_app(&report));
        }
        ctx.ui.print_warning(
            "Terminal is too small for the task browser; falling back to text output",
        );
    }
    if ctx.is_json() {
        write_json(&report)?;
    } else {
        print_text(ctx, &report);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskListReport {
    tasks: Vec<TaskListRow>,
    invalid_tasks: Vec<InvalidTaskRow>,
    #[serde(skip_serializing)]
    hidden_task_count: usize,
    #[serde(skip_serializing)]
    full_inventory: bool,
}

pub(crate) fn browser_rows(report: &TaskListReport) -> Vec<crate::tui::app::BrowserRow> {
    report.tasks.iter().map(browser_row).collect()
}

pub(crate) fn browser_app(report: &TaskListReport) -> crate::tui::app::AppState {
    crate::tui::app::AppState::with_diagnostics(browser_rows(report), browser_diagnostics(report))
}

#[derive(Debug, Serialize)]
struct TaskListRow {
    key: String,
    path: String,
    title: String,
    branch: Option<String>,
    origin: Option<TaskOriginSummary>,
    origin_health: OriginHealthSummary,
    publish_state: String,
    source: String,
    body_summary: Option<String>,
    #[serde(skip_serializing)]
    display: task::TaskDocumentDisplay,
}

impl TaskListRow {
    #[allow(dead_code)]
    pub(crate) fn origin_action_menu(&self) -> OriginActionMenu {
        let title = self.display.label();
        if let Some(origin) = &self.origin {
            OriginActionMenu::for_origin_task(
                &self.key,
                title,
                OriginLabel::new(&origin.provider, &origin.id),
            )
        } else {
            OriginActionMenu::for_local_task(&self.key, title)
        }
    }
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

fn collect(ctx: &Ctx, all: bool) -> Result<TaskListReport> {
    let mut tasks = Vec::new();
    let mut invalid_tasks = Vec::new();
    let mut hidden_task_count = 0;

    for path in task::task_document_paths(ctx)? {
        let key = task_key_from_path(&path).unwrap_or_default();
        let relative_path = task_relative_path(ctx, &path);
        match read_task_row(ctx, &path) {
            Ok(row) => {
                if all || task_run::task_is_selectable(ctx, &row.key)? {
                    tasks.push(row);
                } else {
                    hidden_task_count += 1;
                }
            }
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
        hidden_task_count,
        full_inventory: all,
    })
}

fn read_task_row(ctx: &Ctx, path: &Path) -> Result<TaskListRow> {
    let key = task_key_from_path(path)?;
    let relative_path = task_relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read task: {relative_path}"))?;
    let document: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {relative_path}"))?;
    Ok(task_row(ctx, key, relative_path, document))
}

fn task_row(ctx: &Ctx, key: String, path: String, document: TaskDocument) -> TaskListRow {
    let display = task::TaskDocumentDisplay::for_document(&key, &document);
    let origin = document.origin.as_ref().map(|origin| TaskOriginSummary {
        provider: origin.provider.clone(),
        id: origin.id.clone(),
    });
    let origin_health = task_origin_health(ctx, &key, &document);
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
        origin_health,
        publish_state: publish_state.into(),
        source: source.into(),
        body_summary: body_summary(&document.body),
        display,
    }
}

fn task_origin_health(ctx: &Ctx, key: &str, document: &TaskDocument) -> OriginHealthSummary {
    let Some(origin) = document.origin.as_ref() else {
        return OriginHealthSummary::task_without_origin();
    };
    let local_fields = FieldSnapshot::new(document.title.clone(), document.body.clone());
    match read_task_snapshot(&ctx.storage_root, key) {
        Ok(snapshot) => OriginHealthSummary::from_snapshot(
            &origin.provider,
            &origin.id,
            &local_fields,
            snapshot.as_ref(),
            "run",
        ),
        Err(err) => OriginHealthSummary::from_snapshot_error(&origin.provider, &origin.id, &err),
    }
}

fn should_open_browser(ctx: &Ctx) -> bool {
    !ctx.is_json() && !ctx.quiet && ctx.ui.can_prompt() && std::io::stdout().is_terminal()
}

fn browser_row(row: &TaskListRow) -> crate::tui::app::BrowserRow {
    crate::tui::app::BrowserRow {
        key: row.key.clone(),
        title: row.display.label().to_string(),
        status: row.origin_health.status.clone(),
        origin_label: row.origin_health.origin_label.clone(),
        next_action: row.origin_health.next_action.clone(),
        preview_lines: browser_preview_lines(row),
    }
}

fn browser_preview_lines(row: &TaskListRow) -> Vec<String> {
    vec![
        format!("Local path  {}", row.path),
        format!(
            "Branch      {}",
            row.branch.as_deref().unwrap_or("not prepared")
        ),
        format!("Origin      {}", row.origin_health.origin_label),
        format!(
            "Fetched     {}",
            row.origin_health.last_fetched.as_deref().unwrap_or("never")
        ),
        format!(
            "Divergence  {}",
            row.origin_health.divergence.as_deref().unwrap_or("none")
        ),
        format!("Next        {}", row.origin_health.next_action),
    ]
}

fn browser_diagnostics(report: &TaskListReport) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let invalid_count = report.invalid_tasks.len();
    if invalid_count > 0 {
        let noun = if invalid_count == 1 {
            "task file"
        } else {
            "task files"
        };
        diagnostics.push(format!("{invalid_count} invalid {noun}"));
    }
    if report.hidden_task_count > 0 {
        diagnostics.push(format!("{} hidden - use --all", report.hidden_task_count));
    }
    diagnostics
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
    if report.tasks.is_empty() && report.invalid_tasks.is_empty() && report.hidden_task_count == 0 {
        ctx.ui
            .print_plain("No tasks found in <repo-root>/.wt/execution/tasks");
        return;
    }

    if report.tasks.is_empty() && report.invalid_tasks.is_empty() && !report.full_inventory {
        ctx.ui
            .print_plain("No actionable tasks found in <repo-root>/.wt/execution/tasks");
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
    if report.hidden_task_count > 0 {
        if emitted_group {
            lines.push(BAR.to_string());
        }
        lines.push(format!(
            "{BAR} {}",
            hidden_task_count_hint(report.hidden_task_count)
        ));
    }
    lines.push(FOOTER.to_string());
    lines
}

fn hidden_task_count_hint(count: usize) -> String {
    let noun = if count == 1 { "task" } else { "tasks" };
    format!("{count} {noun} hidden; use wt task list --all to show the full inventory")
}

#[derive(Debug, Clone, Copy)]
struct TaskListColumnWidths {
    status: usize,
    title: usize,
    source: usize,
    task: usize,
    next: usize,
    branch: usize,
}

fn task_list_column_widths(rows: &[&TaskListRow]) -> TaskListColumnWidths {
    rows.iter().fold(
        TaskListColumnWidths {
            status: 0,
            title: 0,
            source: 0,
            task: 0,
            next: 0,
            branch: 0,
        },
        |widths, row| {
            let columns = task_inventory_columns(row);
            TaskListColumnWidths {
                status: capped_width(widths.status, &columns.status, STATUS_COLUMN_MAX),
                title: capped_width(widths.title, &columns.title, TITLE_COLUMN_MAX),
                source: capped_width(widths.source, &columns.source, SOURCE_COLUMN_MAX),
                task: capped_width(widths.task, &columns.task, TASK_COLUMN_MAX),
                next: capped_width(widths.next, &columns.next, NEXT_COLUMN_MAX),
                branch: columns.branch.as_deref().map_or(widths.branch, |branch| {
                    capped_width(widths.branch, branch, BRANCH_COLUMN_MAX)
                }),
            }
        },
    )
}

#[derive(Debug, Clone)]
struct TaskInventoryColumns {
    status: String,
    title: String,
    source: String,
    task: String,
    next: String,
    branch: Option<String>,
}

fn task_inventory_columns(row: &TaskListRow) -> TaskInventoryColumns {
    TaskInventoryColumns {
        status: row.origin_health.status.clone(),
        title: row.display.label().to_string(),
        source: row.origin_health.origin_label.clone(),
        task: format!("task {}", row.key),
        next: row.origin_health.next_action.clone(),
        branch: row.branch.as_ref().map(|branch| format!("branch {branch}")),
    }
}

fn task_inventory_label(row: &TaskListRow, widths: &TaskListColumnWidths) -> String {
    let columns = task_inventory_columns(row);
    let mut parts = vec![
        pad_column(&columns.status, widths.status),
        pad_column(&columns.source, widths.source),
        pad_column(&columns.title, widths.title),
        pad_column(&columns.task, widths.task),
        pad_column(&columns.next, widths.next),
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
    use crate::task_run;

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
        let tasks_dir = dir.path().join(".wt/execution/tasks");
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

        let report = collect(&ctx, false).unwrap();

        assert_eq!(report.tasks.len(), 2);
        assert_eq!(report.invalid_tasks.len(), 1);
        assert_eq!(report.hidden_task_count, 0);
        assert!(!report.full_inventory);
        assert_eq!(report.tasks[0].key, "PROJ-123");
        assert_eq!(
            report.tasks[0].path,
            "<repo-root>/.wt/execution/tasks/PROJ-123.toml"
        );
        assert_eq!(report.tasks[0].publish_state, "published");
        assert_eq!(report.tasks[0].source, "provider-origin");
        assert_eq!(report.tasks[0].origin.as_ref().unwrap().provider, "linear");
        assert_eq!(report.tasks[0].origin_health.status, "stale");
        assert_eq!(report.tasks[0].origin_health.next_action, "fetch");
        assert_eq!(
            report.tasks[0].origin_health.origin_label,
            "Linear PROJ-123"
        );
        assert_eq!(report.tasks[1].key, "local");
        assert_eq!(report.tasks[1].publish_state, "local");
        assert_eq!(report.tasks[1].source, "local");
        assert_eq!(report.tasks[1].origin_health.status, "local");
        assert_eq!(report.tasks[1].origin_health.next_action, "pub");
        assert_eq!(report.tasks[1].origin_health.origin_label, "not published");
        assert_eq!(report.invalid_tasks[0].key, "bad");
    }

    #[test]
    fn origin_health_uses_snapshot_without_provider_calls() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Original title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Remote title", "local body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();

        let report = collect(&ctx, true).unwrap();

        assert_eq!(report.tasks[0].origin_health.status, "conflict");
        assert_eq!(report.tasks[0].origin_health.next_action, "diff");
        assert_eq!(report.tasks[0].origin_health.origin_label, "Linear WT-142");
    }

    #[test]
    fn task_row_builds_origin_action_menu() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let row = task_row(
            &ctx,
            "scratch-clean".into(),
            "<repo-root>/.wt/execution/tasks/scratch-clean.toml".into(),
            TaskDocument {
                title: "Scratch cleanup".into(),
                branch: "scratch-clean".into(),
                body: String::new(),
                origin: None,
            },
        );

        let menu = row.origin_action_menu();
        assert!(menu.enabled("Publish as issue"));
        assert_eq!(
            menu.disabled_reason("Pull from issue").unwrap(),
            "no origin attached"
        );
    }

    #[test]
    fn browser_rows_project_origin_health_from_task_rows() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();

        let report = collect(&ctx, true).unwrap();
        let rows = browser_rows(&report);

        assert_eq!(rows[0].key, "origin-sync-tui");
        assert_eq!(rows[0].origin_label, "Linear WT-142");
        assert!(
            rows[0]
                .preview_lines
                .iter()
                .any(|line| line.contains("Linear WT-142"))
        );
    }

    fn browser_report_text(report: &TaskListReport) -> String {
        let backend = ratatui::backend::TestBackend::new(100, 18);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let app = browser_app(report);
        terminal
            .draw(|frame| crate::tui::render::draw(frame, &app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..18)
            .map(|y| {
                (0..100)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn browser_report_renders_invalid_task_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("valid.toml"),
            r#"title = "Valid"
branch = "valid"
"#,
        )
        .unwrap();
        fs::write(tasks_dir.join("bad.toml"), "unknown = true\n").unwrap();

        let report = collect(&ctx, false).unwrap();
        let text = browser_report_text(&report);

        assert!(text.contains("1 invalid task file"));
    }

    #[test]
    fn browser_report_renders_hidden_task_hint() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("visible.toml"),
            r#"title = "Visible"
branch = "visible"
"#,
        )
        .unwrap();
        fs::write(
            tasks_dir.join("hidden.toml"),
            r#"title = "Hidden"
branch = "hidden"
"#,
        )
        .unwrap();
        task_run::create(&ctx, "hidden", "hidden", None, task_run::STATUS_PASSED).unwrap();

        let report = collect(&ctx, false).unwrap();
        let text = browser_report_text(&report);

        assert!(text.contains("1 hidden"));
        assert!(text.contains("use --all"));
    }

    #[test]
    fn collect_does_not_apply_selector_visible_cap() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
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

        let report = collect(&ctx, false).unwrap();

        assert_eq!(report.tasks.len(), 11);
        assert_eq!(report.invalid_tasks.len(), 0);
        assert_eq!(report.tasks[10].key, "task-9");
    }

    #[test]
    fn collect_default_uses_task_selectability_and_all_keeps_inventory() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();

        for key in [
            "passed",
            "failed",
            "latest-failed",
            "no-run",
            "prepared",
            "running",
            "skipped",
        ] {
            fs::write(
                tasks_dir.join(format!("{key}.toml")),
                format!(
                    r#"title = "{key}"
branch = "feature/{key}"
"#
                ),
            )
            .unwrap();
        }

        task_run::create(
            &ctx,
            "passed",
            "feature/passed",
            None,
            task_run::STATUS_PASSED,
        )
        .unwrap();
        task_run::create(
            &ctx,
            "failed",
            "feature/failed",
            None,
            task_run::STATUS_FAILED,
        )
        .unwrap();
        task_run::create(
            &ctx,
            "latest-failed",
            "feature/latest-failed",
            None,
            task_run::STATUS_RUNNING,
        )
        .unwrap();
        task_run::create(
            &ctx,
            "latest-failed",
            "feature/latest-failed",
            None,
            task_run::STATUS_FAILED,
        )
        .unwrap();
        task_run::create(
            &ctx,
            "prepared",
            "feature/prepared",
            None,
            task_run::STATUS_PREPARED,
        )
        .unwrap();
        task_run::create(
            &ctx,
            "running",
            "feature/running",
            None,
            task_run::STATUS_RUNNING,
        )
        .unwrap();
        task_run::create(
            &ctx,
            "skipped",
            "feature/skipped",
            None,
            task_run::STATUS_SKIPPED,
        )
        .unwrap();

        let report = collect(&ctx, false).unwrap();

        let keys = report
            .tasks
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            ["failed", "latest-failed", "no-run", "prepared", "skipped"]
        );
        assert_eq!(report.hidden_task_count, 2);
        assert!(!report.full_inventory);

        let all_report = collect(&ctx, true).unwrap();

        let all_keys = all_report
            .tasks
            .iter()
            .map(|row| row.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            all_keys,
            [
                "failed",
                "latest-failed",
                "no-run",
                "passed",
                "prepared",
                "running",
                "skipped"
            ]
        );
        assert_eq!(all_report.hidden_task_count, 0);
        assert!(all_report.full_inventory);
    }
}
