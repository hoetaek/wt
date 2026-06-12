use crate::tui::theme;
use crate::ui::selector::strip_terminal_sequences;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SyntectColor, FontStyle, Style as SyntectStyle, Theme};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use unicode_width::UnicodeWidthChar;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static SYNTAX_THEME: OnceLock<Theme> = OnceLock::new();

pub(crate) fn render_reader_lines(body: &str, width: usize) -> Vec<Line<'static>> {
    let body = strip_terminal_sequences(body).replace('\t', " ");
    let blocks = markdown_blocks(&body);
    let mut lines = Vec::new();
    let mut previous = BlockKind::Blank;

    for block in blocks {
        let current = block.kind();
        if should_insert_rhythm(previous, current) && !line_vec_ends_blank(&lines) {
            lines.push(Line::default());
        }
        lines.extend(block.wrapped(width.max(1)));
        previous = current;
    }
    lines
}

#[derive(Debug, Clone)]
enum Block {
    Paragraph(Vec<StyledSpan>),
    Heading {
        level: u8,
        text: String,
    },
    ListItem {
        marker: String,
        checkbox: Option<bool>,
        spans: Vec<StyledSpan>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(Vec<StyledSpan>),
    Rule,
    TableRow {
        header: bool,
        cells: Vec<Vec<StyledSpan>>,
    },
}

impl Block {
    fn kind(&self) -> BlockKind {
        match self {
            Self::Heading { level: 1, .. } => BlockKind::Heading1,
            Self::Heading { .. } => BlockKind::Heading,
            Self::ListItem { .. } => BlockKind::List,
            Self::Code { .. } => BlockKind::Code,
            Self::Quote(_) => BlockKind::Quote,
            Self::Rule => BlockKind::Rule,
            Self::TableRow { .. } => BlockKind::Table,
            Self::Paragraph(_) => BlockKind::Paragraph,
        }
    }

