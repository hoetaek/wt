use crate::commands;
use crate::context::Ctx;
use crate::origin_action_menu::OriginAction;
use crate::origin_snapshot::read_task_snapshot;
use crate::task;
use crate::tui::terminal::TerminalSession;
use anyhow::{Context, Result, bail};
use ratatui::crossterm::event;

pub(crate) trait DispatchBackend {
    fn run(&self, action: OriginAction, key: &str) -> Result<()>;
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
    fn run(&self, action: OriginAction, key: &str) -> Result<()> {
        let tasks = [key.to_string()];
        match action {
            OriginAction::Diff => commands::task_origin::diff(self.ctx, &tasks),
            OriginAction::Fetch => commands::task_origin::fetch(self.ctx, &tasks),
            OriginAction::Pull => commands::task_origin::pull(self.ctx, &tasks),
            OriginAction::Push => commands::task_origin::push(self.ctx, &tasks),
            OriginAction::Publish => commands::task_origin::publish(self.ctx, &tasks),
            OriginAction::Attach => attach_origin(self.ctx, key),
            OriginAction::KeepLocal => {
                self.ctx
                    .ui
                    .print_plain(&format!("Keeping {key} local-only; no origin changes made"));
                Ok(())
            }
            OriginAction::OpenInBrowser => open_origin_url(self.ctx, key),
            OriginAction::CopyReference => copy_reference(self.ctx, key),
        }
    }
}

pub(crate) trait DispatchLifecycle {
    fn suspend(&mut self) -> Result<()>;
    fn wait_for_ack(&mut self);
    fn resume(&mut self) -> Result<()>;
}

pub(crate) struct TerminalDispatchLifecycle<'a> {
    session: &'a mut TerminalSession,
}

impl<'a> TerminalDispatchLifecycle<'a> {
    pub(crate) fn new(session: &'a mut TerminalSession) -> Self {
        Self { session }
    }
}

impl DispatchLifecycle for TerminalDispatchLifecycle<'_> {
    fn suspend(&mut self) -> Result<()> {
        self.session.suspend()
    }

    fn wait_for_ack(&mut self) {
        println!();
        println!("Press any key to return to the task browser...");
        let _ = event::read();
    }

    fn resume(&mut self) -> Result<()> {
        self.session.resume()
    }
}

pub(crate) fn dispatch(
    action: OriginAction,
    key: &str,
    backend: &impl DispatchBackend,
    lifecycle: &mut impl DispatchLifecycle,
) -> Result<()> {
    lifecycle.suspend()?;
    if let Err(err) = backend.run(action, key) {
        eprintln!("{err:#}");
    }
    lifecycle.wait_for_ack();
    lifecycle.resume()
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

fn open_origin_url(ctx: &Ctx, key: &str) -> Result<()> {
    let Some(snapshot) = read_task_snapshot(&ctx.storage_root, key)? else {
        ctx.ui
            .print_warning(&format!("No fetched origin snapshot for {key}"));
        return Ok(());
    };
    let document = task::read_task_document(ctx, key)?;
    if !document
        .origin
        .as_ref()
        .is_some_and(|origin| snapshot.matches_origin(&origin.provider, &origin.id))
    {
        ctx.ui.print_warning(&format!(
            "origin changed since last fetch — run wt task origin fetch {key}"
        ));
        return Ok(());
    }
    let Some(url) = snapshot.origin.url.as_deref() else {
        ctx.ui
            .print_warning(&format!("No origin URL recorded for {key}"));
        return Ok(());
    };
    opener::open_browser(url).with_context(|| format!("Failed to open origin URL for {key}"))?;
    ctx.ui.print_plain(&format!("Opened origin URL for {key}"));
    Ok(())
}

fn copy_reference(ctx: &Ctx, key: &str) -> Result<()> {
    let document = task::read_task_document(ctx, key)?;
    let reference = document
        .origin
        .as_ref()
        .map(|origin| format!("{}:{}", origin.provider, origin.id))
        .unwrap_or_else(|| key.to_string());
    let quoted = shell_words::quote(&reference);
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
    ctx.ui.print_plain(&format!("Copied reference {reference}"));
    Ok(())
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
        calls: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl DispatchBackend for RecordingBackend {
        fn run(&self, action: OriginAction, key: &str) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push(format!("{action:?}:{key}"));
            if self.fail {
                anyhow::bail!("provider unreachable")
            }
            Ok(())
        }
    }

    struct RecordingLifecycle {
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl DispatchLifecycle for RecordingLifecycle {
        fn suspend(&mut self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("suspend");
            Ok(())
        }

        fn wait_for_ack(&mut self) {
            self.log.lock().unwrap().push("ack");
        }

        fn resume(&mut self) -> anyhow::Result<()> {
            self.log.lock().unwrap().push("resume");
            Ok(())
        }
    }

    #[test]
    fn dispatch_wraps_backend_with_suspend_ack_resume() {
        let calls = Arc::new(Mutex::new(vec![]));
        let log = Arc::new(Mutex::new(vec![]));
        let backend = RecordingBackend {
            calls: Arc::clone(&calls),
            fail: false,
        };
        let mut lifecycle = RecordingLifecycle {
            log: Arc::clone(&log),
        };

        dispatch(
            OriginAction::Diff,
            "origin-sync-tui",
            &backend,
            &mut lifecycle,
        )
        .unwrap();

        assert_eq!(*calls.lock().unwrap(), vec!["Diff:origin-sync-tui"]);
        assert_eq!(*log.lock().unwrap(), vec!["suspend", "ack", "resume"]);
    }

    #[test]
    fn backend_failure_still_acks_and_resumes() {
        let calls = Arc::new(Mutex::new(vec![]));
        let log = Arc::new(Mutex::new(vec![]));
        let backend = RecordingBackend {
            calls: Arc::clone(&calls),
            fail: true,
        };
        let mut lifecycle = RecordingLifecycle {
            log: Arc::clone(&log),
        };

        let result = dispatch(
            OriginAction::Push,
            "origin-sync-tui",
            &backend,
            &mut lifecycle,
        );

        assert!(
            result.is_ok(),
            "디스패치는 백엔드 에러를 표시 후 삼키고 세션을 유지한다"
        );
        assert_eq!(*log.lock().unwrap(), vec!["suspend", "ack", "resume"]);
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
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new_with_options(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
            CtxOptions {
                output_mode: OutputMode::Text,
                ..CtxOptions::default()
            },
        );
        write_snapshot(&ctx.storage_root, &snapshot).unwrap();
        let backend = CtxBackend::new(&ctx);

        backend
            .run(OriginAction::OpenInBrowser, "origin-sync-tui")
            .unwrap();

        assert_eq!(
            ui.warnings.lock().unwrap().as_slice(),
            ["origin changed since last fetch — run wt task origin fetch origin-sync-tui"]
        );
    }
}
