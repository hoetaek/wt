use crate::config::Config;
use crate::context::{Ctx, PromptItem};
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::services::github::{GithubService, PullRequest};
use crate::setup;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;

pub fn run(ctx: &Ctx, numbers: &[u32], profile: Option<&str>) -> Result<()> {
    let github = GithubService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let profile_config = load_profile_config(ctx, profile)?;
    let config = profile_config.as_ref().unwrap_or(&ctx.config);

    if numbers.is_empty() {
        let prs = select_prs(ctx, &github)?;
        for pr in prs {
            run_resolved_pr(ctx, &git, &pr, profile, profile_config.as_ref(), config)
                .with_context(|| format!("PR #{}", pr.number))?;
        }
    } else {
        for number in numbers {
            let pr = github
                .get_pr(*number)
                .with_context(|| format!("PR #{number}"))?;
            run_resolved_pr(ctx, &git, &pr, profile, profile_config.as_ref(), config)
                .with_context(|| format!("PR #{}", pr.number))?;
        }
    }

    Ok(())
}

fn select_prs(ctx: &Ctx, github: &GithubService<'_>) -> Result<Vec<PullRequest>> {
    let prs = github.list_prs()?;
    if prs.is_empty() {
        bail!("No open PRs found");
    }

    let items = format_pr_select_prompt_items(&prs);
    let selected_indices = ctx.ui.multi_select_items("PRs to start", &items)?;
    if selected_indices.is_empty() {
        ctx.ui.print_warning("No PRs selected");
        return Ok(Vec::new());
    }

    let mut selected = Vec::new();
    for index in selected_indices {
        let pr = prs
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Invalid PR selection"))?;
        selected.push(pr.clone());
    }
    Ok(selected)
}

fn run_resolved_pr(
    ctx: &Ctx,
    git: &GitService<'_>,
    pr: &PullRequest,
    profile: Option<&str>,
    profile_config: Option<&Config>,
    config: &Config,
) -> Result<()> {
    let pr_number = pr.number;
    let title = pr.title.as_str();
    let branch_name = pr.head_ref_name.as_str();
    let base_branch = pr.base_ref_name.as_str();

    ctx.ui.print_step(&format_pr_summary(pr));

    let names = WorktreeNames::new_with_config(
        branch_name,
        &ctx.parent_dir,
        &ctx.repo_root,
        &ctx.repo_name,
        Some(title),
        config.has_site().then_some(""),
        config.worktree.path.as_deref(),
    )?;

    let mut extra_vars: HashMap<String, String> = [
        ("pr_number".into(), pr_number.to_string()),
        ("base_branch".into(), base_branch.to_string()),
    ]
    .into();
    if let Some(profile) = profile {
        extra_vars.insert("profile".into(), profile.to_string());
    }

    // Check if branch is already checked out
    let existing_path = git.checked_out_path(branch_name)?;
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
            git.set_branch_parent(branch_name, base_branch).ok();
            setup::run_setup(
                ctx,
                existing,
                &names,
                Some(title),
                "pr",
                Some(&extra_vars),
                profile_config,
            )?;
            return Ok(());
        }
    }

    // Handle existing worktree
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
                git.set_branch_parent(branch_name, base_branch).ok();
                setup::run_setup(
                    ctx,
                    &names.path,
                    &names,
                    Some(title),
                    "pr",
                    Some(&extra_vars),
                    profile_config,
                )?;
                return Ok(());
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // Fetch and create worktree from remote branch
    ctx.ui
        .print_step(&format!("Fetching and creating worktree for {branch_name}"));
    git.fetch_branch(branch_name)?;

    if git.local_branch_exists(branch_name)? {
        ctx.ui
            .print_step(&format!("Reusing existing local branch: {branch_name}"));
        git.worktree_add(&names.path, branch_name)?;
    } else {
        let remote_ref = format!("origin/{branch_name}");
        git.worktree_add_new_branch(&names.path, branch_name, &remote_ref)?;
        git.set_upstream(branch_name, &remote_ref, &names.path)?;
    }

    git.set_branch_parent(branch_name, base_branch).ok();

    // Setup
    setup::run_setup(
        ctx,
        &names.path,
        &names,
        Some(title),
        "pr",
        Some(&extra_vars),
        profile_config,
    )?;

    Ok(())
}

fn load_profile_config(ctx: &Ctx, profile: Option<&str>) -> Result<Option<Config>> {
    let Some(profile) = profile else {
        return Ok(None);
    };
    Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile}' not found"))
}

fn format_pr_summary(pr: &PullRequest) -> String {
    let number = pr_number_label(pr);
    let author = author_label(pr);
    format!("PR {number}  {author}  {} ({})", pr.title, pr.state)
}

