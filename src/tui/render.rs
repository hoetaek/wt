use crate::tui::app::{AppState, BrowserRow, Mode};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};

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

    if app.mode() == Mode::Menu {
        draw_menu(frame, centered_rect(area), app);
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let mut summary = if app.is_empty() {
        vec![Span::raw(format!(
            "Origin health: {}",
            app.empty_origin_summary()
        ))]
    } else {
        let mut spans = vec![Span::raw(format!(
            "Origin health: {} {}",
            app.row_count(),
            app.origin_count_label()
        ))];
        for (status, count) in app.origin_status_counts() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(status.to_string(), status_style(status)));
            spans.push(Span::raw(format!(" {count}")));
        }
        spans
    };
    if !app.diagnostics().is_empty() {
        summary.push(Span::raw("  "));
        summary.push(Span::raw(app.diagnostics().join("  ")));
    }
    let header = Paragraph::new(Line::from(summary)).block(chrome_block());
    frame.render_widget(header, area);
}

fn draw_rows(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    if app.is_empty() {
        let empty = Paragraph::new(app.empty_inventory_message()).block(
            Block::default()
                .title(app.inventory_title())
                .borders(Borders::ALL)
                .border_style(theme::chrome_style())
                .title_style(theme::chrome_style()),
        );
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
    .header(
        Row::new(vec![
            "status",
            "origin",
            "title",
            app.key_column_title(),
            "next",
        ])
        .style(theme::chrome_style()),
    )
    .column_spacing(1)
    .block(
        Block::default()
            .title(app.inventory_title())
            .borders(Borders::ALL)
            .border_style(theme::chrome_style())
            .title_style(theme::chrome_style()),
    );
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
        Cell::from(row.status.as_str()).style(status_style(&row.status)),
        Cell::from(row.origin_label.as_str()),
        Cell::from(row.title.as_str()),
        Cell::from(row.key.as_str()),
        Cell::from(row.next_action.as_str()),
    ])
}

fn row_style(app: &AppState, index: usize) -> Style {
    if app.selected_visible_index() == Some(index) {
        theme::selected_style()
    } else {
        Style::default()
    }
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let mut lines = app
        .diagnostics()
        .iter()
        .map(|line| Line::from(line.as_str()))
        .collect::<Vec<_>>();
    if !lines.is_empty() && app.selected_row().is_some() {
        lines.push(Line::from(""));
    }
    lines.extend(
        app.selected_row()
            .map(|row| {
                row.preview_lines
                    .iter()
                    .map(|line| Line::from(line.as_str()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![Line::from(app.no_selection_message())]),
    );
    let title = app
        .selected_row()
        .map(|row| format!("Preview {}", row.title))
        .unwrap_or_else(|| "Preview".into());
    let preview = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::chrome_style())
            .title_style(theme::chrome_style()),
    );
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
    frame.render_widget(Paragraph::new(Line::styled(line, theme::dim_style())), area);
}

fn centered_rect(area: Rect) -> Rect {
    let [area] = Layout::vertical([Constraint::Length(14)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::horizontal([Constraint::Percentage(70)])
        .flex(Flex::Center)
        .areas(area);
    area
}

fn draw_menu(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let Some(row) = app.selected_row() else {
        return;
    };
    let selected_index = app.selected_menu_index();
    let mut lines = Vec::new();
    let mut disabled_header_added = false;

    for (index, item) in row.menu.items().iter().enumerate() {
        if !item.is_enabled() && !disabled_header_added {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::styled("disabled:", theme::dim_style()));
            disabled_header_added = true;
        }
        let mut text = item.render_plain();
        if item.is_external_write() && !text.contains("External write") {
            text.push_str("  External write; confirmation required");
        }
        if index == selected_index {
            text = format!("> {text}");
        } else {
            text = format!("  {text}");
        }
        lines.push(menu_item_line(text, item));
    }

    lines.push(Line::from(""));
    lines
        .push(Line::styled("Enter run  Esc back", theme::dim_style()).alignment(Alignment::Center));
    let menu = Paragraph::new(lines).block(
        Block::default()
            .title(row.title.as_str())
            .borders(Borders::ALL)
            .border_style(theme::chrome_style())
            .title_style(theme::chrome_style()),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(menu, area);
}

fn chrome_block<'a>() -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(theme::chrome_style())
        .title_style(theme::chrome_style())
}

fn status_style(status: &str) -> Style {
    theme::status_color(status)
        .map(|color| Style::default().fg(color))
        .unwrap_or_default()
}

fn menu_item_line<'a>(
    text: String,
    item: &crate::origin_action_menu::OriginActionItem,
) -> Line<'a> {
    if !item.is_enabled() {
        return Line::styled(text, theme::dim_style());
    }

    if item.is_external_write() {
        styled_segment_line(
            text,
            "External write; confirmation required",
            theme::external_write_style(),
        )
    } else {
        Line::from(text)
    }
}

