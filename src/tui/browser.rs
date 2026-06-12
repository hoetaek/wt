use crate::commands::task_list;
use crate::context::Ctx;
use crate::error::WtError;
use crate::tui::app::{AppState, BrowserRow, KeyInput, Outcome, PopupOutcome, PopupSpec};
use crate::tui::dispatch::{
    self, CtxBackend, DispatchBackend, DispatchStart, InFlightAction, WorkflowCtxBackend,
};
use crate::tui::remote_ui::{PrintKind, UiReply, UiRequest};
use crate::tui::render::draw;
use crate::tui::terminal::TerminalSession;
use anyhow::{Context, Result, anyhow};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io;
use std::sync::mpsc;
use std::time::Duration;

const MIN_BROWSER_WIDTH: u16 = 40;
const MIN_BROWSER_HEIGHT: u16 = 8;

pub(crate) fn terminal_size_allows_browser() -> bool {
    ratatui::crossterm::terminal::size()
        .map(|(width, height)| width >= MIN_BROWSER_WIDTH && height >= MIN_BROWSER_HEIGHT)
        .unwrap_or(false)
}

pub(crate) fn run_browser(
    ctx: &Ctx,
    app: AppState,
    refresh: impl FnMut() -> Result<(Vec<BrowserRow>, Vec<String>)>,
) -> Result<()> {
    run_browser_with_backend(app, refresh, CtxBackend::new(ctx), Some(ctx))
}

pub(crate) fn run_workflow_browser(
    ctx: &Ctx,
    app: AppState,
    refresh: impl FnMut() -> Result<(Vec<BrowserRow>, Vec<String>)>,
) -> Result<()> {
    run_browser_with_backend(app, refresh, WorkflowCtxBackend::new(ctx), None)
}

