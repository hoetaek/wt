use super::render::{workflow_stack_task_handoff_section, workflow_task_label};
use super::stack_plan::{next_runnable_stack_task, parent_for_stack_task};
use super::state::{read_stack_workflow_task_states, update_workflow_task_run};
use super::{apply_workflow_color, is_cancelled, validate_profile};
use crate::commands::issue;
use crate::commands::task as task_command;
use crate::commands::task_run::{self, STATUS_FAILED, STATUS_RUNNING, STATUS_SKIPPED};
use crate::context::Ctx;
use crate::workflow as workflow_store;
use crate::workflow::{WorkflowMetadata, WorkflowTask};
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

    if let Some(state) = states.iter().find(|state| state.run.is_stack_completable()) {
        bail!(
            "Workflow stack task {} is already running. Mark it complete with: wt workflow complete {} {}",
            workflow_task_label(&state.row),
            workflow_path.display(),
            workflow_task_label(&state.row)
        );
    }

    let Some(idx) = next_runnable_stack_task(&states) else {
        ctx.ui
            .print_step("No prepared or failed tasks to run in this workflow.");
        return Ok(());
    };

    let parent = parent_for_stack_task(metadata, &states, idx)?;
    metadata.tasks[idx].parent = Some(parent.clone());
    workflow_store::touch(metadata);
    workflow_store::write(ctx, workflow_path, metadata)?;
    task_run::update(ctx, &metadata.tasks[idx].run, STATUS_RUNNING, None, None)?;

    let result = run_stack_workflow_task(
        ctx,
        workflow_path,
        &metadata.tasks[idx],
        idx,
        metadata.tasks.len(),
        &parent,
        metadata.profile.as_deref(),
    );

    match result {
        Ok(result) => {
            task_command::write_task_branch(ctx, &metadata.tasks[idx].task, &result.branch_name)?;
            update_workflow_task_run(ctx, &metadata.tasks[idx], STATUS_RUNNING, None)?;
            apply_workflow_color(ctx, &result.worktree_path, metadata.color.as_deref());
            ctx.ui.print_step(&format!(
                "Started workflow task {}. Mark it complete with: wt workflow complete {} {}",
                workflow_task_label(&metadata.tasks[idx]),
                workflow_path.display(),
                workflow_task_label(&metadata.tasks[idx])
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

fn run_stack_workflow_task(
    ctx: &Ctx,
    workflow_path: &Path,
    row: &WorkflowTask,
    idx: usize,
    total_tasks: usize,
    parent: &str,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let (task_doc, task_path, content) = task_command::read_task_file(ctx, &row.task)?;
    let completion_section = workflow_stack_task_handoff_section(workflow_path, row);
    let branch_name = task_command::prepared_branch_name(&task_doc.branch);
    if branch_name.is_none() && task_doc.origin.is_none() {
        bail!("Workflow task {} has no branch", workflow_task_label(row));
    }
    let base = Some(parent.to_string());
    let identifier = task_doc.identifier_or_key(&row.task);
    let title = task_doc.title_or_key(&row.task);
    let workspace_label = task_command::workspace_run_label(
        idx,
        total_tasks,
        task_doc.origin.as_ref().map(|origin| origin.id.as_str()),
    );

    issue::run_with_issue_snapshot(
        ctx,
        &base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &identifier,
            title: &title,
            branch_name,
            mode: task_doc.mode(),
            on_start_issue_id: task_doc.origin.as_ref().map(|origin| origin.id.as_str()),
            prompt_intro: "Use this task before changing code.",
            completion_section: Some(&completion_section),
            workspace_label: Some(workspace_label),
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task path",
                path: &task_path,
                content: &content,
            },
        },
    )
}
