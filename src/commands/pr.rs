use crate::config::Config;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::services::github::{GithubService, PullRequest};
use crate::setup;
use anyhow::{Result, bail};
use std::collections::HashMap;

pub fn run(ctx: &Ctx, number: Option<u32>, profile: Option<&str>) -> Result<()> {
    let github = GithubService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let profile_config = load_profile_config(ctx, profile)?;
    let config = profile_config.as_ref().unwrap_or(&ctx.config);

    // 1. Resolve PR
    let (pr_number, title, branch_name, base_branch) = if let Some(num) = number {
        let pr = github.get_pr(num)?;
        ctx.ui.print_step(&format_pr_summary(&pr));
        (pr.number, pr.title, pr.head_ref_name, pr.base_ref_name)
    } else {
        let prs = github.list_prs()?;
        if prs.is_empty() {
            bail!("No open PRs found");
        }

        let items = format_pr_select_items(&prs);

        let idx = ctx.ui.select("Select a PR", &items)?;
        let selected = &prs[idx];
        ctx.ui.print_step(&format_pr_summary(selected));
        (
            selected.number,
            selected.title.clone(),
            selected.head_ref_name.clone(),
            selected.base_ref_name.clone(),
        )
    };

    let names = WorktreeNames::new_with_config(
        &branch_name,
        &ctx.parent_dir,
        &ctx.repo_root,
        &ctx.repo_name,
        Some(&title),
        config.has_site().then_some(""),
        config.worktree.path.as_deref(),
    )?;

    let mut extra_vars: HashMap<String, String> = [
        ("pr_number".into(), pr_number.to_string()),
        ("base_branch".into(), base_branch.clone()),
    ]
    .into();
    if let Some(profile) = profile {
        extra_vars.insert("profile".into(), profile.to_string());
    }

    // 2. Check if branch is already checked out
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
            git.set_branch_parent(&branch_name, &base_branch).ok();
            setup::run_setup(
                ctx,
                existing,
                &names,
                Some(&title),
                "pr",
                Some(&extra_vars),
                profile_config.as_ref(),
            )?;
            return Ok(());
        }
    }

    // 3. Handle existing worktree
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
                git.set_branch_parent(&branch_name, &base_branch).ok();
                setup::run_setup(
                    ctx,
                    &names.path,
                    &names,
                    Some(&title),
                    "pr",
                    Some(&extra_vars),
                    profile_config.as_ref(),
                )?;
                return Ok(());
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // 4. Fetch and create worktree from remote branch
    ctx.ui
        .print_step(&format!("Fetching and creating worktree for {branch_name}"));
    git.fetch_branch(&branch_name)?;

    if git.local_branch_exists(&branch_name)? {
        ctx.ui
            .print_step(&format!("Reusing existing local branch: {branch_name}"));
        git.worktree_add(&names.path, &branch_name)?;
    } else {
        let remote_ref = format!("origin/{branch_name}");
        git.worktree_add_new_branch(&names.path, &branch_name, &remote_ref)?;
        git.set_upstream(&branch_name, &remote_ref, &names.path)?;
    }

    git.set_branch_parent(&branch_name, &base_branch).ok();

    // 5. Setup
    setup::run_setup(
        ctx,
        &names.path,
        &names,
        Some(&title),
        "pr",
        Some(&extra_vars),
        profile_config.as_ref(),
    )?;

    Ok(())
}

fn load_profile_config(ctx: &Ctx, profile: Option<&str>) -> Result<Option<Config>> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    if profile == "default" {
        return Ok(Some(ctx.config.clone()));
    }
    Config::load_profile(&ctx.repo_root, profile, &ctx.config)?
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile}' not found"))
}

fn format_pr_summary(pr: &PullRequest) -> String {
    let number = pr_number_label(pr);
    let author = author_label(pr);
    format!("PR {number}  {author}  {} ({})", pr.title, pr.state)
}

fn format_pr_select_items(prs: &[PullRequest]) -> Vec<String> {
    let number_width = prs
        .iter()
        .map(|pr| pr_number_label(pr).len())
        .max()
        .unwrap_or(0);
    let author_width = prs
        .iter()
        .map(|pr| author_label(pr).len())
        .max()
        .unwrap_or(0);

    prs.iter()
        .map(|pr| format_pr_select_item(pr, number_width, author_width))
        .collect()
}

