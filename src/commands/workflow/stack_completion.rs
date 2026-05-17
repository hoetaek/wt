use super::resolve_mutating_target;
use crate::context::Ctx;
use crate::services::git::GitService;
use crate::task as task_store;
use crate::task_run::STATUS_DONE;
use crate::workflow as workflow_store;
use crate::workflow::render::workflow_task_label;
use crate::workflow::run::{
    read_stack_workflow_task_states, run_workflow, update_workflow_task_run,
};
use crate::workflow::{WorkflowMode, WorkflowTask};
use anyhow::{Result, bail};

pub(super) fn complete_stack_workflow(
    ctx: &Ctx,
    workflow: &str,
    task: Option<&str>,
    run_next: bool,
) -> Result<()> {
    let path = resolve_mutating_target(ctx, workflow, "complete")?;
    let mut metadata = workflow_store::read(&path)?;
    if metadata.mode != WorkflowMode::Stack {
        bail!("wt workflow complete only supports mode stack");
    }

    let states = read_stack_workflow_task_states(ctx, &path, &metadata)?;
    let completable = states
        .iter()
        .filter(|state| state.run.is_stack_completable())
        .collect::<Vec<_>>();
    let Some(state) = completable.first().copied() else {
        ctx.ui.print_warning("No running workflow stack task found");
        return Ok(());
    };
    if completable.len() > 1 {
        bail!(
            "Multiple running workflow stack tasks found; run `wt workflow repair {workflow}` first"
        );
    }
    let idx = state.idx;

    if let Some(task) = task {
        let running = &metadata.tasks[idx];
        if !workflow_task_matches(ctx, running, task) {
            bail!(
                "Running workflow task is {}, but complete was requested for {task}",
                workflow_task_label(running)
            );
        }
    }

    validate_completable_stack_task(ctx, &metadata.tasks[idx])?;
    update_workflow_task_run(ctx, &metadata.tasks[idx], STATUS_DONE, None)?;
    workflow_store::touch(&mut metadata);
    workflow_store::write(ctx, &path, &mut metadata)?;

    ctx.ui.print_step(&format!(
        "Marked {} done",
        workflow_task_label(&metadata.tasks[idx])
    ));
    if run_next {
        run_workflow(ctx, &path, 1)?;
    }
    Ok(())
}

fn workflow_task_matches(ctx: &Ctx, row: &WorkflowTask, target: &str) -> bool {
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

fn validate_completable_stack_task(ctx: &Ctx, row: &WorkflowTask) -> Result<()> {
    let task_doc = task_store::read_task_document(ctx, &row.task)?;
    let branch = task_store::prepared_branch_name(&task_doc.branch).ok_or_else(|| {
        anyhow::anyhow!("Workflow task {} has no branch", workflow_task_label(row))
    })?;
    let parent = row.parent.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Workflow task {} has no parent", workflow_task_label(row))
    })?;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    if let Some(path) = git.checked_out_path(branch)? {
        let status = git.status_porcelain(&path)?;
        let relevant_status = relevant_worktree_status(ctx, &status);
        if !relevant_status.trim().is_empty() {
            bail!(
                "Workflow task {} has uncommitted changes in {}. Commit or stash them before completing.\n{}",
                workflow_task_label(row),
                path.display(),
                relevant_status.trim_end()
            );
        }
    }

    if !git.branch_has_commits_ahead(parent, branch)? {
        bail!(
            "Workflow task {} has no commits ahead of parent {parent}. Commit the task work before completing.",
            workflow_task_label(row)
        );
    }

    Ok(())
}

fn relevant_worktree_status(ctx: &Ctx, status: &str) -> String {
    status
        .lines()
        .filter(|line| !is_configured_link_status_line(ctx, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_configured_link_status_line(ctx: &Ctx, line: &str) -> bool {
    let Some(path) = porcelain_status_path(line) else {
        return false;
    };

    ctx.config
        .worktree
        .link
        .iter()
        .map(|linked| linked.trim_end_matches('/'))
        .any(|linked| path == linked || path.starts_with(&format!("{linked}/")))
}

fn porcelain_status_path(line: &str) -> Option<&str> {
    let path = line.get(3..)?.trim();
    let path = path.rsplit(" -> ").next().unwrap_or(path);
    Some(path.trim_matches('"'))
}
