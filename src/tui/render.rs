use crate::tui::app::{
    AppState, BrowserCell, BrowserColumn, BrowserColumnWidth, BrowserRow, Mode, PopupView,
    SidebarPosition,
};
use crate::tui::body_markup::LineKind;
use crate::tui::remote_ui::PrintKind;
use crate::tui::theme;
use console::measure_text_width;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};

const SIDEBAR_RIGHT_MIN_LIST_WIDTH: u16 = 80;
const SIDEBAR_RIGHT_MIN_WIDTH: u16 = 32;
const SIDEBAR_BOTTOM_MIN_WIDTH: u16 = 50;
const SIDEBAR_BOTTOM_HEIGHT: u16 = 7;
const SPINNER_FRAMES: [char; 4] = ['|', '/', '-', '\\'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailSidebarLayout {
    Hidden,
    Right(u16),
    Bottom,
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut AppState) {
    let area = frame.area();
    let show_body = app.body_view_open();
    let show_output = !app.output_lines().is_empty();
    let header_height = header_height(app, area);
    let detail_layout = if show_body {
        DetailSidebarLayout::Hidden
    } else {
        resolve_detail_sidebar_layout(area, app, header_height, show_output)
    };
    let row_min_height = if show_body || matches!(detail_layout, DetailSidebarLayout::Bottom) {
        5
    } else {
        1
    };
    let mut constraints = vec![
        Constraint::Length(header_height),
        Constraint::Min(row_min_height),
    ];
    if show_output {
        constraints.push(Constraint::Length(output_panel_height(area)));
    }
    if matches!(detail_layout, DetailSidebarLayout::Bottom) {
        constraints.push(Constraint::Length(SIDEBAR_BOTTOM_HEIGHT));
    }
    constraints.push(Constraint::Length(1));

    let chunks = Layout::vertical(constraints).split(area);
    let mut chunk_index = 0;
    draw_header(frame, chunks[chunk_index], app);
    chunk_index += 1;
    if show_body {
        draw_body_view(frame, chunks[chunk_index], app);
    } else if let DetailSidebarLayout::Right(sidebar_width) = detail_layout {
        let row_chunks =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(sidebar_width)])
                .split(chunks[chunk_index]);
        draw_rows(frame, row_chunks[0], app);
        draw_detail_sidebar(frame, row_chunks[1], app);
    } else {
        draw_rows(frame, chunks[chunk_index], app);
    }
    chunk_index += 1;
    if show_output {
        draw_output_panel(frame, chunks[chunk_index], app);
        chunk_index += 1;
    }
    if matches!(detail_layout, DetailSidebarLayout::Bottom) {
        draw_detail_sidebar(frame, chunks[chunk_index], app);
        chunk_index += 1;
    }
    draw_status(frame, chunks[chunk_index], app);

    if app.mode() == Mode::Menu {
        draw_menu(frame, centered_rect(area), app);
    }
    if app.help_open() {
        draw_help_popup(frame, area, app);
    }
    if let Some(popup) = app.popup_view() {
        draw_popup(frame, area, popup);
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let mut lines = Vec::new();
    let tabs = source_view_tab_line(app, area.width);
    if let Some(tabs) = tabs {
        lines.push(tabs);
    }
    if let Some(search) = search_line(app) {
        lines.push(search);
    }
    lines.push(summary_line(app));

    let header = Paragraph::new(lines).block(chrome_block());
    frame.render_widget(header, area);
}

fn header_height(app: &AppState, area: Rect) -> u16 {
    if area.height <= 8 {
        return 3.min(area.height.saturating_sub(1).max(1));
    }

    let mut inner_lines = 1;
    if !app.source_view_tabs().is_empty() {
        inner_lines += 1;
    }
    if app.mode() == Mode::FilterInput || !app.filter().is_empty() {
        inner_lines += 1;
    }
    (inner_lines + 2).min(area.height.saturating_sub(1).max(1))
}

fn source_view_tab_line(app: &AppState, area_width: u16) -> Option<Line<'static>> {
    let tabs = app.source_view_tabs();
    if tabs.is_empty() {
        return None;
    }

    let full_text = tabs
        .iter()
        .map(|(label, count, active)| source_view_tab_text(label, *count, *active))
        .collect::<Vec<_>>()
        .join(" | ");
    let inner_width = area_width.saturating_sub(2) as usize;
    if measure_text_width(&full_text) > inner_width {
        let (label, count, _) = tabs
            .iter()
            .find(|(_, _, active)| *active)
            .copied()
            .expect("source view tabs should include active view");
        return Some(Line::from(format!(
            "view: {}",
            source_view_tab_text(label, count, true)
        )));
    }

    let mut spans = Vec::new();
    for (index, (label, count, active)) in tabs.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" | ", theme::dim_style()));
        }
        spans.push(Span::raw(source_view_tab_text(label, count, active)));
    }
    Some(Line::from(spans))
}

