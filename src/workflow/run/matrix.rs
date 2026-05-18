use super::state::{WorkflowTaskState, read_matrix_workflow_task_states};
use super::{
    apply_workflow_color, is_cancelled, task_issue_closing_references,
    update_workflow_profile_task_run,
};
use crate::commands::issue;
use crate::commands::profile_selection;
use crate::config::AGENT_PROMPT_WORKFLOW_SCOPE;
use crate::context::Ctx;
use crate::setup;
use crate::task as task_store;
use crate::task_run::{self, STATUS_FAILED, STATUS_RUNNING};
use crate::workflow as workflow_store;
use crate::workflow::planner::workflow_base_raw;
use crate::workflow::render::{
    failed_workflow_task_message, no_runnable_workflow_tasks_message,
    started_workflow_task_message, starting_workflow_task_message,
    workflow_batch_task_prompt_intro, workflow_matrix_task_handoff_section,
    workflow_metadata_prompt_context,
};
use crate::workflow::{WorkflowMetadata, WorkflowPolicy};
use anyhow::{Result, bail};
use std::path::Path;

pub(super) fn run_matrix_workflow(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &mut WorkflowMetadata,
    _jobs: usize,
) -> Result<()> {
    profile_selection::load_selected_profiles(ctx, &metadata.profiles)?;
    if metadata
        .color
        .as_deref()
        .is_none_or(|color| color.trim().is_empty())
    {
        workflow_store::write(ctx, workflow_path, metadata)?;
    }

    let states = read_matrix_workflow_task_states(ctx, workflow_path, metadata)?;
    if states.is_empty() {
        bail!("Workflow has no matrix runs: {}", workflow_path.display());
    }
    let runnable = states
        .into_iter()
        .filter(|state| state.run.is_runnable())
        .collect::<Vec<_>>();
    if runnable.is_empty() {
        ctx.ui.print_step(no_runnable_workflow_tasks_message());
        return Ok(());
    }

    let base = workflow_base_raw(metadata)?.expect("workflow base is validated");
    let workflow_context = workflow_metadata_prompt_context(metadata);
    let mut failed = false;
    for (idx, state) in runnable.iter().enumerate() {
        let Some(profile) = state.profile.as_deref() else {
            bail!("matrix workflow task run is missing profile");
        };
        let label = matrix_task_label(&state.row.task, profile);
        ctx.ui.print_step(&starting_workflow_task_message(&label));
        update_workflow_profile_task_run(
            ctx,
            &state.row,
            profile,
            &state.run_id,
            STATUS_RUNNING,
            None,
            None,
        )?;

        let result = run_matrix_workflow_task(
            ctx,
            MatrixWorkflowTaskContext {
                workflow_path,
                state,
                base: &base,
                policy: &metadata.policy,
                workflow_context: workflow_context.clone(),
                profile,
            },
        );
        match apply_matrix_workflow_result(ctx, state, result, metadata.color.as_deref())? {
            MatrixWorkflowTaskOutcome::Started => {}
            MatrixWorkflowTaskOutcome::Failed => failed = true,
            MatrixWorkflowTaskOutcome::Cancelled => {
                failed = true;
                for skipped in runnable.iter().skip(idx + 1) {
                    record_matrix_workflow_failure(
                        ctx,
                        skipped,
                        task_run::STATUS_SKIPPED,
                        "Skipped after user cancellation",
                    )?;
                }
                break;
            }
        }
    }

    if failed {
        bail!("Workflow matrix failed: {}", workflow_path.display());
    }
    Ok(())
}

struct MatrixWorkflowTaskContext<'a> {
    workflow_path: &'a Path,
    state: &'a WorkflowTaskState,
    base: &'a str,
    policy: &'a WorkflowPolicy,
    workflow_context: Option<String>,
    profile: &'a str,
}

enum MatrixWorkflowTaskOutcome {
    Started,
    Failed,
    Cancelled,
}

