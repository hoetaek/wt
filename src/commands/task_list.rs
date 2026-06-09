use crate::config::{ColumnConfig, Config};
use crate::context::Ctx;
use crate::origin_action_menu::{OriginActionMenu, OriginLabel};
use crate::origin_snapshot::{FieldSnapshot, OriginHealthSummary, read_task_snapshot};
use crate::services::issues::IssueListItem;
use crate::task::{self, TaskDocument};
use crate::task_run;
use anyhow::{Context, Result};
use console::measure_text_width;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;

const LIST_START: &str = "◆";
const BAR: &str = "│";
const FOOTER: &str = "└";
const BULLET: &str = "•";
const RUN_COLUMN_MAX: usize = 9;
const DUR_COLUMN_MAX: usize = 10;
const SOURCE_COLUMN_MAX: usize = 18;
const ORIGIN_STATUS_COLUMN_MAX: usize = 13;
const SIZE_COLUMN_MAX: usize = 12;
const TASK_COLUMN_MAX: usize = 80;
const NEXT_COLUMN_MAX: usize = 8;
const BRANCH_COLUMN_MAX: usize = 48;
const TASK_BROWSER_MIN: usize = 20;
const BRANCH_BROWSER_WIDTH: usize = 24;

pub(crate) fn run(ctx: &Ctx, all: bool) -> Result<()> {
    let report = collect(ctx, all)?;
    if should_open_browser(ctx) {
        if crate::tui::terminal_size_allows_task_browser() {
            return crate::tui::run_task_browser_with(ctx, browser_app(ctx, &report), || {
                let report = collect(ctx, all)?;
                Ok((browser_rows(&report), browser_diagnostics(&report)))
            });
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

pub(crate) fn browser_app(ctx: &Ctx, report: &TaskListReport) -> crate::tui::app::AppState {
    let rows = browser_rows(report);
    let columns = default_task_list_columns(&ctx.config);
    let browser_columns = browser_columns(&columns, &rows);
    crate::tui::app::AppState::task_with_columns(rows, browser_diagnostics(report), browser_columns)
}

pub(crate) fn normalize_origin_key(provider: &str, id: &str) -> (String, String) {
    (
        provider.trim().to_ascii_lowercase(),
        id.trim()
            .trim_start_matches('#')
            .trim()
            .to_ascii_lowercase(),
    )
}

pub(crate) fn local_origin_keys(ctx: &Ctx) -> Result<HashSet<(String, String)>> {
    let mut keys = HashSet::new();
    for path in task::task_document_paths(ctx)? {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = toml::from_str::<TaskDocument>(&content) else {
            continue;
        };
        if let Some(origin) = document.origin {
            keys.insert(normalize_origin_key(&origin.provider, &origin.id));
        }
    }
    Ok(keys)
}

pub(crate) fn local_task_keys(ctx: &Ctx) -> Result<HashSet<String>> {
    let mut keys = HashSet::new();
    for path in task::task_document_paths(ctx)? {
        if let Ok(key) = task::task_key_from_path(&path) {
            keys.insert(key);
        }
    }
    Ok(keys)
}

pub(crate) fn origin_only_rows(
    issues: Vec<IssueListItem>,
    local_origin_keys: &HashSet<(String, String)>,
    local_task_keys: &HashSet<String>,
    provider: &str,
) -> Vec<crate::tui::app::BrowserRow> {
    let provider = provider.trim().to_ascii_lowercase();
    issues
        .into_iter()
        .filter_map(|issue| {
            // 정규화 id는 매칭/중복 제거 전용. 표시·import dispatch에는 provider가
            // 돌려준 원본 대소문자를 보존한다 (Linear `WT-142` 등 case-sensitive id).
            let (_, normalized_id) = normalize_origin_key(&provider, &issue.identifier);
            let import_id = issue
                .identifier
                .trim()
                .trim_start_matches('#')
                .trim()
                .to_string();
            let import_key = task::safe_task_key(&issue.identifier);
            if local_origin_keys.contains(&(provider.clone(), normalized_id))
                || local_task_keys.contains(&import_key)
            {
                return None;
            }
            let key = format!("{provider}:{import_id}");
            let body = issue.hint.unwrap_or_default();
            Some(crate::tui::app::BrowserRow {
                key,
                title: issue.title,
                status: "origin-only".into(),
                run_status: "-".into(),
                origin_label: issue.display,
                next_action: "import".into(),
                duration: None,
                size: None,
                branch: None,
                source: "provider-origin".into(),
                preview_lines: origin_issue_preview_lines(&provider, &import_id, &body),
                body,
                menu: OriginActionMenu::for_origin_issue_placeholder("Provider issue"),
            })
        })
        .collect()
}

fn origin_issue_preview_lines(provider: &str, id: &str, hint: &str) -> Vec<String> {
    let mut lines = vec![
        format!("Origin      {provider} {id}"),
        "Source      provider-origin".into(),
        "Local path  not imported".into(),
        "Branch      not prepared".into(),
        "Next        import".into(),
    ];
    if !hint.trim().is_empty() {
        lines.push(format!("Hint        {}", one_line(hint)));
    }
    lines
}

#[derive(Debug, Serialize)]
struct TaskListRow {
    key: String,
    path: String,
    title: String,
    branch: Option<String>,
    origin: Option<TaskOriginSummary>,
    origin_health: OriginHealthSummary,
    run_status: String,
    duration: Option<String>,
    size: Option<String>,
    publish_state: String,
    source: String,
    body: String,
    body_summary: Option<String>,
    #[serde(skip_serializing)]
    display: task::TaskDocumentDisplay,
}

impl TaskListRow {
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
    let run_statuses = TaskRunStatusIndex::load(ctx)?;
    collect_with_task_run_statuses(ctx, all, &run_statuses)
}

fn collect_with_task_run_statuses(
    ctx: &Ctx,
    all: bool,
    run_statuses: &TaskRunStatusIndex,
) -> Result<TaskListReport> {
    let mut tasks = Vec::new();
    let mut invalid_tasks = Vec::new();
    let mut hidden_task_count = 0;

    for path in task::task_document_paths(ctx)? {
        let key = task_key_from_path(&path).unwrap_or_default();
        let relative_path = task_relative_path(ctx, &path);
        match read_task_row(ctx, &path, run_statuses) {
            Ok(row) => {
                if all || run_statuses.is_selectable(&row.key) {
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

#[derive(Debug, Clone, Default)]
struct TaskRunStatusIndex {
    latest_by_task: HashMap<String, IndexedTaskRunRecord>,
}

impl TaskRunStatusIndex {
    fn load(ctx: &Ctx) -> Result<Self> {
        Ok(Self::from_inventory(task_run::list_lossy(ctx)?))
    }

    fn from_inventory(inventory: task_run::TaskRunInventory) -> Self {
        let mut index = Self::default();
        for record in inventory.records {
            index.insert(
                record.run.task.clone(),
                IndexedTaskRunRecord {
                    order: TaskRunOrder::from_valid_record(&record),
                    status: IndexedTaskRunStatus::Known(record.run.status),
                },
            );
        }
        for record in inventory.invalid {
            if let Some((task, entry)) = invalid_task_run_index_entry(&record) {
                index.insert(task, entry);
            }
        }
        index
    }

    fn insert(&mut self, task: String, entry: IndexedTaskRunRecord) {
        if self
            .latest_by_task
            .get(&task)
            .is_none_or(|current| current.order <= entry.order)
        {
            self.latest_by_task.insert(task, entry);
        }
    }

    fn status_for(&self, key: &str) -> String {
        let task = task::safe_task_key(key);
        self.latest_by_task
            .get(&task)
            .map(|entry| match entry.status {
                IndexedTaskRunStatus::Known(status) => status.as_str().to_string(),
                IndexedTaskRunStatus::Unknown => "unknown".into(),
            })
            .unwrap_or_else(|| "new".into())
    }

    fn is_selectable(&self, key: &str) -> bool {
        let task = task::safe_task_key(key);
        match self.latest_by_task.get(&task).map(|entry| entry.status) {
            Some(IndexedTaskRunStatus::Known(status)) => status.is_task_selectable(),
            Some(IndexedTaskRunStatus::Unknown) => false,
            None => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedTaskRunRecord {
    order: TaskRunOrder,
    status: IndexedTaskRunStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexedTaskRunStatus {
    Known(task_run::TaskRunStatus),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TaskRunOrder {
    creation_order: Option<u64>,
    created_at: Option<String>,
    id: String,
}

impl TaskRunOrder {
    fn from_valid_record(record: &task_run::TaskRunRecord) -> Self {
        Self {
            creation_order: record.run.creation_order,
            created_at: task_run::normalized_utc_timestamp(&record.run.created_at),
            id: record.id.clone(),
        }
    }
}

fn read_task_row(ctx: &Ctx, path: &Path, run_statuses: &TaskRunStatusIndex) -> Result<TaskListRow> {
    let key = task_key_from_path(path)?;
    let relative_path = task_relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read task: {relative_path}"))?;
    let document: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {relative_path}"))?;
    let run_status = run_statuses.status_for(&key);
    task_row_with_run_status(ctx, key, relative_path, document, run_status)
}

#[cfg(test)]
fn task_row(ctx: &Ctx, key: String, path: String, document: TaskDocument) -> Result<TaskListRow> {
    let run_statuses = TaskRunStatusIndex::load(ctx)?;
    let run_status = run_statuses.status_for(&key);
    task_row_with_run_status(ctx, key, path, document, run_status)
}

fn task_row_with_run_status(
    ctx: &Ctx,
    key: String,
    path: String,
    document: TaskDocument,
    run_status: String,
) -> Result<TaskListRow> {
    let display = task::TaskDocumentDisplay::for_document(&key, &document);
    let origin = document.origin.as_ref().map(|origin| TaskOriginSummary {
        provider: origin.provider.clone(),
        id: origin.id.clone(),
    });
    let origin_health = task_origin_health(ctx, &key, &document);
    let duration = parse_planning_field(&document.body, "예상 소요", "expected duration");
    let size = parse_planning_field(&document.body, "크기", "size class");
    let body_summary = body_summary(&document.body);
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

    Ok(TaskListRow {
        key,
        path,
        title: document.title,
        branch: task::prepared_branch_name(&document.branch).map(str::to_string),
        origin,
        origin_health,
        run_status,
        duration,
        size,
        publish_state: publish_state.into(),
        source: source.into(),
        body: document.body,
        body_summary,
        display,
    })
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

fn invalid_task_run_index_entry(
    record: &task_run::InvalidTaskRunRecord,
) -> Option<(String, IndexedTaskRunRecord)> {
    let content = fs::read_to_string(&record.path).ok()?;
    let value = toml::from_str::<toml::Value>(&content).ok()?;
    let task = value
        .get("task")
        .and_then(toml::Value::as_str)
        .map(task::safe_task_key)?;
    let creation_order = value
        .get("creation_order")
        .and_then(toml::Value::as_integer)
        .and_then(|order| u64::try_from(order).ok());
    let created_at = value
        .get("created_at")
        .and_then(toml::Value::as_str)
        .and_then(task_run::normalized_utc_timestamp);
    Some((
        task,
        IndexedTaskRunRecord {
            order: TaskRunOrder {
                creation_order,
                created_at,
                id: record.id.clone(),
            },
            status: IndexedTaskRunStatus::Unknown,
        },
    ))
}

fn should_open_browser(ctx: &Ctx) -> bool {
    !ctx.is_json() && !ctx.quiet && ctx.ui.can_prompt() && std::io::stdout().is_terminal()
}

fn browser_row(row: &TaskListRow) -> crate::tui::app::BrowserRow {
    crate::tui::app::BrowserRow {
        key: row.key.clone(),
        title: row.display.label().to_string(),
        status: row.origin_health.status.clone(),
        run_status: row.run_status.clone(),
        origin_label: row.origin_health.origin_label.clone(),
        next_action: row.origin_health.next_action.clone(),
        duration: row.duration.clone(),
        size: row.size.clone(),
        branch: row.branch.clone(),
        source: row.source.clone(),
        body: row.body.clone(),
        preview_lines: browser_preview_lines(row),
        menu: row.origin_action_menu(),
    }
}

fn browser_preview_lines(row: &TaskListRow) -> Vec<String> {
    vec![
        format!("Local path  {}", row.path),
        format!("Run         {}", row.run_status),
        format!(
            "Duration    {}",
            row.duration.as_deref().unwrap_or("not set")
        ),
        format!("Size        {}", row.size.as_deref().unwrap_or("not set")),
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
        diagnostics.extend(report.invalid_tasks.iter().map(|invalid| {
            format!(
                "invalid task {}  file {}  {}",
                invalid.key,
                invalid.path,
                one_line(&invalid.error)
            )
        }));
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

fn parse_planning_field(body: &str, ko: &str, en: &str) -> Option<String> {
    let mut in_planning = false;
    let en = en.to_ascii_lowercase();

    for line in body.lines().map(str::trim) {
        if markdown_heading_text(line).is_some() {
            in_planning = markdown_heading_text(line).is_some_and(is_planning_heading);
            continue;
        }
        if !in_planning {
            continue;
        }

        let Some(item) = line.strip_prefix('-').map(str::trim) else {
            continue;
        };
        let Some((label, value)) = item.split_once(':') else {
            continue;
        };
        if !planning_label_matches(label.trim(), ko, &en) {
            continue;
        }

        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }

    None
}

fn markdown_heading_text(line: &str) -> Option<&str> {
    let heading = line.strip_prefix('#')?;
    Some(heading.trim_start_matches('#').trim())
}

fn is_planning_heading(heading: &str) -> bool {
    heading.contains("계획") || heading.to_ascii_lowercase().contains("planning")
}

fn planning_label_matches(label: &str, ko: &str, en: &str) -> bool {
    let normalized = label.to_ascii_lowercase();
    label.contains(ko) || normalized == en || normalized.contains(&format!("({en})"))
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

    let columns = default_task_list_columns(&ctx.config);
    for line in render_text_lines(report, &columns) {
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

fn render_text_lines(report: &TaskListReport, columns: &[Column]) -> Vec<String> {
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
        let widths = task_list_column_widths(&rows, columns);
        for row in rows {
            lines.push(format!(
                "{BAR}  {BULLET}  {}",
                task_inventory_label(row, columns, &widths)
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

#[derive(Debug, Clone)]
struct Column {
    title: String,
    hidden: bool,
    width: Option<u16>,
    grow: bool,
    kind: TaskListColumnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskListColumnKind {
    Run,
    Next,
    Dur,
    Task,
    Branch,
    Source,
    OriginStatus,
    Size,
}

fn default_task_list_columns(cfg: &Config) -> Vec<Column> {
    let mut columns = vec![
        task_list_column(
            TaskListColumnKind::Run,
            "run",
            false,
            false,
            cfg.task_list.columns.run,
        ),
        task_list_column(
            TaskListColumnKind::Next,
            "next",
            false,
            false,
            cfg.task_list.columns.next,
        ),
        task_list_column(
            TaskListColumnKind::Dur,
            "dur",
            false,
            false,
            cfg.task_list.columns.dur,
        ),
        task_list_column(
            TaskListColumnKind::Task,
            "task",
            false,
            true,
            cfg.task_list.columns.task,
        ),
        task_list_column(
            TaskListColumnKind::Branch,
            "branch",
            false,
            false,
            cfg.task_list.columns.branch,
        ),
        task_list_column(
            TaskListColumnKind::Source,
            "source",
            true,
            false,
            cfg.task_list.columns.source,
        ),
        task_list_column(
            TaskListColumnKind::OriginStatus,
            "origin_status",
            true,
            false,
            cfg.task_list.columns.origin_status,
        ),
        task_list_column(
            TaskListColumnKind::Size,
            "size",
            true,
            false,
            cfg.task_list.columns.size,
        ),
    ];

    if columns.iter().all(|column| column.hidden) {
        if let Some(task) = columns
            .iter_mut()
            .find(|column| column.kind == TaskListColumnKind::Task)
        {
            task.hidden = false;
        }
    }

    columns
}

fn task_list_column(
    kind: TaskListColumnKind,
    title: &str,
    default_hidden: bool,
    grow: bool,
    config: ColumnConfig,
) -> Column {
    Column {
        title: title.into(),
        hidden: config.hidden.unwrap_or(default_hidden),
        width: config.width,
        grow,
        kind,
    }
}

fn browser_columns(
    columns: &[Column],
    rows: &[crate::tui::app::BrowserRow],
) -> Vec<crate::tui::app::BrowserColumn> {
    let task_min_width = browser_task_column_min_width(rows);
    let task_content_width = browser_task_column_content_width(rows);
    columns
        .iter()
        .filter(|column| !column.hidden)
        .map(|column| {
            let cell = match column.kind {
                TaskListColumnKind::Run => crate::tui::app::BrowserCell::RunStatus,
                TaskListColumnKind::Next => crate::tui::app::BrowserCell::NextAction,
                TaskListColumnKind::Dur => crate::tui::app::BrowserCell::Duration,
                TaskListColumnKind::Task => crate::tui::app::BrowserCell::Task,
                TaskListColumnKind::Branch => crate::tui::app::BrowserCell::Branch,
                TaskListColumnKind::Source => crate::tui::app::BrowserCell::Source,
                TaskListColumnKind::OriginStatus => crate::tui::app::BrowserCell::OriginStatus,
                TaskListColumnKind::Size => crate::tui::app::BrowserCell::Size,
            };
            let width = column.width.unwrap_or_else(|| {
                if column.kind == TaskListColumnKind::Task {
                    task_content_width as u16
                } else {
                    browser_task_list_column_width(column.kind, column.grow, task_min_width) as u16
                }
            });
            if column.kind == TaskListColumnKind::Task && column.width.is_none() {
                crate::tui::app::BrowserColumn::length(&column.title, cell, width)
            } else if column.grow && column.width.is_none() {
                crate::tui::app::BrowserColumn::min(&column.title, cell, width)
            } else {
                crate::tui::app::BrowserColumn::length(&column.title, cell, width)
            }
        })
        .collect()
}

fn browser_task_column_content_width(rows: &[crate::tui::app::BrowserRow]) -> usize {
    rows.iter()
        .map(|row| measure_text_width(&format!("{}  task {}", row.title, row.key)))
        .max()
        .unwrap_or(0)
        .clamp(TASK_BROWSER_MIN, TASK_COLUMN_MAX)
}

fn browser_task_column_min_width(rows: &[crate::tui::app::BrowserRow]) -> usize {
    rows.iter()
        .map(|row| measure_text_width(&format!("task {}", row.key)))
        .max()
        .unwrap_or(0)
        .max(TASK_BROWSER_MIN)
}

fn browser_task_list_column_width(
    kind: TaskListColumnKind,
    grow: bool,
    task_width: usize,
) -> usize {
    match kind {
        TaskListColumnKind::Task if grow => task_width,
        TaskListColumnKind::Branch => {
            BRANCH_BROWSER_WIDTH.saturating_sub(task_width.saturating_sub(TASK_BROWSER_MIN))
        }
        _ => task_list_column_max(kind, grow),
    }
}

fn task_list_column_widths(rows: &[&TaskListRow], columns: &[Column]) -> Vec<usize> {
    columns
        .iter()
        .filter(|column| !column.hidden)
        .map(|column| {
            if let Some(width) = column.width {
                return width as usize;
            }
            let max_width = task_list_column_max(column.kind, column.grow);
            rows.iter()
                .map(|row| measure_text_width(&task_inventory_column(row, column.kind)))
                .max()
                .unwrap_or(0)
                .max(measure_text_width(&column.title))
                .min(max_width)
        })
        .collect()
}

fn task_inventory_label(row: &TaskListRow, columns: &[Column], widths: &[usize]) -> String {
    columns
        .iter()
        .filter(|column| !column.hidden)
        .zip(widths.iter().copied())
        .map(|(column, width)| {
            if column.kind == TaskListColumnKind::Task {
                pad_task_column(row, width)
            } else {
                pad_column(&task_inventory_column(row, column.kind), width)
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn task_inventory_column(row: &TaskListRow, kind: TaskListColumnKind) -> String {
    match kind {
        TaskListColumnKind::Run => row.run_status.clone(),
        TaskListColumnKind::Next => row.origin_health.next_action.clone(),
        TaskListColumnKind::Dur => row.duration.clone().unwrap_or_else(|| "-".into()),
        TaskListColumnKind::Task => format!("{}  task {}", row.display.label(), row.key),
        TaskListColumnKind::Branch => row
            .branch
            .as_ref()
            .map(|branch| format!("branch {branch}"))
            .unwrap_or_else(|| "not prepared".into()),
        TaskListColumnKind::Source => row.source.clone(),
        TaskListColumnKind::OriginStatus => row.origin_health.status.clone(),
        TaskListColumnKind::Size => row.size.clone().unwrap_or_else(|| "-".into()),
    }
}

fn task_list_column_max(kind: TaskListColumnKind, grow: bool) -> usize {
    if grow {
        return TASK_COLUMN_MAX;
    }

    match kind {
        TaskListColumnKind::Run => RUN_COLUMN_MAX,
        TaskListColumnKind::Next => NEXT_COLUMN_MAX,
        TaskListColumnKind::Dur => DUR_COLUMN_MAX,
        TaskListColumnKind::Task => TASK_COLUMN_MAX,
        TaskListColumnKind::Branch => BRANCH_COLUMN_MAX,
        TaskListColumnKind::Source => SOURCE_COLUMN_MAX,
        TaskListColumnKind::OriginStatus => ORIGIN_STATUS_COLUMN_MAX,
        TaskListColumnKind::Size => SIZE_COLUMN_MAX,
    }
}

fn pad_column(value: &str, width: usize) -> String {
    let value = truncate_display_width(value, width);
    let padding = width.saturating_sub(measure_text_width(&value));
    format!("{value}{}", " ".repeat(padding))
}

fn pad_task_column(row: &TaskListRow, width: usize) -> String {
    let key = format!("task {}", row.key);
    let key_width = measure_text_width(&key);
    if width <= key_width {
        return pad_column(&key, width);
    }

    let separator = "  ";
    let separator_width = measure_text_width(separator);
    let title_width = width.saturating_sub(key_width + separator_width);
    if title_width == 0 {
        return pad_column(&key, width);
    }

    let title = truncate_display_width(row.display.label(), title_width);
    pad_column(&format!("{title}{separator}{key}"), width)
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
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};
    use crate::origin_action_menu::OriginActionMenu;
    use crate::services::issues::IssueListItem;
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

    fn browser_row_fixture(key: &str, title: &str, source: &str) -> crate::tui::app::BrowserRow {
        crate::tui::app::BrowserRow {
            key: key.into(),
            title: title.into(),
            status: "local".into(),
            run_status: "new".into(),
            origin_label: String::new(),
            next_action: "publish".into(),
            duration: None,
            size: None,
            branch: Some("demo".into()),
            source: source.into(),
            body: String::new(),
            preview_lines: Vec::new(),
            menu: OriginActionMenu::for_local_task(key, title),
        }
    }

    #[test]
    fn normalize_origin_key_unifies_github_and_linear_forms() {
        assert_eq!(
            normalize_origin_key("github", "#182"),
            normalize_origin_key("github", "182")
        );
        assert_ne!(
            normalize_origin_key("github", "142"),
            normalize_origin_key("linear", "WT-142")
        );
        assert_eq!(
            normalize_origin_key("linear", "WT-142"),
            normalize_origin_key("linear", "wt-142")
        );
    }

    #[test]
    fn origin_only_rows_excludes_locally_linked_issues() {
        use std::collections::HashSet;

        let local: HashSet<(String, String)> = [normalize_origin_key("github", "#182")]
            .into_iter()
            .collect();
        let local_task_keys = HashSet::new();
        let issues = vec![
            IssueListItem {
                identifier: "175".into(),
                title: "A".into(),
                display: "github #175".into(),
                hint: None,
            },
            IssueListItem {
                identifier: "182".into(),
                title: "B".into(),
                display: "github #182".into(),
                hint: None,
            },
        ];

        let rows = origin_only_rows(issues, &local, &local_task_keys, "github");

        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec!["github:175"]);
        assert_eq!(rows[0].source, "provider-origin");
        assert!(rows[0].branch.is_none());
    }

    #[test]
    fn origin_only_rows_excludes_existing_local_task_import_keys() {
        use std::collections::HashSet;

        let local_origin_keys = HashSet::new();
        let local_task_keys: HashSet<String> = ["PROJ-123".to_string()].into_iter().collect();
        let issues = vec![
            IssueListItem {
                identifier: "PROJ-123".into(),
                title: "Already local".into(),
                display: "linear PROJ-123".into(),
                hint: None,
            },
            IssueListItem {
                identifier: "PROJ-124".into(),
                title: "Provider only".into(),
                display: "linear PROJ-124".into(),
                hint: None,
            },
        ];

        let rows = origin_only_rows(issues, &local_origin_keys, &local_task_keys, "linear");

        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec!["linear:PROJ-124"]);
    }

    #[test]
    fn origin_only_rows_preserves_identifier_case_in_key() {
        use std::collections::HashSet;

        let local_origin_keys: HashSet<(String, String)> =
            [normalize_origin_key("linear", "WT-142")]
                .into_iter()
                .collect();
        let local_task_keys = HashSet::new();
        let issues = vec![
            IssueListItem {
                identifier: "WT-142".into(),
                title: "Already imported".into(),
                display: "linear WT-142".into(),
                hint: None,
            },
            IssueListItem {
                identifier: "WT-143".into(),
                title: "Provider only".into(),
                display: "linear WT-143".into(),
                hint: None,
            },
        ];

        let rows = origin_only_rows(issues, &local_origin_keys, &local_task_keys, "linear");

        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["linear:WT-143"],
            "중복 제거는 case-insensitive로 유지하되 key는 원본 대소문자를 보존해야 한다"
        );
    }

    #[test]
    fn local_task_keys_collects_task_document_keys_without_origin() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("PROJ-123.toml"),
            r#"title = "Already local"
branch = "proj-123"
body = "Task body"
"#,
        )
        .unwrap();

        let keys = local_task_keys(&ctx).unwrap();

        assert!(keys.contains("PROJ-123"));
    }

    #[test]
    fn local_origin_keys_skips_malformed_tasks_and_keeps_valid_origins() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("valid.toml"),
            r##"title = "Valid"
branch = "valid"
body = "Task body"

[origin]
provider = "github"
id = "#182"
"##,
        )
        .unwrap();
        fs::write(tasks_dir.join("bad.toml"), "unknown = true\n").unwrap();

        let keys = local_origin_keys(&ctx).unwrap();

        assert!(keys.contains(&normalize_origin_key("github", "182")));
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
    fn task_row_carries_run_status_duration_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("demo.toml"),
            r#"title = "Demo"
branch = "demo"
body = '''
## 계획 (Planning)

- 유형 (type): AFK
- 예상 소요 (expected duration): 2h
- 크기 (size class): medium

## 작업

- Keep the full body for later browser views.
'''
"#,
        )
        .unwrap();

        let report = collect(&ctx, false).unwrap();
        let row = report.tasks.iter().find(|row| row.key == "demo").unwrap();

        assert_eq!(row.run_status, "new");
        assert_eq!(row.duration.as_deref(), Some("2h"));
        assert_eq!(row.size.as_deref(), Some("medium"));
        assert!(row.body.contains("## 계획"));

        let browser_rows = browser_rows(&report);
        assert_eq!(browser_rows[0].run_status, "new");
        assert_eq!(browser_rows[0].duration.as_deref(), Some("2h"));
        assert_eq!(browser_rows[0].size.as_deref(), Some("medium"));
        assert!(browser_rows[0].body.contains("full body"));
    }

    #[test]
    fn duration_is_none_when_planning_absent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("provider.toml"),
            r#"title = "Provider"
branch = "provider"
body = "Imported provider body without a planning section."
"#,
        )
        .unwrap();

        let report = collect(&ctx, false).unwrap();

        assert_eq!(report.tasks[0].duration, None);
        assert_eq!(report.tasks[0].size, None);
    }

    #[test]
    fn invariant_i1_lossy_malformed_task_run_does_not_drop_task_document() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let task_runs_dir = dir.path().join(".wt/execution/task-runs");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(
            tasks_dir.join("demo.toml"),
            r#"title = "Demo"
branch = "demo"
body = "Task body"
"#,
        )
        .unwrap();
        fs::write(
            task_runs_dir.join("run-broken.toml"),
            r#"task = "demo"
branch = "demo"
status = "started"
created_at = "2026-05-18T00:00:00Z"
updated_at = "2026-05-18T00:00:00Z"
"#,
        )
        .unwrap();

        let report = collect(&ctx, true).unwrap();

        assert_eq!(report.tasks.len(), 1);
        assert_eq!(report.tasks[0].key, "demo");
        assert_eq!(report.tasks[0].run_status, "unknown");
        assert!(report.invalid_tasks.is_empty());
    }

    #[test]
    fn invariant_i5_all_inventory_includes_unknown_runs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let task_runs_dir = dir.path().join(".wt/execution/task-runs");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(
            tasks_dir.join("demo.toml"),
            r#"title = "Demo"
branch = "demo"
body = "Task body"
"#,
        )
        .unwrap();
        fs::write(
            task_runs_dir.join("run-broken.toml"),
            r#"task = "demo"
branch = "demo"
status = "started"
created_at = "2026-05-18T00:00:00Z"
updated_at = "2026-05-18T00:00:00Z"
"#,
        )
        .unwrap();

        let report = collect(&ctx, false).unwrap();

        assert!(report.tasks.is_empty());
        assert_eq!(report.hidden_task_count, 1);

        let all_report = collect(&ctx, true).unwrap();
        assert_eq!(all_report.tasks.len(), 1);
        assert_eq!(all_report.tasks[0].key, "demo");
        assert_eq!(all_report.tasks[0].run_status, "unknown");
    }

    #[test]
    fn invariant_i4_latest_malformed_masks_older_valid_run() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let task_runs_dir = dir.path().join(".wt/execution/task-runs");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(
            tasks_dir.join("demo.toml"),
            r#"title = "Demo"
branch = "demo"
body = "Task body"
"#,
        )
        .unwrap();
        task_run::create(&ctx, "demo", "demo", None, task_run::STATUS_FAILED).unwrap();
        fs::write(
            task_runs_dir.join("run-newer-broken.toml"),
            r#"task = "demo"
branch = "demo"
status = "started"
creation_order = 99
created_at = "2026-05-18T00:00:01Z"
updated_at = "2026-05-18T00:00:01Z"
"#,
        )
        .unwrap();

        let report = collect(&ctx, false).unwrap();

        assert!(report.tasks.is_empty());
        assert_eq!(report.hidden_task_count, 1);

        let all_report = collect(&ctx, true).unwrap();
        assert_eq!(all_report.tasks.len(), 1);
        assert_eq!(all_report.tasks[0].key, "demo");
        assert_eq!(all_report.tasks[0].run_status, "unknown");
    }

    #[test]
    fn invariant_i7_source_column_uses_same_value_for_text_and_browser_rows() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let row = task_row(
            &ctx,
            "origin-sync-tui".into(),
            "<repo-root>/.wt/execution/tasks/origin-sync-tui.toml".into(),
            TaskDocument {
                title: "Origin sync TUI".into(),
                branch: "origin-sync-tui".into(),
                body: "local body".into(),
                origin: Some(task::TaskOrigin {
                    provider: "linear".into(),
                    id: "WT-142".into(),
                }),
            },
        )
        .unwrap();

        assert_eq!(
            task_inventory_column(&row, TaskListColumnKind::Source),
            browser_row(&row).source
        );
    }

    #[test]
    fn unrelated_malformed_task_run_leaves_new_run_status() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let task_runs_dir = dir.path().join(".wt/execution/task-runs");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(
            tasks_dir.join("demo.toml"),
            r#"title = "Demo"
branch = "demo"
body = "Task body"
"#,
        )
        .unwrap();
        fs::write(
            task_runs_dir.join("run-broken.toml"),
            r#"task = "other"
branch = "other"
status = "started"
created_at = "2026-05-18T00:00:00Z"
updated_at = "2026-05-18T00:00:00Z"
"#,
        )
        .unwrap();

        let report = collect(&ctx, true).unwrap();

        assert_eq!(report.tasks.len(), 1);
        assert_eq!(report.tasks[0].key, "demo");
        assert_eq!(report.tasks[0].run_status, "new");
        assert!(report.invalid_tasks.is_empty());
    }

    #[test]
    fn invariant_i2_collect_reuses_single_task_run_status_index() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let task_runs_dir = dir.path().join(".wt/execution/task-runs");
        fs::create_dir_all(&tasks_dir).unwrap();

        for idx in 1..=6 {
            fs::write(
                tasks_dir.join(format!("task-{idx}.toml")),
                format!(
                    r#"title = "Task {idx}"
branch = "task-{idx}"
body = "Task body"
"#
                ),
            )
            .unwrap();
            task_run::create(
                &ctx,
                &format!("task-{idx}"),
                &format!("task-{idx}"),
                None,
                task_run::STATUS_PREPARED,
            )
            .unwrap();
        }
        task_run::create(&ctx, "task-3", "task-3", None, task_run::STATUS_RUNNING).unwrap();

        let run_statuses = TaskRunStatusIndex::from_inventory(task_run::list_lossy(&ctx).unwrap());
        fs::rename(
            &task_runs_dir,
            dir.path().join(".wt/execution/task-runs.removed"),
        )
        .unwrap();

        let report = collect_with_task_run_statuses(&ctx, true, &run_statuses).unwrap();

        assert_eq!(report.tasks.len(), 6);
        assert_eq!(
            report
                .tasks
                .iter()
                .find(|row| row.key == "task-3")
                .unwrap()
                .run_status,
            "running"
        );
    }

    #[test]
    fn invariant_i6_text_task_column_preserves_key_when_title_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let row = task_row(
            &ctx,
            "stable-key".into(),
            "<repo-root>/.wt/execution/tasks/stable-key.toml".into(),
            TaskDocument {
                title: "This title is intentionally much longer than the configured task width"
                    .into(),
                branch: "stable-key".into(),
                body: String::new(),
                origin: None,
            },
        )
        .unwrap();
        let columns = vec![Column {
            title: "task".into(),
            hidden: false,
            width: Some(24),
            grow: true,
            kind: TaskListColumnKind::Task,
        }];
        let label = task_inventory_label(&row, &columns, &[24]);

        assert!(
            label.contains("task stable-key"),
            "task key should remain visible in `{label}`"
        );
    }

    #[test]
    fn default_columns_show_run_dur_task_branch_hide_size() {
        let cols = default_task_list_columns(&Config::default());
        let visible = cols
            .iter()
            .filter(|column| !column.hidden)
            .map(|column| column.title.as_str())
            .collect::<Vec<_>>();

        assert!(visible.contains(&"run"));
        assert!(visible.contains(&"dur"));
        assert!(visible.contains(&"task"));
        assert!(visible.contains(&"branch"));
        assert!(!visible.contains(&"size"));
    }

    #[test]
    fn task_column_is_content_capped_not_leftover_fill() {
        let rows = vec![browser_row_fixture("a", "Short", "demo")];
        let task_columns = default_task_list_columns(&Config::default())
            .into_iter()
            .filter(|column| column.title == "task")
            .collect::<Vec<_>>();

        let cols = browser_columns(&task_columns, &rows);
        let task = cols.iter().find(|column| column.title == "task").unwrap();

        assert!(
            matches!(task.width, crate::tui::app::BrowserColumnWidth::Length(_)),
            "Task column should be content-width Length, not leftover-fill Min"
        );
    }

    #[test]
    fn config_can_hide_run_column_and_set_width() {
        let mut cfg = Config::default();
        cfg.task_list.columns.run = ColumnConfig {
            hidden: Some(true),
            width: Some(7),
        };

        let cols = default_task_list_columns(&cfg);
        let run = cols.iter().find(|column| column.title == "run").unwrap();

        assert!(run.hidden);
        assert_eq!(run.width, Some(7));
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
        )
        .unwrap();

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

    #[test]
    fn browser_rows_attach_action_menu_from_row_model() {
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

        assert!(rows[0].menu.enabled("Diff with issue"));
        assert!(rows[0].menu.disabled_reason("Publish as issue").is_some());
    }

    fn browser_report_text(ctx: &Ctx, report: &TaskListReport) -> String {
        browser_report_text_with_size(ctx, report, 100, 18)
    }

    fn browser_report_text_with_size(
        ctx: &Ctx,
        report: &TaskListReport,
        width: u16,
        height: u16,
    ) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let mut app = browser_app(ctx, report);
        terminal
            .draw(|frame| crate::tui::render::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn browser_default_columns_fit_narrow_width() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("demo.toml"),
            r#"title = "Demo task"
branch = "feature/demo"
body = '''
## 계획 (Planning)

- 예상 소요 (expected duration): 2h
'''
"#,
        )
        .unwrap();

        let report = collect(&ctx, true).unwrap();
        let text = browser_report_text_with_size(&ctx, &report, 80, 18);

        assert!(
            text.lines().any(|line| {
                line.contains("run")
                    && line.contains("next")
                    && line.contains("dur")
                    && line.contains("task")
                    && line.contains("branch")
            }),
            "default browser columns should remain visible at narrow width:\n{text}"
        );
    }

    #[test]
    fn browser_default_columns_preserve_long_key_at_narrow_width() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("task-list-row-columns.toml"),
            r#"title = "Long key layout"
branch = "task-list-row-columns"
body = '''
## 계획 (Planning)

- 예상 소요 (expected duration): 2h
'''
"#,
        )
        .unwrap();

        let report = collect(&ctx, true).unwrap();
        let text = browser_report_text_with_size(&ctx, &report, 80, 18);

        assert!(
            text.contains("task task-list-row-columns"),
            "default browser task column should preserve long keys:\n{text}"
        );
        assert!(
            text.lines()
                .any(|line| line.contains("run") && line.contains("next") && line.contains("dur")),
            "run/next/dur columns should remain visible with long keys:\n{text}"
        );
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
        let text = browser_report_text(&ctx, &report);

        assert!(text.contains("1 invalid task file"));
        assert!(text.contains("<repo-root>/.wt/execution/tasks/bad.toml"));
        assert!(text.contains("Failed to parse task"));
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
        let text = browser_report_text(&ctx, &report);

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
    fn invariant_i3_default_inventory_matches_task_selector_statuses() {
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
