use crate::cli::{BaseMode, WorkflowModeArg};
use crate::commands::editor;
use crate::commands::issue_selection;
use crate::commands::task::{self as task_command, PreparedTask};
use crate::commands::task_run::{self, STATUS_PREPARED};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::services::git::GitService;
use crate::workflow as workflow_store;
use crate::workflow::{WorkflowMetadata, WorkflowMode, WorkflowTask};
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::path::Path;

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

pub fn run(_ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let Some(workflow) = workflow else {
        bail!(
            "wt workflow run requires a workflow id or path until runnable selection is implemented"
        );
    };
    bail!(
        "wt workflow run is not implemented yet for workflow {workflow}; mode-specific execution will be added by the workflow mode tasks"
    )
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