fn run_matrix_workflow_task(
    ctx: &Ctx,
    task: MatrixWorkflowTaskContext<'_>,
) -> Result<issue::IssueRunResult> {
    let state = task.state;
    let completion_section = workflow_matrix_task_handoff_section(
        task.workflow_path,
        &state.row,
        task.profile,
        task.policy,
        task.base,
        &task_issue_closing_references(&state.document),
    );
    let branch_name = task_store::prepared_branch_name(&state.document.branch);
    if branch_name.is_none() && state.document.origin.is_none() {
        bail!("Workflow task {} has no branch", state.row.task);
    }
    let identifier = state.document.identifier_or_key(&state.row.task);
    let title = state.document.title_or_key(&state.row.task);
    let base = Some(task.base.to_string());
    let prepared = issue::PreparedIssueContext {
        identifier: &identifier,
        title: &title,
        branch_name,
        setup_mode: state.document.setup_mode(),
        additional_prompt_scope: Some(AGENT_PROMPT_WORKFLOW_SCOPE),
        workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
        on_start_issue_id: state
            .document
            .origin
            .as_ref()
            .map(|origin| origin.id.as_str()),
        prompt_intro: workflow_batch_task_prompt_intro(),
        completion_section: Some(&completion_section),
        pre_snapshot_context: task.workflow_context.as_deref(),
        workspace_label: None,
        snapshot: issue::IssueSnapshotContext {
            path_label: "Task path",
            path: &state.path,
            content: &state.content,
        },
    };
    issue::run_with_issue_snapshot(ctx, &base, Some(task.profile), false, prepared)
}

fn apply_matrix_workflow_result(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    result: Result<issue::IssueRunResult>,
    color: Option<&str>,
) -> Result<MatrixWorkflowTaskOutcome> {
    match result {
        Ok(result) => {
            record_matrix_workflow_success(ctx, state, result, color)?;
            Ok(MatrixWorkflowTaskOutcome::Started)
        }
        Err(err) if is_cancelled(&err) => {
            record_matrix_workflow_failure(ctx, state, task_run::STATUS_SKIPPED, "User cancelled")?;
            Ok(MatrixWorkflowTaskOutcome::Cancelled)
        }
        Err(err) => {
            let message = err.to_string();
            record_matrix_workflow_failure(ctx, state, STATUS_FAILED, &message)?;
            Ok(MatrixWorkflowTaskOutcome::Failed)
        }
    }
}

fn record_matrix_workflow_success(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    result: issue::IssueRunResult,
    color: Option<&str>,
) -> Result<()> {
    if state.document.branch != result.canonical_branch_name {
        task_store::write_task_branch(ctx, &state.row.task, &result.canonical_branch_name)?;
    }
    let profile = state.profile.as_deref().unwrap_or("<missing-profile>");
    update_workflow_profile_task_run(
        ctx,
        &state.row,
        profile,
        &state.run_id,
        STATUS_RUNNING,
        Some(&result.branch_name),
        None,
    )?;
    apply_workflow_color(ctx, &result.worktree_path, color);
    ctx.ui
        .print_step(&started_workflow_task_message(&matrix_task_label(
            &state.row.task,
            profile,
        )));
    Ok(())
}

fn record_matrix_workflow_failure(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    status: task_run::TaskRunStatus,
    error: &str,
) -> Result<()> {
    let profile = state.profile.as_deref().unwrap_or("<missing-profile>");
    ctx.ui.print_warning(&failed_workflow_task_message(
        &matrix_task_label(&state.row.task, profile),
        error,
    ));
    update_workflow_profile_task_run(
        ctx,
        &state.row,
        profile,
        &state.run_id,
        status,
        None,
        Some(error),
    )?;
    Ok(())
}

fn matrix_task_label(task: &str, profile: &str) -> String {
    format!("{task}:{profile}")
}
