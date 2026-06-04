use crate::context::Ctx;
use crate::workflow::render::base_label;
use crate::workflow::run::{
    read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states,
};
use crate::workflow::{WorkflowMetadata, WorkflowMode};
use anyhow::Result;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize)]
pub(crate) struct WorkflowShowSnapshot {
    pub(crate) path: String,
    pub(crate) mode: String,
    pub(crate) base: String,
    pub(crate) title: Option<String>,
    pub(crate) pull_request: String,
    pub(crate) landing: String,
    pub(crate) review: WorkflowShowReviewPolicySnapshot,
    pub(crate) tasks: Vec<WorkflowShowTaskSnapshot>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkflowShowReviewPolicySnapshot {
    pub(crate) codex_base: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkflowShowTaskSnapshot {
    pub(crate) order: usize,
    pub(crate) task: String,
    pub(crate) status: String,
    pub(crate) branch: String,
    pub(crate) parent: Option<String>,
    pub(crate) title: String,
}

pub(crate) fn show_workflow_json(
    ctx: &Ctx,
    path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<()> {
    let snapshot = collect_workflow_snapshot(ctx, path, metadata)?;
    write_workflow_snapshot_json(&snapshot)
}

pub(crate) fn collect_workflow_snapshot(
    ctx: &Ctx,
    path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<WorkflowShowSnapshot> {
    let states = match metadata.mode {
        WorkflowMode::Single => read_single_workflow_task_states(ctx, path, metadata)?,
        WorkflowMode::Batch => read_batch_workflow_task_states(ctx, path, metadata)?,
        WorkflowMode::Stack => read_stack_workflow_task_states(ctx, path, metadata)?,
        WorkflowMode::Matrix => read_matrix_workflow_task_states(ctx, path, metadata)?,
    };

    let tasks = states
        .into_iter()
        .map(|state| {
            let task = state.row.task;
            let title = state.document.title_or_key(&task);
            WorkflowShowTaskSnapshot {
                order: state.idx + 1,
                task,
                status: state.run.status.as_str().to_string(),
                branch: state.run.branch,
                parent: state.row.parent,
                title,
            }
        })
        .collect();

    Ok(WorkflowShowSnapshot {
        path: ctx.storage_root.display_path(path),
        mode: metadata.mode.as_str().to_string(),
        base: base_label(metadata),
        title: metadata.title.clone(),
        pull_request: metadata.policy.pull_request.as_str().to_string(),
        landing: metadata.policy.landing.as_str().to_string(),
        review: WorkflowShowReviewPolicySnapshot {
            codex_base: metadata.policy.review.codex_base.as_str().to_string(),
        },
        tasks,
    })
}

pub(super) fn write_workflow_snapshot_json(snapshot: &WorkflowShowSnapshot) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, snapshot)?;
    writeln!(handle)?;
    Ok(())
}
