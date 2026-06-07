use crate::commands;
use crate::context::Ctx;
use crate::origin_action_menu::OriginAction;
use crate::origin_snapshot::{read_task_snapshot, read_workflow_snapshot};
use crate::task;
use crate::tui::terminal::{TerminalEffects, TerminalSession};
use crate::workflow;
use anyhow::{Context, Result, bail};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::terminal;

/// Where an action's result goes. Terminal actions need the real terminal —
/// network calls, backend gates (confirm/select/input), or long output — so
/// the browser suspends around them. Status-line actions finish locally with
/// a one-line result, so the browser stays up and shows the line in its
/// status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputSink {
    Terminal,
    StatusLine,
}

pub(crate) trait DispatchBackend {
    /// Classify the action. Each backend keeps this match exhaustive so a new
    /// action cannot ship unclassified.
    fn output_sink(&self, action: OriginAction) -> OutputSink;
    /// The CLI command that performs the same work; shown as a `+ <cmd>`
    /// header before terminal-sink output. Only called for terminal actions.
    fn command_hint(&self, action: OriginAction, key: &str) -> String;
    fn run_terminal(&self, action: OriginAction, key: &str) -> Result<()>;
    /// Run a status-line action and return its one-line result without
    /// writing to stdout/stderr.
    fn run_status_line(&self, action: OriginAction, key: &str) -> Result<String>;
}

pub(crate) struct CtxBackend<'a> {
    ctx: &'a Ctx,
}

impl<'a> CtxBackend<'a> {
    pub(crate) fn new(ctx: &'a Ctx) -> Self {
        Self { ctx }
    }
}

impl DispatchBackend for CtxBackend<'_> {
    fn output_sink(&self, action: OriginAction) -> OutputSink {
        match action {
            OriginAction::Diff
            | OriginAction::Fetch
            | OriginAction::Pull
            | OriginAction::Push
            | OriginAction::Publish
            | OriginAction::Attach => OutputSink::Terminal,
            OriginAction::KeepLocal | OriginAction::OpenInBrowser | OriginAction::CopyReference => {
                OutputSink::StatusLine
            }
        }
    }

    fn command_hint(&self, action: OriginAction, key: &str) -> String {
        match action {
            OriginAction::Diff => format!("wt task origin diff {key}"),
            OriginAction::Fetch => format!("wt task origin fetch {key}"),
            OriginAction::Pull => format!("wt task origin pull {key}"),
            OriginAction::Push => format!("wt task origin push {key}"),
            OriginAction::Publish => format!("wt task origin publish {key}"),
            OriginAction::Attach => format!("wt task origin attach {key}"),
            // Status-line actions never render a terminal header.
            OriginAction::KeepLocal | OriginAction::OpenInBrowser | OriginAction::CopyReference => {
                String::new()
            }
        }
    }

    fn run_terminal(&self, action: OriginAction, key: &str) -> Result<()> {
        let tasks = [key.to_string()];
        match action {
            OriginAction::Diff => commands::task_origin::diff(self.ctx, &tasks),
            OriginAction::Fetch => commands::task_origin::fetch(self.ctx, &tasks),
            OriginAction::Pull => commands::task_origin::pull(self.ctx, &tasks),
            OriginAction::Push => commands::task_origin::push(self.ctx, &tasks),
            OriginAction::Publish => commands::task_origin::publish(self.ctx, &tasks),
            OriginAction::Attach => attach_origin(self.ctx, key),
            OriginAction::KeepLocal | OriginAction::OpenInBrowser | OriginAction::CopyReference => {
                bail!("{action:?} is a status-line action")
            }
        }
    }

    fn run_status_line(&self, action: OriginAction, key: &str) -> Result<String> {
        match action {
            OriginAction::KeepLocal => {
                Ok(format!("Keeping {key} local-only; no origin changes made"))
            }
            OriginAction::OpenInBrowser => open_origin_url(self.ctx, key),
            OriginAction::CopyReference => copy_reference(self.ctx, key),
            OriginAction::Diff
            | OriginAction::Fetch
            | OriginAction::Pull
            | OriginAction::Push
            | OriginAction::Publish
            | OriginAction::Attach => bail!("{action:?} is a terminal action"),
        }
    }
}

