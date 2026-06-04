use crate::cli::{WorkflowModeArg, WorkflowPrModeArg};
use crate::commands::editor;
use crate::commands::issue_selection;
use crate::commands::task as task_command;
#[cfg(test)]
use crate::config::{
    ReviewCodexBasePolicy, ReviewConfig, WorkflowDefaultLandingPolicy,
    WorkflowDefaultPullRequestMode,
};
use crate::context::Ctx;
use crate::task::{self as task_store, PreparedTask};
#[cfg(test)]
use crate::task_run::{self, STATUS_PREPARED};
#[cfg(test)]
use crate::task_run::{STATUS_FAILED, STATUS_PASSED, STATUS_RUNNING, STATUS_SKIPPED};
use crate::workflow as workflow_store;
#[cfg(test)]
use crate::workflow::planner::parent_for_stack_task;
use crate::workflow::render::shell_arg;
#[cfg(test)]
use crate::workflow::render::{
    render_single_workflow_snapshot, stack_task_already_running_message,
    started_stack_task_message, test_auto_landing_policy, test_workflow_policy,
    workflow_batch_task_prompt_content, workflow_batch_task_prompt_content_for_policy,
    workflow_matrix_task_handoff_section, workflow_metadata_prompt_context,
    workflow_single_task_prompt_content, workflow_single_task_prompt_content_for_policy,
    workflow_single_task_prompt_content_for_policy_and_closing_refs,
    workflow_stack_task_prompt_content, workflow_task_prompt_content_with_policy,
    workflow_task_prompt_content_with_policy_and_parent,
};
use crate::workflow::run as workflow_runner;
#[cfg(test)]
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_single_workflow_task_states,
    read_stack_workflow_task_states, task_run_record,
};
#[cfg(test)]
use crate::workflow::{
    WorkflowCodexBaseReview, WorkflowLandingPolicy, WorkflowMetadata, WorkflowMode, WorkflowOrigin,
    WorkflowPullRequestMode, WorkflowTask,
};
use anyhow::{Result, bail};
use std::env;
use std::path::{Path, PathBuf};

mod archive;
mod display;
mod list_command;
mod repair;
mod selection;
mod show_command;
mod stack_completion;
mod watch_command;

use display::show_workflow;
#[cfg(test)]
use selection::list_runnable_workflow_candidates;
use selection::resolve_run_workflow_path;
use show_command::show_workflow_json;
use stack_completion::pass_workflow;

pub fn archive(ctx: &Ctx, workflow: &str) -> Result<()> {
    archive::run(ctx, workflow)
}

pub(crate) fn active_inventory_issues(
    ctx: &Ctx,
) -> Result<Vec<list_command::ActiveWorkflowInventoryIssue>> {
    list_command::active_inventory_issues(ctx)
}

pub fn list(ctx: &Ctx) -> Result<()> {
    list_command::run(ctx)
}

pub struct TaskOptions<'a> {
    pub mode: WorkflowModeArg,
    pub profile: Option<&'a str>,
    pub profiles: &'a [String],
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub body_file: Option<&'a Path>,
    pub origin_provider: Option<&'a str>,
    pub origin_id: Option<&'a str>,
    pub base: &'a Option<String>,
    pub pr: Option<WorkflowPrModeArg>,
}

pub struct IssueOptions<'a> {
    pub mode: WorkflowModeArg,
    pub profile: Option<&'a str>,
    pub title: Option<&'a str>,
    pub body: Option<&'a str>,
    pub body_file: Option<&'a Path>,
    pub origin_provider: Option<&'a str>,
    pub origin_id: Option<&'a str>,
    pub base: &'a Option<String>,
    pub pr: Option<WorkflowPrModeArg>,
}

pub fn task(ctx: &Ctx, tasks: &[String], options: TaskOptions<'_>) -> Result<()> {
    workflow_runner::validate_prepare_options(
        ctx,
        options.mode,
        options.profile,
        options.profiles,
        options.pr,
    )?;
    workflow_runner::validate_workflow_metadata_options(
        options.title,
        options.body,
        options.body_file,
        options.origin_provider,
        options.origin_id,
    )?;
    if options.mode == WorkflowModeArg::Matrix && tasks.len() > 1 {
        bail!("matrix mode workflow requires exactly one task");
    }
    let prepared_tasks = if tasks.is_empty() {
        let selected = task_store::select_local_task_documents(ctx)?;
        if options.mode == WorkflowModeArg::Matrix && selected.len() != 1 {
            bail!("matrix mode workflow requires exactly one task");
        }
        selected
            .into_iter()
            .map(|task| PreparedTask {
                key: task.key,
                branch: task.document.branch,
            })
            .collect()
    } else {
        task_command::prepare_named_tasks(ctx, tasks)?
    };
    workflow_runner::prepare_workflow(
        ctx,
        workflow_runner::PrepareWorkflowOptions {
            mode: options.mode,
            profile: options.profile,
            profiles: options.profiles,
            title: options.title,
            body: options.body,
            body_file: options.body_file,
            origin_provider: options.origin_provider,
            origin_id: options.origin_id,
            base: options.base,
            pr: options.pr,
        },
        prepared_tasks,
    )
}

pub fn issue(ctx: &Ctx, issues: &[String], options: IssueOptions<'_>) -> Result<()> {
    if options.mode == WorkflowModeArg::Matrix {
        bail!(
            "wt workflow issue does not support mode matrix; use wt workflow task --mode matrix with one local TaskDocument"
        );
    }
    workflow_runner::validate_prepare_options(ctx, options.mode, options.profile, &[], options.pr)?;
    workflow_runner::validate_workflow_metadata_options(
        options.title,
        options.body,
        options.body_file,
        options.origin_provider,
        options.origin_id,
    )?;

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
    workflow_runner::prepare_workflow(
        ctx,
        workflow_runner::PrepareWorkflowOptions {
            mode: options.mode,
            profile: options.profile,
            profiles: &[],
            title: options.title,
            body: options.body,
            body_file: options.body_file,
            origin_provider: options.origin_provider,
            origin_id: options.origin_id,
            base: options.base,
            pr: options.pr,
        },
        prepared_tasks,
    )
}

pub fn show(ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let path = resolve_read_target(ctx, workflow)?;
    let metadata = workflow_store::read(&path)?;
    if ctx.is_json() {
        return show_workflow_json(ctx, &path, &metadata);
    }
    show_workflow(ctx, &path, &metadata)
}

pub fn watch(
    ctx: &Ctx,
    workflow: Option<&str>,
    interval_secs: u64,
    timeout_secs: Option<u64>,
    heartbeat_secs: Option<u64>,
) -> Result<()> {
    watch_command::run(ctx, workflow, interval_secs, timeout_secs, heartbeat_secs)
}

pub fn edit(ctx: &Ctx, workflow: Option<&str>) -> Result<()> {
    let path = resolve_read_target(ctx, workflow)?;
    editor::open_file(ctx, &path)
}

pub fn repair(ctx: &Ctx, workflow: &str, apply: bool) -> Result<()> {
    repair::run(ctx, workflow, apply)
}

pub fn run(ctx: &Ctx, workflow: Option<&str>, jobs: usize) -> Result<()> {
    require_coordinator_session_for_workflow_run()?;
    run_after_coordinator_session_check(ctx, workflow, jobs)
}

fn run_after_coordinator_session_check(
    ctx: &Ctx,
    workflow: Option<&str>,
    jobs: usize,
) -> Result<()> {
    let Some(path) = resolve_run_workflow_path(ctx, workflow)? else {
        return Ok(());
    };
    workflow_runner::run_workflow(ctx, &path, jobs)
}

fn require_coordinator_session_for_workflow_run() -> Result<()> {
    let has_agent_id =
        env::var_os("WT_AGENT_ID").is_some_and(|value| !value.to_string_lossy().trim().is_empty());
    if has_agent_id {
        return Ok(());
    }

    bail!(
        "wt workflow run requires a coordinator session.\n\
         Set WT_AGENT_ID in this shell first, for example:\n\n\
             eval \"$(wt session set coord-<work-slug>)\"\n\n\
         Pick a semantic one-segment name for this work, such as coord-review-routing or coord-release-prep."
    )
}

pub fn pass(ctx: &Ctx, workflow: &str, task: Option<&str>, run_next: bool) -> Result<()> {
    pass_workflow(ctx, workflow, task, run_next)
}