fn source_view_tab_text(label: &str, count: Option<usize>, active: bool) -> String {
    let label = if active {
        format!("[{label}]")
    } else {
        label.to_string()
    };
    let count = count
        .map(|count| count.to_string())
        .unwrap_or_else(|| "-".into());
    format!("{label} ({count})")
}

fn search_line(app: &AppState) -> Option<Line<'static>> {
    if app.mode() != Mode::FilterInput && app.filter().is_empty() {
        return None;
    }
    let query = if app.filter().is_empty() {
        "type to filter"
    } else {
        app.filter()
    };
    Some(Line::from(vec![
        Span::styled("/ ", theme::dim_style()),
        Span::raw(query.to_string()),
    ]))
}

fn summary_line(app: &AppState) -> Line<'static> {
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
    Line::from(summary)
}

fn resolve_detail_sidebar_layout(
    area: Rect,
    app: &AppState,
    header_height: u16,
    show_output: bool,
) -> DetailSidebarLayout {
    if !app.sidebar_open() {
        return DetailSidebarLayout::Hidden;
    }

    let output_height = if show_output {
        output_panel_height(area)
    } else {
        0
    };
    let available_height = area
        .height
        .saturating_sub(header_height)
        .saturating_sub(output_height)
        .saturating_sub(1);
    if available_height < 5 {
        return DetailSidebarLayout::Hidden;
    }

    let right = right_sidebar_width(area.width).filter(|_| available_height >= 5);
    let bottom = bottom_sidebar_possible(area.width, available_height);

    match app.sidebar_position() {
        SidebarPosition::Auto => right
            .map(DetailSidebarLayout::Right)
            .or_else(|| bottom.then_some(DetailSidebarLayout::Bottom))
            .unwrap_or(DetailSidebarLayout::Hidden),
        SidebarPosition::Right => right
            .map(DetailSidebarLayout::Right)
            .or_else(|| bottom.then_some(DetailSidebarLayout::Bottom))
            .unwrap_or(DetailSidebarLayout::Hidden),
        SidebarPosition::Bottom => {
            if bottom {
                DetailSidebarLayout::Bottom
            } else {
                right
                    .map(DetailSidebarLayout::Right)
                    .unwrap_or(DetailSidebarLayout::Hidden)
            }
        }
    }
}

fn right_sidebar_width(width: u16) -> Option<u16> {
    let max_sidebar_width = width.saturating_sub(SIDEBAR_RIGHT_MIN_LIST_WIDTH);
    if max_sidebar_width < SIDEBAR_RIGHT_MIN_WIDTH {
        return None;
    }
    let desired = width.saturating_mul(35) / 100;
    Some(desired.clamp(SIDEBAR_RIGHT_MIN_WIDTH, max_sidebar_width))
}

fn bottom_sidebar_possible(width: u16, available_height: u16) -> bool {
    width >= SIDEBAR_BOTTOM_MIN_WIDTH && available_height >= SIDEBAR_BOTTOM_HEIGHT + 5
}

fn draw_rows(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    if let Some(message) = app.empty_state_message() {
        let empty = Paragraph::new(message).block(
            Block::default()
                .title(app.inventory_title_line())
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
    let columns = app.columns();
    let effective_widths = effective_column_widths(columns, area.width);
    let rows = visible_rows
        .into_iter()
        .enumerate()
        .skip(offset)
        .take(row_capacity)
        .map(|(index, row)| {
            browser_row(row, columns, &effective_widths).style(row_style(app, index))
        });
    let constraints = columns
        .iter()
        .map(|column| match column.width {
            BrowserColumnWidth::Length(width) => Constraint::Length(width),
            BrowserColumnWidth::Min(width) => Constraint::Min(width),
        })
        .collect::<Vec<_>>();
    let table = Table::new(rows, constraints)
        .header(
            Row::new(
                columns
                    .iter()
                    .map(|column| column.title.as_str())
                    .collect::<Vec<_>>(),
            )
            .style(theme::chrome_style()),
        )
        .column_spacing(1)
        .block(
            Block::default()
                .title(app.inventory_title_line())
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

fn effective_column_widths(columns: &[BrowserColumn], area_width: u16) -> Vec<usize> {
    let mut widths = columns
        .iter()
        .map(browser_column_width_hint)
        .collect::<Vec<_>>();
    let spacing = columns.len().saturating_sub(1);
    let total_width = widths.iter().sum::<usize>() + spacing;
    let inner_width = area_width.saturating_sub(2) as usize;
    let overflow = total_width.saturating_sub(inner_width);
    if overflow > 0 {
        if let Some(task_index) = columns
            .iter()
            .position(|column| column.cell == BrowserCell::Task)
        {
            widths[task_index] = widths[task_index].saturating_sub(overflow).max(1);
        }
    }
    widths
}

fn browser_row<'a>(
    row: &'a BrowserRow,
    columns: &'a [BrowserColumn],
    effective_widths: &'a [usize],
) -> Row<'a> {
    Row::new(
        columns
            .iter()
            .zip(effective_widths.iter().copied())
            .map(|(column, effective_width)| browser_cell(row, column, effective_width))
            .collect::<Vec<_>>(),
    )
}

fn browser_cell<'a>(
    row: &'a BrowserRow,
    column: &BrowserColumn,
    effective_width: usize,
) -> Cell<'a> {
    let text = browser_cell_text(row, column, effective_width);
    let cell = Cell::from(text);
    match column.cell {
        BrowserCell::Status | BrowserCell::OriginStatus => cell.style(status_style(&row.status)),
        BrowserCell::RunStatus => cell.style(run_status_style(&row.run_status)),
        _ => cell,
    }
}

