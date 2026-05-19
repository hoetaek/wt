use crate::commands::issue;
use crate::context::Ctx;
use crate::error::WtError;
use crate::setup;
use crate::task;
use crate::task_run;
use anyhow::{Result, bail};
use std::collections::HashSet;

const TASK_RUN_COORDINATOR_HANDOFF_SECTION: &str = r#"## Task Run Coordinator Handoff

Send the Agent Completion Report back to the coordinator cmux surface that started this task run:

```bash
cmux send --workspace {{coordinator_cmux_workspace}} --surface {{coordinator_cmux_surface}} "Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>"
{{coordinator_enter_command}}
```

The coordinator also owns the file inbox target `coordinator`, which `wt msg send` normalizes to `agents/coordinator`. If the cmux target is unavailable or stale, send the same report through the coordinator inbox:

```bash
wt msg send --to coordinator "Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>"
```

This immediate TaskDocument run has no Workflow orchestration or pull-request handoff intent. When this task is complete and committed, do not open a pull request from the task agent; report `PR=none`.

After sending the report, wait for the coordinator to review, land, and clean up the task run explicitly.

If neither coordinator route is available, leave the same report in this task session and wait."#;

pub fn run(
    ctx: &Ctx,
    task_args: &[String],
    base_raw: &Option<String>,
    profile: Option<&str>,
) -> Result<()> {
    let selected = if task_args.is_empty() {
        task::select_local_tasks(ctx)?
    } else {
        select_named_tasks(ctx, task_args)?
    };
    if selected.is_empty() {
        bail!("No local tasks selected");
    }

    for task in &selected {
        run_selected_task(ctx, task, base_raw, profile)?;
    }

    Ok(())
}

fn select_named_tasks(ctx: &Ctx, task_keys: &[String]) -> Result<Vec<task::SelectedTask>> {
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for task_key in task_keys {
        let task_key = task_key.trim();
        if task_key.is_empty() {
            bail!("Task key cannot be empty");
        }
        let safe_key = task::safe_task_key(task_key);
        if !seen.insert(safe_key.clone()) {
            bail!("Duplicate task: {safe_key}");
        }
        selected.push(task::select_local_task_by_key(ctx, &safe_key)?);
    }
    Ok(selected)
}

fn run_selected_task(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    base_raw: &Option<String>,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let branch_name = task::prepared_branch_name(&selected.document.branch);
    if branch_name.is_none() && selected.document.origin.is_none() {
        bail!("Task {} has no branch", selected.key);
    }

    let identifier = selected.document.identifier_or_key(&selected.key);
    let title = selected.document.title_or_key(&selected.key);

    if profile.is_some() {
        let result = issue::run_with_issue_snapshot(
            ctx,
            base_raw,
            profile,
            false,
            issue::PreparedIssueContext {
                identifier: &identifier,
                title: &title,
                branch_name,
                setup_mode: selected.document.setup_mode(),
                additional_prompt_scope: None,
                workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
                on_start_issue_id: selected
                    .document
                    .origin
                    .as_ref()
                    .map(|origin| origin.id.as_str()),
                prompt_intro: "Use this task before changing code.",
                completion_section: Some(TASK_RUN_COORDINATOR_HANDOFF_SECTION),
                pre_snapshot_context: None,
                workspace_label: None,
                snapshot: issue::IssueSnapshotContext {
                    path_label: "Task path",
                    path: &selected.path,
                    content: &selected.content,
                },
            },
        );
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                record_task_failure(ctx, selected, &err);
                return Err(err);
            }
        };
        record_task_profile_success(ctx, selected, &result)?;
        return Ok(result);
    }

    let run = task_run::create(
        ctx,
        &selected.key,
        &selected.document.branch,
        None,
        task_run::STATUS_PREPARED,
    )?;

    let result = issue::run_with_issue_snapshot(
        ctx,
        base_raw,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &identifier,
            title: &title,
            branch_name,
            setup_mode: selected.document.setup_mode(),
            additional_prompt_scope: None,
            workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
            on_start_issue_id: selected
                .document
                .origin
                .as_ref()
                .map(|origin| origin.id.as_str()),
            prompt_intro: "Use this task before changing code.",
            completion_section: Some(TASK_RUN_COORDINATOR_HANDOFF_SECTION),
            pre_snapshot_context: None,
            workspace_label: None,
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task path",
                path: &selected.path,
                content: &selected.content,
            },
        },
    );

    let result = match result {
        Ok(result) => result,
        Err(err) => {
            let status = if is_cancelled(&err) {
                task_run::STATUS_SKIPPED
            } else {
                task_run::STATUS_FAILED
            };
            let message = err.to_string();
            let _ = task_run::update(ctx, &run.id, status, None, Some(&message));
            return Err(err);
        }
    };

    if selected.document.branch != result.canonical_branch_name {
        if let Err(err) = task::write_task_branch(ctx, &selected.key, &result.canonical_branch_name)
        {
            let message = err.to_string();
            let _ = task_run::update(
                ctx,
                &run.id,
                task_run::STATUS_FAILED,
                Some(&result.branch_name),
                Some(&message),
            );
            return Err(err);
        }
    }

    task_run::update(
        ctx,
        &run.id,
        task_run::STATUS_RUNNING,
        Some(&result.branch_name),
        None,
    )?;

    Ok(result)
}

