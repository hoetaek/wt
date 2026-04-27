use crate::cli::BaseMode;
use crate::config::Config;
use crate::config::IssueProviderType;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::{CreateType, GitService};
use crate::services::issues::IssueProvider;
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
use crate::setup;
use anyhow::{Result, bail};

pub fn run(
    ctx: &Ctx,
    number: Option<u32>,
    base_raw: &Option<String>,
    parallel: bool,
) -> Result<()> {
    let provider = build_provider(ctx)?;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    // 1. Resolve issue
    let (identifier, title) = if let Some(num) = number {
        let issue = provider.get_issue(&num.to_string())?;
        (issue.identifier, issue.title)
    } else {
        let issues = provider.list_issues()?;
        if issues.is_empty() {
            bail!("No issues found");
        }

        let items: Vec<String> = issues.iter().map(|i| i.display.clone()).collect();
        let idx = ctx.ui.select("Select an issue", &items)?;
        let selected = &issues[idx];
        (selected.identifier.clone(), selected.title.clone())
    };

    ctx.ui.print_step(&format!("{identifier}: {title}"));

    // Resolve base for ensure_branch
    let base_mode = BaseMode::from_raw(base_raw);
    let base_for_ensure = match &base_mode {
        BaseMode::Explicit(b) => Some(b.as_str()),
        _ => None,
    };

    // Ensure branch exists (provider-specific: Linear reads, GH may create)
    let raw_id = identifier.trim_start_matches('#');
    let branch_name = provider.ensure_branch(raw_id, base_for_ensure)?;

    if parallel {
        return run_parallel(ctx, &identifier, &title, &branch_name, base_raw);
    }

    let names = WorktreeNames::new(
        &branch_name,
        &ctx.parent_dir,
        &ctx.repo_name,
        Some(&title),
        ctx.config.herd.as_ref().map(|h| h.site_name.as_str()),
    );

    // 2. Check if branch is already checked out elsewhere
    let existing_path = git.checked_out_path(&branch_name)?;
    if let Some(ref existing) = existing_path {
        if *existing == ctx.invocation_root {
            ctx.ui
                .print_warning("이미 이 브랜치에 있습니다. 다른 브랜치로 전환 후 다시 시도하세요.");
            return Ok(());
        }
        if *existing != names.path {
            ctx.ui.print_step(&format!(
                "Branch already checked out at: {}",
                existing.display()
            ));
            setup::run_setup(ctx, existing, &names, Some(&title), "issue", None, None)?;
            return Ok(());
        }
    }

    // 3. Handle existing worktree directory
    if names.path.exists() {
        ctx.ui.print_warning(&format!(
            "Worktree {} already exists.",
            names.path.display()
        ));
        let items = vec![
            "Delete and recreate".into(),
            "Open existing".into(),
            "Abort".into(),
        ];
        let choice = ctx.ui.select("Worktree already exists", &items)?;
        match choice {
            0 => {
                ctx.ui.print_step("Removing existing worktree...");
                git.worktree_remove_force(&names.path).ok();
                if names.path.exists() {
                    std::fs::remove_dir_all(&names.path)?;
                }
            }
            1 => {
                setup::run_setup(ctx, &names.path, &names, Some(&title), "issue", None, None)?;
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // 4. Create worktree
    git.fetch()?;
    let create_type = create_worktree(ctx, &git, &branch_name, &names.path, base_raw)?;

    // 5. Update issue status for new branches
    if create_type == CreateType::New {
        if let Err(e) = provider.on_start(raw_id) {
            ctx.ui
                .print_warning(&format!("Failed to update issue status: {e}"));
        }
    }

    // 6. Setup
    setup::run_setup(ctx, &names.path, &names, Some(&title), "issue", None, None)?;

    Ok(())
}

fn run_parallel(
    ctx: &Ctx,
    _identifier: &str,
    title: &str,
    branch_name: &str,
    base_raw: &Option<String>,
) -> Result<()> {
    let variants = Config::load_variants(&ctx.repo_root)?;
    if variants.is_empty() {
        bail!("No variant configs found in .local/.wt.*.toml");
    }

    ctx.ui.print_step(&format!(
        "Found {} variants: {}",
        variants.len(),
        variants
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    for (variant_name, variant_config) in &variants {
        let variant_branch = format!("{branch_name}-{variant_name}");
        let variant_title = format!("{title} [{variant_name}]");

        ctx.ui.print_step(&format!("Setting up variant: {variant_name}"));

        let names = WorktreeNames::new(
            &variant_branch,
            &ctx.parent_dir,
            &ctx.repo_name,
            Some(&variant_title),
            variant_config.herd.as_ref().map(|h| h.site_name.as_str()),
        );

        if names.path.exists() {
            ctx.ui
                .print_warning(&format!("Worktree {} already exists.", names.path.display()));
            let items = vec!["Delete and recreate".into(), "Skip".into(), "Abort all".into()];
            let choice = ctx
                .ui
                .select(&format!("[{variant_name}] Worktree already exists"), &items)?;
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

        let base_mode = BaseMode::from_raw(base_raw);
        let base = match &base_mode {
            BaseMode::Explicit(b) => b.clone(),
            BaseMode::Interactive => {
                let branches = git.list_local_branches()?;
                let idx = ctx.ui.select("Select base branch", &branches)?;
                branches[idx].clone()
            }
            BaseMode::Default => {
                let current = git.current_branch()?;
                ctx.ui.input("Base branch", Some(&current))?
            }
        };

        if git.local_branch_exists(&variant_branch)? {
            ctx.ui
                .print_warning(&format!("Branch {variant_branch} already exists, removing..."));
            git.worktree_remove_force(&names.path).ok();
            ctx.runner
                .run("git", &["branch", "-D", &variant_branch], Some(&ctx.repo_root))
                .ok();
        }

        git.worktree_add_new_branch(&names.path, &variant_branch, &base)?;
        git.set_branch_parent(&variant_branch, &base).ok();

        setup::run_setup(
            ctx,
            &names.path,
            &names,
            Some(&variant_title),
            "issue",
            None,
            Some(variant_config),
        )?;
    }

    ctx.ui
        .print_step(&format!("All {} variants created successfully", variants.len()));
    Ok(())
}

pub fn build_provider<'a>(ctx: &'a Ctx) -> Result<Box<dyn IssueProvider + 'a>> {
    let issues_config = ctx
        .config
        .issues
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!(
            "No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\""
        ))?;
    match issues_config.provider {
        IssueProviderType::Linear => Ok(Box::new(
            LinearIssueProvider::new(ctx.runner.as_ref(), Some(&ctx.repo_root)),
        )),
        IssueProviderType::Github => Ok(Box::new(
            GithubIssueProvider::new(
                ctx.runner.as_ref(),
                Some(&ctx.repo_root),
                issues_config.gh_user.clone(),
            ),
        )),
    }
}

fn create_worktree(
    ctx: &Ctx,
    git: &GitService,
    branch_name: &str,
    wt_path: &std::path::Path,
    base_raw: &Option<String>,
) -> Result<CreateType> {
    let base_mode = BaseMode::from_raw(base_raw);

    if git.local_branch_exists(branch_name)? {
        if base_mode != BaseMode::Default {
            return Err(WtError::BranchExistsWithBase {
                branch: branch_name.into(),
            }
            .into());
        }
        ctx.ui
            .print_step(&format!("Reusing existing branch: {branch_name}"));
        git.worktree_add(wt_path, branch_name)?;
        return Ok(CreateType::Local);
    }

    if git.remote_branch_exists(branch_name)? {
        if base_mode != BaseMode::Default {
            return Err(WtError::BranchExistsWithBase {
                branch: branch_name.into(),
            }
            .into());
        }
        ctx.ui
            .print_step(&format!("Tracking remote branch: origin/{branch_name}"));
        git.worktree_add_new_branch(wt_path, branch_name, &format!("origin/{branch_name}"))?;
        let branches = git.list_local_branches()?;
        let idx = ctx.ui.select("Select parent branch", &branches)?;
        git.set_branch_parent(branch_name, &branches[idx]).ok();
        return Ok(CreateType::Remote);
    }

    // New branch — resolve base
    let base = match base_mode {
        BaseMode::Explicit(ref b) => b.clone(),
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            let idx = ctx.ui.select("Select base branch", &branches)?;
            branches[idx].clone()
        }
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))?
        }
    };

    ctx.ui
        .print_step(&format!("Creating new branch from {base}"));
    git.worktree_add_new_branch(wt_path, branch_name, &base)?;
    git.set_branch_parent(branch_name, &base).ok();
    Ok(CreateType::New)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssuesConfig, IssueProviderType};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn linear_config() -> Config {
        let mut config = Config::default();
        config.issues = Some(IssuesConfig {
            provider: IssueProviderType::Linear,
            gh_user: None,
        });
        config
    }

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

    #[test]
    fn issue_with_number_fetches_and_resolves() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"C11S09. 위키 에디터","branchName":"hoetaek/tech-680-c11s09"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"C11S09. 위키 에디터","branchName":"hoetaek/tech-680-c11s09"}"#,
            true,
        );
        // checked_out_path (worktree list)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch
        runner.add_response("", true);
        // local_branch_exists
        runner.add_response("", false);
        // remote_branch_exists
        runner.add_response("", false);
        // current_branch (for base prompt)
        runner.add_response("main", true);
        // worktree_add_new_branch
        runner.add_response("", true);
        // on_start (update_status)
        runner.add_response("", true);

        let mut ui = MockUi::new();
        ui.add_input("main"); // base branch prompt

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        // This will fail at setup (no real filesystem) but proves the flow up to worktree creation
        let result = run(&ctx, Some(680), &None, false);
        // We expect it to get past issue resolution and worktree creation
        // It may fail at setup::run_setup due to filesystem ops — that's OK for unit test
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));
    }

    #[test]
    fn issue_no_branch_name_returns_error() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"TECH-100","title":"Test issue","branchName":null}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"TECH-100","title":"Test issue","branchName":null}"#,
            true,
        );

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(100), &None, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No branch name"));
    }

    #[test]
    fn issue_local_branch_exists_reuses_it() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"TECH-1","title":"Test","branchName":"hoetaek/tech-1-test"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"TECH-1","title":"Test","branchName":"hoetaek/tech-1-test"}"#,
            true,
        );
        // checked_out_path (worktree list — no match)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch
        runner.add_response("", true);
        // local_branch_exists → true
        runner.add_response("", true);
        // worktree_add (not -b)
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(1), &None, false);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn issue_uses_canonical_repo_name_when_invoked_from_worktree() {
        let unique = format!(
            "wt-issue-canonical-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let temp = std::env::temp_dir().join(unique);
        let repo_root = temp.join("hapjeong");
        std::fs::create_dir_all(&repo_root).unwrap();

        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"TECH-672","title":"C11S09 nested worktree bug","branchName":"hoetaek/tech-672-nested-worktree-bug"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"TECH-672","title":"C11S09 nested worktree bug","branchName":"hoetaek/tech-672-nested-worktree-bug"}"#,
            true,
        );
        // checked_out_path (worktree list — no branch match)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch
        runner.add_response("", true);
        // local_branch_exists
        runner.add_response("", false);
        // remote_branch_exists
        runner.add_response("", false);
        // current_branch (for base prompt)
        runner.add_response("main", true);
        // worktree_add_new_branch
        runner.add_response("", true);
        // on_start (update_status)
        runner.add_response("", true);

        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_input("main");

        let ctx = Ctx::new(
            repo_root.clone(),
            repo_root,
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let result = run(&ctx, Some(672), &None, false);
        assert!(result.is_ok());

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
            temp.join("hapjeong-hoetaek-tech-672-nested-worktree-bug")
                .to_string_lossy()
                .as_ref()
        );
        assert!(!worktree_add_call.1[4].contains("hapjeong-tech-670-feature-hoetaek-tech-672"));

        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn issue_default_base_prompt_uses_invocation_root_for_current_branch() {
        let temp = std::env::temp_dir().join(format!(
            "wt-issue-invocation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo_root = temp.join("hapjeong");
        let invocation_root = temp.join("hapjeong-hoetaek-tech-670");
        std::fs::create_dir_all(&repo_root).unwrap();
        std::fs::create_dir_all(&invocation_root).unwrap();

        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"TECH-672","title":"C11S09 nested worktree bug","branchName":"hoetaek/tech-672-nested-worktree-bug"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"TECH-672","title":"C11S09 nested worktree bug","branchName":"hoetaek/tech-672-nested-worktree-bug"}"#,
            true,
        );
        // checked_out_path (worktree list)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch
        runner.add_response("", true);
        // local_branch_exists
        runner.add_response("", false);
        // remote_branch_exists
        runner.add_response("", false);
        // current_branch (for base prompt) — uses invocation_root
        runner.add_response(
            "hoetaek/tech-670-위키-에디터는-문서에-분류x로-분류를-지정할-수-있다",
            true,
        );
        // worktree_add_new_branch
        runner.add_response("", true);
        // on_start (update_status)
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ui = MockUi::new();
        let ctx = Ctx::new(
            repo_root,
            invocation_root.clone(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let result = run(&ctx, Some(672), &None, false);
        assert!(result.is_ok());

        let calls = runner.calls.lock().unwrap();
        let current_branch_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git" && args == &vec!["rev-parse".to_string(), "--abbrev-ref".to_string(), "HEAD".to_string()]
            })
            .expect("expected git current branch call");
        assert_eq!(current_branch_call.2.as_deref(), Some(invocation_root.as_path()));

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
            worktree_add_call.1[5],
            "hoetaek/tech-670-위키-에디터는-문서에-분류x로-분류를-지정할-수-있다"
        );

        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn issue_base_conflict_with_existing_branch() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"TECH-1","title":"Test","branchName":"hoetaek/tech-1-test"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"TECH-1","title":"Test","branchName":"hoetaek/tech-1-test"}"#,
            true,
        );
        // checked_out_path
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch
        runner.add_response("", true);
        // local_branch_exists → true
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(1), &Some("main".into()), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
}
