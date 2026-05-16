use crate::cli::{BaseMode, WorkflowModeArg};
use crate::commands::editor;
use crate::commands::issue;
use crate::commands::issue_selection;
use crate::commands::task::{self as task_command, PreparedTask};
use crate::commands::task_run::{
    self, STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::workflow as workflow_store;
use crate::workflow::{WorkflowMetadata, WorkflowMode, WorkflowTask};
use crate::worktree_naming;
use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

pub fn task(
    ctx: &Ctx,
    tasks: &[String],
    mode: WorkflowModeArg,
    profile: Option<&str>,
    base: &Option<String>,
    pull_request: bool,
) -> Result<()> {
    validate_profile(ctx, profile)?;
    validate_mode_options(mode, pull_request)?;
    let prepared_tasks = if tasks.is_empty() {
        task_command::select_local_tasks(ctx)?
            .into_iter()
            .map(|task| PreparedTask {
                key: task.key,
                branch: task.document.branch,
            })
            .collect()
    } else {
        task_command::prepare_named_tasks(ctx, tasks)?
    };
    write_prepared_workflow(ctx, mode, profile, base, prepared_tasks, pull_request)
}

pub fn issue(
    ctx: &Ctx,
    issues: &[String],
    mode: WorkflowModeArg,
    profile: Option<&str>,
    base: &Option<String>,
    pull_request: bool,
) -> Result<()> {
    validate_profile(ctx, profile)?;
    validate_mode_options(mode, pull_request)?;

    let selected_issues = if issues.is_empty() {
        issue_selection::select_issues(ctx, "Select issues for workflow")?
            .into_iter()
            .map(|issue| issue.identifier)
            .collect::<Vec<_>>()
    } else {
        issues.to_vec()
    };

    if selected_issues.is_empty() {
        ctx.ui.print_warning("No issues selected");
        return Ok(());
    }

    let prepared_tasks = task_command::prepare_issue_tasks(ctx, &selected_issues)?;
    write_prepared_workflow(ctx, mode, profile, base, prepared_tasks, pull_request)
}

pub fn show(ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let path = resolve_read_target(ctx, workflow)?;
    let metadata = workflow_store::read(&path)?;
    let display_path = path
        .strip_prefix(&ctx.repo_root)
        .unwrap_or(&path)
        .display()
        .to_string();

    ctx.ui.print_step(&format!("Workflow: {display_path}"));
    ctx.ui
        .print_dim(&format!("  Mode: {}", metadata.mode.as_str()));
    ctx.ui
        .print_dim(&format!("  Base: {}", base_label(&metadata)));
    if let Some(profile) = metadata.profile.as_deref() {
        ctx.ui.print_dim(&format!("  Profile: {profile}"));
    }
    if let Some(color) = metadata.color.as_deref() {
        ctx.ui.print_dim(&format!("  Color: {color}"));
    }
    ctx.ui
        .print_dim(&format!("  Tasks: {}", metadata.tasks.len()));

    for (idx, item) in metadata.tasks.iter().enumerate() {
        let run = task_run_record(ctx, &item.run);
        let status = run
            .as_ref()
            .map(|run| run.status.as_str())
            .unwrap_or("missing");
        let task_doc = task_command::read_task_document(ctx, &item.task);
        let title = task_doc
            .as_ref()
            .map(|document| document.title_or_key(&item.task))
            .unwrap_or_else(|_| item.task.clone());
        ctx.ui.print_dim(&format!(
            "  {}. {} [{}] {}",
            idx + 1,
            item.task,
            status,
            title
        ));
        ctx.ui.print_dim(&format!(
            "     Task: {}",
            task_command::task_relative_path(&item.task)
        ));
        match task_doc {
            Ok(document) => {
                if !document.branch.trim().is_empty() {
                    ctx.ui
                        .print_dim(&format!("     Branch: {}", document.branch));
                }
            }
            Err(err) => ctx.ui.print_dim(&format!("     Task error: {err}")),
        }
        if let Some(parent) = item.parent.as_deref() {
            ctx.ui.print_dim(&format!("     Parent: {parent}"));
        }
        if metadata.mode == WorkflowMode::Stack {
            ctx.ui.print_dim(&format!(
                "     Pull request: {}",
                if item.pull_request.unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                }
            ));
        }
        if let Some(error) = run.and_then(|run| run.error) {
            if !error.trim().is_empty() {
                ctx.ui.print_dim(&format!("     Error: {error}"));
            }
        }
    }
    Ok(())
}

