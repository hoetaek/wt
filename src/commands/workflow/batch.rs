use super::render::workflow_batch_task_handoff_section;
use super::state::{WorkflowTaskState, read_batch_workflow_task_states};
use super::{apply_workflow_color, is_cancelled, validate_profile, workflow_base_raw};
use crate::commands::issue;
use crate::context::Ctx;
use crate::task as task_store;
use crate::task_run::{self, STATUS_FAILED, STATUS_RUNNING};
use crate::workflow as workflow_store;
use crate::workflow::WorkflowMetadata;
use crate::worktree_naming;
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

pub(super) fn run_batch_workflow(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &mut WorkflowMetadata,
    jobs: usize,
) -> Result<()> {
    validate_profile(ctx, metadata.profile.as_deref())?;
    if metadata
        .color
        .as_deref()
        .is_none_or(|color| color.trim().is_empty())
    {
        workflow_store::write(ctx, workflow_path, metadata)?;
    }

    let states = read_batch_workflow_task_states(ctx, workflow_path, metadata)?;
    if states.is_empty() {
        bail!("Workflow has no tasks: {}", workflow_path.display());
    }
    let runnable = states
        .into_iter()
        .filter(|state| state.run.is_runnable())
        .collect::<Vec<_>>();
    if runnable.is_empty() {
        ctx.ui
            .print_step("No prepared or failed tasks to run in this workflow.");
        return Ok(());
    }

    let base = workflow_base_raw(metadata)?.expect("workflow base is validated");
    let failed = if jobs <= 1 {
        run_batch_workflow_sequential(ctx, metadata, runnable, base)?
    } else {
        run_batch_workflow_parallel(ctx, metadata, runnable, base, jobs)?
    };

    if failed {
        bail!("Workflow batch failed: {}", workflow_path.display());
    }
    Ok(())
}

fn run_batch_workflow_sequential(
    ctx: &Ctx,
    metadata: &WorkflowMetadata,
    states: Vec<WorkflowTaskState>,
    base: String,
) -> Result<bool> {
    let mut failed = false;
    let total = metadata.tasks.len();
    for (idx, state) in states.iter().enumerate() {
        ctx.ui.print_step(&format!("Starting {}", state.row.task));
        task_run::update(ctx, &state.row.run, STATUS_RUNNING, None, None)?;
        let result =
            run_batch_workflow_task(ctx, state, &base, metadata.profile.as_deref(), true, total);
        match apply_batch_workflow_result(ctx, state, result, metadata.color.as_deref())? {
            BatchWorkflowTaskOutcome::Started => {}
            BatchWorkflowTaskOutcome::Failed => failed = true,
            BatchWorkflowTaskOutcome::Cancelled => {
                failed = true;
                for skipped in states.iter().skip(idx + 1) {
                    record_batch_workflow_failure(
                        ctx,
                        skipped,
                        task_run::STATUS_SKIPPED,
                        "Skipped after user cancellation",
                    )?;
                }
                break;
            }
        }
    }
    Ok(failed)
}

