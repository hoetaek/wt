pub(crate) use crate::agents::WorkState;
use crate::agents::{self, AgentKind, AgentObservation};
use crate::commands::task_run;
use crate::context::Ctx;
use crate::services::cmux::{CmuxEvent, CmuxPaneSelectedSurface, CmuxService, CmuxWorkspace};
use crate::services::git::{GitService, WorktreeEntry};
use anyhow::{Result, bail};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const WORK_OBSERVATION_SCREEN_LINES: usize = 80;
const WORK_OBSERVATION_EVENT_LIMIT: usize = 4000;
const WORK_OBSERVATION_EVENT_TIMEOUT: Duration = Duration::from_millis(150);

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
    pub(crate) selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Work {
    pub(crate) target: WorkTarget,
    pub(crate) state: WorkState,
    pub(crate) session_state: WorkSessionState,
    pub(crate) cmux: Option<WorkCmuxSurface>,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkSessionState {
    NoLocalWorktree,
    CmuxUnavailable,
    NoCmuxWorkspace,
    NoTerminalSurface,
    TerminalSurfaceReady,
}

impl WorkSessionState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoLocalWorktree => "no_local_worktree",
            Self::CmuxUnavailable => "cmux_unavailable",
            Self::NoCmuxWorkspace => "no_cmux_workspace",
            Self::NoTerminalSurface => "no_terminal_surface",
            Self::TerminalSurfaceReady => "terminal_surface_ready",
        }
    }
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
            selected: true,
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

#[derive(Clone, Debug)]
struct WorkTargetCandidate {
    target: WorkTarget,
    matches: Vec<String>,
}

pub(crate) fn observe_work(ctx: &Ctx, target: Option<&str>) -> Result<Work> {
    let target = resolve_target(ctx, target)?;
    observe_target(ctx, target)
}

pub(crate) fn observe_target(ctx: &Ctx, target: WorkTarget) -> Result<Work> {
    let Some(worktree) = target.worktree.clone() else {
        return Ok(Work {
            message: Some(format!(
                "Target branch is not checked out in a local worktree: {}",
                target.branch
            )),
            target,
            state: WorkState::no_session(AgentKind::Unknown),
            session_state: WorkSessionState::NoLocalWorktree,
            cmux: None,
        });
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        return Ok(Work {
            target,
            state: WorkState::no_session(AgentKind::Unknown),
            session_state: WorkSessionState::CmuxUnavailable,
            cmux: None,
            message: Some("cmux command not found".into()),
        });
    }

    let workspaces = match cmux.list_workspaces() {
        Ok(workspaces) => workspaces,
        Err(err) => {
            return Ok(Work {
                target,
                state: WorkState::no_session(AgentKind::Unknown),
                session_state: WorkSessionState::CmuxUnavailable,
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
            state: WorkState::no_session(AgentKind::Unknown),
            session_state: WorkSessionState::NoCmuxWorkspace,
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
                state: WorkState::no_session(AgentKind::Unknown),
                session_state: WorkSessionState::NoTerminalSurface,
                cmux: Some(surface),
                message: Some(format!("cmux pane lookup failed: {err:#}")),
            });
        }
    };

    let Some(selected) = selected_surfaces.into_iter().next() else {
        return Ok(Work {
            target,
            state: WorkState::no_session(AgentKind::Unknown),
            session_state: WorkSessionState::NoTerminalSurface,
            cmux: Some(surface),
            message: Some(format!(
                "No selected cmux surface found for workspace: {}",
                workspace.handle
            )),
        });
    };

    surface.pane_id = Some(selected.pane_id.clone());
    surface.pane_ref = Some(selected.pane_handle.clone());
    surface.surface_id = Some(selected.selected_surface_id.clone());
    surface.surface_ref = Some(selected.selected_surface_handle.clone());

    match cmux.read_screen_lines(
        &selected.selected_surface_handle,
        &workspace.handle,
        WORK_OBSERVATION_SCREEN_LINES,
    ) {
        Ok(screen) => {
            let statuses = cmux.list_status(&workspace.handle).unwrap_or_default();
            let events = cmux
                .replay_events_after(
                    0,
                    WORK_OBSERVATION_EVENT_LIMIT,
                    WORK_OBSERVATION_EVENT_TIMEOUT,
                )
                .map(|events| filter_work_events(events, &workspace, &selected))
                .unwrap_or_default();
            let observation = AgentObservation::new(Some(&screen), &statuses, &events);
            Ok(Work {
                target,
                state: agents::classify(&observation),
                session_state: WorkSessionState::TerminalSurfaceReady,
                cmux: Some(surface),
                message: None,
            })
        }
        Err(err) => Ok(Work {
            target,
            state: WorkState::no_session(AgentKind::Unknown),
            session_state: WorkSessionState::NoTerminalSurface,
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
                selected: false,
            });
        }
    }
    if contacts.len() > 1 {
        mark_selected_cmux_contacts(&cmux, &mut contacts);
    }
    Ok(contacts)
}

