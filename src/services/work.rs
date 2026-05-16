use crate::commands::task_run;
use crate::context::Ctx;
use crate::services::cmux::{CmuxService, CmuxWorkspace};
use crate::services::git::{GitService, WorktreeEntry};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkTarget {
    pub(crate) label: String,
    pub(crate) branch: String,
    pub(crate) worktree: Option<PathBuf>,
    pub(crate) task_run: Option<task_run::TaskRunRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CmuxContact {
    pub(crate) workspace: String,
    pub(crate) surface: String,
    pub(crate) pane: String,
    pub(crate) title: String,
    pub(crate) window: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) target: WorkTarget,
    pub(crate) state: WorkState,
    pub(crate) cmux: Option<WorkCmuxSurface>,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkState {
    NoLocalWorktree,
    CmuxUnavailable,
    NoCmuxWorkspace,
    NoTerminalSurface,
    TerminalSurfaceReady,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkCmuxSurface {
    pub(crate) workspace_id: String,
    pub(crate) workspace_ref: String,
    pub(crate) workspace_title: String,
    pub(crate) window_id: String,
    pub(crate) window_ref: String,
    pub(crate) pane_id: Option<String>,
    pub(crate) pane_ref: Option<String>,
    pub(crate) surface_id: Option<String>,
    pub(crate) surface_ref: Option<String>,
}

impl WorkCmuxSurface {
    pub(crate) fn contact(&self) -> Option<CmuxContact> {
        Some(CmuxContact {
            workspace: self.workspace_ref.clone(),
            surface: self.surface_ref.clone()?,
            pane: self.pane_ref.clone()?,
            title: self.workspace_title.clone(),
            window: self.window_ref.clone(),
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum WorkTargetError {
    #[error("Work target not found: {target}")]
    NotFound { target: String },
    #[error("Work target is ambiguous: {target} matches {matches}")]
    Ambiguous { target: String, matches: String },
}

pub(crate) fn observe_work(ctx: &Ctx, target: Option<&str>) -> Result<Work> {
    let target = resolve_target(ctx, target)?;
    let Some(worktree) = target.worktree.clone() else {
        return Ok(Work {
            message: Some(format!(
                "Target branch is not checked out in a local worktree: {}",
                target.branch
            )),
            target,
            state: WorkState::NoLocalWorktree,
            cmux: None,
        });
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        return Ok(Work {
            target,
            state: WorkState::CmuxUnavailable,
            cmux: None,
            message: Some("cmux command not found".into()),
        });
    }

    let workspaces = match cmux.list_workspaces() {
        Ok(workspaces) => workspaces,
        Err(err) => {
            return Ok(Work {
                target,
                state: WorkState::CmuxUnavailable,
                cmux: None,
                message: Some(format!("cmux workspace lookup failed: {err:#}")),
            });
        }
    };

    let Some(workspace) = workspaces
        .into_iter()
        .find(|workspace| cmux_workspace_matches(workspace, &worktree))
    else {
        return Ok(Work {
            target,
            state: WorkState::NoCmuxWorkspace,
            cmux: None,
            message: Some(format!(
                "No cmux workspace found for worktree: {}",
                worktree.display()
            )),
        });
    };

    let mut surface = work_cmux_workspace(&workspace);
    let selected_surfaces = match cmux.selected_surfaces(&workspace.handle) {
        Ok(selected_surfaces) => selected_surfaces,
        Err(err) => {
            return Ok(Work {
                target,
                state: WorkState::NoTerminalSurface,
                cmux: Some(surface),
                message: Some(format!("cmux pane lookup failed: {err:#}")),
            });
        }
    };

    let Some(selected) = selected_surfaces.into_iter().next() else {
        return Ok(Work {
            target,
            state: WorkState::NoTerminalSurface,
            cmux: Some(surface),
            message: Some(format!(
                "No selected cmux surface found for workspace: {}",
                workspace.handle
            )),
        });
    };

    surface.pane_id = Some(selected.pane_id);
    surface.pane_ref = Some(selected.pane_handle);
    surface.surface_id = Some(selected.selected_surface_id);
    surface.surface_ref = Some(selected.selected_surface_handle.clone());

    match cmux.read_screen_lines(&selected.selected_surface_handle, &workspace.handle, 1) {
        Ok(_) => Ok(Work {
            target,
            state: WorkState::TerminalSurfaceReady,
            cmux: Some(surface),
            message: None,
        }),
        Err(err) => Ok(Work {
            target,
            state: WorkState::NoTerminalSurface,
            cmux: Some(surface),
            message: Some(format!("cmux terminal surface is not ready: {err:#}")),
        }),
    }
}

pub(crate) fn resolve_target(ctx: &Ctx, target: Option<&str>) -> Result<WorkTarget> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let worktrees = git.worktree_list()?;
    match target {
        None => {
            let branch = git.current_branch()?;
            let worktree = worktrees
                .iter()
                .find(|entry| entry.branch == branch)
                .map(|entry| entry.path.clone())
                .or_else(|| Some(ctx.invocation_root.clone()));
            Ok(WorkTarget {
                label: branch.clone(),
                branch,
                worktree,
                task_run: None,
            })
        }
        Some(raw) => resolve_explicit_target(ctx, &git, &worktrees, raw),
    }
}

pub(crate) fn cmux_contacts(ctx: &Ctx, worktree: &Path) -> Result<Vec<CmuxContact>> {
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        bail!("cmux command not found");
    }

    let workspaces = cmux.list_workspaces()?;
    let mut contacts = Vec::new();
    for workspace in workspaces
        .iter()
        .filter(|workspace| cmux_workspace_matches(workspace, worktree))
    {
        for (pane, surface) in cmux_surfaces(&cmux, &workspace.handle)? {
            contacts.push(CmuxContact {
                workspace: workspace.handle.clone(),
                surface,
                pane,
                title: workspace.title.clone(),
                window: workspace.window_handle.clone(),
            });
        }
    }
    Ok(contacts)
}

fn resolve_explicit_target(
    ctx: &Ctx,
    git: &GitService,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> Result<WorkTarget> {
    if let Some(path) = existing_directory_target(ctx, raw) {
        let branch = branch_at_path(ctx, &path)?;
        return Ok(WorkTarget {
            label: raw.to_string(),
            branch,
            worktree: Some(path),
            task_run: None,
        });
    }

    if let Ok(path) = task_run::resolve(ctx, raw) {
        if path.is_file() {
            let run = task_run::read(&path)?;
            let id = task_run_id(&path)?;
            let worktree = worktree_for_branch(worktrees, &run.branch)
                .or_else(|| git.checked_out_path(&run.branch).ok().flatten());
            return Ok(WorkTarget {
                label: id.clone(),
                branch: run.branch.clone(),
                worktree,
                task_run: Some(task_run::TaskRunRecord { id, path, run }),
            });
        }
    }

    if let Some(entry) = worktrees.iter().find(|entry| entry.branch == raw) {
        return Ok(WorkTarget {
            label: raw.to_string(),
            branch: entry.branch.clone(),
            worktree: Some(entry.path.clone()),
            task_run: None,
        });
    }

    let matches = worktrees
        .iter()
        .filter(|entry| worktree_path_matches(entry, raw))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => {}
        [entry] => {
            return Ok(WorkTarget {
                label: raw.to_string(),
                branch: entry.branch.clone(),
                worktree: Some(entry.path.clone()),
                task_run: None,
            });
        }
        _ => {
            let matches = matches
                .iter()
                .map(|entry| format!("{} ({})", entry.branch, entry.path.display()))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(WorkTargetError::Ambiguous {
                target: raw.into(),
                matches,
            }
            .into());
        }
    }

    if git.local_branch_exists(raw)? {
        return Ok(WorkTarget {
            label: raw.to_string(),
            branch: raw.to_string(),
            worktree: git.checked_out_path(raw)?,
            task_run: None,
        });
    }

    Err(WorkTargetError::NotFound { target: raw.into() }.into())
}

fn existing_directory_target(ctx: &Ctx, raw: &str) -> Option<PathBuf> {
    let raw_path = PathBuf::from(raw);
    let mut candidates = Vec::new();
    if raw_path.is_absolute() {
        candidates.push(raw_path);
    } else {
        candidates.push(ctx.invocation_root.join(raw));
        candidates.push(ctx.repo_root.join(raw));
        candidates.push(ctx.parent_dir.join(raw));
    }

    candidates.into_iter().find(|path| path.is_dir())
}

fn branch_at_path(ctx: &Ctx, path: &Path) -> Result<String> {
    let out = ctx
        .runner
        .run("git", &["rev-parse", "--abbrev-ref", "HEAD"], Some(path))?;
    if out.success && !out.stdout.is_empty() {
        Ok(out.stdout)
    } else {
        bail!(
            "Failed to read worktree branch at {}: {}",
            path.display(),
            if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            }
        )
    }
}

fn worktree_for_branch(worktrees: &[WorktreeEntry], branch: &str) -> Option<PathBuf> {
    worktrees
        .iter()
        .find(|entry| entry.branch == branch)
        .map(|entry| entry.path.clone())
}

fn worktree_path_matches(entry: &WorktreeEntry, raw: &str) -> bool {
    entry.path.to_string_lossy() == raw
        || entry
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == raw)
}

fn cmux_surfaces(cmux: &CmuxService<'_>, workspace_handle: &str) -> Result<Vec<(String, String)>> {
    let panes = cmux.list_panes(workspace_handle)?;
    let mut contacts = Vec::new();
    for pane in panes {
        for surface in cmux.list_pane_surfaces(&pane, workspace_handle)? {
            contacts.push((pane.clone(), surface));
        }
    }
    Ok(contacts)
}

fn cmux_workspace_matches(workspace: &CmuxWorkspace, worktree: &Path) -> bool {
    workspace
        .current_directory
        .as_deref()
        .is_some_and(|cwd| same_path(cwd, worktree))
}

fn work_cmux_workspace(workspace: &CmuxWorkspace) -> WorkCmuxSurface {
    WorkCmuxSurface {
        workspace_id: workspace.id.clone(),
        workspace_ref: workspace.handle.clone(),
        workspace_title: workspace.title.clone(),
        window_id: workspace.window_id.clone(),
        window_ref: workspace.window_handle.clone(),
        pane_id: None,
        pane_ref: None,
        surface_id: None,
        surface_ref: None,
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn task_run_id(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("TaskRun path is missing a file stem: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};

    #[test]
    fn resolve_target_accepts_branch_target() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        let ctx = fixture.ctx(runner);

        let target = resolve_target(&ctx, Some("feature")).unwrap();

        assert_eq!(target.label, "feature");
        assert_eq!(target.branch, "feature");
        assert_eq!(target.worktree.as_deref(), Some(fixture.worktree.as_path()));
        assert!(target.task_run.is_none());
    }

    #[test]
    fn resolve_target_accepts_worktree_basename_target() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("feature", true);
        let ctx = fixture.ctx(runner);

        let basename = fixture.worktree.file_name().unwrap().to_str().unwrap();
        let target = resolve_target(&ctx, Some(basename)).unwrap();

        assert_eq!(target.label, basename);
        assert_eq!(target.branch, "feature");
        assert_eq!(target.worktree.as_deref(), Some(fixture.worktree.as_path()));
    }

    #[test]
    fn resolve_target_accepts_absolute_path_target() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("feature", true);
        let ctx = fixture.ctx(runner);

        let target = resolve_target(&ctx, Some(fixture.worktree.to_str().unwrap())).unwrap();

        assert_eq!(target.branch, "feature");
        assert_eq!(target.worktree.as_deref(), Some(fixture.worktree.as_path()));
    }

    #[test]
    fn resolve_target_accepts_task_run_id_target() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".local/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nsource = \"stack\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        let ctx = fixture.ctx(runner);

        let target = resolve_target(&ctx, Some("run-feature")).unwrap();

        assert_eq!(target.label, "run-feature");
        assert_eq!(target.branch, "feature");
        assert_eq!(
            target.task_run.as_ref().map(|run| run.id.as_str()),
            Some("run-feature")
        );
        assert_eq!(target.worktree.as_deref(), Some(fixture.worktree.as_path()));
    }

    #[test]
    fn cmux_contacts_returns_matching_workspace_surfaces() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_matching_workspace(&mut runner, &fixture);
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        let ctx = fixture.ctx(runner);

        let contacts = cmux_contacts(&ctx, &fixture.worktree).unwrap();

        assert_eq!(
            contacts,
            vec![CmuxContact {
                workspace: "workspace:1".into(),
                surface: "surface:4".into(),
                pane: "pane:3".into(),
                title: "feature".into(),
                window: "window:1".into(),
            }]
        );
    }

    #[test]
    fn cmux_contacts_returns_empty_without_matching_workspace() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-2","ref":"workspace:2","title":"other","current_directory":"/tmp/other"}]}"#,
            true,
        );
        let ctx = fixture.ctx(runner);

        let contacts = cmux_contacts(&ctx, &fixture.worktree).unwrap();

        assert!(contacts.is_empty());
    }

    #[test]
    fn cmux_contacts_fails_when_cmux_is_unavailable() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(MockRunner::new());

        let err = cmux_contacts(&ctx, &fixture.worktree)
            .unwrap_err()
            .to_string();

        assert!(err.contains("cmux command not found"));
    }

    #[test]
    fn cmux_contacts_fails_when_session_lookup_fails() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response("no cmux session", false);
        let ctx = fixture.ctx(runner);

        let err = cmux_contacts(&ctx, &fixture.worktree)
            .unwrap_err()
            .to_string();

        assert!(err.contains("cmux window.list failed"));
    }

    #[test]
    fn cmux_contacts_returns_empty_when_workspace_has_no_surface() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_matching_workspace(&mut runner, &fixture);
        runner.add_response("pane:3", true);
        runner.add_response("", true);
        let ctx = fixture.ctx(runner);

        let contacts = cmux_contacts(&ctx, &fixture.worktree).unwrap();

        assert!(contacts.is_empty());
    }

    #[test]
    fn observe_work_returns_ready_mapping_with_selected_surface() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("ready", true);
        let ctx = fixture.ctx(runner);

        let work = observe_work(&ctx, Some("feature")).unwrap();

        assert_eq!(work.state, WorkState::TerminalSurfaceReady);
        assert_eq!(work.target.branch, "feature");
        assert_eq!(
            work.target.worktree.as_deref(),
            Some(fixture.worktree.as_path())
        );
        let cmux = work.cmux.unwrap();
        assert_eq!(cmux.workspace_id, "uuid-workspace-1");
        assert_eq!(cmux.workspace_ref, "workspace:1");
        assert_eq!(cmux.workspace_title, "feature");
        assert_eq!(cmux.window_id, "uuid-window-1");
        assert_eq!(cmux.window_ref, "window:1");
        assert_eq!(cmux.pane_id.as_deref(), Some("uuid-pane-3"));
        assert_eq!(cmux.pane_ref.as_deref(), Some("pane:3"));
        assert_eq!(cmux.surface_id.as_deref(), Some("uuid-surface-4"));
        assert_eq!(cmux.surface_ref.as_deref(), Some("surface:4"));
    }

    #[test]
    fn observe_work_distinguishes_no_cmux_workspace() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-2","ref":"workspace:2","title":"other","current_directory":"/tmp/other"}]}"#,
            true,
        );
        let ctx = fixture.ctx(runner);

        let work = observe_work(&ctx, Some("feature")).unwrap();

        assert_eq!(work.state, WorkState::NoCmuxWorkspace);
        assert!(work.cmux.is_none());
        assert!(work.message.unwrap().contains("No cmux workspace found"));
    }

    #[test]
    fn observe_work_distinguishes_no_terminal_surface_without_selected_surface() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":null,"selected_surface_ref":null}]}"#,
            true,
        );
        let ctx = fixture.ctx(runner);

        let work = observe_work(&ctx, Some("feature")).unwrap();

        assert_eq!(work.state, WorkState::NoTerminalSurface);
        let cmux = work.cmux.unwrap();
        assert_eq!(cmux.workspace_ref, "workspace:1");
        assert!(cmux.surface_ref.is_none());
    }

    #[test]
    fn observe_work_distinguishes_cold_terminal_surface() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Terminal surface not found", false);
        let ctx = fixture.ctx(runner);

        let work = observe_work(&ctx, Some("feature")).unwrap();

        assert_eq!(work.state, WorkState::NoTerminalSurface);
        let cmux = work.cmux.unwrap();
        assert_eq!(cmux.surface_ref.as_deref(), Some("surface:4"));
        assert!(
            work.message
                .unwrap()
                .contains("terminal surface is not ready")
        );
    }

    #[test]
    fn observe_work_distinguishes_cmux_unavailable() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        let ctx = fixture.ctx(runner);

        let work = observe_work(&ctx, Some("feature")).unwrap();

        assert_eq!(work.state, WorkState::CmuxUnavailable);
        assert!(work.message.unwrap().contains("cmux command not found"));
    }

    #[test]
    fn resolve_target_reports_missing_target() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        let ctx = fixture.ctx(runner);

        let err = resolve_target(&ctx, Some("missing"))
            .unwrap_err()
            .to_string();

        assert!(err.contains("Work target not found: missing"));
    }

    #[test]
    fn resolve_target_rejects_ambiguous_worktree_name() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_response(
            "\
worktree /tmp/one/dup
HEAD abc
branch refs/heads/one

worktree /tmp/two/dup
HEAD def
branch refs/heads/two

",
            true,
        );
        let ctx = fixture.ctx(runner);

        let err = resolve_target(&ctx, Some("dup")).unwrap_err().to_string();

        assert!(err.contains("Work target is ambiguous"));
        assert!(err.contains("one (/tmp/one/dup)"));
        assert!(err.contains("two (/tmp/two/dup)"));
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        repo: PathBuf,
        worktree: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().join("sample");
            let worktree = dir.path().join("sample-feature");
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::create_dir_all(&worktree).unwrap();
            Self {
                _dir: dir,
                repo,
                worktree,
            }
        }

        fn ctx(&self, runner: MockRunner) -> Ctx {
            Ctx::new(
                self.repo.clone(),
                self.repo.clone(),
                Config::default(),
                Box::new(runner),
                Box::new(MockUi::new()),
            )
        }
    }

    fn add_worktree_list(runner: &mut MockRunner, fixture: &Fixture) {
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                fixture.repo.display(),
                fixture.worktree.display()
            ),
            true,
        );
    }

    fn add_matching_workspace(runner: &mut MockRunner, fixture: &Fixture) {
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                fixture.worktree.display()
            ),
            true,
        );
    }

    fn add_selected_surface(runner: &mut MockRunner) {
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
    }
}
