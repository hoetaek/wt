use crate::cli::{BaseMode, WorkflowModeArg};
use crate::commands::editor;
use crate::commands::issue;
use crate::commands::issue_selection;
use crate::commands::task::{self as task_command, PreparedTask};
use crate::commands::task_run::{self, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING};
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
    let prepared_tasks = task_command::prepare_named_tasks(ctx, tasks)?;
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
        let status = task_run_status(ctx, &item.run);
        let title = task_command::read_task_document(ctx, &item.task)
            .map(|document| document.title_or_key(&item.task))
            .unwrap_or_else(|_| item.task.clone());
        ctx.ui.print_dim(&format!(
            "  {}. {} [{}] {}",
            idx + 1,
            item.task,
            status,
            title
        ));
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
    let path = workflow_store::resolve(ctx, workflow)?;
    let mut metadata = workflow_store::read(&path)?;
    match metadata.mode {
        WorkflowMode::Single => run_single_workflow(ctx, &path, &mut metadata),
        WorkflowMode::Batch => run_batch_workflow(ctx, &path, &mut metadata, jobs),
        WorkflowMode::Stack => bail!(
            "wt workflow run is not implemented yet for mode stack; stack-mode execution will be added by the workflow stack-mode task"
        ),
    }
}

pub fn complete(_ctx: &Ctx, workflow: &str, _task: Option<&str>, _run_next: bool) -> Result<()> {
    bail!(
        "wt workflow complete is not implemented yet for workflow {workflow}; stack-mode completion will be added by the workflow stack-mode task"
    )
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

    let states = read_workflow_task_states(ctx, metadata)?;
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

#[derive(Clone)]
struct WorkflowTaskState {
    idx: usize,
    row: WorkflowTask,
    document: task_command::TaskDocument,
    path: String,
    content: String,
    run: task_run::TaskRun,
}

fn read_workflow_task_states(
    ctx: &Ctx,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
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
            validate_single_workflow_task_run(row, &run)?;
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

fn validate_single_workflow_task_run(row: &WorkflowTask, run: &task_run::TaskRun) -> Result<()> {
    if run.task != row.task {
        bail!(
            "Workflow task {} references TaskRun {} for task {}",
            row.task,
            row.run,
            run.task
        );
    }
    if run.source != task_run::SOURCE_NEW {
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
        other => bail!("single mode workflow run only supports explicit base, found {other}"),
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

    let states = read_batch_workflow_task_states(ctx, metadata)?;
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
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    let states = read_workflow_task_states(ctx, metadata)?;
    for state in &states {
        if state.run.source != task_run::SOURCE_BATCH {
            bail!(
                "Workflow task {} references TaskRun {} with source {}",
                state.row.task,
                state.row.run,
                state.run.source
            );
        }
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

fn task_run_status(ctx: &Ctx, run: &str) -> String {
    task_run::resolve(ctx, run)
        .and_then(|path| task_run::read(&path))
        .map(|run| run.status)
        .unwrap_or_else(|_| "missing".into())
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
}