fn run_batch_workflow_parallel(
    ctx: &Ctx,
    metadata: &WorkflowMetadata,
    states: Vec<WorkflowTaskState>,
    base: String,
    jobs: usize,
) -> Result<bool> {
    let mut failed = preflight_batch_workflow(ctx, &states, metadata.profile.as_deref())?;
    let runnable = states
        .into_iter()
        .filter(|state| {
            let failed = failed_indices(&failed);
            !failed.contains(&state.row.run)
        })
        .collect::<Vec<_>>();
    if runnable.is_empty() {
        return Ok(!failed.is_empty());
    }

    let worker_count = jobs.max(1);
    let (tx, rx) = mpsc::channel::<BatchWorkflowCompletion>();
    let mut next = 0;
    let mut active = 0;
    let mut cancelled = false;
    let total = metadata.tasks.len();

    thread::scope(|scope| -> Result<()> {
        loop {
            while !cancelled && active < worker_count && next < runnable.len() {
                let state = runnable[next].clone();
                ctx.ui.print_step(&format!("Starting {}", state.row.task));
                task_run::update(ctx, &state.row.run, STATUS_RUNNING, None, None)?;
                let tx = tx.clone();
                let base = base.clone();
                let profile = metadata.profile.clone();
                scope.spawn(move || {
                    let result = run_batch_workflow_task(
                        ctx,
                        &state,
                        &base,
                        profile.as_deref(),
                        false,
                        total,
                    );
                    let _ = tx.send(BatchWorkflowCompletion { state, result });
                });
                active += 1;
                next += 1;
            }

            if active == 0 {
                break;
            }

            let completion = rx
                .recv()
                .map_err(|_| anyhow::anyhow!("Workflow batch worker result channel closed"))?;
            active -= 1;
            match completion.result {
                Ok(result) => {
                    record_batch_workflow_success(
                        ctx,
                        &completion.state,
                        result,
                        metadata.color.as_deref(),
                    )?;
                }
                Err(err) if is_cancelled(&err) => {
                    record_batch_workflow_failure(
                        ctx,
                        &completion.state,
                        task_run::STATUS_SKIPPED,
                        "User cancelled",
                    )?;
                    cancelled = true;
                }
                Err(err) => {
                    let message = err.to_string();
                    record_batch_workflow_failure(ctx, &completion.state, STATUS_FAILED, &message)?;
                    failed.push(BatchWorkflowFailure {
                        run: completion.state.row.run.clone(),
                        error: message,
                    });
                }
            }
        }

        Ok(())
    })?;

    if cancelled {
        for state in runnable.iter().skip(next) {
            record_batch_workflow_failure(
                ctx,
                state,
                task_run::STATUS_SKIPPED,
                "Skipped after user cancellation",
            )?;
        }
    }

    Ok(!failed.is_empty() || cancelled)
}

#[derive(Clone)]
struct BatchWorkflowFailure {
    run: String,
    error: String,
}

struct BatchWorkflowCompletion {
    state: WorkflowTaskState,
    result: Result<issue::IssueRunResult>,
}

enum BatchWorkflowTaskOutcome {
    Started,
    Failed,
    Cancelled,
}

fn failed_indices(failures: &[BatchWorkflowFailure]) -> HashSet<String> {
    failures.iter().map(|failure| failure.run.clone()).collect()
}

fn preflight_batch_workflow(
    ctx: &Ctx,
    states: &[WorkflowTaskState],
    profile: Option<&str>,
) -> Result<Vec<BatchWorkflowFailure>> {
    let mut failures = Vec::new();
    let mut branches: HashMap<String, Vec<&WorkflowTaskState>> = HashMap::new();
    let mut paths: HashMap<PathBuf, Vec<&WorkflowTaskState>> = HashMap::new();

    for state in states {
        match batch_workflow_plan(ctx, state, profile) {
            Ok(plan) => {
                for branch in plan.branches {
                    branches.entry(branch).or_default().push(state);
                }
                for path in plan.paths {
                    paths.entry(path).or_default().push(state);
                }
            }
            Err(err) => failures.push(BatchWorkflowFailure {
                run: state.row.run.clone(),
                error: err.to_string(),
            }),
        }
    }

    for (branch, states) in branches {
        if states.len() <= 1 {
            continue;
        }
        for state in states {
            failures.push(BatchWorkflowFailure {
                run: state.row.run.clone(),
                error: format!(
                    "Multiple runnable workflow batch tasks target branch {branch}; adjust task branches before parallel run"
                ),
            });
        }
    }
    for (path, states) in paths {
        if states.len() <= 1 {
            continue;
        }
        for state in states {
            failures.push(BatchWorkflowFailure {
                run: state.row.run.clone(),
                error: format!(
                    "Multiple runnable workflow batch tasks target worktree path {}; adjust task branches before parallel run",
                    path.display()
                ),
            });
        }
    }

    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for failure in failures {
        if seen.insert(failure.run.clone()) {
            if let Some(state) = states.iter().find(|state| state.row.run == failure.run) {
                record_batch_workflow_failure(ctx, state, STATUS_FAILED, &failure.error)?;
            }
            deduped.push(failure);
        }
    }
    Ok(deduped)
}

struct BatchWorkflowPlan {
    branches: Vec<String>,
    paths: Vec<PathBuf>,
}