pub fn deprecated_complete(workflow: &str, task: Option<&str>, run_next: bool) -> Result<()> {
    let mut suggestion = format!("wt workflow pass {}", shell_arg(workflow));
    if let Some(task) = task {
        suggestion.push(' ');
        suggestion.push_str(&shell_arg(task));
    }
    if run_next {
        suggestion.push_str(" --run-next");
    }
    bail!(
        "`wt workflow complete` has been replaced by `wt workflow pass`; run `{suggestion}` instead"
    )
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
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx, PromptRow};
    use std::fs;
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&Path>,
        ) -> Result<crate::context::CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_launcher(root: &Path, launcher: &str) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            crate::context::CtxOptions {
                launcher_coordinator_id: Some(launcher.into()),
                ..crate::context::CtxOptions::default()
            },
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

    fn write_profile(root: &Path, name: &str) {
        let profile_dir = root.join(".wt/config/profiles").join(name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("profile.toml"),
            r#"
[agent]
cli = "none"
"#,
        )
        .unwrap();
    }

    fn write_task(root: &Path, key: &str, content: &str) {
        let tasks_dir = root.join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(tasks_dir.join(format!("{key}.toml")), content).unwrap();
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn task(
        ctx: &Ctx,
        tasks: &[String],
        mode: WorkflowModeArg,
        profile: Option<&str>,
        title: Option<&str>,
        base: &Option<String>,
        pr: Option<WorkflowPrModeArg>,
    ) -> Result<()> {
        super::task(
            ctx,
            tasks,
            TaskOptions {
                mode,
                profile,
                profiles: &[],
                title,
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base,
                pr,
            },
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

    fn prepare_matrix_workflow(
        ctx: &Ctx,
        tasks: &[String],
        profiles: &[String],
    ) -> workflow_store::WorkflowRecord {
        matrix_task(ctx, tasks, profiles).unwrap();
        workflow_store::list(ctx).unwrap().pop().unwrap()
    }

    fn matrix_task(ctx: &Ctx, tasks: &[String], profiles: &[String]) -> Result<()> {
        let base = Some("main".into());
        super::task(
            ctx,
            tasks,
            TaskOptions {
                mode: WorkflowModeArg::Matrix,
                profile: None,
                profiles,
                title: None,
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base: &base,
                pr: None,
            },
        )
    }

    fn update_task_run(
        ctx: &Ctx,
        row: &WorkflowTask,
        status: task_run::TaskRunStatus,
        branch: Option<&str>,
    ) {
        task_run::update(ctx, &row.run, status, branch, None).unwrap();
    }

    fn task_run_update_record(ctx: &Ctx, run_id: &str) -> task_run::TaskRunRecord {
        task_run::list(ctx)
            .unwrap()
            .into_iter()
            .find(|record| record.id == run_id)
            .unwrap()
    }

    fn record_accepted_review_after_report(
        ctx: &Ctx,
        run_id: &str,
        reported_at: &str,
        reviewed_at: &str,
    ) {
        let run = task_run_update_record(ctx, run_id);
        task_run::update_report_metadata(&run, "msg-report").unwrap();
        let run = task_run_update_record(ctx, run_id);
        task_run::update_review_metadata(&run, task_run::REVIEW_ACCEPTED, "msg-review").unwrap();
        set_task_run_review_timestamps(ctx, run_id, reported_at, reviewed_at);
    }

    fn set_task_run_review_timestamps(
        ctx: &Ctx,
        run_id: &str,
        reported_at: &str,
        reviewed_at: &str,
    ) {
        let path = task_run::resolve(ctx, run_id).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let mut document = content.parse::<toml_edit::DocumentMut>().unwrap();
        document["last_reported_at"] = toml_edit::value(reported_at);
        document["last_reviewed_at"] = toml_edit::value(reviewed_at);
        std::fs::write(path, document.to_string()).unwrap();
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

    fn rewrite_as_legacy_workflow_route(ctx: &Ctx, run_id: &str) {
        let path = task_run::resolve(ctx, run_id).unwrap();
        let run = task_run::read(&path).unwrap();
        let mut content = format!(
            "task = \"{}\"\nbranch = \"{}\"\nstatus = \"{}\"\n",
            run.task, run.branch, run.status
        );
        if let Some(group) = run.group.as_deref() {
            content.push_str(&format!("group = \"{group}\"\n"));
        }
        if let Some(creation_order) = run.creation_order {
            content.push_str(&format!("creation_order = {creation_order}\n"));
        }
        if let Some(coordinator_id) = run.coordinator_id.as_deref() {
            content.push_str(&format!("coordinator_id = \"{coordinator_id}\"\n"));
        }
        content.push_str(&format!(
            "created_at = \"{}\"\nupdated_at = \"{}\"\n",
            run.created_at, run.updated_at
        ));
        fs::write(path, content).unwrap();
    }

    fn assert_workflow_runs_have_routes(ctx: &Ctx, expected: usize) {
        let runs = task_run::list(ctx).unwrap();
        assert_eq!(runs.len(), expected);
        assert!(runs.iter().all(|record| {
            record.run.coordinator_id.as_deref() == Some("agents/coord-workflow")
        }));
        assert!(runs.iter().all(|record| {
            record.run.coordinator_label.as_deref()
                == Some("Coordinator for workflow \"Workflow routing\"")
        }));
        assert!(runs.iter().all(
            |record| record.run.agent_id.as_deref().is_some_and(|agent| {
                agent.starts_with("agents/run-") && agent.contains(&record.run.task)
            })
        ));
    }

    fn assert_report_only_workflow_handoff(content: &str) {
        assert!(content.contains("## Workflow Coordinator Handoff"));
        assert!(content.contains("Workflow policy sets `pull_request = \"none\"`"));
        assert!(content.contains("PR=none"));
        assert!(content.contains("TaskRun report route"));
        assert!(content.contains("wt task report \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>\""));
        assert!(content.contains("stored coordinator route and workflow scope"));
        assert!(!content.contains("wt msg send --scope workflow:"));
        assert!(content.contains("If the file inbox route is unavailable"));
        assert!(content.contains("cmux send --workspace {{coordinator_cmux_workspace}} --surface {{coordinator_cmux_surface}} \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>\""));
        assert!(content.contains("{{coordinator_enter_command}}"));
        assert!(content.contains("If `wt task report` fails"));
        assert_inbox_route_precedes_cmux_fallback(content);
        assert!(content.contains("wt workflow pass"));
        assert!(!content.contains("--run-next"));
    }

    fn assert_workflow_handoff_precedes_task_body(content: &str, body: &str) {
        assert!(
            content.find("## Workflow Coordinator Handoff").unwrap() < content.find(body).unwrap()
        );
    }

    fn assert_inbox_route_precedes_cmux_fallback(content: &str) {
        assert!(
            content.find("wt task report").unwrap()
                < content.find("fallback cmux surface").unwrap()
        );
    }

    fn assert_workflow_inbox_command_precedes_policy(content: &str) {
        assert!(
            content.find("wt task report").unwrap()
                < content
                    .find("Workflow policy sets")
                    .unwrap_or(content.len())
        );
    }

    #[test]
    fn deprecated_workflow_complete_fails_with_pass_guidance() {
        let err = deprecated_complete("work flow", Some("task one"), true).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("`wt workflow complete` has been replaced"));
        assert!(message.contains("wt workflow pass 'work flow' 'task one' --run-next"));
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
        assert_eq!(workflow.policy.pull_request, WorkflowPullRequestMode::None);
        assert_eq!(workflow.policy.landing, WorkflowLandingPolicy::Manual);
        assert_eq!(task_run::list(&ctx).unwrap().len(), 2);
    }

    #[test]
    fn task_prepares_workflow_task_runs_with_launcher_routes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_launcher(dir.path(), "agents/coord-workflow");

        task(
            &ctx,
            &["workflow docs".into(), "workflow state".into()],
            WorkflowModeArg::Batch,
            None,
            Some("Workflow migration"),
            &Some("main".into()),
            None,
        )
        .unwrap();

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|record| {
            record.run.coordinator_id.as_deref() == Some("agents/coord-workflow")
        }));
        assert!(runs.iter().all(|record| {
            record.run.coordinator_label.as_deref()
                == Some("Coordinator for workflow \"Workflow migration\"")
        }));
        assert!(runs.iter().all(
            |record| record.run.agent_id.as_deref().is_some_and(|agent| {
                agent.starts_with("agents/run-") && agent.contains(&record.run.task)
            })
        ));
    }

    #[test]
    fn task_prepares_workflow_task_runs_with_auto_created_coordinator_when_unset() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Batch,
            None,
            None,
            &Some("main".into()),
            None,
        )
        .unwrap();

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(runs[0].run.coordinator_id.as_deref().is_some_and(|agent| {
            agent.starts_with("agents/surface-")
                || agent.starts_with("agents/claude-")
                || agent.starts_with("agents/codex-")
                || agent.starts_with("agents/shell-")
        }));
        assert_eq!(
            runs[0].run.coordinator_label.as_deref(),
            Some("Coordinator for workflow \"workflow docs\"")
        );
        assert_eq!(
            runs[0].run.agent_id.as_deref(),
            Some("agents/run-1-workflow-docs")
        );
    }

    #[test]
    fn task_prepares_routes_for_single_stack_and_matrix_modes() {
        for mode in [WorkflowModeArg::Single, WorkflowModeArg::Stack] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx_with_launcher(dir.path(), "agents/coord-workflow");
            let tasks = match mode {
                WorkflowModeArg::Single => vec!["workflow docs".into()],
                WorkflowModeArg::Stack => vec!["workflow docs".into(), "workflow state".into()],
                WorkflowModeArg::Batch | WorkflowModeArg::Matrix => unreachable!(),
            };

            task(
                &ctx,
                &tasks,
                mode,
                None,
                Some("Workflow routing"),
                &Some("main".into()),
                None,
            )
            .unwrap();

            assert_workflow_runs_have_routes(&ctx, tasks.len());
        }

        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_profile(dir.path(), "beta");
        let ctx = ctx_with_launcher(dir.path(), "agents/coord-workflow");
        super::task(
            &ctx,
            &["workflow docs".into()],
            TaskOptions {
                mode: WorkflowModeArg::Matrix,
                profile: None,
                profiles: &["alpha".into(), "beta".into()],
                title: Some("Workflow routing"),
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        assert_workflow_runs_have_routes(&ctx, 2);
    }

    #[test]
    fn workflow_run_repairs_legacy_task_run_routes_before_launch() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        );

        let mut runner = MockRunner::new();
        runner.add_response("", false); // profile branch local_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent local_branch_exists
        runner.add_response("", true); // set parent config
        let runner = Arc::new(runner);
        let ctx = Ctx::new_with_options(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
            crate::context::CtxOptions {
                launcher_coordinator_id: Some("agents/coord-workflow".into()),
                ..crate::context::CtxOptions::default()
            },
        );
        let base = Some("main".into());
        super::task(
            &ctx,
            &["add-schema".into()],
            TaskOptions {
                mode: WorkflowModeArg::Matrix,
                profile: None,
                profiles: &["alpha".into()],
                title: Some("Workflow routing"),
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base: &base,
                pr: None,
            },
        )
        .unwrap();
        let record = workflow_store::list(&ctx).unwrap().remove(0);
        let run_id = &record.workflow.tasks[0].runs[0].run;
        rewrite_as_legacy_workflow_route(&ctx, run_id);
        assert!(task_run_record(&ctx, run_id).unwrap().agent_id.is_none());

        run_after_coordinator_session_check(&ctx, Some(record.path.to_str().unwrap()), 1).unwrap();

        let repaired = task_run_record(&ctx, run_id).unwrap();
        assert_eq!(repaired.status, STATUS_RUNNING);
        assert_eq!(
            repaired.agent_id.as_deref(),
            Some("agents/run-1-add-schema")
        );
        assert_eq!(
            repaired.coordinator_id.as_deref(),
            Some("agents/coord-workflow")
        );
        assert_eq!(
            repaired.coordinator_label.as_deref(),
            Some("Coordinator for workflow \"Workflow routing\"")
        );
    }

    #[test]
    fn task_prepares_workflow_with_title_body_origin_and_show_displays_it() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));

        super::task(
            &ctx,
            &["workflow docs".into()],
            TaskOptions {
                mode: WorkflowModeArg::Batch,
                profile: None,
                profiles: &[],
                title: Some("Workflow migration"),
                body: Some("Ship the larger workflow migration"),
                body_file: None,
                origin_provider: Some("linear"),
                origin_id: Some("WT-123"),
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(record.workflow.title.as_deref(), Some("Workflow migration"));
        assert_eq!(
            record.workflow.body.as_deref(),
            Some("Ship the larger workflow migration")
        );
        assert_eq!(record.workflow.origin.as_ref().unwrap().provider, "linear");
        assert_eq!(record.workflow.origin.as_ref().unwrap().id, "WT-123");
        let content = std::fs::read_to_string(&record.path).unwrap();
        assert!(content.contains("title = \"Workflow migration\""));
        assert!(content.contains("body = \"\"\"Ship the larger workflow migration\"\"\""));
        assert!(content.contains("[origin]"));
        assert!(content.contains("provider = \"linear\""));
        assert!(content.contains("id = \"WT-123\""));
        assert!(!content.contains("objective ="));

        show(&ctx, Some(&record.id)).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("Title: Workflow migration"));
        assert!(dims.contains("Body: Ship the larger workflow migration"));
        assert!(dims.contains("Origin: linear:WT-123"));
    }

    #[test]
    fn task_generates_readable_default_workflow_title_from_tasks() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-schema",
            "title = \"스키마 추가\"\nbranch = \"add-schema\"\n",
        );
        write_task(
            dir.path(),
            "wire-api",
            "title = \"API 연결\"\nbranch = \"wire-api\"\n",
        );
        let ctx = ctx(dir.path());

        super::task(
            &ctx,
            &["add-schema".into(), "wire-api".into()],
            TaskOptions {
                mode: WorkflowModeArg::Batch,
                profile: None,
                profiles: &[],
                title: None,
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.title.as_deref(),
            Some("스키마 추가 외 1개 작업")
        );
        let content = std::fs::read_to_string(&record.path).unwrap();
        assert!(content.contains("title = \"스키마 추가 외 1개 작업\""));
    }

    #[test]
    fn task_generates_readable_default_matrix_title_from_profiles() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_profile(dir.path(), "beta");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"스키마 추가\"\nbranch = \"add-schema\"\n",
        );
        let ctx = ctx(dir.path());

        super::task(
            &ctx,
            &["add-schema".into()],
            TaskOptions {
                mode: WorkflowModeArg::Matrix,
                profile: None,
                profiles: &strings(&["alpha", "beta"]),
                title: None,
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.title.as_deref(),
            Some("스키마 추가 (2개 프로필)")
        );
        let content = std::fs::read_to_string(&record.path).unwrap();
        assert!(content.contains("title = \"스키마 추가 (2개 프로필)\""));
    }

    #[test]
    fn task_reads_workflow_body_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let body_path = dir.path().join("workflow-body.md");
        fs::write(&body_path, "Large workflow context\n").unwrap();

        super::task(
            &ctx,
            &["workflow docs".into()],
            TaskOptions {
                mode: WorkflowModeArg::Batch,
                profile: None,
                profiles: &[],
                title: Some("Workflow migration"),
                body: None,
                body_file: Some(&body_path),
                origin_provider: None,
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.body.as_deref(),
            Some("Large workflow context")
        );
    }

    #[test]
    fn task_rejects_ambiguous_workflow_metadata_flags() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let body_path = dir.path().join("workflow-body.md");
        fs::write(&body_path, "Large workflow context").unwrap();

        let err = super::task(
            &ctx,
            &["workflow docs".into()],
            TaskOptions {
                mode: WorkflowModeArg::Batch,
                profile: None,
                profiles: &[],
                title: None,
                body: Some("inline"),
                body_file: Some(&body_path),
                origin_provider: None,
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("--body cannot be used with --body-file")
        );

        let err = super::task(
            &ctx,
            &["workflow docs".into()],
            TaskOptions {
                mode: WorkflowModeArg::Batch,
                profile: None,
                profiles: &[],
                title: None,
                body: None,
                body_file: None,
                origin_provider: Some("linear"),
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("--origin-provider requires --origin-id")
        );
    }

    #[test]
    fn task_without_args_multi_selects_existing_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
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
                .all(|record| record.run.status == STATUS_PREPARED)
        );
        assert!(runs.iter().all(|record| record.run.group.is_some()));
    }

    #[test]
    fn task_without_args_can_select_completed_task_documents() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
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
        ui.add_multi_select(vec![0]);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );
        task_run::create(&ctx, "add-schema", "add-schema", None, STATUS_PASSED).unwrap();

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
        assert_eq!(workflow.tasks.len(), 1);
        assert_eq!(workflow.tasks[0].task, "add-schema");
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
        assert!(states.iter().all(|state| state.run.group.is_some()));
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
        let foreign_run = replace_first_workflow_run_with_foreign_group(&ctx, &mut record.workflow);

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
        let foreign_run = replace_first_workflow_run_with_foreign_group(&ctx, &mut record.workflow);

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

        let err = run_after_coordinator_session_check(&ctx, Some(record.path.to_str().unwrap()), 1)
            .unwrap_err();

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
    fn stack_workflow_preserves_task_start_error_message() {
        let dir = tempfile::tempdir().unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true); // checked_out_path
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local branch exists
        runner.add_response("", false); // remote branch exists
        runner.add_response_with_stderr("", "fatal: invalid reference: finished-stack", false);
        let ctx = ctx_with_runner_ui(dir.path(), runner, Arc::new(MockUi::new()));

        let record = prepare_workflow(
            &ctx,
            WorkflowModeArg::Stack,
            &["finished stack", "retry stack"],
        );
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_PASSED,
            Some("finished-stack"),
        );
        update_task_run(&ctx, &record.workflow.tasks[1], STATUS_FAILED, None);

        let err = run_after_coordinator_session_check(&ctx, Some(record.path.to_str().unwrap()), 1)
            .unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains("git worktree add -b"),
            "unexpected error message: {message}"
        );
        assert!(
            message.contains("fatal: invalid reference: finished-stack"),
            "unexpected error message: {message}"
        );
        assert!(
            !message.contains("Workflow stack failed"),
            "unexpected error message: {message}"
        );

        let retry_run = task_run_record(&ctx, &record.workflow.tasks[1].run).unwrap();
        assert_eq!(retry_run.status, STATUS_FAILED);
        assert_eq!(retry_run.error.as_deref(), Some(message.as_str()));
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
        assert!(runs[0].run.group.is_some());
        assert_eq!(runs[0].run.status, STATUS_PREPARED);
    }

    #[test]
    fn task_prepares_matrix_mode_workflow_with_profile_runs_in_order() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "beta");
        write_profile(dir.path(), "alpha");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        );
        let ctx = ctx(dir.path());

        let record =
            prepare_matrix_workflow(&ctx, &["add-schema".into()], &strings(&["beta", "alpha"]));

        let workflow = &record.workflow;
        assert_eq!(workflow.mode, WorkflowMode::Matrix);
        assert!(workflow.profile.is_none());
        assert_eq!(workflow.profiles, strings(&["beta", "alpha"]));
        assert_eq!(workflow.tasks.len(), 1);
        let row = &workflow.tasks[0];
        assert_eq!(row.task, "add-schema");
        assert!(row.run.is_empty());
        assert_eq!(
            row.runs
                .iter()
                .map(|run| run.profile.as_str())
                .collect::<Vec<_>>(),
            vec!["beta", "alpha"]
        );

        let task_runs = task_run::list(&ctx).unwrap();
        assert_eq!(task_runs.len(), 2);
        assert_eq!(
            row.runs
                .iter()
                .map(|profile_run| {
                    let run = task_runs
                        .iter()
                        .find(|record| record.id == profile_run.run)
                        .expect("expected linked TaskRun");
                    (
                        profile_run.profile.as_str(),
                        run.run.branch.as_str(),
                        run.run.status,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("beta", "add-schema-beta", STATUS_PREPARED),
                ("alpha", "add-schema-alpha", STATUS_PREPARED),
            ]
        );
        assert!(task_runs.iter().all(|record| record.run.group.is_some()));

        let content = fs::read_to_string(&record.path).unwrap();
        assert!(content.contains("mode = \"matrix\""));
        assert!(content.contains("profiles = [\"beta\", \"alpha\"]"));
        assert!(content.contains("[[tasks.runs]]"));
        assert!(!content.contains("\nrun = \"workflow-"));
    }

    #[test]
    fn task_matrix_profile_validation_fails_before_workflow_or_task_runs() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        );
        let ctx = ctx(dir.path());

        let duplicate =
            matrix_task(&ctx, &["add-schema".into()], &strings(&["alpha", "alpha"])).unwrap_err();
        assert!(duplicate.to_string().contains("Duplicate profile: alpha"));
        assert!(workflow_store::list(&ctx).unwrap().is_empty());
        assert!(task_run::list(&ctx).unwrap().is_empty());

        let missing =
            matrix_task(&ctx, &["add-schema".into()], &strings(&["missing"])).unwrap_err();
        assert!(missing.to_string().contains("Profile 'missing' not found"));
        assert!(workflow_store::list(&ctx).unwrap().is_empty());
        assert!(task_run::list(&ctx).unwrap().is_empty());

        let reserved =
            matrix_task(&ctx, &["add-schema".into()], &strings(&["default"])).unwrap_err();
        assert!(reserved.to_string().contains("reserved"));
        assert!(workflow_store::list(&ctx).unwrap().is_empty());
        assert!(task_run::list(&ctx).unwrap().is_empty());
    }

    #[test]
    fn task_matrix_requires_exactly_one_task_and_explicit_profiles() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        );
        write_task(
            dir.path(),
            "wire-api",
            "title = \"Wire API\"\nbranch = \"wire-api\"\n",
        );
        let ctx = ctx(dir.path());

        let no_profiles = matrix_task(&ctx, &["add-schema".into()], &[]).unwrap_err();
        assert!(no_profiles.to_string().contains("--profiles is required"));
        assert!(workflow_store::list(&ctx).unwrap().is_empty());
        assert!(task_run::list(&ctx).unwrap().is_empty());

        let too_many_tasks = matrix_task(
            &ctx,
            &["add-schema".into(), "wire-api".into()],
            &strings(&["alpha"]),
        )
        .unwrap_err();
        assert!(
            too_many_tasks
                .to_string()
                .contains("matrix mode workflow requires exactly one task")
        );
        assert!(workflow_store::list(&ctx).unwrap().is_empty());
        assert!(task_run::list(&ctx).unwrap().is_empty());
    }

    #[test]
    fn workflow_matrix_run_starts_profile_runs_in_profile_order() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_profile(dir.path(), "beta");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        );

        let mut runner = MockRunner::new();
        for _ in 0..2 {
            runner.add_response("", false); // profile branch local_branch_exists
            runner.add_response("", true); // worktree_add_new_branch
            runner.add_response("", true); // parent local_branch_exists
            runner.add_response("", true); // set parent config
        }
        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let record =
            prepare_matrix_workflow(&ctx, &["add-schema".into()], &strings(&["alpha", "beta"]));

        run_after_coordinator_session_check(&ctx, Some(record.path.to_str().unwrap()), 1).unwrap();

        let calls = runner.calls.lock().unwrap();
        let added_branches = calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 4
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .map(|(_, args, _)| args[3].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            added_branches,
            vec![
                "add-schema-alpha".to_string(),
                "add-schema-beta".to_string()
            ]
        );
        drop(calls);

        let updated = workflow_store::read(&record.path).unwrap();
        let row = &updated.tasks[0];
        for profile_run in &row.runs {
            let task_run = task_run_record(&ctx, &profile_run.run).unwrap();
            assert_eq!(task_run.status, STATUS_RUNNING);
            assert_eq!(
                task_run.branch,
                format!("add-schema-{}", profile_run.profile)
            );
        }
    }

    #[test]
    fn workflow_pass_marks_one_matrix_profile_run_passed() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_profile(dir.path(), "beta");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        );
        let ctx = ctx(dir.path());
        let record =
            prepare_matrix_workflow(&ctx, &["add-schema".into()], &strings(&["alpha", "beta"]));
        let row = &record.workflow.tasks[0];
        for profile_run in &row.runs {
            task_run::update(
                &ctx,
                &profile_run.run,
                STATUS_RUNNING,
                Some(&format!("add-schema-{}", profile_run.profile)),
                None,
            )
            .unwrap();
        }

        pass(
            &ctx,
            record.path.to_str().unwrap(),
            Some("add-schema:alpha"),
            false,
        )
        .unwrap();

        let alpha = task_run_record(&ctx, &row.runs[0].run).unwrap();
        let beta = task_run_record(&ctx, &row.runs[1].run).unwrap();
        assert_eq!(alpha.status, STATUS_PASSED);
        assert_eq!(beta.status, STATUS_RUNNING);
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
        assert_eq!(workflow.tasks[1].parent.as_deref(), Some("contract"));
        assert_eq!(workflow.policy.pull_request, WorkflowPullRequestMode::Draft);
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
        assert_eq!(
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::None
        );
        assert_eq!(
            record.workflow.policy.landing,
            WorkflowLandingPolicy::Manual
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("[policy]"));
        assert!(content.contains("pull_request = \"none\""));
        assert!(content.contains("landing = \"manual\""));
    }

    #[test]
    fn workflow_show_displays_prepared_policy() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join("api.toml"),
            "title = \"API\"\nbranch = \"api\"\n",
        )
        .unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), ui.clone());
        let mut workflow = WorkflowMetadata::new(
            WorkflowMode::Single,
            "explicit",
            Some("main".into()),
            vec![WorkflowTask::new("api", "run-api")],
        );
        workflow.policy.pull_request = WorkflowPullRequestMode::None;
        workflow.policy.landing = WorkflowLandingPolicy::Manual;
        workflow.policy.review.codex_base = WorkflowCodexBaseReview::Required;
        let record = workflow_store::create(&ctx, workflow).unwrap();

        show(&ctx, Some(&record.id)).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("Pull request: none"));
        assert!(dims.contains("Landing: manual"));
        assert!(dims.contains("Review codex_base: required"));
    }

    #[test]
    fn task_snapshots_workflow_config_policy() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            workflow: crate::config::WorkflowConfig {
                pull_request: Some(WorkflowDefaultPullRequestMode::Draft),
                landing: Some(WorkflowDefaultLandingPolicy::Auto),
            },
            review: ReviewConfig {
                codex_base: Some(ReviewCodexBasePolicy::Required),
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
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::Draft
        );
        assert_eq!(record.workflow.policy.landing, WorkflowLandingPolicy::Auto);
        assert_eq!(
            record.workflow.policy.review.codex_base,
            WorkflowCodexBaseReview::Required
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("pull_request = \"draft\""));
        assert!(content.contains("[policy]"));
        assert!(content.contains("landing = \"auto\""));
        assert!(content.contains("[policy.review]"));
        assert!(content.contains("codex_base = \"required\""));
    }

    #[test]
    fn task_snapshots_selected_profile_workflow_policy() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".wt/config/profiles/codex");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("profile.toml"),
            r#"