pub fn edit(ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let path = resolve_read_target(ctx, workflow)?;
    editor::open_file(ctx, &path)
}

pub fn run(ctx: &Ctx, workflow: Option<&str>, jobs: usize) -> Result<()> {
    let Some(workflow) = workflow else {
        bail!(
            "wt workflow run requires a workflow id or path until runnable selection is implemented"
        );
    };
    let path = resolve_mutating_target(ctx, workflow, "run")?;
    let mut metadata = workflow_store::read(&path)?;
    match metadata.mode {
        WorkflowMode::Single => run_single_workflow(ctx, &path, &mut metadata),
        WorkflowMode::Batch => run_batch_workflow(ctx, &path, &mut metadata, jobs),
        WorkflowMode::Stack => run_stack_workflow(ctx, &path, &mut metadata),
    }
}

pub fn complete(ctx: &Ctx, workflow: &str, task: Option<&str>, run_next: bool) -> Result<()> {
    let path = resolve_mutating_target(ctx, workflow, "complete")?;
    let mut metadata = workflow_store::read(&path)?;
    if metadata.mode != WorkflowMode::Stack {
        bail!("wt workflow complete only supports mode stack");
    }

    let states = read_stack_workflow_task_states(ctx, &path, &metadata)?;
    let Some(state) = states
        .iter()
        .find(|state| state.run.status == STATUS_RUNNING)
    else {
        ctx.ui.print_warning("No running workflow stack task found");
        return Ok(());
    };
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

    validate_completable_workflow_stack_task(ctx, &metadata.tasks[idx])?;
    update_workflow_task_run(ctx, &metadata.tasks[idx], STATUS_DONE, None)?;
    workflow_store::touch(&mut metadata);
    workflow_store::write(ctx, &path, &mut metadata)?;

    ctx.ui.print_step(&format!(
        "Marked {} done",
        workflow_task_label(&metadata.tasks[idx])
    ));
    if run_next {
        run(ctx, Some(path.to_string_lossy().as_ref()), 1)?;
    }
    Ok(())
}

fn write_prepared_workflow(
    ctx: &Ctx,
    mode: WorkflowModeArg,
    profile: Option<&str>,
    base: &Option<String>,
    prepared_tasks: Vec<PreparedTask>,
    pull_request: bool,
) -> Result<()> {
    if prepared_tasks.is_empty() {
        ctx.ui.print_warning("No tasks selected");
        return Ok(());
    }

    validate_single_mode_branches(mode, &prepared_tasks)?;
    let resolved_base = resolve_workflow_base(ctx, base)?;
    let workflow_path = workflow_store::next_available_path(ctx)?;
    let prepared = workflow_tasks_from_prepared(
        ctx,
        mode,
        &workflow_path,
        &resolved_base,
        prepared_tasks,
        pull_request,
    )?;

    let mut metadata = WorkflowMetadata::new(
        workflow_mode(mode),
        "explicit",
        Some(resolved_base),
        prepared.tasks,
    );
    metadata.profile = profile.map(str::to_string);

    if let Err(err) = workflow_store::write(ctx, &workflow_path, &mut metadata) {
        rollback_task_runs(&prepared.task_runs);
        return Err(err);
    }

    ctx.ui
        .print_step(&format!("Prepared workflow: {}", workflow_path.display()));
    Ok(())
}

fn run_single_workflow(
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

    let states = read_single_workflow_task_states(ctx, workflow_path, metadata)?;
    if states.is_empty() {
        bail!("Workflow has no tasks: {}", workflow_path.display());
    }
    if states
        .iter()
        .any(|state| !is_runnable_status(&state.run.status))
    {
        bail!("single mode workflow can only run prepared or failed TaskRuns");
    }

    let base = workflow_base_raw(metadata)?;
    let result = if states.len() == 1 {
        run_single_workflow_task(ctx, &states[0], &base, metadata.profile.as_deref())
    } else {
        run_single_workflow_group(ctx, &states, &base, metadata.profile.as_deref())
    };

    let result = match result {
        Ok(result) => result,
        Err(err) => {
            mark_single_workflow_failed(ctx, &states, &err);
            return Err(err);
        }
    };

    for state in &states {
        if state.document.branch != result.branch_name {
            task_command::write_task_branch(ctx, &state.row.task, &result.branch_name)?;
        }
        task_run::update(
            ctx,
            &state.row.run,
            STATUS_RUNNING,
            Some(&result.branch_name),
            None,
        )?;
    }
    apply_workflow_color(ctx, &result.worktree_path, metadata.color.as_deref());
    Ok(())
}