fn resolve_explicit_target(
    ctx: &Ctx,
    git: &GitService,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> Result<WorkTarget> {
    if is_explicit_path_target(raw) {
        return resolve_explicit_path_target(ctx, worktrees, raw);
    }

    if let Some(candidate) = explicit_task_run_path_candidate(ctx, worktrees, raw)? {
        return Ok(candidate.target);
    }

    let mut candidates = Vec::new();
    let mut path_error = None;

    if let Some(path) = existing_directory_target(ctx, raw) {
        match path_target_candidate(ctx, raw, path) {
            Ok(candidate) => add_work_target_candidate(&mut candidates, candidate),
            Err(err) => path_error = Some(err),
        }
    }

    if let Some(candidate) = task_run_candidate(ctx, worktrees, raw)? {
        add_work_target_candidate(&mut candidates, candidate);
    }

    let has_checked_out_branch = add_checked_out_branch_candidate(&mut candidates, worktrees, raw);
    for entry in worktrees
        .iter()
        .filter(|entry| worktree_path_matches(entry, raw))
    {
        add_work_target_candidate(&mut candidates, worktree_name_candidate(raw, entry));
    }

    if !has_checked_out_branch && candidates.len() <= 1 && git.local_branch_exists(raw)? {
        add_work_target_candidate(&mut candidates, branch_candidate(raw, None));
    }

    if candidates.is_empty() {
        if let Some(err) = path_error {
            return Err(err);
        }
        return Err(WorkTargetError::NotFound { target: raw.into() }.into());
    }

    select_work_target(raw, candidates)
}

fn is_explicit_path_target(raw: &str) -> bool {
    let raw_path = Path::new(raw);
    raw_path.is_absolute()
        || raw == "."
        || raw == ".."
        || raw.starts_with("./")
        || raw.starts_with("../")
}

fn resolve_explicit_path_target(
    ctx: &Ctx,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> Result<WorkTarget> {
    if let Some(candidate) = task_run_candidate(ctx, worktrees, raw)? {
        return Ok(candidate.target);
    }

    if let Some(path) = existing_directory_target(ctx, raw) {
        return Ok(path_target_candidate(ctx, raw, path)?.target);
    }

    Err(WorkTargetError::NotFound { target: raw.into() }.into())
}

fn explicit_task_run_path_candidate(
    ctx: &Ctx,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> Result<Option<WorkTargetCandidate>> {
    if !raw.contains('/') {
        return Ok(None);
    }

    let Some(path) = task_run_path(ctx, raw) else {
        return Ok(None);
    };
    if is_task_run_file_path(&path) {
        return task_run_candidate_from_path(worktrees, path).map(Some);
    }

    Ok(None)
}

fn path_target_candidate(ctx: &Ctx, raw: &str, path: PathBuf) -> Result<WorkTargetCandidate> {
    let branch = branch_at_path(ctx, &path)?;
    Ok(WorkTargetCandidate {
        target: WorkTarget {
            label: raw.to_string(),
            branch: branch.clone(),
            worktree: Some(path.clone()),
            task_run: None,
        },
        matches: vec![format!("path {} (branch {})", path.display(), branch)],
    })
}

fn task_run_candidate(
    ctx: &Ctx,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> Result<Option<WorkTargetCandidate>> {
    let Some(path) = task_run_path(ctx, raw) else {
        return Ok(None);
    };
    task_run_candidate_from_path(worktrees, path).map(Some)
}

fn task_run_path(ctx: &Ctx, raw: &str) -> Option<PathBuf> {
    match task_run::resolve(ctx, raw) {
        Ok(path) if path.is_file() => Some(path),
        _ => None,
    }
}

fn task_run_candidate_from_path(
    worktrees: &[WorktreeEntry],
    path: PathBuf,
) -> Result<WorkTargetCandidate> {
    let run = task_run::read(&path)?;
    let id = task_run_id(&path)?;
    let worktree = worktree_for_branch(worktrees, &run.branch);
    Ok(WorkTargetCandidate {
        target: WorkTarget {
            label: id.clone(),
            branch: run.branch.clone(),
            worktree,
            task_run: Some(task_run::TaskRunRecord {
                id: id.clone(),
                path,
                run: run.clone(),
            }),
        },
        matches: vec![format!("TaskRun {} (branch {})", id, run.branch)],
    })
}

fn is_task_run_file_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "task-runs")
        && path
            .parent()
            .and_then(|parent| parent.parent())
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == ".local")
}

fn add_checked_out_branch_candidate(
    candidates: &mut Vec<WorkTargetCandidate>,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> bool {
    if let Some(entry) = worktrees.iter().find(|entry| entry.branch == raw) {
        add_work_target_candidate(candidates, branch_candidate(raw, Some(entry.path.clone())));
        return true;
    }
    false
}

fn branch_candidate(raw: &str, worktree: Option<PathBuf>) -> WorkTargetCandidate {
    let detail = match worktree.as_ref() {
        Some(path) => format!("branch {} ({})", raw, path.display()),
        None => format!("branch {raw}"),
    };
    WorkTargetCandidate {
        target: WorkTarget {
            label: raw.to_string(),
            branch: raw.to_string(),
            worktree,
            task_run: None,
        },
        matches: vec![detail],
    }
}

fn worktree_name_candidate(raw: &str, entry: &WorktreeEntry) -> WorkTargetCandidate {
    WorkTargetCandidate {
        target: WorkTarget {
            label: raw.to_string(),
            branch: entry.branch.clone(),
            worktree: Some(entry.path.clone()),
            task_run: None,
        },
        matches: vec![format!("{} ({})", entry.branch, entry.path.display())],
    }
}

fn add_work_target_candidate(
    candidates: &mut Vec<WorkTargetCandidate>,
    candidate: WorkTargetCandidate,
) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|existing| same_work_target(&existing.target, &candidate.target))
    {
        for item in candidate.matches {
            if !existing.matches.contains(&item) {
                existing.matches.push(item);
            }
        }
        return;
    }

    candidates.push(candidate);
}

