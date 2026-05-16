use crate::context::CommandRunner;
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;

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
pub struct CmuxCaller {
    pub window: Option<String>,
    pub workspace: Option<String>,
    pub pane: Option<String>,
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
        if let Some(window) = self.caller_context().and_then(|caller| caller.window) {
            args.extend(["--window".into(), window]);
        }
        args.extend(["--focus".into(), self.focus_new_workspace.to_string()]);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let out = self.runner.run("cmux", &arg_refs, None)?;
        let handle = out
            .stdout
            .split_whitespace()
            .nth(1)
            .unwrap_or(&out.stdout)
            .to_string();
        Ok(handle)
    }

    pub fn caller_context(&self) -> Option<CmuxCaller> {
        let out = self.runner.run("cmux", &["identify"], None).ok()?;
        if !out.success {
            return None;
        }

        let response: IdentifyResponse = serde_json::from_str(&out.stdout).ok()?;
        response.caller.map(Into::into)
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
        let panes: Vec<String> = out
            .stdout
            .split_whitespace()
            .filter(|s| s.starts_with("pane:"))
            .map(String::from)
            .collect();
        Ok(panes)
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
        let surfaces: Vec<String> = out
            .stdout
            .split_whitespace()
            .filter(|s| s.starts_with("surface:"))
            .map(String::from)
            .collect();
        Ok(surfaces)
    }

    pub fn new_surface(&self, pane: &str, workspace: &str) -> Result<String> {
        let out = self.runner.run(
            "cmux",
            &["new-surface", "--pane", pane, "--workspace", workspace],
            None,
        )?;
        let handle = out
            .stdout
            .split_whitespace()
            .nth(1)
            .unwrap_or(&out.stdout)
            .to_string();
        Ok(handle)
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
        self.runner.run(
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
        Ok(())
    }

    pub fn read_screen(&self, surface: &str, workspace: &str) -> Result<String> {
        let out = self.runner.run(
            "cmux",
            &[
                "read-screen",
                "--surface",
                surface,
                "--workspace",
                workspace,
            ],
            None,
        )?;
        if !out.success {
            bail!("cmux read-screen failed: {}", command_error(&out));
        }
        Ok(out.stdout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CmuxWindow {
    id: String,
}

fn command_error(out: &crate::context::CmdOutput) -> &str {
    if out.stderr.is_empty() {
        &out.stdout
    } else {
        &out.stderr
    }
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
    caller: Option<IdentifyCaller>,
}

#[derive(Debug, Deserialize)]
struct IdentifyCaller {
    window_ref: Option<String>,
    workspace_ref: Option<String>,
    pane_ref: Option<String>,
}

impl From<IdentifyCaller> for CmuxCaller {
    fn from(caller: IdentifyCaller) -> Self {
        Self {
            window: caller.window_ref,
            workspace: caller.workspace_ref,
            pane: caller.pane_ref,
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
    fn list_panes_filters_pane_ids() {
        let mut runner = MockRunner::new();
        runner.add_response("pane:0 extra pane:1 data", true);

        let svc = CmuxService::new(&runner);
        let panes = svc.list_panes("workspace:1").unwrap();
        assert_eq!(panes, vec!["pane:0", "pane:1"]);
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
    fn read_screen_reports_cmux_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("Terminal surface not found", false);

        let svc = CmuxService::new(&runner);
        let err = svc
            .read_screen("surface:0", "workspace:1")
            .expect_err("read-screen failure should propagate");
        assert!(err.to_string().contains("cmux read-screen failed"));
    }
}
