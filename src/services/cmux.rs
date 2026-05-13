use crate::context::CommandRunner;
use anyhow::Result;
use std::path::Path;

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