pub(crate) struct WorkflowCtxBackend<'a> {
    ctx: &'a Ctx,
}

impl<'a> WorkflowCtxBackend<'a> {
    pub(crate) fn new(ctx: &'a Ctx) -> Self {
        Self { ctx }
    }
}

impl DispatchBackend for WorkflowCtxBackend<'_> {
    fn output_sink(&self, action: OriginAction) -> OutputSink {
        match action {
            OriginAction::Diff
            | OriginAction::Fetch
            | OriginAction::Pull
            | OriginAction::Push
            | OriginAction::Attach => OutputSink::Terminal,
            // Publish and KeepLocal answer with a one-line unsupported notice
            // for workflows, so they stay in the browser.
            OriginAction::Publish
            | OriginAction::KeepLocal
            | OriginAction::OpenInBrowser
            | OriginAction::CopyReference => OutputSink::StatusLine,
        }
    }

    fn command_hint(&self, action: OriginAction, key: &str) -> String {
        match action {
            OriginAction::Diff => format!("wt workflow origin diff {key}"),
            OriginAction::Fetch => format!("wt workflow origin fetch {key}"),
            OriginAction::Pull => format!("wt workflow origin pull {key}"),
            OriginAction::Push => format!("wt workflow origin push {key}"),
            OriginAction::Attach => format!("wt workflow origin attach {key}"),
            OriginAction::Publish
            | OriginAction::KeepLocal
            | OriginAction::OpenInBrowser
            | OriginAction::CopyReference => String::new(),
        }
    }

    fn run_terminal(&self, action: OriginAction, key: &str) -> Result<()> {
        let workflows = [key.to_string()];
        match action {
            OriginAction::Diff => commands::workflow::origin::diff(self.ctx, &workflows),
            OriginAction::Fetch => commands::workflow::origin::fetch(self.ctx, &workflows),
            OriginAction::Pull => commands::workflow::origin::pull(self.ctx, &workflows),
            OriginAction::Push => commands::workflow::origin::push(self.ctx, &workflows),
            OriginAction::Attach => attach_workflow_origin(self.ctx, key),
            OriginAction::Publish
            | OriginAction::KeepLocal
            | OriginAction::OpenInBrowser
            | OriginAction::CopyReference => bail!("{action:?} is a status-line action"),
        }
    }

    fn run_status_line(&self, action: OriginAction, key: &str) -> Result<String> {
        match action {
            OriginAction::Publish => Ok(format!(
                "Publish is not available for workflow {key}; attach an existing workflow origin instead"
            )),
            OriginAction::KeepLocal => Ok(format!(
                "Keep local-only is not available for workflow {key}; workflow origins are optional by omission"
            )),
            OriginAction::OpenInBrowser => open_workflow_origin_url(self.ctx, key),
            OriginAction::CopyReference => copy_workflow_reference(self.ctx, key),
            OriginAction::Diff
            | OriginAction::Fetch
            | OriginAction::Pull
            | OriginAction::Push
            | OriginAction::Attach => bail!("{action:?} is a terminal action"),
        }
    }
}

pub(crate) trait DispatchLifecycle {
    fn suspend(&mut self) -> Result<()>;
    fn announce(&mut self, command_hint: &str);
    fn wait_for_ack(&mut self);
    fn resume(&mut self) -> Result<()>;
}

pub(crate) struct TerminalDispatchLifecycle<'a, E: TerminalEffects, B: Backend> {
    session: &'a mut TerminalSession<E>,
    terminal: &'a mut Terminal<B>,
}

impl<'a, E: TerminalEffects, B: Backend> TerminalDispatchLifecycle<'a, E, B> {
    pub(crate) fn new(session: &'a mut TerminalSession<E>, terminal: &'a mut Terminal<B>) -> Self {
        Self { session, terminal }
    }
}

