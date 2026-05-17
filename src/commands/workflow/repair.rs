use super::resolve_mutating_target;
use crate::commands::task_run::{self, STATUS_FAILED, TaskRunRecord, TaskRunStatus};
use crate::context::Ctx;
use crate::services::runtime_binding::RuntimeBindingResolver;
use crate::services::work::{Work, WorkSessionState};
use crate::workflow as workflow_store;
use crate::workflow::{WorkflowMetadata, WorkflowTask};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) fn run(ctx: &Ctx, workflow: &str, apply: bool) -> Result<()> {
    let path = if apply {
        resolve_mutating_target(ctx, workflow, "repair")?
    } else {
        workflow_store::resolve(ctx, workflow)?
    };
    let metadata = workflow_store::read(&path)?;
    let plan = build_repair_plan(ctx, &path, &metadata)?;
    print_repair_plan(ctx, &plan, apply);
    if apply {
        apply_repair_plan(ctx, &plan)?;
    }
    Ok(())
}

struct RepairPlan {
    workflow_path: PathBuf,
    items: Vec<RepairItem>,
}

struct RepairItem {
    task: String,
    run: String,
    status: Option<TaskRunStatus>,
    branch: Option<String>,
    problem: String,
    action: RepairAction,
}

enum RepairAction {
    MarkFailed { error: String },
    Manual { note: String },
}

