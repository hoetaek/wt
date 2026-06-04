use crate::context::Ctx;
use crate::task as task_store;
use crate::workflow::WorkflowTask;
use crate::workflow::run::WorkflowTaskState;

pub(super) fn workflow_matrix_task_matches(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    target: &str,
) -> bool {
    let profile = state.profile.as_deref();
    if profile.is_some_and(|profile| target == profile) {
        return true;
    }
    if profile.is_some_and(|profile| target == format!("{}:{profile}", state.row.task)) {
        return true;
    }
    if target == state.run.branch {
        return true;
    }
    workflow_task_matches(ctx, &state.row, target)
}

pub(super) fn workflow_task_matches(ctx: &Ctx, row: &WorkflowTask, target: &str) -> bool {
    if row.task == target {
        return true;
    }
    let Ok(task_doc) = task_store::read_task_document(ctx, &row.task) else {
        return false;
    };
    task_doc.title == target
        || task_store::prepared_branch_name(&task_doc.branch) == Some(target)
        || task_doc.branch.rsplit('/').next() == Some(target)
}
