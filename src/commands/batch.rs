use crate::cli::BaseMode;
use crate::commands::issue;
use crate::commands::issue_selection;
use crate::commands::task::{self, PreparedTask};
use crate::commands::task_run::{
    self, STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::git::GitService;
use crate::worktree_naming;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const STATUS_PARTIAL: &str = "partial";

pub fn issue(
    ctx: &Ctx,
    issues: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;

    let selected_issues = if issues.is_empty() {
        issue_selection::select_issues(ctx, "Select issues for batch")?
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

    let prepared_tasks = task::prepare_issue_tasks(ctx, &selected_issues)?;
    write_prepared_batch(ctx, profile, base, prepared_tasks)
}

pub fn task(
    ctx: &Ctx,
    tasks: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;
    let prepared_tasks = task::prepare_named_tasks(ctx, tasks)?;
    write_prepared_batch(ctx, profile, base, prepared_tasks)
}

fn write_prepared_batch(
    ctx: &Ctx,
    profile: Option<&str>,
    base: &Option<String>,
    prepared_tasks: Vec<PreparedTask>,
) -> Result<()> {
    if prepared_tasks.is_empty() {
        ctx.ui.print_warning("No tasks selected");
        return Ok(());
    }

    let resolved_base = resolve_batch_base(ctx, base)?;
    let batch_path = next_available_batch_path(ctx)?;
    write_prepared_batch_at_path(ctx, profile, &resolved_base, &batch_path, prepared_tasks)?;

    ctx.ui
        .print_step(&format!("Prepared batch: {}", batch_path.display()));
    Ok(())
}

fn write_prepared_batch_at_path(
    ctx: &Ctx,
    profile: Option<&str>,
    resolved_base: &str,
    batch_path: &Path,
    prepared_tasks: Vec<PreparedTask>,
) -> Result<()> {
    let prepared = batch_tasks_from_prepared(ctx, batch_path, prepared_tasks)?;
    let now = current_utc_timestamp();
    let batch = BatchMetadata {
        profile: profile.map(str::to_string),
        base_mode: "explicit".into(),
        base: Some(resolved_base.to_string()),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        tasks: prepared.tasks,
    };
    if let Err(err) = write_batch_metadata(batch_path, &batch) {
        rollback_task_runs(&prepared.task_runs);
        return Err(err);
    }
    Ok(())
}

struct PreparedBatchTasks {
    tasks: Vec<BatchTask>,
    task_runs: Vec<task_run::TaskRunRecord>,
}

fn batch_tasks_from_prepared(
    ctx: &Ctx,
    batch_path: &Path,
    prepared_tasks: Vec<PreparedTask>,
) -> Result<PreparedBatchTasks> {
    let group = task_run::group_from_path(batch_path)?;
    let mut tasks = Vec::new();
    let mut task_runs = Vec::new();
    for task in prepared_tasks {
        let run = match task_run::create(
            ctx,
            &task.key,
            &task.branch,
            task_run::SOURCE_BATCH,
            Some(&group),
            STATUS_PREPARED,
        ) {
            Ok(run) => run,
            Err(err) => {
                rollback_task_runs(&task_runs);
                return Err(err);
            }
        };
        let run_id = run.id.clone();
        task_runs.push(run);
        tasks.push(BatchTask::from_prepared(task, run_id));
    }
    Ok(PreparedBatchTasks { tasks, task_runs })
}

fn rollback_task_runs(task_runs: &[task_run::TaskRunRecord]) {
    for run in task_runs.iter().rev() {
        let _ = task_run::delete_record(run);
    }
}

fn resolve_batch_base(ctx: &Ctx, base: &Option<String>) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match BaseMode::from_raw(base) {
        BaseMode::Explicit(branch) => Ok(branch),
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            Ok(branches[idx].clone())
        }
        BaseMode::Current => git.current_branch(),
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))
        }
    }?;

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }

    Ok(base)
}

pub fn run(ctx: &Ctx, batch: &str, jobs: usize) -> Result<()> {
    let batch_path = resolve_batch_path(ctx, batch)?;
    let mut metadata = read_batch_metadata(&batch_path)?;
    validate_profile(ctx, metadata.profile.as_deref())?;

    if metadata.tasks.is_empty() {
        bail!("Batch has no tasks: {}", batch_path.display());
    }

    let task_states = read_batch_task_states(ctx, &batch_path, &metadata)?;
    let has_runnable_task = task_states
        .iter()
        .any(|state| is_runnable_status(&state.run.status));
    let base = if has_runnable_task {
        batch_base_option(&metadata)?
    } else {
        None
    };
    let ran_any = if jobs == 1 {
        run_batch_sequential(ctx, &batch_path, &mut metadata, &task_states, base)?
    } else {
        run_batch_parallel(ctx, &batch_path, &mut metadata, &task_states, base, jobs)?
    };

    finish_batch_run(ctx, &batch_path, &mut metadata, ran_any)
}

fn run_batch_sequential(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &mut BatchMetadata,
    task_states: &[BatchTaskState],
    base: Option<String>,
) -> Result<bool> {
    let mut ran_any = false;

    for state in task_states {
        let Some(execution) = next_task_execution(ctx, metadata, state, base.clone(), true)? else {
            continue;
        };

        ran_any = true;
        apply_task_event(
            ctx,
            batch_path,
            metadata,
            TaskEvent::Started { idx: execution.idx },
        )?;

        let result = run_batch_task(ctx, &execution);
        let cancelled = apply_task_result(ctx, batch_path, metadata, execution.idx, result)?;
        if cancelled {
            return Ok(ran_any);
        }
    }

    Ok(ran_any)
}

fn run_batch_parallel(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &mut BatchMetadata,
    task_states: &[BatchTaskState],
    base: Option<String>,
    jobs: usize,
) -> Result<bool> {
    let mut touched_any = false;
    let mut executions = Vec::new();

    for state in task_states {
        let Some(execution) = next_task_execution(ctx, metadata, state, base.clone(), false)?
        else {
            continue;
        };
        touched_any = true;
        executions.push(execution);
    }

    let (preflight_failures, runnable_executions) = preflight_parallel_executions(ctx, &executions);
    for failure in preflight_failures {
        apply_task_event(
            ctx,
            batch_path,
            metadata,
            TaskEvent::Failed {
                idx: failure.idx,
                error: failure.error,
            },
        )?;
    }

    if runnable_executions.is_empty() {
        return Ok(touched_any);
    }

    run_bounded_workers(ctx, batch_path, metadata, runnable_executions, jobs)?;
    Ok(touched_any)
}

fn next_task_execution(
    ctx: &Ctx,
    metadata: &BatchMetadata,
    state: &BatchTaskState,
    base: Option<String>,
    allow_interactive_prompts: bool,
) -> Result<Option<BatchTaskExecution>> {
    let current_status = state.run.status.as_str();
    if !is_runnable_status(current_status) {
        ctx.ui.print_step(&format!(
            "Skipping {} ({current_status})",
            state.batch_task.label()
        ));
        return Ok(None);
    }

    Ok(Some(BatchTaskExecution {
        idx: state.idx,
        batch_task: state.batch_task.clone(),
        total_tasks: metadata.tasks.len(),
        base: base.expect("batch base is validated before running a task"),
        profile: metadata.profile.clone(),
        allow_interactive_prompts,
    }))
}

fn preflight_parallel_executions(
    ctx: &Ctx,
    executions: &[BatchTaskExecution],
) -> (Vec<PreflightFailure>, Vec<BatchTaskExecution>) {
    let mut failures = Vec::new();
    let mut runnable = Vec::new();
    let mut branches: HashMap<String, Vec<usize>> = HashMap::new();
    let mut paths: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut plans = Vec::new();

    for execution in executions {
        match preflight_parallel_execution(ctx, execution) {
            Ok(plan) => {
                for branch in &plan.branches {
                    branches
                        .entry(branch.clone())
                        .or_default()
                        .push(execution.idx);
                }
                for path in &plan.paths {
                    paths.entry(path.clone()).or_default().push(execution.idx);
                }
                plans.push((execution, plan));
            }
            Err(err) => failures.push(PreflightFailure {
                idx: execution.idx,
                error: err.to_string(),
            }),
        }
    }

    let mut failed_indices = HashSet::new();
    for (branch, indices) in branches {
        if indices.len() > 1 {
            for idx in indices {
                if failed_indices.insert(idx) {
                    failures.push(PreflightFailure {
                        idx,
                        error: format!(
                            "Multiple runnable batch tasks target branch {branch}; adjust task branches before parallel run"
                        ),
                    });
                }
            }
        }
    }
    for (path, indices) in paths {
        if indices.len() > 1 {
            for idx in indices {
                if failed_indices.insert(idx) {
                    failures.push(PreflightFailure {
                        idx,
                        error: format!(
                            "Multiple runnable batch tasks target worktree path {}; adjust task branches before parallel run",
                            path.display()
                        ),
                    });
                }
            }
        }
    }

    for (execution, _) in plans {
        if !failed_indices.contains(&execution.idx) {
            runnable.push(execution.clone());
        }
    }

    (dedupe_preflight_failures(failures), runnable)
}

