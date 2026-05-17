use super::render::{
    base_label, shell_arg, workflow_filtered_task_summary, workflow_relative_path,
    workflow_selection_status_counts, workflow_task_title_label,
};
use super::resolve_mutating_target;
use super::state::{
    WorkflowTaskState, read_batch_workflow_task_states, read_single_workflow_task_states,
    read_stack_workflow_task_states,
};
use crate::commands::task_run::{STATUS_DONE, STATUS_SKIPPED};
use crate::context::{Ctx, PromptItem};
use crate::workflow as workflow_store;
use crate::workflow::{WorkflowMetadata, WorkflowMode};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub(super) struct RunnableWorkflowCandidate {
    pub(super) id: String,
    pub(super) path: PathBuf,
    item: PromptItem,
    label: String,
}

struct RunnableWorkflowInfo {
    runnable_count: usize,
    next_idx: Option<usize>,
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
    for record in workflow_store::list(ctx)? {
        let states = read_workflow_candidate_states(ctx, &record.path, &record.workflow)
            .with_context(|| {
                format!(
                    "Failed to read workflow task state: {}",
                    record.path.display()
                )
            })?;
        let Some(info) = runnable_workflow_info(&record.workflow.mode, &states) else {
            continue;
        };
        let item = workflow_selection_item(
            ctx,
            &record.path,
            &record.id,
            &record.workflow,
            &states,
            &info,
        );
        let label = item.render_plain();
        candidates.push(RunnableWorkflowCandidate {
            id: record.id,
            path: record.path,
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

fn read_workflow_candidate_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    match metadata.mode {
        WorkflowMode::Single => read_single_workflow_task_states(ctx, workflow_path, metadata),
        WorkflowMode::Batch => read_batch_workflow_task_states(ctx, workflow_path, metadata),
        WorkflowMode::Stack => read_stack_workflow_task_states(ctx, workflow_path, metadata),
    }
}

fn runnable_workflow_info(
    mode: &WorkflowMode,
    states: &[WorkflowTaskState],
) -> Option<RunnableWorkflowInfo> {
    match mode {
        WorkflowMode::Single => {
            if !states.is_empty() && states.iter().all(|state| state.run.is_runnable()) {
                Some(RunnableWorkflowInfo {
                    runnable_count: states.len(),
                    next_idx: None,
                })
            } else {
                None
            }
        }
        WorkflowMode::Batch => {
            let runnable_count = states
                .iter()
                .filter(|state| state.run.is_runnable())
                .count();
            (runnable_count > 0).then_some(RunnableWorkflowInfo {
                runnable_count,
                next_idx: None,
            })
        }
        WorkflowMode::Stack => {
            if states.iter().any(|state| state.run.is_stack_completable()) {
                return None;
            }
            next_runnable_workflow_stack_task(states).map(|next_idx| RunnableWorkflowInfo {
                runnable_count: 1,
                next_idx: Some(next_idx),
            })
        }
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
        WorkflowMode::Single | WorkflowMode::Batch => {
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

pub(super) fn next_runnable_workflow_stack_task(items: &[WorkflowTaskState]) -> Option<usize> {
    for item in items {
        match item.run.status {
            STATUS_DONE | STATUS_SKIPPED => continue,
            status if status.is_runnable() => return Some(item.idx),
            _ => return None,
        }
    }
    None
}