fn same_work_target(a: &WorkTarget, b: &WorkTarget) -> bool {
    match (&a.task_run, &b.task_run) {
        (Some(a_run), Some(b_run)) => a_run.id == b_run.id && same_path(&a_run.path, &b_run.path),
        (None, None) => {
            a.branch == b.branch && same_optional_path(a.worktree.as_deref(), b.worktree.as_deref())
        }
        _ => false,
    }
}

fn same_optional_path(a: Option<&Path>, b: Option<&Path>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => same_path(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn select_work_target(raw: &str, candidates: Vec<WorkTargetCandidate>) -> Result<WorkTarget> {
    match candidates.as_slice() {
        [candidate] => Ok(candidate.target.clone()),
        _ => {
            let matches = candidates
                .iter()
                .map(|candidate| candidate.matches.join(" / "))
                .collect::<Vec<_>>()
                .join(", ");
            Err(WorkTargetError::Ambiguous {
                target: raw.into(),
                matches,
            }
            .into())
        }
    }
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

fn mark_selected_cmux_contacts(cmux: &CmuxService<'_>, contacts: &mut [CmuxContact]) {
    let workspaces = contacts
        .iter()
        .map(|contact| contact.workspace.clone())
        .collect::<BTreeSet<_>>();
    for workspace in workspaces {
        let Ok(selected_surfaces) = cmux.selected_surfaces(&workspace) else {
            continue;
        };
        let selected = selected_surfaces
            .into_iter()
            .map(|surface| (surface.pane_handle, surface.selected_surface_handle))
            .collect::<HashSet<_>>();
        for contact in contacts
            .iter_mut()
            .filter(|contact| contact.workspace == workspace)
        {
            contact.selected = selected.contains(&(contact.pane.clone(), contact.surface.clone()));
        }
    }
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

fn filter_work_events(
    events: Vec<CmuxEvent>,
    workspace: &CmuxWorkspace,
    surface: &CmuxPaneSelectedSurface,
) -> Vec<CmuxEvent> {
    events
        .into_iter()
        .filter(|event| event_matches_work(event, workspace, surface))
        .collect()
}

fn event_matches_work(
    event: &CmuxEvent,
    workspace: &CmuxWorkspace,
    surface: &CmuxPaneSelectedSurface,
) -> bool {
    let workspace_matches = event
        .workspace_id
        .as_deref()
        .is_some_and(|id| id == workspace.id.as_str() || id == workspace.handle.as_str())
        || payload_string(event, "workspace_ref")
            .is_some_and(|value| value == workspace.handle.as_str())
        || payload_string(event, "workspace_id")
            .is_some_and(|value| value == workspace.id.as_str());
    if !workspace_matches {
        return false;
    }

    event.surface_id.as_deref().is_none_or(|id| {
        id == surface.selected_surface_id.as_str() || id == surface.selected_surface_handle.as_str()
    }) || payload_string(event, "surface_ref")
        .is_some_and(|value| value == surface.selected_surface_handle.as_str())
        || payload_string(event, "surface_id")
            .is_some_and(|value| value == surface.selected_surface_id.as_str())
}

fn payload_string<'a>(event: &'a CmuxEvent, key: &str) -> Option<&'a str> {
    event
        .payload
        .get(key)
        .or_else(|| {
            event
                .payload
                .get("result")
                .and_then(|result| result.get(key))
        })
        .and_then(serde_json::Value::as_str)
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
    use crate::agents::{AgentKind, AgentStatus};
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
        runner.add_response("", false);
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
        runner.add_response("", false);
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
    fn resolve_target_rejects_path_branch_collision() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join("feature")).unwrap();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("path-branch", true);
        let ctx = fixture.ctx(runner);

        let err = resolve_target(&ctx, Some("feature"))
            .unwrap_err()
            .to_string();

        assert!(err.contains("Work target is ambiguous"));
        assert!(err.contains("path "));
        assert!(err.contains("branch path-branch"));
        assert!(err.contains("branch feature"));
    }

    #[test]
    fn resolve_target_accepts_explicit_relative_path_collision() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join("feature")).unwrap();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("path-branch", true);
        let ctx = fixture.ctx(runner);

        let target = resolve_target(&ctx, Some("./feature")).unwrap();

        assert_eq!(target.label, "./feature");
        assert_eq!(target.branch, "path-branch");
        assert!(same_path(
            target.worktree.as_deref().unwrap(),
            &fixture.repo.join("./feature")
        ));
    }

    #[test]
    fn resolve_target_rejects_task_run_id_branch_collision() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".local/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nsource = \"stack\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", true);
        let ctx = fixture.ctx(runner);

        let err = resolve_target(&ctx, Some("run-feature"))
            .unwrap_err()
            .to_string();

        assert!(err.contains("Work target is ambiguous"));
        assert!(err.contains("TaskRun run-feature"));
        assert!(err.contains("branch run-feature"));
    }

    #[test]
    fn resolve_target_accepts_explicit_task_run_path_collision() {
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

        let target = resolve_target(&ctx, Some(".local/task-runs/run-feature.toml")).unwrap();

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
                selected: false,
            }]
        );
    }

    #[test]
    fn cmux_contacts_marks_the_selected_matching_surface() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_matching_workspace(&mut runner, &fixture);
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5\nsurface:6", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-5","selected_surface_ref":"surface:5"}]}"#,
            true,
        );
        let ctx = fixture.ctx(runner);

        let contacts = cmux_contacts(&ctx, &fixture.worktree).unwrap();

        assert_eq!(
            contacts,
            vec![
                CmuxContact {
                    workspace: "workspace:1".into(),
                    surface: "surface:4".into(),
                    pane: "pane:3".into(),
                    title: "feature".into(),
                    window: "window:1".into(),
                    selected: false,
                },
                CmuxContact {
                    workspace: "workspace:1".into(),
                    surface: "surface:5".into(),
                    pane: "pane:3".into(),
                    title: "feature".into(),
                    window: "window:1".into(),
                    selected: true,
                },
                CmuxContact {
                    workspace: "workspace:1".into(),
                    surface: "surface:6".into(),
                    pane: "pane:3".into(),
                    title: "feature".into(),
                    window: "window:1".into(),
                    selected: false,
                },
            ]
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

        assert_eq!(work.session_state, WorkSessionState::TerminalSurfaceReady);
        assert_eq!(work.state.agent_kind, AgentKind::Unknown);
        assert_eq!(work.state.status, AgentStatus::Idle);
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

        assert_eq!(work.session_state, WorkSessionState::NoCmuxWorkspace);
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

        assert_eq!(work.session_state, WorkSessionState::NoTerminalSurface);
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

        assert_eq!(work.session_state, WorkSessionState::NoTerminalSurface);
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

        assert_eq!(work.session_state, WorkSessionState::CmuxUnavailable);
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
