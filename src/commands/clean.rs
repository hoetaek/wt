use crate::commands::issue::build_provider;
use crate::commands::profile_match;
use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::{CmuxService, CmuxWorkspace};
use crate::services::git::GitService;
use crate::services::site::{SiteService, provider_label};
use crate::setup;
use anyhow::{Result, bail};
use std::path::Path;

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

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let mut cmux_workspaces = load_cmux_workspaces(ctx, &cmux);

    for &idx in &selected {
        let entry = &additional[idx];
        let wt_path = &entry.path;
        let branch = &entry.branch;
        let profile_config = profile_match::load_profile_config_for_branch(ctx, branch)?;
        let config = profile_config.as_ref().unwrap_or(&ctx.config);

        close_matching_cmux_workspaces(ctx, &cmux, &mut cmux_workspaces, entry);

        // Site unlink
        unlink_site(ctx, config, wt_path, branch)?;

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

fn unlink_site(ctx: &Ctx, config: &Config, wt_path: &Path, branch: &str) -> Result<()> {
    let Some(site_config) = config.effective_site() else {
        return Ok(());
    };
    let site = SiteService::new(ctx.runner.as_ref());
    if !site.is_available(&site_config.provider) {
        return Ok(());
    }

    let names = WorktreeNames::new(branch, &ctx.parent_dir, &ctx.repo_name, None, Some(""));
    let mut vars = setup::build_template_vars(ctx, wt_path, &names, None);
    let Some(site_descriptor) = setup::apply_site_template_vars(config, &mut vars) else {
        return Ok(());
    };

    if site.unregister(&site_config.provider, &site_descriptor.name)? {
        ctx.ui.print_step(&format!(
            "  {}: {} unlinked",
            provider_label(&site_config.provider),
            site_descriptor.url
        ));
    }
    Ok(())
}

fn load_cmux_workspaces(ctx: &Ctx, cmux: &CmuxService<'_>) -> Vec<CmuxWorkspace> {
    if !cmux.is_available() {
        return Vec::new();
    }

    match cmux.list_workspaces() {
        Ok(workspaces) => workspaces,
        Err(e) => {
            if ctx.config.workspace.is_some() {
                ctx.ui
                    .print_warning(&format!("  cmux workspace lookup: {e}"));
            }
            Vec::new()
        }
    }
}

fn close_matching_cmux_workspaces(
    ctx: &Ctx,
    cmux: &CmuxService<'_>,
    cmux_workspaces: &mut Vec<CmuxWorkspace>,
    entry: &crate::services::git::WorktreeEntry,
) {
    if cmux_workspaces.is_empty() {
        return;
    }

    close_workspaces_at_path(ctx, cmux, cmux_workspaces, &entry.path);
}

fn close_workspaces_at_path(
    ctx: &Ctx,
    cmux: &CmuxService<'_>,
    cmux_workspaces: &mut Vec<CmuxWorkspace>,
    worktree_path: &Path,
) -> usize {
    let mut closed = 0;
    let mut idx = 0;

    while idx < cmux_workspaces.len() {
        if cmux_workspaces[idx]
            .current_directory
            .as_deref()
            .is_some_and(|cwd| same_path(cwd, worktree_path))
        {
            let workspace = cmux_workspaces.remove(idx);
            match cmux.close_workspace(&workspace.handle) {
                Ok(()) => {
                    closed += 1;
                    ctx.ui
                        .print_step(&format!("  cmux workspace closed: {}", workspace.title));
                }
                Err(e) => ctx.ui.print_warning(&format!(
                    "  Failed to close cmux workspace {}: {e}",
                    workspace.title
                )),
            }
        } else {
            idx += 1;
        }
    }

    closed
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
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
        || branch_issue_matches(&entry.branch, target)
        || entry.path.to_string_lossy() == target
        || entry.path.file_name().and_then(|name| name.to_str()) == Some(target)
}

fn branch_issue_matches(branch: &str, target: &str) -> bool {
    let target = target.trim_start_matches('#').to_ascii_lowercase();
    if target.is_empty() {
        return false;
    }

    let short = branch
        .rsplit('/')
        .next()
        .unwrap_or(branch)
        .to_ascii_lowercase();
    if short == target || short.starts_with(&format!("{target}-")) {
        return true;
    }

    short.contains(&format!("-{target}-")) || short.ends_with(&format!("-{target}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, WorkspaceConfig};
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner};
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

    #[test]
    fn clean_closes_matching_cmux_workspace() {
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/test-repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );
        runner.add_response(r#"{"windows":[{"ref":"window:1"}]}"#, true);
        runner.add_response(
            r#"{"workspaces":[{"ref":"workspace:1","title":"feature","current_directory":"/tmp/other-repo-feature"},{"ref":"workspace:2","title":"custom title","current_directory":"/tmp/test-repo-feature"}]}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config {
                workspace: Some(WorkspaceConfig::default()),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "cmux"
                && args
                    == &vec![
                        "--window".to_string(),
                        "window:1".to_string(),
                        "rpc".to_string(),
                        "workspace.list".to_string(),
                        "{}".to_string(),
                    ]
        }));
        let close_idx = calls
            .iter()
            .position(|(cmd, args, _)| {
                cmd == "cmux" && args.first().is_some_and(|arg| arg == "close-workspace")
            })
            .expect("expected cmux close-workspace call");
        let remove_idx = calls
            .iter()
            .position(|(cmd, args, _)| {
                cmd == "git"
                    && args.first().is_some_and(|arg| arg == "worktree")
                    && args.get(1).is_some_and(|arg| arg == "remove")
            })
            .expect("expected git worktree remove call");

        assert!(close_idx < remove_idx);
        assert_eq!(
            calls[close_idx].1,
            vec!["close-workspace", "--workspace", "workspace:2"]
        );
    }

    #[test]
    fn clean_does_not_close_same_title_workspace_for_different_path() {
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/test-repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );
        runner.add_response(r#"{"windows":[{"ref":"window:1"}]}"#, true);
        runner.add_response(
            r#"{"workspaces":[{"ref":"workspace:2","title":"feature","current_directory":"/tmp/other-repo-feature"}]}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            Config {
                workspace: Some(WorkspaceConfig::default()),
                ..Config::default()
            },
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|(cmd, args, _)| {
            cmd == "cmux" && args.first().is_some_and(|arg| arg == "close-workspace")
        }));
    }

    #[test]
    fn clean_uses_matching_profile_config_for_site_unlink() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = repo.path().with_file_name("repo-cms-codex");
        let profile_dir = repo.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
[site]
provider = "herd"
name = "profile-{{branch_slug}}"
"#,
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("herd");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/feature/cms-codex\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("", true); // herd unlink
        runner.add_response("", true); // worktree remove
        runner.add_response("", true); // branch delete
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );
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

        run_with_targets(&ctx, &["cms-codex".into()]).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "herd" && args == &vec!["unlink".to_string(), "profile-cms-codex".to_string()]
        }));
    }

    #[test]
    fn worktree_matches_issue_number_or_key() {
        let entry = crate::services::git::WorktreeEntry {
            path: "/tmp/sample-app-proj-123-fix-editor".into(),
            branch: "alice/proj-123-fix-editor".into(),
        };

        assert!(worktree_matches(&entry, "123"));
        assert!(worktree_matches(&entry, "PROJ-123"));
        assert!(!worktree_matches(&entry, "12"));
    }
}