    fn wrapped(self, width: usize) -> Vec<Line<'static>> {
        match self {
            Self::Paragraph(spans) => wrap_spans(spans, width, None),
            Self::Heading { level, text } => wrap_spans(
                vec![StyledSpan::new(
                    heading_display_text(level, &text),
                    heading_style(level),
                    None,
                )],
                width,
                None,
            ),
            Self::ListItem {
                marker,
                checkbox,
                spans,
            } => {
                let mut rendered = vec![StyledSpan::new(marker.clone(), list_marker_style(), None)];
                rendered.push(StyledSpan::plain(" "));
                if let Some(checked) = checkbox {
                    rendered.push(StyledSpan::new(
                        if checked { "☑" } else { "☐" },
                        list_marker_style(),
                        None,
                    ));
                    rendered.push(StyledSpan::plain(" "));
                }
                rendered.extend(spans);
                let continuation = display_width(&marker) + 1 + checkbox.map(|_| 2).unwrap_or(0);
                wrap_spans(rendered, width, Some(continuation))
            }
            Self::Code { language, text } => code_lines(&text, language.as_deref(), width),
            Self::Quote(spans) => {
                let mut rendered = vec![
                    StyledSpan::new("│", quote_style(), None),
                    StyledSpan::plain(" "),
                ];
                rendered.extend(spans);
                wrap_spans(rendered, width, Some(2))
            }
            Self::Rule => wrap_spans(
                vec![StyledSpan::new("─".repeat(24), theme::dim_style(), None)],
                width,
                None,
            ),
            Self::TableRow { header, cells } => {
                let style = if header {
                    table_header_style()
                } else {
                    Style::default()
                };
                let text = cells
                    .into_iter()
                    .map(|cell| styled_text(&cell))
                    .collect::<Vec<_>>()
                    .join(" | ");
                wrap_spans(
                    vec![StyledSpan::new(format!("| {text} |"), style, None)],
                    width,
                    None,
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Blank,
    Heading1,
    Heading,
    List,
    Code,
    Quote,
    Rule,
    Table,
    Paragraph,
}

#[derive(Debug, Clone)]
struct StyledSpan {
    text: String,
    style: Style,
    link: Option<String>,
}

impl StyledSpan {
    fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Style::default(), None)
    }

    fn new(text: impl Into<String>, style: Style, link: Option<String>) -> Self {
        Self {
            text: text.into(),
            style,
            link,
        }
    }
}

#[derive(Debug, Default)]
struct MarkdownState {
    blocks: Vec<Block>,
    inline: Vec<StyledSpan>,
    heading: Option<(u8, String)>,
    code: Option<(Option<String>, String)>,
    list_stack: Vec<Option<u64>>,
    item: Option<ItemState>,
    quote_depth: usize,
    strong: usize,
    emphasis: usize,
    strike: usize,
    link: Option<String>,
    table: Option<TableState>,
}

#[derive(Debug, Default)]
struct ItemState {
    marker: String,
    checkbox: Option<bool>,
    spans: Vec<StyledSpan>,
}

#[derive(Debug, Default)]
struct TableState {
    head: bool,
    current_row: Option<Vec<Vec<StyledSpan>>>,
    current_cell: Option<Vec<StyledSpan>>,
}

fn markdown_blocks(body: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let mut state = MarkdownState::default();
    for event in Parser::new_ext(body, options) {
        state.event(event);
    }
    state.finish();
    state.blocks
}

impl MarkdownState {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(text.as_ref()),
            Event::Code(code) => self.push_span(StyledSpan::new(
                code.to_string(),
                code_style(),
                self.link.clone(),
            )),
            Event::SoftBreak | Event::HardBreak => self.flush_inline(),
            Event::Rule => self.blocks.push(Block::Rule),
            Event::TaskListMarker(checked) => {
                if let Some(item) = &mut self.item {
                    item.checkbox = Some(checked);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(html.as_ref()),
            Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.heading = Some((heading_level_number(level), String::new()))
            }
            Tag::CodeBlock(kind) => {
                self.code = Some((language_from_code_block(kind), String::new()))
            }
            Tag::List(start) => self.list_stack.push(start),
            Tag::Item => self.start_item(),
            Tag::Strong => self.strong += 1,
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::BlockQuote(_) => self.quote_depth += 1,
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.to_string()),
            Tag::Paragraph => {}
            Tag::Table(_) => self.table = Some(TableState::default()),
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = true;
                    table.current_row = Some(Vec::new());
                }
            }
            Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.current_row = Some(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(table) = &mut self.table {
                    table.current_cell = Some(Vec::new());
                }
            }
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::Image { .. }
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                if let Some((level, text)) = self.heading.take() {
                    self.blocks.push(Block::Heading { level, text });
                }
            }
            TagEnd::CodeBlock => {
                if let Some((language, text)) = self.code.take() {
                    self.blocks.push(Block::Code { language, text });
                }
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                if let Some(item) = self.item.take() {
                    self.blocks.push(Block::ListItem {
                        marker: item.marker,
                        checkbox: item.checkbox,
                        spans: item.spans,
                    });
                }
            }
            TagEnd::Paragraph => self.flush_inline(),
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::BlockQuote(_) => self.quote_depth = self.quote_depth.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Table => self.table = None,
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(table) = &mut self.table
                    && let Some(row) = table.current_row.take()
                {
                    self.blocks.push(Block::TableRow {
                        header: table.head,
                        cells: row,
                    });
                    table.head = false;
                }
            }
            TagEnd::TableCell => {
                if let Some(table) = &mut self.table
                    && let Some(cell) = table.current_cell.take()
                    && let Some(row) = &mut table.current_row
                {
                    row.push(cell);
                }
            }
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Image
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
        }
    }

    fn start_item(&mut self) {
        let depth = self.list_stack.len().saturating_sub(1);
        let marker = match self.list_stack.last_mut().and_then(Option::as_mut) {
            Some(next) => {
                let marker = format!("{}{next}.", "  ".repeat(depth));
                *next += 1;
                marker
            }
            None => format!("{}•", "  ".repeat(depth)),
        };
        self.item = Some(ItemState {
            marker,
            ..ItemState::default()
        });
    }

    fn text(&mut self, text: &str) {
        if let Some((_, code)) = &mut self.code {
            code.push_str(text);
            return;
        }
        if let Some((_, heading)) = &mut self.heading {
            heading.push_str(text);
            return;
        }
        self.push_span(StyledSpan::new(
            text.to_string(),
            self.current_style(),
            self.link.clone(),
        ));
    }

    fn push_span(&mut self, span: StyledSpan) {
        if let Some(table) = &mut self.table
            && let Some(cell) = &mut table.current_cell
        {
            cell.push(span);
            return;
        }
        if let Some(item) = &mut self.item {
            item.spans.push(span);
            return;
        }
        self.inline.push(span);
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.inline);
        if self.quote_depth > 0 {
            self.blocks.push(Block::Quote(spans));
        } else {
            self.blocks.push(Block::Paragraph(spans));
        }
    }

    fn finish(&mut self) {
        self.flush_inline();
    }

    fn current_style(&self) -> Style {
        let mut style = Style::default();
        if self.strong > 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.emphasis > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.link.is_some() {
            style = style.patch(link_style());
        }
        style
    }
}

