use crate::cli::{BaseMode, WorkflowModeArg, WorkflowPrModeArg};
use crate::commands::issue;
use crate::commands::profile_selection;
use crate::config::{
    AGENT_PROMPT_WORKFLOW_SCOPE, Config, WorkflowDefaultPolicy, validate_profile_name,
};
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::setup;
use crate::task::{self as task_store, PreparedTask};
use crate::task_run::{self, STATUS_PREPARED, STATUS_RUNNING};
use crate::workflow as workflow_store;
use crate::workflow::planner::{
    validate_single_mode_branches, workflow_base_raw, workflow_mode, workflow_policy,
    workflow_pr_mode,
};
use crate::workflow::render::{
    no_tasks_selected_message, prepared_workflow_message, render_single_workflow_snapshot,
    single_workflow_group_title, workflow_objective_prompt_context,
    workflow_single_group_prompt_intro, workflow_single_task_handoff_section,
    workflow_single_task_prompt_intro,
};
use crate::workflow::{WorkflowMetadata, WorkflowMode, WorkflowTask, WorkflowTaskRun};
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod batch;
mod matrix;
mod stack;
pub(crate) mod state;

use batch::run_batch_workflow;
use matrix::run_matrix_workflow;
use stack::run_stack_workflow;
pub(crate) use state::{
    WorkflowTaskState, read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states, task_run_record,
    update_workflow_profile_task_run, update_workflow_task_run,
};

#[derive(Clone, Copy)]
pub(crate) struct PrepareWorkflowOptions<'a> {
    pub(crate) mode: WorkflowModeArg,
    pub(crate) profile: Option<&'a str>,
    pub(crate) profiles: &'a [String],
    pub(crate) objective: Option<&'a str>,
    pub(crate) base: &'a Option<String>,
    pub(crate) pr: Option<WorkflowPrModeArg>,
}

pub(crate) fn validate_prepare_options(
    ctx: &Ctx,
    mode: WorkflowModeArg,
    profile: Option<&str>,
    profiles: &[String],
    pr: Option<WorkflowPrModeArg>,
) -> Result<()> {
    if mode == WorkflowModeArg::Matrix {
        if profile.is_some() {
            bail!("--profile cannot be used with --mode matrix; use --profiles");
        }
        if profiles.is_empty() {
            bail!("--profiles is required with --mode matrix");
        }
        profile_selection::load_selected_profiles(ctx, profiles)?;
    } else {
        if !profiles.is_empty() {
            bail!("--profiles requires --mode matrix");
        }
        validate_profile(ctx, profile)?;
    }
    validate_mode_options(mode, pr)
}

pub(crate) fn prepare_workflow(
    ctx: &Ctx,
    options: PrepareWorkflowOptions<'_>,
    prepared_tasks: Vec<PreparedTask>,
) -> Result<()> {
    if prepared_tasks.is_empty() {
        ctx.ui.print_warning(no_tasks_selected_message());
        return Ok(());
    }

    validate_prepare_options(
        ctx,
        options.mode,
        options.profile,
        options.profiles,
        options.pr,
    )?;
    if options.mode == WorkflowModeArg::Matrix && prepared_tasks.len() != 1 {
        bail!("matrix mode workflow requires exactly one task");
    }
    validate_single_mode_branches(options.mode, &prepared_tasks)?;
    let resolved_base = resolve_workflow_base(ctx, options.base)?;
    let workflow_path = workflow_store::next_available_path(ctx)?;
    let default_policy = workflow_default_policy(ctx, options.profile)?;
    let pull_request = workflow_pr_mode(options.pr, default_policy);
    let prepared = workflow_tasks_from_prepared(
        ctx,
        options.mode,
        &workflow_path,
        &resolved_base,
        prepared_tasks,
        options.profiles,
    )?;

    let mut metadata = WorkflowMetadata::new(
        workflow_mode(options.mode),
        "explicit",
        Some(resolved_base),
        prepared.tasks,
    );
    metadata.profile = options.profile.map(str::to_string);
    if options.mode == WorkflowModeArg::Matrix {
        metadata.profiles = options.profiles.to_vec();
    }
    metadata.objective = normalized_objective(options.objective)?;
    metadata.policy = workflow_policy(default_policy, pull_request);

    if let Err(err) = workflow_store::write(ctx, &workflow_path, &mut metadata) {
        rollback_task_runs(&prepared.task_runs);
        return Err(err);
    }

    ctx.ui
        .print_step(&prepared_workflow_message(&workflow_path));
    Ok(())
}

