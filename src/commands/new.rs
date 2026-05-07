use crate::cli::BaseMode;
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
    base_raw: &Option<String>,
    parallel: bool,
) -> Result<()> {
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

    let branch_name = kebab;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    if parallel {
        let base_mode = BaseMode::from_raw(base_raw);
        let base = resolve_base_branch(ctx, &git, &base_mode)?;
        return run_parallel(ctx, &branch_name, &base);
    }

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
    git.set_branch_parent(&branch_name, &base).ok();

    setup::run_setup(ctx, &names.path, &names, None, "new", None, None)?;

    Ok(())
}

fn run_parallel(ctx: &Ctx, branch_name: &str, base: &str) -> Result<()> {
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

        ctx.ui
            .print_step(&format!("Setting up variant: {variant_name}"));

        let names = WorktreeNames::new(
            &variant_branch,
            &ctx.parent_dir,
            &ctx.repo_name,
            None,
            variant_config.herd.as_ref().map(|h| h.site_name.as_str()),
        );

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

        if git.local_branch_exists(&variant_branch)? {
            ctx.ui.print_warning(&format!(
                "Branch {variant_branch} already exists, removing..."
            ));
            git.worktree_remove_force(&names.path).ok();
            ctx.runner
                .run(
                    "git",
                    &["branch", "-D", &variant_branch],
                    Some(&ctx.repo_root),
                )
                .ok();
        }

        git.worktree_add_new_branch(&names.path, &variant_branch, base)?;
        git.set_branch_parent(&variant_branch, base).ok();

        setup::run_setup(
            ctx,
            &names.path,
            &names,
            None,
            "new",
            None,
            Some(variant_config),
        )?;
    }

    ctx.ui.print_step(&format!(
        "All {} variants created successfully",
        variants.len()
    ));
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

        let result = run(&ctx, &[], &None, false);
        assert!(result.is_err());
    }

    #[test]
    fn new_default_base_prompt_uses_invocation_root_for_current_branch() {
        let repo_root = PathBuf::from("/tmp/hapjeong");
        let invocation_root = PathBuf::from("/tmp/hapjeong-alice-tech-670");
        let mut runner = MockRunner::new();
        runner.add_response("alice/tech-670-current", true);
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
        let result = run(&ctx, &words, &None, false);
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
        assert_eq!(worktree_add_call.1[5], "alice/tech-670-current");
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
        let result = run(&ctx, &words, &None, false);
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
        let result = run(&ctx, &words, &Some("develop".into()), false);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
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
}