#[derive(Clone, Debug)]
struct WorkflowTaskState {
    idx: usize,
    row: WorkflowTask,
    document: task_command::TaskDocument,
    path: String,
    content: String,
    run: task_run::TaskRun,
}

fn read_single_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let states = read_workflow_task_states(ctx, workflow_path, metadata)?;
    for state in &states {
        validate_workflow_task_run_source(&state.row, &state.run, task_run::SOURCE_NEW)?;
    }
    Ok(states)
}

fn read_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let group = task_run::group_from_path(workflow_path)?;
    metadata
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let (document, path, content) = task_command::read_task_file(ctx, &row.task)?;
            let run_path = task_run::resolve(ctx, &row.run).with_context(|| {
                format!(
                    "Workflow task {} references missing TaskRun {}",
                    row.task, row.run
                )
            })?;
            let run = task_run::read(&run_path)?;
            validate_workflow_task_run(row, &run)?;
            validate_workflow_task_run_group(row, &run, &group)?;
            Ok(WorkflowTaskState {
                idx,
                row: row.clone(),
                document,
                path,
                content,
                run,
            })
        })
        .collect()
}

fn validate_workflow_task_run(row: &WorkflowTask, run: &task_run::TaskRun) -> Result<()> {
    if run.task != row.task {
        bail!(
            "Workflow task {} references TaskRun {} for task {}",
            row.task,
            row.run,
            run.task
        );
    }
    Ok(())
}

fn validate_workflow_task_run_group(
    row: &WorkflowTask,
    run: &task_run::TaskRun,
    group: &str,
) -> Result<()> {
    if run.group.as_deref() != Some(group) {
        bail!(
            "Workflow task {} references TaskRun {} outside workflow group {}",
            row.task,
            row.run,
            group
        );
    }
    Ok(())
}

fn validate_workflow_task_run_source(
    row: &WorkflowTask,
    run: &task_run::TaskRun,
    source: &str,
) -> Result<()> {
    if run.source != source {
        bail!(
            "Workflow task {} references TaskRun {} with source {}",
            row.task,
            row.run,
            run.source
        );
    }
    Ok(())
}

fn run_single_workflow_task(
    ctx: &Ctx,
    state: &WorkflowTaskState,
    base: &Option<String>,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let branch_name = task_command::prepared_branch_name(&state.document.branch);
    if branch_name.is_none() && state.document.origin.is_none() {
        bail!("Workflow task {} has no branch", state.row.task);
    }
    let identifier = state.document.identifier_or_key(&state.row.task);
    let title = state.document.title_or_key(&state.row.task);

    issue::run_with_issue_snapshot(
        ctx,
        base,
        profile,
        false,
        issue::PreparedIssueContext {
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
            workspace_label: None,
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task path",
                path: &state.path,
                content: &state.content,
            },
        },
    )
}

fn run_single_workflow_group(
    ctx: &Ctx,
    states: &[WorkflowTaskState],
    base: &Option<String>,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let branch = shared_single_workflow_branch(states)?;
    let snapshot_path = states
        .iter()
        .map(|state| state.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let snapshot_content = render_single_workflow_snapshot(states);
    let title = single_workflow_group_title(states);

    issue::run_with_issue_snapshot(
        ctx,
        base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &branch,
            title: &title,
            branch_name: Some(&branch),
            mode: "new",
            on_start_issue_id: None,
            prompt_intro: "Use these tasks before changing code. Work in this single workspace and address every selected TaskDocument.",
            workspace_label: None,
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task paths",
                path: &snapshot_path,
                content: &snapshot_content,
            },
        },
    )
}

fn shared_single_workflow_branch(states: &[WorkflowTaskState]) -> Result<String> {
    let branches = states
        .iter()
        .filter_map(|state| task_command::prepared_branch_name(&state.document.branch))
        .collect::<HashSet<_>>();
    let mut branches = branches.into_iter();
    let Some(branch) = branches.next() else {
        bail!("single mode workflow with multiple tasks requires a shared branch");
    };
    if branches.next().is_some() {
        bail!("single mode workflow with multiple tasks requires one shared branch");
    }
    Ok(branch.to_string())
}

