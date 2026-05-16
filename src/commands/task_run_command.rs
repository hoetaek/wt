use crate::commands::{issue, task, task_run};
use crate::context::Ctx;
use crate::error::WtError;
use anyhow::{Result, bail};
use std::collections::HashSet;

pub fn run(
    ctx: &Ctx,
    task_args: &[String],
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
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
        run_selected_task(ctx, task, base_raw, profile, matrix)?;
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
    matrix: bool,
) -> Result<issue::IssueRunResult> {
    let branch_name = task::prepared_branch_name(&selected.document.branch);
    if branch_name.is_none() && selected.document.origin.is_none() {
        bail!("Task {} has no branch", selected.key);
    }

    let identifier = selected.document.identifier_or_key(&selected.key);
    let title = selected.document.title_or_key(&selected.key);

    if matrix || profile.is_some() {
        let results = issue::run_with_issue_snapshot_many(
            ctx,
            base_raw,
            profile,
            matrix,
            issue::PreparedIssueContext {
                identifier: &identifier,
                title: &title,
                branch_name,
                mode: selected.document.mode(),
                on_start_issue_id: selected
                    .document
                    .origin
                    .as_ref()
                    .map(|origin| origin.id.as_str()),
                prompt_intro: "Use this task before changing code.",
                workspace_label: None,
                snapshot: issue::IssueSnapshotContext {
                    path_label: "Task path",
                    path: &selected.path,
                    content: &selected.content,
                },
            },
        );
        let results = match results {
            Ok(results) => results,
            Err(err) => {
                if let Some(partial) = err.downcast_ref::<issue::IssueRunPartialFailure>() {
                    if let Err(record_err) = record_task_profile_results(ctx, selected, partial) {
                        return Err(anyhow::anyhow!(
                            "Failed to record partial profile TaskRuns after profile run failed ({err}): {record_err}"
                        ));
                    }
                } else {
                    record_task_failure(ctx, selected, &err);
                }
                return Err(err);
            }
        };
        if results.is_empty() {
            bail!("No profile worktrees created");
        }
        record_task_profile_successes(ctx, selected, &results)?;
        return results
            .into_iter()
            .last()
            .ok_or_else(|| anyhow::anyhow!("No profile worktrees created"));
    }

    let run = task_run::create(
        ctx,
        &selected.key,
        &selected.document.branch,
        task_run::SOURCE_NEW,
        None,
        task_run::STATUS_PREPARED,
    )?;

    let result = issue::run_with_issue_snapshot(
        ctx,
        base_raw,
        profile,
        matrix,
        issue::PreparedIssueContext {
            identifier: &identifier,
            title: &title,
            branch_name,
            mode: selected.document.mode(),
            on_start_issue_id: selected
                .document
                .origin
                .as_ref()
                .map(|origin| origin.id.as_str()),
            prompt_intro: "Use this task before changing code.",
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
    if let Ok(run) = task_run::create(
        ctx,
        &selected.key,
        &selected.document.branch,
        task_run::SOURCE_NEW,
        None,
        status,
    ) {
        let _ = task_run::update(ctx, &run.id, status, None, Some(&message));
    }
}

fn record_task_profile_successes(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    results: &[issue::IssueRunResult],
) -> Result<()> {
    for result in results {
        task_run::create(
            ctx,
            &selected.key,
            &result.branch_name,
            task_run::SOURCE_NEW,
            None,
            task_run::STATUS_RUNNING,
        )?;
    }
    write_task_branch_from_results(ctx, selected, results)?;
    Ok(())
}

fn write_task_branch_from_results(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    results: &[issue::IssueRunResult],
) -> Result<()> {
    let Some(result) = results.first() else {
        return Ok(());
    };
    if selected.document.branch != result.canonical_branch_name {
        task::write_task_branch(ctx, &selected.key, &result.canonical_branch_name)?;
    }
    Ok(())
}

fn record_task_profile_results(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    partial: &issue::IssueRunPartialFailure,
) -> Result<()> {
    record_task_profile_successes(ctx, selected, &partial.completed)?;
    if let Some(failed) = &partial.failed {
        let run = task_run::create(
            ctx,
            &selected.key,
            &failed.branch_name,
            task_run::SOURCE_NEW,
            None,
            task_run::STATUS_FAILED,
        )?;
        task_run::update(
            ctx,
            &run.id,
            task_run::STATUS_FAILED,
            None,
            Some(partial.message()),
        )?;
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
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{CommandCall, MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
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
        let profile_dir = root.join(".local/profiles").join(name);
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
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

    #[test]
    fn duplicate_task_values_are_rejected() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
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
        let tasks_dir = repo.path().join(".local/tasks");
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

        run(&ctx, &["add-schema".into()], &None, None, false).unwrap();

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
        assert_eq!(runs[0].run.source, task_run::SOURCE_NEW);
        assert_eq!(runs[0].run.status, task_run::STATUS_RUNNING);
        assert_eq!(runs[0].run.branch, "add-schema");
    }

    #[test]
    fn task_run_with_key_records_new_run_after_prior_done() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
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
            task_run::SOURCE_NEW,
            None,
            task_run::STATUS_DONE,
        )
        .unwrap();

        run(&ctx, &["add-schema".into()], &None, None, false).unwrap();

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
        assert_eq!(latest.run.source, task_run::SOURCE_NEW);
        assert_eq!(latest.run.status, task_run::STATUS_RUNNING);
        assert_eq!(latest.run.branch, "add-schema");
    }

    #[test]
    fn bare_task_run_selects_local_tasks() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
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

        run(&ctx, &[], &None, None, false).unwrap();

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
        assert_eq!(runs[0].run.source, task_run::SOURCE_NEW);
        assert_eq!(runs[0].run.status, task_run::STATUS_RUNNING);
        assert_eq!(runs[0].run.branch, "second");
        assert_eq!(runs[0].run.group, None);
    }

    #[test]
    fn task_run_multiple_keys_start_separate_worktrees() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
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
        assert!(
            runs.iter()
                .all(|record| record.run.source == task_run::SOURCE_NEW)
        );
        let branches = runs
            .iter()
            .map(|record| record.run.branch.as_str())
            .collect::<Vec<_>>();
        assert!(branches.contains(&"add-schema"));
        assert!(branches.contains(&"publish-issues"));
    }

    #[test]
    fn task_run_matrix_records_created_profile_runs_after_later_profile_failure() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\nbody = \"Create the schema first.\"\n",
        )
        .unwrap();

        let alpha_dir = repo.path().join(".local/profiles/alpha");
        std::fs::create_dir_all(&alpha_dir).unwrap();
        std::fs::write(alpha_dir.join("profile.toml"), "").unwrap();

        let beta_dir = repo.path().join(".local/profiles/beta");
        std::fs::create_dir_all(beta_dir.join("scaffold/AGENTS.override.md")).unwrap();
        std::fs::write(
            beta_dir.join("profile.toml"),
            r#"
[worktree]
inject_local_context = "context"

[agent]
cli = "codex"
"#,
        )
        .unwrap();

        let mut config = Config::default();
        config.worktree.path = Some("worktrees/{{branch_sanitized}}".into());

        let mut runner = MockRunner::new();
        runner.add_response("", false); // alpha profile branch local_branch_exists
        runner.add_response("", true); // alpha worktree_add_new_branch
        runner.add_response("", true); // alpha parent local_branch_exists
        runner.add_response("", true); // alpha set parent config
        runner.add_response("", false); // beta profile branch local_branch_exists
        runner.add_response("", true); // beta worktree_add_new_branch
        runner.add_response("", true); // beta parent local_branch_exists
        runner.add_response("", true); // beta set parent config
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            config.clone(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let result = run(
            &ctx,
            &["add-schema".into()],
            &Some("main".into()),
            None,
            true,
        );

        assert!(result.is_err());
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

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        let alpha = runs
            .iter()
            .find(|record| record.run.branch == "add-schema-alpha")
            .expect("expected alpha profile TaskRun");
        assert_eq!(alpha.run.task, "add-schema");
        assert_eq!(alpha.run.source, task_run::SOURCE_NEW);
        assert_eq!(alpha.run.status, task_run::STATUS_RUNNING);
        assert!(alpha.run.error.is_none());

        let beta = runs
            .iter()
            .find(|record| record.run.branch == "add-schema-beta")
            .expect("expected beta profile TaskRun");
        assert_eq!(beta.run.task, "add-schema");
        assert_eq!(beta.run.source, task_run::SOURCE_NEW);
        assert_eq!(beta.run.status, task_run::STATUS_FAILED);
        assert!(beta.run.error.is_some());
        assert!(task_run::task_is_selectable(&ctx, "add-schema").unwrap());

        let alpha_path = repo.path().join("worktrees/add-schema-alpha");
        let mut clean_runner = MockRunner::new();
        clean_runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/add-schema-alpha\n\n",
                repo.path().display(),
                alpha_path.display()
            ),
            true,
        );
        clean_runner.add_response("", true); // worktree remove
        clean_runner.add_response("", true); // branch delete
        clean_runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        let clean_ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            config,
            Box::new(clean_runner),
            Box::new(MockUi::new()),
        );

        crate::commands::clean::run_with_targets(&clean_ctx, &["add-schema-alpha".into()]).unwrap();

        let runs = task_run::list(&clean_ctx).unwrap();
        let alpha = runs
            .iter()
            .find(|record| record.run.branch == "add-schema-alpha")
            .expect("expected alpha profile TaskRun");
        assert_eq!(alpha.run.status, task_run::STATUS_DONE);
        let beta = runs
            .iter()
            .find(|record| record.run.branch == "add-schema-beta")
            .expect("expected beta profile TaskRun");
        assert_eq!(beta.run.status, task_run::STATUS_FAILED);
    }

    #[test]
    fn task_run_updates_task_and_run_branch_from_issue_origin() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
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

        run(&ctx, &["PROJ-123".into()], &None, None, false).unwrap();

        let task_content =
            std::fs::read_to_string(repo.path().join(".local/tasks/PROJ-123.toml")).unwrap();
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
        let tasks_dir = repo.path().join(".local/tasks");
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
        let tasks_dir = repo.path().join(".local/tasks");
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
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, _, _)| cmd != "linear"));
    }
}