impl<E: TerminalEffects, B: Backend> DispatchLifecycle for TerminalDispatchLifecycle<'_, E, B> {
    fn suspend(&mut self) -> Result<()> {
        self.session.suspend()
    }

    fn announce(&mut self, command_hint: &str) {
        println!("+ {command_hint}");
        println!();
    }

    fn wait_for_ack(&mut self) {
        println!();
        println!("Press any key to return to the browser...");
        // The session is suspended here, so the terminal is in cooked mode and
        // buffers input line by line — a bare event::read() would wait for
        // Enter. Read the ack in raw mode so any single key returns.
        let raw_mode_enabled = terminal::enable_raw_mode().is_ok();
        loop {
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        if raw_mode_enabled {
            let _ = terminal::disable_raw_mode();
        }
    }

    fn resume(&mut self) -> Result<()> {
        self.session.resume()?;
        // The alternate screen is fresh after resume while ratatui's back
        // buffer still holds the pre-suspend frame; clear resets it so the
        // next draw repaints the full browser instead of a stale diff.
        self.terminal
            .clear()
            .context("clear TUI browser for full redraw after resume")
    }
}

/// Run one origin action through its output sink.
///
/// Returns `None` for terminal actions (the caller refreshes rows after the
/// suspend-resume round trip) and `Some(message)` for status-line actions
/// (the caller shows the line in the browser status line; no refresh — these
/// actions do not change on-disk state).
pub(crate) fn dispatch(
    action: OriginAction,
    key: &str,
    backend: &impl DispatchBackend,
    lifecycle: &mut impl DispatchLifecycle,
) -> Result<Option<String>> {
    match backend.output_sink(action) {
        OutputSink::Terminal => {
            lifecycle.suspend()?;
            lifecycle.announce(&backend.command_hint(action, key));
            if let Err(err) = backend.run_terminal(action, key) {
                eprintln!("{err:#}");
            }
            lifecycle.wait_for_ack();
            lifecycle.resume()?;
            Ok(None)
        }
        OutputSink::StatusLine => Ok(Some(
            backend
                .run_status_line(action, key)
                .unwrap_or_else(|err| format!("error: {err:#}")),
        )),
    }
}

fn attach_origin(ctx: &Ctx, key: &str) -> Result<()> {
    let issue = ctx.ui.input("Issue id to attach", None)?;
    let issue = issue.trim();
    if issue.is_empty() {
        ctx.ui
            .print_warning(&format!("Skipped attach for {key}: no issue id entered"));
        return Ok(());
    }
    commands::task_origin::attach(ctx, key, issue)
}

fn attach_workflow_origin(ctx: &Ctx, key: &str) -> Result<()> {
    let issue = ctx.ui.input("Issue id to attach", None)?;
    let issue = issue.trim();
    if issue.is_empty() {
        ctx.ui.print_warning(&format!(
            "Skipped workflow attach for {key}: no issue id entered"
        ));
        return Ok(());
    }
    commands::workflow::origin::attach(ctx, key, issue)
}

fn open_origin_url(ctx: &Ctx, key: &str) -> Result<String> {
    let Some(snapshot) = read_task_snapshot(&ctx.storage_root, key)? else {
        return Ok(format!("No fetched origin snapshot for {key}"));
    };
    let document = task::read_task_document(ctx, key)?;
    if !document
        .origin
        .as_ref()
        .is_some_and(|origin| snapshot.matches_origin(&origin.provider, &origin.id))
    {
        return Ok(format!(
            "origin changed since last fetch — run wt task origin fetch {key}"
        ));
    }
    let Some(url) = snapshot.origin.url.as_deref() else {
        return Ok(format!("No origin URL recorded for {key}"));
    };
    opener::open_browser(url).with_context(|| format!("Failed to open origin URL for {key}"))?;
    Ok(format!("Opened origin URL for {key}"))
}