fn record_task_failure(ctx: &Ctx, selected: &task::SelectedTask, err: &anyhow::Error) {
    let status = if is_cancelled(err) {
        task_run::STATUS_SKIPPED
    } else {
        task_run::STATUS_FAILED
    };
    let message = err.to_string();
    if let Ok(run) = task_run::create(ctx, &selected.key, &selected.document.branch, None, status) {
        let _ = task_run::update(ctx, &run.id, status, None, Some(&message));
    }
}

fn record_task_profile_success(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    result: &issue::IssueRunResult,
) -> Result<()> {
    task_run::create(
        ctx,
        &selected.key,
        &result.branch_name,
        None,
        task_run::STATUS_RUNNING,
    )?;
    write_task_branch_from_result(ctx, selected, result)?;
    Ok(())
}

fn write_task_branch_from_result(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    result: &issue::IssueRunResult,
) -> Result<()> {
    if selected.document.branch != result.canonical_branch_name {
        task::write_task_branch(ctx, &selected.key, &result.canonical_branch_name)?;
    }
    Ok(())
}

fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<WtError>()
        .is_some_and(|err| matches!(err, WtError::Cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentCli, AgentConfig, Config, IssueProviderType, IssuesConfig, ReadyMode, SubmitMode,
        WorkspaceConfig,
    };
    use crate::context::mock::{CommandCall, MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
    use std::collections::HashMap;
    use std::path::Path;
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

    fn write_empty_profile(root: &Path, name: &str) {
        let profile_dir = root.join(".git/wt/profiles").join(name);
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
    }

    fn run(
        ctx: &Ctx,
        task_args: &[String],
        base_raw: &Option<String>,
        profile: Option<&str>,
        selected_profiles: &[String],
        matrix: bool,
    ) -> Result<()> {
        assert!(selected_profiles.is_empty());
        assert!(!matrix);
        super::run(ctx, task_args, base_raw, profile)
    }

    fn count_linear_start_updates(calls: &[CommandCall], issue_id: &str) -> usize {
        let expected = vec![
            "issue".to_string(),
            "update".to_string(),
            issue_id.to_string(),
            "--state".to_string(),
            "In Progress".to_string(),
        ];
        calls
            .iter()
            .filter(|(cmd, args, _)| cmd == "linear" && args == &expected)
            .count()
    }

    fn add_task_worktree_creation_responses(runner: &mut MockRunner, repo: &Path) {
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.display()
            ),
            true,
        );
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local branch exists
        runner.add_response("", false); // remote branch exists
        runner.add_response("main", true); // current branch
        runner.add_response("", true); // worktree add
        runner.add_response("", true); // parent local branch exists
        runner.add_response("", true); // set parent config
    }

    fn add_cmux_workspace_responses(runner: &mut MockRunner) {
        runner.add_command("cmux");
        runner.add_response("{}", true); // cmux identify
        runner.add_response("workspace:200", true); // cmux new-workspace
        runner.add_response("", true); // cmux workspace-action set-color
        runner.add_response("", true); // cmux list-panes
    }

    fn task_color_config() -> Config {
        Config {
            workspace: Some(WorkspaceConfig {
                colors: HashMap::from([
                    (
                        crate::setup::WORKSPACE_COLOR_KIND_TASK.into(),
                        "cyan".into(),
                    ),
                    (
                        crate::setup::WORKSPACE_COLOR_KIND_ISSUE.into(),
                        "red".into(),
                    ),
                    (
                        crate::setup::WORKSPACE_COLOR_KIND_BRANCH.into(),
                        "green".into(),
                    ),
                ]),
                ..WorkspaceConfig::default()
            }),
            ..Config::default()
        }
    }

    fn cmux_set_color_values(calls: &[CommandCall]) -> Vec<String> {
        calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "cmux"
                    && args.first().is_some_and(|arg| arg == "workspace-action")
                    && args.iter().any(|arg| arg == "set-color")
            })
            .filter_map(|(_, args, _)| {
                args.iter()
                    .position(|arg| arg == "--color")
                    .and_then(|idx| args.get(idx + 1))
                    .cloned()
            })
            .collect()
    }

    #[test]
    fn duplicate_task_values_are_rejected() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = run(
            &ctx,
            &["add-schema".into(), "add-schema".into()],
            &None,
            None,
            &[],
            false,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate task: add-schema")
        );
    }

    #[test]
    fn task_run_with_key_runs_named_task_snapshot() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config {
                issues: Some(IssuesConfig {
                    provider: IssueProviderType::Linear,
                    gh_user: None,
                }),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, &["add-schema".into()], &None, None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let worktree_add_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .expect("expected git worktree add -b call");
        assert_eq!(worktree_add_call.1[3], "add-schema");
        assert_eq!(worktree_add_call.1[5], "main");
        assert!(calls.iter().all(|(cmd, _, _)| cmd != "linear"));
        drop(calls);

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run.task, "add-schema");
        assert_eq!(runs[0].run.status, task_run::STATUS_RUNNING);
        assert_eq!(runs[0].run.branch, "add-schema");
    }

    #[test]
    fn task_run_uses_task_workspace_color_for_local_task() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        add_task_worktree_creation_responses(&mut runner, repo.path());
        add_cmux_workspace_responses(&mut runner);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            task_color_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, &["add-schema".into()], &None, None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(cmux_set_color_values(&calls), vec!["cyan"]);
    }

    #[test]
    fn task_run_uses_task_workspace_color_for_provider_origin_task() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("PROJ-123.toml"),
            r#"title = "Fix editor"
