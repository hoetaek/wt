use super::resolve_mutating_target;
use crate::context::{Ctx, PromptItem};
use crate::workflow as workflow_store;
use crate::workflow::planner::{RunnableWorkflowInfo, runnable_workflow_info};
use crate::workflow::render::{
    base_label, shell_arg, workflow_filtered_task_summary, workflow_relative_path,
    workflow_selection_status_counts, workflow_task_title_label,
};
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states,
};
use crate::workflow::{WorkflowMetadata, WorkflowMode};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub(super) struct RunnableWorkflowCandidate {
    pub(super) id: String,
    pub(super) path: PathBuf,
    item: PromptItem,
    label: String,
}

pub(super) fn resolve_run_workflow_path(
    ctx: &Ctx,
    workflow: Option<&str>,
) -> Result<Option<PathBuf>> {
    match workflow {
        Some(target) => Ok(Some(resolve_mutating_target(ctx, target, "run")?)),
        None => select_runnable_workflow_path(ctx),
    }
}

fn select_runnable_workflow_path(ctx: &Ctx) -> Result<Option<PathBuf>> {
    let candidates = list_runnable_workflow_candidates(ctx)?;
    match candidates.len() {
        0 => {
            ctx.ui.print_warning("No runnable workflows found");
            Ok(None)
        }
        1 => Ok(Some(candidates[0].path.clone())),
        _ if !ctx.ui.can_prompt() => {
            bail!("{}", multiple_runnable_workflows_message(ctx, &candidates))
        }
        _ => {
            let items = candidates
                .iter()
                .map(|candidate| candidate.item.clone())
                .collect::<Vec<_>>();
            let idx = ctx.ui.select_items("Workflow to run", &items)?;
            let candidate = candidates
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("Selected workflow index out of range: {idx}"))?;
            Ok(Some(candidate.path.clone()))
        }
    }
}

pub(super) fn list_runnable_workflow_candidates(
    ctx: &Ctx,
) -> Result<Vec<RunnableWorkflowCandidate>> {
    let mut candidates = Vec::new();
    for path in workflow_store::workflow_paths(ctx)? {
        let id = workflow_store::id_from_path(&path)?;
        let workflow = match workflow_store::read(&path) {
            Ok(workflow) => workflow,
            Err(err) => {
                warn_skipped_workflow(ctx, &path, &err);
                continue;
            }
        };
        let states = match read_workflow_candidate_states(ctx, &path, &workflow) {
            Ok(states) => states,
            Err(err) => {
                warn_skipped_workflow_state(ctx, &path, &err);
                continue;
            }
        };
        let Some(info) = runnable_workflow_info(&workflow.mode, &states) else {
            continue;
        };
        let item = workflow_selection_item(ctx, &path, &id, &workflow, &states, &info);
        let label = item.render_plain();
        candidates.push(RunnableWorkflowCandidate {
            id,
            path,
            item,
            label,
        });
    }

    candidates.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(candidates)
}

fn warn_skipped_workflow(ctx: &Ctx, path: &Path, err: &anyhow::Error) {
    ctx.ui.print_warning(&format!(
        "Skipping unreadable workflow {}: {}",
        workflow_relative_path(ctx, path),
        first_error_line(err)
    ));
}

fn warn_skipped_workflow_state(ctx: &Ctx, path: &Path, err: &anyhow::Error) {
    ctx.ui.print_warning(&format!(
        "Skipping workflow with unreadable state {}: {}",
        workflow_relative_path(ctx, path),
        first_error_line(err)
    ));
}

fn first_error_line(err: &anyhow::Error) -> String {
    format!("{err:#}")
        .lines()
        .next()
        .unwrap_or("unknown error")
        .to_string()
}

fn read_workflow_candidate_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    match metadata.mode {
        WorkflowMode::Single => read_single_workflow_task_states(ctx, workflow_path, metadata),
        WorkflowMode::Batch => read_batch_workflow_task_states(ctx, workflow_path, metadata),
        WorkflowMode::Stack => read_stack_workflow_task_states(ctx, workflow_path, metadata),
        WorkflowMode::Matrix => read_matrix_workflow_task_states(ctx, workflow_path, metadata),
    }
}

fn workflow_selection_item(
    ctx: &Ctx,
    workflow_path: &Path,
    workflow_id: &str,
    metadata: &WorkflowMetadata,
    states: &[WorkflowTaskState],
    info: &RunnableWorkflowInfo,
) -> PromptItem {
    let mut fields = vec![format!("mode {}", metadata.mode.as_str())];
    match metadata.mode {
        WorkflowMode::Single | WorkflowMode::Batch | WorkflowMode::Matrix => {
            fields.push(format!("{} runnable", info.runnable_count));
            fields.push(format!(
                "tasks {}",
                workflow_filtered_task_summary(ctx, states, |state| { state.run.is_runnable() })
                    .unwrap_or_else(|| "none".into())
            ));
        }
        WorkflowMode::Stack => {
            if let Some(next_idx) = info.next_idx {
                let state = &states[next_idx];
                fields.push(format!(
                    "next {} [{}]",
                    workflow_task_title_label(ctx, &state.row.task),
                    state.run.status
                ));
            }
        }
    }
    fields.push(format!(
        "status {}",
        workflow_selection_status_counts(states)
    ));
    fields.push(format!("base {}", base_label(metadata)));
    if let Some(profile) = metadata.profile.as_deref() {
        fields.push(format!("profile {profile}"));
    }
    if !metadata.profiles.is_empty() {
        fields.push(format!("profiles {}", metadata.profiles.join(",")));
    }
    fields.push(format!(
        "path {}",
        workflow_relative_path(ctx, workflow_path)
    ));

    PromptItem::from_hint_parts(workflow_id, fields)
}

fn multiple_runnable_workflows_message(
    ctx: &Ctx,
    candidates: &[RunnableWorkflowCandidate],
) -> String {
    let mut rows = candidates
        .iter()
        .take(10)
        .map(|candidate| {
            format!(
                "  wt workflow run {}  # {}",
                shell_arg(&candidate.id),
                workflow_relative_path(ctx, &candidate.path)
            )
        })
        .collect::<Vec<_>>();

    if candidates.len() > rows.len() {
        rows.push(format!("  ...(+{} more)", candidates.len() - rows.len()));
    }

    format!(
        "Multiple runnable workflows found; pass one explicitly:\n{}",
        rows.join("\n")
    )
}