fn render_single_workflow_snapshot(states: &[WorkflowTaskState]) -> String {
    let mut content = String::new();
    content.push_str("Selected TaskDocuments:\n");
    for state in states {
        content.push_str(&format!("- {}: {}\n", state.row.task, state.path));
    }
    for state in states {
        content.push_str(&format!("\n--- {} ({}) ---\n", state.row.task, state.path));
        content.push_str(state.content.trim_end());
        content.push('\n');
    }
    content
}

fn single_workflow_group_title(states: &[WorkflowTaskState]) -> String {
    let first = states
        .first()
        .map(|state| state.document.title_or_key(&state.row.task))
        .unwrap_or_else(|| "workflow".into());
    format!("{}개 작업: {first}", states.len())
}

fn workflow_base_raw(metadata: &WorkflowMetadata) -> Result<Option<String>> {
    match metadata.base_mode.as_str() {
        "explicit" => Ok(Some(metadata.base.clone().ok_or_else(|| {
            anyhow::anyhow!("Workflow base_mode is explicit but base is missing")
        })?)),
        other => bail!("workflow run only supports explicit base, found {other}"),
    }
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, STATUS_PREPARED | STATUS_FAILED)
}

fn mark_single_workflow_failed(ctx: &Ctx, states: &[WorkflowTaskState], err: &anyhow::Error) {
    let status = if is_cancelled(err) {
        task_run::STATUS_SKIPPED
    } else {
        task_run::STATUS_FAILED
    };
    let message = err.to_string();
    for state in states {
        let _ = task_run::update(ctx, &state.row.run, status, None, Some(&message));
    }
}

fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<WtError>()
        .is_some_and(|err| matches!(err, WtError::Cancelled))
}

fn apply_workflow_color(ctx: &Ctx, worktree_path: &Path, color: Option<&str>) {
    let Some(color) = color.map(str::trim).filter(|color| !color.is_empty()) else {
        return;
    };
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        return;
    }
    let target = comparable_path(worktree_path);
    let Ok(workspaces) = cmux.list_workspaces() else {
        return;
    };
    for workspace in workspaces {
        let Some(current_directory) = workspace.current_directory.as_deref() else {
            continue;
        };
        if comparable_path(current_directory) == target {
            let _ = cmux.set_color(&workspace.handle, color);
            break;
        }
    }
}

fn comparable_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn run_batch_workflow(
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
        .filter(|state| is_runnable_status(&state.run.status))
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

fn read_batch_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let states = read_workflow_task_states(ctx, workflow_path, metadata)?;
    for state in &states {
        validate_workflow_task_run_source(&state.row, &state.run, task_run::SOURCE_BATCH)?;
    }
    Ok(states)
}

