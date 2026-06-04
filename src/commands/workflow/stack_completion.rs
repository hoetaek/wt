use super::resolve_mutating_target;
use crate::context::Ctx;
use crate::services::git::GitService;
use crate::task as task_store;
use crate::task_run::{REVIEW_ACCEPTED, STATUS_PASSED};
use crate::workflow as workflow_store;
use crate::workflow::render::{shell_arg, workflow_task_label};
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states, run_workflow,
    update_workflow_profile_task_run, update_workflow_task_run,
};
use crate::workflow::{WorkflowCodexBaseReview, WorkflowMetadata, WorkflowMode, WorkflowTask};
use anyhow::{Result, bail};

pub(super) fn pass_workflow(
    ctx: &Ctx,
    workflow: &str,
    task: Option<&str>,
    run_next: bool,
) -> Result<()> {
    let path = resolve_mutating_target(ctx, workflow, "pass")?;
    let mut metadata = workflow_store::read(&path)?;
    if run_next && metadata.mode != WorkflowMode::Stack {
        bail!("wt workflow pass --run-next only supports mode stack");
    }

    if metadata.mode == WorkflowMode::Matrix {
        return pass_matrix_workflow(ctx, &path, &mut metadata, workflow, task);
    }

    let states = read_completable_workflow_task_states(ctx, &path, &metadata)?;
    let pass_indices = pass_indices(ctx, workflow, task, &metadata, &states)?;
    if pass_indices.is_empty() {
        return Ok(());
    }

    for idx in &pass_indices {
        validate_required_codex_base_review(&metadata, &states[*idx])?;
    }
    if metadata.mode == WorkflowMode::Stack {
        for idx in &pass_indices {
            validate_completable_stack_task(ctx, &metadata.tasks[*idx])?;
        }
    }
    for idx in &pass_indices {
        update_workflow_task_run(ctx, &metadata.tasks[*idx], STATUS_PASSED, None)?;
    }
    workflow_store::touch(&mut metadata);
    workflow_store::write(ctx, &path, &mut metadata)?;

    for idx in pass_indices {
        ctx.ui.print_step(&format!(
            "Marked {} passed",
            workflow_task_label(&metadata.tasks[idx])
        ));
    }
    if run_next {
        run_workflow(ctx, &path, 1)?;
    }
    Ok(())
}

fn read_completable_workflow_task_states(
    ctx: &Ctx,
    path: &std::path::Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    match metadata.mode {
        WorkflowMode::Single => read_single_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Batch => read_batch_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Stack => read_stack_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Matrix => read_matrix_workflow_task_states(ctx, path, metadata),
    }
}

fn pass_matrix_workflow(
    ctx: &Ctx,
    path: &std::path::Path,
    metadata: &mut WorkflowMetadata,
    workflow: &str,
    task: Option<&str>,
) -> Result<()> {
    let states = read_matrix_workflow_task_states(ctx, path, metadata)?;
    let passed = pass_matrix_states(ctx, workflow, task, &states)?;
    if passed.is_empty() {
        return Ok(());
    }

    for state in &passed {
        validate_required_codex_base_review(metadata, state)?;
    }
    for state in &passed {
        let profile = state
            .profile
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("matrix workflow TaskRun is missing profile"))?;
        update_workflow_profile_task_run(
            ctx,
            &state.row,
            profile,
            &state.run_id,
            STATUS_PASSED,
            Some(&state.run.branch),
            None,
        )?;
    }
    workflow_store::touch(metadata);
    workflow_store::write(ctx, path, metadata)?;

    for state in passed {
        let profile = state.profile.as_deref().unwrap_or("<missing-profile>");
        ctx.ui.print_step(&format!(
            "Marked {}:{profile} passed",
            workflow_task_label(&state.row)
        ));
    }
    Ok(())
}

