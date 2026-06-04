use crate::cli::{WorkflowModeArg, WorkflowPrModeArg};
use crate::config::{
    ReviewCodexBasePolicy, WorkflowDefaultLandingPolicy, WorkflowDefaultPolicy,
    WorkflowDefaultPullRequestMode,
};
use crate::task as task_store;
use crate::task::PreparedTask;
use crate::task_run::{STATUS_PASSED, STATUS_SKIPPED};
use crate::workflow::render::workflow_task_label;
use crate::workflow::run::WorkflowTaskState;
use crate::workflow::{
    WorkflowCodexBaseReview, WorkflowLandingPolicy, WorkflowMetadata, WorkflowMode, WorkflowPolicy,
    WorkflowPullRequestMode, WorkflowReviewPolicy,
};
use anyhow::{Result, bail};

pub(crate) struct RunnableWorkflowInfo {
    pub(crate) runnable_count: usize,
    pub(crate) next_idx: Option<usize>,
}

pub(crate) fn runnable_workflow_info(
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
        WorkflowMode::Matrix => {
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
            if states
                .iter()
                .any(|state| state.run.status.is_stack_completable())
            {
                return None;
            }
            next_runnable_stack_task(states).map(|next_idx| RunnableWorkflowInfo {
                runnable_count: 1,
                next_idx: Some(next_idx),
            })
        }
    }
}

pub(crate) fn next_runnable_stack_task(items: &[WorkflowTaskState]) -> Option<usize> {
    for item in items {
        match item.run.status {
            STATUS_PASSED | STATUS_SKIPPED => continue,
            status if status.is_runnable() => return Some(item.idx),
            _ => return None,
        }
    }
    None
}

pub(crate) fn parent_for_stack_task(
    metadata: &WorkflowMetadata,
    states: &[WorkflowTaskState],
    idx: usize,
) -> Result<String> {
    if idx == 0 {
        return workflow_base_raw(metadata)?
            .ok_or_else(|| anyhow::anyhow!("Workflow stack has no base"));
    }

    for previous in states.iter().rev().filter(|state| state.idx < idx) {
        match previous.run.status {
            STATUS_PASSED => {
                return task_store::prepared_branch_name(&previous.document.branch)
                    .map(str::to_string)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Previous workflow task {} has no branch",
                            workflow_task_label(&previous.row)
                        )
                    });
            }
            STATUS_SKIPPED => continue,
            _ => bail!(
                "Previous workflow task {} has not passed",
                workflow_task_label(&previous.row)
            ),
        }
    }

    workflow_base_raw(metadata)?.ok_or_else(|| anyhow::anyhow!("Workflow stack has no base"))
}

pub(crate) fn workflow_base_raw(metadata: &WorkflowMetadata) -> Result<Option<String>> {
    match metadata.base_mode.as_str() {
        "explicit" => Ok(Some(metadata.base.clone().ok_or_else(|| {
            anyhow::anyhow!("Workflow base_mode is explicit but base is missing")
        })?)),
        other => bail!("workflow run only supports explicit base, found {other}"),
    }
}

pub(crate) fn validate_single_mode_branches(
    mode: WorkflowModeArg,
    prepared_tasks: &[PreparedTask],
) -> Result<()> {
    if mode != WorkflowModeArg::Single || prepared_tasks.len() <= 1 {
        return Ok(());
    }

    let branches = prepared_tasks
        .iter()
        .filter_map(|task| task_store::prepared_branch_name(&task.branch).map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
    if branches.len() > 1 {
        bail!(
            "single mode with multiple tasks requires the selected TaskDocuments to share one branch"
        );
    }
    Ok(())
}

pub(crate) fn workflow_pr_mode(
    pr: Option<WorkflowPrModeArg>,
    default_policy: WorkflowDefaultPolicy,
) -> WorkflowPullRequestMode {
    match pr {
        Some(WorkflowPrModeArg::Draft) => WorkflowPullRequestMode::Draft,
        Some(WorkflowPrModeArg::Ready) => WorkflowPullRequestMode::Ready,
        Some(WorkflowPrModeArg::None) => WorkflowPullRequestMode::None,
        None => match default_policy.pull_request {
            WorkflowDefaultPullRequestMode::None => WorkflowPullRequestMode::None,
            WorkflowDefaultPullRequestMode::Draft => WorkflowPullRequestMode::Draft,
            WorkflowDefaultPullRequestMode::Ready => WorkflowPullRequestMode::Ready,
        },
    }
}

pub(crate) fn workflow_policy(
    default_policy: WorkflowDefaultPolicy,
    pull_request: WorkflowPullRequestMode,
) -> WorkflowPolicy {
    WorkflowPolicy {
        pull_request,
        landing: match default_policy.landing {
            WorkflowDefaultLandingPolicy::Manual => WorkflowLandingPolicy::Manual,
            WorkflowDefaultLandingPolicy::Auto => WorkflowLandingPolicy::Auto,
        },
        review: WorkflowReviewPolicy {
            codex_base: match default_policy.review.codex_base {
                ReviewCodexBasePolicy::None => WorkflowCodexBaseReview::None,
                ReviewCodexBasePolicy::Advisory => WorkflowCodexBaseReview::Advisory,
                ReviewCodexBasePolicy::Required => WorkflowCodexBaseReview::Required,
            },
        },
    }
}

pub(crate) fn workflow_mode(mode: WorkflowModeArg) -> WorkflowMode {
    match mode {
        WorkflowModeArg::Single => WorkflowMode::Single,
        WorkflowModeArg::Batch => WorkflowMode::Batch,
        WorkflowModeArg::Stack => WorkflowMode::Stack,
        WorkflowModeArg::Matrix => WorkflowMode::Matrix,
    }
}