fn open_workflow_origin_url(ctx: &Ctx, key: &str) -> Result<String> {
    let Some(snapshot) = read_workflow_snapshot(&ctx.storage_root, key)? else {
        return Ok(format!("No fetched workflow origin snapshot for {key}"));
    };
    let metadata = read_workflow_metadata(ctx, key)?;
    let Some(origin) = metadata.origin.as_ref() else {
        return Ok(format!("No workflow origin recorded for {key}"));
    };
    if !snapshot.matches_origin(&origin.provider, &origin.id) {
        return Ok(format!(
            "workflow origin changed since last fetch — run wt workflow origin fetch {key}"
        ));
    }
    let Some(url) = snapshot.origin.url.as_deref() else {
        return Ok(format!("No workflow origin URL recorded for {key}"));
    };
    opener::open_browser(url)
        .with_context(|| format!("Failed to open workflow origin URL for {key}"))?;
    Ok(format!("Opened workflow origin URL for {key}"))
}

fn copy_reference(ctx: &Ctx, key: &str) -> Result<String> {
    let document = task::read_task_document(ctx, key)?;
    let reference = document
        .origin
        .as_ref()
        .map(|origin| format!("{}:{}", origin.provider, origin.id))
        .unwrap_or_else(|| key.to_string());
    copy_reference_text(ctx, &reference)
}

fn copy_workflow_reference(ctx: &Ctx, key: &str) -> Result<String> {
    let metadata = read_workflow_metadata(ctx, key)?;
    let reference = metadata
        .origin
        .as_ref()
        .map(|origin| format!("{}:{}", origin.provider, origin.id))
        .unwrap_or_else(|| key.to_string());
    copy_reference_text(ctx, &reference)
}

fn read_workflow_metadata(ctx: &Ctx, key: &str) -> Result<workflow::WorkflowMetadata> {
    let path = workflow::resolve(ctx, key)?;
    workflow::read(&path)
}

fn copy_reference_text(ctx: &Ctx, reference: &str) -> Result<String> {
    let quoted = shell_words::quote(reference);
    let script = format!("printf %s {quoted} | pbcopy");
    let out = ctx
        .runner
        .run("sh", &["-c", &script], None)
        .context("Failed to copy origin reference")?;
    if !out.success {
        bail!(
            "Failed to copy origin reference: {}",
            command_failure(&out.stderr, &out.stdout)
        );
    }
    Ok(format!("Copied reference {reference}"))
}