struct ParallelPreflightPlan {
    branches: Vec<String>,
    paths: Vec<PathBuf>,
}

fn preflight_parallel_execution(
    ctx: &Ctx,
    execution: &BatchTaskExecution,
) -> Result<ParallelPreflightPlan> {
    let batch_task = &execution.batch_task;
    let task_doc = task::read_task_document(ctx, &batch_task.task)?;
    let branch_name = prepared_branch_name(&task_doc.branch);
    if branch_name.is_none() && task_doc.origin.is_none() {
        bail!("Batch task {} has no branch", batch_task.label());
    }

    let Some(branch_name) = branch_name else {
        return Ok(ParallelPreflightPlan {
            branches: Vec::new(),
            paths: Vec::new(),
        });
    };

    let identifier = task_doc.identifier_or_key(&batch_task.task);
    let title = task_doc.title_or_key(&batch_task.task);
    let naming = worktree_naming::generate(ctx, &identifier, &title, Some(branch_name))?;
    let plans = issue::planned_worktrees_for_prepared_issue(
        ctx,
        &title,
        branch_name,
        execution.profile.as_deref(),
        naming.as_ref(),
    )?;

    for plan in &plans {
        if plan.path.exists() {
            bail!(
                "Worktree {} already exists; parallel batch workers cannot prompt to delete or open it",
                plan.path.display()
            );
        }
    }

    Ok(ParallelPreflightPlan {
        branches: plans.iter().map(|plan| plan.branch_name.clone()).collect(),
        paths: plans.into_iter().map(|plan| plan.path).collect(),
    })
}

fn dedupe_preflight_failures(failures: Vec<PreflightFailure>) -> Vec<PreflightFailure> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for failure in failures {
        if seen.insert((failure.idx, failure.error.clone())) {
            deduped.push(failure);
        }
    }
    deduped
}

fn run_bounded_workers(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &mut BatchMetadata,
    executions: Vec<BatchTaskExecution>,
    jobs: usize,
) -> Result<()> {
    let worker_count = jobs.max(1);
    let (tx, rx) = mpsc::channel::<BatchTaskCompletion>();
    let mut next = 0;
    let mut active = 0;
    let mut cancelled = false;

    thread::scope(|scope| -> Result<()> {
        loop {
            while !cancelled && active < worker_count && next < executions.len() {
                let execution = executions[next].clone();
                apply_task_event(
                    ctx,
                    batch_path,
                    metadata,
                    TaskEvent::Started { idx: execution.idx },
                )?;
                let tx = tx.clone();
                scope.spawn(move || {
                    let result = run_batch_task(ctx, &execution);
                    let _ = tx.send(BatchTaskCompletion {
                        idx: execution.idx,
                        result,
                    });
                });
                active += 1;
                next += 1;
            }

            if active == 0 {
                break;
            }

            let completion = rx
                .recv()
                .map_err(|_| anyhow::anyhow!("Batch worker result channel closed"))?;
            active -= 1;
            if apply_task_result(ctx, batch_path, metadata, completion.idx, completion.result)? {
                cancelled = true;
            }
        }

        Ok(())
    })?;

    if cancelled {
        for execution in executions.iter().skip(next) {
            apply_task_event(
                ctx,
                batch_path,
                metadata,
                TaskEvent::Skipped {
                    idx: execution.idx,
                    error: "Skipped after user cancellation".into(),
                },
            )?;
        }
    }

    Ok(())
}

fn apply_task_result(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &mut BatchMetadata,
    idx: usize,
    result: Result<BatchTaskSuccess>,
) -> Result<bool> {
    match result {
        Ok(success) => {
            if let Some(branch) = success.branch_update {
                if let Err(err) = task::write_task_branch(ctx, &metadata.tasks[idx].task, &branch) {
                    apply_task_event(
                        ctx,
                        batch_path,
                        metadata,
                        TaskEvent::Failed {
                            idx,
                            error: err.to_string(),
                        },
                    )?;
                    return Ok(false);
                }
            }
            apply_task_event(ctx, batch_path, metadata, TaskEvent::Succeeded { idx })?;
            Ok(false)
        }
        Err(err) if is_cancelled(&err) => {
            apply_task_event(ctx, batch_path, metadata, TaskEvent::Cancelled { idx })?;
            Ok(true)
        }
        Err(err) => {
            apply_task_event(
                ctx,
                batch_path,
                metadata,
                TaskEvent::Failed {
                    idx,
                    error: err.to_string(),
                },
            )?;
            Ok(false)
        }
    }
}

fn apply_task_event(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &mut BatchMetadata,
    event: TaskEvent,
) -> Result<()> {
    let (idx, status, error) = match event {
        TaskEvent::Started { idx } => {
            ctx.ui
                .print_step(&format!("Starting {}", metadata.tasks[idx].label()));
            metadata.status = STATUS_RUNNING.into();
            (idx, STATUS_RUNNING, None)
        }
        TaskEvent::Succeeded { idx } => {
            ctx.ui
                .print_step(&format!("Started {}", metadata.tasks[idx].label()));
            (idx, STATUS_RUNNING, None)
        }
        TaskEvent::Failed { idx, error } => {
            ctx.ui
                .print_warning(&format!("Failed {}: {error}", metadata.tasks[idx].label()));
            (idx, STATUS_FAILED, Some(error))
        }
        TaskEvent::Cancelled { idx } => {
            ctx.ui.print_warning(&format!(
                "Cancelled {}; not starting additional batch tasks",
                metadata.tasks[idx].label()
            ));
            (idx, STATUS_SKIPPED, Some("User cancelled".into()))
        }
        TaskEvent::Skipped { idx, error } => {
            ctx.ui.print_step(&format!(
                "Skipping {} ({error})",
                metadata.tasks[idx].label()
            ));
            (idx, STATUS_SKIPPED, Some(error))
        }
    };

    record_batch_task_run(
        ctx,
        batch_path,
        &metadata.tasks[idx],
        status,
        error.as_deref(),
    )?;

    metadata.status = summarize_current_batch_status(ctx, batch_path, metadata)?;
    metadata.updated_at = current_utc_timestamp();
    write_batch_metadata(batch_path, metadata)
}

fn record_batch_task_run(
    ctx: &Ctx,
    batch_path: &Path,
    item: &BatchTask,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let path = task_run::resolve(ctx, &item.run).with_context(|| {
        format!(
            "Batch task {} references missing TaskRun {}",
            item.label(),
            item.run
        )
    })?;
    let run = task_run::read(&path)?;
    validate_batch_task_run(batch_path, item, &run)?;

    let branch = task::read_task_document(ctx, &item.task)
        .ok()
        .map(|task| task.branch);
    let updated = task_run::update(ctx, &item.run, status, branch.as_deref(), error)?;
    validate_batch_task_run(batch_path, item, &updated.run)?;
    Ok(())
}

fn finish_batch_run(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &mut BatchMetadata,
    ran_any: bool,
) -> Result<()> {
    if !ran_any {
        ctx.ui
            .print_step("No prepared or failed tasks to run in this batch.");
    }

    metadata.status = summarize_current_batch_status(ctx, batch_path, metadata)?;
    metadata.updated_at = current_utc_timestamp();
    write_batch_metadata(batch_path, metadata)?;
    ctx.ui
        .print_step(&format!("Batch status: {}", metadata.status));

    if metadata.status == STATUS_FAILED {
        bail!("Batch failed: {}", batch_path.display());
    }

    Ok(())
}

fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<WtError>()
        .is_some_and(|err| matches!(err, WtError::Cancelled))
}