fn run_browser_with_backend(
    mut app: AppState,
    mut refresh: impl FnMut() -> Result<(Vec<BrowserRow>, Vec<String>)>,
    dispatch_backend: impl DispatchBackend,
    origin_ctx: Option<&Ctx>,
) -> Result<()> {
    let _session = TerminalSession::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("open TUI browser terminal")?;
    let mut inflight: Option<InFlightAction> = None;
    let mut origin_fetch: Option<dispatch::InFlightOriginFetch> = None;
    let mut pending_reply: Option<mpsc::Sender<UiReply>> = None;
    let mut ctrl_c_armed = false;

    loop {
        terminal
            .draw(|frame| draw(frame, &mut app))
            .context("draw TUI browser")?;

        poll_origin_fetch(&mut app, &mut origin_fetch, &mut pending_reply, origin_ctx);

        if let Some((key, verb, result)) =
            poll_inflight(&mut app, &mut inflight, &mut pending_reply, origin_ctx)
        {
            match result {
                Ok(()) => {
                    let (rows, diagnostics) = refresh()?;
                    app.replace_rows_preserving_selection(rows, diagnostics, &key);
                    app.finish_action(format!("{verb} {key} done"));
                }
                Err(err) if matches!(err.downcast_ref::<WtError>(), Some(WtError::Cancelled)) => {
                    app.finish_action("cancelled".into());
                }
                Err(err) => {
                    app.push_output(PrintKind::Error, format!("{err:#}"));
                    app.finish_action(format!("{verb} {key} failed — see output"));
                }
            }
            inflight = None;
            pending_reply = None;
            ctrl_c_armed = false;
        }

        if !event::poll(Duration::from_millis(50)).context("poll TUI browser event")? {
            if inflight.is_some() || app.origin_fetching() {
                app.tick_spinner();
            }
            continue;
        }

        match event::read().context("read TUI browser event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if is_ctrl_c(key) {
                    if inflight.is_some() {
                        if ctrl_c_armed {
                            break;
                        }
                        app.show_dispatch_message(
                            "action in progress — Ctrl-C again to force quit".into(),
                        );
                        ctrl_c_armed = true;
                        continue;
                    }
                    break;
                }
                ctrl_c_armed = false;

                if let Some(input) = key_input(key) {
                    if app.has_popup() {
                        if let Some(outcome) = app.handle_popup_key(input) {
                            if let Some(reply) = pending_reply.take() {
                                let _ = reply.send(reply_for(outcome));
                            }
                        }
                        continue;
                    }

                    match app.handle(input) {
                        Outcome::Continue => {}
                        Outcome::Quit => break,
                        Outcome::Refresh => {
                            let preferred_key = app
                                .selected_row()
                                .map(|row| row.key.clone())
                                .unwrap_or_default();
                            if app.refresh_fetches_origin_issues() {
                                app.begin_origin_fetch();
                                start_origin_fetch(&mut app, &mut origin_fetch, origin_ctx);
                            }
                            let (rows, diagnostics) = refresh()?;
                            app.replace_rows_preserving_selection(
                                rows,
                                diagnostics,
                                &preferred_key,
                            );
                        }
                        Outcome::FetchOriginIssues => {
                            start_origin_fetch(&mut app, &mut origin_fetch, origin_ctx);
                        }
                        Outcome::Dispatch { key, action } => {
                            if app.action_in_flight() {
                                app.show_dispatch_message(
                                    "action in progress — wait for it to finish".into(),
                                );
                                continue;
                            }
                            match dispatch::dispatch(action, &key, &dispatch_backend)? {
                                DispatchStart::Message(message) => {
                                    app.show_dispatch_message(message)
                                }
                                DispatchStart::Started(started) => {
                                    app.begin_action(&started.key, started.verb);
                                    inflight = Some(started);
                                }
                            }
                        }
                    }
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }

    terminal
        .show_cursor()
        .context("restore TUI browser cursor")?;
    Ok(())
}

fn start_origin_fetch(
    app: &mut AppState,
    origin_fetch: &mut Option<dispatch::InFlightOriginFetch>,
    origin_ctx: Option<&Ctx>,
) {
    let Some(ctx) = origin_ctx else {
        app.apply_origin_fetch(Err("origin issue fetch is unavailable here".into()), "");
        return;
    };
    if origin_fetch.is_some() {
        app.show_dispatch_message("origin issue fetch already in progress".into());
        return;
    }
    *origin_fetch = Some(dispatch::spawn_origin_fetch(ctx));
}

fn poll_origin_fetch(
    app: &mut AppState,
    origin_fetch: &mut Option<dispatch::InFlightOriginFetch>,
    pending_reply: &mut Option<mpsc::Sender<UiReply>>,
    origin_ctx: Option<&Ctx>,
) {
    let Some(current) = origin_fetch.as_mut() else {
        return;
    };
    drain_origin_fetch_requests(app, current, pending_reply, origin_ctx);

    match current.done_rx.try_recv() {
        Ok(result) => {
            finish_origin_fetch_after_done(app, current, pending_reply, origin_ctx, result);
            *origin_fetch = None;
        }
        Err(mpsc::TryRecvError::Empty) => {}
        Err(mpsc::TryRecvError::Disconnected) => {
            if app.origin_fetching() {
                app.apply_origin_fetch(
                    Err(
                        "origin issue fetch worker disconnected before reporting completion".into(),
                    ),
                    "",
                );
            }
            *origin_fetch = None;
        }
    }
}

fn drain_origin_fetch_requests(
    app: &mut AppState,
    current: &mut dispatch::InFlightOriginFetch,
    pending_reply: &mut Option<mpsc::Sender<UiReply>>,
    origin_ctx: Option<&Ctx>,
) {
    loop {
        match current.ui_rx.try_recv() {
            Ok(request) => handle_ui_request(app, request, pending_reply, origin_ctx),
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

fn finish_origin_fetch_after_done(
    app: &mut AppState,
    current: &mut dispatch::InFlightOriginFetch,
    pending_reply: &mut Option<mpsc::Sender<UiReply>>,
    origin_ctx: Option<&Ctx>,
    result: Result<()>,
) {
    drain_origin_fetch_requests(app, current, pending_reply, origin_ctx);
    match result {
        Ok(()) if app.origin_fetching() => {
            app.apply_origin_fetch(
                Err("origin issue fetch finished without a result".into()),
                "",
            );
        }
        Err(err) if app.origin_fetching() => {
            app.apply_origin_fetch(Err(format!("{err:#}")), "");
        }
        Ok(()) | Err(_) => {}
    }
}

fn poll_inflight(
    app: &mut AppState,
    inflight: &mut Option<InFlightAction>,
    pending_reply: &mut Option<mpsc::Sender<UiReply>>,
    origin_ctx: Option<&Ctx>,
) -> Option<(String, &'static str, Result<()>)> {
    let current = inflight.as_mut()?;
    loop {
        match current.ui_rx.try_recv() {
            Ok(request) => handle_ui_request(app, request, pending_reply, origin_ctx),
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }

    match current.done_rx.try_recv() {
        Ok(result) => Some((current.key.clone(), current.verb, result)),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => Some((
            current.key.clone(),
            current.verb,
            Err(anyhow!("worker disconnected before reporting completion")),
        )),
    }
}

fn handle_ui_request(
    app: &mut AppState,
    request: UiRequest,
    pending_reply: &mut Option<mpsc::Sender<UiReply>>,
    origin_ctx: Option<&Ctx>,
) {
    match request {
        UiRequest::Print { kind, line } => app.push_output(kind, line),
        UiRequest::Confirm {
            prompt,
            default,
            reply,
        } => {
            app.open_popup(PopupSpec::Confirm { prompt, default });
            *pending_reply = Some(reply);
        }
        UiRequest::Select {
            prompt,
            items,
            reply,
        } => {
            app.open_popup(PopupSpec::Select {
                prompt,
                items,
                multi: false,
            });
            *pending_reply = Some(reply);
        }
        UiRequest::MultiSelect {
            prompt,
            items,
            reply,
        } => {
            app.open_popup(PopupSpec::Select {
                prompt,
                items,
                multi: true,
            });
            *pending_reply = Some(reply);
        }
        UiRequest::Input {
            prompt,
            default,
            reply,
        } => {
            app.open_popup(PopupSpec::Input { prompt, default });
            *pending_reply = Some(reply);
        }
        UiRequest::OriginIssuesLoaded { provider, result } => {
            let result = reconcile_origin_issues(origin_ctx, &provider, result);
            app.apply_origin_fetch(result, "just now");
        }
    }
}

fn reconcile_origin_issues(
    origin_ctx: Option<&Ctx>,
    provider: &str,
    result: std::result::Result<Vec<crate::services::issues::IssueListItem>, String>,
) -> std::result::Result<Vec<BrowserRow>, String> {
    let issues = result?;
    let ctx = origin_ctx.ok_or_else(|| "origin issue fetch is unavailable here".to_string())?;
    let local_origin_keys = task_list::local_origin_keys(ctx).map_err(|err| format!("{err:#}"))?;
    let local_task_keys = task_list::local_task_keys(ctx).map_err(|err| format!("{err:#}"))?;
    Ok(task_list::origin_only_rows(
        issues,
        &local_origin_keys,
        &local_task_keys,
        provider,
    ))
}

fn reply_for(outcome: PopupOutcome) -> UiReply {
    match outcome {
        PopupOutcome::Bool(value) => UiReply::Bool(value),
        PopupOutcome::Index(index) => UiReply::Index(index),
        PopupOutcome::Indices(indices) => UiReply::Indices(indices),
        PopupOutcome::Text(text) => UiReply::Text(text),
        PopupOutcome::Cancelled => UiReply::Cancelled,
    }
}

fn key_input(key: KeyEvent) -> Option<KeyInput> {
    match key.code {
        KeyCode::Up => Some(KeyInput::Up),
        KeyCode::Down => Some(KeyInput::Down),
        KeyCode::PageUp => Some(KeyInput::PageUp),
        KeyCode::PageDown => Some(KeyInput::PageDown),
        KeyCode::Enter => Some(KeyInput::Enter),
        KeyCode::Esc => Some(KeyInput::Esc),
        KeyCode::Backspace => Some(KeyInput::Backspace),
        KeyCode::Char(ch) if key.modifiers == KeyModifiers::CONTROL && ch != 'c' => {
            Some(KeyInput::CtrlChar(ch))
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            Some(KeyInput::Char(ch))
        }
        _ => None,
    }
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};
    use crate::services::issues::IssueListItem;
    use std::sync::mpsc;

    fn test_ctx(root: &std::path::Path) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                output_mode: OutputMode::Text,
                ..CtxOptions::default()
            },
        )
    }

    #[test]
    fn popup_outcome_maps_to_ui_reply_exhaustively() {
        assert_eq!(reply_for(PopupOutcome::Bool(true)), UiReply::Bool(true));
        assert_eq!(reply_for(PopupOutcome::Index(2)), UiReply::Index(2));
        assert_eq!(
            reply_for(PopupOutcome::Indices(vec![0, 1])),
            UiReply::Indices(vec![0, 1])
        );
        assert_eq!(
            reply_for(PopupOutcome::Text("WT-7".into())),
            UiReply::Text("WT-7".into())
        );
        assert_eq!(reply_for(PopupOutcome::Cancelled), UiReply::Cancelled);
    }

    #[test]
    fn page_keys_map_to_scroll_inputs() {
        assert_eq!(
            key_input(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            Some(KeyInput::PageUp)
        );
        assert_eq!(
            key_input(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            Some(KeyInput::PageDown)
        );
    }

    #[test]
    fn key_input_maps_ctrl_d_and_ctrl_u() {
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        let ctrl_u = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(key_input(ctrl_d), Some(KeyInput::CtrlChar('d')));
        assert_eq!(key_input(ctrl_u), Some(KeyInput::CtrlChar('u')));

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(key_input(ctrl_c), None);
    }

    #[test]
    fn origin_fetch_done_drains_pending_loaded_request_before_clear() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let (ui_tx, ui_rx) = mpsc::channel();
        let (_done_tx, done_rx) = mpsc::channel();
        ui_tx
            .send(UiRequest::OriginIssuesLoaded {
                provider: "github".into(),
                result: Ok(vec![IssueListItem {
                    identifier: "175".into(),
                    title: "A".into(),
                    display: "github #175".into(),
                    hint: None,
                }]),
            })
            .unwrap();
        let mut fetch = dispatch::InFlightOriginFetch { ui_rx, done_rx };
        let mut app = AppState::new(Vec::new());
        app.set_source_view_for_test(crate::tui::app::SourceView::OriginOnly);
        app.begin_origin_fetch();
        let mut pending_reply = None;

        finish_origin_fetch_after_done(
            &mut app,
            &mut fetch,
            &mut pending_reply,
            Some(&ctx),
            Ok(()),
        );

        assert_eq!(app.visible_keys(), vec!["github:175"]);
        assert!(!app.origin_fetching());
    }
}