fn format_pr_select_item(pr: &PullRequest, number_width: usize, author_width: usize) -> String {
    let number = pr_number_label(pr);
    let author = author_label(pr);
    format!(
        "{number:<number_width$}  {author:<author_width$}  {}",
        pr.title
    )
}

fn pr_number_label(pr: &PullRequest) -> String {
    format!("#{}", pr.number)
}

fn author_label(pr: &PullRequest) -> String {
    pr.author_login()
        .map(|login| format!("@{login}"))
        .unwrap_or_else(|| "unknown author".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx};
    use crate::services::github::PullRequestAuthor;
    use anyhow::Result;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    #[test]
    fn pr_with_number_fetches_and_resolves() {
        let mut runner = MockRunner::new();
        // get_pr
        runner.add_response(
            r#"{"number":42,"title":"Add feature","headRefName":"alice/feature","baseRefName":"main","state":"OPEN","author":{"login":"alice"}}"#,
            true,
        );
        // checked_out_path (worktree list)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch origin branch
        runner.add_response("", true);
        // worktree_add_new_branch
        runner.add_response("", true);
        // set upstream
        runner.add_response("", true);
        // set_branch_parent
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(42), None);
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));
    }

    #[test]
    fn pr_with_profile_uses_profile_config_for_setup() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".local/profiles/codex-yolo");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[worktree]
path = "profile-worktrees/{{default_name}}"

[agent]
cli = "codex"
"#,
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"number":42,"title":"Add feature","headRefName":"alice/feature","baseRefName":"main","state":"OPEN","author":{"login":"alice"}}"#,
            true,
        );
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
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

        run(&ctx, Some(42), Some("codex-yolo")).unwrap();

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
        assert!(worktree_add_call.1[4].contains("profile-worktrees/"));
        assert!(worktree_add_call.1[4].ends_with("-alice-feature"));
    }

    #[test]
    fn pr_empty_list_returns_error() {
        let mut runner = MockRunner::new();
        runner.add_response("[]", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No open PRs"));
    }

    #[test]
    fn pr_reuses_local_branch_when_exists() {
        let mut runner = MockRunner::new();
        // get_pr
        runner.add_response(
            r#"{"number":10,"title":"Fix bug","headRefName":"alice/fix","baseRefName":"main","state":"OPEN","author":{"login":"alice"}}"#,
            true,
        );
        // checked_out_path (no match)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // fetch_branch
        runner.add_response("", true);
        // local_branch_exists → true
        runner.add_response("", true);
        // worktree_add (reuse local)
        runner.add_response("", true);
        // set_branch_parent
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(10), None);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn formats_pr_summary_with_author() {
        let pr = PullRequest {
            number: 42,
            title: "Add feature".into(),
            head_ref_name: "alice/feature".into(),
            base_ref_name: "main".into(),
            state: "OPEN".into(),
            author: Some(PullRequestAuthor {
                login: "alice".into(),
            }),
        };

        assert_eq!(format_pr_summary(&pr), "PR #42  @alice  Add feature (OPEN)");
        assert_eq!(
            format_pr_select_items(&[pr]),
            vec!["#42  @alice  Add feature"]
        );
    }

    #[test]
    fn formats_pr_summary_without_author() {
        let pr = PullRequest {
            number: 42,
            title: "Add feature".into(),
            head_ref_name: "alice/feature".into(),
            base_ref_name: "main".into(),
            state: "OPEN".into(),
            author: None,
        };

        assert_eq!(
            format_pr_summary(&pr),
            "PR #42  unknown author  Add feature (OPEN)"
        );
        assert_eq!(
            format_pr_select_items(&[pr]),
            vec!["#42  unknown author  Add feature"]
        );
    }

    #[test]
    fn formats_pr_select_items_with_aligned_author_column() {
        let prs = vec![
            PullRequest {
                number: 9,
                title: "Short author".into(),
                head_ref_name: "alice/short".into(),
                base_ref_name: "main".into(),
                state: "OPEN".into(),
                author: Some(PullRequestAuthor { login: "a".into() }),
            },
            PullRequest {
                number: 123,
                title: "Long author".into(),
                head_ref_name: "octocat/long".into(),
                base_ref_name: "main".into(),
                state: "OPEN".into(),
                author: Some(PullRequestAuthor {
                    login: "octocat".into(),
                }),
            },
        ];

        assert_eq!(
            format_pr_select_items(&prs),
            vec![
                "#9    @a        Short author",
                "#123  @octocat  Long author"
            ]
        );
    }
}