pub(crate) fn run_workflow(ctx: &Ctx, workflow_path: &Path, jobs: usize) -> Result<()> {
    let mut metadata = workflow_store::read(workflow_path)?;
    match metadata.mode {
        WorkflowMode::Single => run_single_workflow(ctx, workflow_path, &mut metadata),
        WorkflowMode::Batch => run_batch_workflow(ctx, workflow_path, &mut metadata, jobs),
        WorkflowMode::Stack => run_stack_workflow(ctx, workflow_path, &mut metadata),
        WorkflowMode::Matrix => run_matrix_workflow(ctx, workflow_path, &mut metadata, jobs),
    }
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
    if states.iter().any(|state| !state.run.is_runnable()) {
        bail!("single mode workflow can only run prepared or failed TaskRuns");
    }

    let base = workflow_base_raw(metadata)?;
    let result = if states.len() == 1 {
        run_single_workflow_task(
            workflow_path,
            metadata,
            ctx,
            &states[0],
            &base,
            metadata.profile.as_deref(),
        )
    } else {
        run_single_workflow_group(
            workflow_path,
            metadata,
            ctx,
            &states,
            &base,
            metadata.profile.as_deref(),
        )
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
            task_store::write_task_branch(ctx, &state.row.task, &result.branch_name)?;
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

fn run_single_workflow_task(
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
    ctx: &Ctx,
    state: &WorkflowTaskState,
    base: &Option<String>,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let completion_section = workflow_single_task_handoff_section(
        workflow_path,
        Some(&state.row),
        &metadata.policy,
        workflow_pr_base(base),
        &task_issue_closing_references(&state.document),
    );
    let workflow_context = workflow_objective_prompt_context(metadata.objective.as_deref());
    let branch_name = task_store::prepared_branch_name(&state.document.branch);
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
            setup_mode: state.document.setup_mode(),
            additional_prompt_scope: Some(AGENT_PROMPT_WORKFLOW_SCOPE),
            workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
            on_start_issue_id: state
                .document
                .origin
                .as_ref()
                .map(|origin| origin.id.as_str()),
            prompt_intro: workflow_single_task_prompt_intro(),
            completion_section: Some(&completion_section),
            pre_snapshot_context: workflow_context.as_deref(),
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
    workflow_path: &Path,
    metadata: &WorkflowMetadata,
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
    let completion_section = workflow_single_task_handoff_section(
        workflow_path,
        None,
        &metadata.policy,
        workflow_pr_base(base),
        &workflow_issue_closing_references(states),
    );
    let workflow_context = workflow_objective_prompt_context(metadata.objective.as_deref());
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
            setup_mode: setup::WORKSPACE_COLOR_KIND_NEW,
            additional_prompt_scope: Some(AGENT_PROMPT_WORKFLOW_SCOPE),
            workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
            on_start_issue_id: None,
            prompt_intro: workflow_single_group_prompt_intro(),
            completion_section: Some(&completion_section),
            pre_snapshot_context: workflow_context.as_deref(),
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
        .filter_map(|state| task_store::prepared_branch_name(&state.document.branch))
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

pub(crate) fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<WtError>()
        .is_some_and(|err| matches!(err, WtError::Cancelled))
}

pub(super) fn task_issue_closing_references(document: &task_store::TaskDocument) -> Vec<String> {
    document
        .origin
        .as_ref()
        .map(|origin| origin.id.trim())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .into_iter()
        .collect()
}

fn workflow_issue_closing_references(states: &[WorkflowTaskState]) -> Vec<String> {
    states
        .iter()
        .flat_map(|state| task_issue_closing_references(&state.document))
        .collect()
}

pub(crate) fn apply_workflow_color(ctx: &Ctx, worktree_path: &Path, color: Option<&str>) {
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
    profiles: &[String],
) -> Result<PreparedWorkflowTasks> {
    if mode == WorkflowModeArg::Matrix {
        return matrix_workflow_tasks_from_prepared(ctx, workflow_path, prepared_tasks, profiles);
    }

    let group = task_run::group_from_path(workflow_path)?;
    let mut parent = Some(initial_parent.to_string());
    let mut tasks = Vec::new();
    let mut task_runs = Vec::new();
    for task in prepared_tasks {
        let run =
            match task_run::create(ctx, &task.key, &task.branch, Some(&group), STATUS_PREPARED) {
                Ok(run) => run,
                Err(err) => {
                    rollback_task_runs(&task_runs);
                    return Err(err);
                }
            };

        let mut row = WorkflowTask::new(task.key.clone(), run.id.clone());
        if mode == WorkflowModeArg::Stack {
            row.parent = parent.clone();
            parent = task_store::prepared_branch_name(&task.branch).map(str::to_string);
        }
        task_runs.push(run);
        tasks.push(row);
    }
    Ok(PreparedWorkflowTasks { tasks, task_runs })
}

fn matrix_workflow_tasks_from_prepared(
    ctx: &Ctx,
    workflow_path: &Path,
    prepared_tasks: Vec<PreparedTask>,
    profiles: &[String],
) -> Result<PreparedWorkflowTasks> {
    let Some(task) = prepared_tasks.into_iter().next() else {
        bail!("matrix mode workflow requires exactly one task");
    };
    let group = task_run::group_from_path(workflow_path)?;
    let mut task_runs = Vec::new();
    let mut runs = Vec::new();
    for profile in profiles {
        let branch = matrix_profile_branch(&task.branch, profile)?;
        let run = match task_run::create(ctx, &task.key, &branch, Some(&group), STATUS_PREPARED) {
            Ok(run) => run,
            Err(err) => {
                rollback_task_runs(&task_runs);
                return Err(err);
            }
        };
        runs.push(WorkflowTaskRun {
            profile: profile.clone(),
            run: run.id.clone(),
        });
        task_runs.push(run);
    }

    let mut row = WorkflowTask::new(task.key, "");
    row.runs = runs;
    Ok(PreparedWorkflowTasks {
        tasks: vec![row],
        task_runs,
    })
}

fn matrix_profile_branch(branch: &str, profile: &str) -> Result<String> {
    let Some(branch) = task_store::prepared_branch_name(branch) else {
        bail!("matrix mode workflow task has no branch");
    };
    Ok(format!("{branch}-{profile}"))
}

fn validate_mode_options(mode: WorkflowModeArg, pr: Option<WorkflowPrModeArg>) -> Result<()> {
    let _ = (mode, pr);
    Ok(())
}

fn normalized_objective(objective: Option<&str>) -> Result<Option<String>> {
    let Some(objective) = objective else {
        return Ok(None);
    };
    let objective = objective.trim();
    if objective.is_empty() {
        bail!("Workflow objective cannot be empty");
    }
    Ok(Some(objective.to_string()))
}

fn workflow_pr_base(base: &Option<String>) -> &str {
    base.as_deref().unwrap_or("<workflow-base>")
}

fn rollback_task_runs(task_runs: &[task_run::TaskRunRecord]) {
    for run in task_runs.iter().rev() {
        let _ = task_run::delete_record(run);
    }
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

fn workflow_default_policy(ctx: &Ctx, profile: Option<&str>) -> Result<WorkflowDefaultPolicy> {
    let Some(profile) = profile else {
        return Ok(ctx.config.workflow_default_policy());
    };

    let Some(config) = Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)? else {
        bail!("Profile '{profile}' not found");
    };
    Ok(config.workflow_default_policy())
}