fn browser_cell_text(row: &BrowserRow, column: &BrowserColumn, effective_width: usize) -> String {
    match column.cell {
        BrowserCell::Status | BrowserCell::OriginStatus => row.status.clone(),
        BrowserCell::RunStatus => row.run_status.clone(),
        BrowserCell::OriginLabel => row.origin_label.clone(),
        BrowserCell::Title => row.title.clone(),
        BrowserCell::Key => row.key.clone(),
        BrowserCell::NextAction => row.next_action.clone(),
        BrowserCell::Duration => row.duration.clone().unwrap_or_else(|| "-".into()),
        BrowserCell::Size => row.size.clone().unwrap_or_else(|| "-".into()),
        BrowserCell::Branch => row.branch.clone().unwrap_or_else(|| "not prepared".into()),
        BrowserCell::Source => row.source.clone(),
        BrowserCell::Task => browser_task_cell_text(row, effective_width),
    }
}

fn browser_column_width_hint(column: &BrowserColumn) -> usize {
    match column.width {
        BrowserColumnWidth::Length(width) | BrowserColumnWidth::Min(width) => width as usize,
    }
}

fn browser_task_cell_text(row: &BrowserRow, width: usize) -> String {
    let key = format!("task {}", row.key);
    let key_width = measure_text_width(&key);
    let separator = "  ";
    let separator_width = measure_text_width(separator);
    if width <= key_width + separator_width {
        return key;
    }

    let title_width = width - key_width - separator_width;
    let title = truncate_display_width(&row.title, title_width);
    format!("{title}{separator}{key}")
}

fn truncate_display_width(value: &str, max_width: usize) -> String {
    if measure_text_width(value) <= max_width {
        return value.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut width = 0;
    let mut truncated = String::new();
    let target_width = max_width - 3;
    for ch in value.chars() {
        let ch_width = measure_text_width(&ch.to_string());
        if width + ch_width > target_width {
            break;
        }
        truncated.push(ch);
        width += ch_width;
    }
    truncated.push_str("...");
    truncated
}

fn row_style(app: &AppState, index: usize) -> Style {
    if app.selected_visible_index() == Some(index) {
        theme::selected_style()
    } else {
        Style::default()
    }
}

fn draw_detail_sidebar(frame: &mut Frame<'_>, area: Rect, app: &mut AppState) {
    let visible_count = area.height.saturating_sub(2) as usize;
    let content_width = area.width.saturating_sub(2).max(1) as usize;
    let mut lines = detail_sidebar_lines(app, content_width);
    if lines.is_empty() {
        lines.push(Line::styled(
            app.no_selection_message().to_string(),
            theme::dim_style(),
        ));
    }

    let max_start = body_viewport_max_start(lines.len(), visible_count);
    app.clamp_sidebar_scroll_to(max_start);
    let start = app.sidebar_scroll();
    let percent = body_scroll_percent(lines.len(), visible_count, start);
    let title = app
        .selected_row()
        .map(|row| format!("Detail {} {percent}%", row.title))
        .unwrap_or_else(|| format!("Detail {percent}%"));
    let rendered_lines = lines
        .into_iter()
        .skip(start)
        .take(visible_count)
        .collect::<Vec<_>>();
    let detail = Paragraph::new(rendered_lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::chrome_style())
            .title_style(theme::chrome_style()),
    );
    frame.render_widget(detail, area);
}

