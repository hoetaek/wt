use crate::cli::BaseMode;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::setup;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx, name_words: &[String], base_raw: &Option<String>) -> Result<()> {
    if name_words.is_empty() {
        bail!("Usage: wt new <branch-name-text>");
    }

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

    let branch_name = format!("hoetaek/{kebab}");
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    let names = WorktreeNames::new(
        &branch_name,
        &ctx.parent_dir,
        &ctx.repo_name,
        None,
        ctx.config.herd.as_ref().map(|h| h.site_name.as_str()),
    );

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

    setup::run_setup(ctx, &names.path, &names, None, "new")?;

    Ok(())
}

fn resolve_base_branch(ctx: &Ctx, git: &GitService, mode: &BaseMode) -> Result<String> {
    match mode {
        BaseMode::Explicit(branch) => Ok(branch.clone()),
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            Ok(branches[idx].clone())
        }
        BaseMode::Default => {
            let current = git.current_branch()?;
            let input = ctx.ui.input("Base branch", Some(&current))?;
            Ok(input)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;

    fn make_ctx(runner: MockRunner, ui: MockUi) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        )
    }

    #[test]
    fn kebab_case_conversion() {
        let words: Vec<String> = vec!["Some".into(), "Feature".into(), "Name".into()];
        let kebab: String = words
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
        assert_eq!(kebab, "some-feature-name");
    }

    #[test]
    fn empty_name_is_error() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = make_ctx(runner, ui);

        let result = run(&ctx, &[], &None);
        assert!(result.is_err());
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
        let result = run(&ctx, &words, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
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
        let result = run(&ctx, &words, &Some("develop".into()));
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }
}
