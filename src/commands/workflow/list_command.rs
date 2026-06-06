use crate::context::Ctx;
use crate::origin_snapshot::{
    FieldSnapshot, OriginHealthSummary, origin_label, read_workflow_snapshot,
};
use crate::task as task_store;
use crate::task_run::{
    STATUS_FAILED, STATUS_PASSED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::workflow as workflow_store;
use crate::workflow::planner::runnable_workflow_info;
use crate::workflow::render::{
    workflow_body_summary, workflow_relative_path, workflow_title_label,
};
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states, task_run_record,
};
use crate::workflow::{WorkflowMetadata, WorkflowMode};
use anyhow::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

const LIST_START: &str = "◆";
const BAR: &str = "│";
const FOOTER: &str = "└";
const BULLET: &str = "•";

pub(super) fn run(ctx: &Ctx) -> Result<()> {
    let report = collect(ctx)?;
    if ctx.is_json() {
        write_json(&report)?;
    } else {
        print_text(ctx, &report);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct WorkflowListReport {
    workflows: Vec<WorkflowListRow>,
    invalid_workflows: Vec<InvalidWorkflowRow>,
}

#[derive(Debug, Serialize)]
struct WorkflowListRow {
    id: String,
    path: String,
    mode: String,
    title: String,
    body: Option<String>,
    body_summary: Option<String>,
    origin: Option<WorkflowOriginSummary>,
    origin_health: OriginHealthSummary,
    child_origins: Vec<ChildOriginSummary>,
    task_count: usize,
    task_runs: TaskRunSummary,
    runnable: RunnableMetadata,
    base_mode: String,
    base: Option<String>,
    profile: Option<String>,
    profiles: Vec<String>,
    policy: WorkflowPolicySummary,
    updated_at: String,
    state_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowOriginSummary {
    provider: String,
    id: String,
}

#[derive(Debug, Serialize)]
struct ChildOriginSummary {
    task: String,
    origin_label: String,
    provider: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskRunSummary {
    total: usize,
    prepared: usize,
    running: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    missing: usize,
    summary: String,
}

#[derive(Debug, Serialize)]
struct RunnableMetadata {
    runnable: bool,
    runnable_count: usize,
    next_task: Option<String>,
    reason: String,
}

#[derive(Debug, Serialize)]
struct WorkflowPolicySummary {
    pull_request: String,
    landing: String,
    review_codex_base: String,
}

#[derive(Debug, Serialize)]
struct InvalidWorkflowRow {
    id: String,
    path: String,
    error: String,
}

#[derive(Debug)]
pub(crate) struct ActiveWorkflowInventoryIssue {
    pub id: String,
    pub path: String,
    pub error: String,
}

pub(crate) fn active_inventory_issues(ctx: &Ctx) -> Result<Vec<ActiveWorkflowInventoryIssue>> {
    let mut issues = Vec::new();

    for path in workflow_store::workflow_paths(ctx)? {
        let id = workflow_store::id_from_path(&path)?;
        match workflow_store::read(&path) {
            Ok(metadata) => {
                if let Err(err) = read_workflow_states(ctx, &path, &metadata) {
                    issues.push(ActiveWorkflowInventoryIssue {
                        id,
                        path: workflow_relative_path(ctx, &path),
                        error: format!("{err:#}"),
                    });
                }
            }
            Err(err) => issues.push(ActiveWorkflowInventoryIssue {
                id,
                path: workflow_relative_path(ctx, &path),
                error: format!("{err:#}"),
            }),
        }
    }

    Ok(issues)
}

fn collect(ctx: &Ctx) -> Result<WorkflowListReport> {
    let mut workflows = Vec::new();
    let mut invalid_workflows = Vec::new();

    for path in workflow_store::workflow_paths(ctx)? {
        let id = workflow_store::id_from_path(&path)?;
        match workflow_store::read(&path) {
            Ok(metadata) => workflows.push(workflow_row(ctx, &path, id, metadata)),
            Err(err) => invalid_workflows.push(InvalidWorkflowRow {
                id,
                path: workflow_relative_path(ctx, &path),
                error: format!("{err:#}"),
            }),
        }
    }

    Ok(WorkflowListReport {
        workflows,
        invalid_workflows,
    })
}

fn workflow_row(ctx: &Ctx, path: &Path, id: String, metadata: WorkflowMetadata) -> WorkflowListRow {
    let task_runs = task_run_summary(ctx, &metadata);
    let title = workflow_title_label(ctx, &id, &metadata);
    let body = metadata.body.clone();
    let body_summary = workflow_body_summary(&metadata);
    let origin = metadata
        .origin
        .as_ref()
        .map(|origin| WorkflowOriginSummary {
            provider: origin.provider.clone(),
            id: origin.id.clone(),
        });
    let origin_health = workflow_origin_health(ctx, &id, &metadata);
    let child_origins = child_origin_summaries(ctx, &metadata);
    let (runnable, state_error) = match read_workflow_states(ctx, path, &metadata) {
        Ok(states) => (runnable_metadata(&metadata.mode, &states), None),
        Err(err) => (
            RunnableMetadata {
                runnable: false,
                runnable_count: 0,
                next_task: None,
                reason: "state_unavailable".into(),
            },
            Some(format!("{err:#}")),
        ),
    };

    WorkflowListRow {
        id,
        path: workflow_relative_path(ctx, path),
        mode: metadata.mode.as_str().into(),
        title,
        body,
        body_summary,
        origin,
        origin_health,
        child_origins,
        task_count: metadata.tasks.len(),
        task_runs,
        runnable,
        base_mode: metadata.base_mode,
        base: metadata.base,
        profile: metadata.profile,
        profiles: metadata.profiles,
        policy: WorkflowPolicySummary {
            pull_request: metadata.policy.pull_request.as_str().into(),
            landing: metadata.policy.landing.as_str().into(),
            review_codex_base: metadata.policy.review.codex_base.as_str().into(),
        },
        updated_at: metadata.updated_at,
        state_error,
    }
}

fn workflow_origin_health(ctx: &Ctx, id: &str, metadata: &WorkflowMetadata) -> OriginHealthSummary {
    let Some(origin) = metadata.origin.as_ref() else {
        return OriginHealthSummary::workflow_without_origin();
    };
    let local_fields = FieldSnapshot::new(
        metadata.title.clone().unwrap_or_default(),
        metadata.body.clone().unwrap_or_default(),
    );
    match read_workflow_snapshot(&ctx.storage_root, id) {
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

fn child_origin_summaries(ctx: &Ctx, metadata: &WorkflowMetadata) -> Vec<ChildOriginSummary> {
    metadata
        .tasks
        .iter()
        .map(|row| child_origin_summary(ctx, &row.task))
        .collect()
}

fn child_origin_summary(ctx: &Ctx, task: &str) -> ChildOriginSummary {
    match task_store::read_task_document(ctx, task) {
        Ok(document) => match document.origin {
            Some(origin) => ChildOriginSummary {
                task: task.to_string(),
                origin_label: origin_label(&origin.provider, &origin.id),
                provider: Some(origin.provider),
                id: Some(origin.id),
            },
            None => ChildOriginSummary {
                task: task.to_string(),
                origin_label: "not published".into(),
                provider: None,
                id: None,
            },
        },
        Err(_) => ChildOriginSummary {
            task: task.to_string(),
            origin_label: "unknown".into(),
            provider: None,
            id: None,
        },
    }
}

fn read_workflow_states(
    ctx: &Ctx,
    path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    match metadata.mode {
        WorkflowMode::Single => read_single_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Batch => read_batch_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Stack => read_stack_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Matrix => read_matrix_workflow_task_states(ctx, path, metadata),
    }
}

fn task_run_summary(ctx: &Ctx, metadata: &WorkflowMetadata) -> TaskRunSummary {
    let mut summary = TaskRunSummary {
        total: workflow_run_count(metadata),
        prepared: 0,
        running: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
        missing: 0,
        summary: String::new(),
    };

    for run_id in workflow_run_ids(metadata) {
        match task_run_record(ctx, run_id).map(|run| run.status) {
            Some(STATUS_PREPARED) => summary.prepared += 1,
            Some(STATUS_RUNNING) => summary.running += 1,
            Some(STATUS_PASSED) => summary.passed += 1,
            Some(STATUS_FAILED) => summary.failed += 1,
            Some(STATUS_SKIPPED) => summary.skipped += 1,
            None => summary.missing += 1,
        }
    }

    summary.summary = task_run_summary_text(&summary);
    summary
}

fn workflow_run_count(metadata: &WorkflowMetadata) -> usize {
    if matches!(metadata.mode, WorkflowMode::Matrix) {
        metadata.tasks.iter().map(|row| row.runs.len()).sum()
    } else {
        metadata.tasks.len()
    }
}

fn workflow_run_ids(metadata: &WorkflowMetadata) -> Vec<&str> {
    if matches!(metadata.mode, WorkflowMode::Matrix) {
        return metadata
            .tasks
            .iter()
            .flat_map(|row| row.runs.iter().map(|run| run.run.as_str()))
            .collect();
    }
    metadata.tasks.iter().map(|row| row.run.as_str()).collect()
}

fn task_run_summary_text(summary: &TaskRunSummary) -> String {
    let parts = [
        ("prepared", summary.prepared),
        ("running", summary.running),
        ("passed", summary.passed),
        ("failed", summary.failed),
        ("skipped", summary.skipped),
        ("missing", summary.missing),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(name, count)| format!("{count} {name}"))
    .collect::<Vec<_>>();

    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(" / ")
    }
}

fn runnable_metadata(mode: &WorkflowMode, states: &[WorkflowTaskState]) -> RunnableMetadata {
    if let Some(info) = runnable_workflow_info(mode, states) {
        return RunnableMetadata {
            runnable: true,
            runnable_count: info.runnable_count,
            next_task: info
                .next_idx
                .and_then(|idx| states.get(idx))
                .map(|state| state.row.task.clone()),
            reason: match mode {
                WorkflowMode::Single => "single_all_task_runs_runnable",
                WorkflowMode::Batch => "batch_has_runnable_task_runs",
                WorkflowMode::Stack => "stack_next_task_run_runnable",
                WorkflowMode::Matrix => "matrix_has_runnable_profile_runs",
            }
            .into(),
        };
    }

    RunnableMetadata {
        runnable: false,
        runnable_count: 0,
        next_task: None,
        reason: non_runnable_reason(mode, states).into(),
    }
}

fn non_runnable_reason(mode: &WorkflowMode, states: &[WorkflowTaskState]) -> &'static str {
    match mode {
        WorkflowMode::Single => "single_requires_all_task_runs_prepared_or_failed",
        WorkflowMode::Batch => "batch_has_no_prepared_or_failed_task_runs",
        WorkflowMode::Matrix => "matrix_has_no_prepared_or_failed_profile_runs",
        WorkflowMode::Stack
            if states
                .iter()
                .any(|state| state.run.status.is_stack_completable()) =>
        {
            "stack_has_running_task_run"
        }
        WorkflowMode::Stack => "stack_has_no_next_prepared_or_failed_task_run",
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

fn print_text(ctx: &Ctx, report: &WorkflowListReport) {
    if report.workflows.is_empty() && report.invalid_workflows.is_empty() {
        ctx.ui.print_plain(&format!(
            "No workflows found in {}",
            ctx.storage_root
                .display_path(&ctx.storage_root.workflows_dir())
        ));
        return;
    }

    for line in render_text_lines(report) {
        ctx.ui.print_plain(&line);
    }

    if !report.invalid_workflows.is_empty() {
        for invalid in &report.invalid_workflows {
            ctx.ui.print_warning(&format!(
                "{}  file {}  {}",
                invalid.id,
                invalid.path,
                invalid_workflow_error_summary(&invalid.error)
            ));
        }
    }
}

fn render_text_lines(report: &WorkflowListReport) -> Vec<String> {
    let mut lines = vec![format!("{LIST_START} Workflows"), BAR.to_string()];
    let mut emitted_group = false;
    for group in [
        WorkflowDisplayGroup::Runnable,
        WorkflowDisplayGroup::Waiting,
        WorkflowDisplayGroup::Passed,
    ] {
        let rows = report
            .workflows
            .iter()
            .filter(|row| display_group(row) == group)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            continue;
        }

        if emitted_group {
            lines.push(BAR.to_string());
        }
        lines.push(format!("{BAR} {}", group.label()));
        emitted_group = true;
        for row in rows {
            lines.push(format!(
                "{BAR}  {BULLET}  {}",
                workflow_row_summary(row, group)
            ));
            if !row.child_origins.is_empty() {
                lines.push(format!(
                    "{BAR}     child origins {}",
                    child_origin_detail(row)
                ));
            }
        }
    }

    if !report.invalid_workflows.is_empty() {
        if emitted_group {
            lines.push(BAR.to_string());
        }
        lines.push(format!("{BAR} invalid"));
        for invalid in &report.invalid_workflows {
            lines.push(format!(
                "{BAR}  {BULLET}  {}  file {}",
                invalid.id, invalid.path
            ));
        }
    }

    lines.push(FOOTER.to_string());
    lines
}

fn invalid_workflow_error_summary(error: &str) -> String {
    if error.contains("Workflow uses removed `objective`") {
        "uses removed `objective`; edit the workflow file to use top-level `title`, `body`, and optional `[origin]`".into()
    } else {
        one_line(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkflowDisplayGroup {
    Runnable,
    Waiting,
    Passed,
}

impl WorkflowDisplayGroup {
    fn label(self) -> &'static str {
        match self {
            Self::Runnable => "runnable",
            Self::Waiting => "waiting",
            Self::Passed => "passed",
        }
    }
}

fn display_group(row: &WorkflowListRow) -> WorkflowDisplayGroup {
    if row.runnable.runnable {
        return WorkflowDisplayGroup::Runnable;
    }

    let terminal = row.task_runs.passed + row.task_runs.skipped;
    if row.state_error.is_none() && row.task_runs.total > 0 && terminal == row.task_runs.total {
        WorkflowDisplayGroup::Passed
    } else {
        WorkflowDisplayGroup::Waiting
    }
}

fn workflow_row_summary(row: &WorkflowListRow, group: WorkflowDisplayGroup) -> String {
    let mut parts = vec![
        format!("status {}", row.origin_health.status),
        format!("origin {}", row.origin_health.origin_label),
        format!("id {}", row.id),
        format!("mode {}", row.mode),
        format!("runs {}", task_run_row_summary(&row.task_runs)),
    ];

    if let Some(profile) = row_profile_preview(row) {
        parts.push(profile);
    }
    if let Some(action) = workflow_action_detail(row, group) {
        parts.push(action);
    }
    parts.push(format!(
        "policy {}/{} review:{}",
        row.policy.pull_request, row.policy.landing, row.policy.review_codex_base
    ));

    format!("{}  {}", row.title, parts.join(" · "))
}

fn child_origin_detail(row: &WorkflowListRow) -> String {
    row.child_origins
        .iter()
        .map(|origin| format!("{} {}", origin.task, origin.origin_label))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn task_run_row_summary(summary: &TaskRunSummary) -> String {
    let counts = [
        ("prepared", summary.prepared),
        ("running", summary.running),
        ("passed", summary.passed),
        ("failed", summary.failed),
        ("skipped", summary.skipped),
        ("missing", summary.missing),
    ];
    let non_zero = counts
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect::<Vec<_>>();

    match non_zero.as_slice() {
        [] => "0 none".into(),
        [(status, count)] if *count == summary.total => format!("{} {status}", summary.total),
        _ => format!("{} mixed ({})", summary.total, summary.summary),
    }
}

fn workflow_action_detail(row: &WorkflowListRow, group: WorkflowDisplayGroup) -> Option<String> {
    match group {
        WorkflowDisplayGroup::Runnable => {
            if let Some(task) = row.runnable.next_task.as_deref() {
                Some(format!("next {}", truncate_chars(task, 48)))
            } else if row.runnable.runnable_count > 0
                && row.runnable.runnable_count < row.task_runs.total
            {
                Some(format!("runnable {}", row.runnable.runnable_count))
            } else {
                None
            }
        }
        WorkflowDisplayGroup::Waiting => Some(format!(
            "reason {}",
            human_non_runnable_reason(&row.runnable.reason)
        )),
        WorkflowDisplayGroup::Passed => None,
    }
}

fn human_non_runnable_reason(reason: &str) -> &'static str {
    match reason {
        "single_requires_all_task_runs_prepared_or_failed" => {
            "waiting for all task runs to be prepared or failed"
        }
        "batch_has_no_prepared_or_failed_task_runs" => "waiting for a prepared or failed task run",
        "matrix_has_no_prepared_or_failed_profile_runs" => {
            "waiting for a prepared or failed profile run"
        }
        "stack_has_running_task_run" => "waiting for running task",
        "stack_has_no_next_prepared_or_failed_task_run" => {
            "waiting for next prepared or failed task"
        }
        "state_unavailable" => "workflow state unavailable",
        _ => "not currently runnable",
    }
}

fn row_profile_preview(row: &WorkflowListRow) -> Option<String> {
    if !row.profiles.is_empty() {
        Some(format!("profiles {}", row.profiles.len()))
    } else {
        row.profile
            .as_deref()
            .filter(|profile| !profile.trim().is_empty())
            .map(|profile| format!("profile {}", truncate_chars(profile, 24)))
    }
}

fn write_json(report: &WorkflowListReport) -> Result<()> {
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
    use std::fs;
    use std::sync::Arc;

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

    fn ctx_with_ui(root: &Path, output_mode: OutputMode) -> (Ctx, Arc<MockUi>) {
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
            CtxOptions {
                output_mode,
                ..CtxOptions::default()
            },
        );
        (ctx, ui)
    }

    fn workflow_toml_with_one_task_origin(title: &str, origin_id: &str, task: &str) -> String {
        format!(
            r#"title = "{title}"
body = "workflow body"
mode = "batch"
base_mode = "default"
created_at = "2026-06-06T00:00:00Z"
updated_at = "2026-06-06T00:00:00Z"

[origin]
provider = "linear"
id = "{origin_id}"

[policy]
pull_request = "none"
landing = "manual"

[policy.review]
codex_base = "none"

[[tasks]]
task = "{task}"
run = "run-{task}"
"#
        )
    }

    #[test]
    fn collect_lists_valid_workflows_and_reports_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        write_task(dir.path(), "schema", "feature/schema");
        write_task_run(
            dir.path(),
            "run-2026-05-18-001-schema",
            "schema",
            "feature/schema",
            "prepared",
            "2026-05-18-001",
        );
        write_workflow(
            dir.path(),
            "2026-05-18-001",
            "batch",
            r#"title = "Ship search"
body = "Coordinate the search workflow without making list rows too long."
origin = { provider = "linear", id = "WT-123" }
profile = "codex"
"#,
            r#"[[tasks]]
task = "schema"
run = "run-2026-05-18-001-schema"
"#,
        );
        fs::write(
            dir.path().join(".wt/execution/workflows/bad.toml"),
            "mode = \"batch\"\n",
        )
        .unwrap();

        let report = collect(&ctx).unwrap();

        assert_eq!(report.workflows.len(), 1);
        assert_eq!(report.invalid_workflows.len(), 1);
        let row = &report.workflows[0];
        assert_eq!(row.id, "2026-05-18-001");
        assert_eq!(row.mode, "batch");
        assert_eq!(row.title, "Ship search");
        assert_eq!(
            row.body.as_deref(),
            Some("Coordinate the search workflow without making list rows too long.")
        );
        assert_eq!(
            row.body_summary.as_deref(),
            Some("Coordinate the search workflow without making list rows too long.")
        );
        assert_eq!(row.origin.as_ref().unwrap().provider, "linear");
        assert_eq!(row.origin.as_ref().unwrap().id, "WT-123");
        assert_eq!(row.origin_health.status, "stale");
        assert_eq!(row.origin_health.next_action, "fetch");
        assert_eq!(row.origin_health.origin_label, "Linear WT-123");
        assert_eq!(row.child_origins[0].origin_label, "not published");
        assert_eq!(row.task_count, 1);
        assert_eq!(row.task_runs.prepared, 1);
        assert!(row.runnable.runnable);
        assert_eq!(row.runnable.runnable_count, 1);
        assert_eq!(row.profile.as_deref(), Some("codex"));
        assert_eq!(row.policy.pull_request, "none");
        assert_eq!(row.policy.review_codex_base, "none");
        assert_eq!(row.state_error, None);
        assert_eq!(report.invalid_workflows[0].id, "bad");
    }

    #[test]
    fn origin_health_shows_workflow_and_child_task_origins_separately() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "task body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            workflow_toml_with_one_task_origin(
                "Ship provider-origin UX",
                "WT-100",
                "origin-sync-tui",
            ),
        )
        .unwrap();

        let report = collect(&ctx).unwrap();
        let row = &report.workflows[0];

        assert_eq!(row.origin_health.origin_label, "Linear WT-100");
        assert_eq!(row.child_origins.len(), 1);
        assert_eq!(row.child_origins[0].task, "origin-sync-tui");
        assert_eq!(row.child_origins[0].origin_label, "Linear WT-142");
    }

    #[test]
    fn print_text_groups_rows_by_derived_action_state() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, ui) = ctx_with_ui(dir.path(), OutputMode::Text);
        write_task(dir.path(), "runnable-task", "feature/runnable");
        write_task_run(
            dir.path(),
            "run-2026-05-18-001-runnable",
            "runnable-task",
            "feature/runnable",
            "prepared",
            "2026-05-18-001",
        );
        write_workflow(
            dir.path(),
            "2026-05-18-001",
            "batch",
            r#"title = "Ship search"
body = "Keep search work coordinated."
"#,
            r#"[[tasks]]
task = "runnable-task"
run = "run-2026-05-18-001-runnable"
"#,
        );

        write_task(dir.path(), "waiting-task", "feature/waiting");
        write_task_run(
            dir.path(),
            "run-2026-05-18-002-waiting",
            "waiting-task",
            "feature/waiting",
            "running",
            "2026-05-18-002",
        );
        write_workflow(
            dir.path(),
            "2026-05-18-002",
            "stack",
            "",
            r#"[[tasks]]
task = "waiting-task"
run = "run-2026-05-18-002-waiting"
"#,
        );

        write_task(dir.path(), "passed-task", "feature/passed");
        write_task_run(
            dir.path(),
            "run-2026-05-18-003-passed",
            "passed-task",
            "feature/passed",
            "passed",
            "2026-05-18-003",
        );
        write_workflow(
            dir.path(),
            "2026-05-18-003",
            "single",
            r#"profile = "codex"
"#,
            r#"[[tasks]]
task = "passed-task"
run = "run-2026-05-18-003-passed"
"#,
        );

        let report = collect(&ctx).unwrap();
        print_text(&ctx, &report);

        let steps = ui.steps.lock().unwrap();
        let dims = ui.dims.lock().unwrap();
        assert!(dims.is_empty());
        let rendered = steps.join("\n");
        assert!(rendered.contains("◆ Workflows"));
        assert!(rendered.contains("│ runnable"));
        assert!(rendered.contains("│ waiting"));
        assert!(rendered.contains("│ passed"));
        assert!(rendered.contains(
            "│  •  Ship search  status local · origin none · id 2026-05-18-001 · mode batch · runs 1 prepared · policy none/manual review:none"
        ));
        assert!(rendered.contains("│     child origins runnable-task not published"));
        assert!(rendered.contains(
            "│  •  waiting-task  status local · origin none · id 2026-05-18-002 · mode stack · runs 1 running · reason waiting for running task · policy none/manual review:none"
        ));
        assert!(rendered.contains(
            "│  •  passed-task  status local · origin none · id 2026-05-18-003 · mode single · runs 1 passed · profile codex · policy none/manual review:none"
        ));
        assert!(!rendered.contains("body Keep search work coordinated."));
        assert!(!rendered.contains("file <repo-root>/.wt/execution/workflows/2026-05-18-001.toml"));
        assert!(!rendered.contains("stack_has_running_task_run"));
        assert!(!rendered.contains("runnable no"));
        assert!(!rendered.contains("updated 2026-05-18T00:00:00Z"));
    }

    #[test]
    fn print_text_summarizes_matrix_profiles_as_row_preview() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, ui) = ctx_with_ui(dir.path(), OutputMode::Text);
        let workflow_id = "2026-05-18-002";
        let task = "profile-task";
        let profiles = [
            "devtools-port-with-extra-long-label-alpha",
            "mcp-owned-profile-name-that-keeps-going",
            "codex-review-profile-name-that-keeps-going",
        ];

        write_task(dir.path(), task, "feature/profile-task");
        for (idx, profile) in profiles.iter().enumerate() {
            write_task_run(
                dir.path(),
                &format!("run-{workflow_id}-{idx}"),
                task,
                &format!("feature/profile-task-{profile}"),
                "prepared",
                workflow_id,
            );
        }
        write_workflow(
            dir.path(),
            workflow_id,
            "matrix",
            &format!(
                r#"profiles = ["{}", "{}", "{}"]
"#,
                profiles[0], profiles[1], profiles[2]
            ),
            &format!(
                r#"[[tasks]]
task = "{task}"

[[tasks.runs]]
profile = "{}"
run = "run-{workflow_id}-0"

[[tasks.runs]]
profile = "{}"
run = "run-{workflow_id}-1"

[[tasks.runs]]
profile = "{}"
run = "run-{workflow_id}-2"
"#,
                profiles[0], profiles[1], profiles[2]
            ),
        );

        let report = collect(&ctx).unwrap();
        print_text(&ctx, &report);

        let steps = ui.steps.lock().unwrap();
        let row = steps
            .iter()
            .find(|line| line.contains("2026-05-18-002"))
            .expect("matrix row should be rendered");
        assert!(row.contains("matrix"));
        assert!(row.contains("3 prepared"));
        assert!(row.contains("profiles 3"));
        assert!(row.contains("none/manual"));
        assert!(!row.contains("devtools-port-with-extra-long-label-alpha"));
        assert!(!steps.iter().any(|line| {
            line.contains("<repo-root>/.wt/execution/workflows/2026-05-18-002.toml")
        }));
    }

    #[test]
    fn collect_does_not_apply_runnable_selector_cap() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), OutputMode::Json);

        for idx in 1..=11 {
            write_workflow(
                dir.path(),
                &format!("2026-05-18-{idx:03}"),
                "batch",
                "",
                &format!(
                    r#"[[tasks]]
task = "task-{idx}"
run = "run-{idx}"
"#
                ),
            );
        }

        let report = collect(&ctx).unwrap();

        assert_eq!(report.workflows.len(), 11);
        assert_eq!(report.invalid_workflows.len(), 0);
        assert_eq!(report.workflows[10].id, "2026-05-18-011");
    }

    #[test]
    fn collect_reports_removed_objective_guidance_for_old_workflows() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, ui) = ctx_with_ui(dir.path(), OutputMode::Text);
        write_workflow(
            dir.path(),
            "2026-05-18-099",
            "batch",
            r#"objective = "Ship search"
"#,
            r#"[[tasks]]
task = "schema"
run = "run-2026-05-18-099-schema"
"#,
        );

        let report = collect(&ctx).unwrap();

        assert!(report.workflows.is_empty());
        assert_eq!(report.invalid_workflows.len(), 1);
        let error = &report.invalid_workflows[0].error;
        assert!(error.contains("removed `objective`"));
        assert!(error.contains("top-level `title`, `body`, and optional `[origin]`"));

        print_text(&ctx, &report);
        let warnings = ui.warnings.lock().unwrap();
        assert!(warnings.iter().any(|warning| {
            warning == "2026-05-18-099  file <repo-root>/.wt/execution/workflows/2026-05-18-099.toml  uses removed `objective`; edit the workflow file to use top-level `title`, `body`, and optional `[origin]`"
        }));
    }

    fn write_task(root: &Path, key: &str, branch: &str) {
        let dir = root.join(".wt/execution/tasks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{key}.toml")),
            format!(
                r#"title = "{key}"
branch = "{branch}"
body = "Task body"
"#
            ),
        )
        .unwrap();
    }

    fn write_task_run(root: &Path, id: &str, task: &str, branch: &str, status: &str, group: &str) {
        let dir = root.join(".wt/execution/task-runs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{id}.toml")),
            format!(
                r#"task = "{task}"
branch = "{branch}"
status = "{status}"
group = "{group}"
creation_order = 1
created_at = "2026-05-18T00:00:00.000000000Z"
updated_at = "2026-05-18T00:00:00.000000000Z"
"#
            ),
        )
        .unwrap();
    }

    fn write_workflow(root: &Path, id: &str, mode: &str, extra: &str, tasks: &str) {
        let dir = root.join(".wt/execution/workflows");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{id}.toml")),
            format!(
                r#"mode = "{mode}"
{extra}base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-18T00:00:00Z"
updated_at = "2026-05-18T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

{tasks}"#
            ),
        )
        .unwrap();
    }
}
