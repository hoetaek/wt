use crate::context::Ctx;
use crate::task as task_store;
use crate::task_run;
use crate::workflow::render::workflow_task_label;
use crate::workflow::{WorkflowMetadata, WorkflowTask};
use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Clone, Debug)]
pub(crate) struct WorkflowTaskState {
    pub(crate) idx: usize,
    pub(crate) row: WorkflowTask,
    pub(crate) profile: Option<String>,
    pub(crate) run_id: String,
    pub(crate) document: task_store::TaskDocument,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) run: task_run::TaskRun,
}

pub(crate) fn read_single_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    read_workflow_task_states(ctx, workflow_path, metadata)
}

pub(crate) fn read_batch_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    read_workflow_task_states(ctx, workflow_path, metadata)
}

pub(crate) fn read_stack_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    read_workflow_task_states(ctx, workflow_path, metadata)
}

pub(crate) fn read_matrix_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let group = task_run::group_from_path(workflow_path)?;
    let Some(row) = metadata.tasks.first() else {
        return Ok(Vec::new());
    };
    let (document, path, content) = task_store::read_task_file(ctx, &row.task)?;
    row.runs
        .iter()
        .enumerate()
        .map(|(idx, profile_run)| {
            let run_path = task_run::resolve(ctx, &profile_run.run).with_context(|| {
                format!(
                    "Workflow task {} profile {} references missing TaskRun {}",
                    row.task, profile_run.profile, profile_run.run
                )
            })?;
            let run = task_run::read(&run_path)?;
            validate_workflow_task_run(row, &run)?;
            validate_workflow_task_run_group(row, &run, &group)?;
            Ok(WorkflowTaskState {
                idx,
                row: row.clone(),
                profile: Some(profile_run.profile.clone()),
                run_id: profile_run.run.clone(),
                document: document.clone(),
                path: path.clone(),
                content: content.clone(),
                run,
            })
        })
        .collect()
}

fn read_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let group = task_run::group_from_path(workflow_path)?;
    metadata
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let (document, path, content) = task_store::read_task_file(ctx, &row.task)?;
            let run_path = task_run::resolve(ctx, &row.run).with_context(|| {
                format!(
                    "Workflow task {} references missing TaskRun {}",
                    row.task, row.run
                )
            })?;
            let run = task_run::read(&run_path)?;
            validate_workflow_task_run(row, &run)?;
            validate_workflow_task_run_group(row, &run, &group)?;
            Ok(WorkflowTaskState {
                idx,
                row: row.clone(),
                profile: None,
                run_id: row.run.clone(),
                document,
                path,
                content,
                run,
            })
        })
        .collect()
}

pub(crate) fn validate_workflow_task_run(
    row: &WorkflowTask,
    run: &task_run::TaskRun,
) -> Result<()> {
    if run.task != row.task {
        bail!(
            "Workflow task {} references TaskRun {} for task {}",
            row.task,
            row.run,
            run.task
        );
    }
    Ok(())
}

pub(crate) fn update_workflow_profile_task_run(
    ctx: &Ctx,
    row: &WorkflowTask,
    profile: &str,
    run_id: &str,
    status: task_run::TaskRunStatus,
    branch: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    let path = task_run::resolve(ctx, run_id).with_context(|| {
        format!(
            "Workflow task {} profile {} references missing TaskRun {}",
            workflow_task_label(row),
            profile,
            run_id
        )
    })?;
    let run = task_run::read(&path)?;
    validate_workflow_task_run(row, &run)?;

    let branch = branch
        .map(str::to_string)
        .unwrap_or_else(|| run.branch.clone());
    let updated = task_run::update(ctx, run_id, status, Some(&branch), error)?;
    validate_workflow_task_run(row, &updated.run)?;
    Ok(())
}

fn validate_workflow_task_run_group(
    row: &WorkflowTask,
    run: &task_run::TaskRun,
    group: &str,
) -> Result<()> {
    if run.group.as_deref() != Some(group) {
        bail!(
            "Workflow task {} references TaskRun {} outside workflow group {}",
            row.task,
            row.run,
            group
        );
    }
    Ok(())
}

pub(crate) fn update_workflow_task_run(
    ctx: &Ctx,
    row: &WorkflowTask,
    status: task_run::TaskRunStatus,
    error: Option<&str>,
) -> Result<()> {
    let path = task_run::resolve(ctx, &row.run).with_context(|| {
        format!(
            "Workflow task {} references missing TaskRun {}",
            workflow_task_label(row),
            row.run
        )
    })?;
    let run = task_run::read(&path)?;
    validate_workflow_task_run(row, &run)?;

    let branch = task_store::read_task_document(ctx, &row.task)
        .ok()
        .map(|task| task.branch);
    let updated = task_run::update(ctx, &row.run, status, branch.as_deref(), error)?;
    validate_workflow_task_run(row, &updated.run)?;
    Ok(())
}

pub(crate) fn task_run_record(ctx: &Ctx, run: &str) -> Option<task_run::TaskRun> {
    task_run::resolve(ctx, run)
        .and_then(|path| task_run::read(&path))
        .ok()
}