fn language_from_code_block(kind: CodeBlockKind<'_>) -> Option<String> {
    match kind {
        CodeBlockKind::Fenced(info) => info
            .split([',', ' ', '\t'])
            .next()
            .filter(|language| !language.is_empty())
            .map(str::to_string),
        CodeBlockKind::Indented => None,
    }
}

fn code_lines(text: &str, language: Option<&str>, width: usize) -> Vec<Line<'static>> {
    if let Some(language) = language
        && let Some(lines) = highlight_code(text, language)
    {
        return lines
            .into_iter()
            .flat_map(|line| wrap_spans(line, width, None))
            .collect();
    }

    let mut lines = text
        .lines()
        .flat_map(|line| wrap_spans(vec![StyledSpan::new(line, code_style(), None)], width, None))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(Line::styled(String::new(), code_style()));
    }
    lines
}

fn highlight_code(code: &str, language: &str) -> Option<Vec<Vec<StyledSpan>>> {
    if code.is_empty() || code.len() > 512 * 1024 || code.lines().count() > 10_000 {
        return None;
    }
    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_by_token(language)
        .or_else(|| syntax_set.find_syntax_by_extension(language))?;
    let mut highlighter = HighlightLines::new(syntax, syntax_theme());
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, syntax_set).ok()?;
        let spans = ranges
            .into_iter()
            .filter_map(|(style, text)| {
                let text = text.trim_end_matches(['\n', '\r']);
                (!text.is_empty()).then(|| StyledSpan::new(text, syntect_style(style), None))
            })
            .collect::<Vec<_>>();
        lines.push(if spans.is_empty() {
            vec![StyledSpan::new(String::new(), code_style(), None)]
        } else {
            spans
        });
    }
    Some(lines)
}

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn syntax_theme() -> &'static Theme {
    SYNTAX_THEME.get_or_init(|| {
        two_face::theme::extra()
            .get(two_face::theme::EmbeddedThemeName::InspiredGithub)
            .clone()
    })
}

fn wrap_spans(
    spans: Vec<StyledSpan>,
    width: usize,
    continuation_width: Option<usize>,
) -> Vec<Line<'static>> {
    let units = span_units(&spans);
    if units.is_empty() {
        return vec![Line::default()];
    }

    let mut lines = Vec::new();
    let mut start = 0;
    let mut continuation = false;
    while start < units.len() {
        let prefix_width = if continuation {
            continuation_width.unwrap_or(0)
        } else {
            0
        };
        let available = width.saturating_sub(prefix_width).max(1);
        let mut end = start;
        let mut current_width = 0;
        let mut last_space_after = None;
        while end < units.len() {
            let unit = &units[end];
            if current_width > 0 && current_width + unit.width > available {
                break;
            }
            if current_width == 0 && unit.width > available {
                end += 1;
                break;
            }
            current_width += unit.width;
            end += 1;
            if unit.ch.is_whitespace() {
                last_space_after = Some(end);
            }
        }
        let mut line_end = end;
        let mut next_start = end;
        if end < units.len()
            && let Some(space_after) = last_space_after
            && space_after > start
        {
            line_end = trim_trailing_space(&units, space_after);
            next_start = skip_space(&units, space_after);
        }
        if line_end <= start {
            line_end = end.max(start + 1).min(units.len());
            next_start = line_end;
        }
        lines.push(line_from_units(
            continuation.then_some(continuation_width.unwrap_or(0)),
            &units[start..line_end],
        ));
        start = next_start;
        continuation = true;
    }
    lines
}

#[derive(Debug, Clone)]
struct Unit {
    ch: char,
    width: usize,
    style: Style,
    link: Option<String>,
}

fn span_units(spans: &[StyledSpan]) -> Vec<Unit> {
    spans
        .iter()
        .flat_map(|span| {
            span.text.chars().map(|ch| Unit {
                ch,
                width: char_width(ch),
                style: span.style,
                link: span.link.clone(),
            })
        })
        .collect()
}

fn line_from_units(prefix_width: Option<usize>, units: &[Unit]) -> Line<'static> {
    let mut spans = Vec::new();
    if let Some(prefix_width) = prefix_width.filter(|width| *width > 0) {
        spans.push(Span::raw(" ".repeat(prefix_width)));
    }
    let mut current = String::new();
    let mut current_style = None;
    let mut current_link = None::<String>;
    for unit in units {
        if current_style != Some(unit.style) || current_link != unit.link {
            push_rendered_span(&mut spans, &mut current, current_style, current_link.take());
            current_style = Some(unit.style);
            current_link = unit.link.clone();
        }
        current.push(unit.ch);
    }
    push_rendered_span(&mut spans, &mut current, current_style, current_link);
    Line::from(spans)
}

