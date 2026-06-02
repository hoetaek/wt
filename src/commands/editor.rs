use crate::config::{EditorConfig, EditorPlacement};
use crate::context::{CmdOutput, Ctx};
use crate::services::cmux::CmuxService;
use crate::template;
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn open_file(ctx: &Ctx, path: &Path) -> Result<()> {
    let path = normalize_path(path);
    let command = render_editor_command(&ctx.config.editor, &path);

    match ctx.config.editor.effective_placement() {
        EditorPlacement::CmuxSurface => open_in_cmux_surface(ctx, &command, &path),
        EditorPlacement::Process => open_as_process(ctx, &command),
    }
}

fn open_in_cmux_surface(ctx: &Ctx, command: &str, path: &Path) -> Result<()> {
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui
            .print_warning("cmux command not found; opening editor as a process");
        return open_as_process(ctx, command);
    }

    ctx.ui
        .print_step(&format!("Opening editor: {}", path.display()));
    match cmux.open_command_surface(command) {
        Ok(_) => Ok(()),
        Err(err) => {
            ctx.ui.print_warning(&format!(
                "cmux editor surface failed: {err}; opening editor as a process"
            ));
            open_as_process(ctx, command)
        }
    }
}

fn open_as_process(ctx: &Ctx, command: &str) -> Result<()> {
    let out = ctx
        .runner
        .run("sh", &["-lc", command], Some(&ctx.invocation_root))?;
    if !out.success {
        bail!("editor command failed: {}", command_error(&out));
    }
    Ok(())
}

fn render_editor_command(config: &EditorConfig, path: &Path) -> String {
    let command = config
        .command
        .clone()
        .or_else(env_editor_command)
        .unwrap_or_else(|| "vi {{path}}".into());
    let template = if command.contains("{{path") {
        command
    } else {
        format!("{command} {{{{path}}}}")
    };

    let raw_path = path.to_string_lossy().into_owned();
    let mut vars = HashMap::new();
    vars.insert("path".into(), shell_quote(&raw_path));
    vars.insert("path_raw".into(), raw_path);
    template::render(&template, &vars)
}

fn env_editor_command() -> Option<String> {
    std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn command_error(out: &CmdOutput) -> &str {
    if out.stderr.is_empty() {
        &out.stdout
    } else {
        &out.stderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
    use std::path::Path;
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

    fn make_ctx(config: Config, runner: Arc<MockRunner>) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(SharedRunner { inner: runner }),
            Box::new(MockUi::new()),
        )
    }

    #[test]
    fn render_editor_command_appends_quoted_path_when_missing_placeholder() {
        let config = EditorConfig {
            command: Some("code --reuse-window".into()),
            placement: None,
        };

        assert_eq!(
            render_editor_command(&config, Path::new("/tmp/my file.toml")),
            "code --reuse-window '/tmp/my file.toml'"
        );
    }

    #[test]
    fn open_file_uses_cmux_surface_by_default() {
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:2","pane_ref":"pane:3","surface_ref":"surface:3"}}"#,
            true,
        );
        runner.add_response("OK surface:4 workspace:2", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut config = Config::default();
        config.editor.command = Some("vi {{path}}".into());
        let ctx = make_ctx(config, Arc::clone(&runner));

        open_file(&ctx, Path::new("/tmp/batch.toml")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "cmux");
        assert_eq!(
            calls[1].1,
            vec![
                "new-split",
                "right",
                "--workspace",
                "workspace:2",
                "--surface",
                "surface:3"
            ]
        );
        assert_eq!(calls[2].0, "cmux");
        assert_eq!(
            calls[2].1,
            vec![
                "send",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2",
                "--",
                "vi '/tmp/batch.toml'\n"
            ]
        );
    }

    #[test]
    fn open_file_can_run_editor_as_process() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut config = Config::default();
        config.editor.command = Some("code {{path}}".into());
        config.editor.placement = Some(EditorPlacement::Process);
        let ctx = make_ctx(config, Arc::clone(&runner));

        open_file(&ctx, Path::new("/tmp/config.toml")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "sh");
        assert_eq!(calls[0].1, vec!["-lc", "code '/tmp/config.toml'"]);
    }

    #[test]
    fn open_file_falls_back_to_process_when_cmux_context_is_missing() {
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(r#"{"caller":null}"#, true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut config = Config::default();
        config.editor.command = Some("vi {{path}}".into());
        let ctx = make_ctx(config, Arc::clone(&runner));

        open_file(&ctx, Path::new("/tmp/config.toml")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "cmux");
        assert_eq!(calls[0].1, vec!["identify"]);
        assert_eq!(calls[1].0, "sh");
        assert_eq!(calls[1].1, vec!["-lc", "vi '/tmp/config.toml'"]);
    }
}
