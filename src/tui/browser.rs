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
    run_browser_with_backend(app, refresh, CtxBackend::new(ctx))
}

pub(crate) fn run_workflow_browser(
    ctx: &Ctx,
    app: AppState,
    refresh: impl FnMut() -> Result<(Vec<BrowserRow>, Vec<String>)>,
) -> Result<()> {
    run_browser_with_backend(app, refresh, WorkflowCtxBackend::new(ctx))
}

fn run_browser_with_backend(
    mut app: AppState,
    mut refresh: impl FnMut() -> Result<(Vec<BrowserRow>, Vec<String>)>,
    dispatch_backend: impl DispatchBackend,
) -> Result<()> {
    let _session = TerminalSession::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("open TUI browser terminal")?;
    let mut inflight: Option<InFlightAction> = None;
    let mut pending_reply: Option<mpsc::Sender<UiReply>> = None;
    let mut ctrl_c_armed = false;

    loop {
        terminal
            .draw(|frame| draw(frame, &app))
            .context("draw TUI browser")?;

        if let Some((key, verb, result)) =
            poll_inflight(&mut app, &mut inflight, &mut pending_reply)
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
            if inflight.is_some() {
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

fn poll_inflight(
    app: &mut AppState,
    inflight: &mut Option<InFlightAction>,
    pending_reply: &mut Option<mpsc::Sender<UiReply>>,
) -> Option<(String, &'static str, Result<()>)> {
    let current = inflight.as_mut()?;
    loop {
        match current.ui_rx.try_recv() {
            Ok(request) => handle_ui_request(app, request, pending_reply),
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
    }
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
        KeyCode::Enter => Some(KeyInput::Enter),
        KeyCode::Esc => Some(KeyInput::Esc),
        KeyCode::Backspace => Some(KeyInput::Backspace),
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
}