fn batch_workflow_plan(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    profile: Option<&str>,
) -> Result<BatchWorkflowPlan> {
    let branch_name = task_store::prepared_branch_name(&state.document.branch);
    if branch_name.is_none() && state.document.origin.is_none() {
        bail!("Workflow task {} has no branch", state.row.task);
    }
    let Some(branch_name) = branch_name else {
        return Ok(BatchWorkflowPlan {
            branches: Vec::new(),
            paths: Vec::new(),
        });
    };

    let identifier = state.document.identifier_or_key(&state.row.task);
    let title = state.document.title_or_key(&state.row.task);
    let naming = worktree_naming::generate(ctx, &identifier, &title, Some(branch_name))?;
    let plans = issue::planned_worktrees_for_prepared_issue(
        ctx,
        &title,
        branch_name,
        profile,
        naming.as_ref(),
    )?;
    for plan in &plans {
        if plan.path.exists() {
            bail!(
                "Worktree {} already exists; parallel workflow batch workers cannot prompt to delete or open it",
                plan.path.display()
            );
        }
    }
    Ok(BatchWorkflowPlan {
        branches: plans.iter().map(|plan| plan.branch_name.clone()).collect(),
        paths: plans.into_iter().map(|plan| plan.path).collect(),
    })
}

fn run_batch_workflow_task(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    base: &str,
    profile: Option<&str>,
    allow_interactive_prompts: bool,
    total: usize,
) -> Result<issue::IssueRunResult> {
    let completion_section = workflow_batch_task_handoff_section();
    let branch_name = task_store::prepared_branch_name(&state.document.branch);
    if branch_name.is_none() && state.document.origin.is_none() {
        bail!("Workflow task {} has no branch", state.row.task);
    }
    let identifier = state.document.identifier_or_key(&state.row.task);
    let title = state.document.title_or_key(&state.row.task);
    let workspace_label = task_store::workspace_run_label(
        state.idx,
        total,
        state
            .document
            .origin
            .as_ref()
            .map(|origin| origin.id.as_str()),
    );
    let base = Some(base.to_string());
    let prepared = issue::PreparedIssueContext {
        identifier: &identifier,
        title: &title,
        branch_name,
        mode: state.document.mode(),
        on_start_issue_id: state
            .document
            .origin
            .as_ref()
            .map(|origin| origin.id.as_str()),
        prompt_intro: "Use this task before changing code.",
        completion_section: Some(&completion_section),
        workspace_label: Some(workspace_label),
        snapshot: issue::IssueSnapshotContext {
            path_label: "Task path",
            path: &state.path,
            content: &state.content,
        },
    };
    if allow_interactive_prompts {
        issue::run_with_issue_snapshot(ctx, &base, profile, false, prepared)
    } else {
        issue::run_with_issue_snapshot_non_interactive(ctx, &base, profile, false, prepared)
    }
}

fn apply_batch_workflow_result(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    result: Result<issue::IssueRunResult>,
    color: Option<&str>,
) -> Result<BatchWorkflowTaskOutcome> {
    match result {
        Ok(result) => {
            record_batch_workflow_success(ctx, state, result, color)?;
            Ok(BatchWorkflowTaskOutcome::Started)
        }
        Err(err) if is_cancelled(&err) => {
            record_batch_workflow_failure(ctx, state, task_run::STATUS_SKIPPED, "User cancelled")?;
            Ok(BatchWorkflowTaskOutcome::Cancelled)
        }
        Err(err) => {
            let message = err.to_string();
            record_batch_workflow_failure(ctx, state, STATUS_FAILED, &message)?;
            Ok(BatchWorkflowTaskOutcome::Failed)
        }
    }
}

fn record_batch_workflow_success(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    result: issue::IssueRunResult,
    color: Option<&str>,
) -> Result<()> {
    if state.document.branch != result.branch_name {
        task_store::write_task_branch(ctx, &state.row.task, &result.branch_name)?;
    }
    task_run::update(
        ctx,
        &state.row.run,
        STATUS_RUNNING,
        Some(&result.branch_name),
        None,
    )?;
    apply_workflow_color(ctx, &result.worktree_path, color);
    ctx.ui.print_step(&format!("Started {}", state.row.task));
    Ok(())
}

fn record_batch_workflow_failure(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    status: task_run::TaskRunStatus,
    error: &str,
) -> Result<()> {
    ctx.ui
        .print_warning(&format!("Failed {}: {error}", state.row.task));
    task_run::update(ctx, &state.row.run, status, None, Some(error))?;
    Ok(())
}