pub fn show(ctx: &Ctx, batch: Option<&str>) -> Result<()> {
    let batch_path = match batch {
        Some(target) => resolve_batch_path(ctx, target)?,
        None => latest_batch_path(ctx)?,
    };
    let metadata = read_batch_metadata(&batch_path)?;
    let task_states = read_batch_task_states(ctx, &batch_path, &metadata)?;
    let status = summarize_batch_status(&task_states);

    ctx.ui
        .print_step(&format!("Batch: {}", batch_path.display()));
    ctx.ui.print_dim(&format!("  Status: {status}"));
    ctx.ui
        .print_dim(&format!("  Base: {}", describe_batch_base(&metadata)?));
    ctx.ui.print_dim(&format!(
        "  Profile: {}",
        metadata.profile.as_deref().unwrap_or("(effective config)")
    ));
    ctx.ui.print_dim(&format!(
        "  Tasks: {} ({})",
        metadata.tasks.len(),
        batch_status_counts(&task_states)
    ));

    for state in &task_states {
        let item = &state.batch_task;
        let status = state.run.status.as_str();
        let task_doc = task::read_task_document(ctx, &item.task);
        let title = task_doc
            .as_ref()
            .map(|doc| doc.title_or_key(&item.task))
            .unwrap_or_else(|_| "(missing task)".into());
        let summary = if title.is_empty() {
            format!("  {}. {} [{}]", state.idx + 1, item.label(), status)
        } else {
            format!(
                "  {}. {} [{}] {}",
                state.idx + 1,
                item.label(),
                status,
                title
            )
        };
        ctx.ui.print_dim(&summary);
        ctx.ui.print_dim(&format!(
            "     Task: {}",
            task::task_relative_path(&item.task)
        ));
        match task_doc {
            Ok(doc) => {
                if !doc.branch.trim().is_empty() {
                    ctx.ui.print_dim(&format!("     Branch: {}", doc.branch));
                }
            }
            Err(err) => ctx.ui.print_dim(&format!("     Task error: {err}")),
        }
        if let Some(error) = state.run.error.as_deref() {
            if !error.trim().is_empty() {
                ctx.ui.print_dim(&format!("     Error: {error}"));
            }
        }
    }

    Ok(())
}

pub fn edit(ctx: &Ctx, batch: Option<&str>) -> Result<()> {
    let batch_path = match batch {
        Some(target) => resolve_batch_path(ctx, target)?,
        None => latest_batch_path(ctx)?,
    };
    crate::commands::editor::open_file(ctx, &batch_path)
}