fn build_repair_plan(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<RepairPlan> {
    let resolver = RuntimeBindingResolver::new(ctx);
    let mut items = Vec::new();

    for row in &metadata.tasks {
        match read_task_run_record(ctx, row) {
            Ok(record) => {
                if let Some(item) = repair_item_for_record(ctx, &resolver, row, &record)? {
                    items.push(item);
                }
            }
            Err(err) => items.push(RepairItem {
                task: row.task.clone(),
                run: row.run.clone(),
                status: None,
                branch: None,
                problem: format!("Workflow task references an unreadable TaskRun: {err:#}"),
                action: RepairAction::Manual {
                    note:
                        "Recreate the TaskRun or edit the workflow row after inspecting local state"
                            .into(),
                },
            }),
        }
    }

    Ok(RepairPlan {
        workflow_path: workflow_path.to_path_buf(),
        items,
    })
}

fn read_task_run_record(ctx: &Ctx, row: &WorkflowTask) -> Result<TaskRunRecord> {
    let path = task_run::resolve(ctx, &row.run).with_context(|| {
        format!(
            "Workflow task {} references missing TaskRun {}",
            row.task, row.run
        )
    })?;
    let run = task_run::read(&path)?;
    Ok(TaskRunRecord {
        id: row.run.clone(),
        path,
        run,
    })
}

fn repair_item_for_record(
    ctx: &Ctx,
    resolver: &RuntimeBindingResolver<'_>,
    row: &WorkflowTask,
    record: &TaskRunRecord,
) -> Result<Option<RepairItem>> {
    match record.run.status {
        task_run::STATUS_RUNNING | task_run::STATUS_PREPARED => {
            if let Some(error) = startup_failure_error(record.run.error.as_deref()) {
                return Ok(Some(mark_failed_item(
                    row,
                    record,
                    "TaskRun records a startup or prompt-delivery failure but is not failed",
                    error,
                )));
            }
            match record.run.status {
                task_run::STATUS_RUNNING => running_repair_item(ctx, resolver, row, record),
                task_run::STATUS_PREPARED => prepared_repair_item(ctx, resolver, row, record),
                _ => unreachable!(),
            }
        }
        _ => Ok(None),
    }
}

fn running_repair_item(
    _ctx: &Ctx,
    resolver: &RuntimeBindingResolver<'_>,
    row: &WorkflowTask,
    record: &TaskRunRecord,
) -> Result<Option<RepairItem>> {
    let work = resolver.observe(Some(&record.id))?;
    match work.session_state {
        WorkSessionState::TerminalSurfaceReady => Ok(None),
        WorkSessionState::AmbiguousTerminalSurface => Ok(Some(manual_runtime_item(
            row,
            record,
            "Running TaskRun has multiple live cmux agent surfaces; no unique runtime binding was validated",
            "Inspect wt agent status or wt inspect output and choose the intended cmux surface before changing TaskRun state",
        ))),
        WorkSessionState::NoLocalWorktree => Ok(Some(mark_failed_item(
            row,
            record,
            "Running TaskRun has no usable local worktree",
            "Workflow runtime repair: running TaskRun has no usable local worktree",
        ))),
        WorkSessionState::CmuxUnavailable
        | WorkSessionState::NoCmuxWorkspace
        | WorkSessionState::NoTerminalSurface => {
            let problem = no_live_surface_problem(&work);
            let error = format!("Workflow runtime repair: {problem}");
            Ok(Some(mark_failed_item(row, record, &problem, &error)))
        }
    }
}

fn prepared_repair_item(
    _ctx: &Ctx,
    resolver: &RuntimeBindingResolver<'_>,
    row: &WorkflowTask,
    record: &TaskRunRecord,
) -> Result<Option<RepairItem>> {
    let work = resolver.observe(Some(&record.id))?;
    match work.session_state {
        WorkSessionState::NoLocalWorktree => Ok(None),
        WorkSessionState::TerminalSurfaceReady => Ok(Some(manual_runtime_item(
            row,
            record,
            "Prepared TaskRun already has a live cmux agent surface",
            "Inspect the worktree before deciding whether the TaskRun should be running or failed",
        ))),
        WorkSessionState::AmbiguousTerminalSurface => Ok(Some(manual_runtime_item(
            row,
            record,
            "Prepared TaskRun has multiple live cmux agent surfaces; no unique runtime binding was validated",
            "Inspect wt agent status or wt inspect output before changing TaskRun state",
        ))),
        WorkSessionState::CmuxUnavailable
        | WorkSessionState::NoCmuxWorkspace
        | WorkSessionState::NoTerminalSurface => {
            let problem = format!(
                "Prepared TaskRun has a local worktree but no live validated agent surface ({})",
                work.session_state.as_str()
            );
            let error = format!("Workflow runtime repair: {problem}");
            Ok(Some(mark_failed_item(row, record, &problem, &error)))
        }
    }
}

fn startup_failure_error(error: Option<&str>) -> Option<&str> {
    let error = error?.trim();
    if error.is_empty() {
        return None;
    }
    let lower = error.to_ascii_lowercase();
    let is_startup_failure = lower.contains("agent prompt")
        || (lower.contains("prompt") && lower.contains("failed"))
        || (lower.contains("cmux") && lower.contains("failed"))
        || (lower.contains("startup") && lower.contains("failed"));
    is_startup_failure.then_some(error)
}

fn no_live_surface_problem(work: &Work) -> String {
    match work.session_state {
        WorkSessionState::CmuxUnavailable => work
            .message
            .as_deref()
            .map(|message| {
                format!("Running TaskRun has no live validated agent surface ({message})")
            })
            .unwrap_or_else(|| "Running TaskRun has no live validated agent surface".into()),
        WorkSessionState::NoCmuxWorkspace => {
            "Running TaskRun has no matching cmux workspace for its worktree".into()
        }
        WorkSessionState::NoTerminalSurface => {
            "Running TaskRun has no live validated agent surface".into()
        }
        _ => "Running TaskRun has no live validated agent surface".into(),
    }
}

fn mark_failed_item(
    row: &WorkflowTask,
    record: &TaskRunRecord,
    problem: &str,
    error: &str,
) -> RepairItem {
    RepairItem {
        task: row.task.clone(),
        run: record.id.clone(),
        status: Some(record.run.status),
        branch: Some(record.run.branch.clone()),
        problem: problem.into(),
        action: RepairAction::MarkFailed {
            error: error.into(),
        },
    }
}

fn manual_runtime_item(
    row: &WorkflowTask,
    record: &TaskRunRecord,
    problem: &str,
    note: &str,
) -> RepairItem {
    RepairItem {
        task: row.task.clone(),
        run: record.id.clone(),
        status: Some(record.run.status),
        branch: Some(record.run.branch.clone()),
        problem: problem.into(),
        action: RepairAction::Manual { note: note.into() },
    }
}

fn print_repair_plan(ctx: &Ctx, plan: &RepairPlan, apply: bool) {
    let mode = if apply { "apply" } else { "preview" };
    ctx.ui.print_step(&format!(
        "Workflow repair {mode}: {}",
        plan.workflow_path.display()
    ));

    if plan.items.is_empty() {
        ctx.ui
            .print_step("No workflow runtime repairs recommended.");
        return;
    }

    ctx.ui
        .print_dim(&format!("  Recommendations: {}", plan.items.len()));
    for item in &plan.items {
        let status = item
            .status
            .map(|status| status.as_str().to_string())
            .unwrap_or_else(|| "unreadable".into());
        let branch = item.branch.as_deref().unwrap_or("unknown");
        ctx.ui.print_dim(&format!(
            "  - {} (TaskRun {}, status={}, branch={})",
            item.task, item.run, status, branch
        ));
        ctx.ui.print_dim(&format!("    Problem: {}", item.problem));
        match &item.action {
            RepairAction::MarkFailed { .. } if apply => {
                ctx.ui.print_dim("    Action: mark TaskRun failed");
            }
            RepairAction::MarkFailed { .. } => {
                ctx.ui
                    .print_dim("    Action: mark TaskRun failed (requires --apply)");
            }
            RepairAction::Manual { note } => {
                ctx.ui
                    .print_warning(&format!("    Manual follow-up: {note}"));
            }
        }
    }

    if !apply {
        ctx.ui.print_step(
            "Preview only; no TaskRun state changed. Re-run with --apply to apply repairable actions.",
        );
    }
}

fn apply_repair_plan(ctx: &Ctx, plan: &RepairPlan) -> Result<()> {
    let mut applied = 0;
    for item in &plan.items {
        let RepairAction::MarkFailed { error } = &item.action else {
            continue;
        };
        task_run::update(ctx, &item.run, STATUS_FAILED, None, Some(error))?;
        applied += 1;
    }

    if applied == 0 {
        ctx.ui
            .print_step("No automatic workflow runtime repairs to apply.");
    } else {
        ctx.ui.print_step(&format!(
            "Applied {applied} workflow runtime repair{}.",
            if applied == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}
