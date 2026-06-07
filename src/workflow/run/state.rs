use crate::context::Ctx;
use crate::services::current_actor;
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

pub(crate) fn ensure_workflow_task_routes(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<WorkflowRouteRepairSummary> {
    let group = task_run::group_from_path(workflow_path)?;
    let label = workflow_coordinator_label(metadata.title.as_deref(), &group);
    let mut fallback_coordinator_id = None::<String>;
    let mut repaired_missing_coordinator = false;

    for row in &metadata.tasks {
        if row.runs.is_empty() {
            ensure_workflow_task_run_route(
                ctx,
                &group,
                row,
                &row.run,
                &label,
                &mut fallback_coordinator_id,
                &mut repaired_missing_coordinator,
            )?;
        } else {
            for profile_run in &row.runs {
                ensure_workflow_task_run_route(
                    ctx,
                    &group,
                    row,
                    &profile_run.run,
                    &label,
                    &mut fallback_coordinator_id,
                    &mut repaired_missing_coordinator,
                )?;
            }
        }
    }

    Ok(WorkflowRouteRepairSummary {
        fallback_coordinator_id,
        repaired_missing_coordinator,
    })
}

pub(crate) struct WorkflowRouteRepairSummary {
    pub(crate) fallback_coordinator_id: Option<String>,
    pub(crate) repaired_missing_coordinator: bool,
}

fn ensure_workflow_task_run_route(
    ctx: &Ctx,
    group: &str,
    row: &WorkflowTask,
    run_id: &str,
    label: &str,
    fallback_coordinator_id: &mut Option<String>,
    repaired_missing_coordinator: &mut bool,
) -> Result<()> {
    let run_path = task_run::resolve(ctx, run_id).with_context(|| {
        format!(
            "Workflow task {} references missing TaskRun {}",
            workflow_task_label(row),
            run_id
        )
    })?;
    let run = task_run::read(&run_path)?;
    validate_workflow_task_run(row, &run)?;
    validate_workflow_task_run_group(row, &run, group)?;
    let coordinator_id = workflow_route_coordinator_id(
        ctx,
        &run,
        fallback_coordinator_id,
        repaired_missing_coordinator,
    )?;
    let record = task_run::TaskRunRecord {
        id: run_id.to_string(),
        path: run_path,
        run,
    };
    task_run::ensure_workflow_routes(&record, &coordinator_id, Some(label))?;
    Ok(())
}

fn workflow_route_coordinator_id(
    ctx: &Ctx,
    run: &task_run::TaskRun,
    fallback_coordinator_id: &mut Option<String>,
    repaired_missing_coordinator: &mut bool,
) -> Result<String> {
    if let Some(id) = run
        .coordinator_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return Ok(id.to_string());
    }
    *repaired_missing_coordinator = true;
    if fallback_coordinator_id.is_none() {
        *fallback_coordinator_id = Some(
            current_actor::resolve_launch_coordinator(ctx, None)?
                .as_str()
                .to_string(),
        );
    }
    Ok(fallback_coordinator_id
        .as_ref()
        .expect("fallback coordinator id was just initialized")
        .clone())
}

fn workflow_coordinator_label(title: Option<&str>, workflow_id: &str) -> String {
    match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => format!("Coordinator for workflow \"{title}\""),
        None => format!("Coordinator for workflow {workflow_id}"),
    }
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
        let actual = run.group.as_deref().unwrap_or("none");
        bail!(
            "Workflow task {} references TaskRun {} outside workflow group: expected {}, found {}",
            row.task,
            row.run,
            group,
            actual
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
