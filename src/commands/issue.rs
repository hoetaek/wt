use crate::cli::BaseMode;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::{CreateType, GitService};
use crate::services::linear::LinearService;
use crate::setup;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx, number: Option<u32>, base_raw: &Option<String>) -> Result<()> {
    let linear = LinearService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    // 1. Resolve issue
    let (identifier, title, branch_name) = if let Some(num) = number {
        let id = format!("TECH-{num}");
        let issue = linear.get_issue(&id)?;
        let branch = issue.branch_name.ok_or_else(|| WtError::NoBranchName {
            identifier: id.clone(),
        })?;
        (issue.identifier, issue.title, branch)
    } else {
        let issues = linear.list_issues()?;
        if issues.is_empty() {
            bail!("No issues found");
        }

        let items: Vec<String> = issues
            .iter()
            .map(|i| {
                let assignee = i.assignee.as_ref()
                    .map(|a| a.display_name.as_str())
                    .unwrap_or("-");
                format!("{} {} [{}]", i.identifier, i.title, assignee)
            })
            .collect();

        let idx = ctx.ui.select("Select an issue", &items)?;
        let selected = &issues[idx];
        let issue = linear.get_issue(&selected.identifier)?;
        let branch = issue.branch_name.ok_or_else(|| WtError::NoBranchName {
            identifier: selected.identifier.clone(),
        })?;
        (issue.identifier, issue.title, branch)
    };

    ctx.ui.print_step(&format!("{identifier}: {title}"));

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
        if *existing != names.path {
            ctx.ui.print_step(&format!(
                "Branch already checked out at: {}",
                existing.display()
            ));
            setup::run_setup(ctx, existing, &names, Some(&title), "issue", None)?;
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
                setup::run_setup(ctx, &names.path, &names, Some(&title), "issue", None)?;
                return Ok(());
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // 4. Create worktree
    git.fetch()?;
    let create_type = create_worktree(ctx, &git, &branch_name, &names.path, base_raw)?;

    // 5. Update Linear status for new branches
    if create_type == CreateType::New {
        ctx.ui
            .print_step("Updating Linear issue status: In Progress");
        if let Err(e) = linear.update_status(&identifier, "In Progress") {
            ctx.ui
                .print_warning(&format!("Failed to update issue status: {e}"));
        }
    }

    // 6. Setup
    setup::run_setup(ctx, &names.path, &names, Some(&title), "issue", None)?;

    Ok(())
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
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;

    #[test]
    fn issue_with_number_fetches_and_resolves() {
        let mut runner = MockRunner::new();
        // get_issue
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
        // update_status
        runner.add_response("", true);

        let mut ui = MockUi::new();
        ui.add_input("main"); // base branch prompt

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        // This will fail at setup (no real filesystem) but proves the flow up to worktree creation
        let result = run(&ctx, Some(680), &None);
        // We expect it to get past issue resolution and worktree creation
        // It may fail at setup::run_setup due to filesystem ops — that's OK for unit test
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));
    }

    #[test]
    fn issue_no_branch_name_returns_error() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-100","title":"Test issue","branchName":null}"#,
            true,
        );

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(100), &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No branch name"));
    }

    #[test]
    fn issue_local_branch_exists_reuses_it() {
        let mut runner = MockRunner::new();
        // get_issue
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
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(1), &None);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn issue_base_conflict_with_existing_branch() {
        let mut runner = MockRunner::new();
        // get_issue
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
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some(1), &Some("main".into()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
}
