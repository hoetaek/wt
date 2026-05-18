use crate::context::Ctx;
use crate::task_run::{
    STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::workflow as workflow_store;
use crate::workflow::planner::runnable_workflow_info;
use crate::workflow::render::workflow_relative_path;
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states, task_run_record,
};
use crate::workflow::{WorkflowMetadata, WorkflowMode};
use anyhow::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

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
    objective_summary: Option<String>,
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
struct TaskRunSummary {
    total: usize,
    prepared: usize,
    running: usize,
    done: usize,
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
}

#[derive(Debug, Serialize)]
struct InvalidWorkflowRow {
    id: String,
    path: String,
    error: String,
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
        objective_summary: objective_summary(metadata.objective.as_deref()),
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
        },
        updated_at: metadata.updated_at,
        state_error,
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
        done: 0,
        failed: 0,
        skipped: 0,
        missing: 0,
        summary: String::new(),
    };

    for run_id in workflow_run_ids(metadata) {
        match task_run_record(ctx, run_id).map(|run| run.status) {
            Some(STATUS_PREPARED) => summary.prepared += 1,
            Some(STATUS_RUNNING) => summary.running += 1,
            Some(STATUS_DONE) => summary.done += 1,
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
        ("done", summary.done),
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

fn objective_summary(objective: Option<&str>) -> Option<String> {
    objective
        .map(one_line)
        .filter(|objective| !objective.is_empty())
        .map(|objective| truncate_chars(&objective, 80))
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
        ctx.ui.print_step("No workflows found in .local/workflows");
        return;
    }

    for row in &report.workflows {
        ctx.ui.print_step(&workflow_identity_label(row));
        if let Some(objective) = row.objective_summary.as_deref() {
            ctx.ui.print_dim(&format!("  Objective: {objective}"));
        }
        ctx.ui.print_dim(&workflow_detail_label(row));
        ctx.ui.print_dim(&format!("  Path: {}", row.path));
        if let Some(error) = row.state_error.as_deref() {
            ctx.ui.print_warning(&format!(
                "Workflow state unavailable for {}: {}",
                row.id,
                one_line(error)
            ));
        }
    }

    for invalid in &report.invalid_workflows {
        ctx.ui.print_warning(&format!(
            "Invalid workflow {}: {}",
            invalid.path,
            one_line(&invalid.error)
        ));
    }
}

fn workflow_identity_label(row: &WorkflowListRow) -> String {
    format!(
        "{}  mode {}  task_runs {}  runnable {}  updated {}",
        row.id,
        row.mode,
        task_run_label(&row.task_runs),
        runnable_label(&row.runnable),
        row.updated_at
    )
}

fn workflow_detail_label(row: &WorkflowListRow) -> String {
    format!(
        "  Base: {}  Profile: {}  Policy: {}/{}",
        row_base_label(row),
        row_profile_label(row),
        row.policy.pull_request,
        row.policy.landing
    )
}

fn task_run_label(summary: &TaskRunSummary) -> String {
    format!("{} ({})", summary.total, summary.summary)
}

fn runnable_label(runnable: &RunnableMetadata) -> String {
    if runnable.runnable {
        format!("yes ({})", runnable.runnable_count)
    } else {
        format!("no ({})", non_runnable_label(&runnable.reason))
    }
}

fn non_runnable_label(reason: &str) -> &'static str {
    match reason {
        "single_requires_all_task_runs_prepared_or_failed" => {
            "needs all task runs prepared or failed"
        }
        "batch_has_no_prepared_or_failed_task_runs" => "no prepared or failed task runs",
        "matrix_has_no_prepared_or_failed_profile_runs" => "no prepared or failed profile runs",
        "stack_has_running_task_run" => "running task must complete",
        "stack_has_no_next_prepared_or_failed_task_run" => "no next prepared or failed task",
        "state_unavailable" => "state unavailable",
        _ => "not runnable",
    }
}

fn row_base_label(row: &WorkflowListRow) -> String {
    row.base
        .clone()
        .unwrap_or_else(|| format!("({})", row.base_mode))
}