body = "Use the issue branch."

[origin]
provider = "linear"
id = "PROJ-123"
"#,
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        add_task_worktree_creation_responses(&mut runner, repo.path());
        runner.add_response("", true); // issue on_start
        add_cmux_workspace_responses(&mut runner);
        let runner = Arc::new(runner);

        let mut config = task_color_config();
        config.issues = Some(IssuesConfig {
            provider: IssueProviderType::Linear,
            gh_user: None,
        });
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, &["PROJ-123".into()], &None, None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(cmux_set_color_values(&calls), vec!["cyan"]);
    }

    #[test]
    fn task_run_prompt_includes_rendered_coordinator_handoff() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            r#"{"caller":{"window_ref":"window:7","workspace_ref":"workspace:34","pane_ref":"pane:8","surface_ref":"surface:103"}}"#,
            true,
        );
        runner.add_response("workspace:200 workspace:200", true);
        runner.add_response("", true);
        runner.add_response("pane:1", true);
        runner.add_response("pane:1", true);
        runner.add_response("surface:999", true);
        runner.add_response("", true);
        runner.add_response("handoff prompt", true);
        runner.add_response("task prompt ready", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config {
                workspace: Some(WorkspaceConfig::default()),
                agent: Some(AgentConfig {
                    cli: AgentCli::None,
                    args: Vec::new(),
                    command: None,
                    ready: ReadyMode::Auto,
                    submit: SubmitMode::None,
                    timeout: 1,
                    send_after: 0,
                    prompt: HashMap::from([("branch".into(), vec!["Existing prompt".into()])]),
                    ..AgentConfig::default()
                }),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, &["add-schema".into()], &None, None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let send_calls = calls
            .iter()
            .filter(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|arg| arg == "send"))
            .collect::<Vec<_>>();
        assert_eq!(send_calls.len(), 2);

        let handoff_prompt = send_calls[0].1.last().unwrap();
        assert!(handoff_prompt.contains("## Task Run Coordinator Handoff"));
        assert!(
            handoff_prompt.find("cmux send --workspace").unwrap()
                < handoff_prompt
                    .find("This immediate TaskDocument run")
                    .unwrap()
        );
        assert!(handoff_prompt.contains("cmux send --workspace workspace:34 --surface surface:103 \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>\""));
        assert!(
            handoff_prompt
                .contains("cmux send-key --workspace workspace:34 --surface surface:103 enter")
        );
        assert!(handoff_prompt.contains("wt msg send --to coordinator \"Agent Completion Report"));
        assert!(handoff_prompt.contains("normalizes to `agents/coordinator`"));
        assert!(handoff_prompt.contains("If neither coordinator route is available"));
        assert!(!handoff_prompt.contains("Task path: `<git-common-dir>/wt/tasks/add-schema.toml`"));
        assert!(!handoff_prompt.contains("Create the schema first."));
        assert!(!handoff_prompt.contains("wt workflow complete"));

        let task_prompt = send_calls[1].1.last().unwrap();
        assert!(task_prompt.contains("Task path: `<git-common-dir>/wt/tasks/add-schema.toml`"));
        assert!(task_prompt.contains("Create the schema first."));
        assert!(task_prompt.contains("Existing prompt"));
    }

    #[test]
    fn task_run_records_failed_when_agent_prompt_delivery_fails() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            r#"{"caller":{"window_ref":"window:7","workspace_ref":"workspace:34","pane_ref":"pane:8","surface_ref":"surface:103"}}"#,
            true,
        );
        runner.add_response("workspace:200 workspace:200", true);
        runner.add_response("", true);
        runner.add_response("pane:1", true);
        runner.add_response("pane:1", true);
        runner.add_response("surface:999", true);
        runner.add_response("", true);
        runner.add_response("same screen", true);
        runner.add_response("same screen", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config {
                workspace: Some(WorkspaceConfig::default()),
                agent: Some(AgentConfig {
                    cli: AgentCli::None,
                    args: Vec::new(),
                    command: None,
                    ready: ReadyMode::Auto,
                    submit: SubmitMode::None,
                    timeout: 1,
                    send_after: 0,
                    prompt: HashMap::from([("branch".into(), vec!["Existing prompt".into()])]),
                    ..AgentConfig::default()
                }),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let err = run(&ctx, &["add-schema".into()], &None, None, &[], false)
            .unwrap_err()
            .to_string();

        assert!(err.contains("Agent prompt 2/2 failed"));
        assert!(err.contains("unchanged screen"));
        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run.status, task_run::STATUS_FAILED);
        assert!(
            runs[0]
                .run
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("Agent prompt 2/2 failed")
        );
    }

    #[test]
    fn task_run_with_key_records_new_run_after_prior_done() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        task_run::create(
            &ctx,
            "add-schema",
            "add-schema",
            None,
            task_run::STATUS_DONE,
        )
        .unwrap();

        run(&ctx, &["add-schema".into()], &None, None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let worktree_add_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .expect("expected git worktree add -b call");
        assert_eq!(worktree_add_call.1[3], "add-schema");
        assert_eq!(worktree_add_call.1[5], "main");

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        let latest = task_run::latest_for_task(&ctx, "add-schema")
            .unwrap()
            .expect("expected latest task run");
        assert_eq!(latest.run.task, "add-schema");
        assert_eq!(latest.run.status, task_run::STATUS_RUNNING);
        assert_eq!(latest.run.branch, "add-schema");
    }

    #[test]
    fn bare_task_run_selects_local_tasks() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("a-first.toml"),
            "title = \"First\"\nbranch = \"first\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("b-second.toml"),
            "title = \"Second\"\nbranch = \"second\"\nbody = \"details\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx, &[], &None, None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let worktree_add_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .expect("expected git worktree add -b call");
        assert_eq!(worktree_add_call.1[3], "second");
        assert_eq!(worktree_add_call.1[5], "main");

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run.task, "b-second");
        assert_eq!(runs[0].run.status, task_run::STATUS_RUNNING);
        assert_eq!(runs[0].run.branch, "second");
        assert_eq!(runs[0].run.group, None);
    }

    #[test]
    fn task_run_multiple_keys_start_separate_worktrees() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("publish-issues.toml"),
            "title = \"Publish issues\"\nbranch = \"publish-issues\"\nbody = \"Publish the issue tasks.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        runner.add_response("", true); // first fetch
        runner.add_response("", false); // first local branch exists
        runner.add_response("", false); // first remote branch exists
        runner.add_response("", true); // current branch for first base prompt
        runner.add_response("", true); // first worktree add
        runner.add_response("", true); // first parent local branch exists
        runner.add_response("", true); // first set parent config
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        runner.add_response("", true); // second fetch
        runner.add_response("", false); // second local branch exists
        runner.add_response("", false); // second remote branch exists
        runner.add_response("", true); // current branch for second base prompt
        runner.add_response("", true); // second worktree add
        runner.add_response("", true); // second parent local branch exists
        runner.add_response("", true); // second set parent config
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_input("main");
        ui.add_input("main");
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(
            &ctx,
            &["add-schema".into(), "publish-issues".into()],
            &None,
            None,
            &[],
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        let added_branches = calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .map(|(_, args, _)| args[3].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            added_branches,
            vec!["add-schema".to_string(), "publish-issues".to_string()]
        );
        drop(calls);

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|record| record.run.group.is_none()));
        let branches = runs
            .iter()
            .map(|record| record.run.branch.as_str())
            .collect::<Vec<_>>();
        assert!(branches.contains(&"add-schema"));
        assert!(branches.contains(&"publish-issues"));
    }

    #[test]
    fn task_run_updates_task_and_run_branch_from_issue_origin() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("PROJ-123.toml"),
            r#"title = "Fix editor"
