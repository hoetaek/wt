use crate::cli::{WorkflowModeArg, WorkflowPrModeArg};
use crate::commands::editor;
use crate::commands::issue_selection;
use crate::commands::task as task_command;
#[cfg(test)]
use crate::config::{WorkflowDefaultLandingPolicy, WorkflowDefaultPullRequestMode};
use crate::context::Ctx;
use crate::task::{self as task_store, PreparedTask};
#[cfg(test)]
use crate::task_run::{self, STATUS_PREPARED};
#[cfg(test)]
use crate::task_run::{STATUS_DONE, STATUS_FAILED, STATUS_RUNNING, STATUS_SKIPPED};
use crate::workflow as workflow_store;
#[cfg(test)]
use crate::workflow::planner::parent_for_stack_task;
#[cfg(test)]
use crate::workflow::render::{
    render_single_workflow_snapshot, workflow_batch_task_prompt_content,
    workflow_single_task_prompt_content, workflow_stack_task_prompt_content,
};
use crate::workflow::run as workflow_runner;
#[cfg(test)]
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_single_workflow_task_states,
    read_stack_workflow_task_states, task_run_record,
};
#[cfg(test)]
use crate::workflow::{
    WorkflowLandingPolicy, WorkflowMetadata, WorkflowMode, WorkflowPullRequestMode, WorkflowTask,
};
use anyhow::{Result, bail};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

mod display;
mod repair;
mod selection;
mod stack_completion;

use display::show_workflow;
#[cfg(test)]
use selection::list_runnable_workflow_candidates;
use selection::resolve_run_workflow_path;
use stack_completion::complete_stack_workflow;

pub fn task(
    ctx: &Ctx,
    tasks: &[String],
    mode: WorkflowModeArg,
    profile: Option<&str>,
    objective: Option<&str>,
    base: &Option<String>,
    pr: Option<WorkflowPrModeArg>,
) -> Result<()> {
    workflow_runner::validate_prepare_options(ctx, mode, profile, pr)?;
    let prepared_tasks = if tasks.is_empty() {
        task_store::select_local_tasks(ctx)?
            .into_iter()
            .map(|task| PreparedTask {
                key: task.key,
                branch: task.document.branch,
            })
            .collect()
    } else {
        task_command::prepare_named_tasks(ctx, tasks)?
    };
    workflow_runner::prepare_workflow(ctx, mode, profile, objective, base, prepared_tasks, pr)
}

pub fn issue(
    ctx: &Ctx,
    issues: &[String],
    mode: WorkflowModeArg,
    profile: Option<&str>,
    objective: Option<&str>,
    base: &Option<String>,
    pr: Option<WorkflowPrModeArg>,
) -> Result<()> {
    workflow_runner::validate_prepare_options(ctx, mode, profile, pr)?;

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
    workflow_runner::prepare_workflow(ctx, mode, profile, objective, base, prepared_tasks, pr)
}

pub fn show(ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let path = resolve_read_target(ctx, workflow)?;
    let metadata = workflow_store::read(&path)?;
    show_workflow(ctx, &path, &metadata)
}

pub fn edit(ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let path = resolve_read_target(ctx, workflow)?;
    editor::open_file(ctx, &path)
}

pub fn repair(ctx: &Ctx, workflow: &str, apply: bool) -> Result<()> {
    repair::run(ctx, workflow, apply)
}

pub fn run(ctx: &Ctx, workflow: Option<&str>, jobs: usize) -> Result<()> {
    let Some(path) = resolve_run_workflow_path(ctx, workflow)? else {
        return Ok(());
    };
    workflow_runner::run_workflow(ctx, &path, jobs)
}

pub fn complete(ctx: &Ctx, workflow: &str, task: Option<&str>, run_next: bool) -> Result<()> {
    complete_stack_workflow(ctx, workflow, task, run_next)
}