fn detail_sidebar_lines(app: &AppState, content_width: usize) -> Vec<Line<'static>> {
    let mut lines = app
        .diagnostics()
        .iter()
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    let Some(row) = app.selected_row() else {
        return lines;
    };

    if !lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines.extend(
        row.preview_lines
            .iter()
            .map(|line| Line::from(line.clone())),
    );

    let body_lines = app.wrapped_body_lines(content_width);
    if !body_lines.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        lines.extend(
            body_lines
                .iter()
                .map(|(kind, text)| body_wrapped_visual_line(kind, text)),
        );
    } else if !lines.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::styled("no body", theme::dim_style()));
    }
    lines
}

fn draw_output_panel(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let visible_count = area.height.saturating_sub(2) as usize;
    let lines = app.output_lines();
    let start = output_viewport_start(lines.len(), visible_count, app.output_scroll());
    let rendered_lines = lines
        .iter()
        .skip(start)
        .take(visible_count)
        .map(|(kind, line)| output_line(*kind, line))
        .collect::<Vec<_>>();
    let output = Paragraph::new(rendered_lines).block(
        Block::default()
            .title("Output")
            .borders(Borders::ALL)
            .border_style(theme::chrome_style())
            .title_style(theme::chrome_style()),
    );
    frame.render_widget(output, area);
}

fn draw_body_view(frame: &mut Frame<'_>, area: Rect, app: &mut AppState) {
    let visible_count = area.height.saturating_sub(2) as usize;
    let content_width = area.width.saturating_sub(2).max(1) as usize;
    let body_lines = app.wrapped_body_lines(content_width);
    let mut visual_lines = body_lines
        .iter()
        .map(|(kind, text)| body_wrapped_visual_line(kind, text))
        .collect::<Vec<_>>();
    if visual_lines.is_empty() {
        visual_lines.push(Line::styled("no body", theme::dim_style()));
    }

    let max_start = body_viewport_max_start(visual_lines.len(), visible_count);
    app.clamp_body_scroll_to(max_start);
    let start = app.body_scroll();
    let percent = body_scroll_percent(visual_lines.len(), visible_count, start);
    let title = app
        .selected_row()
        .map(|row| format!("Body {} {percent}%", row.title))
        .unwrap_or_else(|| format!("Body {percent}%"));
    let rendered_lines = visual_lines
        .into_iter()
        .skip(start)
        .take(visible_count)
        .collect::<Vec<_>>();
    let body = Paragraph::new(rendered_lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::chrome_style())
            .title_style(theme::chrome_style()),
    );
    frame.render_widget(body, area);
}

fn body_viewport_max_start(line_count: usize, visible_count: usize) -> usize {
    if visible_count == 0 || line_count <= visible_count {
        return 0;
    }
    line_count - visible_count
}

fn body_scroll_percent(line_count: usize, visible_count: usize, start: usize) -> usize {
    if line_count == 0 {
        return 0;
    }
    let visible_end = start.saturating_add(visible_count).min(line_count);
    visible_end.saturating_mul(100) / line_count
}

fn body_wrapped_visual_line(kind: &LineKind, text: &str) -> Line<'static> {
    Line::styled(text.to_string(), body_line_style(kind))
}

fn body_line_style(kind: &LineKind) -> Style {
    match kind {
        LineKind::Heading => Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        LineKind::Code => theme::dim_style(),
        LineKind::Checkbox(_) | LineKind::Plain => Style::default(),
    }
}

fn draw_status(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let base_status = app
        .running_status_line()
        .unwrap_or_else(|| app.status_line().to_string());
    let mut line = if app.mode() == Mode::FilterInput {
        format!("filter: {}  Esc clear", app.filter())
    } else if !app.diagnostics().is_empty() {
        format!("{}  {}", app.diagnostics().join("  "), base_status)
    } else {
        base_status
    };
    if let Some(frame_index) = app.spinner_frame() {
        line = format!(
            "{} {line}",
            SPINNER_FRAMES[frame_index % SPINNER_FRAMES.len()]
        );
    }
    frame.render_widget(Paragraph::new(Line::styled(line, theme::dim_style())), area);
}

fn draw_help_popup(frame: &mut Frame<'_>, area: Rect, app: &AppState) {
    let keymap = app.help_keymap();
    let desired_height = keymap.len().div_ceil(2) as u16 + 2;
    let area = popup_rect(area, desired_height);
    frame.render_widget(Clear, area);
    let lines = keymap
        .chunks(2)
        .map(|chunk| {
            let mut spans = Vec::new();
            for (index, (key, desc)) in chunk.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::styled("  |  ", theme::dim_style()));
                }
                spans.push(Span::styled(format!("{key:<10}"), theme::chrome_style()));
                spans.push(Span::raw(desc.to_string()));
            }
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    let popup = Paragraph::new(lines).block(popup_block("Help"));
    frame.render_widget(popup, area);
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

