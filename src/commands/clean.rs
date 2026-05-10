use crate::commands::issue::build_provider;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::services::site::{SiteService, provider_label};
use crate::setup;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx) -> Result<()> {
    run_with_targets(ctx, &[])
}

pub fn run_with_targets(ctx: &Ctx, targets: &[String]) -> Result<()> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    let entries = git.worktree_list()?;
    let additional: Vec<_> = entries
        .into_iter()
        .filter(|e| e.path != ctx.repo_root)
        .collect();

    if additional.is_empty() {
        ctx.ui.print_warning("No additional worktrees found");
        return Ok(());
    }

    let selected = if targets.is_empty() {
        let items: Vec<String> = additional.iter().map(|e| e.branch.clone()).collect();
        ctx.ui
            .multi_select("Select worktrees to remove (Space to select)", &items)?
    } else {
        resolve_targets(&additional, targets)?
    };
    if selected.is_empty() {
        ctx.ui.print_warning("No worktrees selected");
        return Ok(());
    }

    ctx.ui
        .print_step(&format!("Removing {} worktree(s)...", selected.len()));

    let site_config = ctx.config.effective_site();
    let site = site_config
        .as_ref()
        .map(|_| SiteService::new(ctx.runner.as_ref()));
    let site_available = site
        .as_ref()
        .zip(site_config.as_ref())
        .is_some_and(|(svc, config)| svc.is_available(&config.provider));

    for &idx in &selected {
        let entry = &additional[idx];
        let wt_path = &entry.path;
        let branch = &entry.branch;

        // Site unlink
        if site_available {
            let site = site.as_ref().unwrap();
            let site_config = site_config.as_ref().unwrap();
            let names = WorktreeNames::new(branch, &ctx.parent_dir, &ctx.repo_name, None, Some(""));
            let mut vars = setup::build_template_vars(ctx, &names, None);
            let site_descriptor = setup::apply_site_template_vars(&ctx.config, &mut vars);
            if let Some(site_descriptor) = site_descriptor {
                if site.unregister(&site_config.provider, &site_descriptor.name)? {
                    ctx.ui.print_step(&format!(
                        "  {}: {} unlinked",
                        provider_label(&site_config.provider),
                        site_descriptor.url
                    ));
                }
            }
        }

        // Issue provider cleanup hook
        if let Ok(provider) = build_provider(ctx) {
            if let Err(e) = provider.on_clean(&entry.branch, &entry.branch) {
                ctx.ui.print_warning(&format!("  Issue cleanup: {e}"));
            }
        }

        // Remove worktree
        let remove_result = git.worktree_remove(wt_path)?;
        if remove_result.success {
            ctx.ui
                .print_step(&format!("  Removed: {}", wt_path.display()));
        } else {
            ctx.ui
                .print_warning(&format!("  Failed to remove {}", wt_path.display()));
            if !remove_result.stderr.is_empty() {
                ctx.ui.print_warning(&format!("  {}", remove_result.stderr));
            }
            git.worktree_remove_force(wt_path).ok();
            ctx.ui.print_step("  Force removed");
        }

        // Clean up leftover directory
        if wt_path.exists() {
            std::fs::remove_dir_all(wt_path).ok();
        }

        // Delete local branch
        let del_result = git.branch_delete(branch)?;
        if del_result.success {
            ctx.ui.print_step("  Branch deleted");
        } else {
            ctx.ui
                .print_warning("  Not fully merged, force deleting...");
            git.branch_delete_force(branch)?;
            ctx.ui.print_step("  Branch force deleted");
        }
    }

    // Summary
    let remaining = git.worktree_list()?;
    let remaining_count = remaining.iter().filter(|e| e.path != ctx.repo_root).count();
    if remaining_count == 0 {
        ctx.ui.print_step("All additional worktrees removed");
    } else {
        ctx.ui
            .print_step(&format!("{remaining_count} worktree(s) remaining"));
    }

    Ok(())
}

fn resolve_targets(
    entries: &[crate::services::git::WorktreeEntry],
    targets: &[String],
) -> Result<Vec<usize>> {
    let mut selected = Vec::new();
    for target in targets {
        let matches = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| worktree_matches(entry, target))
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [idx] => {
                if !selected.contains(idx) {
                    selected.push(*idx);
                }
            }
            [] => bail!("No worktree matches {target:?}"),
            _ => bail!("Multiple worktrees match {target:?}"),
        }
    }
    Ok(selected)
}

fn worktree_matches(entry: &crate::services::git::WorktreeEntry, target: &str) -> bool {
    entry.branch == target
        || entry.branch.rsplit('/').next() == Some(target)
        || entry.path.to_string_lossy() == target
        || entry.path.file_name().and_then(|name| name.to_str()) == Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;

    #[test]
    fn clean_with_no_additional_worktrees_returns_ok() {
        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn clean_with_empty_selection_returns_ok() {
        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/test-repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![]);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        assert!(run(&ctx).is_ok());
    }

    #[test]
    fn clean_removes_worktree_and_deletes_branch() {
        let mut runner = MockRunner::new();
        // worktree list
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/test-repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );
        // worktree remove (success)
        runner.add_response("", true);
        // branch delete (success)
        runner.add_response("", true);
        // worktree list for summary
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        assert!(run(&ctx).is_ok());
    }
}