fn styled_segment_line<'a>(text: String, segment: &str, style: Style) -> Line<'a> {
    let Some(start) = text.find(segment) else {
        return Line::from(text);
    };
    let end = start + segment.len();
    Line::from(vec![
        Span::raw(text[..start].to_string()),
        Span::styled(text[start..end].to_string(), style),
        Span::raw(text[end..].to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin_action_menu::{OriginActionMenu, OriginLabel};
    use crate::tui::app::{AppState, BrowserRow, KeyInput};
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use ratatui::{Terminal, backend::TestBackend};

    struct ColorGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        prev: bool,
    }

    impl ColorGuard {
        fn set(enabled: bool) -> Self {
            let lock = crate::tui::theme::COLOR_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev = console::colors_enabled();
            console::set_colors_enabled(enabled);
            Self { _lock: lock, prev }
        }
    }

    impl Drop for ColorGuard {
        fn drop(&mut self) {
            console::set_colors_enabled(self.prev);
        }
    }

    fn render_buffer(width: u16, height: u16, app: &AppState) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_text(width: u16, height: u16, app: &AppState) -> String {
        let buffer = render_buffer(width, height, app);
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn row(key: &str, title: &str, status: &str) -> BrowserRow {
        BrowserRow {
            key: key.into(),
            title: title.into(),
            status: status.into(),
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
            preview_lines: vec!["Origin      Linear WT-142".into()],
            menu: OriginActionMenu::for_origin_task(
                key,
                title,
                OriginLabel::new("linear", "WT-142"),
            ),
        }
    }

    fn line_contains_text(buffer: &Buffer, width: u16, y: u16, text: &str) -> bool {
        let chars = text.chars().collect::<Vec<_>>();
        (0..=width.saturating_sub(chars.len() as u16)).any(|x| {
            chars
                .iter()
                .enumerate()
                .all(|(offset, ch)| buffer[(x + offset as u16, y)].symbol() == ch.to_string())
        })
    }

    fn first_cell_of_text_has_fg_on_line_with(
        buffer: &Buffer,
        width: u16,
        height: u16,
        text: &str,
        same_line_text: &str,
        color: Color,
    ) -> bool {
        let chars = text.chars().collect::<Vec<_>>();
        (0..height).any(|y| {
            line_contains_text(buffer, width, y, same_line_text)
                && (0..=width.saturating_sub(chars.len() as u16)).any(|x| {
                    chars.iter().enumerate().all(|(offset, ch)| {
                        buffer[(x + offset as u16, y)].symbol() == ch.to_string()
                    }) && buffer[(x, y)].style().fg == Some(color)
                })
        })
    }

    fn has_colored_cell(buffer: &Buffer, width: u16, height: u16) -> bool {
        (0..height).any(|y| {
            (0..width).any(|x| {
                matches!(
                    buffer[(x, y)].style().fg,
                    Some(Color::Red)
                        | Some(Color::Yellow)
                        | Some(Color::Green)
                        | Some(Color::Indexed(_))
                )
            })
        })
    }

    #[test]
    fn renders_header_rows_and_preview() {
        let app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        let text = buffer_text(80, 24, &app);
        assert!(text.contains("conflict"));
        assert!(text.contains("Origin sync TUI"));
        assert!(text.contains("Linear WT-142"));
        assert!(text.contains("q quit"));
    }

    #[test]
    fn status_cell_uses_semantic_color_when_enabled() {
        let _guard = ColorGuard::set(true);
        let app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        let buffer = render_buffer(80, 24, &app);

        assert!(
            first_cell_of_text_has_fg_on_line_with(
                &buffer,
                80,
                24,
                "conflict",
                "Linear WT-142",
                Color::Red
            ),
            "conflict status cell should be red"
        );
    }

    #[test]
    fn render_is_colorless_when_colors_disabled() {
        let _guard = ColorGuard::set(false);
        let app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        let buffer = render_buffer(80, 24, &app);

        assert!(
            !has_colored_cell(&buffer, 80, 24),
            "render should not contain semantic or chrome colors when colors are disabled"
        );
    }

    #[test]
    fn renders_empty_state_with_guidance() {
        let app = AppState::new(vec![]);
        let text = buffer_text(80, 24, &app);
        assert!(text.contains("No actionable tasks"));
    }

    #[test]
    fn workflow_browser_uses_workflow_inventory_copy() {
        let app = AppState::workflow_with_diagnostics(
            vec![row("2026-06-06-001", "Ship provider-origin UX", "stale")],
            Vec::new(),
        );

        let text = buffer_text(100, 24, &app);

        assert!(text.contains("saved workflows"));
        assert!(text.contains("Workflows"));
        assert!(text.contains("workflow"));
        assert!(!text.contains("actionable tasks"));
        assert!(!text.contains("No task selected"));
    }

    #[test]
    fn empty_workflow_browser_uses_workflow_empty_state() {
        let app = AppState::workflow_with_diagnostics(Vec::new(), Vec::new());

        let text = buffer_text(100, 24, &app);

        assert!(text.contains("No saved workflows"));
        assert!(text.contains("Workflows"));
        assert!(!text.contains("No actionable tasks"));
        assert!(!text.contains("Tasks"));
    }

    #[test]
    fn narrow_terminal_omits_preview_panel() {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.preview_lines = vec!["PREVIEW-MARKER".into()];
        let app = AppState::new(vec![browser_row]);
        let text = buffer_text(80, 10, &app);
        assert!(!text.contains("PREVIEW-MARKER"));
        assert!(text.contains("Origin sync TUI"));
    }

    #[test]
    fn renders_action_menu_overlay() {
        let mut app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        app.handle(KeyInput::Enter);

        let text = buffer_text(100, 24, &app);

        assert!(text.contains("Diff with issue"));
        assert!(text.contains("disabled:"));
        assert!(text.contains("External write"));
        assert!(text.contains("Enter run"));
    }

    #[test]
    fn selected_row_remains_visible_when_list_scrolls() {
        let mut app = AppState::new(
            (0..30)
                .map(|idx| {
                    let mut browser_row =
                        row(&format!("task-{idx}"), &format!("Task {idx}"), "stale");
                    browser_row.origin_label = format!("Linear WT-{idx}");
                    browser_row.next_action = "fetch".into();
                    browser_row.preview_lines = vec![format!("Origin      Linear WT-{idx}")];
                    browser_row
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