fn popup_rect(area: Rect, desired_height: u16) -> Rect {
    let max_height = area.height.saturating_mul(60).saturating_div(100).max(3);
    let height = desired_height.min(max_height).min(area.height.max(1));
    let [area] = Layout::vertical([Constraint::Length(height)])
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

fn draw_popup(frame: &mut Frame<'_>, area: Rect, popup: PopupView<'_>) {
    let area = popup_rect(area, popup_desired_height(popup));
    frame.render_widget(Clear, area);
    match popup {
        PopupView::Confirm { prompt, selected } => {
            draw_confirm_popup(frame, area, prompt, selected)
        }
        PopupView::Select {
            prompt,
            items,
            multi,
            cursor,
            selected,
        } => draw_select_popup(frame, area, prompt, items, multi, cursor, selected),
        PopupView::Input { prompt, buffer } => draw_input_popup(frame, area, prompt, buffer),
    }
}

fn popup_desired_height(popup: PopupView<'_>) -> u16 {
    match popup {
        PopupView::Confirm { .. } | PopupView::Input { .. } => 5,
        PopupView::Select { items, .. } => (items.len() as u16).saturating_add(3).max(5),
    }
}

fn draw_confirm_popup(frame: &mut Frame<'_>, area: Rect, prompt: &str, selected: bool) {
    let choice = if selected { "(Y/n)" } else { "(y/N)" };
    let popup = Paragraph::new(vec![
        Line::from(choice).alignment(Alignment::Center),
        Line::styled("y/n choose  Enter confirm  Esc cancel", theme::dim_style())
            .alignment(Alignment::Center),
    ])
    .block(popup_block(prompt));
    frame.render_widget(popup, area);
}

fn draw_select_popup(
    frame: &mut Frame<'_>,
    area: Rect,
    prompt: &str,
    items: &[String],
    multi: bool,
    cursor: usize,
    selected: &[bool],
) {
    let item_capacity = area.height.saturating_sub(3) as usize;
    let offset = selected_viewport_offset(cursor, item_capacity, items.len());
    let mut lines = items
        .iter()
        .enumerate()
        .skip(offset)
        .take(item_capacity)
        .map(|(index, item)| select_item_line(index, item, multi, cursor, selected))
        .collect::<Vec<_>>();
    if items.is_empty() {
        lines.push(Line::styled("No choices", theme::dim_style()).alignment(Alignment::Center));
    }
    lines.push(
        Line::styled(select_popup_hint(multi), theme::dim_style()).alignment(Alignment::Center),
    );
    let popup = Paragraph::new(lines).block(popup_block(prompt));
    frame.render_widget(popup, area);
}

fn draw_input_popup(frame: &mut Frame<'_>, area: Rect, prompt: &str, buffer: &str) {
    let input = if buffer.is_empty() {
        "_".to_string()
    } else {
        format!("{buffer}_")
    };
    let popup = Paragraph::new(vec![
        Line::from(input),
        Line::styled("Enter submit  Esc cancel", theme::dim_style()).alignment(Alignment::Center),
    ])
    .block(popup_block(prompt));
    frame.render_widget(popup, area);
}

fn popup_block<'a>(title: &'a str) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::chrome_style())
        .title_style(theme::chrome_style())
}

fn chrome_block<'a>() -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(theme::chrome_style())
        .title_style(theme::chrome_style())
}

fn output_panel_height(area: Rect) -> u16 {
    let available = area.height.saturating_sub(4).max(1);
    area.height
        .saturating_mul(30)
        .saturating_div(100)
        .clamp(3, 8)
        .min(available)
}

fn output_viewport_start(line_count: usize, visible_count: usize, scroll: usize) -> usize {
    if visible_count == 0 || line_count <= visible_count {
        return 0;
    }
    let max_start = line_count - visible_count;
    max_start.saturating_sub(scroll.min(max_start))
}

fn output_line(kind: PrintKind, text: &str) -> Line<'_> {
    Line::styled(text, output_style(kind))
}

fn output_style(kind: PrintKind) -> Style {
    match kind {
        PrintKind::Step => theme::chrome_style(),
        PrintKind::Plain => Style::default(),
        PrintKind::Dim => theme::dim_style(),
        PrintKind::Warning => status_style("stale"),
        PrintKind::Error => status_style("error"),
    }
}

