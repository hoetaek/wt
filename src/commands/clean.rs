use crate::commands::issue::build_provider;
use crate::commands::profile_match;
use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::{CmuxService, CmuxWorkspace};
use crate::services::git::GitService;
use crate::services::site::{SiteService, provider_label};
use crate::setup;
use crate::task_run::{self, TaskRunContext, TaskRunRecord};
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
        validate_task_run_targets_without_worktrees(ctx, targets)?;
        ctx.ui.print_warning("No additional worktrees found");
        return Ok(());
    }

    let selected = if targets.is_empty() {
        let items: Vec<String> = additional.iter().map(|e| e.branch.clone()).collect();
        ctx.ui
            .multi_select("Select worktrees to remove (Space to select)", &items)?
    } else {
        resolve_targets(ctx, &additional, targets)?
    };
    if selected.is_empty() {
        ctx.ui.print_warning("No worktrees selected");
        return Ok(());
    }

    ctx.ui
        .print_step(&format!("Removing {} worktree(s)...", selected.len()));

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let mut cmux_workspaces = load_cmux_workspaces(ctx, &cmux);
    let mut closed_cmux_workspace_ids = Vec::new();

    for &idx in &selected {
        let entry = &additional[idx];
        let wt_path = &entry.path;
        let branch = &entry.branch;
        let profile_config = profile_match::load_profile_config_for_branch(ctx, branch)?;
        let config = profile_config.as_ref().unwrap_or(&ctx.config);

        closed_cmux_workspace_ids.extend(close_matching_cmux_workspaces(
            ctx,
            &cmux,
            &mut cmux_workspaces,
            entry,
        ));

        // Site unlink
        unlink_site(ctx, config, wt_path, branch)?;

        // Issue provider cleanup hook
        if let Ok(provider) = build_provider(ctx) {
            if let Err(e) = provider.on_clean(&entry.branch, &entry.branch) {
                ctx.ui.print_warning(&format!("  Issue cleanup: {e}"));
            }
        }

        // Remove worktree
        if !remove_worktree(ctx, &git, wt_path)? {
            continue;
        }
        mark_matching_task_runs_done(ctx, entry);

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

    restore_caller_cmux_workspace(ctx, &cmux, &cmux_workspaces, &closed_cmux_workspace_ids);

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

fn remove_worktree(ctx: &Ctx, git: &GitService<'_>, wt_path: &Path) -> Result<bool> {
    let remove_result = git.worktree_remove(wt_path)?;
    if remove_result.success {
        ctx.ui
            .print_step(&format!("  Removed: {}", wt_path.display()));
        return Ok(true);
    }

    ctx.ui
        .print_warning(&format!("  Failed to remove {}", wt_path.display()));
    if !remove_result.stderr.is_empty() {
        ctx.ui.print_warning(&format!("  {}", remove_result.stderr));
    }

    match git.worktree_remove_force(wt_path) {
        Ok(()) => {
            ctx.ui.print_step("  Force removed");
            Ok(true)
        }
        Err(err) => {
            ctx.ui
                .print_warning(&format!("  Force remove failed: {err}"));
            Ok(false)
        }
    }
}

fn mark_matching_task_runs_done(ctx: &Ctx, entry: &crate::services::git::WorktreeEntry) {
    let runs = match task_run::running_cleanup_matches(ctx, &entry.branch) {
        Ok(runs) => runs,
        Err(err) => {
            ctx.ui.print_warning(&format!("  TaskRun lookup: {err}"));
            return;
        }
    };

    for record in runs {
        match task_run::update(ctx, &record.id, task_run::STATUS_DONE, None, None) {
            Ok(_) => ctx
                .ui
                .print_step(&format!("  TaskRun marked done: {}", record.id)),
            Err(err) => ctx.ui.print_warning(&format!(
                "  Failed to mark TaskRun {} done: {err}",
                record.id
            )),
        }
    }
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
) -> Vec<String> {
    if cmux_workspaces.is_empty() {
        return Vec::new();
    }

    close_workspaces_at_path(ctx, cmux, cmux_workspaces, &entry.path)
}

fn close_workspaces_at_path(
    ctx: &Ctx,
    cmux: &CmuxService<'_>,
    cmux_workspaces: &mut Vec<CmuxWorkspace>,
    worktree_path: &Path,
) -> Vec<String> {
    let mut closed = Vec::new();
    let mut idx = 0;

    while idx < cmux_workspaces.len() {
        if cmux_workspaces[idx]
            .current_directory
            .as_deref()
            .is_some_and(|cwd| same_path(cwd, worktree_path))
        {
            let workspace = cmux_workspaces.remove(idx);
            match cmux.close_workspace(&workspace.id) {
                Ok(()) => {
                    closed.push(workspace.id);
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

fn restore_caller_cmux_workspace(
    ctx: &Ctx,
    cmux: &CmuxService<'_>,
    cmux_workspaces: &[CmuxWorkspace],
    closed_workspace_ids: &[String],
) {
    if closed_workspace_ids.is_empty() {
        return;
    }

    let Some(caller_workspace) = cmux
        .caller_context()
        .and_then(|caller| caller.workspace)
        .filter(|workspace| !workspace.trim().is_empty())
    else {
        return;
    };

    let Some(workspace) = cmux_workspaces
        .iter()
        .find(|workspace| workspace.id == caller_workspace || workspace.handle == caller_workspace)
    else {
        return;
    };

    if closed_workspace_ids.contains(&workspace.id) {
        return;
    }

    if let Err(e) = cmux.select_workspace(&workspace.id) {
        ctx.ui
            .print_warning(&format!("  Failed to restore cmux workspace focus: {e}"));
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn resolve_targets(
    ctx: &Ctx,
    entries: &[crate::services::git::WorktreeEntry],
    targets: &[String],
) -> Result<Vec<usize>> {
    let mut selected = Vec::new();
    for target in targets {
        let worktree_matches = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| worktree_matches(entry, target))
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();
        let task_run_match = direct_task_run_worktree_match(ctx, entries, target)?;

        match (worktree_matches.as_slice(), task_run_match) {
            ([idx], None) => {
                if !selected.contains(idx) {
                    selected.push(*idx);
                }
            }
            ([], Some(idx)) => {
                if !selected.contains(&idx) {
                    selected.push(idx);
                }
            }
            ([], None) => bail!("No worktree matches {target:?}"),
            (_, Some(_)) => {
                bail!(
                    "Work target is ambiguous: {target:?} matches both a worktree target and a TaskRun id"
                )
            }
            _ => bail!("Multiple worktrees match {target:?}"),
        }
    }
    Ok(selected)
}

fn direct_task_run_worktree_match(
    ctx: &Ctx,
    entries: &[crate::services::git::WorktreeEntry],
    target: &str,
) -> Result<Option<usize>> {
    let Some(record) = task_run_record_for_id(ctx, target)? else {
        return Ok(None);
    };

    reject_non_direct_task_run(ctx, &record)?;

    let matches = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.branch == record.run.branch)
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [idx] => Ok(Some(*idx)),
        [] => bail!(
            "No worktree matches TaskRun {:?} (branch {:?})",
            record.id,
            record.run.branch
        ),
        _ => bail!(
            "Multiple worktrees match TaskRun {:?} (branch {:?})",
            record.id,
            record.run.branch
        ),
    }
}

fn validate_task_run_targets_without_worktrees(ctx: &Ctx, targets: &[String]) -> Result<()> {
    for target in targets {
        if task_run_record_for_id(ctx, target)?.is_some() {
            direct_task_run_worktree_match(ctx, &[], target)?;
        }
    }
    Ok(())
}

fn task_run_record_for_id(ctx: &Ctx, target: &str) -> Result<Option<TaskRunRecord>> {
    Ok(task_run::list(ctx)?
        .into_iter()
        .find(|record| record.id == target))
}

fn reject_non_direct_task_run(ctx: &Ctx, record: &TaskRunRecord) -> Result<()> {
    match task_run::resolve_context(ctx, record)? {
        TaskRunContext::Direct => Ok(()),
        TaskRunContext::WorkflowLinked(context) => bail!(
            "TaskRun {} is workflow-linked to {} task {}. Use `wt inspect {}` for context and complete it with `wt workflow complete {} {}`; `wt done` only accepts direct TaskRun ids.",
            record.id,
            context.workflow_id,
            context.task,
            record.id,
            context.workflow_path.display(),
            context.task
        ),
        TaskRunContext::UnresolvedWorkflowGroup { group } => bail!(
            "TaskRun {} belongs to workflow group {}, but the workflow file was not discovered. Use `wt inspect {}` for context and complete the workflow path instead; `wt done` only accepts direct TaskRun ids.",
            record.id,
            group,
            record.id
        ),
    }
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
    use crate::task_run;
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
    fn clean_marks_matching_running_direct_task_runs_done() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = repo.path().with_file_name("test-repo-add-schema");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/add-schema\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("", true); // worktree remove
        runner.add_response("", true); // branch delete
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let direct_run = task_run::create(
            &ctx,
            "add-schema",
            "alice/add-schema",
            None,
            task_run::STATUS_RUNNING,
        )
        .unwrap();
        let grouped_run = task_run::create(
            &ctx,
            "workflow-schema",
            "alice/add-schema",
            Some("2026-05-16-001"),
            task_run::STATUS_RUNNING,
        )
        .unwrap();

        run_with_targets(&ctx, &["alice/add-schema".into()]).unwrap();

        assert_eq!(
            task_run::read(&direct_run.path).unwrap().status,
            task_run::STATUS_DONE
        );
        assert_eq!(
            task_run::read(&grouped_run.path).unwrap().status,
            task_run::STATUS_RUNNING
        );
    }

    #[test]
    fn clean_accepts_direct_task_run_id_as_target() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = repo.path().with_file_name("test-repo-add-schema");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/add-schema\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("", true); // worktree remove
        runner.add_response("", true); // branch delete
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let run = task_run::create(
            &ctx,
            "add-schema",
            "alice/add-schema",
            None,
            task_run::STATUS_RUNNING,
        )
        .unwrap();

        run_with_targets(&ctx, std::slice::from_ref(&run.id)).unwrap();

        assert_eq!(
            task_run::read(&run.path).unwrap().status,
            task_run::STATUS_DONE
        );
    }

    #[test]
    fn clean_rejects_workflow_linked_task_run_id() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = repo.path().with_file_name("test-repo-add-schema");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/add-schema\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let run = task_run::create(
            &ctx,
            "workflow-schema",
            "alice/add-schema",
            Some("2026-05-16-001"),
            task_run::STATUS_RUNNING,
        )
        .unwrap();
        let workflow_path = repo.path().join(".local/workflows/2026-05-16-001.toml");
        let mut workflow = crate::workflow::WorkflowMetadata::new(
            crate::workflow::WorkflowMode::Batch,
            "branch",
            Some("main".into()),
            vec![crate::workflow::WorkflowTask::new(
                "workflow-schema",
                run.id.clone(),
            )],
        );
        crate::workflow::write(&ctx, &workflow_path, &mut workflow).unwrap();

        let err = run_with_targets(&ctx, std::slice::from_ref(&run.id)).unwrap_err();
        let message = format!("{err:#}");

        assert!(message.contains("workflow-linked"));
        assert!(message.contains("wt workflow complete"));
        assert_eq!(
            task_run::read(&run.path).unwrap().status,
            task_run::STATUS_RUNNING
        );
    }

    #[test]
    fn clean_rejects_direct_task_run_id_without_matching_worktree() {
        let repo = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        );

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let run = task_run::create(
            &ctx,
            "add-schema",
            "alice/add-schema",
            None,
            task_run::STATUS_RUNNING,
        )
        .unwrap();

        let err = run_with_targets(&ctx, std::slice::from_ref(&run.id)).unwrap_err();
        let message = format!("{err:#}");

        assert!(message.contains("No worktree matches TaskRun"));
        assert!(message.contains("alice/add-schema"));
    }

    #[test]
    fn clean_does_not_mark_task_run_done_when_worktree_remove_fails() {
        let repo = tempfile::tempdir().unwrap();
        let worktree = repo.path().with_file_name("test-repo-add-schema");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/add-schema\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("", false); // worktree remove
        runner.add_response("", false); // force worktree remove
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/add-schema\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let run = task_run::create(
            &ctx,
            "add-schema",
            "alice/add-schema",
            None,
            task_run::STATUS_RUNNING,
        )
        .unwrap();

        run_with_targets(&ctx, &["alice/add-schema".into()]).unwrap();

        assert_eq!(
            task_run::read(&run.path).unwrap().status,
            task_run::STATUS_RUNNING
        );
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
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"/tmp/other-repo-feature"},{"id":"uuid-workspace-2","ref":"workspace:2","title":"custom title","current_directory":"/tmp/test-repo-feature"}]}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(r#"{"caller":null}"#, true);
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
                        "rpc".to_string(),
                        "workspace.list".to_string(),
                        r#"{"window_id":"uuid-window-1"}"#.to_string(),
                    ]
        }));
        let close_idx = calls
            .iter()
            .position(|(cmd, args, _)| {
                cmd == "cmux" && args.get(1).is_some_and(|arg| arg == "workspace.close")
            })
            .expect("expected cmux workspace.close call");
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
            vec![
                "rpc",
                "workspace.close",
                r#"{"workspace_id":"uuid-workspace-2"}"#
            ]
        );
    }

    #[test]
    fn clean_restores_caller_cmux_workspace_after_closing_another_workspace() {
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/test-repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"},{"id":"uuid-window-2","ref":"window:2"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-caller","ref":"workspace:1","title":"main","current_directory":"/tmp/test-repo"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-2","window_ref":"window:2","workspaces":[{"id":"uuid-target","ref":"workspace:2","title":"feature","current_directory":"/tmp/test-repo-feature"}]}"#,
            true,
        );
        runner.add_response("", true); // cmux workspace.close
        runner.add_response("", true); // git worktree remove
        runner.add_response("", true); // git branch delete
        runner.add_response(r#"{"caller":{"workspace_ref":"workspace:1"}}"#, true);
        runner.add_response("", true); // cmux workspace.select
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
        let close_idx = calls
            .iter()
            .position(|(cmd, args, _)| {
                cmd == "cmux" && args.get(1).is_some_and(|arg| arg == "workspace.close")
            })
            .expect("expected cmux workspace.close call");
        let select_idx = calls
            .iter()
            .position(|(cmd, args, _)| {
                cmd == "cmux" && args.get(1).is_some_and(|arg| arg == "workspace.select")
            })
            .expect("expected cmux workspace.select call");

        assert!(close_idx < select_idx);
        assert_eq!(
            calls[select_idx].1,
            vec![
                "rpc",
                "workspace.select",
                r#"{"workspace_id":"uuid-caller"}"#
            ]
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
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-2","ref":"workspace:2","title":"feature","current_directory":"/tmp/other-repo-feature"}]}"#,
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
            cmd == "cmux" && args.get(1).is_some_and(|arg| arg == "workspace.close")
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

    #[test]
    fn explicit_targets_preserve_branch_path_and_issue_shorthand_matching() {
        let repo = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let entries = vec![crate::services::git::WorktreeEntry {
            path: "/tmp/sample-app-proj-123-fix-editor".into(),
            branch: "alice/proj-123-fix-editor".into(),
        }];

        assert_eq!(
            resolve_targets(&ctx, &entries, &["alice/proj-123-fix-editor".into()]).unwrap(),
            vec![0]
        );
        assert_eq!(
            resolve_targets(&ctx, &entries, &["sample-app-proj-123-fix-editor".into()]).unwrap(),
            vec![0]
        );
        assert_eq!(
            resolve_targets(&ctx, &entries, &["PROJ-123".into()]).unwrap(),
            vec![0]
        );
    }
}