[workflow]
pull_request = "ready"
landing = "auto"
"#,
        )
        .unwrap();
        let config = Config {
            workflow: crate::config::WorkflowConfig {
                pull_request: Some(WorkflowDefaultPullRequestMode::Draft),
                landing: Some(WorkflowDefaultLandingPolicy::Manual),
            },
            ..Config::default()
        };
        let ctx = ctx_with_config(dir.path(), config);

        task(
            &ctx,
            &["contract".into()],
            WorkflowModeArg::Stack,
            Some("codex"),
            None,
            &Some("main".into()),
            None,
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(record.workflow.profile.as_deref(), Some("codex"));
        assert_eq!(
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::Ready
        );
        assert_eq!(record.workflow.policy.landing, WorkflowLandingPolicy::Auto);
        let content = fs::read_to_string(record.path).unwrap();
        assert!(content.contains("profile = \"codex\""));
        assert!(content.contains("pull_request = \"ready\""));
        assert!(content.contains("landing = \"auto\""));
    }

    #[test]
    fn explicit_pr_none_overrides_config_default() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            workflow: crate::config::WorkflowConfig {
                pull_request: Some(WorkflowDefaultPullRequestMode::Ready),
                landing: Some(WorkflowDefaultLandingPolicy::Manual),
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
        assert_eq!(
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::None
        );
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("pull_request = \"none\""));
    }

    #[test]
    fn explicit_pr_none_overrides_selected_profile_default() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".wt/config/profiles/codex");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("profile.toml"),
            r#"
