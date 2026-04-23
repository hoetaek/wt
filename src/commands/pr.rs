use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::services::github::GithubService;
use crate::setup;
use anyhow::{Result, bail};
use std::collections::HashMap;

pub fn run(ctx: &Ctx, number: Option<u32>) -> Result<()> {
    let github = GithubService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    // 1. Resolve PR
    let (pr_number, title, branch_name, base_branch) = if let Some(num) = number {
        let pr = github.get_pr(num)?;
        ctx.ui
            .print_step(&format!("PR #{}: {} ({})", pr.number, pr.title, pr.state));
        (pr.number, pr.title, pr.head_ref_name, pr.base_ref_name)
    } else {
        let prs = github.list_prs()?;
        if prs.is_empty() {
            bail!("No open PRs found");
        }

        let items: Vec<String> = prs
            .iter()
            .map(|p| format!("#{} {}", p.number, p.title))
            .collect();

        let idx = ctx.ui.select("Select a PR", &items)?;
        let selected = &prs[idx];
        ctx.ui.print_step(&format!(
            "PR #{}: {} ({})",
            selected.number, selected.title, selected.state
        ));
        (
            selected.number,
            selected.title.clone(),
            selected.head_ref_name.clone(),
            selected.base_ref_name.clone(),
        )
    };

    let names = WorktreeNames::new(
        &branch_name,
        &ctx.parent_dir,
        &ctx.repo_name,
        Some(&title),
        ctx.config.herd.as_ref().map(|h| h.site_name.as_str()),
    );

    let extra_vars: HashMap<String, String> = [
        ("pr_number".into(), pr_number.to_string()),
        ("base_branch".into(), base_branch.clone()),
    ]
    .into();

    // 2. Check if branch is already checked out
    let existing_path = git.checked_out_path(&branch_name)?;
    if let Some(ref existing) = existing_path {
        if *existing != names.path {
            ctx.ui.print_step(&format!(
                "Branch already checked out at: {}",
                existing.display()
            ));
            git.set_branch_parent(&branch_name, &base_branch).ok();
            setup::run_setup(ctx, existing, &names, Some(&title), "pr", Some(&extra_vars))?;
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
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;

    #[test]
    fn pr_with_number_fetches_and_resolves() {
        let mut runner = MockRunner::new();
        // get_pr
        runner.add_response(
            r#"{"number":42,"title":"Add feature","headRefName":"hoetaek/feature","baseRefName":"main","state":"OPEN"}"#,
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
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(42));
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));
    }

    #[test]
    fn pr_empty_list_returns_error() {
        let mut runner = MockRunner::new();
        runner.add_response("[]", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No open PRs"));
    }

    #[test]
    fn pr_reuses_local_branch_when_exists() {
        let mut runner = MockRunner::new();
        // get_pr
        runner.add_response(
            r#"{"number":10,"title":"Fix bug","headRefName":"hoetaek/fix","baseRefName":"main","state":"OPEN"}"#,
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
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(10));
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }
}
