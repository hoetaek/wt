use crate::cli::BaseMode;
use crate::commands::{issue, task, task_run};
use crate::config::Config;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::setup;
use anyhow::{Result, bail};

pub fn run(
    ctx: &Ctx,
    name_words: &[String],
    task_key: Option<Option<&str>>,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
) -> Result<()> {
    if let Some(task_key) = task_key {
        if !name_words.is_empty() {
            bail!("wt new accepts branch-name text or --task, not both");
        }
        let task_key = task_key.filter(|key| !key.trim().is_empty());
        return run_selected_task(ctx, task_key, base_raw, profile, matrix).map(|_| ());
    }

    if name_words.is_empty() {
        bail!("wt new requires branch-name text or --task [<task-key>]");
    }

    let branch_name = branch_name_from_words(name_words)?;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    if matrix || profile.is_some() {
        let base_mode = BaseMode::from_raw(base_raw);
        let base = resolve_base_branch(ctx, &git, &base_mode)?;
        return run_profiles(ctx, &branch_name, &base, profile);
    }

    let names = WorktreeNames::new_with_config(
        &branch_name,
        &ctx.parent_dir,
        &ctx.repo_root,
        &ctx.repo_name,
        None,
        ctx.config.has_site().then_some(""),
        ctx.config.worktree.path.as_deref(),
    )?;

    // Check if worktree path already exists
    if names.path.exists() {
        ctx.ui.print_warning(&format!(
            "Worktree {} already exists.",
            names.path.display()
        ));
        let items = vec!["Delete and recreate".into(), "Abort".into()];
        let choice = ctx.ui.select("Worktree already exists", &items)?;
        match choice {
            0 => {
                ctx.ui.print_step("Removing existing worktree...");
                git.worktree_remove_force(&names.path).ok();
                if names.path.exists() {
                    std::fs::remove_dir_all(&names.path)?;
                }
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // Resolve base branch
    let base_mode = BaseMode::from_raw(base_raw);
    let base = resolve_base_branch(ctx, &git, &base_mode)?;

    // Check if branch already exists
    if git.local_branch_exists(&branch_name)? {
        return Err(WtError::BranchExistsWithBase {
            branch: branch_name.clone(),
        }
        .into());
    }

    ctx.ui
        .print_step(&format!("Creating new branch from {base}"));
    git.worktree_add_new_branch(&names.path, &branch_name, &base)?;
    git.set_branch_parent(&branch_name, &base).ok();

    setup::run_setup(ctx, &names.path, &names, None, "new", None, None)?;

    Ok(())
}

fn run_selected_task(
    ctx: &Ctx,
    task_key: Option<&str>,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
) -> Result<issue::IssueRunResult> {
    let selected = if let Some(task_key) = task_key {
        task::select_local_task_by_key(ctx, task_key)?
    } else {
        task::select_local_task(ctx)?
    };
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
                    if let Err(record_err) =
                        record_new_task_profile_results(ctx, &selected, partial)
                    {
                        return Err(anyhow::anyhow!(
                            "Failed to record partial profile TaskRuns after profile run failed ({err}): {record_err}"
                        ));
                    }
                } else {
                    record_new_task_failure(ctx, &selected, &err);
                }
                return Err(err);
            }
        };
        if results.is_empty() {
            bail!("No profile worktrees created");
        }
        record_new_task_profile_successes(ctx, &selected, &results)?;
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

    if selected.document.branch != result.branch_name {
        if let Err(err) = task::write_task_branch(ctx, &selected.key, &result.branch_name) {
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

fn record_new_task_failure(ctx: &Ctx, selected: &task::SelectedTask, err: &anyhow::Error) {
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

fn record_new_task_profile_successes(
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
    Ok(())
}

fn record_new_task_profile_results(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    partial: &issue::IssueRunPartialFailure,
) -> Result<()> {
    record_new_task_profile_successes(ctx, selected, &partial.completed)?;
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

pub(crate) fn branch_name_from_words(name_words: &[String]) -> Result<String> {
    let kebab: String = name_words
        .join(" ")
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    if kebab.is_empty() {
        bail!("Failed to create valid branch name from input");
    }

    Ok(kebab)
}

fn run_profiles(ctx: &Ctx, branch_name: &str, base: &str, profile: Option<&str>) -> Result<()> {
    let profiles = load_selected_profiles(ctx, profile)?;

    ctx.ui.print_step(&format!(
        "Found {} profiles: {}",
        profiles.len(),
        profiles
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    for (profile_name, profile_config) in &profiles {
        let profile_branch = format!("{branch_name}-{profile_name}");

        ctx.ui
            .print_step(&format!("Setting up profile: {profile_name}"));

        let names = WorktreeNames::new_with_config(
            &profile_branch,
            &ctx.parent_dir,
            &ctx.repo_root,
            &ctx.repo_name,
            None,
            profile_config.has_site().then_some(""),
            profile_config.worktree.path.as_deref(),
        )?;

        if names.path.exists() {
            ctx.ui.print_warning(&format!(
                "Worktree {} already exists.",
                names.path.display()
            ));
            let items = vec![
                "Delete and recreate".into(),
                "Skip".into(),
                "Abort all".into(),
            ];
            let choice = ctx
                .ui
                .select(&format!("[{profile_name}] Worktree already exists"), &items)?;
            match choice {
                0 => {
                    ctx.ui.print_step("Removing existing worktree...");
                    git.worktree_remove_force(&names.path).ok();
                    if names.path.exists() {
                        std::fs::remove_dir_all(&names.path)?;
                    }
                }
                1 => continue,
                _ => return Err(WtError::Cancelled.into()),
            }
        }

        if git.local_branch_exists(&profile_branch)? {
            ctx.ui.print_warning(&format!(
                "Branch {profile_branch} already exists, removing..."
            ));
            git.worktree_remove_force(&names.path).ok();
            ctx.runner
                .run(
                    "git",
                    &["branch", "-D", &profile_branch],
                    Some(&ctx.repo_root),
                )
                .ok();
        }

        git.worktree_add_new_branch(&names.path, &profile_branch, base)?;
        git.set_branch_parent(&profile_branch, base).ok();

        setup::run_setup(
            ctx,
            &names.path,
            &names,
            None,
            "new",
            None,
            Some(profile_config),
        )?;
    }

    ctx.ui.print_step(&format!(
        "All {} profiles created successfully",
        profiles.len()
    ));
    Ok(())
}

fn load_selected_profiles(ctx: &Ctx, profile: Option<&str>) -> Result<Vec<(String, Config)>> {
    if let Some(profile) = profile {
        let config = Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?
            .ok_or_else(|| anyhow::anyhow!("Profile '{profile}' not found"))?;
        return Ok(vec![(profile.to_string(), config)]);
    }

    let profiles = Config::load_profiles(&ctx.repo_root, &ctx.base_config)?;
    if profiles.is_empty() {
        bail!("No profile configs found in .local/profiles/*/profile.toml");
    }
    Ok(profiles)
}

fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<WtError>()
        .is_some_and(|err| matches!(err, WtError::Cancelled))
}

fn resolve_base_branch(ctx: &Ctx, git: &GitService, mode: &BaseMode) -> Result<String> {
    let base = match mode {
        BaseMode::Explicit(branch) => Ok(branch.clone()),
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
            let input = ctx.ui.input("Base branch", Some(&current))?;
            Ok(input)
        }
    }?;

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
    use std::path::{Path, PathBuf};
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

    fn make_ctx(runner: MockRunner, ui: MockUi) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        )
    }

    #[test]
    fn kebab_case_conversion() {
        let words: Vec<String> = vec!["Some".into(), "Feature".into(), "Name".into()];
        let kebab = branch_name_from_words(&words).unwrap();
        assert_eq!(kebab, "some-feature-name");
    }

    #[test]
    fn empty_name_requires_branch_text_or_task_option() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = make_ctx(runner, ui);

        let result = run(&ctx, &[], None, &None, None, false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("wt new requires branch-name text or --task")
        );
    }

    #[test]
    fn task_option_without_value_reaches_local_task_selection() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = make_ctx(runner, ui);

        let result = run(&ctx, &[], Some(None), &None, None, false);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No task files found in .local/tasks")
        );
    }

    #[test]
    fn task_option_rejects_branch_name_text() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = make_ctx(runner, ui);

        let result = run(
            &ctx,
            &["add".into(), "schema".into()],
            Some(Some("add-schema")),
            &None,
            None,
            false,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("wt new accepts branch-name text or --task, not both")
        );
    }

    #[test]
    fn task_option_selects_local_task_and_runs_task_snapshot() {
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

        let mut ui = MockUi::new();
        ui.add_select(0);
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx, &[], Some(None), &None, None, false).unwrap();

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
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run.task, "add-schema");
        assert_eq!(runs[0].run.source, task_run::SOURCE_NEW);
        assert_eq!(runs[0].run.status, task_run::STATUS_RUNNING);
        assert_eq!(runs[0].run.branch, "add-schema");
    }

    #[test]
    fn task_option_with_key_runs_named_task_snapshot() {
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

        run(&ctx, &[], Some(Some("add-schema")), &None, None, false).unwrap();

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
    fn task_option_matrix_records_created_profile_runs_after_later_profile_failure() {
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
            &[],
            Some(Some("add-schema")),
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
    fn task_option_updates_task_and_run_branch_from_issue_origin() {
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

        run(&ctx, &[], Some(Some("PROJ-123")), &None, None, false).unwrap();

        let task_content =
            std::fs::read_to_string(repo.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(task_content.contains("branch = \"alice/proj-123-fix-editor\""));

        let latest = task_run::latest_for_task(&ctx, "PROJ-123")
            .unwrap()
            .expect("expected latest task run");
        assert_eq!(latest.run.status, task_run::STATUS_RUNNING);
        assert_eq!(latest.run.branch, "alice/proj-123-fix-editor");
    }

    #[test]
    fn new_default_base_prompt_uses_invocation_root_for_current_branch() {
        let repo_root = PathBuf::from("/tmp/sample-app");
        let invocation_root = PathBuf::from("/tmp/sample-app-alice-proj-670");
        let mut runner = MockRunner::new();
        runner.add_response("alice/proj-670-current", true);
        runner.add_response("", false);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ui = MockUi::new();
        let ctx = Ctx::new(
            repo_root,
            invocation_root.clone(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let words: Vec<String> = vec!["my".into(), "feature".into()];
        let result = run(&ctx, &words, None, &None, None, false);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));

        let calls = runner.calls.lock().unwrap();
        let current_branch_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args
                        == &vec![
                            "rev-parse".to_string(),
                            "--abbrev-ref".to_string(),
                            "HEAD".to_string(),
                        ]
            })
            .expect("expected git current branch call");
        assert_eq!(
            current_branch_call.2.as_deref(),
            Some(invocation_root.as_path())
        );

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
        assert_eq!(worktree_add_call.1[5], "alice/proj-670-current");
    }

    #[test]
    fn branch_already_exists_returns_error() {
        let mut runner = MockRunner::new();
        // current_branch for base resolution
        runner.add_response("main", true);
        // local_branch_exists returns true
        runner.add_response("", true);

        let mut ui = MockUi::new();
        ui.add_input("main");

        let ctx = make_ctx(runner, ui);
        let words: Vec<String> = vec!["my".into(), "feature".into()];
        let result = run(&ctx, &words, None, &None, None, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn default_base_rejects_empty_prompt_result() {
        let mut runner = MockRunner::new();
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_input("   ");
        let ctx = make_ctx(runner, ui);
        let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

        let result = resolve_base_branch(&ctx, &git, &BaseMode::Default);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Base branch cannot be empty")
        );
    }

    #[test]
    fn explicit_base_branch_skips_prompt() {
        let mut runner = MockRunner::new();
        // local_branch_exists returns false
        runner.add_response("", false);
        // worktree_add_new_branch
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = make_ctx(runner, ui);
        let words: Vec<String> = vec!["my".into(), "feature".into()];
        let result = run(&ctx, &words, None, &Some("develop".into()), None, false);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn current_base_uses_current_branch_without_prompt() {
        let mut runner = MockRunner::new();
        // current_branch for --base .
        runner.add_response("feature/current", true);
        // local_branch_exists returns false
        runner.add_response("", false);
        // worktree_add_new_branch
        runner.add_response("", true);
        // set_branch_parent local_branch_exists
        runner.add_response("", true);
        // set_branch_parent config
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );
        let words: Vec<String> = vec!["my".into(), "feature".into()];
        run(&ctx, &words, None, &Some(".".into()), None, false).unwrap();

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
        assert_eq!(worktree_add_call.1[5], "feature/current");
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "config".to_string(),
                        "branch.my-feature.parentbranch".to_string(),
                        "feature/current".to_string(),
                    ]
        }));
    }

    #[test]
    fn new_with_profile_records_parentbranch_for_profile_branch() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".local/profiles/codex-yolo");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();

        let mut runner = MockRunner::new();
        // profile branch local_branch_exists
        runner.add_response("", false);
        // worktree_add_new_branch
        runner.add_response("", true);
        // set_branch_parent local_branch_exists
        runner.add_response("", true);
        // set_branch_parent config
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

        run(
            &ctx,
            &["my".into(), "feature".into()],
            None,
            &Some("main".into()),
            Some("codex-yolo"),
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "config".to_string(),
                        "branch.my-feature-codex-yolo.parentbranch".to_string(),
                        "main".to_string(),
                    ]
        }));
    }

    #[test]
    fn new_uses_unprefixed_branch_name_by_default() {
        let repo = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        // local_branch_exists returns false
        runner.add_response("", false);
        // worktree_add_new_branch
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().join("repo"),
            repo.path().join("repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            &["my".into(), "feature".into()],
            None,
            &Some("develop".into()),
            None,
            false,
        )
        .unwrap();

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
        assert_eq!(worktree_add_call.1[3], "my-feature");
    }

    #[test]
    fn new_uses_configured_worktree_path() {
        let repo = tempfile::tempdir().unwrap();
        let repo_root = repo.path().join("repo");
        let mut config = Config::default();
        config.worktree.path = Some("worktrees/{{default_name}}".into());

        let mut runner = MockRunner::new();
        // local_branch_exists returns false
        runner.add_response("", false);
        // worktree_add_new_branch
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo_root.clone(),
            repo_root.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            &["my".into(), "feature".into()],
            None,
            &Some("develop".into()),
            None,
            false,
        )
        .unwrap();

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
        assert_eq!(
            worktree_add_call.1[4],
            repo_root
                .join("worktrees/repo-my-feature")
                .to_string_lossy()
                .as_ref()
        );
    }
}
