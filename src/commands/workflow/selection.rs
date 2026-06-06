use super::resolve_mutating_target;
use crate::context::{Ctx, PromptItem, PromptRow};
use crate::task_run::STATUS_PREPARED;
use crate::workflow as workflow_store;
use crate::workflow::planner::{RunnableWorkflowInfo, runnable_workflow_info};
use crate::workflow::render::{
    shell_arg, workflow_relative_path, workflow_selection_status_counts, workflow_title_label,
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
    creation_order: Option<u64>,
    item: PromptItem,
    group: String,
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
        _ if !ctx.ui.can_prompt() => bail!("{}", runnable_workflows_message(ctx, &candidates)),
        _ => {
            let rows = workflow_candidate_rows(&candidates);
            let idx = ctx.ui.select_rows("Workflow to run", &rows)?;
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
        let item = workflow_selection_item(ctx, &id, &workflow, &states, &info);
        let group = workflow_selection_group(&workflow.mode);
        let label = item.render_plain();
        candidates.push(RunnableWorkflowCandidate {
            id,
            path,
            creation_order: workflow_candidate_creation_order(&states),
            item,
            group,
            label,
        });
    }

    candidates.sort_by(compare_runnable_workflow_candidates);
    Ok(candidates)
}

fn compare_runnable_workflow_candidates(
    left: &RunnableWorkflowCandidate,
    right: &RunnableWorkflowCandidate,
) -> std::cmp::Ordering {
    match (left.creation_order, right.creation_order) {
        (Some(left_order), Some(right_order)) => left_order
            .cmp(&right_order)
            .then_with(|| compare_runnable_workflow_candidate_fallbacks(left, right)),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => compare_runnable_workflow_candidate_fallbacks(left, right),
    }
}

fn compare_runnable_workflow_candidate_fallbacks(
    left: &RunnableWorkflowCandidate,
    right: &RunnableWorkflowCandidate,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.label.cmp(&right.label))
}

fn workflow_candidate_creation_order(states: &[WorkflowTaskState]) -> Option<u64> {
    states
        .iter()
        .filter_map(|state| state.run.creation_order)
        .min()
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
    workflow_id: &str,
    metadata: &WorkflowMetadata,
    states: &[WorkflowTaskState],
    info: &RunnableWorkflowInfo,
) -> PromptItem {
    let mut fields = vec![metadata.mode.as_str().to_string()];
    match metadata.mode {
        WorkflowMode::Single | WorkflowMode::Batch | WorkflowMode::Matrix => {
            fields.push(format!("runnable {}", info.runnable_count));
            if states
                .iter()
                .any(|state| state.run.status != STATUS_PREPARED)
            {
                fields.push(workflow_selection_status_counts(states));
            }
        }
        WorkflowMode::Stack => {
            if let Some(next_idx) = info.next_idx {
                let state = &states[next_idx];
                let mut next = format!("next {}", state.row.task);
                if state.run.status != STATUS_PREPARED {
                    next.push_str(&format!(" [{}]", state.run.status));
                }
                fields.push(next);
            }
        }
    }
    if let Some(profile) = metadata.profile.as_deref() {
        fields.push(format!("profile {profile}"));
    }
    if !metadata.profiles.is_empty() {
        fields.push(format!("profiles {}", metadata.profiles.join(",")));
    }

    PromptItem::from_hint_parts(workflow_title_label(ctx, workflow_id, metadata), fields)
}

fn workflow_candidate_rows(candidates: &[RunnableWorkflowCandidate]) -> Vec<PromptRow> {
    let mut rows = Vec::new();
    let mut groups = Vec::<String>::new();
    for candidate in candidates {
        if !groups.contains(&candidate.group) {
            groups.push(candidate.group.clone());
        }
    }

    for group in groups {
        rows.push(PromptRow::section(group.clone()));
        for (index, candidate) in candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.group == group)
        {
            rows.push(PromptRow::from_indexed_item(index, candidate.item.clone()));
        }
    }
    rows
}

fn workflow_selection_group(mode: &WorkflowMode) -> String {
    match mode {
        WorkflowMode::Single => "single workflows",
        WorkflowMode::Batch => "batch workflows",
        WorkflowMode::Stack => "stack workflows",
        WorkflowMode::Matrix => "matrix workflows",
    }
    .to_string()
}

fn runnable_workflows_message(ctx: &Ctx, candidates: &[RunnableWorkflowCandidate]) -> String {
    let mut rows = candidates
        .iter()
        .take(10)
        .map(|candidate| {
            format!(
                "  wt run workflow {}  # {}",
                shell_arg(&candidate.id),
                workflow_relative_path(ctx, &candidate.path)
            )
        })
        .collect::<Vec<_>>();

    if candidates.len() > rows.len() {
        rows.push(format!("  ...(+{} more)", candidates.len() - rows.len()));
    }

    let heading = if candidates.len() == 1 {
        "Runnable workflow found; pass it explicitly:"
    } else {
        "Multiple runnable workflows found; pass one explicitly:"
    };

    format!("{heading}\n{}", rows.join("\n"))
}
