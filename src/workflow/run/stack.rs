use super::state::{read_stack_workflow_task_states, update_workflow_task_run};
use super::{apply_workflow_color, is_cancelled, task_issue_closing_references, validate_profile};
use crate::commands::issue;
use crate::config::AGENT_PROMPT_WORKFLOW_SCOPE;
use crate::context::Ctx;
use crate::setup;
use crate::task as task_store;
use crate::task_run::{self, STATUS_FAILED, STATUS_RUNNING, STATUS_SKIPPED};
use crate::workflow as workflow_store;
use crate::workflow::planner::{next_runnable_stack_task, parent_for_stack_task};
use crate::workflow::render::{
    no_runnable_workflow_tasks_message, stack_task_already_running_message,
    started_stack_task_message, workflow_metadata_prompt_context,
    workflow_stack_task_handoff_section, workflow_stack_task_prompt_intro, workflow_task_label,
};
use crate::workflow::{WorkflowMetadata, WorkflowPolicy, WorkflowTask};
use anyhow::{Result, bail};
use std::path::Path;

pub(super) fn run_stack_workflow(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &mut WorkflowMetadata,
) -> Result<()> {
    validate_profile(ctx, metadata.profile.as_deref())?;
    if metadata
        .color
        .as_deref()
        .is_none_or(|color| color.trim().is_empty())
    {
        workflow_store::write(ctx, workflow_path, metadata)?;
    }

    let states = read_stack_workflow_task_states(ctx, workflow_path, metadata)?;
    if states.is_empty() {
        bail!("Workflow has no tasks: {}", workflow_path.display());
    }

    if let Some(state) = states
        .iter()
        .find(|state| state.run.status.is_stack_completable())
    {
        bail!(
            "{}",
            stack_task_already_running_message(workflow_path, &state.row)
        );
    }

    let Some(idx) = next_runnable_stack_task(&states) else {
        ctx.ui.print_step(no_runnable_workflow_tasks_message());
        return Ok(());
    };

    let parent = parent_for_stack_task(metadata, &states, idx)?;
    metadata.tasks[idx].parent = Some(parent.clone());
    workflow_store::touch(metadata);
    workflow_store::write(ctx, workflow_path, metadata)?;
    task_run::update(ctx, &metadata.tasks[idx].run, STATUS_RUNNING, None, None)?;

    let result = run_stack_workflow_task(
        ctx,
        StackWorkflowTaskContext {
            workflow_path,
            row: &metadata.tasks[idx],
            policy: &metadata.policy,
            idx,
            total_tasks: metadata.tasks.len(),
            parent: &parent,
            profile: metadata.profile.as_deref(),
            workflow_context: workflow_metadata_prompt_context(metadata),
        },
    );

    match result {
        Ok(result) => {
            task_store::write_task_branch(ctx, &metadata.tasks[idx].task, &result.branch_name)?;
            update_workflow_task_run(ctx, &metadata.tasks[idx], STATUS_RUNNING, None)?;
            apply_workflow_color(ctx, &result.worktree_path, metadata.color.as_deref());
            ctx.ui.print_step(&started_stack_task_message(
                workflow_path,
                &metadata.tasks[idx],
            ));
            Ok(())
        }
        Err(err) if is_cancelled(&err) => {
            update_workflow_task_run(
                ctx,
                &metadata.tasks[idx],
                STATUS_SKIPPED,
                Some("User cancelled"),
            )?;
            Ok(())
        }
        Err(err) => {
            let error = err.to_string();
            update_workflow_task_run(ctx, &metadata.tasks[idx], STATUS_FAILED, Some(&error))?;
            bail!("Workflow stack failed: {}", workflow_path.display())
        }
    }
}

struct StackWorkflowTaskContext<'a> {
    workflow_path: &'a Path,
    row: &'a WorkflowTask,
    policy: &'a WorkflowPolicy,
    idx: usize,
    total_tasks: usize,
    parent: &'a str,
    profile: Option<&'a str>,
    workflow_context: Option<String>,
}

fn run_stack_workflow_task(
    ctx: &Ctx,
    task: StackWorkflowTaskContext<'_>,
) -> Result<issue::IssueRunResult> {
    let (task_doc, task_path, content) = task_store::read_task_file(ctx, &task.row.task)?;
    let completion_section = workflow_stack_task_handoff_section(
        task.workflow_path,
        task.row,
        task.policy,
        task.parent,
        &task_issue_closing_references(&task_doc),
    );
    let branch_name = task_store::prepared_branch_name(&task_doc.branch);
    if branch_name.is_none() && task_doc.origin.is_none() {
        bail!(
            "Workflow task {} has no branch",
            workflow_task_label(task.row)
        );
    }
    let base = Some(task.parent.to_string());
    let identifier = task_doc.identifier_or_key(&task.row.task);
    let title = task_doc.title_or_key(&task.row.task);
    let workspace_label = task_store::workspace_run_label(
        task.idx,
        task.total_tasks,
        task_doc.origin.as_ref().map(|origin| origin.id.as_str()),
    );

    issue::run_with_issue_snapshot(
        ctx,
        &base,
        task.profile,
        false,
        issue::PreparedIssueContext {
            identifier: &identifier,
            title: &title,
            branch_name,
            setup_mode: task_doc.setup_mode(),
            additional_prompt_scope: Some(AGENT_PROMPT_WORKFLOW_SCOPE),
            workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
            on_start_issue_id: task_doc.origin.as_ref().map(|origin| origin.id.as_str()),
            prompt_intro: workflow_stack_task_prompt_intro(),
            completion_section: Some(&completion_section),
            pre_snapshot_context: task.workflow_context.as_deref(),
            workspace_label: Some(workspace_label),
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task path",
                path: &task_path,
                content: &content,
            },
        },
    )
}
