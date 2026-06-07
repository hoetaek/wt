use crate::tui::app::{AppState, BrowserRow, KeyInput, Outcome};
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

pub(crate) fn run_browser(rows: Vec<BrowserRow>) -> Result<()> {
    let _session = TerminalSession::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("open task browser terminal")?;
    let mut app = AppState::new(rows);

    loop {
        terminal
            .draw(|frame| draw(frame, &app))
            .context("draw task browser")?;
        match event::read().context("read task browser event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if is_ctrl_c(key) {
                    break;
                }
                if let Some(input) = key_input(key)
                    && app.handle(input) == Outcome::Quit
                {
                    break;
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
    }

    terminal
        .show_cursor()
        .context("restore task browser cursor")?;
    Ok(())
}

fn key_input(key: KeyEvent) -> Option<KeyInput> {
    match key.code {
        KeyCode::Up => Some(KeyInput::Up),
        KeyCode::Down => Some(KeyInput::Down),
        KeyCode::Enter => Some(KeyInput::Enter),
        KeyCode::Esc => Some(KeyInput::Esc),
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            Some(KeyInput::Char(ch))
        }
        _ => None,
    }
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}
