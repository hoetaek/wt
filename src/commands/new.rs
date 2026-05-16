use crate::cli::BaseMode;
use crate::commands::profile_workspace::{
    ProfileBranchDecision, PromptPolicy, resolve_profile_branch,
};
use crate::commands::{issue, task, task_run};
use crate::config::Config;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::setup;
use anyhow::{Result, bail};
use std::collections::HashSet;

pub fn run(
    ctx: &Ctx,
    name_words: &[String],
    task_args: &[String],
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
) -> Result<()> {
    if !task_args.is_empty() {
        let interactive_selection = task_args.iter().all(|task| task.trim().is_empty());
        let selected = if interactive_selection {
            if task_args.len() > 1 {
                bail!("Use either one bare --task selector or explicit --task <task> values");
            }
            task::select_local_tasks(ctx)?
        } else {
            if task_args.iter().any(|task| task.trim().is_empty()) {
                bail!("Use either one bare --task selector or explicit --task <task> values");
            }
            select_named_tasks(ctx, task_args)?
        };
        if selected.is_empty() {
            bail!("No local tasks selected");
        }
        if name_words.is_empty() {
            if selected.len() == 1 {
                return run_single_selected_task(ctx, &selected[0], base_raw, profile, matrix)
                    .map(|_| ());
            }
            if !interactive_selection {
                bail!("wt new with multiple --task values requires branch-name text");
            }
            let branch_name = prompt_workspace_branch(ctx, &selected)?;
            return run_selected_tasks_workspace(
                ctx,
                &branch_name,
                &branch_name_words(&branch_name),
                &selected,
                base_raw,
                profile,
                matrix,
            )
            .map(|_| ());
        }

        let branch_name = branch_name_from_words(name_words)?;
        return run_selected_tasks_workspace(
            ctx,
            &branch_name,
            name_words,
            &selected,
            base_raw,
            profile,
            matrix,
        )
        .map(|_| ());
    }

    if name_words.is_empty() {
        bail!("wt new requires branch-name text or --task <task-key>");
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

fn prompt_workspace_branch(ctx: &Ctx, selected: &[task::SelectedTask]) -> Result<String> {
    let default = branch_name_from_words(&[selected
        .iter()
        .map(|task| task.key.as_str())
        .collect::<Vec<_>>()
        .join(" ")])?;
    let input = ctx.ui.input("Workspace branch name", Some(&default))?;
    branch_name_from_words(&[input])
}

fn branch_name_words(branch_name: &str) -> Vec<String> {
    branch_name
        .split('-')
        .filter(|part| !part.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn run_single_selected_task(
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
                    if let Err(record_err) =
                        record_new_task_profile_results(ctx, selected, partial, None)
                    {
                        return Err(anyhow::anyhow!(
                            "Failed to record partial profile TaskRuns after profile run failed ({err}): {record_err}"
                        ));
                    }
                } else {
                    record_new_task_failure(ctx, selected, &err);
                }
                return Err(err);
            }
        };
        if results.is_empty() {
            bail!("No profile worktrees created");
        }
        record_new_task_profile_successes(ctx, selected, &results, None)?;
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

fn run_selected_tasks_workspace(
    ctx: &Ctx,
    branch_name: &str,
    name_words: &[String],
    selected: &[task::SelectedTask],
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
) -> Result<Vec<issue::IssueRunResult>> {
    let title = name_words.join(" ");
    let snapshot_path = selected
        .iter()
        .map(|task| task.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let snapshot_content = render_selected_tasks_snapshot(selected);
    let prompt_intro = if selected.len() == 1 {
        "Use this task before changing code."
    } else {
        "Use these tasks before changing code. Work in this single workspace and address every selected TaskDocument."
    };
    let workspace_label = (selected.len() > 1).then(|| format!("{} tasks", selected.len()));
    let group = (selected.len() > 1)
        .then(|| next_new_task_group(ctx, branch_name))
        .transpose()?;

    let results = issue::run_with_issue_snapshot_many(
        ctx,
        base_raw,
        profile,
        matrix,
        issue::PreparedIssueContext {
            identifier: branch_name,
            title: &title,
            branch_name: Some(branch_name),
            mode: "new",
            on_start_issue_id: None,
            prompt_intro,
            workspace_label,
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task paths",
                path: &snapshot_path,
                content: &snapshot_content,
            },
        },
    );

    let results = match results {
        Ok(results) => results,
        Err(err) => {
            if let Some(partial) = err.downcast_ref::<issue::IssueRunPartialFailure>() {
                if let Err(record_err) =
                    record_new_tasks_profile_results(ctx, selected, partial, group.as_deref())
                {
                    return Err(anyhow::anyhow!(
                        "Failed to record partial TaskRuns after profile run failed ({err}): {record_err}"
                    ));
                }
            } else if let Err(record_err) =
                record_new_tasks_failure(ctx, selected, branch_name, &err, group.as_deref())
            {
                return Err(anyhow::anyhow!(
                    "Failed to record failed TaskRuns after run failed ({err}): {record_err}"
                ));
            }
            return Err(err);
        }
    };

    if results.is_empty() {
        bail!("No worktrees created");
    }
    record_new_tasks_profile_successes(ctx, selected, &results, group.as_deref())?;
    Ok(results)
}

fn next_new_task_group(ctx: &Ctx, branch_name: &str) -> Result<String> {
    let prefix = task::safe_task_key(branch_name);
    let next = task_run::list(ctx)?
        .into_iter()
        .filter(|record| record.run.source == task_run::SOURCE_NEW)
        .filter_map(|record| record.run.group)
        .filter_map(|group| group_sequence(&group, &prefix))
        .max()
        .unwrap_or(0)
        + 1;
    Ok(format!("{prefix}-{next:03}"))
}

fn group_sequence(group: &str, prefix: &str) -> Option<u64> {
    group
        .strip_prefix(prefix)?
        .strip_prefix('-')?
        .parse::<u64>()
        .ok()
}

fn render_selected_tasks_snapshot(selected: &[task::SelectedTask]) -> String {
    let mut content = String::new();
    content.push_str("Selected TaskDocuments:\n");
    for task in selected {
        content.push_str(&format!("- {}: {}\n", task.key, task.path));
    }
    for task in selected {
        content.push_str(&format!("\n--- {} ({}) ---\n", task.key, task.path));
        content.push_str(task.content.trim_end());
        content.push('\n');
    }
    content
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
    group: Option<&str>,
) -> Result<()> {
    for result in results {
        task_run::create(
            ctx,
            &selected.key,
            &result.branch_name,
            task_run::SOURCE_NEW,
            group,
            task_run::STATUS_RUNNING,
        )?;
    }
    Ok(())
}

fn record_new_tasks_profile_successes(
    ctx: &Ctx,
    selected: &[task::SelectedTask],
    results: &[issue::IssueRunResult],
    group: Option<&str>,
) -> Result<()> {
    for task in selected {
        record_new_task_profile_successes(ctx, task, results, group)?;
    }
    Ok(())
}

fn record_new_task_profile_results(
    ctx: &Ctx,
    selected: &task::SelectedTask,
    partial: &issue::IssueRunPartialFailure,
    group: Option<&str>,
) -> Result<()> {
    record_new_task_profile_successes(ctx, selected, &partial.completed, group)?;
    if let Some(failed) = &partial.failed {
        let run = task_run::create(
            ctx,
            &selected.key,
            &failed.branch_name,
            task_run::SOURCE_NEW,
            group,
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

fn record_new_tasks_profile_results(
    ctx: &Ctx,
    selected: &[task::SelectedTask],
    partial: &issue::IssueRunPartialFailure,
    group: Option<&str>,
) -> Result<()> {
    for task in selected {
        record_new_task_profile_results(ctx, task, partial, group)?;
    }
    Ok(())
}

fn record_new_tasks_failure(
    ctx: &Ctx,
    selected: &[task::SelectedTask],
    branch: &str,
    err: &anyhow::Error,
    group: Option<&str>,
) -> Result<()> {
    let status = if is_cancelled(err) {
        task_run::STATUS_SKIPPED
    } else {
        task_run::STATUS_FAILED
    };
    let message = err.to_string();
    for task in selected {
        let run = task_run::create(ctx, &task.key, branch, task_run::SOURCE_NEW, group, status)?;
        task_run::update(ctx, &run.id, status, None, Some(&message))?;
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

        match resolve_profile_branch(
            ctx,
            &git,
            profile_name,
            &profile_branch,
            &names.path,
            PromptPolicy::Allow,
        )? {
            ProfileBranchDecision::CreateNew { .. } => {}
            ProfileBranchDecision::ReuseExisting { path } => {
                setup::run_setup(ctx, &path, &names, None, "new", None, Some(profile_config))?;
                continue;
            }
            ProfileBranchDecision::Skip => continue,
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
        "All {} profiles processed successfully",
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
    use crate::context::mock::{CommandCall, MockRunner, MockUi};
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

        let result = run(&ctx, &[], &[], &None, None, false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("wt new requires branch-name text or --task")
        );
    }

    #[test]
    fn multiple_task_values_require_branch_name_text() {
        let repo = tempfile::tempdir().unwrap();
        let tasks_dir = repo.path().join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("publish-issues.toml"),
            "title = \"Publish issues\"\nbranch = \"publish-issues\"\n",
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
            &[],
            &["add-schema".into(), "publish-issues".into()],
            &None,
            None,
            false,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("wt new with multiple --task values requires branch-name text")
        );
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
            &["workspace".into()],
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

        run(&ctx, &[], &["add-schema".into()], &None, None, false).unwrap();

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
    fn task_option_with_key_records_new_run_after_prior_done() {
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

        run(&ctx, &[], &["add-schema".into()], &None, None, false).unwrap();

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
    fn task_option_without_value_selects_one_local_task() {
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

        run(&ctx, &[], &["".into()], &None, None, false).unwrap();

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
    fn bare_task_multi_select_prompts_branch_and_records_group() {
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
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 1]);
        ui.add_input("team-run");
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx, &[], &["".into()], &Some("main".into()), None, false).unwrap();

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
        assert_eq!(worktree_add_call.1[3], "team-run");
        assert_eq!(worktree_add_call.1[5], "main");

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|record| record.run.branch == "team-run"));
        assert!(
            runs.iter()
                .all(|record| record.run.group.as_deref() == Some("team-run-001"))
        );
        assert!(
            runs.iter()
                .all(|record| record.id.starts_with("new-team-run-001-"))
        );
    }

    #[test]
    fn named_workspace_records_one_task_run_per_selected_task() {
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
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local branch exists
        runner.add_response("", false); // remote branch exists
        runner.add_response("", true); // worktree add
        runner.add_response("", true); // parent local branch exists
        runner.add_response("", true); // set parent config
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
            &["publish".into(), "tasks".into()],
            &["add-schema".into(), "publish-issues".into()],
            &Some("main".into()),
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
        assert_eq!(worktree_add_call.1[3], "publish-tasks");
        assert_eq!(worktree_add_call.1[5], "main");

        let runs = task_run::list(&ctx).unwrap();
        assert_eq!(runs.len(), 2);
        let task_keys = runs
            .iter()
            .map(|record| record.run.task.as_str())
            .collect::<Vec<_>>();
        assert!(task_keys.contains(&"add-schema"));
        assert!(task_keys.contains(&"publish-issues"));
        assert!(
            runs.iter()
                .all(|record| record.run.branch == "publish-tasks")
        );
        assert!(
            runs.iter()
                .all(|record| record.run.source == task_run::SOURCE_NEW)
        );
        assert!(
            runs.iter()
                .all(|record| record.run.status == task_run::STATUS_RUNNING)
        );
        assert!(
            runs.iter()
                .all(|record| record.run.group.as_deref() == Some("publish-tasks-001"))
        );
        assert!(
            runs.iter()
                .all(|record| record.id.starts_with("new-publish-tasks-001-"))
        );
    }

    #[test]
    fn named_workspace_local_tasks_do_not_update_issue_provider() {
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
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local branch exists
        runner.add_response("", false); // remote branch exists
        runner.add_response("", true); // worktree add
        runner.add_response("", true); // parent local branch exists
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
            &["publish".into(), "tasks".into()],
            &["add-schema".into(), "publish-issues".into()],
            &Some("main".into()),
            None,
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, _, _)| cmd != "linear"));
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

        run(&ctx, &[], &["PROJ-123".into()], &None, None, false).unwrap();

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
    fn task_option_with_provider_origin_and_profile_updates_start_status() {
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
            &[],
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

        let calls = runner.calls.lock().unwrap();
        assert_eq!(count_linear_start_updates(&calls, "PROJ-123"), 1);
    }

    #[test]
    fn task_option_with_local_task_and_profile_does_not_touch_issue_provider() {
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
            &[],
            &["add-schema".into()],
            &Some("main".into()),
            Some("codex"),
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, _, _)| cmd != "linear"));
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
        let result = run(&ctx, &words, &[], &None, None, false);
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
        let result = run(&ctx, &words, &[], &None, None, false);
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
        let result = run(&ctx, &words, &[], &Some("develop".into()), None, false);
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
        run(&ctx, &words, &[], &Some(".".into()), None, false).unwrap();

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
    fn new_profile_existing_branch_without_worktree_reuses_branch() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true); // profile branch local_branch_exists
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        ); // checked_out_path
        runner.add_response("", true); // worktree_add existing branch
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0); // reuse existing branch
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
            &["my".into(), "feature".into()],
            &[],
            &Some("main".into()),
            Some("codex"),
            false,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args.len() == 4
                && args[0] == "worktree"
                && args[1] == "add"
                && args[3] == "my-feature-codex"
        }));
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args
                    == &vec![
                        "branch".to_string(),
                        "-D".to_string(),
                        "my-feature-codex".to_string(),
                    ])
        }));
    }

    #[test]
    fn new_profile_branch_delete_failure_is_reported_directly() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true); // profile branch local_branch_exists
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        ); // checked_out_path
        runner.add_response_with_stderr("", "cannot delete protected branch", false);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(1); // delete and recreate
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let result = run(
            &ctx,
            &["my".into(), "feature".into()],
            &[],
            &Some("main".into()),
            Some("codex"),
            false,
        );

        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(message.contains("git branch -D my-feature-codex failed"));
        assert!(message.contains("cannot delete protected branch"));

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args.len() >= 3
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b")
        }));
    }

    #[test]
    fn new_profile_path_recreate_reports_branch_delete_failure_directly() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
        std::fs::create_dir_all(repo.path().join("worktrees/my-feature-codex")).unwrap();

        let mut config = Config::default();
        config.worktree.path = Some("worktrees/{{branch_sanitized}}".into());

        let mut runner = MockRunner::new();
        runner.add_response("", true); // worktree_remove_force
        runner.add_response("", true); // profile branch local_branch_exists
        runner.add_response_with_stderr("", "cannot delete protected branch", false);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0); // delete and recreate existing path
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let result = run(
            &ctx,
            &["my".into(), "feature".into()],
            &[],
            &Some("main".into()),
            Some("codex"),
            false,
        );

        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(message.contains("git branch -D my-feature-codex failed"));
        assert!(message.contains("cannot delete protected branch"));

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "branch".to_string(),
                        "-D".to_string(),
                        "my-feature-codex".to_string(),
                    ]
        }));
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args.len() >= 3
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b")
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
            &[],
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
            &[],
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
            &[],
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
