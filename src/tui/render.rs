use crate::tui::app::{AppState, BrowserRow, Mode};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

const PREVIEW_MIN_HEIGHT: u16 = 16;

pub(crate) fn draw(frame: &mut Frame<'_>, app: &AppState) {
    let area = frame.area();
    let show_preview = area.height >= PREVIEW_MIN_HEIGHT;
    let chunks = if show_preview {
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area)
    };

    draw_header(frame, chunks[0], app);
    draw_rows(frame, chunks[1], app);
    if show_preview {
        draw_preview(frame, chunks[2], app);
        draw_status(frame, chunks[3], app);
    } else {
        draw_status(frame, chunks[2], app);
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let mut summary = if app.is_empty() {
        "Origin health: no actionable tasks".to_string()
    } else {
        let counts = app
            .origin_status_counts()
            .into_iter()
            .map(|(status, count)| format!("{status} {count}"))
            .collect::<Vec<_>>()
            .join("  ");
        format!(
            "Origin health: {} actionable tasks  {counts}",
            app.row_count()
        )
    };
    if !app.diagnostics().is_empty() {
        summary.push_str("  ");
        summary.push_str(&app.diagnostics().join("  "));
    }
    let header = Paragraph::new(summary).block(Block::default().borders(Borders::ALL));
    frame.render_widget(header, area);
}

fn draw_rows(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    if app.is_empty() {
        let empty = Paragraph::new("No actionable tasks")
            .block(Block::default().title("Tasks").borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    }

    let visible_rows = app.visible_rows();
    let selected_index = app.selected_visible_index().unwrap_or(0);
    let row_capacity = table_row_capacity(area);
    let offset = row_viewport_offset(selected_index, row_capacity);
    let rows = visible_rows
        .into_iter()
        .enumerate()
        .skip(offset)
        .take(row_capacity)
        .map(|(index, row)| browser_row(row).style(row_style(app, index)));
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(18),
            Constraint::Min(18),
            Constraint::Length(24),
            Constraint::Length(8),
        ],
    )
    .header(Row::new(["status", "origin", "title", "task", "next"]))
    .column_spacing(1)
    .block(Block::default().title("Tasks").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn table_row_capacity(area: Rect) -> usize {
    area.height.saturating_sub(3) as usize
}

fn row_viewport_offset(selected_index: usize, row_capacity: usize) -> usize {
    if row_capacity == 0 {
        selected_index
    } else {
        selected_index.saturating_sub(row_capacity - 1)
    }
}

fn browser_row(row: &BrowserRow) -> Row<'_> {
    Row::new(vec![
        Cell::from(row.status.as_str()),
        Cell::from(row.origin_label.as_str()),
        Cell::from(row.title.as_str()),
        Cell::from(row.key.as_str()),
        Cell::from(row.next_action.as_str()),
    ])
}

fn row_style(app: &AppState, index: usize) -> Style {
    if app.selected_visible_index() == Some(index) {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    }
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let lines = app
        .selected_row()
        .map(|row| {
            row.preview_lines
                .iter()
                .map(|line| Line::from(line.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![Line::from("No task selected")]);
    let title = app
        .selected_row()
        .map(|row| format!("Preview {}", row.title))
        .unwrap_or_else(|| "Preview".into());
    let preview = Paragraph::new(lines).block(Block::default().title(title).borders(Borders::ALL));
    frame.render_widget(preview, area);
}

fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let line = if app.mode() == Mode::FilterInput {
        format!("filter: {}  Esc clear", app.filter())
    } else if !app.diagnostics().is_empty() {
        format!("{}  {}", app.diagnostics().join("  "), app.status_line())
    } else {
        app.status_line().to_string()
    };
    frame.render_widget(Paragraph::new(line), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::{AppState, BrowserRow, KeyInput};
    use ratatui::{Terminal, backend::TestBackend};

    fn buffer_text(width: u16, height: u16, app: &AppState) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_header_rows_and_preview() {
        let app = AppState::new(vec![BrowserRow {
            key: "origin-sync-tui".into(),
            title: "Origin sync TUI".into(),
            status: "conflict".into(),
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
            preview_lines: vec!["Origin      Linear WT-142".into()],
        }]);
        let text = buffer_text(80, 24, &app);
        assert!(text.contains("conflict"));
        assert!(text.contains("Origin sync TUI"));
        assert!(text.contains("Linear WT-142"));
        assert!(text.contains("q quit"));
    }

    #[test]
    fn renders_empty_state_with_guidance() {
        let app = AppState::new(vec![]);
        let text = buffer_text(80, 24, &app);
        assert!(text.contains("No actionable tasks"));
    }

    #[test]
    fn narrow_terminal_omits_preview_panel() {
        let app = AppState::new(vec![BrowserRow {
            key: "origin-sync-tui".into(),
            title: "Origin sync TUI".into(),
            status: "conflict".into(),
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
            preview_lines: vec!["PREVIEW-MARKER".into()],
        }]);
        let text = buffer_text(80, 10, &app);
        assert!(!text.contains("PREVIEW-MARKER"));
        assert!(text.contains("Origin sync TUI"));
    }

    #[test]
    fn selected_row_remains_visible_when_list_scrolls() {
        let mut app = AppState::new(
            (0..30)
                .map(|idx| BrowserRow {
                    key: format!("task-{idx}"),
                    title: format!("Task {idx}"),
                    status: "stale".into(),
                    origin_label: format!("Linear WT-{idx}"),
                    next_action: "fetch".into(),
                    preview_lines: vec![format!("Origin      Linear WT-{idx}")],
                })
                .collect(),
        );
        for _ in 0..29 {
            app.handle(KeyInput::Down);
        }

        let text = buffer_text(80, 12, &app);

        assert!(text.contains("task-29"));
    }
}