#[cfg(test)]
fn format_pr_select_items(prs: &[PullRequest]) -> Vec<String> {
    format_pr_select_prompt_items(prs)
        .iter()
        .map(PromptItem::render_plain)
        .collect()
}

fn format_pr_select_prompt_items(prs: &[PullRequest]) -> Vec<PromptItem> {
    prs.iter().map(format_pr_select_item).collect()
}

fn format_pr_select_item(pr: &PullRequest) -> PromptItem {
    let number = pr_number_label(pr);
    let author = author_label(pr);
    PromptItem::from_hint_parts(
        pr.title.clone(),
        vec![
            format!("PR {number}"),
            author,
            pr.state.clone(),
            format!("head {}", pr.head_ref_name),
            format!("base {}", pr.base_ref_name),
        ],
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

    fn pr_json(number: u32, title: &str, branch: &str) -> String {
        format!(
            r#"{{"number":{number},"title":"{title}","headRefName":"{branch}","baseRefName":"main","state":"OPEN","author":{{"login":"alice"}}}}"#
        )
    }

    fn queue_remote_worktree_creation(runner: &mut MockRunner) {
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // fetch origin branch
        runner.add_response("", false); // local branch does not exist
        runner.add_response("", true); // git worktree add -b
        runner.add_response("", true); // git branch --set-upstream-to
        runner.add_response("", true); // parent branch exists locally
        runner.add_response("", true); // git config branch.<name>.parentbranch
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
        // local_branch_exists
        runner.add_response("", false);
        // worktree_add_new_branch
        runner.add_response("", true);
        // set upstream
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

        let result = run(&ctx, &[42], None);
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "config".to_string(),
                        "branch.alice/feature.parentbranch".to_string(),
                        "main".to_string(),
                    ]
        }));
    }

    #[test]
    fn pr_with_multiple_numbers_starts_each_worktree() {
        let mut runner = MockRunner::new();
        runner.add_response(&pr_json(42, "First PR", "alice/first"), true);
        queue_remote_worktree_creation(&mut runner);
        runner.add_response(&pr_json(43, "Second PR", "alice/second"), true);
        queue_remote_worktree_creation(&mut runner);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, &[42, 43], None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let viewed_prs = calls
            .iter()
            .filter(|(cmd, args, _)| cmd == "gh" && args.starts_with(&["pr".into(), "view".into()]))
            .map(|(_, args, _)| args[2].clone())
            .collect::<Vec<_>>();
        assert_eq!(viewed_prs, vec!["42", "43"]);

        let created_branches = calls
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
        assert_eq!(created_branches, vec!["alice/first", "alice/second"]);
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

        run(&ctx, &[42], Some("codex-yolo")).unwrap();

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
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "config".to_string(),
                        "branch.alice/feature.parentbranch".to_string(),
                        "main".to_string(),
                    ]
        }));
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

        let result = run(&ctx, &[], None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No open PRs"));
    }

    #[test]
    fn pr_without_numbers_multi_selects_prs() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[
                {"number":42,"title":"First PR","headRefName":"alice/first","baseRefName":"main","state":"OPEN","author":{"login":"alice"}},
                {"number":43,"title":"Skipped PR","headRefName":"alice/skipped","baseRefName":"main","state":"OPEN","author":{"login":"alice"}},
                {"number":44,"title":"Third PR","headRefName":"alice/third","baseRefName":"main","state":"OPEN","author":{"login":"alice"}}
            ]"#,
            true,
        );
        queue_remote_worktree_creation(&mut runner);
        queue_remote_worktree_creation(&mut runner);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 2]);
        let ui = Arc::new(ui);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(Arc::clone(&ui)),
        );

        run(&ctx, &[], None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let created_branches = calls
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
        assert_eq!(created_branches, vec!["alice/first", "alice/third"]);
        assert!(ui.warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn pr_without_numbers_empty_selection_returns_ok() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"number":42,"title":"First PR","headRefName":"alice/first","baseRefName":"main","state":"OPEN","author":{"login":"alice"}}]"#,
            true,
        );
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![]);
        let ui = Arc::new(ui);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(Arc::clone(&ui)),
        );

        run(&ctx, &[], None).unwrap();

        assert_eq!(ui.warnings.lock().unwrap().as_slice(), ["No PRs selected"]);
        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git" && args.len() >= 2 && args[0] == "worktree" && args[1] == "add")
        }));
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

        let result = run(&ctx, &[10], None);
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
            vec!["Add feature  PR #42 | @alice | OPEN | head alice/feature | base main"]
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
            vec!["Add feature  PR #42 | unknown author | OPEN | head alice/feature | base main"]
        );
    }

    #[test]
    fn formats_pr_select_items_with_title_labels_and_metadata_hints() {
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
                "Short author  PR #9 | @a | OPEN | head alice/short | base main",
                "Long author  PR #123 | @octocat | OPEN | head octocat/long | base main"
            ]
        );
    }
}