fn push_rendered_span(
    spans: &mut Vec<Span<'static>>,
    text: &mut String,
    style: Option<Style>,
    link: Option<String>,
) {
    if text.is_empty() {
        return;
    }
    let content = if let Some(destination) = link.and_then(|link| safe_hyperlink_destination(&link))
    {
        format!("\x1b]8;;{destination}\x07{text}\x1b]8;;\x07")
    } else {
        text.clone()
    };
    spans.push(Span::styled(content, style.unwrap_or_default()));
    text.clear();
}

fn safe_hyperlink_destination(destination: &str) -> Option<String> {
    let destination = strip_terminal_sequences(destination).replace(['\x1b', '\x07'], "");
    (!destination.trim().is_empty()
        && (destination.starts_with("http://") || destination.starts_with("https://")))
    .then_some(destination)
}

fn skip_space(units: &[Unit], mut index: usize) -> usize {
    while index < units.len() && units[index].ch.is_whitespace() {
        index += 1;
    }
    index
}

fn trim_trailing_space(units: &[Unit], mut end: usize) -> usize {
    while end > 0 && units[end - 1].ch.is_whitespace() {
        end -= 1;
    }
    end
}

fn styled_text(spans: &[StyledSpan]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

fn line_vec_ends_blank(lines: &[Line<'_>]) -> bool {
    lines
        .last()
        .is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty()))
}

fn should_insert_rhythm(previous: BlockKind, current: BlockKind) -> bool {
    if previous == BlockKind::Blank || current == BlockKind::Blank {
        return false;
    }
    matches!(
        (previous, current),
        (
            _,
            BlockKind::Heading1 | BlockKind::Heading | BlockKind::Code | BlockKind::Table
        ) | (
            BlockKind::Heading1 | BlockKind::Heading | BlockKind::Code | BlockKind::Table,
            _
        ) | (_, BlockKind::Quote)
            | (BlockKind::Quote, _)
    )
}

fn heading_level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn heading_display_text(level: u8, text: &str) -> String {
    if level <= 2 {
        text.to_string()
    } else {
        format!("{} {text}", "#".repeat(level as usize))
    }
}

fn heading_style(level: u8) -> Style {
    let mut style = Style::default().add_modifier(Modifier::BOLD);
    if level <= 2 {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if theme::colors_active() {
        style.fg(Color::Indexed(match level {
            1 => 81,
            2 => 110,
            _ => 153,
        }))
    } else {
        style
    }
}

fn code_style() -> Style {
    if theme::colors_active() {
        Style::default().fg(Color::Indexed(244))
    } else {
        Style::default()
    }
}

fn quote_style() -> Style {
    if theme::colors_active() {
        Style::default().fg(Color::Indexed(108))
    } else {
        Style::default()
    }
}

fn list_marker_style() -> Style {
    theme::dim_style().add_modifier(Modifier::BOLD)
}

fn link_style() -> Style {
    let style = Style::default().add_modifier(Modifier::UNDERLINED);
    if theme::colors_active() {
        style.fg(Color::Blue)
    } else {
        style
    }
}

fn table_header_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn syntect_style(style: SyntectStyle) -> Style {
    let mut rendered = Style::default();
    if theme::colors_active() {
        rendered = rendered.fg(syntect_color(style.foreground));
    }
    if style.font_style.contains(FontStyle::BOLD) {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        rendered = rendered.add_modifier(Modifier::UNDERLINED);
    }
    rendered
}

fn syntect_color(color: SyntectColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn renders_heading_code_checkbox_quote_with_styles() {
        let body = "## Title\n\n- [x] done\n- [ ] todo\n\n```rust\nfn main() {}\n```\n\n> quoted\n";
        let lines = render_reader_lines(body, 40);

        assert!(lines.iter().any(|line| text(line).contains("Title")));
        assert!(
            lines
                .iter()
                .any(|line| text(line).contains("[x]") || text(line).contains("done"))
        );
        assert!(lines.iter().any(|line| text(line).contains("fn main")));
        assert!(
            lines
                .iter()
                .any(|line| text(line).starts_with("│") || text(line).contains("quoted"))
        );
    }

    #[test]
    fn wraps_to_width_with_unicode_and_strips_control_sequences() {
        let body = "가나다라마바사아자차카타파하 ".repeat(8);
        let lines = render_reader_lines(&body, 20);
        assert!(lines.len() > 1);

        let hostile = "plain \u{1b}]0;evil\u{7} text";
        let lines = render_reader_lines(hostile, 80);
        assert!(!lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains('\u{1b}'))
        }));
    }
}