fn command_failure(stderr: &str, stdout: &str) -> String {
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if detail.is_empty() {
        "command exited unsuccessfully".into()
    } else {
        detail.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};
    use crate::origin_action_menu::OriginAction;
    use crate::origin_snapshot::{FieldSnapshot, OriginRef, OriginSnapshot, write_snapshot};
    use std::sync::{Arc, Mutex};

    struct RecordingBackend {
        sink: OutputSink,
        calls: Mutex<Vec<String>>,
        fail: bool,
    }

    impl RecordingBackend {
        fn new(sink: OutputSink, fail: bool) -> Self {
            Self {
                sink,
                calls: Mutex::new(vec![]),
                fail,
            }
        }
    }

    impl DispatchBackend for RecordingBackend {
        fn output_sink(&self, _action: OriginAction) -> OutputSink {
            self.sink
        }

        fn command_hint(&self, action: OriginAction, key: &str) -> String {
            format!("hint {action:?} {key}")
        }

        fn run_terminal(&self, action: OriginAction, key: &str) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("terminal {action:?}:{key}"));
            if self.fail {
                anyhow::bail!("provider unreachable")
            }
            Ok(())
        }

        fn run_status_line(&self, action: OriginAction, key: &str) -> anyhow::Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("status-line {action:?}:{key}"));
            if self.fail {
                anyhow::bail!("provider unreachable")
            }
            Ok(format!("done {key}"))
        }
    }

    struct RecordingLifecycle {
        log: Mutex<Vec<String>>,
    }

    impl RecordingLifecycle {
        fn new() -> Self {
            Self {
                log: Mutex::new(vec![]),
            }
        }
    }

    impl DispatchLifecycle for RecordingLifecycle {
        fn suspend(&mut self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("suspend".into());
            Ok(())
        }

        fn announce(&mut self, command_hint: &str) {
            self.log
                .lock()
                .unwrap()
                .push(format!("announce {command_hint}"));
        }

        fn wait_for_ack(&mut self) {
            self.log.lock().unwrap().push("ack".into());
        }

        fn resume(&mut self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("resume".into());
            Ok(())
        }
    }

    #[test]
    fn terminal_dispatch_wraps_backend_with_suspend_announce_ack_resume() {
        let backend = RecordingBackend::new(OutputSink::Terminal, false);
        let mut lifecycle = RecordingLifecycle::new();

        let outcome = dispatch(
            OriginAction::Diff,
            "origin-sync-tui",
            &backend,
            &mut lifecycle,
        )
        .unwrap();

        assert_eq!(
            outcome, None,
            "terminal 싱크는 status 메시지를 만들지 않는다"
        );
        assert_eq!(
            *backend.calls.lock().unwrap(),
            vec!["terminal Diff:origin-sync-tui"]
        );
        assert_eq!(
            *lifecycle.log.lock().unwrap(),
            vec![
                "suspend",
                "announce hint Diff origin-sync-tui",
                "ack",
                "resume"
            ]
        );
    }

    #[test]
    fn terminal_backend_failure_still_acks_and_resumes() {
        let backend = RecordingBackend::new(OutputSink::Terminal, true);
        let mut lifecycle = RecordingLifecycle::new();

        let outcome = dispatch(
            OriginAction::Push,
            "origin-sync-tui",
            &backend,
            &mut lifecycle,
        );

        assert!(
            matches!(outcome, Ok(None)),
            "디스패치는 백엔드 에러를 표시 후 삼키고 세션을 유지한다"
        );
        assert_eq!(
            *lifecycle.log.lock().unwrap(),
            vec![
                "suspend",
                "announce hint Push origin-sync-tui",
                "ack",
                "resume"
            ]
        );
    }

    #[test]
    fn status_line_dispatch_returns_message_without_touching_lifecycle() {
        let backend = RecordingBackend::new(OutputSink::StatusLine, false);
        let mut lifecycle = RecordingLifecycle::new();

        let outcome = dispatch(
            OriginAction::CopyReference,
            "origin-sync-tui",
            &backend,
            &mut lifecycle,
        )
        .unwrap();

        assert_eq!(outcome, Some("done origin-sync-tui".into()));
        assert_eq!(
            *backend.calls.lock().unwrap(),
            vec!["status-line CopyReference:origin-sync-tui"]
        );
        assert!(
            lifecycle.log.lock().unwrap().is_empty(),
            "status-line 싱크는 suspend/ack/resume을 거치지 않는다"
        );
    }

    #[test]
    fn status_line_dispatch_maps_errors_to_error_message() {
        let backend = RecordingBackend::new(OutputSink::StatusLine, true);
        let mut lifecycle = RecordingLifecycle::new();

        let outcome = dispatch(
            OriginAction::CopyReference,
            "origin-sync-tui",
            &backend,
            &mut lifecycle,
        )
        .unwrap();

        assert_eq!(outcome, Some("error: provider unreachable".into()));
        assert!(lifecycle.log.lock().unwrap().is_empty());
    }

    fn test_ctx(dir: &std::path::Path) -> Ctx {
        Ctx::new_with_options(
            dir.to_path_buf(),
            dir.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::new(MockUi::new())),
            CtxOptions {
                output_mode: OutputMode::Text,
                ..CtxOptions::default()
            },
        )
    }

    #[test]
    fn task_backend_classifies_actions_by_terminal_need() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = CtxBackend::new(&ctx);

        for action in [
            OriginAction::Diff,
            OriginAction::Fetch,
            OriginAction::Pull,
            OriginAction::Push,
            OriginAction::Publish,
            OriginAction::Attach,
        ] {
            assert_eq!(
                backend.output_sink(action),
                OutputSink::Terminal,
                "{action:?}"
            );
        }
        for action in [
            OriginAction::KeepLocal,
            OriginAction::OpenInBrowser,
            OriginAction::CopyReference,
        ] {
            assert_eq!(
                backend.output_sink(action),
                OutputSink::StatusLine,
                "{action:?}"
            );
        }
    }

    #[test]
    fn workflow_backend_classifies_unsupported_actions_as_status_line() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = WorkflowCtxBackend::new(&ctx);

        for action in [
            OriginAction::Diff,
            OriginAction::Fetch,
            OriginAction::Pull,
            OriginAction::Push,
            OriginAction::Attach,
        ] {
            assert_eq!(
                backend.output_sink(action),
                OutputSink::Terminal,
                "{action:?}"
            );
        }
        for action in [
            OriginAction::Publish,
            OriginAction::KeepLocal,
            OriginAction::OpenInBrowser,
            OriginAction::CopyReference,
        ] {
            assert_eq!(
                backend.output_sink(action),
                OutputSink::StatusLine,
                "{action:?}"
            );
        }
    }

    #[test]
    fn command_hints_name_the_equivalent_cli_command() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        assert_eq!(
            CtxBackend::new(&ctx).command_hint(OriginAction::Diff, "origin-sync-tui"),
            "wt task origin diff origin-sync-tui"
        );
        assert_eq!(
            WorkflowCtxBackend::new(&ctx).command_hint(OriginAction::Fetch, "2026-06-06-001"),
            "wt workflow origin fetch 2026-06-06-001"
        );
    }

    #[test]
    fn task_keep_local_returns_status_message() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = CtxBackend::new(&ctx);

        let message = backend
            .run_status_line(OriginAction::KeepLocal, "origin-sync-tui")
            .unwrap();

        assert_eq!(
            message,
            "Keeping origin-sync-tui local-only; no origin changes made"
        );
    }

    #[test]
    fn workflow_publish_returns_unsupported_status_message() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = WorkflowCtxBackend::new(&ctx);

        let message = backend
            .run_status_line(OriginAction::Publish, "2026-06-06-001")
            .unwrap();

        assert_eq!(
            message,
            "Publish is not available for workflow 2026-06-06-001; attach an existing workflow origin instead"
        );
    }

    struct NoopEffects;

    impl crate::tui::terminal::TerminalEffects for NoopEffects {
        fn enter(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn leave(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn resume_clears_terminal_to_force_full_redraw() {
        use crate::tui::terminal::TerminalSession;
        use ratatui::backend::TestBackend;
        use ratatui::widgets::Paragraph;

        let mut session = TerminalSession::with_effects(NoopEffects).unwrap();
        let mut terminal = ratatui::Terminal::new(TestBackend::new(10, 3)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("browser"), frame.area()))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "b");

        let mut lifecycle = TerminalDispatchLifecycle::new(&mut session, &mut terminal);
        lifecycle.suspend().unwrap();
        lifecycle.resume().unwrap();

        assert_eq!(
            terminal.backend().buffer()[(0, 0)].symbol(),
            " ",
            "resume은 back buffer를 리셋해 다음 draw가 full redraw가 되게 한다"
        );
    }

    #[test]
    fn open_in_browser_rejects_snapshot_for_changed_origin() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-143"
"#,
        )
        .unwrap();
        let mut origin = OriginRef::new("linear", "WT-142");
        origin.url = Some("https://linear.app/team/issue/WT-142".into());
        let snapshot = OriginSnapshot::task(
            "origin-sync-tui",
            origin,
            FieldSnapshot::new("Original title", "local body"),
            FieldSnapshot::new("Remote title", "local body"),
        );
        let ctx = test_ctx(dir.path());
        write_snapshot(&ctx.storage_root, &snapshot).unwrap();
        let backend = CtxBackend::new(&ctx);

        let message = backend
            .run_status_line(OriginAction::OpenInBrowser, "origin-sync-tui")
            .unwrap();

        assert_eq!(
            message,
            "origin changed since last fetch — run wt task origin fetch origin-sync-tui"
        );
    }
}
