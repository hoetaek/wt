use super::render::workflow_task_label;
use super::state::WorkflowTaskState;
use super::workflow_base_raw;
use crate::task as task_store;
use crate::task_run::{STATUS_DONE, STATUS_SKIPPED};
use crate::workflow::WorkflowMetadata;
use anyhow::{Result, bail};

pub(super) fn next_runnable_stack_task(items: &[WorkflowTaskState]) -> Option<usize> {
    for item in items {
        match item.run.status {
            STATUS_DONE | STATUS_SKIPPED => continue,
            status if status.is_runnable() => return Some(item.idx),
            _ => return None,
        }
    }
    None
}

pub(super) fn parent_for_stack_task(
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
            STATUS_DONE => {
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
                "Previous workflow task {} is not done",
                workflow_task_label(&previous.row)
            ),
        }
    }

    workflow_base_raw(metadata)?.ok_or_else(|| anyhow::anyhow!("Workflow stack has no base"))
}