fn pass_matrix_states(
    ctx: &Ctx,
    workflow: &str,
    task: Option<&str>,
    states: &[WorkflowTaskState],
) -> Result<Vec<WorkflowTaskState>> {
    let running = states
        .iter()
        .filter(|state| state.run.status.is_stack_completable())
        .cloned()
        .collect::<Vec<_>>();
    if running.is_empty() {
        ctx.ui.print_warning("No running workflow task found");
        return Ok(Vec::new());
    }

    if let Some(task) = task {
        let matching = running
            .into_iter()
            .filter(|state| workflow_matrix_task_matches(ctx, state, task))
            .collect::<Vec<_>>();
        if matching.is_empty() {
            bail!("No running workflow task matches {task}");
        }
        if matching.len() > 1 {
            bail!(
                "Multiple running workflow profile tasks match {task}; pass a profile target like `wt workflow pass {workflow} <task>:<profile>`"
            );
        }
        return Ok(matching);
    }

    if running.len() > 1 {
        bail!(
            "Multiple running workflow profile tasks found; pass a profile target like `wt workflow pass {workflow} <task>:<profile>`"
        );
    }
    Ok(running)
}

fn pass_indices(
    ctx: &Ctx,
    workflow: &str,
    task: Option<&str>,
    metadata: &WorkflowMetadata,
    states: &[WorkflowTaskState],
) -> Result<Vec<usize>> {
    let running = states
        .iter()
        .filter(|state| state.run.status.is_stack_completable())
        .collect::<Vec<_>>();
    if running.is_empty() {
        ctx.ui.print_warning("No running workflow task found");
        return Ok(Vec::new());
    }

    if let Some(task) = task {
        let matching = running
            .into_iter()
            .filter(|state| workflow_task_matches(ctx, &state.row, task))
            .map(|state| state.idx)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            bail!("No running workflow task matches {task}");
        }
        return Ok(matching);
    }

    match metadata.mode {
        WorkflowMode::Single => Ok(running.into_iter().map(|state| state.idx).collect()),
        WorkflowMode::Batch | WorkflowMode::Stack | WorkflowMode::Matrix => {
            if running.len() > 1 {
                bail!(
                    "Multiple running workflow tasks found; pass a task to `wt workflow pass {workflow} <task>` or run `wt workflow repair {workflow}` first"
                );
            }
            Ok(vec![running[0].idx])
        }
    }
}

fn validate_required_codex_base_review(
    metadata: &WorkflowMetadata,
    state: &WorkflowTaskState,
) -> Result<()> {
    if metadata.policy.review.codex_base != WorkflowCodexBaseReview::Required {
        return Ok(());
    }
    if state.run.last_review_status == Some(REVIEW_ACCEPTED) {
        return Ok(());
    }

    let parent = codex_review_parent(metadata, &state.row)?;
    let status = state
        .run
        .last_review_status
        .map(|status| status.as_str())
        .unwrap_or("missing");
    bail!(
        "Workflow task {} requires Codex base review evidence before pass; last review status is {status}. Open a Codex surface and run `{}` against this task. For non-interactive runs, use `{}`. Then record acceptance with `wt task review {} --accept \"Codex base review passed against {}: <summary/evidence>\"` before running `wt workflow pass`.",
        workflow_task_label(&state.row),
        codex_surface_review_command(&parent),
        codex_cli_review_command(&parent),
        state.run_id,
        parent
    )
}

fn codex_review_parent(metadata: &WorkflowMetadata, row: &WorkflowTask) -> Result<String> {
    if let Some(parent) = row.parent.clone() {
        return Ok(parent);
    }
    match metadata.base_mode.as_str() {
        "explicit" => metadata
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Workflow base_mode is explicit but base is missing")),
        other => bail!("workflow pass only supports explicit base, found {other}"),
    }
}

fn codex_surface_review_command(parent: &str) -> String {
    format!("/review --base {}", shell_arg(parent))
}

fn codex_cli_review_command(parent: &str) -> String {
    format!("codex review --base {}", shell_arg(parent))
}

fn workflow_matrix_task_matches(ctx: &Ctx, state: &WorkflowTaskState, target: &str) -> bool {
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
    let parent = row.parent.clone().ok_or_else(|| {
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

    if !git.branch_has_commits_ahead(&parent, branch)? {
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