fn row_profile_label(row: &WorkflowListRow) -> String {
    if !row.profiles.is_empty() {
        let preview = row
            .profiles
            .iter()
            .take(2)
            .map(|profile| truncate_chars(profile, 24))
            .collect::<Vec<_>>();
        let remaining = row.profiles.len().saturating_sub(preview.len());
        let mut parts = preview;
        if remaining > 0 {
            parts.push(format!("+{remaining}"));
        }
        let noun = if row.profiles.len() == 1 {
            "profile"
        } else {
            "profiles"
        };
        format!("{} {} ({})", row.profiles.len(), noun, parts.join(", "))
    } else {
        row.profile.as_deref().unwrap_or("-").into()
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
            r#"objective = "Ship search"
profile = "codex"
"#,
            r#"[[tasks]]
task = "schema"
run = "run-2026-05-18-001-schema"
"#,
        );
        fs::write(
            dir.path().join(".local/workflows/bad.toml"),
            "mode = \"batch\"\n",
        )
        .unwrap();

        let report = collect(&ctx).unwrap();

        assert_eq!(report.workflows.len(), 1);
        assert_eq!(report.invalid_workflows.len(), 1);
        let row = &report.workflows[0];
        assert_eq!(row.id, "2026-05-18-001");
        assert_eq!(row.mode, "batch");
        assert_eq!(row.objective_summary.as_deref(), Some("Ship search"));
        assert_eq!(row.task_count, 1);
        assert_eq!(row.task_runs.prepared, 1);
        assert!(row.runnable.runnable);
        assert_eq!(row.runnable.runnable_count, 1);
        assert_eq!(row.profile.as_deref(), Some("codex"));
        assert_eq!(row.policy.pull_request, "none");
        assert_eq!(row.state_error, None);
        assert_eq!(report.invalid_workflows[0].id, "bad");
    }

    #[test]
    fn print_text_keeps_state_on_primary_line_and_details_secondary() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, ui) = ctx_with_ui(dir.path(), OutputMode::Text);
        write_task(dir.path(), "schema", "feature/schema");
        write_task_run(
            dir.path(),
            "run-2026-05-18-001-schema",
            "schema",
            "feature/schema",
            "done",
            "2026-05-18-001",
        );
        write_workflow(
            dir.path(),
            "2026-05-18-001",
            "single",
            r#"objective = "Ship search"
profile = "codex"
"#,
            r#"[[tasks]]
task = "schema"
run = "run-2026-05-18-001-schema"
"#,
        );

        let report = collect(&ctx).unwrap();
        print_text(&ctx, &report);

        let steps = ui.steps.lock().unwrap();
        let dims = ui.dims.lock().unwrap();
        assert_eq!(steps.len(), 1);
        let primary = &steps[0];
        assert!(primary.contains("2026-05-18-001"));
        assert!(primary.contains("mode single"));
        assert!(primary.contains("task_runs 1 (1 done)"));
        assert!(primary.contains("runnable no (needs all task runs prepared or failed)"));
        assert!(primary.contains("updated 2026-05-18T00:00:00Z"));
        assert!(!primary.contains("single_requires_all_task_runs_prepared_or_failed"));
        assert!(!primary.contains("base main"));
        assert!(!primary.contains("profile codex"));
        assert!(dims.iter().any(|line| line == "  Objective: Ship search"));
        assert!(dims.iter().any(|line| {
            line.contains("Base: main")
                && line.contains("Profile: codex")
                && line.contains("Policy: none/manual")
        }));
        assert!(
            dims.iter()
                .any(|line| line == "  Path: .local/workflows/2026-05-18-001.toml")
        );
    }

    #[test]
    fn print_text_summarizes_matrix_profiles_off_primary_line() {
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
        let dims = ui.dims.lock().unwrap();
        let primary = &steps[0];
        assert!(primary.contains("mode matrix"));
        assert!(primary.contains("task_runs 3 (3 prepared)"));
        assert!(primary.contains("runnable yes (3)"));
        assert!(!primary.contains("devtools-port-with-extra-long-label-alpha"));

        let detail = dims
            .iter()
            .find(|line| line.contains("Profile: 3 profiles"))
            .expect("matrix profile summary should be on a detail line");
        assert!(detail.contains("devtools-port-with-extra..."));
        assert!(detail.contains("+1"));
        assert!(!detail.contains("codex-review-profile-name-that-keeps-going"));
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

    fn write_task(root: &Path, key: &str, branch: &str) {
        let dir = root.join(".local/tasks");
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
        let dir = root.join(".local/task-runs");
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
        let dir = root.join(".local/workflows");
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