body = "Use the issue branch."

[origin]
provider = "linear"
id = "PROJ-123"
"#,
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local branch exists
        runner.add_response("", false); // remote branch exists
        runner.add_response("main", true); // current branch
        runner.add_response("", true); // worktree add
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // issue on_start
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config {
                issues: Some(IssuesConfig {
                    provider: IssueProviderType::Linear,
                    gh_user: None,
                }),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, &["PROJ-123".into()], &None, None, &[], false).unwrap();

        let task_content =
            std::fs::read_to_string(repo.path().join(".git/wt/tasks/PROJ-123.toml")).unwrap();
        assert!(task_content.contains("branch = \"alice/proj-123-fix-editor\""));

        let latest = task_run::latest_for_task(&ctx, "PROJ-123")
            .unwrap()
            .expect("expected latest task run");
        assert_eq!(latest.run.status, task_run::STATUS_RUNNING);
        assert_eq!(latest.run.branch, "alice/proj-123-fix-editor");

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "linear"
                && args
                    == &vec![
                        "issue".to_string(),
                        "update".to_string(),
                        "PROJ-123".to_string(),
                        "--state".to_string(),
                        "In Progress".to_string(),
                    ]
        }));
    }

    #[test]
    fn task_run_with_provider_origin_and_profile_updates_start_status() {
        let repo = tempfile::tempdir().unwrap();
        write_empty_profile(repo.path(), "codex");
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("PROJ-123.toml"),
            r#"title = "Fix editor"
body = "Use the issue branch."

[origin]
provider = "linear"
id = "PROJ-123"
"#,
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response("", false); // profile branch local_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // on_start
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config {
                issues: Some(IssuesConfig {
                    provider: IssueProviderType::Linear,
                    gh_user: None,
                }),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            &["PROJ-123".into()],
            &Some("main".into()),
            Some("codex"),
            &[],
            false,
        )
        .unwrap();

        let latest = task_run::latest_for_task(&ctx, "PROJ-123")
            .unwrap()
            .expect("expected latest task run");
        assert_eq!(latest.run.status, task_run::STATUS_RUNNING);
        assert_eq!(latest.run.branch, "alice/proj-123-fix-editor-codex");

        let task_doc = task::read_task_document(&ctx, "PROJ-123").unwrap();
        assert_eq!(task_doc.branch, "alice/proj-123-fix-editor");

        let calls = runner.calls.lock().unwrap();
        assert_eq!(count_linear_start_updates(&calls, "PROJ-123"), 1);
    }

    #[test]
    fn task_run_with_local_task_and_profile_does_not_touch_issue_provider() {
        let repo = tempfile::tempdir().unwrap();
        write_empty_profile(repo.path(), "codex");
        let tasks_dir = repo.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", false); // profile branch local_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config {
                issues: Some(IssuesConfig {
                    provider: IssueProviderType::Linear,
                    gh_user: None,
                }),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            &["add-schema".into()],
            &Some("main".into()),
            Some("codex"),
            &[],
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, _, _)| cmd != "linear"));
    }
}
