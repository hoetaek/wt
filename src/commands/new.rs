use crate::cli::BaseMode;
use crate::commands::profile_selection::{self, ProfileSelection};
use crate::commands::profile_workspace::{
    ProfileBranchDecision, PromptPolicy, resolve_profile_branch,
};
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::setup;
use anyhow::{Result, bail};

pub fn run(
    ctx: &Ctx,
    name_words: &[String],
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
) -> Result<()> {
    if name_words.is_empty() {
        bail!(
            "wt run branch starts one ad hoc worktree from branch-name text. Pass branch-name text, or use `wt run task [<task>...]` for prepared local tasks."
        );
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

    setup::run_setup(
        ctx,
        &names.path,
        &names,
        None,
        setup::WORKSPACE_COLOR_KIND_BRANCH,
        None,
        None,
    )?;

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
    let profiles =
        profile_selection::load_profile_selection(ctx, ProfileSelection::new(profile, &[]))?;

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
                setup::run_setup(
                    ctx,
                    &path,
                    &names,
                    None,
                    setup::WORKSPACE_COLOR_KIND_BRANCH,
                    None,
                    Some(profile_config),
                )?;
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
            setup::WORKSPACE_COLOR_KIND_BRANCH,
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
    use crate::config::Config;
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

    fn write_empty_profile(root: &Path, name: &str) {
        let profile_dir = root.join(".wt/config/profiles").join(name);
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
    }

    fn run(
        ctx: &Ctx,
        name_words: &[String],
        base_raw: &Option<String>,
        profile: Option<&str>,
        selected_profiles: &[String],
        matrix: bool,
    ) -> Result<()> {
        assert!(selected_profiles.is_empty());
        super::run(ctx, name_words, base_raw, profile, matrix)
    }

    #[test]
    fn kebab_case_conversion() {
        let words: Vec<String> = vec!["Some".into(), "Feature".into(), "Name".into()];
        let kebab = branch_name_from_words(&words).unwrap();
        assert_eq!(kebab, "some-feature-name");
    }

    #[test]
    fn empty_name_requires_branch_text() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = make_ctx(runner, ui);

        let result = run(&ctx, &[], &None, None, &[], false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("wt run branch starts one ad hoc worktree from branch-name text")
        );
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
        let result = run(&ctx, &words, &None, None, &[], false);
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
        let result = run(&ctx, &words, &None, None, &[], false);
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
        let result = run(&ctx, &words, &Some("develop".into()), None, &[], false);
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
        run(&ctx, &words, &Some(".".into()), None, &[], false).unwrap();

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
        let profile_dir = repo.path().join(".wt/config/profiles/codex");
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
            &Some("main".into()),
            Some("codex"),
            &[],
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
        let profile_dir = repo.path().join(".wt/config/profiles/codex");
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
            &Some("main".into()),
            Some("codex"),
            &[],
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
        let profile_dir = repo.path().join(".wt/config/profiles/codex");
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
            &Some("main".into()),
            Some("codex"),
            &[],
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
        write_empty_profile(repo.path(), "codex-yolo");

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
            &Some("main".into()),
            Some("codex-yolo"),
            &[],
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
            &Some("develop".into()),
            None,
            &[],
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
            &Some("develop".into()),
            None,
            &[],
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