pub fn clean(ctx: &Ctx, batch: Option<&str>) -> Result<()> {
    let batch_path = match batch {
        Some(target) => resolve_batch_path(ctx, target)?,
        None => latest_batch_path(ctx)?,
    };
    let metadata = read_batch_metadata(&batch_path)?;

    if metadata.tasks.is_empty() {
        bail!("Batch has no tasks: {}", batch_path.display());
    }

    let task_states = read_batch_task_states(ctx, &batch_path, &metadata)?;
    let blocked = task_states
        .iter()
        .filter(|state| !is_cleanable_status(&state.run.status))
        .map(|state| format!("{} [{}]", state.batch_task.label(), state.run.status))
        .collect::<Vec<_>>();
    if !blocked.is_empty() {
        bail!(
            "Batch has non-terminal tasks; cleanup requires every task to be done or skipped: {}",
            blocked.join(", ")
        );
    }

    let external_references = collect_external_task_references(ctx, &batch_path)?;
    let mut seen = HashSet::new();
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    let mut missing = Vec::new();

    for state in &task_states {
        let item = &state.batch_task;
        let key = task::safe_task_key(&item.task);
        if !seen.insert(key.clone()) {
            continue;
        }

        let relative_path = task::task_relative_path(&key);
        let path = ctx.repo_root.join(&relative_path);
        if !path.exists() {
            missing.push(relative_path);
        } else if external_references.contains(&key) {
            skipped.push(relative_path);
        } else {
            fs::remove_file(&path)?;
            deleted.push(relative_path);
        }
    }

    for path in &deleted {
        ctx.ui.print_step(&format!("Deleted {path}"));
    }
    for path in &skipped {
        ctx.ui.print_step(&format!(
            "Skipped {path} (referenced by another batch or stack)"
        ));
    }
    for path in &missing {
        ctx.ui.print_dim(&format!("Already clean {path} (missing)"));
    }
    if deleted.is_empty() && skipped.is_empty() && missing.is_empty() {
        ctx.ui.print_step("No task files to clean.");
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchMetadata {
    #[serde(default)]
    profile: Option<String>,
    base_mode: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(default = "default_batch_status")]
    status: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    tasks: Vec<BatchTask>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchTask {
    task: String,
    run: String,
}

#[derive(Debug, Deserialize)]
struct TaskReferenceMetadata {
    #[serde(default)]
    tasks: Vec<TaskReference>,
}

#[derive(Debug, Deserialize)]
struct TaskReference {
    task: String,
}

impl BatchTask {
    fn from_prepared(task: PreparedTask, run: String) -> Self {
        Self {
            task: task.key,
            run,
        }
    }

    fn label(&self) -> &str {
        if self.task.trim().is_empty() {
            "batch-task"
        } else {
            self.task.trim()
        }
    }
}

fn default_batch_status() -> String {
    STATUS_PREPARED.into()
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

#[derive(Clone)]
struct BatchTaskState {
    idx: usize,
    batch_task: BatchTask,
    run: task_run::TaskRun,
}

#[derive(Clone)]
struct BatchTaskExecution {
    idx: usize,
    batch_task: BatchTask,
    total_tasks: usize,
    base: String,
    profile: Option<String>,
    allow_interactive_prompts: bool,
}

struct BatchTaskSuccess {
    branch_update: Option<String>,
}

struct BatchTaskCompletion {
    idx: usize,
    result: Result<BatchTaskSuccess>,
}

struct PreflightFailure {
    idx: usize,
    error: String,
}

enum TaskEvent {
    Started { idx: usize },
    Succeeded { idx: usize },
    Failed { idx: usize, error: String },
    Cancelled { idx: usize },
    Skipped { idx: usize, error: String },
}

fn run_batch_task(ctx: &Ctx, execution: &BatchTaskExecution) -> Result<BatchTaskSuccess> {
    let batch_task = &execution.batch_task;
    let (task_doc, task_path, content) = task::read_task_file(ctx, &batch_task.task)?;
    let branch_name = prepared_branch_name(&task_doc.branch);
    if branch_name.is_none() && task_doc.origin.is_none() {
        bail!("Batch task {} has no branch", batch_task.label());
    }
    let identifier = task_doc.identifier_or_key(&batch_task.task);
    let title = task_doc.title_or_key(&batch_task.task);
    let workspace_label = task::workspace_run_label(
        "B",
        execution.idx,
        execution.total_tasks,
        task_doc.origin.as_ref().map(|origin| origin.id.as_str()),
    );
    let base = Some(execution.base.clone());
    let profile = execution.profile.as_deref();

    let prepared = issue::PreparedIssueContext {
        identifier: &identifier,
        title: &title,
        branch_name,
        mode: task_doc.mode(),
        prompt_intro: "Use this task before changing code.",
        workspace_label: Some(workspace_label),
        snapshot: issue::IssueSnapshotContext {
            path_label: "Task path",
            path: &task_path,
            content: &content,
        },
    };
    let result = if execution.allow_interactive_prompts {
        issue::run_with_issue_snapshot(ctx, &base, profile, false, prepared)?
    } else {
        issue::run_with_issue_snapshot_non_interactive(ctx, &base, profile, false, prepared)?
    };

    Ok(BatchTaskSuccess {
        branch_update: (task_doc.branch != result.branch_name).then_some(result.branch_name),
    })
}

fn prepared_branch_name(branch: &str) -> Option<&str> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "-" {
        None
    } else {
        Some(branch)
    }
}

fn next_available_batch_path(ctx: &Ctx) -> Result<PathBuf> {
    let batches_dir = ctx.repo_root.join(".local/batches");
    fs::create_dir_all(&batches_dir)?;

    let date = current_utc_date();
    let mut seq = 1;
    loop {
        let candidate = batches_dir.join(format!("{date}-{seq:03}.toml"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        seq += 1;
    }
}

fn read_batch_metadata(path: &Path) -> Result<BatchMetadata> {
    let content = fs::read_to_string(path)?;
    let mut metadata: BatchMetadata = toml::from_str(&content)?;
    for item in &mut metadata.tasks {
        validate_batch_task(item)?;
    }
    Ok(metadata)
}

fn read_batch_task_states(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &BatchMetadata,
) -> Result<Vec<BatchTaskState>> {
    let group = task_run::group_from_path(batch_path)?;
    metadata
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let path = task_run::resolve(ctx, &item.run).with_context(|| {
                format!(
                    "Batch task {} references missing TaskRun {}",
                    item.label(),
                    item.run
                )
            })?;
            let run = task_run::read(&path)?;
            validate_batch_task_run_with_group(&group, item, &run)?;
            Ok(BatchTaskState {
                idx,
                batch_task: item.clone(),
                run,
            })
        })
        .collect()
}

fn validate_batch_task_run(
    batch_path: &Path,
    item: &BatchTask,
    run: &task_run::TaskRun,
) -> Result<()> {
    let group = task_run::group_from_path(batch_path)?;
    validate_batch_task_run_with_group(&group, item, run)
}

fn validate_batch_task_run_with_group(
    group: &str,
    item: &BatchTask,
    run: &task_run::TaskRun,
) -> Result<()> {
    let expected_task = task::safe_task_key(&item.task);
    if run.task != expected_task {
        bail!(
            "Batch task {} references TaskRun {} for task {}",
            item.label(),
            item.run,
            run.task
        );
    }
    if run.source != task_run::SOURCE_BATCH {
        bail!(
            "Batch task {} references TaskRun {} with source {}",
            item.label(),
            item.run,
            run.source
        );
    }
    if run.group.as_deref() != Some(group) {
        bail!(
            "Batch task {} references TaskRun {} outside batch group {}",
            item.label(),
            item.run,
            group
        );
    }
    Ok(())
}

fn validate_batch_task(item: &BatchTask) -> Result<()> {
    if item.task.trim().is_empty() {
        bail!("Batch task is missing task");
    }
    if item.run.trim().is_empty() {
        bail!("Batch task {} is missing TaskRun id", item.label());
    }
    Ok(())
}

fn write_batch_metadata(path: &Path, batch: &BatchMetadata) -> Result<()> {
    let mut content = String::new();
    if let Some(profile) = batch.profile.as_deref() {
        content.push_str(&format!("profile = {}\n", toml_quote(profile)));
    }
    content.push_str(&format!("base_mode = {}\n", toml_quote(&batch.base_mode)));
    if let Some(base) = &batch.base {
        content.push_str(&format!("base = {}\n", toml_quote(base)));
    }
    content.push_str(&format!("status = {}\n", toml_quote(&batch.status)));
    content.push_str(&format!("created_at = {}\n", toml_quote(&batch.created_at)));
    content.push_str(&format!("updated_at = {}\n", toml_quote(&batch.updated_at)));

    for item in &batch.tasks {
        content.push_str("\n[[tasks]]\n");
        content.push_str(&format!("task = {}\n", toml_quote(&item.task)));
        content.push_str(&format!("run = {}\n", toml_quote(&item.run)));
    }

    fs::write(path, content)
        .with_context(|| format!("Failed to write batch metadata: {}", path.display()))?;
    Ok(())
}

pub(crate) fn task_keys_for_selector(ctx: &Ctx, target: &str) -> Result<Vec<String>> {
    let batch_path = resolve_batch_path(ctx, target)?;
    let metadata = read_batch_metadata(&batch_path)
        .with_context(|| format!("Failed to read batch: {}", batch_path.display()))?;
    Ok(metadata
        .tasks
        .iter()
        .map(|item| task::safe_task_key(&item.task))
        .collect())
}

fn resolve_batch_path(ctx: &Ctx, target: &str) -> Result<PathBuf> {
    if target == "latest" {
        return latest_batch_path(ctx);
    }

    let path = PathBuf::from(target);
    if path.is_absolute() && path.exists() {
        return Ok(path);
    }

    let invocation_path = ctx.invocation_root.join(target);
    if invocation_path.exists() {
        return Ok(invocation_path);
    }

    let repo_path = ctx.repo_root.join(target);
    if repo_path.exists() {
        return Ok(repo_path);
    }

    if !target.ends_with(".toml") {
        let shorthand = ctx
            .repo_root
            .join(".local/batches")
            .join(format!("{target}.toml"));
        if shorthand.exists() {
            return Ok(shorthand);
        }
    }

    bail!("Batch not found: {target}");
}

fn latest_batch_path(ctx: &Ctx) -> Result<PathBuf> {
    let batches_dir = ctx.repo_root.join(".local/batches");
    let mut paths = Vec::new();
    if batches_dir.exists() {
        for entry in fs::read_dir(&batches_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .pop()
        .ok_or_else(|| anyhow::anyhow!("No batch files found in .local/batches"))
}

fn describe_batch_base(batch: &BatchMetadata) -> Result<String> {
    match batch.base_mode.as_str() {
        "explicit" => batch
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Batch base_mode is explicit but base is missing")),
        "default" | "interactive" | "current" => bail!(
            "Batch base_mode must be explicit; recreate the batch with wt batch task or wt batch issue"
        ),
        other => bail!("Unknown batch base_mode: {other}"),
    }
}

fn batch_status_counts(items: &[BatchTaskState]) -> String {
    let statuses = [
        STATUS_PREPARED,
        STATUS_RUNNING,
        STATUS_DONE,
        STATUS_FAILED,
        STATUS_SKIPPED,
    ];
    let counts = statuses
        .iter()
        .filter_map(|status| {
            let count = items
                .iter()
                .filter(|item| item.run.status == *status)
                .count();
            (count > 0).then(|| format!("{status}={count}"))
        })
        .collect::<Vec<_>>();

    if counts.is_empty() {
        "none".into()
    } else {
        counts.join(", ")
    }
}

fn batch_base_option(batch: &BatchMetadata) -> Result<Option<String>> {
    match batch.base_mode.as_str() {
        "explicit" => batch
            .base
            .clone()
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("Batch base_mode is explicit but base is missing")),
        "default" | "interactive" | "current" => bail!(
            "Batch base_mode must be explicit before run; recreate the batch with wt batch task or wt batch issue"
        ),
        other => bail!("Unknown batch base_mode: {other}"),
    }
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, STATUS_PREPARED | STATUS_FAILED)
}

fn is_cleanable_status(status: &str) -> bool {
    matches!(status, STATUS_DONE | STATUS_SKIPPED)
}

fn collect_external_task_references(
    ctx: &Ctx,
    current_batch_path: &Path,
) -> Result<HashSet<String>> {
    let mut references = HashSet::new();
    collect_task_references_from_dir(
        &ctx.repo_root.join(".local/batches"),
        Some(current_batch_path),
        &mut references,
    )?;
    collect_task_references_from_dir(&ctx.repo_root.join(".local/stacks"), None, &mut references)?;
    Ok(references)
}

fn collect_task_references_from_dir(
    dir: &Path,
    excluded_path: Option<&Path>,
    references: &mut HashSet<String>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        if excluded_path.is_some_and(|excluded| paths_refer_to_same_file(&path, excluded)) {
            continue;
        }

        let content = fs::read_to_string(&path)?;
        let metadata: TaskReferenceMetadata = toml::from_str(&content)?;
        for item in metadata.tasks {
            if !item.task.trim().is_empty() {
                references.insert(task::safe_task_key(&item.task));
            }
        }
    }

    Ok(())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn summarize_current_batch_status(
    ctx: &Ctx,
    batch_path: &Path,
    metadata: &BatchMetadata,
) -> Result<String> {
    let states = read_batch_task_states(ctx, batch_path, metadata)?;
    Ok(summarize_batch_status(&states))
}

fn summarize_batch_status(items: &[BatchTaskState]) -> String {
    if items.is_empty() {
        return STATUS_DONE.into();
    }
    if items.iter().any(|item| item.run.status == STATUS_FAILED) {
        return STATUS_FAILED.into();
    }
    if items.iter().any(|item| item.run.status == STATUS_RUNNING) {
        return STATUS_RUNNING.into();
    }
    if items
        .iter()
        .all(|item| matches!(item.run.status.as_str(), STATUS_DONE | STATUS_SKIPPED))
    {
        return STATUS_DONE.into();
    }
    if items.iter().all(|item| item.run.status == STATUS_PREPARED) {
        return STATUS_PREPARED.into();
    }
    STATUS_PARTIAL.into()
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn current_utc_date() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        Config, EditorPlacement, IssueProviderType, IssuesConfig, WorkspaceConfig, WorktreeConfig,
        WorktreeNamingConfig,
    };
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner};
    use anyhow::Result;
    use std::sync::{Arc, Mutex};

    #[test]
    fn civil_date_matches_unix_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    fn batch_task(key: &str, run: &str) -> BatchTask {
        BatchTask {
            task: key.into(),
            run: run.into(),
        }
    }

    fn batch_task_with_status(
        ctx: &Ctx,
        batch_path: &Path,
        key: &str,
        branch: &str,
        status: &str,
        error: &str,
    ) -> BatchTask {
        let group = task_run::group_from_path(batch_path).unwrap();
        let record = task_run::create(
            ctx,
            key,
            branch,
            task_run::SOURCE_BATCH,
            Some(&group),
            status,
        )
        .unwrap();
        if !error.is_empty() {
            task_run::update(ctx, &record.id, status, Some(branch), Some(error)).unwrap();
        }
        batch_task(key, &record.id)
    }

    fn read_run(root: &std::path::Path, run_id: &str) -> task_run::TaskRun {
        task_run::read(&root.join(".local/task-runs").join(format!("{run_id}.toml"))).unwrap()
    }

    fn batch_state_with_status(idx: usize, key: &str, status: &str) -> BatchTaskState {
        BatchTaskState {
            idx,
            batch_task: batch_task(key, &format!("run-{idx}")),
            run: task_run::TaskRun {
                task: key.into(),
                branch: key.into(),
                status: status.into(),
                source: task_run::SOURCE_BATCH.into(),
                group: Some("batch".into()),
                error: None,
                creation_order: Some(idx as u64 + 1),
                created_at: "2026-05-11T00:00:00Z".into(),
                updated_at: "2026-05-11T00:00:00Z".into(),
            },
        }
    }

    fn write_task_file(root: &std::path::Path, key: &str, title: &str, branch: &str, body: &str) {
        let tasks_dir = root.join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let mut content = format!("title = \"{title}\"\n");
        if !branch.is_empty() {
            content.push_str(&format!("branch = \"{branch}\"\n"));
        }
        if !body.is_empty() {
            content.push_str(&format!("body = \"\"\"\n{body}\n\"\"\"\n"));
        }
        std::fs::write(tasks_dir.join(format!("{key}.toml")), content).unwrap();
    }

    fn write_issue_task_file(
        root: &std::path::Path,
        key: &str,
        title: &str,
        branch: &str,
        issue_id: &str,
    ) {
        let tasks_dir = root.join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join(format!("{key}.toml")),
            format!(
                "title = \"{title}\"\nbranch = \"{branch}\"\n\n[origin]\nprovider = \"linear\"\nid = \"{issue_id}\"\n"
            ),
        )
        .unwrap();
    }

    fn write_batch_file(path: &Path, batch: &BatchMetadata) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_batch_metadata(path, batch).unwrap();
    }

    struct ParallelBatchRunner {
        fail_worktree_branch: Option<String>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl ParallelBatchRunner {
        fn new() -> Self {
            Self {
                fail_worktree_branch: None,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn failing_worktree_add(branch: &str) -> Self {
            Self {
                fail_worktree_branch: Some(branch.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandRunner for std::sync::Arc<ParallelBatchRunner> {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&std::path::Path>,
        ) -> Result<CmdOutput> {
            assert_eq!(cmd, "git");
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|arg| arg.to_string()).collect());

            let success = |stdout: &str| {
                Ok(CmdOutput {
                    stdout: stdout.into(),
                    stderr: String::new(),
                    success: true,
                })
            };
            let failure = |stderr: &str| {
                Ok(CmdOutput {
                    stdout: String::new(),
                    stderr: stderr.into(),
                    success: false,
                })
            };

            match args {
                ["worktree", "list", "--porcelain"] => success(&format!(
                    "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                    cwd.unwrap().display()
                )),
                ["fetch", "origin"] => success(""),
                ["show-ref", "--verify", "--quiet", reference] => {
                    if *reference == "refs/heads/main" {
                        success("")
                    } else {
                        failure("")
                    }
                }
                ["worktree", "add", "-b", branch, _path, "main"] => {
                    if self.fail_worktree_branch.as_deref() == Some(*branch) {
                        failure("worktree add failed")
                    } else {
                        success("")
                    }
                }
                ["config", key, "main"] if key.starts_with("branch.") => success(""),
                other => failure(&format!("unexpected git args: {other:?}")),
            }
        }

        fn has_command(&self, _cmd: &str) -> bool {
            false
        }
    }

    fn parallel_batch_config() -> Config {
        Config {
            worktree: WorktreeConfig {
                path: Some(".local/worktrees/{{branch_slug}}".into()),
                ..WorktreeConfig::default()
            },
            ..Config::default()
        }
    }

    #[test]
    fn run_prefixes_workspace_with_short_batch_label() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true); // git fetch origin
        runner.add_response("", false); // local task branch does not exist
        runner.add_response("", false); // remote task branch does not exist
        runner.add_response("", true); // git worktree add
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // git config branch parent
        runner.add_response(r#"{"caller":null}"#, true); // cmux identify
        runner.add_response("workspace:1 workspace:1", true); // cmux new-workspace
        runner.add_response("", true); // cmux list-panes
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config {
                workspace: Some(WorkspaceConfig::default()),
                ..Config::default()
            },
            Box::new(runner),
            Box::new(ui.clone()),
        );
        let batch_path = dir.path().join(".local/batches/2026-05-16-001.toml");
        write_issue_task_file(dir.path(), "PROJ-1", "Schema", "proj-1-schema", "PROJ-1");
        write_issue_task_file(dir.path(), "PROJ-2", "API", "proj-2-api", "PROJ-2");
        let task_done = batch_task_with_status(
            &ctx,
            &batch_path,
            "PROJ-1",
            "proj-1-schema",
            STATUS_DONE,
            "",
        );
        let task_next = batch_task_with_status(
            &ctx,
            &batch_path,
            "PROJ-2",
            "proj-2-api",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-16T00:00:00Z".into(),
            updated_at: "2026-05-16T00:00:00Z".into(),
            tasks: vec![task_done, task_next],
        };
        write_batch_file(&batch_path, &batch);

        run(&ctx, batch_path.to_str().unwrap(), 1).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Opening cmux workspace: B2/2 PROJ-2 API"));
    }

    #[test]
    fn safe_task_key_replaces_unsafe_chars() {
        assert_eq!(task::safe_task_key("#42"), "42");
        assert_eq!(task::safe_task_key("PROJ-123"), "PROJ-123");
        assert_eq!(task::safe_task_key("bad/value"), "bad-value");
    }

    #[test]
    fn prepare_issue_tasks_writes_task_body_outside_batch_toml() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let tasks = task::prepare_issue_tasks(&ctx, &["PROJ-123".into()]).unwrap();

        assert_eq!(tasks[0].key, "PROJ-123");
        assert_eq!(tasks[0].branch, "alice/proj-123-fix-editor");
        let content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(content.contains("title = \"Fix editor\""));
        assert!(content.contains("branch = \"alice/proj-123-fix-editor\""));
        assert!(content.contains("body = \"\"\""));
        assert!(content.contains("Long issue body"));
        assert!(content.contains("[origin]"));
        assert!(content.contains("id = \"PROJ-123\""));
    }

    #[test]
    fn issue_omits_profile_when_default_behavior_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("main", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(&batch_path).unwrap();
        assert!(!content.contains("profile ="));
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"main\""));
        assert!(content.contains("status = \"prepared\""));
        assert!(content.contains("[[tasks]]"));
        assert!(content.contains("task = \"PROJ-123\""));
        assert!(content.contains("run = \"batch-"));
        let task_section = content.split("[[tasks]]").nth(1).unwrap();
        assert!(!task_section.contains("status ="));
        assert!(!task_section.contains("error ="));
        assert!(!content.contains("[[items]]"));
        assert!(!content.contains("[[issues]]"));
        let metadata = read_batch_metadata(&batch_path).unwrap();
        let run = read_run(dir.path(), &metadata.tasks[0].run);
        assert_eq!(run.task, "PROJ-123");
        assert_eq!(run.status, STATUS_PREPARED);
        assert_eq!(run.source, task_run::SOURCE_BATCH);
        let group = task_run::group_from_path(&batch_path).unwrap();
        assert_eq!(run.group.as_deref(), Some(group.as_str()));

        let task_content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(task_content.contains("title = \"Fix editor\""));
        assert!(task_content.contains("branch = \"alice/proj-123-fix-editor\""));
        assert!(task_content.contains("id = \"PROJ-123\""));
    }

    #[test]
    fn task_preparation_creates_batch_task_runs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        super::task(&ctx, &["add schema".into()], None, &Some("main".into())).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let metadata = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(metadata.tasks.len(), 1);
        assert_eq!(metadata.tasks[0].task, "add-schema");
        assert!(metadata.tasks[0].run.starts_with("batch-"));

        let run = read_run(dir.path(), &metadata.tasks[0].run);
        assert_eq!(run.task, "add-schema");
        assert_eq!(run.branch, "add-schema");
        assert_eq!(run.status, STATUS_PREPARED);
        assert_eq!(run.source, task_run::SOURCE_BATCH);
        let group = task_run::group_from_path(&batch_path).unwrap();
        assert_eq!(run.group.as_deref(), Some(group.as_str()));

        let content = std::fs::read_to_string(&batch_path).unwrap();
        let task_section = content.split("[[tasks]]").nth(1).unwrap();
        assert!(task_section.contains("task = \"add-schema\""));
        assert!(task_section.contains("run = \""));
        assert!(!task_section.contains("status ="));
        assert!(!task_section.contains("error ="));
    }

    #[test]
    fn prepare_rolls_back_task_runs_when_batch_metadata_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let existing = task_run::create(
            &ctx,
            "unrelated",
            "unrelated",
            task_run::SOURCE_NEW,
            None,
            STATUS_DONE,
        )
        .unwrap();
        let batch_path = dir.path().join(".local/batches/unwritable.toml");
        std::fs::create_dir_all(&batch_path).unwrap();

        let result = write_prepared_batch_at_path(
            &ctx,
            None,
            "main",
            &batch_path,
            vec![
                PreparedTask {
                    key: "add-schema".into(),
                    branch: "add-schema".into(),
                },
                PreparedTask {
                    key: "wire-api".into(),
                    branch: "wire-api".into(),
                },
            ],
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("Failed to write batch metadata"));
        assert!(error.contains("unwritable.toml"));
        let task_runs_dir = dir.path().join(".local/task-runs");
        assert!(task_runs_dir.join(format!("{}.toml", existing.id)).exists());
        assert!(
            !task_runs_dir
                .join("batch-unwritable-add-schema.toml")
                .exists()
        );
        assert!(
            !task_runs_dir
                .join("batch-unwritable-wire-api.toml")
                .exists()
        );
        let records = task_run::list(&ctx).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, existing.id);
    }

    #[test]
    fn issue_applies_worktree_naming_to_prepared_branch() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response(r#"{"english_slug":"repair-editor"}"#, true);
        runner.add_response("main", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            worktree: crate::config::WorktreeConfig {
                naming: Some(WorktreeNamingConfig {
                    command: "namer".into(),
                    prompt: "{{issue_title}}".into(),
                    branch: Some("{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}".into()),
                    workspace: None,
                }),
                ..Default::default()
            },
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(&batch_path).unwrap();
        assert!(content.contains("task = \"PROJ-123\""));
        let task_content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(task_content.contains("branch = \"alice/proj-123-repair-editor\""));
    }

    #[test]
    fn issue_resolves_current_base_for_dot_base() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("feature", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], None, &Some(".".into())).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"feature\""));
    }

    #[test]
    fn batch_base_option_rejects_non_explicit_base() {
        let batch = BatchMetadata {
            profile: None,
            base_mode: "current".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            tasks: Vec::new(),
        };

        let result = batch_base_option(&batch);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("base_mode must be explicit")
        );
    }

    #[test]
    fn issue_stores_default_base_prompt_result_at_prepare_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_input("develop");
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(ui),
        );

        issue(&ctx, &["PROJ-123".into()], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"develop\""));
    }

    #[test]
    fn issue_records_explicit_named_profile() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response("main", true);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(&ctx, &["PROJ-123".into()], Some("codex"), &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("profile = \"codex\""));
    }

    #[test]
    fn show_prints_batch_metadata_and_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        write_task_file(
            dir.path(),
            "PROJ-123",
            "Fix editor",
            "alice/proj-123-fix-editor",
            "",
        );
        let batch_path = dir.path().join("batch.toml");
        let task = batch_task_with_status(
            &ctx,
            &batch_path,
            "PROJ-123",
            "alice/proj-123-fix-editor",
            STATUS_FAILED,
            "missing task",
        );
        let batch = BatchMetadata {
            profile: Some("codex".into()),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        show(&ctx, Some(batch_path.to_str().unwrap())).unwrap();

        let steps = ui.steps.lock().unwrap();
        assert!(steps[0].contains("Batch:"));
        let details = ui.dims.lock().unwrap().join("\n");
        assert!(details.contains("Status: failed"));
        assert!(details.contains("Base: main"));
        assert!(details.contains("Profile: codex"));
        assert!(details.contains("Tasks: 1 (failed=1)"));
        assert!(details.contains("PROJ-123 [failed] Fix editor"));
        assert!(details.contains("Task: .local/tasks/PROJ-123.toml"));
        assert!(details.contains("Branch: alice/proj-123-fix-editor"));
        assert!(details.contains("Error: missing task"));
    }

    #[test]
    fn show_rejects_non_explicit_base_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "interactive".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: Vec::new(),
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = show(&ctx, Some(batch_path.to_str().unwrap()));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("base_mode must be explicit")
        );
    }

    #[test]
    fn show_rejects_missing_task_run_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        write_task_file(dir.path(), "PROJ-123", "Fix editor", "PROJ-123", "");
        let batch_path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![batch_task("PROJ-123", "missing-run")],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = show(&ctx, Some(batch_path.to_str().unwrap()));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing TaskRun"));
    }

    #[test]
    fn issue_with_no_args_selects_issues_from_provider_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"PROJ-1","title":"Schema","state":{"name":"Todo"}},{"identifier":"PROJ-2","title":"API","state":{"name":"Todo"}}]"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-2","title":"API","branchName":"alice/proj-2-api","description":"API body"}"#,
            true,
        );
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(ui),
        );

        issue(&ctx, &[], None, &None).unwrap();

        let batch_path = latest_batch_path(&ctx).unwrap();
        let content = std::fs::read_to_string(batch_path).unwrap();
        assert!(content.contains("task = \"PROJ-2\""));
        assert!(!content.contains("task = \"PROJ-1\""));
    }

    #[test]
    fn batch_metadata_round_trips_task_run_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("batch.toml");
        let batch = BatchMetadata {
            profile: Some("codex-yolo".into()),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:01:00Z".into(),
            tasks: vec![batch_task("PROJ-123", "batch-batch-PROJ-123")],
        };

        write_batch_metadata(&path, &batch).unwrap();
        let parsed = read_batch_metadata(&path).unwrap();

        assert_eq!(parsed.profile.as_deref(), Some("codex-yolo"));
        assert_eq!(parsed.base.as_deref(), Some("main"));
        assert_eq!(parsed.status, STATUS_PARTIAL);
        assert_eq!(parsed.tasks[0].task, "PROJ-123");
        assert_eq!(parsed.tasks[0].run, "batch-batch-PROJ-123");

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("[[tasks]]"));
        assert!(content.contains("task = \"PROJ-123\""));
        assert!(content.contains("run = \"batch-batch-PROJ-123\""));
        let task_section = content.split("[[tasks]]").nth(1).unwrap();
        assert!(!task_section.contains("status ="));
        assert!(!task_section.contains("error ="));
        assert!(!content.contains("[[issues]]"));
    }

    #[test]
    fn read_batch_metadata_rejects_issues_tables() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.toml");
        std::fs::write(
            &batch_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[issues]]
