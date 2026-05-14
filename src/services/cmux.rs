use crate::context::CommandRunner;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxWorkspace {
    pub handle: String,
    pub title: String,
    pub current_directory: Option<PathBuf>,
}

pub struct CmuxService<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> CmuxService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    pub fn is_available(&self) -> bool {
        self.runner.has_command("cmux")
    }

    pub fn new_workspace(&self, cwd: &Path, name: &str, command: &str) -> Result<String> {
        let out = self.runner.run(
            "cmux",
            &[
                "new-workspace",
                "--cwd",
                &cwd.to_string_lossy(),
                "--name",
                name,
                "--command",
                command,
                "--focus",
                "true",
            ],
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

    pub fn list_workspaces(&self) -> Result<Vec<CmuxWorkspace>> {
        let windows = self.list_windows()?;
        let mut workspaces = Vec::new();

        for window in windows {
            let out = self.runner.run(
                "cmux",
                &["--window", &window.handle, "rpc", "workspace.list", "{}"],
                None,
            )?;
            if !out.success {
                bail!("cmux workspace.list failed: {}", command_error(&out));
            }
            let response: WorkspaceListResponse = serde_json::from_str(&out.stdout)?;
            workspaces.extend(response.workspaces.into_iter().map(Into::into));
        }

        Ok(workspaces)
    }

    pub fn close_workspace(&self, workspace: &str) -> Result<()> {
        let out = self
            .runner
            .run("cmux", &["close-workspace", "--workspace", workspace], None)?;
        if !out.success {
            bail!("cmux close-workspace failed: {}", command_error(&out));
        }
        Ok(())
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
        self.runner.run(
            "cmux",
            &["send", "--surface", surface, "--workspace", workspace, text],
            None,
        )?;
        Ok(())
    }

    pub fn send_key(&self, surface: &str, workspace: &str, key: &str) -> Result<()> {
        self.runner.run(
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
        Ok(out.stdout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CmuxWindow {
    handle: String,
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
    #[serde(rename = "ref")]
    handle: String,
}

impl From<RpcWindow> for CmuxWindow {
    fn from(window: RpcWindow) -> Self {
        Self {
            handle: window.handle,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResponse {
    workspaces: Vec<RpcWorkspace>,
}

#[derive(Debug, Deserialize)]
struct RpcWorkspace {
    #[serde(rename = "ref")]
    handle: String,
    title: String,
    current_directory: Option<PathBuf>,
}

impl From<RpcWorkspace> for CmuxWorkspace {
    fn from(workspace: RpcWorkspace) -> Self {
        Self {
            handle: workspace.handle,
            title: workspace.title,
            current_directory: workspace.current_directory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn new_workspace_extracts_handle() {
        let mut runner = MockRunner::new();
        runner.add_response("workspace:1 workspace:1", true);

        let svc = CmuxService::new(&runner);
        let handle = svc
            .new_workspace(Path::new("/tmp"), "my ws", "bash")
            .unwrap();
        assert_eq!(handle, "workspace:1");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
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
            r#"{"windows":[{"ref":"window:1"},{"ref":"window:2"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"workspaces":[{"ref":"workspace:1","title":"repo","current_directory":"/tmp/repo"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"workspaces":[{"ref":"workspace:2","title":"other","current_directory":null}]}"#,
            true,
        );

        let svc = CmuxService::new(&runner);
        let workspaces = svc.list_workspaces().unwrap();

        assert_eq!(
            workspaces,
            vec![
                CmuxWorkspace {
                    handle: "workspace:1".into(),
                    title: "repo".into(),
                    current_directory: Some(PathBuf::from("/tmp/repo")),
                },
                CmuxWorkspace {
                    handle: "workspace:2".into(),
                    title: "other".into(),
                    current_directory: None,
                },
            ]
        );

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["rpc", "window.list", "{}"]);
        assert_eq!(
            calls[1].1,
            vec!["--window", "window:1", "rpc", "workspace.list", "{}"]
        );
        assert_eq!(
            calls[2].1,
            vec!["--window", "window:2", "rpc", "workspace.list", "{}"]
        );
    }

    #[test]
    fn close_workspace_passes_handle() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = CmuxService::new(&runner);
        svc.close_workspace("workspace:10").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["close-workspace", "--workspace", "workspace:10"]
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
}
