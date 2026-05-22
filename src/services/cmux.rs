use crate::context::CommandRunner;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxWorkspace {
    pub id: String,
    pub handle: String,
    pub window_id: String,
    pub window_handle: String,
    pub title: String,
    pub current_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxPaneSelectedSurface {
    pub pane_id: String,
    pub pane_handle: String,
    pub selected_surface_id: String,
    pub selected_surface_handle: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxSurfaceLocation {
    pub workspace_id: String,
    pub workspace_handle: String,
    pub pane_id: String,
    pub pane_handle: String,
    pub surface_id: String,
    pub surface_handle: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxStatusEntry {
    pub key: String,
    pub value: String,
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxProcessInfo {
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxSurfaceProcesses {
    pub surface: String,
    pub processes: Vec<CmuxProcessInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CmuxEvent {
    pub seq: u64,
    pub name: String,
    pub category: Option<String>,
    pub occurred_at: Option<String>,
    pub window_id: Option<String>,
    pub workspace_id: Option<String>,
    pub pane_id: Option<String>,
    pub surface_id: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxCaller {
    pub window: Option<String>,
    pub workspace: Option<String>,
    pub pane: Option<String>,
    pub surface: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxIdentity {
    pub caller: Option<CmuxCaller>,
    pub focused: Option<CmuxCaller>,
}

pub struct CmuxService<'a> {
    runner: &'a dyn CommandRunner,
    focus_new_workspace: bool,
}

impl<'a> CmuxService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self {
            runner,
            focus_new_workspace: true,
        }
    }

    pub fn new_with_workspace_focus(
        runner: &'a dyn CommandRunner,
        focus_new_workspace: bool,
    ) -> Self {
        Self {
            runner,
            focus_new_workspace,
        }
    }

    pub fn is_available(&self) -> bool {
        self.runner.has_command("cmux")
    }

    pub fn new_workspace(&self, cwd: &Path, name: &str, command: &str) -> Result<String> {
        let caller = self.caller_context();
        self.new_workspace_with_caller(cwd, name, command, caller.as_ref())
    }

    pub fn new_workspace_with_caller(
        &self,
        cwd: &Path,
        name: &str,
        command: &str,
        caller: Option<&CmuxCaller>,
    ) -> Result<String> {
        let cwd = cwd.to_string_lossy().into_owned();
        let mut args = vec![
            "new-workspace".to_string(),
            "--cwd".into(),
            cwd,
            "--name".into(),
            name.into(),
            "--command".into(),
            command.into(),
        ];
        if let Some(window) = caller.and_then(|caller| caller.window.as_deref()) {
            args.extend(["--window".into(), window.into()]);
        }
        args.extend(["--focus".into(), self.focus_new_workspace.to_string()]);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let out = self.runner.run("cmux", &arg_refs, None)?;
        if !out.success {
            bail!("cmux new-workspace failed: {}", command_error(&out));
        }
        extract_handle(&out.stdout, "workspace:", "new-workspace")
    }

    fn list_panes_with_surfaces(&self, workspace: &str) -> Result<Vec<RpcPane>> {
        let params = json!({ "workspace_id": workspace }).to_string();
        let out = self
            .runner
            .run("cmux", &["rpc", "pane.list", &params], None)?;
        if !out.success {
            bail!("cmux pane.list failed: {}", command_error(&out));
        }

        let response: PaneListResponse = serde_json::from_str(&out.stdout)?;
        Ok(response.panes)
    }

    pub fn caller_context(&self) -> Option<CmuxCaller> {
        self.identity_context()?.caller
    }

    pub fn identity_context(&self) -> Option<CmuxIdentity> {
        let out = self.runner.run("cmux", &["identify"], None).ok()?;
        if !out.success {
            return None;
        }

        let response: IdentifyResponse = serde_json::from_str(&out.stdout).ok()?;
        Some(response.into())
    }

    pub fn list_workspaces(&self) -> Result<Vec<CmuxWorkspace>> {
        let windows = self.list_windows()?;
        let mut workspaces = Vec::new();

        for window in windows {
            let params = json!({ "window_id": window.id }).to_string();
            let out = self
                .runner
                .run("cmux", &["rpc", "workspace.list", &params], None)?;
            if !out.success {
                bail!("cmux workspace.list failed: {}", command_error(&out));
            }
            let response: WorkspaceListResponse = serde_json::from_str(&out.stdout)?;
            for workspace in response.workspaces {
                workspaces.push(CmuxWorkspace {
                    id: workspace.id,
                    handle: workspace.handle,
                    window_id: response.window_id.clone(),
                    window_handle: response.window_ref.clone(),
                    title: workspace.title,
                    current_directory: workspace.current_directory,
                });
            }
        }

        Ok(workspaces)
    }

    pub fn close_workspace(&self, workspace_id: &str) -> Result<()> {
        let params = json!({ "workspace_id": workspace_id }).to_string();
        let out = self
            .runner
            .run("cmux", &["rpc", "workspace.close", &params], None)?;
        if !out.success {
            bail!("cmux workspace.close failed: {}", command_error(&out));
        }
        Ok(())
    }

    pub fn select_workspace(&self, workspace_id: &str) -> Result<()> {
        let params = json!({ "workspace_id": workspace_id }).to_string();
        let out = self
            .runner
            .run("cmux", &["rpc", "workspace.select", &params], None)?;
        if !out.success {
            bail!("cmux workspace.select failed: {}", command_error(&out));
        }
        Ok(())
    }

    pub fn focus_surface(&self, surface_id: &str, workspace_id: Option<&str>) -> Result<()> {
        let params = match workspace_id {
            Some(workspace_id) => json!({ "surface_id": surface_id, "workspace_id": workspace_id }),
            None => json!({ "surface_id": surface_id }),
        }
        .to_string();
        let out = self
            .runner
            .run("cmux", &["rpc", "surface.focus", &params], None)?;
        if !out.success {
            bail!("cmux surface.focus failed: {}", command_error(&out));
        }
        Ok(())
    }

    pub fn open_command_surface(&self, command: &str) -> Result<String> {
        let caller = self
            .caller_context()
            .ok_or_else(|| anyhow::anyhow!("cmux caller context not found"))?;
        let workspace = caller
            .workspace
            .filter(|workspace| !workspace.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("cmux caller workspace not found"))?;
        let pane = match caller.pane.filter(|pane| !pane.trim().is_empty()) {
            Some(pane) => pane,
            None => self
                .list_panes(&workspace)?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("cmux caller pane not found"))?,
        };

        let surface = self.new_surface(&pane, &workspace)?;
        self.send(&surface, &workspace, &format!("{command}\n"))?;
        Ok(surface)
    }

    fn list_windows(&self) -> Result<Vec<CmuxWindow>> {
        let out = self
            .runner
            .run("cmux", &["rpc", "window.list", "{}"], None)?;
        if !out.success {
            bail!("cmux window.list failed: {}", command_error(&out));
        }
        let response: WindowListResponse = serde_json::from_str(&out.stdout)?;
        Ok(response.windows.into_iter().map(Into::into).collect())
    }

    pub fn list_panes(&self, workspace: &str) -> Result<Vec<String>> {
        let out = self
            .runner
            .run("cmux", &["list-panes", "--workspace", workspace], None)?;
        if !out.success {
            bail!("cmux list-panes failed: {}", command_error(&out));
        }
        let panes: Vec<String> = out
            .stdout
            .split_whitespace()
            .filter(|s| s.starts_with("pane:"))
            .map(String::from)
            .collect();
        Ok(panes)
    }

    pub fn selected_surfaces(&self, workspace: &str) -> Result<Vec<CmuxPaneSelectedSurface>> {
        Ok(self
            .list_panes_with_surfaces(workspace)?
            .into_iter()
            .filter_map(|pane| {
                let selected_surface_id = pane.selected_surface_id?;
                let selected_surface_handle = pane.selected_surface_ref?;
                Some(CmuxPaneSelectedSurface {
                    pane_id: pane.id,
                    pane_handle: pane.handle,
                    selected_surface_id,
                    selected_surface_handle,
                })
            })
            .collect())
    }

    pub fn list_pane_surfaces(&self, pane: &str, workspace: &str) -> Result<Vec<String>> {
        let out = self.runner.run(
            "cmux",
            &[
                "list-pane-surfaces",
                "--pane",
                pane,
                "--workspace",
                workspace,
            ],
            None,
        )?;
        if !out.success {
            bail!("cmux list-pane-surfaces failed: {}", command_error(&out));
        }
        let surfaces: Vec<String> = out
            .stdout
            .split_whitespace()
            .filter(|s| s.starts_with("surface:"))
            .map(String::from)
            .collect();
        Ok(surfaces)
    }

    pub fn find_surface_location(&self, surface: &str) -> Result<Option<CmuxSurfaceLocation>> {
        for workspace in self.list_workspaces()? {
            for pane in self.list_panes_with_surfaces(&workspace.handle)? {
                let surface_len = pane.surface_ids.len().max(pane.surface_refs.len());
                for index in 0..surface_len {
                    let surface_id = pane.surface_ids.get(index).cloned().unwrap_or_default();
                    let surface_handle = pane
                        .surface_refs
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| surface_id.clone());
                    if surface == surface_id || surface == surface_handle {
                        return Ok(Some(CmuxSurfaceLocation {
                            workspace_id: workspace.id,
                            workspace_handle: workspace.handle,
                            pane_id: pane.id,
                            pane_handle: pane.handle,
                            surface_id,
                            surface_handle,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    pub fn new_surface(&self, pane: &str, workspace: &str) -> Result<String> {
        let out = self.runner.run(
            "cmux",
            &["new-surface", "--pane", pane, "--workspace", workspace],
            None,
        )?;
        if !out.success {
            bail!("cmux new-surface failed: {}", command_error(&out));
        }
        extract_handle(&out.stdout, "surface:", "new-surface")
    }

    pub fn new_surface_with_focus(
        &self,
        pane: &str,
        workspace: &str,
        focus: bool,
    ) -> Result<String> {
        let focus = focus.to_string();
        let out = self.runner.run(
            "cmux",
            &[
                "new-surface",
                "--pane",
                pane,
                "--workspace",
                workspace,
                "--focus",
                &focus,
            ],
            None,
        )?;
        if !out.success {
            bail!("cmux new-surface failed: {}", command_error(&out));
        }
        extract_handle(&out.stdout, "surface:", "new-surface")
    }

    pub fn close_surface(&self, surface: &str, workspace: Option<&str>) -> Result<()> {
        let mut args = vec![
            "close-surface".to_string(),
            "--surface".into(),
            surface.into(),
        ];
        if let Some(workspace) = workspace {
            args.extend(["--workspace".into(), workspace.into()]);
        }
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let out = self.runner.run("cmux", &arg_refs, None)?;
        if !out.success {
            bail!("cmux close-surface failed: {}", command_error(&out));
        }
        Ok(())
    }

    pub fn send(&self, surface: &str, workspace: &str, text: &str) -> Result<()> {
        let out = self.runner.run(
            "cmux",
            &["send", "--surface", surface, "--workspace", workspace, text],
            None,
        )?;
        if !out.success {
            bail!("cmux send failed: {}", command_error(&out));
        }
        Ok(())
    }

    pub fn send_key(&self, surface: &str, workspace: &str, key: &str) -> Result<()> {
        let out = self.runner.run(
            "cmux",
            &[
                "send-key",
                "--surface",
                surface,
                "--workspace",
                workspace,
                key,
            ],
            None,
        )?;
        if !out.success {
            bail!("cmux send-key failed: {}", command_error(&out));
        }
        Ok(())
    }

    pub fn set_color(&self, workspace: &str, color: &str) -> Result<()> {
        let out = self.runner.run(
            "cmux",
            &[
                "workspace-action",
                "--workspace",
                workspace,
                "--action",
                "set-color",
                "--color",
                color,
            ],
            None,
        )?;
        if !out.success {
            bail!(
                "cmux workspace-action set-color failed: {}",
                command_error(&out)
            );
        }
        Ok(())
    }

    pub fn read_screen(&self, surface: &str, workspace: &str) -> Result<String> {
        self.read_screen_with_lines(surface, workspace, None)
    }

    pub fn read_screen_lines(
        &self,
        surface: &str,
        workspace: &str,
        lines: usize,
    ) -> Result<String> {
        self.read_screen_with_lines(surface, workspace, Some(lines))
    }

    pub fn read_screen_with_lines(
        &self,
        surface: &str,
        workspace: &str,
        lines: Option<usize>,
    ) -> Result<String> {
        let mut args = vec![
            "read-screen".to_string(),
            "--surface".into(),
            surface.into(),
            "--workspace".into(),
            workspace.into(),
        ];
        if let Some(lines) = lines {
            args.extend(["--lines".into(), lines.to_string()]);
        }
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let out = self.runner.run("cmux", &arg_refs, None)?;
        if !out.success {
            bail!("cmux read-screen failed: {}", command_error(&out));
        }
        Ok(out.stdout)
    }

    pub fn list_status(&self, workspace: &str) -> Result<Vec<CmuxStatusEntry>> {
        let out = self
            .runner
            .run("cmux", &["list-status", "--workspace", workspace], None)?;
        if !out.success {
            bail!("cmux list-status failed: {}", command_error(&out));
        }
        parse_status_entries(&out.stdout)
    }

    pub fn surface_processes(&self, workspace: &str) -> Result<Vec<CmuxSurfaceProcesses>> {
        let out = self.runner.run(
            "cmux",
            &["top", "--workspace", workspace, "--processes", "--json"],
            None,
        )?;
        if !out.success {
            bail!("cmux top failed: {}", command_error(&out));
        }
        parse_surface_processes(&out.stdout)
    }

    pub fn replay_events_after(
        &self,
        after_seq: u64,
        limit: usize,
        timeout: Duration,
    ) -> Result<Vec<CmuxEvent>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let after_seq = after_seq.to_string();
        let limit = limit.to_string();
        let out = self.runner.run_with_timeout(
            "cmux",
            &[
                "events",
                "--after",
                &after_seq,
                "--limit",
                &limit,
                "--no-ack",
                "--no-heartbeat",
            ],
            None,
            timeout,
        )?;
        if !out.success && !command_timed_out(&out) {
            bail!("cmux events failed: {}", command_error(&out));
        }
        parse_event_records(&out.stdout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CmuxWindow {
    id: String,
}

fn command_error(out: &crate::context::CmdOutput) -> String {
    match (out.stderr.trim().is_empty(), out.stdout.trim().is_empty()) {
        (false, false) => format!(
            "stderr: {}; stdout: {}",
            out.stderr.trim(),
            out.stdout.trim()
        ),
        (false, true) => out.stderr.trim().to_string(),
        (true, false) => out.stdout.trim().to_string(),
        (true, true) => "empty output".into(),
    }
}

fn command_timed_out(out: &crate::context::CmdOutput) -> bool {
    out.stderr
        .lines()
        .any(|line| line.trim_start().starts_with("timed out after "))
}

fn extract_handle(stdout: &str, prefix: &str, operation: &str) -> Result<String> {
    let handle_name = prefix.trim_end_matches(':');
    let output = stdout.trim();
    let output = if output.is_empty() {
        "empty output"
    } else {
        output
    };
    let parts: Vec<&str> = stdout.split_whitespace().collect();
    let handle = parts
        .get(1)
        .copied()
        .filter(|part| part.starts_with(prefix))
        .or_else(|| {
            parts
                .first()
                .copied()
                .filter(|part| part.starts_with(prefix))
        })
        .or_else(|| parts.iter().copied().find(|part| part.starts_with(prefix)));

    handle.map(String::from).ok_or_else(|| {
        anyhow::anyhow!("cmux {operation} did not return {handle_name} handle: {output}")
    })
}

#[derive(Debug, Deserialize)]
struct WindowListResponse {
    windows: Vec<RpcWindow>,
}

#[derive(Debug, Deserialize)]
struct RpcWindow {
    id: String,
}

impl From<RpcWindow> for CmuxWindow {
    fn from(window: RpcWindow) -> Self {
        Self { id: window.id }
    }
}

#[derive(Debug, Deserialize)]
struct IdentifyResponse {
    caller: Option<IdentifyContext>,
    focused: Option<IdentifyContext>,
}

#[derive(Debug, Deserialize)]
struct IdentifyContext {
    window_ref: Option<String>,
    workspace_ref: Option<String>,
    pane_ref: Option<String>,
    surface_ref: Option<String>,
}

impl From<IdentifyResponse> for CmuxIdentity {
    fn from(response: IdentifyResponse) -> Self {
        Self {
            caller: response.caller.map(Into::into),
            focused: response.focused.map(Into::into),
        }
    }
}

impl From<IdentifyContext> for CmuxCaller {
    fn from(caller: IdentifyContext) -> Self {
        Self {
            window: caller.window_ref,
            workspace: caller.workspace_ref,
            pane: caller.pane_ref,
            surface: caller.surface_ref,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResponse {
    window_id: String,
    window_ref: String,
    workspaces: Vec<RpcWorkspace>,
}

#[derive(Debug, Deserialize)]
struct RpcWorkspace {
    id: String,
    #[serde(rename = "ref")]
    handle: String,
    title: String,
    current_directory: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct PaneListResponse {
    panes: Vec<RpcPane>,
}

#[derive(Debug, Deserialize)]
struct RpcPane {
    id: String,
    #[serde(rename = "ref")]
    handle: String,
    selected_surface_id: Option<String>,
    selected_surface_ref: Option<String>,
    #[serde(default)]
    surface_ids: Vec<String>,
    #[serde(default)]
    surface_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EventFrame {
    seq: u64,
    name: String,
    category: Option<String>,
    occurred_at: Option<String>,
    window_id: Option<String>,
    workspace_id: Option<String>,
    pane_id: Option<String>,
    surface_id: Option<String>,
    #[serde(default)]
    payload: Value,
}

impl From<EventFrame> for CmuxEvent {
    fn from(frame: EventFrame) -> Self {
        Self {
            seq: frame.seq,
            name: frame.name,
            category: frame.category,
            occurred_at: frame.occurred_at,
            window_id: frame.window_id,
            workspace_id: frame.workspace_id,
            pane_id: frame.pane_id,
            surface_id: frame.surface_id,
            payload: frame.payload,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TopResponse {
    #[serde(default)]
    windows: Vec<TopWindow>,
}

#[derive(Debug, Deserialize)]
struct TopWindow {
    #[serde(default)]
    workspaces: Vec<TopWorkspace>,
}

#[derive(Debug, Deserialize)]
struct TopWorkspace {
    #[serde(default)]
    panes: Vec<TopPane>,
}

#[derive(Debug, Deserialize)]
struct TopPane {
    #[serde(default)]
    surfaces: Vec<TopSurface>,
}

#[derive(Debug, Deserialize)]
struct TopSurface {
    #[serde(rename = "ref")]
    surface_ref: String,
    #[serde(default)]
    processes: Vec<TopProcess>,
}

#[derive(Debug, Deserialize)]
struct TopProcess {
    #[serde(default)]
    name: String,
    path: Option<String>,
    #[serde(default)]
    children: Vec<TopProcess>,
}

impl TopProcess {
    fn collect_processes(&self, processes: &mut Vec<CmuxProcessInfo>) {
        processes.push(CmuxProcessInfo {
            name: self.name.clone(),
            path: self.path.clone(),
        });
        for child in &self.children {
            child.collect_processes(processes);
        }
    }
}

fn parse_surface_processes(stdout: &str) -> Result<Vec<CmuxSurfaceProcesses>> {
    let response: TopResponse = serde_json::from_str(stdout)?;
    let mut surfaces = Vec::new();
    for window in response.windows {
        for workspace in window.workspaces {
            for pane in workspace.panes {
                for surface in pane.surfaces {
                    let mut processes = Vec::new();
                    for process in &surface.processes {
                        process.collect_processes(&mut processes);
                    }
                    surfaces.push(CmuxSurfaceProcesses {
                        surface: surface.surface_ref,
                        processes,
                    });
                }
            }
        }
    }
    Ok(surfaces)
}

fn parse_status_entries(stdout: &str) -> Result<Vec<CmuxStatusEntry>> {
    stdout
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            Some(parse_status_entry(index + 1, line))
        })
        .collect()
}

fn parse_status_entry(line_number: usize, line: &str) -> Result<CmuxStatusEntry> {
    let mut parts = line.split_whitespace();
    let first = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("cmux list-status line {line_number} is empty"))?;
    let (key, value) = first.split_once('=').ok_or_else(|| {
        anyhow::anyhow!("cmux list-status line {line_number} missing status entry: {line}")
    })?;
    if key.is_empty() || value.is_empty() {
        bail!("cmux list-status line {line_number} has empty status entry: {line}");
    }

    let mut entry = CmuxStatusEntry {
        key: key.into(),
        value: value.into(),
        icon: None,
        color: None,
    };

    for part in parts {
        let Some((field, value)) = part.split_once('=') else {
            continue;
        };
        match field {
            "icon" => entry.icon = Some(value.into()),
            "color" => entry.color = Some(value.into()),
            _ => {}
        }
    }

    Ok(entry)
}

fn parse_event_records(stdout: &str) -> Result<Vec<CmuxEvent>> {
    stdout
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            Some(
                serde_json::from_str::<EventFrame>(line)
                    .with_context(|| format!("failed to parse cmux event frame {}", index + 1))
                    .map(Into::into),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn new_workspace_extracts_handle() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"caller":{"window_ref":"window:1"}}"#, true);
        runner.add_response("workspace:1 workspace:1", true);

        let svc = CmuxService::new(&runner);
        let handle = svc
            .new_workspace(Path::new("/tmp"), "my ws", "bash")
            .unwrap();
        assert_eq!(handle, "workspace:1");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["identify"]);
        assert_eq!(
            calls[1].1,
            vec![
                "new-workspace",
                "--cwd",
                "/tmp",
                "--name",
                "my ws",
                "--command",
                "bash",
                "--window",
                "window:1",
                "--focus",
                "true"
            ]
        );
    }

    #[test]
    fn new_workspace_omits_window_when_caller_is_unknown() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"caller":null}"#, true);
        runner.add_response("workspace:1 workspace:1", true);

        let svc = CmuxService::new(&runner);
        svc.new_workspace(Path::new("/tmp"), "my ws", "bash")
            .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[1].1,
            vec![
                "new-workspace",
                "--cwd",
                "/tmp",
                "--name",
                "my ws",
                "--command",
                "bash",
                "--focus",
                "true"
            ]
        );
    }

    #[test]
    fn new_workspace_with_caller_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("ignored stdout", "cmux workspace failed", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .new_workspace_with_caller(Path::new("/tmp"), "my ws", "bash", None)
            .expect_err("new-workspace failure should propagate");
        let message = err.to_string();
        assert!(message.contains("cmux new-workspace failed"));
        assert!(message.contains("cmux workspace failed"));
        assert!(message.contains("ignored stdout"));
    }

    #[test]
    fn new_workspace_with_caller_rejects_missing_handle() {
        let mut runner = MockRunner::new();
        runner.add_response("created workspace without handle", true);

        let svc = CmuxService::new(&runner);
        let err = svc
            .new_workspace_with_caller(Path::new("/tmp"), "my ws", "bash", None)
            .expect_err("new-workspace output without a workspace handle should fail");
        let message = err.to_string();
        assert!(message.contains("cmux new-workspace did not return workspace handle"));
        assert!(message.contains("created workspace without handle"));
    }

    #[test]
    fn caller_context_reads_surface_ref() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:2","pane_ref":"pane:3","surface_ref":"surface:4"}}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let caller = svc.caller_context().unwrap();

        assert_eq!(caller.window.as_deref(), Some("window:1"));
        assert_eq!(caller.workspace.as_deref(), Some("workspace:2"));
        assert_eq!(caller.pane.as_deref(), Some("pane:3"));
        assert_eq!(caller.surface.as_deref(), Some("surface:4"));
    }

    #[test]
    fn identity_context_reads_focused_surface_ref() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:2","surface_ref":"surface:4"},"focused":{"window_ref":"window:1","workspace_ref":"workspace:2","pane_ref":"pane:3","surface_ref":"surface:5"}}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let identity = svc.identity_context().unwrap();

        assert_eq!(
            identity.caller.unwrap().surface.as_deref(),
            Some("surface:4")
        );
        let focused = identity.focused.unwrap();
        assert_eq!(focused.workspace.as_deref(), Some("workspace:2"));
        assert_eq!(focused.pane.as_deref(), Some("pane:3"));
        assert_eq!(focused.surface.as_deref(), Some("surface:5"));
    }

    #[test]
    fn list_panes_filters_pane_ids() {
        let mut runner = MockRunner::new();
        runner.add_response("pane:0 extra pane:1 data", true);

        let svc = CmuxService::new(&runner);
        let panes = svc.list_panes("workspace:1").unwrap();
        assert_eq!(panes, vec!["pane:0", "pane:1"]);
    }

    #[test]
    fn list_panes_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("workspace missing", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .list_panes("workspace:1")
            .expect_err("list-panes failure should propagate");
        let message = err.to_string();
        assert!(message.contains("cmux list-panes failed"));
        assert!(message.contains("workspace missing"));
    }

    #[test]
    fn list_workspaces_reads_rpc_windows_and_workspace_directories() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"},{"id":"uuid-window-2","ref":"window:2"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-1","ref":"workspace:1","title":"repo","current_directory":"/tmp/repo"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-2","window_ref":"window:2","workspaces":[{"id":"uuid-workspace-2","ref":"workspace:2","title":"other","current_directory":null}]}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let workspaces = svc.list_workspaces().unwrap();

        assert_eq!(
            workspaces,
            vec![
                CmuxWorkspace {
                    id: "uuid-workspace-1".into(),
                    handle: "workspace:1".into(),
                    window_id: "uuid-window-1".into(),
                    window_handle: "window:1".into(),
                    title: "repo".into(),
                    current_directory: Some(PathBuf::from("/tmp/repo")),
                },
                CmuxWorkspace {
                    id: "uuid-workspace-2".into(),
                    handle: "workspace:2".into(),
                    window_id: "uuid-window-2".into(),
                    window_handle: "window:2".into(),
                    title: "other".into(),
                    current_directory: None,
                },
            ]
        );

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["rpc", "window.list", "{}"]);
        assert_eq!(
            calls[1].1,
            vec!["rpc", "workspace.list", r#"{"window_id":"uuid-window-1"}"#]
        );
        assert_eq!(
            calls[2].1,
            vec!["rpc", "workspace.list", r#"{"window_id":"uuid-window-2"}"#]
        );
    }

    #[test]
    fn close_workspace_passes_id() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.close_workspace("uuid-workspace-10").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "rpc",
                "workspace.close",
                r#"{"workspace_id":"uuid-workspace-10"}"#
            ]
        );
    }

    #[test]
    fn select_workspace_passes_id() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.select_workspace("uuid-workspace-10").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "rpc",
                "workspace.select",
                r#"{"workspace_id":"uuid-workspace-10"}"#
            ]
        );
    }

    #[test]
    fn focus_surface_passes_surface_and_workspace_refs() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.focus_surface("surface:10", Some("workspace:2"))
            .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1[0], "rpc");
        assert_eq!(calls[0].1[1], "surface.focus");
        let params: serde_json::Value = serde_json::from_str(&calls[0].1[2]).unwrap();
        assert_eq!(params["surface_id"], "surface:10");
        assert_eq!(params["workspace_id"], "workspace:2");
    }

    #[test]
    fn selected_surfaces_extracts_pane_selected_surface_refs() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace","workspace_ref":"workspace:2","panes":[{"id":"uuid-pane-1","ref":"pane:3","selected_surface_id":"uuid-surface-1","selected_surface_ref":"surface:4"},{"id":"uuid-pane-2","ref":"pane:4","selected_surface_id":null,"selected_surface_ref":null}]}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let surfaces = svc.selected_surfaces("workspace:2").unwrap();

        assert_eq!(
            surfaces,
            vec![CmuxPaneSelectedSurface {
                pane_id: "uuid-pane-1".into(),
                pane_handle: "pane:3".into(),
                selected_surface_id: "uuid-surface-1".into(),
                selected_surface_handle: "surface:4".into(),
            }]
        );
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["rpc", "pane.list", r#"{"workspace_id":"workspace:2"}"#]
        );
    }

    #[test]
    fn find_surface_location_scans_workspace_panes() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"windows":[{"id":"uuid-window-1"}]}"#, true);
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-1","ref":"workspace:1","title":"repo","current_directory":"/tmp/repo"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-1","ref":"pane:1","surface_ids":["uuid-surface-1","uuid-surface-2"],"surface_refs":["surface:1","surface:2"]}]}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let location = svc.find_surface_location("surface:2").unwrap().unwrap();

        assert_eq!(
            location,
            CmuxSurfaceLocation {
                workspace_id: "uuid-workspace-1".into(),
                workspace_handle: "workspace:1".into(),
                pane_id: "uuid-pane-1".into(),
                pane_handle: "pane:1".into(),
                surface_id: "uuid-surface-2".into(),
                surface_handle: "surface:2".into(),
            }
        );
    }

    #[test]
    fn list_pane_surfaces_filters_surface_ids() {
        let mut runner = MockRunner::new();
        runner.add_response("surface:0 other surface:1 data", true);

        let svc = CmuxService::new(&runner);
        let surfaces = svc.list_pane_surfaces("pane:1", "workspace:1").unwrap();
        assert_eq!(surfaces, vec!["surface:0", "surface:1"]);
    }

    #[test]
    fn list_pane_surfaces_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("ignored stdout", "pane surface lookup failed", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .list_pane_surfaces("pane:1", "workspace:1")
            .expect_err("list-pane-surfaces failure should propagate");
        let message = err.to_string();
        assert!(message.contains("cmux list-pane-surfaces failed"));
        assert!(message.contains("pane surface lookup failed"));
        assert!(message.contains("ignored stdout"));
    }

    #[test]
    fn new_surface_extracts_handle() {
        let mut runner = MockRunner::new();
        runner.add_response("surface:4 surface:4", true);

        let svc = CmuxService::new(&runner);
        let surface = svc.new_surface("pane:3", "workspace:2").unwrap();
        assert_eq!(surface, "surface:4");
    }

    #[test]
    fn new_surface_with_focus_passes_focus_flag() {
        let mut runner = MockRunner::new();
        runner.add_response("OK surface:4", true);

        let svc = CmuxService::new(&runner);
        let surface = svc
            .new_surface_with_focus("pane:3", "workspace:2", false)
            .unwrap();
        assert_eq!(surface, "surface:4");

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "new-surface",
                "--pane",
                "pane:3",
                "--workspace",
                "workspace:2",
                "--focus",
                "false"
            ]
        );
    }

    #[test]
    fn close_surface_passes_workspace_when_known() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.close_surface("surface:4", Some("workspace:2")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "close-surface",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2"
            ]
        );
    }

    #[test]
    fn new_surface_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("ignored stdout", "new surface failed", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .new_surface("pane:3", "workspace:2")
            .expect_err("new-surface failure should propagate");
        let message = err.to_string();
        assert!(message.contains("cmux new-surface failed"));
        assert!(message.contains("new surface failed"));
        assert!(message.contains("ignored stdout"));
    }

    #[test]
    fn new_surface_rejects_malformed_output() {
        let mut runner = MockRunner::new();
        runner.add_response("created surface without handle", true);

        let svc = CmuxService::new(&runner);
        let err = svc
            .new_surface("pane:3", "workspace:2")
            .expect_err("new-surface output without a surface handle should fail");
        let message = err.to_string();
        assert!(message.contains("cmux new-surface did not return surface handle"));
        assert!(message.contains("created surface without handle"));
    }

    #[test]
    fn open_command_surface_uses_caller_workspace_and_pane() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:2","pane_ref":"pane:3"}}"#,
            true,
        );
        runner.add_response("surface:4 surface:4", true);
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        let surface = svc.open_command_surface("vi file.toml").unwrap();

        assert_eq!(surface, "surface:4");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["identify"]);
        assert_eq!(
            calls[1].1,
            vec![
                "new-surface",
                "--pane",
                "pane:3",
                "--workspace",
                "workspace:2"
            ]
        );
        assert_eq!(
            calls[2].1,
            vec![
                "send",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2",
                "vi file.toml\n"
            ]
        );
    }

    #[test]
    fn send_passes_text_to_surface() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.send("surface:0", "workspace:1", "lazygit\n").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1[5], "lazygit\n");
    }

    #[test]
    fn send_key_passes_key_to_surface() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.send_key("surface:0", "workspace:1", "enter").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "send-key",
                "--surface",
                "surface:0",
                "--workspace",
                "workspace:1",
                "enter"
            ]
        );
    }

    #[test]
    fn set_color_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("ignored stdout", "color rejected", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .set_color("workspace:1", "#ff00aa")
            .expect_err("set-color failure should propagate");
        let message = err.to_string();
        assert!(message.contains("cmux workspace-action set-color failed"));
        assert!(message.contains("color rejected"));
        assert!(message.contains("ignored stdout"));
    }

    #[test]
    fn read_screen_passes_line_limit() {
        let mut runner = MockRunner::new();
        runner.add_response("last lines", true);

        let svc = CmuxService::new(&runner);
        let screen = svc
            .read_screen_lines("surface:4", "workspace:2", 30)
            .unwrap();

        assert_eq!(screen, "last lines");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "read-screen",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2",
                "--lines",
                "30"
            ]
        );
    }

    #[test]
    fn read_screen_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("Terminal surface not found", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .read_screen("surface:0", "workspace:1")
            .expect_err("read-screen failure should propagate");
        assert!(err.to_string().contains("cmux read-screen failed"));
    }

    #[test]
    fn list_status_parses_sidebar_status_entries() {
        let mut runner = MockRunner::new();
        runner.add_response(
            "codex=Idle icon=pause.circle.fill color=#8E8E93\nclaude_code=Running icon=bolt.fill color=#4C8DFF",
            true,
        );

        let svc = CmuxService::new(&runner);
        let entries = svc.list_status("workspace:58").unwrap();

        assert_eq!(
            entries,
            vec![
                CmuxStatusEntry {
                    key: "codex".into(),
                    value: "Idle".into(),
                    icon: Some("pause.circle.fill".into()),
                    color: Some("#8E8E93".into()),
                },
                CmuxStatusEntry {
                    key: "claude_code".into(),
                    value: "Running".into(),
                    icon: Some("bolt.fill".into()),
                    color: Some("#4C8DFF".into()),
                },
            ]
        );
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["list-status", "--workspace", "workspace:58"]
        );
    }

    #[test]
    fn surface_processes_parses_cmux_top_json() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"windows":[{"workspaces":[{"panes":[{"surfaces":[{"ref":"surface:4","processes":[{"name":"login","path":"/usr/bin/login","children":[{"name":"zsh","path":"/bin/zsh","children":[{"name":"codex","path":"/opt/codex/bin/codex","children":[]}]}]}]},{"ref":"surface:5","processes":[{"name":"lazygit","path":"/opt/homebrew/bin/lazygit","children":[]}]}]}]}]}]}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let surfaces = svc.surface_processes("workspace:58").unwrap();

        assert_eq!(surfaces.len(), 2);
        assert_eq!(surfaces[0].surface, "surface:4");
        assert_eq!(
            surfaces[0]
                .processes
                .iter()
                .map(|process| process.name.as_str())
                .collect::<Vec<_>>(),
            vec!["login", "zsh", "codex"]
        );
        assert_eq!(surfaces[1].surface, "surface:5");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "top",
                "--workspace",
                "workspace:58",
                "--processes",
                "--json"
            ]
        );
    }

    #[test]
    fn cmux_failures_include_stderr_and_stdout_context() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("status stdout", "status stderr", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .list_status("workspace:58")
            .expect_err("list-status failure should propagate");
        let message = err.to_string();

        assert!(message.contains("cmux list-status failed"));
        assert!(message.contains("status stderr"));
        assert!(message.contains("status stdout"));
    }

    #[test]
    fn replay_events_after_uses_bounded_no_ack_command() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"seq":4701,"name":"surface.input_sent","category":"surface","occurred_at":"2026-05-16T07:59:09.500Z","window_id":"uuid-window","workspace_id":"uuid-workspace","pane_id":null,"surface_id":"uuid-surface","payload":{"result":{"workspace_ref":"workspace:58","surface_ref":"surface:182"}}}
{"seq":4702,"name":"sidebar.metadata.updated","category":"sidebar","workspace_id":"uuid-workspace","surface_id":"uuid-surface","payload":{"command":"set_status codex Idle"}}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let events = svc
            .replay_events_after(4700, 25, Duration::from_secs(2))
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 4701);
        assert_eq!(events[0].name, "surface.input_sent");
        assert_eq!(events[0].workspace_id.as_deref(), Some("uuid-workspace"));
        assert_eq!(events[0].surface_id.as_deref(), Some("uuid-surface"));
        assert_eq!(events[0].payload["result"]["workspace_ref"], "workspace:58");
        assert_eq!(events[1].category.as_deref(), Some("sidebar"));

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "events",
                "--after",
                "4700",
                "--limit",
                "25",
                "--no-ack",
                "--no-heartbeat"
            ]
        );
    }

    #[test]
    fn replay_events_after_returns_partial_records_on_timeout() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr(
            r#"{"seq":4701,"name":"surface.input_sent","category":"surface","payload":{}}"#,
            "timed out after 2000ms",
            false,
        );

        let svc = CmuxService::new(&runner);
        let events = svc
            .replay_events_after(4700, 25, Duration::from_secs(2))
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 4701);
    }
}
