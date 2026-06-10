use crate::commands;
use crate::config::IssueProviderType;
use crate::context::{Ctx, CtxOptions, UserInterface};
use crate::origin_action_menu::OriginAction;
use crate::origin_snapshot::{read_task_snapshot, read_workflow_snapshot};
use crate::task;
use crate::tui::remote_ui::{TuiUi, UiRequest};
use crate::workflow;
use anyhow::{Context, Result, bail};
use std::sync::mpsc;

/// Where an action's result goes. Worker actions run backend flows on a worker
/// thread and stream `UiRequest`s back to the browser. Status-line actions
/// finish locally with a one-line result, so the browser stays up and shows
/// the line in its status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputSink {
    Worker,
    StatusLine,
}

pub(crate) type WorkerJob = Box<dyn FnOnce(&Ctx) -> Result<()> + Send>;

pub(crate) trait DispatchBackend {
    /// Classify the action. Each backend keeps this match exhaustive so a new
    /// action cannot ship unclassified.
    fn output_sink(&self, action: OriginAction) -> OutputSink;
    /// Present-tense action label used for in-flight and final status lines.
    fn verb(&self, action: OriginAction) -> &'static str;
    fn worker_job(&self, action: OriginAction, key: &str) -> WorkerJob;
    fn worker_ctx(&self, ui: Box<dyn UserInterface>) -> Ctx;
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
            | OriginAction::Attach
            | OriginAction::Import
            | OriginAction::Archive => OutputSink::Worker,
            OriginAction::KeepLocal | OriginAction::OpenInBrowser | OriginAction::CopyReference => {
                OutputSink::StatusLine
            }
        }
    }

    fn verb(&self, action: OriginAction) -> &'static str {
        match action {
            OriginAction::Diff => "diff",
            OriginAction::Fetch => "fetch",
            OriginAction::Pull => "pull",
            OriginAction::Push => "push",
            OriginAction::Publish => "publish",
            OriginAction::Attach => "attach",
            OriginAction::Import => "import",
            OriginAction::Archive => "archive",
            OriginAction::KeepLocal | OriginAction::OpenInBrowser | OriginAction::CopyReference => {
                "run"
            }
        }
    }

    fn worker_job(&self, action: OriginAction, key: &str) -> WorkerJob {
        let key = key.to_string();
        match action {
            OriginAction::Diff => Box::new(move |ctx| commands::task_origin::diff(ctx, &[key])),
            OriginAction::Fetch => Box::new(move |ctx| commands::task_origin::fetch(ctx, &[key])),
            OriginAction::Pull => Box::new(move |ctx| commands::task_origin::pull(ctx, &[key])),
            OriginAction::Push => Box::new(move |ctx| commands::task_origin::push(ctx, &[key])),
            OriginAction::Publish => {
                Box::new(move |ctx| commands::task_origin::publish(ctx, &[key]))
            }
            OriginAction::Attach => Box::new(move |ctx| attach_origin(ctx, &key)),
            OriginAction::Import => {
                let issue = origin_only_import_id(&key);
                Box::new(move |ctx| commands::task_origin::import(ctx, &[issue]))
            }
            OriginAction::Archive => {
                Box::new(move |ctx| commands::task_archive::archive(ctx, &[key]))
            }
            OriginAction::KeepLocal | OriginAction::OpenInBrowser | OriginAction::CopyReference => {
                Box::new(move |_| bail!("{action:?} is a status-line action"))
            }
        }
    }

    fn worker_ctx(&self, ui: Box<dyn UserInterface>) -> Ctx {
        Ctx::new_with_options(
            self.ctx.repo_root.clone(),
            self.ctx.invocation_root.clone(),
            self.ctx.config.clone(),
            Box::new(crate::runner::RealRunner),
            ui,
            CtxOptions {
                base_config: self.ctx.base_config.clone(),
                config_source: self.ctx.config_source.clone(),
                storage_root: Some(self.ctx.storage_root.clone()),
                output_mode: self.ctx.output_mode,
                verbosity: self.ctx.verbosity,
                quiet: self.ctx.quiet,
                launcher_coordinator_id: self.ctx.launcher_coordinator_id.clone(),
            },
        )
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
            | OriginAction::Attach
            | OriginAction::Import
            | OriginAction::Archive => bail!("{action:?} is a worker action"),
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
            | OriginAction::Attach => OutputSink::Worker,
            // Unsupported task-only actions answer with a one-line notice for
            // workflows, so they stay in the browser.
            OriginAction::Publish
            | OriginAction::KeepLocal
            | OriginAction::Import
            | OriginAction::Archive
            | OriginAction::OpenInBrowser
            | OriginAction::CopyReference => OutputSink::StatusLine,
        }
    }

    fn verb(&self, action: OriginAction) -> &'static str {
        match action {
            OriginAction::Diff => "diff",
            OriginAction::Fetch => "fetch",
            OriginAction::Pull => "pull",
            OriginAction::Push => "push",
            OriginAction::Attach => "attach",
            OriginAction::Publish
            | OriginAction::KeepLocal
            | OriginAction::Import
            | OriginAction::Archive
            | OriginAction::OpenInBrowser
            | OriginAction::CopyReference => "run",
        }
    }

    fn worker_job(&self, action: OriginAction, key: &str) -> WorkerJob {
        let key = key.to_string();
        match action {
            OriginAction::Diff => {
                Box::new(move |ctx| commands::workflow::origin::diff(ctx, &[key]))
            }
            OriginAction::Fetch => {
                Box::new(move |ctx| commands::workflow::origin::fetch(ctx, &[key]))
            }
            OriginAction::Pull => {
                Box::new(move |ctx| commands::workflow::origin::pull(ctx, &[key]))
            }
            OriginAction::Push => {
                Box::new(move |ctx| commands::workflow::origin::push(ctx, &[key]))
            }
            OriginAction::Attach => Box::new(move |ctx| attach_workflow_origin(ctx, &key)),
            OriginAction::Publish
            | OriginAction::KeepLocal
            | OriginAction::Import
            | OriginAction::Archive
            | OriginAction::OpenInBrowser
            | OriginAction::CopyReference => {
                Box::new(move |_| bail!("{action:?} is a status-line action"))
            }
        }
    }

    fn worker_ctx(&self, ui: Box<dyn UserInterface>) -> Ctx {
        Ctx::new_with_options(
            self.ctx.repo_root.clone(),
            self.ctx.invocation_root.clone(),
            self.ctx.config.clone(),
            Box::new(crate::runner::RealRunner),
            ui,
            CtxOptions {
                base_config: self.ctx.base_config.clone(),
                config_source: self.ctx.config_source.clone(),
                storage_root: Some(self.ctx.storage_root.clone()),
                output_mode: self.ctx.output_mode,
                verbosity: self.ctx.verbosity,
                quiet: self.ctx.quiet,
                launcher_coordinator_id: self.ctx.launcher_coordinator_id.clone(),
            },
        )
    }

    fn run_status_line(&self, action: OriginAction, key: &str) -> Result<String> {
        match action {
            OriginAction::Publish => Ok(format!(
                "Publish is not available for workflow {key}; attach an existing workflow origin instead"
            )),
            OriginAction::KeepLocal => Ok(format!(
                "Keep local-only is not available for workflow {key}; workflow origins are optional by omission"
            )),
            OriginAction::Import => Ok(format!(
                "Import is not available for workflow {key}; import provider issues from the task browser"
            )),
            OriginAction::Archive => Ok(format!(
                "Archive is not available for workflow {key}; archive tasks from the task browser"
            )),
            OriginAction::OpenInBrowser => open_workflow_origin_url(self.ctx, key),
            OriginAction::CopyReference => copy_workflow_reference(self.ctx, key),
            OriginAction::Diff
            | OriginAction::Fetch
            | OriginAction::Pull
            | OriginAction::Push
            | OriginAction::Attach => bail!("{action:?} is a worker action"),
        }
    }
}