id = "PROJ-1"
source = "1"
title = "Fix editor"
branch = "alice/proj-1-fix-editor"
snapshot = ".local/issues/PROJ-1.md"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_batch_metadata(&batch_path).is_err());
    }

    #[test]
    fn read_batch_metadata_rejects_task_row_status_and_error() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.toml");
        std::fs::write(
            &batch_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[tasks]]
task = "PROJ-1"
run = "batch-batch-PROJ-1"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_batch_metadata(&batch_path).is_err());
    }

    #[test]
    fn read_batch_metadata_rejects_legacy_items_tables() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.toml");
        std::fs::write(
            &batch_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[items]]
kind = "new"
id = "add-schema"
title = "Add schema"
branch = "add-schema"
snapshot = ".local/issues/add-schema.md"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_batch_metadata(&batch_path).is_err());
    }

    #[test]
    fn read_batch_metadata_rejects_legacy_items_without_kind() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.toml");
        std::fs::write(
            &batch_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[items]]
id = "PROJ-1"
title = "Schema"
branch = "alice/proj-1-schema"
snapshot = ".local/issues/PROJ-1.md"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_batch_metadata(&batch_path).is_err());
    }

    #[test]
    fn latest_batch_path_uses_lexically_newest_batch_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batches_dir = dir.path().join(".local/batches");
        std::fs::create_dir_all(&batches_dir).unwrap();
        std::fs::write(batches_dir.join("2026-05-11-001.toml"), "").unwrap();
        std::fs::write(batches_dir.join("2026-05-11-002.toml"), "").unwrap();

        let latest = latest_batch_path(&ctx).unwrap();

        assert!(latest.ends_with("2026-05-11-002.toml"));
    }

    #[test]
    fn edit_opens_latest_batch_with_configured_editor() {
        let dir = tempfile::tempdir().unwrap();
        let batches_dir = dir.path().join(".local/batches");
        std::fs::create_dir_all(&batches_dir).unwrap();
        std::fs::write(
            batches_dir.join("2026-05-11-001.toml"),
            "base_mode = \"explicit\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let mut config = Config::default();
        config.editor.command = Some("code {{path}}".into());
        config.editor.placement = Some(EditorPlacement::Process);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        edit(&ctx, None).unwrap();
    }

    #[test]
    fn clean_deletes_completed_batch_task_files() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        write_task_file(dir.path(), "done-task", "Done task", "done-task", "");
        write_task_file(
            dir.path(),
            "skipped-task",
            "Skipped task",
            "skipped-task",
            "",
        );
        let batch_path = dir.path().join(".local/batches/2026-05-11-001.toml");
        let done_task =
            batch_task_with_status(&ctx, &batch_path, "done-task", "done-task", STATUS_DONE, "");
        let skipped_task = batch_task_with_status(
            &ctx,
            &batch_path,
            "skipped-task",
            "skipped-task",
            STATUS_SKIPPED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_DONE.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![done_task, skipped_task],
        };
        write_batch_file(&batch_path, &batch);

        clean(&ctx, Some(batch_path.to_str().unwrap())).unwrap();

        assert!(!dir.path().join(".local/tasks/done-task.toml").exists());
        assert!(!dir.path().join(".local/tasks/skipped-task.toml").exists());
        assert!(batch_path.exists());
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Deleted .local/tasks/done-task.toml"));
        assert!(steps.contains("Deleted .local/tasks/skipped-task.toml"));
    }

    #[test]
    fn clean_refuses_non_terminal_batch_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        for key in ["prepared-task", "running-task", "failed-task"] {
            write_task_file(dir.path(), key, key, key, "");
        }
        let batch_path = dir.path().join(".local/batches/2026-05-11-001.toml");
        let prepared_task = batch_task_with_status(
            &ctx,
            &batch_path,
            "prepared-task",
            "prepared-task",
            STATUS_PREPARED,
            "",
        );
        let running_task = batch_task_with_status(
            &ctx,
            &batch_path,
            "running-task",
            "running-task",
            STATUS_RUNNING,
            "",
        );
        let failed_task = batch_task_with_status(
            &ctx,
            &batch_path,
            "failed-task",
            "failed-task",
            STATUS_FAILED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_FAILED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![prepared_task, running_task, failed_task],
        };
        write_batch_file(&batch_path, &batch);

        let result = clean(&ctx, Some(batch_path.to_str().unwrap()));

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("non-terminal tasks"));
        assert!(error.contains("prepared-task [prepared]"));
        assert!(error.contains("running-task [running]"));
        assert!(error.contains("failed-task [failed]"));
        assert!(dir.path().join(".local/tasks/prepared-task.toml").exists());
        assert!(dir.path().join(".local/tasks/running-task.toml").exists());
        assert!(dir.path().join(".local/tasks/failed-task.toml").exists());
    }

    #[test]
    fn clean_skips_tasks_referenced_by_other_batches_or_stacks() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        for key in ["target-only", "shared-batch", "shared-stack"] {
            write_task_file(dir.path(), key, key, key, "");
        }
        let target_path = dir.path().join(".local/batches/2026-05-11-001.toml");
        let target_only = batch_task_with_status(
            &ctx,
            &target_path,
            "target-only",
            "target-only",
            STATUS_DONE,
            "",
        );
        let shared_batch = batch_task_with_status(
            &ctx,
            &target_path,
            "shared-batch",
            "shared-batch",
            STATUS_DONE,
            "",
        );
        let shared_stack = batch_task_with_status(
            &ctx,
            &target_path,
            "shared-stack",
            "shared-stack",
            STATUS_DONE,
            "",
        );
        let target = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_DONE.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![target_only, shared_batch, shared_stack],
        };
        write_batch_file(&target_path, &target);
        let other_batch_path = dir.path().join(".local/batches/2026-05-11-002.toml");
        let other_shared_batch = batch_task_with_status(
            &ctx,
            &other_batch_path,
            "shared-batch",
            "shared-batch",
            STATUS_DONE,
            "",
        );
        let other_batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_DONE.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![other_shared_batch],
        };
        write_batch_file(&other_batch_path, &other_batch);
        let stacks_dir = dir.path().join(".local/stacks");
        std::fs::create_dir_all(&stacks_dir).unwrap();
        std::fs::write(
            stacks_dir.join("2026-05-11-001.toml"),
            r#"base_mode = "explicit"
base = "main"
status = "done"

[[tasks]]
task = "shared-stack"
status = "done"
error = ""
"#,
        )
        .unwrap();

        clean(&ctx, Some(target_path.to_str().unwrap())).unwrap();

        assert!(!dir.path().join(".local/tasks/target-only.toml").exists());
        assert!(dir.path().join(".local/tasks/shared-batch.toml").exists());
        assert!(dir.path().join(".local/tasks/shared-stack.toml").exists());
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Deleted .local/tasks/target-only.toml"));
        assert!(steps.contains(
            "Skipped .local/tasks/shared-batch.toml (referenced by another batch or stack)"
        ));
        assert!(steps.contains(
            "Skipped .local/tasks/shared-stack.toml (referenced by another batch or stack)"
        ));
    }

    #[test]
    fn clean_treats_missing_task_files_as_already_clean() {
        let dir = tempfile::tempdir().unwrap();
        let ui = std::sync::Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        write_task_file(
            dir.path(),
            "present-task",
            "Present task",
            "present-task",
            "",
        );
        let batch_path = dir.path().join(".local/batches/2026-05-11-001.toml");
        let present_task = batch_task_with_status(
            &ctx,
            &batch_path,
            "present-task",
            "present-task",
            STATUS_DONE,
            "",
        );
        let missing_task = batch_task_with_status(
            &ctx,
            &batch_path,
            "missing-task",
            "missing-task",
            STATUS_DONE,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_DONE.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![present_task, missing_task],
        };
        write_batch_file(&batch_path, &batch);

        clean(&ctx, Some(batch_path.to_str().unwrap())).unwrap();

        assert!(!dir.path().join(".local/tasks/present-task.toml").exists());
        assert!(!dir.path().join(".local/tasks/missing-task.toml").exists());
        let steps = ui.steps.lock().unwrap().join("\n");
        let details = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Deleted .local/tasks/present-task.toml"));
        assert!(details.contains("Already clean .local/tasks/missing-task.toml (missing)"));
    }

    #[test]
    fn summarize_status_distinguishes_batch_and_task_state() {
        let task = |idx, status: &str| batch_state_with_status(idx, "PROJ-123", status);

        assert_eq!(
            summarize_batch_status(&[task(0, STATUS_PREPARED)]),
            STATUS_PREPARED
        );
        assert_eq!(
            summarize_batch_status(&[task(0, STATUS_DONE), task(1, STATUS_PREPARED)]),
            STATUS_PARTIAL
        );
        assert_eq!(
            summarize_batch_status(&[task(0, STATUS_DONE), task(1, STATUS_FAILED)]),
            STATUS_FAILED
        );
        assert_eq!(
            summarize_batch_status(&[task(0, STATUS_DONE), task(1, STATUS_SKIPPED)]),
            STATUS_DONE
        );
    }

    #[test]
    fn run_skips_done_tasks_without_touching_issue_provider() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let done_task =
            batch_task_with_status(&ctx, &batch_path, "PROJ-123", "PROJ-123", STATUS_DONE, "");
        let batch = BatchMetadata {
            profile: None,
            base_mode: "default".into(),
            base: None,
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![done_task],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        run(&ctx, batch_path.to_str().unwrap(), 1).unwrap();

        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_DONE);
        assert_eq!(
            read_run(dir.path(), &updated.tasks[0].run).status,
            STATUS_DONE
        );
    }

    #[test]
    fn run_marks_task_failed_and_errors_when_task_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task = batch_task_with_status(
            &ctx,
            &batch_path,
            "PROJ-123",
            "PROJ-123",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap(), 1);

        assert!(result.is_err());
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.base_mode, "explicit");
        assert_eq!(updated.base.as_deref(), Some("main"));
        assert_eq!(updated.status, STATUS_FAILED);
        let run = read_run(dir.path(), &updated.tasks[0].run);
        assert_eq!(run.task, "PROJ-123");
        assert_eq!(run.status, STATUS_FAILED);
        assert_eq!(run.source, task_run::SOURCE_BATCH);
        assert_eq!(run.group.as_deref(), Some("batch"));
        assert!(run.error.unwrap().contains("Failed to read task"));
    }

    #[test]
    fn run_rejects_non_explicit_base_before_touching_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task = batch_task_with_status(
            &ctx,
            &batch_path,
            "PROJ-123",
            "PROJ-123",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "interactive".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap(), 1);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("base_mode must be explicit")
        );
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.base_mode, "interactive");
        assert_eq!(updated.status, STATUS_PREPARED);
        let run = read_run(dir.path(), &updated.tasks[0].run);
        assert_eq!(run.status, STATUS_PREPARED);
        assert!(run.error.is_none());
    }

    #[test]
    fn run_parallel_marks_multiple_tasks_running_through_writer() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "task-one", "Task one", "alice/task-one", "");
        write_task_file(dir.path(), "task-two", "Task two", "alice/task-two", "");
        let runner = std::sync::Arc::new(ParallelBatchRunner::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            parallel_batch_config(),
            Box::new(runner.clone()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task_one = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-one",
            "alice/task-one",
            STATUS_PREPARED,
            "",
        );
        let task_two = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-two",
            "alice/task-two",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task_one, task_two],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        run(&ctx, batch_path.to_str().unwrap(), 2).unwrap();

        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        for item in &updated.tasks {
            let run = read_run(dir.path(), &item.run);
            assert_eq!(run.status, STATUS_RUNNING);
            assert_eq!(run.source, task_run::SOURCE_BATCH);
            assert_eq!(run.group.as_deref(), Some("batch"));
        }
        let calls = runner.calls();
        let worktree_adds = calls
            .iter()
            .filter(|args| args.starts_with(&["worktree".into(), "add".into(), "-b".into()]))
            .count();
        assert_eq!(worktree_adds, 2);
    }

    #[test]
    fn run_parallel_records_worker_failure_and_keeps_other_result() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "task-ok", "Task ok", "alice/task-ok", "");
        write_task_file(dir.path(), "task-fail", "Task fail", "alice/task-fail", "");
        let runner =
            std::sync::Arc::new(ParallelBatchRunner::failing_worktree_add("alice/task-fail"));
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            parallel_batch_config(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task_ok = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-ok",
            "alice/task-ok",
            STATUS_PREPARED,
            "",
        );
        let task_fail = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-fail",
            "alice/task-fail",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task_ok, task_fail],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap(), 2);

        assert!(result.is_err());
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_FAILED);
        let ok_run = read_run(dir.path(), &updated.tasks[0].run);
        let failed_run = read_run(dir.path(), &updated.tasks[1].run);
        assert_eq!(ok_run.status, STATUS_RUNNING);
        assert_eq!(failed_run.status, STATUS_FAILED);
        assert!(failed_run.error.unwrap().contains("worktree add failed"));
    }

    #[test]
    fn run_parallel_skips_non_runnable_tasks() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "task-one", "Task one", "alice/task-one", "");
        write_task_file(dir.path(), "task-done", "Task done", "alice/task-done", "");
        let runner = std::sync::Arc::new(ParallelBatchRunner::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            parallel_batch_config(),
            Box::new(runner.clone()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task_one = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-one",
            "alice/task-one",
            STATUS_PREPARED,
            "",
        );
        let task_done = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-done",
            "alice/task-done",
            STATUS_DONE,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task_one, task_done],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        run(&ctx, batch_path.to_str().unwrap(), 2).unwrap();

        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(
            read_run(dir.path(), &updated.tasks[0].run).status,
            STATUS_RUNNING
        );
        assert_eq!(
            read_run(dir.path(), &updated.tasks[1].run).status,
            STATUS_DONE
        );
        let calls = runner.calls();
        let worktree_adds = calls
            .iter()
            .filter(|args| args.starts_with(&["worktree".into(), "add".into(), "-b".into()]))
            .count();
        assert_eq!(worktree_adds, 1);
    }

    #[test]
    fn run_parallel_marks_existing_worktree_prompt_case_failed_before_worker() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "task-one", "Task one", "alice/task-one", "");
        let existing_path = dir.path().join(".local/worktrees/task-one");
        std::fs::create_dir_all(&existing_path).unwrap();
        let runner = std::sync::Arc::new(ParallelBatchRunner::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            parallel_batch_config(),
            Box::new(runner.clone()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task_one = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-one",
            "alice/task-one",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task_one],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap(), 2);

        assert!(result.is_err());
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_FAILED);
        let run = read_run(dir.path(), &updated.tasks[0].run);
        assert_eq!(run.status, STATUS_FAILED);
        assert!(
            run.error
                .unwrap()
                .contains("parallel batch workers cannot prompt")
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn run_parallel_marks_duplicate_branch_preflight_failed_before_workers() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "task-one", "Task one", "alice/same-task", "");
        write_task_file(dir.path(), "task-two", "Task two", "alice/same-task", "");
        let runner = std::sync::Arc::new(ParallelBatchRunner::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            parallel_batch_config(),
            Box::new(runner.clone()),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task_one = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-one",
            "alice/same-task",
            STATUS_PREPARED,
            "",
        );
        let task_two = batch_task_with_status(
            &ctx,
            &batch_path,
            "task-two",
            "alice/same-task",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task_one, task_two],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        let result = run(&ctx, batch_path.to_str().unwrap(), 2);

        assert!(result.is_err());
        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.status, STATUS_FAILED);
        let first_run = read_run(dir.path(), &updated.tasks[0].run);
        let second_run = read_run(dir.path(), &updated.tasks[1].run);
        assert_eq!(first_run.status, STATUS_FAILED);
        assert_eq!(second_run.status, STATUS_FAILED);
        assert!(
            first_run
                .error
                .unwrap()
                .contains("target branch alice/same-task")
        );
        assert!(
            second_run
                .error
                .unwrap()
                .contains("target branch alice/same-task")
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn run_uses_task_metadata_without_issue_provider_when_branch_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(
            dir.path(),
            "PROJ-123",
            "Fix editor",
            "alice/proj-123-fix-editor",
            "Body",
        );

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        ); // checked_out_path
        runner.add_response("", true); // fetch
        runner.add_response("", false); // branch does not exist locally
        runner.add_response("", false); // branch does not exist remotely
        runner.add_response("", true); // worktree add
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // branch parent config
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let batch_path = dir.path().join("batch.toml");
        let task = batch_task_with_status(
            &ctx,
            &batch_path,
            "PROJ-123",
            "alice/proj-123-fix-editor",
            STATUS_PREPARED,
            "",
        );
        let batch = BatchMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-11T00:00:00Z".into(),
            updated_at: "2026-05-11T00:00:00Z".into(),
            tasks: vec![task],
        };
        write_batch_metadata(&batch_path, &batch).unwrap();

        run(&ctx, batch_path.to_str().unwrap(), 1).unwrap();

        let updated = read_batch_metadata(&batch_path).unwrap();
        assert_eq!(updated.base_mode, "explicit");
        assert_eq!(updated.base.as_deref(), Some("main"));
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(
            read_run(dir.path(), &updated.tasks[0].run).status,
            STATUS_RUNNING
        );
    }
}