[workflow]
pull_request = "ready"
landing = "auto"
"#,
        )
        .unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["contract".into()],
            WorkflowModeArg::Stack,
            Some("codex"),
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::None),
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::None
        );
        assert_eq!(record.workflow.policy.landing, WorkflowLandingPolicy::Auto);
        let content = fs::read_to_string(record.path).unwrap();
        assert!(content.contains("profile = \"codex\""));
        assert!(content.contains("pull_request = \"none\""));
        assert!(content.contains("landing = \"auto\""));
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
                pull_request: Some(WorkflowDefaultPullRequestMode::Ready),
                landing: Some(WorkflowDefaultLandingPolicy::Auto),
            },
            ..Config::default()
        };
        let ctx = ctx_with_config_and_runner(dir.path(), config, runner);

        issue(
            &ctx,
            &["PROJ-123".into()],
            IssueOptions {
                mode: WorkflowModeArg::Stack,
                profile: None,
                title: None,
                body: None,
                body_file: None,
                origin_provider: None,
                origin_id: None,
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::Ready
        );
        assert_eq!(record.workflow.policy.landing, WorkflowLandingPolicy::Auto);
        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("pull_request = \"ready\""));
        assert!(content.contains("[policy]"));
    }

    #[test]
    fn issue_keeps_selected_provider_origin_on_task_and_explicit_origin_on_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"proj-123-fix-editor","description":"Slice issue body"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"proj-123-fix-editor","description":"Slice issue body"}"#,
            true,
        );
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = ctx_with_config_and_runner(dir.path(), config, runner);

        issue(
            &ctx,
            &["PROJ-123".into()],
            IssueOptions {
                mode: WorkflowModeArg::Stack,
                profile: None,
                title: Some("Broad provider issue"),
                body: Some("Split the broad issue into executable slices."),
                body_file: None,
                origin_provider: Some("linear"),
                origin_id: Some("PROJ-ROOT"),
                base: &Some("main".into()),
                pr: None,
            },
        )
        .unwrap();

        let record = workflow_store::list(&ctx).unwrap().remove(0);
        assert_eq!(record.workflow.origin.as_ref().unwrap().provider, "linear");
        assert_eq!(record.workflow.origin.as_ref().unwrap().id, "PROJ-ROOT");
        assert_eq!(record.workflow.tasks[0].task, "PROJ-123");

        let child = task_store::read_task_document(&ctx, "PROJ-123").unwrap();
        assert_eq!(child.origin.as_ref().unwrap().provider, "linear");
        assert_eq!(child.origin.as_ref().unwrap().id, "PROJ-123");
        assert_ne!(
            record.workflow.origin.as_ref().unwrap().id,
            child.origin.as_ref().unwrap().id
        );
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
            record.workflow.policy.pull_request,
            WorkflowPullRequestMode::Ready
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
            STATUS_PASSED,
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
    fn workflow_pass_marks_non_stack_workflow_tasks_passed() {
        for mode in [WorkflowModeArg::Single, WorkflowModeArg::Batch] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx(dir.path());
            let record = prepare_workflow(&ctx, mode, &["feature"]);
            update_task_run(
                &ctx,
                &record.workflow.tasks[0],
                STATUS_RUNNING,
                Some("feature"),
            );

            pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap();

            assert_eq!(
                task_run_record(&ctx, &record.workflow.tasks[0].run)
                    .unwrap()
                    .status,
                STATUS_PASSED
            );
        }
    }

    #[test]
    fn workflow_pass_requires_accepted_review_for_required_codex_base_review() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let mut record = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["feature"]);
        record.workflow.policy.review.codex_base = WorkflowCodexBaseReview::Required;
        workflow_store::write(&ctx, &record.path, &mut record.workflow).unwrap();
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );

        let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("requires Codex base review evidence before pass"));
        assert!(message.contains("last review status is missing"));
        assert!(message.contains("/review --base main"));
        assert!(message.contains("codex review --base main"));
        assert!(message.contains(&format!(
            "wt task review {} --accept",
            record.workflow.tasks[0].run
        )));
        assert_eq!(
            task_run_record(&ctx, &record.workflow.tasks[0].run)
                .unwrap()
                .status,
            STATUS_RUNNING
        );

        let run = task_run_update_record(&ctx, &record.workflow.tasks[0].run);
        task_run::update_review_metadata(&run, task_run::REVIEW_ACCEPTED, "msg-review").unwrap();

        let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();
        assert!(
            err.to_string()
                .contains("latest Agent Completion Report timestamp is missing")
        );
        assert_eq!(
            task_run_record(&ctx, &record.workflow.tasks[0].run)
                .unwrap()
                .status,
            STATUS_RUNNING
        );

        record_accepted_review_after_report(
            &ctx,
            &record.workflow.tasks[0].run,
            "2026-05-18T00:01:00Z",
            "2026-05-18T00:02:00Z",
        );

        pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap();

        assert_eq!(
            task_run_record(&ctx, &record.workflow.tasks[0].run)
                .unwrap()
                .status,
            STATUS_PASSED
        );
    }

    #[test]
    fn workflow_pass_requires_accepted_review_for_required_matrix_profile() {
        let dir = tempfile::tempdir().unwrap();
        write_profile(dir.path(), "alpha");
        write_profile(dir.path(), "beta");
        write_task(
            dir.path(),
            "add-schema",
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        );
        let ctx = ctx(dir.path());
        let mut record =
            prepare_matrix_workflow(&ctx, &["add-schema".into()], &strings(&["alpha", "beta"]));
        record.workflow.policy.review.codex_base = WorkflowCodexBaseReview::Required;
        workflow_store::write(&ctx, &record.path, &mut record.workflow).unwrap();
        let row = &record.workflow.tasks[0];
        for profile_run in &row.runs {
            task_run::update(
                &ctx,
                &profile_run.run,
                STATUS_RUNNING,
                Some(&format!("add-schema-{}", profile_run.profile)),
                None,
            )
            .unwrap();
        }

        let err = pass(
            &ctx,
            record.path.to_str().unwrap(),
            Some("add-schema:alpha"),
            false,
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("requires Codex base review evidence before pass"));
        assert!(message.contains("/review --base main"));
        assert!(message.contains("codex review --base main"));
        assert!(message.contains(&format!("wt task review {} --accept", row.runs[0].run)));

        record_accepted_review_after_report(
            &ctx,
            &row.runs[0].run,
            "2026-05-18T00:01:00Z",
            "2026-05-18T00:02:00Z",
        );

        pass(
            &ctx,
            record.path.to_str().unwrap(),
            Some("add-schema:alpha"),
            false,
        )
        .unwrap();

        let alpha = task_run_record(&ctx, &row.runs[0].run).unwrap();
        let beta = task_run_record(&ctx, &row.runs[1].run).unwrap();
        assert_eq!(alpha.status, STATUS_PASSED);
        assert_eq!(beta.status, STATUS_RUNNING);
    }

    #[test]
    fn workflow_pass_rejects_accepted_review_older_than_latest_report() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let mut record = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["feature"]);
        record.workflow.policy.review.codex_base = WorkflowCodexBaseReview::Required;
        workflow_store::write(&ctx, &record.path, &mut record.workflow).unwrap();
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );
        record_accepted_review_after_report(
            &ctx,
            &record.workflow.tasks[0].run,
            "2026-05-18T00:03:00Z",
            "2026-05-18T00:02:00Z",
        );

        let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

        let message = err.to_string();
        assert!(message.contains(
            "accepted review at 2026-05-18T00:02:00Z is older than latest Agent Completion Report at 2026-05-18T00:03:00Z"
        ));
        assert_eq!(
            task_run_record(&ctx, &record.workflow.tasks[0].run)
                .unwrap()
                .status,
            STATUS_RUNNING
        );

        record_accepted_review_after_report(
            &ctx,
            &record.workflow.tasks[0].run,
            "2026-05-18T00:03:00Z",
            "2026-05-18T00:04:00Z",
        );

        pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap();

        assert_eq!(
            task_run_record(&ctx, &record.workflow.tasks[0].run)
                .unwrap()
                .status,
            STATUS_PASSED
        );
    }

    #[test]
    fn workflow_pass_quotes_required_codex_base_review_accept_message() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let mut record = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["feature"]);
        let review_base = "feature/$(touch pwn)`base`";
        record.workflow.base = Some(review_base.into());
        record.workflow.policy.review.codex_base = WorkflowCodexBaseReview::Required;
        workflow_store::write(&ctx, &record.path, &mut record.workflow).unwrap();
        update_task_run(
            &ctx,
            &record.workflow.tasks[0],
            STATUS_RUNNING,
            Some("feature"),
        );

        let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

        let expected_message =
            format!("Codex base review passed against {review_base}: <summary/evidence>");
        let expected_command = format!(
            "wt task review {} --accept {}",
            record.workflow.tasks[0].run,
            shell_arg(&expected_message)
        );
        let unsafe_command = format!("--accept \"{expected_message}\"");
        let message = err.to_string();
        assert!(message.contains(&expected_command));
        assert!(!message.contains(&unsafe_command));
    }

    #[test]
    fn workflow_pass_run_next_rejects_non_stack_workflows() {
        for mode in [WorkflowModeArg::Single, WorkflowModeArg::Batch] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx(dir.path());
            let record = prepare_workflow(&ctx, mode, &["feature"]);

            let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), true).unwrap_err();

            assert!(
                err.to_string()
                    .contains("--run-next only supports mode stack")
            );
        }
    }

    #[test]
    fn workflow_pass_rejects_dirty_stack_task_worktree() {
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

        let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

        assert!(err.to_string().contains("uncommitted changes"));
        let run = task_run_record(&ctx, &record.workflow.tasks[0].run).unwrap();
        assert_eq!(run.status, STATUS_RUNNING);
    }

    #[test]
    fn workflow_pass_rejects_stack_task_without_commits() {
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

        let err = pass(&ctx, record.path.to_str().unwrap(), Some("feature"), false).unwrap_err();

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
    fn workflow_pass_with_run_next_starts_next_stack_task() {
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
        runner.add_response("", true); // has_remote (origin present)
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

        pass(&ctx, record.path.to_str().unwrap(), Some("schema"), true).unwrap();
        let updated = workflow_store::read(&record.path).unwrap();

        assert_eq!(
            task_run_record(&ctx, &first_run).unwrap().status,
            STATUS_PASSED
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
            runs: Vec::new(),
        };
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/2026-05-16-001.toml");
        let policy = test_workflow_policy(WorkflowPullRequestMode::Draft);

        let content = workflow_task_prompt_content_with_policy(
            "title = \"API\"\n",
            &workflow_path,
            &row,
            &policy,
        );

        assert!(content.contains("## Workflow Coordinator Handoff"));
        assert_workflow_handoff_precedes_task_body(&content, "title = \"API\"");
        assert_workflow_inbox_command_precedes_policy(&content);
        assert_inbox_route_precedes_cmux_fallback(&content);
        assert!(content.contains("Workflow policy sets `pull_request = \"draft\"`"));
        assert!(content.contains("against the workflow parent branch"));
        assert!(content.contains("gh pr create --draft --body-file <pr-body-file> --base PROJ-1"));
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
        assert!(content.contains("wt task report \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr-url>; Risks or follow-ups=<risks>\""));
        assert!(!content.contains("wt msg send --scope workflow:"));
        assert!(content.contains(
            "wt workflow pass /repo/.wt/execution/workflows/2026-05-16-001.toml PROJ-2 --run-next"
        ));
    }

    #[test]
    fn workflow_stack_prompt_uses_ready_pr_handoff_policy() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            runs: Vec::new(),
        };
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/2026-05-16-001.toml");
        let policy = test_workflow_policy(WorkflowPullRequestMode::Ready);

        let content = workflow_task_prompt_content_with_policy(
            "title = \"API\"\n",
            &workflow_path,
            &row,
            &policy,
        );

        assert!(content.contains("Workflow policy sets `pull_request = \"ready\"`"));
        assert!(content.contains("gh pr create --body-file <pr-body-file>"));
        assert!(!content.contains("gh pr create --draft"));
        assert!(!content.contains("gh pr ready"));
        assert!(content.contains("PR=<pr-url>"));
    }

    #[test]
    fn workflow_stack_prompt_uses_validated_parent_for_pr_base() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("stored-parent".into()),
            runs: Vec::new(),
        };
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/2026-05-16-001.toml");
        let mut policy = test_workflow_policy(WorkflowPullRequestMode::Ready);
        policy.review.codex_base = WorkflowCodexBaseReview::Required;

        let content = workflow_task_prompt_content_with_policy_and_parent(
            "title = \"API\"\n",
            &workflow_path,
            &row,
            &policy,
            "validated-runtime-parent",
        );

        assert!(
            content.contains(
                "gh pr create --body-file <pr-body-file> --base validated-runtime-parent"
            )
        );
        assert!(content.contains("/review --base validated-runtime-parent"));
        assert!(content.contains("codex review --base validated-runtime-parent"));
        assert!(!content.contains("gh pr create --body-file <pr-body-file> --base stored-parent"));
        assert!(!content.contains("/review --base stored-parent"));
        assert!(!content.contains("codex review --base stored-parent"));
    }

    #[test]
    fn workflow_stack_pass_status_messages_quote_shell_args() {
        let row = WorkflowTask {
            task: "PROJ weird's task".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            runs: Vec::new(),
        };
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/work flow.toml");
        let expected_command = "wt workflow pass '/repo/.wt/execution/workflows/work flow.toml' 'PROJ weird'\\''s task'";

        let already_running = stack_task_already_running_message(&workflow_path, &row);
        let started = started_stack_task_message(&workflow_path, &row);

        assert!(already_running.contains(expected_command));
        assert!(started.contains(expected_command));
    }

    #[test]
    fn workflow_stack_prompt_reports_none_without_pull_request_intent() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("PROJ-1".into()),
            runs: Vec::new(),
        };
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/2026-05-16-001.toml");

        let content = workflow_stack_task_prompt_content("title = \"API\"\n", &workflow_path, &row);

        assert!(content.contains("Workflow policy sets `pull_request = \"none\"`"));
        assert!(content.contains("do not open a pull request for this workflow task"));
        assert!(content.contains("If coordinator feedback asks for changes"));
        assert!(!content.contains("If Codex/GitHub review"));
        assert!(content.contains("PR=none"));
        assert!(!content.contains("gh pr create"));
        assert!(content.contains(
            "wt workflow pass /repo/.wt/execution/workflows/2026-05-16-001.toml PROJ-2 --run-next"
        ));
    }

    #[test]
    fn workflow_single_prompt_includes_report_only_coordinator_handoff() {
        let content = workflow_single_task_prompt_content("title = \"API\"\n");

        assert_report_only_workflow_handoff(&content);
        assert_workflow_handoff_precedes_task_body(&content, "title = \"API\"");
        assert!(content.find("wt task report").unwrap() < content.find("title = \"API\"").unwrap());
    }

    #[test]
    fn workflow_single_prompt_uses_draft_pr_handoff_policy() {
        let policy = test_workflow_policy(WorkflowPullRequestMode::Draft);
        let content = workflow_single_task_prompt_content_for_policy("title = \"API\"\n", &policy);

        assert!(content.contains("Workflow policy sets `pull_request = \"draft\"`"));
        assert!(content.contains("against the workflow base branch"));
        assert!(content.contains("gh pr create --draft --body-file <pr-body-file> --base main"));
        assert!(content.contains("PR=<pr-url>"));
    }

    #[test]
    fn workflow_pr_handoff_includes_issue_closing_keywords() {
        let policy = test_workflow_policy(WorkflowPullRequestMode::Ready);
        let issue_closing_references = vec!["#52".into(), "PROJ-123".into()];
        let content = workflow_single_task_prompt_content_for_policy_and_closing_refs(
            "title = \"API\"\n",
            &policy,
            &issue_closing_references,
        );

        assert!(content.contains("issue-closing keywords"));
        assert!(content.contains("`Closes #52`"));
        assert!(content.contains("`Closes PROJ-123`"));
        assert!(content.contains("gh pr create --body-file <pr-body-file> --base main"));
    }

    #[test]
    fn workflow_pr_handoff_does_not_close_workflow_origin_without_task_origin() {
        let policy = test_workflow_policy(WorkflowPullRequestMode::Ready);
        let mut metadata = WorkflowMetadata::new(
            WorkflowMode::Stack,
            "explicit",
            Some("main".into()),
            vec![WorkflowTask::new("api", "run-api")],
        );
        metadata.origin = Some(WorkflowOrigin {
            provider: "linear".into(),
            id: "PROJ-ROOT".into(),
        });
        let workflow_context = workflow_metadata_prompt_context(&metadata).unwrap();

        assert!(workflow_context.contains("Workflow origin: linear:PROJ-ROOT"));
        assert!(workflow_context.contains("Do not add PR issue-closing keywords"));

        let content = workflow_single_task_prompt_content_for_policy(&workflow_context, &policy);

        assert!(!content.contains("`Closes PROJ-ROOT`"));
        assert!(!content.contains("so linked provider issues close"));
        assert!(content.contains("gh pr create --body-file <pr-body-file> --base main"));
    }

    #[test]
    fn workflow_batch_prompt_uses_ready_pr_handoff_policy() {
        let policy = test_workflow_policy(WorkflowPullRequestMode::Ready);
        let content = workflow_batch_task_prompt_content_for_policy("title = \"API\"\n", &policy);

        assert!(content.contains("Workflow policy sets `pull_request = \"ready\"`"));
        assert!(content.contains("against the workflow base branch"));
        assert!(content.contains("gh pr create --body-file <pr-body-file> --base main"));
        assert!(!content.contains("gh pr create --draft"));
        assert!(content.contains("PR=<pr-url>"));
    }

    #[test]
    fn workflow_matrix_prompt_uses_scoped_coordinator_handoff() {
        let row = WorkflowTask::new("matrix-task", "run-matrix");
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/2026-05-17-002.toml");
        let policy = test_workflow_policy(WorkflowPullRequestMode::Ready);

        let content = workflow_matrix_task_handoff_section(
            &workflow_path,
            &row,
            "alpha",
            &policy,
            "main",
            &[],
        );

        assert!(content.contains("cmux send --workspace {{coordinator_cmux_workspace}} --surface {{coordinator_cmux_surface}}"));
        assert!(content.contains("wt task report \"Agent Completion Report"));
        assert!(!content.contains("wt msg send --scope workflow:"));
        assert!(content.contains(
            "workflow pass /repo/.wt/execution/workflows/2026-05-17-002.toml matrix-task:alpha"
        ));
        assert!(!content.contains("--run-next"));
    }

    #[test]
    fn workflow_prompt_describes_auto_landing_policy() {
        let policy = test_auto_landing_policy();
        let content = workflow_batch_task_prompt_content_for_policy("title = \"API\"\n", &policy);

        assert!(content.contains("landing and cleanup after its dirty-worktree"));
        assert!(content.contains("safety checks pass"));
    }

    #[test]
    fn workflow_prompt_describes_required_codex_base_review_policy() {
        let mut policy = test_workflow_policy(WorkflowPullRequestMode::None);
        policy.review.codex_base = WorkflowCodexBaseReview::Required;

        let content = workflow_batch_task_prompt_content_for_policy("title = \"API\"\n", &policy);

        assert!(content.contains("Workflow review policy sets `review.codex_base = \"required\"`"));
        assert!(content.contains("coordinator must open a Codex surface"));
        assert!(content.contains("against the workflow base branch"));
        assert!(content.contains("/review --base main"));
        assert!(content.contains("codex review --base main"));
        assert!(!content.contains("codex review --base <parent>"));
        assert!(content.contains("wt task review <task-run-id> --accept"));
        assert!(content.contains("concise review evidence note"));
        assert!(content.contains("before passing or landing this workflow task"));
        assert!(!content.contains("record the log"));
        assert!(content.contains("Workflow policy sets `pull_request = \"none\"`"));
        assert!(!content.contains("gh pr create"));
    }

    #[test]
    fn workflow_prompt_quotes_required_codex_base_review_accept_message() {
        let row = WorkflowTask {
            task: "PROJ-2".into(),
            run: "run-2".into(),
            parent: Some("stored-parent".into()),
            runs: Vec::new(),
        };
        let workflow_path = PathBuf::from("/repo/.wt/execution/workflows/2026-05-16-001.toml");
        let mut policy = test_workflow_policy(WorkflowPullRequestMode::None);
        let review_base = "feature/$(touch pwn)`base`";
        policy.review.codex_base = WorkflowCodexBaseReview::Required;

        let content = workflow_task_prompt_content_with_policy_and_parent(
            "title = \"API\"\n",
            &workflow_path,
            &row,
            &policy,
            review_base,
        );

        let expected_message =
            format!("Codex base review passed against {review_base}: <summary/evidence>");
        let expected_command = format!(
            "wt task review <task-run-id> --accept {}",
            shell_arg(&expected_message)
        );
        let unsafe_command = format!("--accept \"{expected_message}\"");
        assert!(content.contains(&expected_command));
        assert!(!content.contains(&unsafe_command));
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
                    runs: Vec::new(),
                },
                profile: None,
                run_id: "run-api".into(),
                document: task_store::TaskDocument {
                    title: "API".into(),
                    branch: "shared".into(),
                    body: String::new(),
                    origin: None,
                },
                path: "<repo-root>/.wt/execution/tasks/api.toml".into(),
                content: "title = \"API\"\nbranch = \"shared\"\n".into(),
                run: task_run::TaskRun {
                    task: "api".into(),
                    branch: "shared".into(),
                    status: STATUS_PREPARED,
                    group: None,
                    error: None,
                    creation_order: None,
                    agent_id: None,
                    coordinator_id: None,
                    coordinator_label: None,
                    last_report_message_id: None,
                    last_reported_at: None,
                    last_review_status: None,
                    last_review_message_id: None,
                    last_reviewed_at: None,
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
                    runs: Vec::new(),
                },
                profile: None,
                run_id: "run-docs".into(),
                document: task_store::TaskDocument {
                    title: "Docs".into(),
                    branch: "shared".into(),
                    body: String::new(),
                    origin: None,
                },
                path: "<repo-root>/.wt/execution/tasks/docs.toml".into(),
                content: "title = \"Docs\"\nbranch = \"shared\"\n".into(),
                run: task_run::TaskRun {
                    task: "docs".into(),
                    branch: "shared".into(),
                    status: STATUS_PREPARED,
                    group: None,
                    error: None,
                    creation_order: None,
                    agent_id: None,
                    coordinator_id: None,
                    coordinator_label: None,
                    last_report_message_id: None,
                    last_reported_at: None,
                    last_review_status: None,
                    last_review_message_id: None,
                    last_reviewed_at: None,
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
    fn workflow_matrix_prompt_includes_report_only_coordinator_handoff() {
        let row = WorkflowTask::new("task", "run-task");
        let content = workflow_matrix_task_handoff_section(
            Path::new("/repo/.wt/execution/workflows/test.toml"),
            &row,
            "alpha",
            &test_workflow_policy(WorkflowPullRequestMode::None),
            "main",
            &[],
        );

        assert_report_only_workflow_handoff(&content);
        assert!(
            content.contains("wt workflow pass /repo/.wt/execution/workflows/test.toml task:alpha")
        );
    }

    #[test]
    fn pr_modes_apply_to_non_stack_modes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Batch,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::Draft),
        )
        .unwrap();

        let workflow = workflow_store::list(&ctx).unwrap().remove(0).workflow;
        assert_eq!(workflow.policy.pull_request, WorkflowPullRequestMode::Draft);
    }

    #[test]
    fn pr_none_overrides_non_stack_config_default() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_config(
            dir.path(),
            Config {
                workflow: crate::config::WorkflowConfig {
                    pull_request: Some(WorkflowDefaultPullRequestMode::Ready),
                    landing: Some(WorkflowDefaultLandingPolicy::Manual),
                },
                ..Config::default()
            },
        );

        task(
            &ctx,
            &["workflow docs".into()],
            WorkflowModeArg::Single,
            None,
            None,
            &Some("main".into()),
            Some(WorkflowPrModeArg::None),
        )
        .unwrap();

        let workflow = workflow_store::list(&ctx).unwrap().remove(0).workflow;
        assert_eq!(workflow.policy.pull_request, WorkflowPullRequestMode::None);
    }

    fn replace_first_workflow_run_with_foreign_group(
        ctx: &Ctx,
        workflow: &mut WorkflowMetadata,
    ) -> String {
        let row = workflow.tasks.first_mut().unwrap();
        let document = task_store::read_task_document(ctx, &row.task).unwrap();
        let run = task_run::create(
            ctx,
            &row.task,
            &document.branch,
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
        let passed = prepare_workflow(&ctx, WorkflowModeArg::Single, &["passed single"]);
        update_task_run(
            &ctx,
            &passed.workflow.tasks[0],
            STATUS_PASSED,
            Some("passed-single"),
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
        let passed_only = prepare_workflow(
            &ctx,
            WorkflowModeArg::Batch,
            &["passed batch", "skipped batch"],
        );
        update_task_run(
            &ctx,
            &passed_only.workflow.tasks[0],
            STATUS_PASSED,
            Some("passed-batch"),
        );
        update_task_run(
            &ctx,
            &passed_only.workflow.tasks[1],
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
            STATUS_PASSED,
            Some("finished-stack"),
        );
        update_task_run(&ctx, &retry_second.workflow.tasks[1], STATUS_FAILED, None);
        let passed_only = prepare_workflow(
            &ctx,
            WorkflowModeArg::Stack,
            &["passed stack", "skipped stack"],
        );
        update_task_run(
            &ctx,
            &passed_only.workflow.tasks[0],
            STATUS_PASSED,
            Some("passed-stack"),
        );
        update_task_run(
            &ctx,
            &passed_only.workflow.tasks[1],
            STATUS_SKIPPED,
            Some("skipped-stack"),
        );

        assert_eq!(candidate_ids(&ctx), vec![first_ready.id, retry_second.id]);
    }

    #[test]
    fn runnable_workflow_candidates_skip_unreadable_workflows() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let valid = prepare_workflow(&ctx, WorkflowModeArg::Single, &["valid workflow"]);
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::write(workflows_dir.join("bad.toml"), "mode = [").unwrap();

        assert_eq!(candidate_ids(&ctx), vec![valid.id]);
        assert!(
            ui.warnings
                .lock()
                .unwrap()
                .iter()
                .any(|warning| warning.contains("Skipping unreadable workflow"))
        );
    }

    #[test]
    fn runnable_workflow_candidates_skip_unreadable_workflow_state() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let valid = prepare_workflow(&ctx, WorkflowModeArg::Single, &["valid state"]);
        let stale = prepare_workflow(&ctx, WorkflowModeArg::Single, &["stale state"]);
        let stale_run_path = task_run::resolve(&ctx, &stale.workflow.tasks[0].run).unwrap();
        fs::remove_file(stale_run_path).unwrap();

        assert_eq!(candidate_ids(&ctx), vec![valid.id]);
        assert!(
            ui.warnings
                .lock()
                .unwrap()
                .iter()
                .any(|warning| warning.contains("Skipping workflow with unreadable state"))
        );
    }

    #[test]
    fn bare_workflow_run_with_one_candidate_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0);
        let ui = Arc::new(ui);
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let workflow = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["only runnable"]);

        let path = resolve_run_workflow_path(&ctx, None).unwrap().unwrap();

        assert_eq!(path, workflow.path);
        assert_eq!(
            ui.prompts.lock().unwrap().as_slice(),
            ["select: Workflow to run"]
        );
        let items = ui.select_items.lock().unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0][0].starts_with("only runnable"));
        assert!(items[0][0].contains("batch"));
        assert!(items[0][0].contains("runnable 1"));
        assert!(!items[0][0].contains(&workflow.id));
        assert!(!items[0][0].contains("<repo-root>/.wt/execution/workflows/"));
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
        assert!(items[0][0].starts_with("first workflow"));
        assert!(items[0][0].contains("single"));
        assert!(items[0][0].contains("runnable 1"));
        assert!(!items[0][0].contains(&first.id));
        assert!(items[0][1].starts_with("second workflow"));
        assert!(items[0][1].contains("batch"));
        assert!(items[0][1].contains("runnable 1"));
        assert!(!items[0][1].contains(&second.id));
        let rows = ui.select_rows.lock().unwrap();
        assert_eq!(
            section_titles(&rows[0]),
            vec!["single workflows", "batch workflows"]
        );
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
        assert!(message.contains(&format!("wt run workflow {}", first.id)));
        assert!(message.contains(&format!("wt run workflow {}", second.id)));
        assert!(message.contains("<repo-root>/.wt/execution/workflows/"));
        assert!(ui.prompts.lock().unwrap().is_empty());
        assert_eq!(fs::read_to_string(first_run_path).unwrap(), first_before);
        assert_eq!(fs::read_to_string(second_run_path).unwrap(), second_before);
    }

    #[test]
    fn bare_workflow_run_with_one_candidate_non_interactive_lists_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.set_prompt_available(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let workflow = prepare_workflow(&ctx, WorkflowModeArg::Batch, &["only noninteractive"]);
        let run_path = task_run::resolve(&ctx, &workflow.workflow.tasks[0].run).unwrap();
        let run_before = fs::read_to_string(&run_path).unwrap();

        let err = resolve_run_workflow_path(&ctx, None).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("Runnable workflow found"));
        assert!(message.contains(&format!("wt run workflow {}", workflow.id)));
        assert!(message.contains("<repo-root>/.wt/execution/workflows/"));
        assert!(ui.prompts.lock().unwrap().is_empty());
        assert_eq!(fs::read_to_string(run_path).unwrap(), run_before);
    }

    #[test]
    fn bare_workflow_run_without_runnable_workflows_warns_without_state_changes() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_ui(dir.path(), Arc::clone(&ui));
        let workflow = prepare_workflow(&ctx, WorkflowModeArg::Single, &["already passed"]);
        update_task_run(
            &ctx,
            &workflow.workflow.tasks[0],
            STATUS_PASSED,
            Some("already-passed"),
        );
        let run_path = task_run::resolve(&ctx, &workflow.workflow.tasks[0].run).unwrap();
        let workflow_before = fs::read_to_string(&workflow.path).unwrap();
        let run_before = fs::read_to_string(&run_path).unwrap();

        run_after_coordinator_session_check(&ctx, None, 1).unwrap();

        assert_eq!(
            ui.warnings.lock().unwrap().as_slice(),
            ["No runnable workflows found"]
        );
        assert_eq!(fs::read_to_string(workflow.path).unwrap(), workflow_before);
        assert_eq!(fs::read_to_string(run_path).unwrap(), run_before);
    }

    fn section_titles(rows: &[PromptRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                PromptRow::Section(section) => Some(section.title.clone()),
                PromptRow::Option(_) => None,
            })
            .collect()
    }
}