fn resolve_read_target(ctx: &Ctx, workflow: Option<&str>) -> Result<std::path::PathBuf> {
    workflow_store::resolve(ctx, workflow.unwrap_or("latest"))
}

pub(super) fn resolve_mutating_target(ctx: &Ctx, workflow: &str, command: &str) -> Result<PathBuf> {
    if workflow == "latest" {
        bail!(
            "wt workflow {command} latest is not supported; pass a workflow path or id explicitly"
        );
    }
    workflow_store::resolve(ctx, workflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig, WorkflowDefaultsConfig};
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::fs;
    use std::sync::Arc;

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_config(root: &Path, config: Config) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_config_and_runner(root: &Path, config: Config, runner: MockRunner) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_ui(root: &Path, ui: Arc<MockUi>) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        )
    }

    fn ctx_with_runner_ui(root: &Path, runner: MockRunner, ui: Arc<MockUi>) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        )
    }

    fn prepare_workflow(
        ctx: &Ctx,
        mode: WorkflowModeArg,
        task_titles: &[&str],
    ) -> workflow_store::WorkflowRecord {
        let tasks = task_titles
            .iter()
            .map(|title| title.to_string())
            .collect::<Vec<_>>();
        task(ctx, &tasks, mode, None, None, &Some("main".into()), None).unwrap();
        workflow_store::list(ctx).unwrap().pop().unwrap()
    }

    fn update_task_run(
        ctx: &Ctx,
        row: &WorkflowTask,
        status: task_run::TaskRunStatus,
        branch: Option<&str>,
    ) {
        task_run::update(ctx, &row.run, status, branch, None).unwrap();
    }

    fn write_workflow_with_parent(
        ctx: &Ctx,
        record: &mut workflow_store::WorkflowRecord,
        idx: usize,
        parent: &str,
    ) {
        record.workflow.tasks[idx].parent = Some(parent.into());
        workflow_store::write(ctx, &record.path, &mut record.workflow).unwrap();
    }

    fn candidate_ids(ctx: &Ctx) -> Vec<String> {
        list_runnable_workflow_candidates(ctx)
            .unwrap()
            .into_iter()
            .map(|candidate| candidate.id)
            .collect()
    }

    fn assert_report_only_workflow_handoff(content: &str) {
        assert!(content.contains("## Workflow Coordinator Handoff"));
        assert!(content.contains("This workflow mode has no pull-request handoff intent"));
        assert!(content.contains("PR=none"));
        assert!(content.contains("cmux send --workspace {{coordinator_cmux_workspace}} --surface {{coordinator_cmux_surface}} \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>\""));
        assert!(content.contains("{{coordinator_enter_command}}"));
        assert!(content.contains("coordinator cmux target is unavailable or stale"));
        assert!(!content.contains("wt workflow complete"));
    }

    fn assert_workflow_handoff_precedes_task_body(content: &str, body: &str) {
        assert!(
            content.find("## Workflow Coordinator Handoff").unwrap() < content.find(body).unwrap()
        );
    }

    fn assert_workflow_send_command_precedes_policy(content: &str) {
        assert!(
            content.find("cmux send --workspace").unwrap()
                < content
                    .find("Workflow task metadata sets")
                    .unwrap_or(content.len())
        );
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
            None,
            &Some("main".into()),
            None,
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
        assert_eq!(
            workflow.policy.as_ref().unwrap().landing,
            WorkflowLandingPolicy::Manual
        );
        assert!(workflow.policy.as_ref().unwrap().landing_requires_approval);
        assert_eq!(task_run::list(&ctx).unwrap().len(), 2);
    }

    #[test]
    fn task_prepares_workflow_with_objective_and_show_displays_it() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));

        task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Batch,
            None,
            Some("Ship the larger workflow migration"),
            &Some("main".into()),
            None,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.objective.as_deref(),
            Some("Ship the larger workflow migration")
        );
        let content = std::fs::read_to_string(&record.path).unwrap();
        assert!(content.contains("objective = \"Ship the larger workflow migration\""));

        show(&ctx, Some(&record.id)).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("Objective: Ship the larger workflow migration"));
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
            None,
            &Some("main".into()),
            None,
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
            None,
            &Some("main".into()),
            None,
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
            None,
            &Some("main".into()),
            None,
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
            None,
            &Some("main".into()),
            None,
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
    fn sequential_batch_workflow_stops_after_user_cancellation() {
        let dir = tempfile::tempdir().unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true); // checked_out_path for the cancelled task
        let mut ui = MockUi::new();
        ui.add_select(2); // Abort at the existing-worktree prompt
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        task(
            &ctx,
            &["cancel first".into(), "should wait".into()],
            WorkflowModeArg::Batch,
            None,
            None,
            &Some("main".into()),
            None,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        let first_task =
            task_store::read_task_document(&ctx, &record.workflow.tasks[0].task).unwrap();
        let first_worktree = crate::names::WorktreeNames::new_with_workspace_config(
            &first_task.branch,
            &ctx.parent_dir,
            &ctx.repo_root,
            &ctx.repo_name,
            None,
            None,
            None,
        )
        .unwrap()
        .path;
        std::fs::create_dir_all(first_worktree).unwrap();

        let err = run(&ctx, Some(record.path.to_str().unwrap()), 1).unwrap_err();

        assert!(err.to_string().contains("Workflow batch failed"));
        let first_run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        let second_run = task_run_record(&ctx, &record.workflow.tasks[1].run).unwrap();
        assert_eq!(first_run.status, STATUS_SKIPPED);
        assert_eq!(first_run.error.as_deref(), Some("User cancelled"));
        assert_eq!(second_run.status, STATUS_SKIPPED);
        assert_eq!(
            second_run.error.as_deref(),
            Some("Skipped after user cancellation")
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
            None,
            &Some("main".into()),
            None,
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
    fn task_prepares_stack_mode_workflow_with_parents_and_pr_modes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["contract".into(), "state model".into()],
            WorkflowModeArg::Stack,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::Draft),
        )
        .unwrap();

        let workflow = workflow_store::list(&ctx).unwrap().remove(0).workflow;
        assert_eq!(workflow.mode, WorkflowMode::Stack);
        assert!(workflow.profile.is_none());
        assert_eq!(workflow.tasks[0].parent.as_deref(), Some("main"));
        assert_eq!(
            workflow.tasks[0].pull_request,
            Some(WorkflowPullRequestMode::Draft)
        );
        assert_eq!(workflow.tasks[1].parent.as_deref(), Some("contract"));
        assert_eq!(
            workflow.tasks[1].pull_request,
            Some(WorkflowPullRequestMode::Draft)
        );
    }

    #[test]
    fn task_prepares_stack_mode_workflow_without_pr_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["contract".into()],
            WorkflowModeArg::Stack,
            None,
            None,
            &Some("main".into()),
            None,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(record.workflow.tasks[0].pull_request, None);
        assert_eq!(
            record.workflow.policy.as_ref().unwrap().landing,
            WorkflowLandingPolicy::Manual
        );
        assert!(
            record
                .workflow
                .policy
                .as_ref()
                .unwrap()
                .landing_requires_approval
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("[policy]"));
        assert!(content.contains("landing = \"manual\""));
        assert!(content.contains("landing_requires_approval = true"));
        assert!(!content.contains("pull_request"));
    }

    #[test]
    fn task_applies_workflow_defaults_to_stack_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            workflow: crate::config::WorkflowConfig {
                defaults: WorkflowDefaultsConfig {
                    pull_request: Some(WorkflowDefaultPullRequestMode::Draft),
                    landing: Some(WorkflowDefaultLandingPolicy::AfterReview),
                    landing_requires_approval: Some(false),
                },
            },
            ..Config::default()
        };
        let ctx = ctx_with_config(dir.path(), config);

        task(
            &ctx,
            &["contract".into()],
            WorkflowModeArg::Stack,
            None,
            None,
            &Some("main".into()),
            None,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.tasks[0].pull_request,
            Some(WorkflowPullRequestMode::Draft)
        );
        assert_eq!(
            record.workflow.policy.as_ref().unwrap().landing,
            WorkflowLandingPolicy::AfterReview
        );
        assert!(
            !record
                .workflow
                .policy
                .as_ref()
                .unwrap()
                .landing_requires_approval
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("pull_request = \"draft\""));
        assert!(content.contains("[policy]"));
        assert!(content.contains("landing = \"after_review\""));
        assert!(content.contains("landing_requires_approval = false"));
    }

    #[test]
    fn explicit_pr_none_overrides_config_default() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            workflow: crate::config::WorkflowConfig {
                defaults: WorkflowDefaultsConfig {
                    pull_request: Some(WorkflowDefaultPullRequestMode::Ready),
                    landing: Some(WorkflowDefaultLandingPolicy::Manual),
                    landing_requires_approval: Some(true),
                },
            },
            ..Config::default()
        };
        let ctx = ctx_with_config(dir.path(), config);

        task(
            &ctx,
            &["contract".into()],
            WorkflowModeArg::Stack,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::None),
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(record.workflow.tasks[0].pull_request, None);
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(!content.contains("pull_request"));
    }

    #[test]
    fn issue_applies_workflow_defaults_to_stack_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            workflow: crate::config::WorkflowConfig {
                defaults: WorkflowDefaultsConfig {
                    pull_request: Some(WorkflowDefaultPullRequestMode::Ready),
                    landing: Some(WorkflowDefaultLandingPolicy::AfterReview),
                    landing_requires_approval: Some(true),
                },
            },
            ..Config::default()
        };
        let ctx = ctx_with_config_and_runner(dir.path(), config, runner);

        issue(
            &ctx,
            &["PROJ-123".into()],
            WorkflowModeArg::Stack,
            None,
            None,
            &Some("main".into()),
            None,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.tasks[0].pull_request,
            Some(WorkflowPullRequestMode::Ready)
        );
        assert_eq!(
            record.workflow.policy.as_ref().unwrap().landing,
            WorkflowLandingPolicy::AfterReview
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("pull_request = \"ready\""));
        assert!(content.contains("[policy]"));
    }

    #[test]
    fn task_prepares_stack_mode_workflow_with_ready_pr_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["contract".into()],
            WorkflowModeArg::Stack,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::Ready),
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.tasks[0].pull_request,
            Some(WorkflowPullRequestMode::Ready)
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("pull_request = \"ready\""));
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
            None,
            &Some("main".into()),
            None,
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
            parent_for_stack_task(&record.workflow, &states, 2).unwrap(),
            "schema"
        );
    }

    #[test]
    fn workflow_complete_rejects_dirty_stack_task_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-feature\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response(" M src/lib.rs", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let mut record = prepare_workflow(&ctx, WorkflowModeArg::Stack, &["feature"]);
        write_workflow_with_parent(&ctx, &mut record, 0, "main");
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );

        let err =
            complete(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

        assert!(err.to_string().contains("uncommitted changes"));
        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_RUNNING);
    }

    #[test]
    fn workflow_complete_rejects_stack_task_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-feature\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("0", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let mut record = prepare_workflow(&ctx, WorkflowModeArg::Stack, &["feature"]);
        write_workflow_with_parent(&ctx, &mut record, 0, "main");
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );

        let err =
            complete(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

        assert!(err.to_string().contains("no commits ahead"));
        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_RUNNING);
    }

    #[test]
    fn workflow_repair_preview_reports_running_task_without_worktree_without_mutating() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", false);
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_ui(dir.path(), runner, ui.clone());
        let record = prepare_workflow(&ctx, WorkflowModeArg::Stack, &["feature"]);
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );

        repair(&ctx, record.path.to_str().unwrap(), false).unwrap();

        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_RUNNING);
        let dims = ui.dims.lock().unwrap().join("\n");
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(dims.contains("Running TaskRun has no usable local worktree"));
        assert!(dims.contains("Action: mark TaskRun failed (requires --apply)"));
        assert!(steps.contains("Preview only; no TaskRun state changed"));
    }

    #[test]
    fn workflow_repair_apply_marks_running_task_without_live_surface_failed() {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join("wt-feature");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("", false);
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_ui(dir.path(), runner, ui.clone());
        let record = prepare_workflow(&ctx, WorkflowModeArg::Stack, &["feature"]);
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );

        repair(&ctx, record.path.to_str().unwrap(), true).unwrap();

        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_FAILED);
        assert!(
            run.error
                .as_deref()
                .unwrap_or_default()
                .contains("no live validated agent surface")
        );
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Applied 1 workflow runtime repair."));
    }

    #[test]
    fn workflow_repair_does_not_flag_ordinary_prepared_task_without_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", false);
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_ui(dir.path(), runner, ui.clone());
        let record = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["feature"]);

        repair(&ctx, record.path.to_str().unwrap(), false).unwrap();

        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_PREPARED);
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("No workflow runtime repairs recommended."));
        assert!(ui.warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn workflow_repair_apply_marks_prepared_prompt_delivery_failure_failed() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), ui);
        let record = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["feature"]);
        task_run::update(
            &ctx,
            &record.workflow.tasks[0].run,
            STATUS_PREPARED,
            None,
            Some("Agent prompt 1/1 failed: unchanged screen before delivery"),
        )
        .unwrap();

        repair(&ctx, record.path.to_str().unwrap(), true).unwrap();

        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_FAILED);
        assert_eq!(
            run.error.as_deref(),
            Some("Agent prompt 1/1 failed: unchanged screen before delivery")
        );
    }

    #[test]
    fn workflow_complete_with_run_next_starts_next_stack_task() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-schema\nHEAD def\nbranch refs/heads/schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-schema\nHEAD def\nbranch refs/heads/schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let mut record = prepare_workflow(&ctx, WorkflowModeArg::Stack, &["schema", "api"]);
        write_workflow_with_parent(&ctx, &mut record, 0, "main");
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("schema"),
        );
        let first_run = record.workflow.tasks[0].run.clone();
        let second_run = record.workflow.tasks[1].run.clone();

        complete(&ctx, record.path.to_str().unwrap(), Some("schema"), true).unwrap();
        let updated = workflow_store::read(&record.path).unwrap();

        assert_eq!(
            task_run_record(&ctx, &first_run).unwrap().status,
            STATUS_DONE
        );
        assert_eq!(
            task_run_record(&ctx, &second_run).unwrap().status,
            STATUS_RUNNING
        );
        assert_eq!(updated.tasks[1].parent.as_deref(), Some("schema"));
    }

    #[test]
    fn workflow_stack_prompt_uses_draft_pr_handoff_policy() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            pull_request: Some(WorkflowPullRequestMode::Draft),
        };
        let workflow_path = PathBuf::from("/repo/.local/workflows/2026-05-16-001.toml");

        let content = workflow_stack_task_prompt_content("title = \"API\"\n", &workflow_path, &row);

        assert!(content.contains("## Workflow Coordinator Handoff"));
        assert_workflow_handoff_precedes_task_body(&content, "title = \"API\"");
        assert_workflow_send_command_precedes_policy(&content);
        assert!(content.contains("Workflow task metadata sets `pull_request = \"draft\"`"));
        assert!(content.contains("gh pr create --draft --body-file <pr-body-file>"));
        assert!(content.contains(".github/pull_request_template.md"));
        assert!(!content.contains("gh pr ready"));
        assert!(!content.contains("gh pr edit --body-file"));
        assert!(
            content.contains("If Codex/GitHub review or coordinator feedback asks for changes")
        );
        assert!(content.contains("update the pull request body if it became stale"));
        assert!(content.contains("cmux send --workspace {{coordinator_cmux_workspace}} --surface {{coordinator_cmux_surface}} \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr-url>; Risks or follow-ups=<risks>\""));
        assert!(content.contains("{{coordinator_enter_command}}"));
        assert!(content.contains(
            "wt workflow complete /repo/.local/workflows/2026-05-16-001.toml PROJ-2 --run-next"
        ));
    }

    #[test]
    fn workflow_stack_prompt_uses_ready_pr_handoff_policy() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            pull_request: Some(WorkflowPullRequestMode::Ready),
        };
        let workflow_path = PathBuf::from("/repo/.local/workflows/2026-05-16-001.toml");

        let content = workflow_stack_task_prompt_content("title = \"API\"\n", &workflow_path, &row);

        assert!(content.contains("Workflow task metadata sets `pull_request = \"ready\"`"));
        assert!(content.contains("gh pr create --body-file <pr-body-file>"));
        assert!(!content.contains("gh pr create --draft"));
        assert!(!content.contains("gh pr ready"));
        assert!(content.contains("PR=<pr-url>"));
    }

    #[test]
    fn workflow_stack_prompt_reports_none_without_pull_request_intent() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            pull_request: None,
        };
        let workflow_path = PathBuf::from("/repo/.local/workflows/2026-05-16-001.toml");

        let content = workflow_stack_task_prompt_content("title = \"API\"\n", &workflow_path, &row);

        assert!(content.contains("Workflow task metadata omits `pull_request`"));
        assert!(content.contains("do not open a pull request for this workflow task"));
        assert!(content.contains("If coordinator feedback asks for changes"));
        assert!(!content.contains("If Codex/GitHub review"));
        assert!(content.contains("PR=none"));
        assert!(!content.contains("gh pr create"));
        assert!(content.contains(
            "wt workflow complete /repo/.local/workflows/2026-05-16-001.toml PROJ-2 --run-next"
        ));
    }

    #[test]
    fn workflow_single_prompt_includes_report_only_coordinator_handoff() {
        let content = workflow_single_task_prompt_content("title = \"API\"\n");

        assert_report_only_workflow_handoff(&content);
        assert_workflow_handoff_precedes_task_body(&content, "title = \"API\"");
        assert!(
            content.find("cmux send --workspace").unwrap()
                < content.find("title = \"API\"").unwrap()
        );
    }

    #[test]
    fn workflow_grouped_single_prompt_includes_report_only_coordinator_handoff() {
        let states = vec![
            WorkflowTaskState {
                idx: 0,
                row: WorkflowTask {
                    task: "api".into(),
                    run: "run-api".into(),
                    parent: None,
                    pull_request: None,
                },
                document: task_store::TaskDocument {
                    title: "API".into(),
                    branch: "shared".into(),
                    body: String::new(),
                    origin: None,
                },
                path: ".local/tasks/api.toml".into(),
                content: "title = \"API\"\nbranch = \"shared\"\n".into(),
                run: task_run::TaskRun {
                    task: "api".into(),
                    branch: "shared".into(),
                    status: STATUS_PREPARED,
                    source: task_run::SOURCE_NEW,
                    group: None,
                    error: None,
                    creation_order: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            },
            WorkflowTaskState {
                idx: 1,
                row: WorkflowTask {
                    task: "docs".into(),
                    run: "run-docs".into(),
                    parent: None,
                    pull_request: None,
                },
                document: task_store::TaskDocument {
                    title: "Docs".into(),
                    branch: "shared".into(),
                    body: String::new(),
                    origin: None,
                },
                path: ".local/tasks/docs.toml".into(),
                content: "title = \"Docs\"\nbranch = \"shared\"\n".into(),
                run: task_run::TaskRun {
                    task: "docs".into(),
                    branch: "shared".into(),
                    status: STATUS_PREPARED,
                    source: task_run::SOURCE_NEW,
                    group: None,
                    error: None,
                    creation_order: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            },
        ];
        let content =
            workflow_single_task_prompt_content(&render_single_workflow_snapshot(&states));

        assert!(content.contains("Selected TaskDocuments:"));
        assert_report_only_workflow_handoff(&content);
    }

    #[test]
    fn workflow_batch_prompt_includes_report_only_coordinator_handoff() {
        let content = workflow_batch_task_prompt_content("title = \"API\"\n");

        assert_report_only_workflow_handoff(&content);
    }

    #[test]
    fn pr_modes_require_stack_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let err = task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Batch,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::Draft),
        )
        .unwrap_err();

        assert!(err.to_string().contains("--pr is only valid"));
    }

    #[test]
    fn pr_none_requires_stack_mode_for_task_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let err = task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Single,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::None),
        )
        .unwrap_err();

        assert!(err.to_string().contains("--pr is only valid"));
    }

    #[test]
    fn pr_none_requires_stack_mode_for_issue_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let err = issue(
            &ctx,
            &["PROJ-123".into()],
            WorkflowModeArg::Batch,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::None),
        )
        .unwrap_err();

        assert!(err.to_string().contains("--pr is only valid"));
    }

    fn replace_first_workflow_run_with_foreign_group(
        ctx: &Ctx,
        workflow: &mut WorkflowMetadata,
        source: task_run::TaskRunSource,
    ) -> String {
        let row = workflow.tasks.first_mut().unwrap();
        let document = task_store::read_task_document(ctx, &row.task).unwrap();
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

    #[test]
    fn runnable_workflow_candidates_filter_single_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let prepared = prepare_workflow(&ctx, WorkflowModeArg::Single, &["ready single"]);
        let done = prepare_workflow(&ctx, WorkflowModeArg::Single, &["done single"]);
        update_task_run(
            &ctx,
            &done.workflow.tasks[0],
            STATUS_DONE,
            Some("done-single"),
        );
        let running = prepare_workflow(&ctx, WorkflowModeArg::Single, &["running single"]);
        update_task_run(
            &ctx,
            &running.workflow.tasks[0],
            STATUS_RUNNING,
            Some("running-single"),
        );

        assert_eq!(candidate_ids(&ctx), vec![prepared.id]);
    }

    #[test]
    fn runnable_workflow_candidates_filter_batch_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let mixed = prepare_workflow(
            &ctx,
            WorkflowModeArg::Batch,
            &["running batch", "ready batch"],
        );
        update_task_run(
            &ctx,
            &mixed.workflow.tasks[0],
            STATUS_RUNNING,
            Some("running-batch"),
        );
        let done_only = prepare_workflow(
            &ctx,
            WorkflowModeArg::Batch,
            &["done batch", "skipped batch"],
        );
        update_task_run(
            &ctx,
            &done_only.workflow.tasks[0],
            STATUS_DONE,
            Some("done-batch"),
        );
        update_task_run(
            &ctx,
            &done_only.workflow.tasks[1],
            STATUS_SKIPPED,
            Some("skipped-batch"),
        );

        assert_eq!(candidate_ids(&ctx), vec![mixed.id]);
    }

    #[test]
    fn runnable_workflow_candidates_filter_stack_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let first_ready = prepare_workflow(
            &ctx,
            WorkflowModeArg::Stack,
            &["first ready", "second ready"],
        );
        let running = prepare_workflow(
            &ctx,
            WorkflowModeArg::Stack,
            &["running stack", "blocked stack"],
        );
        update_task_run(
            &ctx,
            &running.workflow.tasks[0],
            STATUS_RUNNING,
            Some("running-stack"),
        );
        let retry_second = prepare_workflow(
            &ctx,
            WorkflowModeArg::Stack,
            &["finished stack", "retry stack"],
        );
        update_task_run(
            &ctx,
            &retry_second.workflow.tasks[0],
            STATUS_DONE,
            Some("finished-stack"),
        );
        update_task_run(&ctx, &retry_second.workflow.tasks[1], STATUS_FAILED, None);
        let done_only = prepare_workflow(
            &ctx,
            WorkflowModeArg::Stack,
            &["done stack", "skipped stack"],
        );
        update_task_run(
            &ctx,
            &done_only.workflow.tasks[0],
            STATUS_DONE,
            Some("done-stack"),
        );
        update_task_run(
            &ctx,
            &done_only.workflow.tasks[1],
            STATUS_SKIPPED,
            Some("skipped-stack"),
        );

        assert_eq!(candidate_ids(&ctx), vec![first_ready.id, retry_second.id]);
    }

    #[test]
    fn bare_workflow_run_with_one_candidate_returns_it_without_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let workflow = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["only runnable"]);

        let path = resolve_run_workflow_path(&ctx, None).unwrap().unwrap();

        assert_eq!(path, workflow.path);
        assert!(ui.prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn bare_workflow_run_with_multiple_candidates_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1);
        let ui = Arc::new(ui);
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let first = prepare_workflow(&ctx, WorkflowModeArg::Single, &["first workflow"]);
        let second = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["second workflow"]);

        let path = resolve_run_workflow_path(&ctx, None).unwrap().unwrap();

        assert_eq!(path, second.path);
        assert_eq!(
            ui.prompts.lock().unwrap().as_slice(),
            ["select: Workflow to run"]
        );
        let items = ui.select_items.lock().unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0][0].contains(&first.id));
        assert!(items[0][0].contains("mode single"));
        assert!(items[0][1].contains(&second.id));
        assert!(items[0][1].contains("mode batch"));
    }

    #[test]
    fn bare_workflow_run_with_multiple_candidates_non_interactive_lists_targets() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.set_prompt_available(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let first = prepare_workflow(&ctx, WorkflowModeArg::Single, &["first noninteractive"]);
        let second = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["second noninteractive"]);
        let first_run_path = task_run::resolve(&ctx, &first.workflow.tasks[0].run).unwrap();
        let second_run_path = task_run::resolve(&ctx, &second.workflow.tasks[0].run).unwrap();
        let first_before = fs::read_to_string(&first_run_path).unwrap();
        let second_before = fs::read_to_string(&second_run_path).unwrap();

        let err = resolve_run_workflow_path(&ctx, None).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("Multiple runnable workflows found"));
        assert!(message.contains(&format!("wt workflow run {}", first.id)));
        assert!(message.contains(&format!("wt workflow run {}", second.id)));
        assert!(message.contains(".local/workflows/"));
        assert!(ui.prompts.lock().unwrap().is_empty());
        assert_eq!(fs::read_to_string(first_run_path).unwrap(), first_before);
        assert_eq!(fs::read_to_string(second_run_path).unwrap(), second_before);
    }

    #[test]
    fn bare_workflow_run_without_runnable_workflows_warns_without_state_changes() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let workflow = prepare_workflow(&ctx, WorkflowModeArg::Single, &["already done"]);
        update_task_run(
            &ctx,
            &workflow.workflow.tasks[0],
            STATUS_DONE,
            Some("already-done"),
        );
        let run_path = task_run::resolve(&ctx, &workflow.workflow.tasks[0].run).unwrap();
        let workflow_before = fs::read_to_string(&workflow.path).unwrap();
        let run_before = fs::read_to_string(&run_path).unwrap();

        run(&ctx, None, 1).unwrap();

        assert_eq!(
            ui.warnings.lock().unwrap().as_slice(),
            ["No runnable workflows found"]
        );
        assert_eq!(fs::read_to_string(workflow.path).unwrap(), workflow_before);
        assert_eq!(fs::read_to_string(run_path).unwrap(), run_before);
    }
}