fn origin_only_import_id(key: &str) -> String {
    key.split_once(':')
        .map(|(_, id)| id)
        .unwrap_or(key)
        .trim()
        .trim_start_matches('#')
        .to_string()
}

pub(crate) enum DispatchStart {
    Started(InFlightAction),
    Message(String),
}

pub(crate) struct InFlightAction {
    pub(crate) key: String,
    pub(crate) verb: &'static str,
    pub(crate) ui_rx: mpsc::Receiver<UiRequest>,
    pub(crate) done_rx: mpsc::Receiver<Result<()>>,
}

pub(crate) struct InFlightOriginFetch {
    pub(crate) ui_rx: mpsc::Receiver<UiRequest>,
    pub(crate) done_rx: mpsc::Receiver<Result<()>>,
}

/// Run one origin action through its output sink.
///
/// Worker actions start an in-flight worker thread. Status-line actions return
/// a one-line browser message; no refresh — these actions do not change
/// on-disk state.
pub(crate) fn dispatch(
    action: OriginAction,
    key: &str,
    backend: &impl DispatchBackend,
) -> Result<DispatchStart> {
    match backend.output_sink(action) {
        OutputSink::Worker => {
            let (ui_tx, ui_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let job = backend.worker_job(action, key);
            let ctx = backend.worker_ctx(Box::new(TuiUi::new(ui_tx)));
            std::thread::spawn(move || {
                let result = job(&ctx);
                let _ = done_tx.send(result);
            });
            Ok(DispatchStart::Started(InFlightAction {
                key: key.to_string(),
                verb: backend.verb(action),
                ui_rx,
                done_rx,
            }))
        }
        OutputSink::StatusLine => Ok(DispatchStart::Message(
            backend
                .run_status_line(action, key)
                .unwrap_or_else(|err| format!("error: {err:#}")),
        )),
    }
}

pub(crate) fn spawn_origin_fetch(ctx: &Ctx) -> InFlightOriginFetch {
    let (ui_tx, ui_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let provider = configured_issue_provider(ctx);
    let worker_ui_tx = ui_tx.clone();
    let worker_ctx = Ctx::new_with_options(
        ctx.repo_root.clone(),
        ctx.invocation_root.clone(),
        ctx.config.clone(),
        Box::new(crate::runner::RealRunner),
        Box::new(TuiUi::new(ui_tx)),
        CtxOptions {
            base_config: ctx.base_config.clone(),
            config_source: ctx.config_source.clone(),
            storage_root: Some(ctx.storage_root.clone()),
            output_mode: ctx.output_mode,
            verbosity: ctx.verbosity,
            quiet: ctx.quiet,
            launcher_coordinator_id: ctx.launcher_coordinator_id.clone(),
        },
    );

    std::thread::spawn(move || {
        let result = commands::issue::build_provider(&worker_ctx)
            .and_then(|provider| provider.list_issues())
            .map_err(|err| format!("{err:#}"));
        let send_result = worker_ui_tx
            .send(UiRequest::OriginIssuesLoaded { provider, result })
            .map_err(|_| anyhow::anyhow!("browser closed before origin issue fetch completed"));
        let _ = done_tx.send(send_result);
    });

    InFlightOriginFetch { ui_rx, done_rx }
}

fn configured_issue_provider(ctx: &Ctx) -> String {
    match ctx.config.issues.as_ref().map(|issues| &issues.provider) {
        Some(IssueProviderType::Github) => "github".into(),
        Some(IssueProviderType::Linear) => "linear".into(),
        None => "provider".into(),
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
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx, CtxOptions, OutputMode, UserInterface};
    use crate::error::WtError;
    use crate::origin_action_menu::OriginAction;
    use crate::origin_snapshot::{FieldSnapshot, OriginRef, OriginSnapshot, write_snapshot};
    use crate::task;
    use crate::tui::remote_ui::{UiReply, UiRequest};
    use std::path::Path;
    use std::sync::Arc;

    struct FakeJobBackend;

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

    impl DispatchBackend for FakeJobBackend {
        fn output_sink(&self, action: OriginAction) -> OutputSink {
            match action {
                OriginAction::CopyReference => OutputSink::StatusLine,
                _ => OutputSink::Worker,
            }
        }

        fn verb(&self, _action: OriginAction) -> &'static str {
            "pull"
        }

        fn worker_job(&self, _action: OriginAction, _key: &str) -> WorkerJob {
            Box::new(|ctx| {
                let _confirmed = ctx.ui.confirm("Pull selected provider fields?", false)?;
                Ok(())
            })
        }

        fn worker_ctx(&self, ui: Box<dyn UserInterface>) -> Ctx {
            test_ctx_with_ui(ui)
        }

        fn run_status_line(&self, _action: OriginAction, key: &str) -> anyhow::Result<String> {
            Ok(format!("Copied {key}"))
        }
    }

    #[test]
    fn status_line_action_returns_message_without_spawning() {
        let backend = FakeJobBackend;
        let started = dispatch(OriginAction::CopyReference, "wt-1", &backend).unwrap();
        let DispatchStart::Message(message) = started else {
            panic!("expected message")
        };
        assert_eq!(message, "Copied wt-1");
    }

    #[test]
    fn worker_action_streams_ui_requests_and_finishes() {
        let backend = FakeJobBackend;
        let DispatchStart::Started(inflight) =
            dispatch(OriginAction::Pull, "wt-1", &backend).unwrap()
        else {
            panic!("expected started")
        };
        let UiRequest::Confirm { reply, .. } = inflight.ui_rx.recv().unwrap() else {
            panic!("expected confirm request");
        };
        reply.send(UiReply::Bool(true)).unwrap();
        let result = inflight.done_rx.recv().unwrap();
        assert!(result.is_ok());
    }

    #[test]
    fn cancelled_reply_surfaces_as_cancelled_result() {
        let backend = FakeJobBackend;
        let DispatchStart::Started(inflight) =
            dispatch(OriginAction::Pull, "wt-1", &backend).unwrap()
        else {
            panic!("expected started")
        };
        let UiRequest::Confirm { reply, .. } = inflight.ui_rx.recv().unwrap() else {
            panic!("expected confirm request");
        };
        reply.send(UiReply::Cancelled).unwrap();
        let result = inflight.done_rx.recv().unwrap();
        let err = result.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<WtError>(),
            Some(WtError::Cancelled)
        ));
    }

    fn test_ctx_with_ui(ui: Box<dyn UserInterface>) -> Ctx {
        let dir = tempfile::tempdir().unwrap();
        test_ctx_at(dir.path(), ui)
    }

    fn test_ctx(dir: &std::path::Path) -> Ctx {
        test_ctx_at(dir, Box::new(Arc::new(MockUi::new())))
    }

    fn test_ctx_with_confirm(dir: &std::path::Path, confirmed: bool) -> Ctx {
        let mut ui = MockUi::new();
        ui.add_confirm(confirmed);
        test_ctx_at(dir, Box::new(Arc::new(ui)))
    }

    fn test_ctx_at(dir: &std::path::Path, ui: Box<dyn UserInterface>) -> Ctx {
        Ctx::new_with_options(
            dir.to_path_buf(),
            dir.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            ui,
            CtxOptions {
                output_mode: OutputMode::Text,
                ..CtxOptions::default()
            },
        )
    }

    fn github_config() -> Config {
        Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Github,
                gh_user: None,
                origin_policy: Default::default(),
            }),
            ..Config::default()
        }
    }

    fn test_ctx_with_runner(dir: &std::path::Path, config: Config, runner: Arc<MockRunner>) -> Ctx {
        Ctx::new(
            dir.to_path_buf(),
            dir.to_path_buf(),
            config,
            Box::new(SharedRunner { inner: runner }),
            Box::new(MockUi::new()),
        )
    }

    fn write_task(root: &std::path::Path, key: &str) {
        let tasks_dir = root.join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join(format!("{key}.toml")),
            format!(
                r#"title = "{key}"
branch = "{key}"
body = "Task body"
"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn archive_is_a_worker_action_with_verb() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = CtxBackend::new(&ctx);

        assert_eq!(
            backend.output_sink(OriginAction::Archive),
            OutputSink::Worker
        );
        assert_eq!(backend.verb(OriginAction::Archive), "archive");
    }

    #[test]
    fn import_is_a_worker_action_with_verb() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = CtxBackend::new(&ctx);

        assert_eq!(
            backend.output_sink(OriginAction::Import),
            OutputSink::Worker
        );
        assert_eq!(backend.verb(OriginAction::Import), "import");
    }

    #[test]
    fn archive_worker_job_calls_task_archive_backend() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "demo");
        let backend = CtxBackend::new(&ctx);

        let job = backend.worker_job(OriginAction::Archive, "demo");
        job(&ctx).unwrap();

        assert!(
            ctx.storage_root
                .task_archive_dir("demo")
                .join("demo.toml")
                .exists()
        );
    }

    #[test]
    fn import_worker_job_calls_task_origin_import_with_origin_only_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"number":52,"title":"Fix editor","body":"Long issue body","url":"https://github.com/acme/repo/issues/52"}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("https://github.com/acme/repo/tree/52-fix-editor", true);
        let runner = Arc::new(runner);
        let ctx = test_ctx_with_runner(dir.path(), github_config(), Arc::clone(&runner));
        let backend = CtxBackend::new(&ctx);

        let job = backend.worker_job(OriginAction::Import, "github:52");
        job(&ctx).unwrap();

        let document = task::read_task_document(&ctx, "52").unwrap();
        assert_eq!(document.title, "Fix editor");
        let origin = document.origin.unwrap();
        assert_eq!(origin.provider, "github");
        assert_eq!(origin.id, "#52");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "issue".to_string(),
                "view".to_string(),
                "52".to_string(),
                "--json".to_string(),
                "number,title,body,url".to_string(),
            ]
        );
        assert!(
            calls
                .iter()
                .all(|(_, args, _)| { !args.iter().any(|arg| arg == "github:52") })
        );
    }

    #[test]
    fn task_backend_classifies_actions_by_worker_need() {
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
            OriginAction::Archive,
            OriginAction::Import,
        ] {
            assert_eq!(
                backend.output_sink(action),
                OutputSink::Worker,
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
                OutputSink::Worker,
                "{action:?}"
            );
        }
        for action in [
            OriginAction::Publish,
            OriginAction::KeepLocal,
            OriginAction::Archive,
            OriginAction::OpenInBrowser,
            OriginAction::CopyReference,
            OriginAction::Import,
        ] {
            assert_eq!(
                backend.output_sink(action),
                OutputSink::StatusLine,
                "{action:?}"
            );
        }
    }

    #[test]
    fn backends_name_worker_verbs_for_status_lines() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        assert_eq!(CtxBackend::new(&ctx).verb(OriginAction::Diff), "diff");
        assert_eq!(
            WorkflowCtxBackend::new(&ctx).verb(OriginAction::Fetch),
            "fetch"
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

    #[test]
    fn workflow_archive_returns_unsupported_status_message() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = WorkflowCtxBackend::new(&ctx);

        let message = backend
            .run_status_line(OriginAction::Archive, "2026-06-06-001")
            .unwrap();

        assert_eq!(
            message,
            "Archive is not available for workflow 2026-06-06-001; archive tasks from the task browser"
        );
    }

    #[test]
    fn workflow_import_returns_unsupported_status_message() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let backend = WorkflowCtxBackend::new(&ctx);

        let message = backend
            .run_status_line(OriginAction::Import, "2026-06-06-001")
            .unwrap();

        assert_eq!(
            message,
            "Import is not available for workflow 2026-06-06-001; import provider issues from the task browser"
        );
    }

    #[test]
    fn spawn_origin_fetch_reconciles_via_apply() {
        use crate::services::issues::IssueListItem;
        use std::collections::HashSet;

        let local_origins: HashSet<(String, String)> = HashSet::new();
        let local_task_keys = HashSet::new();
        let rows = crate::commands::task_list::origin_only_rows(
            vec![IssueListItem {
                identifier: "175".into(),
                title: "A".into(),
                display: "github #175".into(),
                hint: None,
            }],
            &local_origins,
            &local_task_keys,
            "github",
        );

        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn spawn_origin_fetch_reports_missing_provider_config() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        let inflight = spawn_origin_fetch(&ctx);

        let UiRequest::OriginIssuesLoaded { provider, result } = inflight.ui_rx.recv().unwrap()
        else {
            panic!("expected origin issues loaded request");
        };
        assert_eq!(provider, "provider");
        assert!(result.unwrap_err().contains("No [issues] section"));
        assert!(inflight.done_rx.recv().unwrap().is_ok());
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
