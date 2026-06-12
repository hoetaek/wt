use crate::commands::task_list;
use crate::context::Ctx;
use crate::error::WtError;
use crate::tui::app::{
    AppState, BrowserRow, KeyInput, Mode, MouseInput, Outcome, PopupOutcome, PopupSpec,
};
use crate::tui::dispatch::{
    self, CtxBackend, DispatchBackend, DispatchStart, InFlightAction, WorkflowCtxBackend,
};
use crate::tui::remote_ui::{PrintKind, UiReply, UiRequest};
use crate::tui::render::draw;
use crate::tui::terminal::TerminalSession;
use anyhow::{Context, Result, anyhow};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

const MIN_BROWSER_WIDTH: u16 = 40;
const MIN_BROWSER_HEIGHT: u16 = 8;
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);

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
    let mut session = TerminalSession::new()?;
    sync_mouse_capture(&mut session, app.mode())?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("open TUI browser terminal")?;
    let mut inflight: Option<InFlightAction> = None;
    let mut origin_fetch: Option<dispatch::InFlightOriginFetch> = None;
    let mut pending_reply: Option<mpsc::Sender<UiReply>> = None;
    let mut ctrl_c_armed = false;
    let mut reader_refresh_tick = Instant::now();
    let mut reader_mtime: Option<SystemTime> = None;
    let mut click_tracker = ClickTracker::default();
    let mut mouse_gesture = MouseGestureTracker::default();

    loop {
        terminal
            .draw(|frame| draw(frame, &mut app))
            .context("draw TUI browser")?;

        if app.reader_open() && reader_mtime.is_none() {
            reader_mtime = app.selected_row_mtime();
        }
        if app.reader_open() && reader_refresh_tick.elapsed() >= Duration::from_secs(1) {
            reader_refresh_tick = Instant::now();
            let current_mtime = app.selected_row_mtime();
            let decision = reader_refresh_decision(true, reader_mtime, current_mtime);
            if decision.refresh {
                if app.refresh_reader_body(true) {
                    reader_mtime = decision.next_mtime;
                }
            } else {
                reader_mtime = decision.next_mtime;
            }
        } else if !app.reader_open() {
            let decision = reader_refresh_decision(false, reader_mtime, None);
            reader_mtime = decision.next_mtime;
        }

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
                click_tracker.reset();
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

                    let was_reader_open = app.reader_open();
                    let outcome = app.handle(input);
                    if app.reader_open() && reader_mtime.is_none() {
                        reader_mtime = app.selected_row_mtime();
                        reader_refresh_tick = Instant::now();
                    } else if was_reader_open && !app.reader_open() {
                        reader_mtime = None;
                    }

                    match outcome {
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
                        Outcome::CopyRows { count, text } => {
                            match arboard::Clipboard::new()
                                .and_then(|mut clipboard| clipboard.set_text(text))
                            {
                                Ok(()) => {
                                    app.show_dispatch_message(format!("{count} row(s) copied"))
                                }
                                Err(err) => {
                                    app.show_dispatch_message(format!("clipboard 사용 불가: {err}"))
                                }
                            }
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
                    sync_mouse_capture(&mut session, app.mode())?;
                }
            }
            Event::Mouse(mouse) => {
                if let Some(input) = mouse_gesture.input(&app, mouse) {
                    let input = mouse_input_for_app(input, &mut click_tracker, Instant::now());
                    let was_reader_open = app.reader_open();
                    let outcome = app.handle_mouse(input);
                    if app.reader_open() && reader_mtime.is_none() {
                        reader_mtime = app.selected_row_mtime();
                        reader_refresh_tick = Instant::now();
                    } else if was_reader_open && !app.reader_open() {
                        reader_mtime = None;
                    }
                    if outcome == Outcome::Quit {
                        break;
                    }
                    sync_mouse_capture(&mut session, app.mode())?;
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

fn mode_wants_mouse_capture(mode: Mode) -> bool {
    mode != Mode::Reader
}

fn sync_mouse_capture(session: &mut TerminalSession, mode: Mode) -> Result<()> {
    session
        .set_mouse_capture(mode_wants_mouse_capture(mode))
        .context("sync TUI browser mouse capture")
}

#[derive(Default)]
struct ClickTracker {
    last_click: Option<(usize, Instant)>,
    pending_double: Option<usize>,
}

impl ClickTracker {
    fn reset(&mut self) {
        self.last_click = None;
        self.pending_double = None;
    }

    fn down(&mut self, visible_index: usize, now: Instant) {
        self.pending_double = None;
        if let Some((last_visible_index, at)) = self.last_click
            && last_visible_index == visible_index
            && now.duration_since(at) <= DOUBLE_CLICK_INTERVAL
        {
            self.pending_double = Some(visible_index);
        }
    }

    fn drag(&mut self) {
        self.reset();
    }

    fn up(&mut self, visible_index: usize, now: Instant) -> bool {
        if self.pending_double == Some(visible_index) {
            self.reset();
            return true;
        }

        self.pending_double = None;
        self.last_click = Some((visible_index, now));
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserMouseInput {
    Down { visible_index: usize },
    Drag { visible_index: usize },
    Up { visible_index: usize, dragged: bool },
}

#[derive(Default)]
struct MouseGestureTracker {
    active: Option<ActiveMouseGesture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveMouseGesture {
    visible_index: usize,
    dragged: bool,
}

impl MouseGestureTracker {
    fn input(&mut self, app: &AppState, mouse: MouseEvent) -> Option<BrowserMouseInput> {
        if matches!(app.mode(), Mode::Reader | Mode::Menu | Mode::FilterInput) || app.has_popup() {
            self.active = None;
            return None;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let target = crate::tui::render::table_mouse_target(app, mouse.column, mouse.row);
                self.active = target.map(|visible_index| ActiveMouseGesture {
                    visible_index,
                    dragged: false,
                });
                target.map(|visible_index| BrowserMouseInput::Down { visible_index })
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let active = self.active.as_mut()?;
                let Some(visible_index) =
                    crate::tui::render::table_mouse_target(app, mouse.column, mouse.row)
                else {
                    active.dragged = true;
                    return None;
                };
                if visible_index == active.visible_index && !active.dragged {
                    return None;
                }
                active.dragged = true;
                Some(BrowserMouseInput::Drag { visible_index })
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.active.take().map(|gesture| BrowserMouseInput::Up {
                    visible_index: gesture.visible_index,
                    dragged: gesture.dragged,
                })
            }
            _ => None,
        }
    }
}

fn mouse_input_for_app(
    input: BrowserMouseInput,
    click_tracker: &mut ClickTracker,
    now: Instant,
) -> MouseInput {
    match input {
        BrowserMouseInput::Down { visible_index } => {
            click_tracker.down(visible_index, now);
            MouseInput::Down { visible_index }
        }
        BrowserMouseInput::Drag { visible_index } => {
            click_tracker.drag();
            MouseInput::Drag { visible_index }
        }
        BrowserMouseInput::Up {
            visible_index,
            dragged,
        } => {
            if dragged {
                click_tracker.drag();
                return MouseInput::Up;
            }
            if click_tracker.up(visible_index, now) {
                MouseInput::DoubleClick { visible_index }
            } else {
                MouseInput::Up
            }
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReaderRefreshDecision {
    refresh: bool,
    next_mtime: Option<SystemTime>,
}

fn reader_refresh_decision(
    reader_open: bool,
    baseline: Option<SystemTime>,
    current_mtime: Option<SystemTime>,
) -> ReaderRefreshDecision {
    if !reader_open {
        return ReaderRefreshDecision {
            refresh: false,
            next_mtime: None,
        };
    }
    let refresh = baseline.is_some() && current_mtime.is_some() && current_mtime != baseline;
    let next_mtime = current_mtime.or(baseline);
    ReaderRefreshDecision {
        refresh,
        next_mtime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};
    use crate::origin_action_menu::{OriginActionMenu, OriginLabel};
    use crate::services::issues::IssueListItem;
    use crate::tui::app::TableMouseLayout;
    use ratatui::layout::Rect;
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

    fn row(key: &str) -> BrowserRow {
        BrowserRow {
            key: key.into(),
            title: key.into(),
            status: "stale".into(),
            run_status: "new".into(),
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
            duration: None,
            size: None,
            branch: Some(format!("feature/{key}")),
            source: "local".into(),
            path: None,
            body: String::new(),
            preview_lines: vec!["Origin      Linear WT-142".into()],
            menu: OriginActionMenu::for_origin_task(key, key, OriginLabel::new("linear", "WT-142")),
        }
    }

    fn app_with_mouse_layout() -> AppState {
        let app = AppState::new(vec![row("a"), row("b")]);
        app.set_table_mouse_layout(Some(TableMouseLayout {
            rows: Rect::new(1, 1, 10, 2),
            first_visible_index: 0,
            visible_count: 2,
        }));
        app
    }

    fn left_mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn handle_browser_mouse(
        app: &mut AppState,
        gesture: &mut MouseGestureTracker,
        clicks: &mut ClickTracker,
        mouse: MouseEvent,
        now: Instant,
    ) {
        if let Some(input) = gesture.input(app, mouse) {
            let input = mouse_input_for_app(input, clicks, now);
            app.handle_mouse(input);
        }
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
    fn reader_refresh_decision_initializes_baseline_without_refresh() {
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1);

        assert_eq!(
            reader_refresh_decision(true, None, Some(mtime)),
            ReaderRefreshDecision {
                refresh: false,
                next_mtime: Some(mtime),
            }
        );
    }

    #[test]
    fn reader_refresh_decision_detects_changed_mtime() {
        let before = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let after = SystemTime::UNIX_EPOCH + Duration::from_secs(2);

        assert_eq!(
            reader_refresh_decision(true, Some(before), Some(after)),
            ReaderRefreshDecision {
                refresh: true,
                next_mtime: Some(after),
            }
        );
    }

    #[test]
    fn reader_refresh_decision_resets_baseline_when_reader_closes() {
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1);

        assert_eq!(
            reader_refresh_decision(false, Some(mtime), Some(mtime)),
            ReaderRefreshDecision {
                refresh: false,
                next_mtime: None,
            }
        );
    }

    #[test]
    fn double_click_same_row_only_within_window() {
        let mut tracker = ClickTracker::default();
        let now = Instant::now();

        tracker.down(0, now);
        assert!(!tracker.up(0, now));
        tracker.down(0, now + Duration::from_millis(399));
        assert!(tracker.up(0, now + Duration::from_millis(399)));

        let mut tracker = ClickTracker::default();
        tracker.down(0, now);
        assert!(!tracker.up(0, now));
        tracker.down(1, now + Duration::from_millis(100));
        assert!(!tracker.up(1, now + Duration::from_millis(100)));

        let mut tracker = ClickTracker::default();
        tracker.down(0, now);
        assert!(!tracker.up(0, now));
        tracker.down(0, now + Duration::from_millis(401));
        assert!(!tracker.up(0, now + Duration::from_millis(401)));
    }

    #[test]
    fn drag_resets_double_click_tracking() {
        let mut tracker = ClickTracker::default();
        let now = Instant::now();

        tracker.down(0, now);
        assert!(!tracker.up(0, now));
        tracker.down(0, now + Duration::from_millis(100));
        tracker.drag();

        assert!(!tracker.up(0, now + Duration::from_millis(100)));
    }

    #[test]
    fn second_click_drag_selects_range_instead_of_opening_reader() {
        let mut app = app_with_mouse_layout();
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now + Duration::from_millis(100),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Drag(MouseButton::Left), 2, 2),
            now + Duration::from_millis(110),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 2),
            now + Duration::from_millis(120),
        );

        assert_eq!(app.mode(), Mode::List);
        assert!(!app.reader_open());
        assert!(app.is_row_marked("a"));
        assert!(app.is_row_marked("b"));
    }

    #[test]
    fn second_click_up_opens_reader_within_double_click_window() {
        let mut app = app_with_mouse_layout();
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now + Duration::from_millis(100),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now + Duration::from_millis(120),
        );

        assert_eq!(app.mode(), Mode::Reader);
    }

    #[test]
    fn drag_release_does_not_seed_next_click_as_double_click() {
        let mut app = app_with_mouse_layout();
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Drag(MouseButton::Left), 2, 2),
            now + Duration::from_millis(10),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 2),
            now + Duration::from_millis(20),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now + Duration::from_millis(100),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now + Duration::from_millis(120),
        );

        assert_eq!(app.mode(), Mode::List);
        assert!(!app.reader_open());
    }

    #[test]
    fn same_row_drag_jitter_remains_click_and_can_seed_double_click() {
        let mut app = app_with_mouse_layout();
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Drag(MouseButton::Left), 2, 1),
            now + Duration::from_millis(10),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now + Duration::from_millis(20),
        );

        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.selected_key_count(), 0);
        assert!(!app.reader_open());

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now + Duration::from_millis(100),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now + Duration::from_millis(120),
        );

        assert_eq!(app.mode(), Mode::Reader);
    }

    #[test]
    fn same_row_jitter_then_other_row_drag_selects_two_rows() {
        let mut app = app_with_mouse_layout();
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Drag(MouseButton::Left), 2, 1),
            now + Duration::from_millis(10),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Drag(MouseButton::Left), 2, 2),
            now + Duration::from_millis(20),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 2),
            now + Duration::from_millis(30),
        );

        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.selected_key_count(), 2);
        assert!(app.is_row_marked("a"));
        assert!(app.is_row_marked("b"));
    }

    #[test]
    fn outside_drag_marks_gesture_without_seeding_double_click() {
        let mut app = app_with_mouse_layout();
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Drag(MouseButton::Left), 0, 0),
            now + Duration::from_millis(10),
        );
        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.selected_key_count(), 0);

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 0, 0),
            now + Duration::from_millis(20),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now + Duration::from_millis(100),
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now + Duration::from_millis(120),
        );

        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.selected_key_count(), 0);
        assert!(!app.reader_open());
    }

    #[test]
    fn outside_mouse_release_does_not_commit_keyboard_range_select() {
        let mut app = app_with_mouse_layout();
        app.handle(KeyInput::Char('v'));
        assert_eq!(app.mode(), Mode::RangeSelect);
        let mut tracker = MouseGestureTracker::default();

        let down = tracker.input(
            &app,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
        );
        let up = tracker.input(
            &app,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 0, 0),
        );

        assert_eq!(down, None);
        assert_eq!(up, None);
        assert_eq!(app.mode(), Mode::RangeSelect);
    }

    #[test]
    fn row_mouse_release_commits_active_range_select_gesture() {
        let mut app = app_with_mouse_layout();
        app.handle(KeyInput::Char('v'));
        assert_eq!(app.mode(), Mode::RangeSelect);
        let mut gesture = MouseGestureTracker::default();
        let mut clicks = ClickTracker::default();
        let now = Instant::now();

        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Down(MouseButton::Left), 2, 1),
            now,
        );
        handle_browser_mouse(
            &mut app,
            &mut gesture,
            &mut clicks,
            left_mouse(MouseEventKind::Up(MouseButton::Left), 2, 1),
            now,
        );

        assert_eq!(app.mode(), Mode::List);
    }

    #[test]
    fn reader_refresh_decision_keeps_baseline_during_temporary_missing_mtime() {
        let before = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let after = SystemTime::UNIX_EPOCH + Duration::from_secs(2);

        assert_eq!(
            reader_refresh_decision(true, Some(before), None),
            ReaderRefreshDecision {
                refresh: false,
                next_mtime: Some(before),
            }
        );
        assert_eq!(
            reader_refresh_decision(true, Some(before), Some(after)),
            ReaderRefreshDecision {
                refresh: true,
                next_mtime: Some(after),
            }
        );
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