fn run_batch_workflow_sequential(
    ctx: &Ctx,
    metadata: &WorkflowMetadata,
    states: Vec<WorkflowTaskState>,
    base: String,
) -> Result<bool> {
    let mut failed = false;
    let total = metadata.tasks.len();
    for state in states {
        ctx.ui.print_step(&format!("Starting {}", state.row.task));
        task_run::update(ctx, &state.row.run, STATUS_RUNNING, None, None)?;
        let result =
            run_batch_workflow_task(ctx, &state, &base, metadata.profile.as_deref(), true, total);
        if apply_batch_workflow_result(ctx, &state, result, metadata.color.as_deref())? {
            failed = true;
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
    let branch_name = task_command::prepared_branch_name(&state.document.branch);
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
    let branch_name = task_command::prepared_branch_name(&state.document.branch);
    if branch_name.is_none() && state.document.origin.is_none() {
        bail!("Workflow task {} has no branch", state.row.task);
    }
    let identifier = state.document.identifier_or_key(&state.row.task);
    let title = state.document.title_or_key(&state.row.task);
    let workspace_label = task_command::workspace_run_label(
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
) -> Result<bool> {
    match result {
        Ok(result) => {
            record_batch_workflow_success(ctx, state, result, color)?;
            Ok(false)
        }
        Err(err) if is_cancelled(&err) => {
            record_batch_workflow_failure(ctx, state, task_run::STATUS_SKIPPED, "User cancelled")?;
            Ok(true)
        }
        Err(err) => {
            let message = err.to_string();
            record_batch_workflow_failure(ctx, state, STATUS_FAILED, &message)?;
            Ok(true)
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
        task_command::write_task_branch(ctx, &state.row.task, &result.branch_name)?;
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
    status: &str,
    error: &str,
) -> Result<()> {
    ctx.ui
        .print_warning(&format!("Failed {}: {error}", state.row.task));
    task_run::update(ctx, &state.row.run, status, None, Some(error))?;
    Ok(())
}

fn run_stack_workflow(
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

    if let Some(state) = states
        .iter()
        .find(|state| state.run.status == STATUS_RUNNING)
    {
        bail!(
            "Workflow stack task {} is already running. Mark it complete with: wt workflow complete {} {}",
            workflow_task_label(&state.row),
            workflow_path.display(),
            workflow_task_label(&state.row)
        );
    }

    let Some(idx) = next_runnable_workflow_stack_task(&states) else {
        ctx.ui
            .print_step("No prepared or failed tasks to run in this workflow.");
        return Ok(());
    };

    let parent = parent_for_workflow_stack_task(metadata, &states, idx)?;
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

fn read_stack_workflow_task_states(
    ctx: &Ctx,
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let states = read_workflow_task_states(ctx, workflow_path, metadata)?;
    for state in &states {
        validate_workflow_task_run_source(&state.row, &state.run, task_run::SOURCE_STACK)?;
    }
    Ok(states)
}

fn next_runnable_workflow_stack_task(items: &[WorkflowTaskState]) -> Option<usize> {
    for item in items {
        match item.run.status.as_str() {
            STATUS_DONE | STATUS_SKIPPED => continue,
            status if is_runnable_status(status) => return Some(item.idx),
            _ => return None,
        }
    }
    None
}

fn parent_for_workflow_stack_task(
    metadata: &WorkflowMetadata,
    states: &[WorkflowTaskState],
    idx: usize,
) -> Result<String> {
    if idx == 0 {
        return workflow_base_raw(metadata)?
            .ok_or_else(|| anyhow::anyhow!("Workflow stack has no base"));
    }

    for previous in states.iter().rev().filter(|state| state.idx < idx) {
        match previous.run.status.as_str() {
            STATUS_DONE => {
                return task_command::prepared_branch_name(&previous.document.branch)
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
    let content = workflow_stack_task_prompt_content(&content, workflow_path, row);
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
            workspace_label: Some(workspace_label),
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task path",
                path: &task_path,
                content: &content,
            },
        },
    )
}

fn workflow_stack_task_prompt_content(
    content: &str,
    workflow_path: &Path,
    row: &WorkflowTask,
) -> String {
    let parent_branch = row.parent.as_deref().unwrap_or("<workflow-parent>");
    let pull_request = row.pull_request.unwrap_or(false);
    let pr_report_value = if pull_request { "<pr-url>" } else { "none" };
    let send_command = format!(
        "cmux send --workspace {{{{coordinator_cmux_workspace}}}} --surface {{{{coordinator_cmux_surface}}}} \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR={pr_report_value}; Risks or follow-ups=<risks>\"\n{{{{coordinator_enter_command}}}}"
    );
    let complete_command = format!(
        "wt workflow complete {} {} --run-next",
        shell_arg(&workflow_path.to_string_lossy()),
        shell_arg(workflow_task_label(row))
    );
    let pull_request_instructions = if pull_request {
        let pr_command = format!(
            "git push -u origin HEAD\ngh pr create --draft --base {} --fill",
            shell_arg(parent_branch)
        );
        format!(
            "Workflow task metadata sets `pull_request = true`. When this task is complete and committed, push the branch and open a draft pull request against the workflow parent branch:\n\n```bash\n{pr_command}\n```"
        )
    } else {
        "Workflow task metadata sets `pull_request = false`. When this task is complete and committed, do not open a pull request for this workflow task.".into()
    };

    format!(
        "{}\n\n## Workflow Coordinator Handoff\n\n{}\n\nThen send the Agent Completion Report back to the coordinator cmux surface that started this workflow:\n\n```bash\n{}\n```\n\nAfter sending the report, wait for the coordinator to review and advance the workflow. The coordinator will run:\n\n```bash\n{}\n```\n\nIf the coordinator cmux target is unavailable or stale, leave the same report in this task session and wait.",
        content.trim_end(),
        pull_request_instructions,
        send_command,
        complete_command
    )
}

fn workflow_task_label(row: &WorkflowTask) -> &str {
    if row.task.trim().is_empty() {
        "workflow-task"
    } else {
        row.task.trim()
    }
}

fn workflow_task_matches(ctx: &Ctx, row: &WorkflowTask, target: &str) -> bool {
    if row.task == target {
        return true;
    }
    let Ok(task_doc) = task_command::read_task_document(ctx, &row.task) else {
        return false;
    };
    task_doc.title == target
        || task_command::prepared_branch_name(&task_doc.branch) == Some(target)
        || task_doc.branch.rsplit('/').next() == Some(target)
}

fn update_workflow_task_run(
    ctx: &Ctx,
    row: &WorkflowTask,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let path = task_run::resolve(ctx, &row.run).with_context(|| {
        format!(
            "Workflow task {} references missing TaskRun {}",
            workflow_task_label(row),
            row.run
        )
    })?;
    let run = task_run::read(&path)?;
    validate_workflow_task_run(row, &run)?;

    let branch = task_command::read_task_document(ctx, &row.task)
        .ok()
        .map(|task| task.branch);
    let updated = task_run::update(ctx, &row.run, status, branch.as_deref(), error)?;
    validate_workflow_task_run(row, &updated.run)?;
    Ok(())
}

fn validate_completable_workflow_stack_task(ctx: &Ctx, row: &WorkflowTask) -> Result<()> {
    let task_doc = task_command::read_task_document(ctx, &row.task)?;
    let branch = task_command::prepared_branch_name(&task_doc.branch).ok_or_else(|| {
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

fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

struct PreparedWorkflowTasks {
    tasks: Vec<WorkflowTask>,
    task_runs: Vec<task_run::TaskRunRecord>,
}

fn workflow_tasks_from_prepared(
    ctx: &Ctx,
    mode: WorkflowModeArg,
    workflow_path: &Path,
    initial_parent: &str,
    prepared_tasks: Vec<PreparedTask>,
    pull_request: bool,
) -> Result<PreparedWorkflowTasks> {
    let group = task_run::group_from_path(workflow_path)?;
    let mut parent = Some(initial_parent.to_string());
    let mut tasks = Vec::new();
    let mut task_runs = Vec::new();
    for task in prepared_tasks {
        let run = match task_run::create(
            ctx,
            &task.key,
            &task.branch,
            source_for_mode(mode),
            Some(&group),
            STATUS_PREPARED,
        ) {
            Ok(run) => run,
            Err(err) => {
                rollback_task_runs(&task_runs);
                return Err(err);
            }
        };

        let mut row = WorkflowTask::new(task.key.clone(), run.id.clone());
        if mode == WorkflowModeArg::Stack {
            row.parent = parent.clone();
            row.pull_request = Some(pull_request);
            parent = task_command::prepared_branch_name(&task.branch).map(str::to_string);
        }
        task_runs.push(run);
        tasks.push(row);
    }
    Ok(PreparedWorkflowTasks { tasks, task_runs })
}

fn validate_mode_options(mode: WorkflowModeArg, pull_request: bool) -> Result<()> {
    if pull_request && mode != WorkflowModeArg::Stack {
        bail!("--pull-request is only valid with --mode stack");
    }
    Ok(())
}

fn validate_single_mode_branches(
    mode: WorkflowModeArg,
    prepared_tasks: &[PreparedTask],
) -> Result<()> {
    if mode != WorkflowModeArg::Single || prepared_tasks.len() <= 1 {
        return Ok(());
    }

    let branches = prepared_tasks
        .iter()
        .filter_map(|task| task_command::prepared_branch_name(&task.branch).map(str::to_string))
        .collect::<HashSet<_>>();
    if branches.len() > 1 {
        bail!(
            "single mode with multiple tasks requires the selected TaskDocuments to share one branch"
        );
    }
    Ok(())
}

fn rollback_task_runs(task_runs: &[task_run::TaskRunRecord]) {
    for run in task_runs.iter().rev() {
        let _ = task_run::delete_record(run);
    }
}

fn resolve_read_target(ctx: &Ctx, workflow: Option<&str>) -> Result<std::path::PathBuf> {
    workflow_store::resolve(ctx, workflow.unwrap_or("latest"))
}

fn resolve_mutating_target(ctx: &Ctx, workflow: &str, command: &str) -> Result<PathBuf> {
    if workflow == "latest" {
        bail!(
            "wt workflow {command} latest is not supported; pass a workflow path or id explicitly"
        );
    }
    workflow_store::resolve(ctx, workflow)
}

fn resolve_workflow_base(ctx: &Ctx, base: &Option<String>) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match BaseMode::from_raw(base) {
        BaseMode::Explicit(branch) => branch,
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            branches[idx].clone()
        }
        BaseMode::Current => git.current_branch()?,
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))?
        }
    };

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
}

fn validate_profile(ctx: &Ctx, profile: Option<&str>) -> Result<()> {
    let Some(profile) = profile else {
        return Ok(());
    };

    validate_profile_name(profile)?;
    if Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?.is_none() {
        bail!("Profile '{profile}' not found");
    }

    Ok(())
}

fn workflow_mode(mode: WorkflowModeArg) -> WorkflowMode {
    match mode {
        WorkflowModeArg::Single => WorkflowMode::Single,
        WorkflowModeArg::Batch => WorkflowMode::Batch,
        WorkflowModeArg::Stack => WorkflowMode::Stack,
    }
}

fn source_for_mode(mode: WorkflowModeArg) -> &'static str {
    match mode {
        WorkflowModeArg::Single => task_run::SOURCE_NEW,
        WorkflowModeArg::Batch => task_run::SOURCE_BATCH,
        WorkflowModeArg::Stack => task_run::SOURCE_STACK,
    }
}

fn base_label(metadata: &WorkflowMetadata) -> String {
    metadata
        .base
        .clone()
        .unwrap_or_else(|| format!("({})", metadata.base_mode))
}

fn task_run_record(ctx: &Ctx, run: &str) -> Option<task_run::TaskRun> {
    task_run::resolve(ctx, run)
        .and_then(|path| task_run::read(&path))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    #[test]
    fn task_prepares_batch_mode_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into(), "workflow state".into()],
            WorkflowModeArg::Batch,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let records = workflow_store::list(&ctx).unwrap();
        assert_eq!(records.len(), 1);
        let workflow = &records[0].workflow;
        assert_eq!(workflow.mode, WorkflowMode::Batch);
        assert_eq!(workflow.base.as_deref(), Some("main"));
        assert_eq!(workflow.tasks.len(), 2);
        assert!(workflow.tasks.iter().all(|row| row.parent.is_none()));
        assert!(workflow.tasks.iter().all(|row| row.pull_request.is_none()));
        assert_eq!(task_run::list(&ctx).unwrap().len(), 2);
    }

    #[test]
    fn task_without_args_multi_selects_existing_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("wire-api.toml"),
            "title = \"Wire API\"\nbranch = \"wire-api\"\n",
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 1]);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        task(
            &ctx,
            &[],
            WorkflowModeArg::Batch,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let workflow = workflow_store::list(&ctx).unwrap().remove(0).workflow;
        assert_eq!(workflow.mode, WorkflowMode::Batch);
        assert_eq!(workflow.tasks.len(), 2);
        assert_eq!(workflow.tasks[0].task, "add-schema");
        assert_eq!(workflow.tasks[1].task, "wire-api");
        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(
            runs.iter()
                .all(|record| record.run.source == task_run::SOURCE_BATCH)
        );
        assert!(
            runs.iter()
                .all(|record| record.run.status == STATUS_PREPARED)
        );
    }

    #[test]
    fn batch_workflow_state_reader_accepts_batch_task_runs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into(), "workflow state".into()],
            WorkflowModeArg::Batch,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        let states = read_batch_workflow_task_states(&ctx, &record.path, &record.workflow).unwrap();

        assert_eq!(states.len(), 2);
        assert!(
            states
                .iter()
                .all(|state| state.run.source == task_run::SOURCE_BATCH)
        );
    }

    #[test]
    fn single_workflow_state_reader_rejects_task_run_from_other_workflow_group() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Single,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let mut record = workflow_store::list(&ctx).unwrap().remove(0);
        let foreign_run = replace_first_workflow_run_with_foreign_group(
            &ctx,
            &mut record.workflow,
            task_run::SOURCE_NEW,
        );

        let err =
            read_single_workflow_task_states(&ctx, &record.path, &record.workflow).unwrap_err();

        assert!(err.to_string().contains("outside workflow group"));
        assert!(err.to_string().contains(&foreign_run));
        assert!(
            err.to_string()
                .contains(&task_run::group_from_path(&record.path).unwrap())
        );
    }

    #[test]
    fn batch_workflow_state_reader_rejects_task_run_from_other_workflow_group() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into(), "workflow state".into()],
            WorkflowModeArg::Batch,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let mut record = workflow_store::list(&ctx).unwrap().remove(0);
        let foreign_run = replace_first_workflow_run_with_foreign_group(
            &ctx,
            &mut record.workflow,
            task_run::SOURCE_BATCH,
        );

        let err =
            read_batch_workflow_task_states(&ctx, &record.path, &record.workflow).unwrap_err();

        assert!(err.to_string().contains("outside workflow group"));
        assert!(err.to_string().contains(&foreign_run));
        assert!(
            err.to_string()
                .contains(&task_run::group_from_path(&record.path).unwrap())
        );
    }

    #[test]
    fn task_prepares_single_mode_workflow_with_new_task_runs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Single,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let workflow = workflow_store::list(&ctx).unwrap().remove(0).workflow;
        assert_eq!(workflow.mode, WorkflowMode::Single);
        assert_eq!(workflow.tasks.len(), 1);
        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run.source, task_run::SOURCE_NEW);
        assert_eq!(runs[0].run.status, STATUS_PREPARED);
    }

    #[test]
    fn task_prepares_stack_mode_workflow_with_parents_and_pull_request() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["contract".into(), "state model".into()],
            WorkflowModeArg::Stack,
            None,
            &Some("main".into()),
            true,
        )
        .unwrap();

        let workflow = workflow_store::list(&ctx).unwrap().remove(0).workflow;
        assert_eq!(workflow.mode, WorkflowMode::Stack);
        assert!(workflow.profile.is_none());
        assert_eq!(workflow.tasks[0].parent.as_deref(), Some("main"));
        assert_eq!(workflow.tasks[0].pull_request, Some(true));
        assert_eq!(workflow.tasks[1].parent.as_deref(), Some("contract"));
        assert_eq!(workflow.tasks[1].pull_request, Some(true));
    }

    #[test]
    fn workflow_stack_parent_skips_skipped_tasks_when_finding_parent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["schema".into(), "api".into(), "ui".into()],
            WorkflowModeArg::Stack,
            None,
            &Some("main".into()),
            false,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        task_run::update(
            &ctx,
            &record.workflow.tasks[0].run,
            STATUS_DONE,
            Some("schema"),
            None,
        )
        .unwrap();
        task_run::update(
            &ctx,
            &record.workflow.tasks[1].run,
            STATUS_SKIPPED,
            Some("api"),
            Some("User cancelled"),
        )
        .unwrap();

        let states = read_stack_workflow_task_states(&ctx, &record.path, &record.workflow).unwrap();

        assert_eq!(
            parent_for_workflow_stack_task(&record.workflow, &states, 2).unwrap(),
            "schema"
        );
    }

    #[test]
    fn workflow_stack_prompt_uses_workflow_completion_and_pr_policy() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            pull_request: Some(true),
        };
        let workflow_path = PathBuf::from("/repo/.local/workflows/2026-05-16-001.toml");

        let content = workflow_stack_task_prompt_content("title = \"API\"\n", &workflow_path, &row);

        assert!(content.contains("## Workflow Coordinator Handoff"));
        assert!(content.contains("Workflow task metadata sets `pull_request = true`"));
        assert!(content.contains("gh pr create --draft --base PROJ-1 --fill"));
        assert!(content.contains(
            "wt workflow complete /repo/.local/workflows/2026-05-16-001.toml PROJ-2 --run-next"
        ));
        assert!(!content.contains("wt stack complete"));
    }

    #[test]
    fn pull_request_requires_stack_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let err = task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Batch,
            None,
            &Some("main".into()),
            true,
        )
        .unwrap_err();

        assert!(err.to_string().contains("--mode stack"));
    }

    fn replace_first_workflow_run_with_foreign_group(
        ctx: &Ctx,
        workflow: &mut WorkflowMetadata,
        source: &str,
    ) -> String {
        let row = workflow.tasks.first_mut().unwrap();
        let document = task_command::read_task_document(ctx, &row.task).unwrap();
        let run = task_run::create(
            ctx,
            &row.task,
            &document.branch,
            source,
            Some("foreign-workflow"),
            STATUS_PREPARED,
        )
        .unwrap();
        row.run = run.id.clone();
        run.id
    }
}