fn selected_viewport_offset(cursor: usize, visible_count: usize, item_count: usize) -> usize {
    if visible_count == 0 || item_count <= visible_count {
        return 0;
    }
    let max_start = item_count - visible_count;
    cursor
        .saturating_sub(visible_count.saturating_sub(1))
        .min(max_start)
}

fn select_item_line<'a>(
    index: usize,
    item: &'a str,
    multi: bool,
    cursor: usize,
    selected: &[bool],
) -> Line<'a> {
    let marker = if index == cursor { ">" } else { " " };
    let text = if multi {
        let check = if selected.get(index).copied().unwrap_or(false) {
            "x"
        } else {
            " "
        };
        format!("{marker} [{check}] {item}")
    } else {
        format!("{marker} {item}")
    };
    if index == cursor {
        Line::styled(text, theme::selected_style())
    } else {
        Line::from(text)
    }
}

fn select_popup_hint(multi: bool) -> &'static str {
    if multi {
        "Space toggle  Enter select  Esc cancel"
    } else {
        "Enter select  Esc cancel"
    }
}

fn status_style(status: &str) -> Style {
    theme::status_color(status)
        .map(|color| Style::default().fg(color))
        .unwrap_or_default()
}

fn run_status_style(status: &str) -> Style {
    theme::run_status_color(status)
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
    use crate::tui::app::{
        AppState, BrowserCell, BrowserColumn, BrowserRow, KeyInput, PopupSpec, SourceView,
    };
    use crate::tui::remote_ui::PrintKind;
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
        let mut app = app.clone();
        render_buffer_mut(width, height, &mut app)
    }

    fn render_buffer_mut(width: u16, height: u16, app: &mut AppState) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_text(width: u16, height: u16, app: &AppState) -> String {
        let buffer = render_buffer(width, height, app);
        buffer_to_text(width, height, &buffer)
    }

    fn buffer_text_mut(width: u16, height: u16, app: &mut AppState) -> String {
        let buffer = render_buffer_mut(width, height, app);
        buffer_to_text(width, height, &buffer)
    }

    fn buffer_to_text(width: u16, height: u16, buffer: &Buffer) -> String {
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
            run_status: "new".into(),
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
            duration: None,
            size: None,
            branch: Some(format!("feature/{key}")),
            source: "provider-origin".into(),
            body: String::new(),
            preview_lines: vec!["Origin      Linear WT-142".into()],
            menu: OriginActionMenu::for_origin_task(
                key,
                title,
                OriginLabel::new("linear", "WT-142"),
            ),
        }
    }

    fn source_row(key: &str, source: &str) -> BrowserRow {
        let mut browser_row = row(key, key, "stale");
        browser_row.source = source.into();
        browser_row
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

    fn has_terminal_control_cell(buffer: &Buffer, width: u16, height: u16) -> bool {
        (0..height)
            .any(|y| (0..width).any(|x| buffer[(x, y)].symbol().chars().any(is_terminal_control)))
    }

    fn is_terminal_control(ch: char) -> bool {
        ch == '\x1b' || ('\u{0080}'..='\u{009f}').contains(&ch) || ch.is_control()
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
    fn invariant_i6_browser_task_column_preserves_key_when_title_is_truncated() {
        let app = AppState::task_with_columns(
            vec![row(
                "stable-key",
                "This imported issue title is much longer than the task column",
                "fresh",
            )],
            Vec::new(),
            vec![BrowserColumn::length("task", BrowserCell::Task, 24)],
        );

        let text = buffer_text(40, 12, &app);

        assert!(
            text.contains("task stable-key"),
            "browser task column should keep the task key visible in `{text}`"
        );
    }

    #[test]
    fn popup_renders_over_list_with_prompt() {
        let mut app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        app.open_popup(PopupSpec::Confirm {
            prompt: "Push to provider?".into(),
            default: false,
        });
        let text = buffer_text(80, 24, &app);
        assert!(text.contains("Push to provider?"));
        assert!(text.contains("y/N"));
    }

    #[test]
    fn output_panel_hidden_when_empty_and_shown_when_filled() {
        let mut app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        let before = buffer_text(80, 24, &app);
        assert!(!before.contains("Pull preview"));
        app.push_output(PrintKind::Plain, "Pull preview - WT-142".into());
        let after = buffer_text(80, 24, &app);
        assert!(after.contains("Pull preview - WT-142"));
    }

    #[test]
    fn running_action_paints_spinner_in_status_line() {
        let mut app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        app.begin_action("origin-sync-tui", "pull");
        let text = buffer_text(80, 24, &app);
        assert!(text.contains("pulling origin-sync-tui"));
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
    fn workflow_browser_footer_omits_archive_shortcut() {
        let app = AppState::workflow_with_diagnostics(
            vec![row("2026-06-06-001", "Ship provider-origin UX", "stale")],
            Vec::new(),
        );

        let text = buffer_text(100, 24, &app);

        assert!(!text.contains("a archive"));
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
    fn header_renders_source_view_tabs_with_separator_and_active() {
        let mut app = AppState::new(vec![
            source_row("a", "local"),
            source_row("b", "provider-origin"),
        ]);
        app.set_source_view_for_test(SourceView::Published);

        let text = buffer_text(120, 24, &app);

        assert!(text.contains("local (1)"));
        assert!(text.contains("[published] (1)"));
        assert!(text.contains("|"));
    }

    #[test]
    fn narrow_header_keeps_active_source_view_visible() {
        let mut app = AppState::new(vec![
            source_row("a", "local"),
            source_row("b", "provider-origin"),
        ]);
        app.set_source_view_for_test(SourceView::Published);

        let text = buffer_text(32, 24, &app);

        assert!(text.contains("published"));
    }

    #[test]
    fn workflow_header_omits_source_view_tabs() {
        let app = AppState::workflow_with_diagnostics(vec![source_row("wf", "local")], Vec::new());

        let text = buffer_text(120, 24, &app);

        assert!(!text.contains("all ("));
        assert!(!text.contains("published"));
    }

    #[test]
    fn header_shows_search_bar_when_filtering() {
        let mut app = AppState::new(vec![source_row("a", "local")]);
        app.handle(KeyInput::Char('/'));
        app.handle(KeyInput::Char('x'));

        let text = buffer_text(120, 24, &app);

        assert!(text.contains("/ x"));
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
    fn wide_terminal_shows_sidebar_on_the_right_with_body_and_meta() {
        let mut browser_row = source_row("a", "provider-origin");
        browser_row.title = "task a".into();
        browser_row.body = "## Detail Body\nbody-marker".into();
        browser_row.preview_lines = vec!["META-MARKER".into()];
        let app = AppState::new(vec![browser_row]);

        let text = buffer_text(140, 24, &app);

        assert!(text.contains("task a"));
        assert!(text.contains("META-MARKER"));
        assert!(text.contains("body-marker"));
    }

    #[test]
    fn narrow_terminal_reflows_sidebar_to_bottom() {
        let mut browser_row = source_row("a", "provider-origin");
        browser_row.title = "task a".into();
        browser_row.body = "body-marker".into();
        browser_row.preview_lines = vec!["META-MARKER".into()];
        let app = AppState::new(vec![browser_row]);

        let text = buffer_text(70, 24, &app);

        assert!(text.contains("task a"));
        assert!(text.contains("body-marker"));
    }

    #[test]
    fn detail_sidebar_reuses_wrapped_body_until_width_or_selection_changes() {
        let mut browser_row = source_row("a", "provider-origin");
        browser_row.title = "task a".into();
        browser_row.body = (0..80)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut second_row = source_row("b", "provider-origin");
        second_row.title = "task b".into();
        second_row.body = "second body marker".into();
        let mut app = AppState::new(vec![browser_row, second_row]);

        assert_eq!(app.body_wrap_recompute_count(), 0);
        let _ = buffer_text_mut(160, 24, &mut app);
        assert_eq!(app.body_wrap_recompute_count(), 1);
        let _ = buffer_text_mut(160, 24, &mut app);
        assert_eq!(app.body_wrap_recompute_count(), 1);

        app.handle(KeyInput::PageDown);
        let _ = buffer_text_mut(160, 24, &mut app);
        assert_eq!(app.body_wrap_recompute_count(), 1);

        let _ = buffer_text_mut(170, 24, &mut app);
        assert_eq!(app.body_wrap_recompute_count(), 2);

        app.handle(KeyInput::Char('j'));
        let _ = buffer_text_mut(170, 24, &mut app);
        assert_eq!(app.body_wrap_recompute_count(), 3);
    }

    #[test]
    fn tiny_terminal_hides_sidebar_no_crash() {
        let mut browser_row = source_row("a", "provider-origin");
        browser_row.title = "task a".into();
        browser_row.body = "body-marker".into();
        browser_row.preview_lines = vec!["META-MARKER".into()];
        let app = AppState::new(vec![browser_row]);

        let text = buffer_text(40, 8, &app);

        assert!(text.contains("task a"));
        assert!(!text.contains("body-marker"));
    }

    #[test]
    fn body_view_renders_markup_body_instead_of_preview() {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.body =
            "## 계획 (Planning)\n- [ ] Step 1\n```rust\nlet x = 1;\n```\nplain".into();
        browser_row.preview_lines = vec!["PREVIEW-MARKER".into()];
        let mut app = AppState::new(vec![browser_row]);
        app.handle(KeyInput::Char('v'));

        let text = buffer_text(80, 24, &app);

        assert!(text.contains("Body Origin sync TUI"));
        assert!(text.contains("(Planning)"));
        assert!(text.contains("☐ Step 1"));
        assert!(text.contains("let x = 1;"));
        assert!(!text.contains("PREVIEW-MARKER"));
    }

    #[test]
    fn body_view_empty_body_renders_no_body_message() {
        let mut app = AppState::new(vec![row("origin-sync-tui", "Origin sync TUI", "conflict")]);
        app.handle(KeyInput::Char('v'));

        let text = buffer_text(80, 24, &app);

        assert!(text.contains("no body"));
    }

    #[test]
    fn body_view_scroll_reaches_wrapped_tail_of_long_single_line() {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        let mut words = (0..40)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>();
        words.push("tail-marker".into());
        browser_row.body = words.join(" ");
        let mut app = AppState::new(vec![browser_row]);
        app.handle(KeyInput::Char('v'));

        let initial = buffer_text(32, 10, &app);
        assert!(!initial.contains("tail-marker"));

        for _ in 0..30 {
            app.handle(KeyInput::Char('j'));
            let text = buffer_text(32, 10, &app);
            if text.contains("tail-marker") {
                return;
            }
        }

        panic!("expected body view scroll to reach wrapped tail of a long source line");
    }

    #[test]
    fn body_view_strips_terminal_control_sequences_from_untrusted_body() {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.body =
            "plain\x1b[31mred\x1b[0m\nosc\x1b]0;title\x07done\nc1\u{009b}31mred".into();
        let mut app = AppState::new(vec![browser_row]);
        app.handle(KeyInput::Char('v'));

        let buffer = render_buffer(80, 24, &app);
        let text = buffer_text(80, 24, &app);

        assert!(text.contains("plainred"));
        assert!(text.contains("oscdone"));
        assert!(text.contains("c1red"));
        assert!(
            !has_terminal_control_cell(&buffer, 80, 24),
            "body view should not render terminal control characters from imported body text"
        );
    }

    #[test]
    fn body_view_normalizes_tabs_from_untrusted_body() {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.body = "alpha\tbeta\n- [ ] gamma\tdelta".into();
        let mut app = AppState::new(vec![browser_row]);
        app.handle(KeyInput::Char('v'));

        let buffer = render_buffer(80, 24, &app);
        let text = buffer_text(80, 24, &app);

        assert!(text.contains("alpha beta"));
        assert!(text.contains("☐ gamma delta"));
        assert!(
            !has_terminal_control_cell(&buffer, 80, 24),
            "body view should render imported tab characters as inert spacing"
        );
    }

    #[test]
    fn body_view_scroll_up_moves_immediately_after_bottom_overscroll() {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.body = (0..20)
            .map(|index| format!("line-{index:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut app = AppState::new(vec![browser_row]);
        app.handle(KeyInput::Char('v'));

        for _ in 0..30 {
            app.handle(KeyInput::Char('j'));
        }
        let bottom = buffer_text_mut(80, 10, &mut app);
        assert!(bottom.contains("line-19"));

        app.handle(KeyInput::Char('k'));
        let after_one_up = buffer_text_mut(80, 10, &mut app);

        assert!(
            !after_one_up.contains("line-19"),
            "first scroll-up after bottom overscroll should move the body view"
        );
    }

    #[test]
    fn body_view_remains_colorless_when_colors_disabled() {
        let _guard = ColorGuard::set(false);
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.body = "## Heading\n- [x] Done".into();
        let mut app = AppState::new(vec![browser_row]);
        app.handle(KeyInput::Char('v'));

        let buffer = render_buffer(80, 24, &app);

        assert!(
            !has_colored_cell(&buffer, 80, 24),
            "body view should not contain semantic or chrome colors when colors are disabled"
        );
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
    fn help_overlay_renders_full_keymap() {
        let mut app = AppState::new(vec![source_row("a", "local")]);
        app.handle(KeyInput::Char('?'));

        let text = buffer_text(100, 24, &app);

        assert!(text.contains("Help"));
        assert!(text.contains("s"));
        assert!(text.contains("sidebar"));
        assert!(text.contains("h/l"));
    }

    #[test]
    fn help_overlay_shows_every_key_on_24_line_terminal() {
        let mut app = AppState::new(vec![source_row("a", "local")]);
        let keys = app.help_keymap();
        app.handle(KeyInput::Char('?'));

        let text = buffer_text(100, 24, &app);

        for (key, _) in keys {
            assert!(text.contains(key), "missing help key {key} in:\n{text}");
        }
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
