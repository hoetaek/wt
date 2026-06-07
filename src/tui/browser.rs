use crate::context::Ctx;
use crate::tui::app::{AppState, BrowserRow, KeyInput, Outcome};
use crate::tui::dispatch::{
    self, CtxBackend, DispatchBackend, TerminalDispatchLifecycle, WorkflowCtxBackend,
};
use crate::tui::render::draw;
use crate::tui::terminal::TerminalSession;
use anyhow::{Context, Result};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::io;

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
    let mut session = TerminalSession::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("open TUI browser terminal")?;

    loop {
        terminal
            .draw(|frame| draw(frame, &app))
            .context("draw TUI browser")?;
        match event::read().context("read TUI browser event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if is_ctrl_c(key) {
                    break;
                }
                if let Some(input) = key_input(key) {
                    match app.handle(input) {
                        Outcome::Continue => {}
                        Outcome::Quit => break,
                        Outcome::Dispatch { key, action } => {
                            let mut lifecycle =
                                TerminalDispatchLifecycle::new(&mut session, &mut terminal);
                            dispatch::dispatch(action, &key, &dispatch_backend, &mut lifecycle)?;
                            let (rows, diagnostics) = refresh()?;
                            app.replace_rows_preserving_selection(rows, diagnostics, &key);
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
