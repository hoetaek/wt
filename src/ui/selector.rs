use console::{Style, measure_text_width};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    terminal::{self, ClearType},
};
use std::cmp;
use std::io::{self, Write};

pub(crate) const DEFAULT_VISIBLE_ROWS: usize = 10;

const PROMPT_START: &str = "◆";
const PROMPT_SUBMIT: &str = "◇";
const PROMPT_CANCEL: &str = "■";
const BAR: &str = "│";
const FOOTER: &str = "└";
const CURSOR_ACTIVE: &str = "❯";
const RADIO_SELECTED: &str = "●";
const RADIO_UNSELECTED: &str = "○";
const HINT_GAP: usize = 2;
const DEFAULT_SUMMARY_LABELS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectorRow {
    Section(SelectorSection),
    Option(SelectorOption),
}

impl SelectorRow {
    #[cfg(test)]
    pub(crate) fn section(title: impl Into<String>) -> Self {
        Self::Section(SelectorSection::new(title))
    }

    #[cfg(test)]
    pub(crate) fn option(index: usize, label: impl Into<String>) -> Self {
        Self::Option(SelectorOption::new(index, label))
    }

    #[cfg(test)]
    pub(crate) fn option_with_hint(
        index: usize,
        label: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::Option(SelectorOption::with_hint(index, label, hint))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectorSection {
    pub(crate) title: String,
    pub(crate) hint: Option<String>,
}

impl SelectorSection {
    pub(crate) fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            hint: None,
        }
    }

    pub(crate) fn with_hint(title: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            hint: non_empty(hint.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectorOption {
    pub(crate) index: usize,
    pub(crate) label: String,
    pub(crate) hint: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) search_text: Vec<String>,
    pub(crate) selected: bool,
    pub(crate) disabled: bool,
}

impl SelectorOption {
    pub(crate) fn new(index: usize, label: impl Into<String>) -> Self {
        Self {
            index,
            label: label.into(),
            hint: None,
            description: None,
            search_text: Vec::new(),
            selected: false,
            disabled: false,
        }
    }

    pub(crate) fn with_hint(
        index: usize,
        label: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        let mut option = Self::new(index, label);
        option.hint = non_empty(hint.into());
        option
    }

    pub(crate) fn search_text(mut self, text: impl Into<String>) -> Self {
        if let Some(text) = non_empty(text.into()) {
            self.search_text.push(text);
        }
        self
    }

    pub(crate) fn description(mut self, description: impl Into<String>) -> Self {
        self.description = non_empty(description.into());
        self
    }

    pub(crate) fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn matches_query(&self, query: &str) -> bool {
        let terms = normalized_terms(query);
        if terms.is_empty() {
            return true;
        }

        let mut haystack = sanitize_selector_text(&self.label).to_lowercase();
        if let Some(hint) = self.hint.as_deref() {
            haystack.push(' ');
            haystack.push_str(&sanitize_selector_text(hint).to_lowercase());
        }
        if let Some(description) = self.description.as_deref() {
            haystack.push(' ');
            haystack.push_str(&sanitize_selector_text(description).to_lowercase());
        }
        for text in &self.search_text {
            haystack.push(' ');
            haystack.push_str(&sanitize_selector_text(text).to_lowercase());
        }

        terms.iter().all(|term| haystack.contains(term))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectorMode {
    Single,
    Multi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectorInput {
    Up,
    Down,
    Space,
    Enter,
    Cancel,
    Backspace,
    Char(char),
}

impl SelectorInput {
    pub(crate) fn from_key_event(event: KeyEvent) -> Option<Self> {
        match event.code {
            KeyCode::Up => Some(Self::Up),
            KeyCode::Down => Some(Self::Down),
            KeyCode::Enter => Some(Self::Enter),
            KeyCode::Esc => Some(Self::Cancel),
            KeyCode::Backspace => Some(Self::Backspace),
            KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Self::Cancel)
            }
            KeyCode::Char(' ') => Some(Self::Space),
            KeyCode::Char(ch)
                if !event.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                Some(Self::Char(ch))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectorTransition {
    Continue,
    Submitted(SelectorSubmission),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectorSubmission {
    Single(usize),
    Multi(Vec<usize>),
}

#[derive(Debug, Clone)]
pub(crate) struct SelectorState {
    mode: SelectorMode,
    rows: Vec<SelectorRow>,
    query: String,
    active_row: Option<usize>,
    visible_option_offset: usize,
    max_visible_rows: usize,
}

impl SelectorState {
    pub(crate) fn single(rows: Vec<SelectorRow>) -> Self {
        Self::new(SelectorMode::Single, rows)
    }

    pub(crate) fn multi(rows: Vec<SelectorRow>) -> Self {
        Self::new(SelectorMode::Multi, rows)
    }

    pub(crate) fn new(mode: SelectorMode, rows: Vec<SelectorRow>) -> Self {
        Self::with_max_visible_rows(mode, rows, DEFAULT_VISIBLE_ROWS)
    }

    pub(crate) fn with_max_visible_rows(
        mode: SelectorMode,
        rows: Vec<SelectorRow>,
        max_visible_rows: usize,
    ) -> Self {
        let mut state = Self {
            mode,
            rows,
            query: String::new(),
            active_row: None,
            visible_option_offset: 0,
            max_visible_rows: max_visible_rows.max(1),
        };
        state.reset_active_after_filter();
        state
    }

    #[cfg(test)]
    pub(crate) fn apply_input(&mut self, input: SelectorInput) -> SelectorTransition {
        self.apply_input_with_filter(input, true)
    }

    pub(crate) fn apply_input_with_filter(
        &mut self,
        input: SelectorInput,
        filter_enabled: bool,
    ) -> SelectorTransition {
        if !filter_enabled {
            match input {
                SelectorInput::Char(_) | SelectorInput::Backspace => {
                    return SelectorTransition::Continue;
                }
                SelectorInput::Space if self.mode == SelectorMode::Single => {
                    return SelectorTransition::Continue;
                }
                _ => {}
            }
        }

        match input {
            SelectorInput::Up => {
                self.move_active(-1);
                SelectorTransition::Continue
            }
            SelectorInput::Down => {
                self.move_active(1);
                SelectorTransition::Continue
            }
            SelectorInput::Space => {
                match self.mode {
                    SelectorMode::Single => {
                        self.query.push(' ');
                        self.reset_active_after_filter();
                    }
                    SelectorMode::Multi => self.toggle_active(),
                }
                SelectorTransition::Continue
            }
            SelectorInput::Enter => self.submit(),
            SelectorInput::Cancel => SelectorTransition::Cancelled,
            SelectorInput::Backspace => {
                self.query.pop();
                self.reset_active_after_filter();
                SelectorTransition::Continue
            }
            SelectorInput::Char(ch) => {
                if !ch.is_control() {
                    self.query.push(ch);
                    self.reset_active_after_filter();
                }
                SelectorTransition::Continue
            }
        }
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    #[cfg(test)]
    pub(crate) fn active_index(&self) -> Option<usize> {
        self.active_option().map(|option| option.index)
    }

    pub(crate) fn selected_indices(&self) -> Vec<usize> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                SelectorRow::Option(option) if option.selected && !option.disabled => {
                    Some(option.index)
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn visible_window(&self) -> SelectorWindow {
        let matching_options = self.matching_option_rows_with_sections();
        let start = self.visible_start(matching_options.len());
        let built = self.build_visible_window(&matching_options, start);
        let min_body_rows = if matching_options.is_empty() {
            1
        } else {
            cmp::min(matching_body_rows(&matching_options), self.max_visible_rows)
        };

        SelectorWindow {
            row_indices: built.row_indices,
            hidden_before: start,
            hidden_after: matching_options
                .len()
                .saturating_sub(start + built.visible_options),
            min_body_rows,
            has_hidden_context: start > 0 || matching_options.len() > start + built.visible_options,
        }
    }

    fn move_active(&mut self, direction: isize) {
        let options = self.focusable_matching_option_rows();
        if options.is_empty() {
            self.active_row = None;
            self.visible_option_offset = 0;
            return;
        }

        let current = self
            .active_row
            .and_then(|active| options.iter().position(|row| *row == active))
            .unwrap_or(0);
        let next = if direction < 0 {
            current.saturating_sub(1)
        } else {
            cmp::min(current + 1, options.len() - 1)
        };
        self.active_row = Some(options[next]);
        self.ensure_active_visible();
    }

    fn toggle_active(&mut self) {
        let Some(active_row) = self.active_row else {
            return;
        };
        if let Some(SelectorRow::Option(option)) = self.rows.get_mut(active_row)
            && !option.disabled
        {
            option.selected = !option.selected;
        }
    }

    fn submit(&self) -> SelectorTransition {
        match self.mode {
            SelectorMode::Single => self
                .active_option()
                .map(|option| {
                    SelectorTransition::Submitted(SelectorSubmission::Single(option.index))
                })
                .unwrap_or(SelectorTransition::Continue),
            SelectorMode::Multi => {
                SelectorTransition::Submitted(SelectorSubmission::Multi(self.selected_indices()))
            }
        }
    }

    fn reset_active_after_filter(&mut self) {
        let focusable = self.focusable_matching_option_rows();
        self.active_row = focusable
            .iter()
            .copied()
            .find(|row| match &self.rows[*row] {
                SelectorRow::Option(option) => option.selected,
                SelectorRow::Section(_) => false,
            })
            .or_else(|| focusable.first().copied());
        self.visible_option_offset = 0;
        self.ensure_active_visible();
    }

    fn ensure_active_visible(&mut self) {
        let matching_options = self.matching_option_rows_with_sections();
        if matching_options.is_empty() {
            self.visible_option_offset = 0;
            return;
        }

        self.visible_option_offset = cmp::min(
            self.visible_option_offset,
            matching_options.len().saturating_sub(1),
        );

        let Some(active_row) = self.active_row else {
            return;
        };
        let Some(active_position) = matching_options
            .iter()
            .position(|option| option.row_index == active_row)
        else {
            return;
        };

        if active_position < self.visible_option_offset {
            self.visible_option_offset = active_position;
            return;
        }

        while !self
            .build_visible_window(&matching_options, self.visible_option_offset)
            .contains_option_position(active_position)
        {
            if self.visible_option_offset >= active_position {
                break;
            }
            self.visible_option_offset += 1;
        }
    }

    fn visible_start(&self, matching_count: usize) -> usize {
        cmp::min(self.visible_option_offset, matching_count.saturating_sub(1))
    }

    fn matching_option_rows_with_sections(&self) -> Vec<MatchingOptionRow> {
        let mut current_section = None;
        let mut matching_options = Vec::new();
        for (row_index, row) in self.rows.iter().enumerate() {
            match row {
                SelectorRow::Section(_) => current_section = Some(row_index),
                SelectorRow::Option(option) if option.matches_query(&self.query) => {
                    matching_options.push(MatchingOptionRow {
                        row_index,
                        section_index: current_section,
                        body_rows: option_body_row_count(option),
                        has_description: !description_lines(option).is_empty(),
                    });
                }
                SelectorRow::Option(_) => {}
            }
        }
        matching_options
    }

    fn build_visible_window(
        &self,
        matching_options: &[MatchingOptionRow],
        start: usize,
    ) -> BuiltSelectorWindow {
        let mut row_indices = Vec::new();
        let mut visible_options = 0;
        let mut body_rows = 0;
        let mut emitted_section = None;
        let mut previous_option_had_description = false;

        for option in matching_options.iter().skip(start) {
            let next_rows = option_body_rows(
                *option,
                emitted_section,
                body_rows > 0,
                previous_option_had_description,
            );
            if visible_options > 0 && body_rows + next_rows > self.max_visible_rows {
                break;
            }

            if let Some(section_index) = option.section_index
                && emitted_section != Some(section_index)
            {
                row_indices.push(section_index);
                emitted_section = Some(section_index);
            }
            row_indices.push(option.row_index);
            body_rows += next_rows;
            previous_option_had_description = option.has_description;
            visible_options += 1;
        }

        BuiltSelectorWindow {
            row_indices,
            first_option_position: start,
            visible_options,
        }
    }

    fn focusable_matching_option_rows(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(row_index, row)| match row {
                SelectorRow::Option(option)
                    if !option.disabled && option.matches_query(&self.query) =>
                {
                    Some(row_index)
                }
                _ => None,
            })
            .collect()
    }

    fn active_option(&self) -> Option<&SelectorOption> {
        self.active_row
            .and_then(|row| self.rows.get(row))
            .and_then(|row| match row {
                SelectorRow::Option(option) if !option.disabled => Some(option),
                _ => None,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectorWindow {
    pub(crate) row_indices: Vec<usize>,
    pub(crate) hidden_before: usize,
    pub(crate) hidden_after: usize,
    pub(crate) min_body_rows: usize,
    pub(crate) has_hidden_context: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatchingOptionRow {
    row_index: usize,
    section_index: Option<usize>,
    body_rows: usize,
    has_description: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuiltSelectorWindow {
    row_indices: Vec<usize>,
    first_option_position: usize,
    visible_options: usize,
}

impl BuiltSelectorWindow {
    fn contains_option_position(&self, option_position: usize) -> bool {
        option_position >= self.first_option_position
            && option_position < self.first_option_position + self.visible_options
    }
}

fn matching_body_rows(matching_options: &[MatchingOptionRow]) -> usize {
    let mut body_rows = 0;
    let mut emitted_section = None;
    let mut previous_option_had_description = false;
    for option in matching_options {
        body_rows += option_body_rows(
            *option,
            emitted_section,
            body_rows > 0,
            previous_option_had_description,
        );
        if option.section_index.is_some() {
            emitted_section = option.section_index;
        }
        previous_option_had_description = option.has_description;
    }
    body_rows
}

fn option_body_rows(
    option: MatchingOptionRow,
    emitted_section: Option<usize>,
    has_previous_rows: bool,
    previous_option_had_description: bool,
) -> usize {
    let section_rows = match option.section_index {
        Some(section_index) if emitted_section != Some(section_index) => {
            1 + usize::from(has_previous_rows)
        }
        _ => 0,
    };
    let option_gap_rows = usize::from(
        section_rows == 0
            && has_previous_rows
            && (previous_option_had_description || option.has_description),
    );
    section_rows + option_gap_rows + option.body_rows
}

fn option_body_row_count(option: &SelectorOption) -> usize {
    1 + description_lines(option).len()
}

#[derive(Debug, Clone)]
pub(crate) struct SelectorRenderOptions {
    prompt: String,
    decorated: bool,
    filter_visible: bool,
    nested: bool,
    show_selected_summary: bool,
    summary_label_limit: usize,
}

impl SelectorRenderOptions {
    pub(crate) fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            decorated: true,
            filter_visible: true,
            nested: false,
            show_selected_summary: false,
            summary_label_limit: DEFAULT_SUMMARY_LABELS,
        }
    }

    pub(crate) fn decorated(mut self, decorated: bool) -> Self {
        self.decorated = decorated;
        self
    }

    pub(crate) fn selected_summary(mut self, show_selected_summary: bool) -> Self {
        self.show_selected_summary = show_selected_summary;
        self
    }

    pub(crate) fn filter_visible(mut self, filter_visible: bool) -> Self {
        self.filter_visible = filter_visible;
        self
    }

    pub(crate) fn nested(mut self, nested: bool) -> Self {
        self.nested = nested;
        self
    }

    #[cfg(test)]
    pub(crate) fn summary_label_limit(mut self, summary_label_limit: usize) -> Self {
        self.summary_label_limit = summary_label_limit.max(1);
        self
    }
}

pub(crate) fn render_selector(state: &SelectorState, options: &SelectorRenderOptions) -> String {
    render_selector_frame(state, options, SelectorRenderFrame::Active)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorRenderFrame {
    Active,
    Submitted,
    Cancelled,
}

fn render_selector_frame(
    state: &SelectorState,
    options: &SelectorRenderOptions,
    frame: SelectorRenderFrame,
) -> String {
    let mut lines = Vec::new();

    if options.nested {
        lines.push(options.prompt.clone());
    } else {
        let (prompt_symbol, prompt_style) = match frame {
            SelectorRenderFrame::Active => (PROMPT_START, accent_style()),
            SelectorRenderFrame::Submitted => (PROMPT_SUBMIT, selected_style()),
            SelectorRenderFrame::Cancelled => (PROMPT_CANCEL, cancel_style()),
        };
        lines.push(format!(
            "{} {}",
            styled(prompt_symbol, prompt_style, options.decorated),
            options.prompt
        ));
    }

    match frame {
        SelectorRenderFrame::Submitted => {
            lines.push(format!(
                "{} {}",
                styled(FOOTER, bar_style(), options.decorated),
                submitted_summary(state, options)
            ));
            return render_selector_lines(lines, options);
        }
        SelectorRenderFrame::Cancelled => {
            lines.push(format!(
                "{} Cancelled",
                styled(FOOTER, bar_style(), options.decorated)
            ));
            return render_selector_lines(lines, options);
        }
        SelectorRenderFrame::Active => {}
    }

    let window = state.visible_window();
    let hint_label_width = hint_label_width(state, &window);

    if options.filter_visible {
        lines.push(render_filter_line(state.query(), options.decorated));
    }
    lines.push(format!(
        "{} {}",
        styled(BAR, bar_style(), options.decorated),
        styled(key_hint(state.mode), hint_style(), options.decorated)
    ));
    lines.push(styled(BAR, bar_style(), options.decorated));

    if window.hidden_before > 0 {
        lines.push(format!(
            "{} ↑ {} more",
            styled(BAR, bar_style(), options.decorated),
            window.hidden_before
        ));
    }

    let mut body_rows = 0;
    let mut previous_row_was_option = false;
    let mut previous_option_had_description = false;
    if window.row_indices.is_empty() {
        lines.push(format!(
            "{} No matches",
            styled(BAR, bar_style(), options.decorated)
        ));
        body_rows += 1;
    } else {
        for (rendered_rows, row_index) in window.row_indices.iter().enumerate() {
            match &state.rows[*row_index] {
                SelectorRow::Section(section) => {
                    if rendered_rows > 0 {
                        lines.push(styled(BAR, bar_style(), options.decorated));
                        body_rows += 1;
                    }
                    lines.push(render_section(section, options.decorated));
                    body_rows += 1;
                    previous_row_was_option = false;
                    previous_option_had_description = false;
                }
                SelectorRow::Option(option) => {
                    let has_description = !description_lines(option).is_empty();
                    if previous_row_was_option
                        && (previous_option_had_description || has_description)
                    {
                        lines.push(styled(BAR, bar_style(), options.decorated));
                        body_rows += 1;
                    }
                    let option_lines = render_option(
                        state,
                        *row_index,
                        option,
                        hint_label_width,
                        options.decorated,
                    );
                    body_rows += option_lines.len();
                    lines.extend(option_lines);
                    previous_row_was_option = true;
                    previous_option_had_description = has_description;
                }
            }
        }
    }

    while body_rows < window.min_body_rows {
        lines.push(styled(BAR, bar_style(), options.decorated));
        body_rows += 1;
    }

    if window.hidden_after > 0 {
        lines.push(format!(
            "{} ↓ {} more",
            styled(BAR, bar_style(), options.decorated),
            window.hidden_after
        ));
    } else if window.has_hidden_context {
        lines.push(styled(BAR, bar_style(), options.decorated));
    }

    if let Some(summary) = selected_summary(state, options) {
        lines.push(format!(
            "{} {}",
            styled(BAR, bar_style(), options.decorated),
            summary
        ));
    }

    lines.push(styled(FOOTER, bar_style(), options.decorated));
    render_selector_lines(lines, options)
}

fn render_selector_lines(lines: Vec<String>, options: &SelectorRenderOptions) -> String {
    if !options.nested {
        return format!("{}\n", lines.join("\n"));
    }
    let rendered = lines
        .into_iter()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n{rendered}\n")
}

pub(crate) fn run_selector_prompt<W: Write>(
    writer: &mut W,
    state: &mut SelectorState,
    render_options: &SelectorRenderOptions,
) -> io::Result<SelectorSubmission> {
    let _raw_mode = RawModeGuard::enable()?;
    queue!(writer, cursor::Hide)?;
    writer.flush()?;

    let result = {
        let mut terminal = CrosstermSelectorTerminal::new(writer);
        terminal.run(state, render_options)
    };
    let cleanup = queue!(writer, cursor::Show).and_then(|_| writer.flush());

    match (result, cleanup) {
        (Ok(submission), Ok(())) => Ok(submission),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err),
    }
}

struct CrosstermSelectorTerminal<'a, W: Write> {
    writer: &'a mut W,
    rendered_lines: u16,
}

impl<'a, W: Write> CrosstermSelectorTerminal<'a, W> {
    fn new(writer: &'a mut W) -> Self {
        Self {
            writer,
            rendered_lines: 0,
        }
    }

    fn run(
        &mut self,
        state: &mut SelectorState,
        render_options: &SelectorRenderOptions,
    ) -> io::Result<SelectorSubmission> {
        self.draw(&render_selector(state, render_options))?;

        loop {
            let Event::Key(event) = event::read()? else {
                continue;
            };
            let Some(input) = SelectorInput::from_key_event(event) else {
                continue;
            };

            match state.apply_input_with_filter(input, render_options.filter_visible) {
                SelectorTransition::Continue => {
                    self.draw(&render_selector(state, render_options))?
                }
                SelectorTransition::Submitted(submission) => {
                    self.draw(&render_selector_frame(
                        state,
                        render_options,
                        SelectorRenderFrame::Submitted,
                    ))?;
                    return Ok(submission);
                }
                SelectorTransition::Cancelled => {
                    self.draw(&render_selector_frame(
                        state,
                        render_options,
                        SelectorRenderFrame::Cancelled,
                    ))?;
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                }
            }
        }
    }

    fn draw(&mut self, rendered: &str) -> io::Result<()> {
        if self.rendered_lines > 0 {
            queue!(
                self.writer,
                cursor::MoveUp(self.rendered_lines),
                terminal::Clear(ClearType::FromCursorDown)
            )?;
        }
        write!(self.writer, "{}", raw_terminal_output(rendered))?;
        self.writer.flush()?;
        self.rendered_lines = rendered_visual_rows(rendered, terminal_columns());
        Ok(())
    }
}

fn terminal_columns() -> u16 {
    terminal::size()
        .map(|(columns, _)| columns)
        .unwrap_or(80)
        .max(1)
}

fn rendered_visual_rows(rendered: &str, columns: u16) -> u16 {
    let columns = usize::from(columns.max(1));
    let rows = rendered
        .lines()
        .map(|line| visual_rows_for_line(line, columns))
        .sum::<usize>();
    rows.try_into().unwrap_or(u16::MAX)
}

fn visual_rows_for_line(line: &str, columns: usize) -> usize {
    let plain = console::strip_ansi_codes(line);
    let width = measure_text_width(plain.as_ref());
    if width == 0 {
        1
    } else {
        ((width - 1) / columns) + 1
    }
}

fn raw_terminal_output(rendered: &str) -> String {
    rendered.replace('\n', "\r\n")
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn render_filter_line(query: &str, decorated: bool) -> String {
    let value = if query.is_empty() {
        styled("type to search", hint_style(), decorated)
    } else {
        query.to_string()
    };
    format!("{} Filter: {}", styled(BAR, bar_style(), decorated), value)
}

fn key_hint(mode: SelectorMode) -> &'static str {
    match mode {
        SelectorMode::Single => "↑↓ move, enter select, esc cancel",
        SelectorMode::Multi => "↑↓ move, space toggle, enter submit, esc cancel",
    }
}

fn render_section(section: &SelectorSection, decorated: bool) -> String {
    let title = sanitize_selector_text(&section.title);
    let title = styled(&title, section_style(), decorated);
    let hint = metadata_text(section.hint.as_deref(), false);
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!(
            "{}{}",
            " ".repeat(HINT_GAP),
            styled(&hint, hint_style(), decorated)
        )
    };
    format!("{} {}{}", styled(BAR, bar_style(), decorated), title, hint)
}

fn render_option(
    state: &SelectorState,
    row_index: usize,
    option: &SelectorOption,
    hint_label_width: Option<usize>,
    decorated: bool,
) -> Vec<String> {
    let active = state.active_row == Some(row_index);
    let cursor = if active {
        styled(CURSOR_ACTIVE, accent_style(), decorated)
    } else {
        " ".repeat(measure_text_width(CURSOR_ACTIVE))
    };
    let selected = match state.mode {
        SelectorMode::Single => active,
        SelectorMode::Multi => option.selected,
    };
    let marker = if selected {
        styled(RADIO_SELECTED, selected_style(), decorated)
    } else {
        styled(RADIO_UNSELECTED, inactive_style(), decorated)
    };
    let label_text = sanitize_selector_text(&option.label);
    let label = styled(
        &label_text,
        option_label_style(active, selected, option.disabled),
        decorated,
    );
    let metadata = metadata_text(option.hint.as_deref(), option.disabled);
    let hint = if metadata.is_empty() {
        String::new()
    } else {
        let label_width = measure_text_width(&label_text);
        let padding = hint_label_width
            .map(|width| width.saturating_sub(label_width))
            .unwrap_or_default()
            + HINT_GAP;
        format!(
            "{}{}",
            " ".repeat(padding),
            styled(&metadata, hint_style(), decorated)
        )
    };

    let mut lines = vec![format!(
        "{} {} {}  {}{}",
        styled(BAR, bar_style(), decorated),
        cursor,
        marker,
        label,
        hint
    )];

    let description_indent =
        measure_text_width(CURSOR_ACTIVE) + 1 + measure_text_width(RADIO_SELECTED) + 2;
    for description in description_lines(option) {
        lines.push(format!(
            "{} {}{}",
            styled(BAR, bar_style(), decorated),
            " ".repeat(description_indent),
            styled(&description, hint_style(), decorated)
        ));
    }

    lines
}

fn hint_label_width(state: &SelectorState, window: &SelectorWindow) -> Option<usize> {
    window
        .row_indices
        .iter()
        .filter_map(|row_index| match &state.rows[*row_index] {
            SelectorRow::Option(option)
                if !metadata_text(option.hint.as_deref(), option.disabled).is_empty() =>
            {
                Some(measure_text_width(&sanitize_selector_text(&option.label)))
            }
            _ => None,
        })
        .max()
}

fn selected_summary(state: &SelectorState, options: &SelectorRenderOptions) -> Option<String> {
    if state.mode != SelectorMode::Multi || !options.show_selected_summary {
        return None;
    }

    let labels = state
        .rows
        .iter()
        .filter_map(|row| match row {
            SelectorRow::Option(option) if option.selected && !option.disabled => {
                let label = sanitize_selector_text(&option.label);
                (!label.is_empty()).then_some(label)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if labels.is_empty() {
        return None;
    }

    let visible = labels
        .iter()
        .take(options.summary_label_limit)
        .cloned()
        .collect::<Vec<_>>();
    let more = labels.len().saturating_sub(visible.len());
    let suffix = if more == 0 {
        String::new()
    } else {
        format!(" +{more} more")
    };
    Some(format!("Selected: {}{}", visible.join(", "), suffix))
}

fn submitted_summary(state: &SelectorState, options: &SelectorRenderOptions) -> String {
    let labels = submitted_labels(state);
    if labels.is_empty() {
        return if options.nested {
            "완료".to_string()
        } else {
            "Submitted".to_string()
        };
    }

    let visible = labels
        .iter()
        .take(options.summary_label_limit)
        .cloned()
        .collect::<Vec<_>>();
    let more = labels.len().saturating_sub(visible.len());
    let suffix = if more == 0 {
        String::new()
    } else if options.nested {
        format!(" 외 {more}개")
    } else {
        format!(" +{more} more")
    };
    let prefix = if options.nested { "선택" } else { "Selected" };
    format!("{prefix}: {}{suffix}", visible.join(", "))
}

fn submitted_labels(state: &SelectorState) -> Vec<String> {
    match state.mode {
        SelectorMode::Single => state
            .active_option()
            .and_then(option_summary)
            .into_iter()
            .collect(),
        SelectorMode::Multi => state
            .rows
            .iter()
            .filter_map(|row| match row {
                SelectorRow::Option(option) if option.selected && !option.disabled => {
                    option_summary(option)
                }
                _ => None,
            })
            .collect(),
    }
}

fn option_summary(option: &SelectorOption) -> Option<String> {
    let label = sanitize_selector_text(&option.label);
    let hint = option
        .hint
        .as_deref()
        .map(sanitize_selector_text)
        .filter(|hint| !hint.is_empty());

    match (label.is_empty(), hint) {
        (false, Some(hint)) => Some(format!("{label} ({hint})")),
        (false, None) => Some(label),
        (true, Some(hint)) => Some(hint),
        (true, None) => None,
    }
}

fn option_label_style(active: bool, selected: bool, disabled: bool) -> Style {
    if disabled {
        inactive_style()
    } else if active {
        accent_style()
    } else if selected {
        selected_style()
    } else {
        Style::new()
    }
}

fn metadata_text(hint: Option<&str>, disabled: bool) -> String {
    let mut parts = hint
        .into_iter()
        .map(sanitize_selector_text)
        .flat_map(|hint| {
            hint.split('|')
                .map(str::to_string)
                .collect::<Vec<_>>()
                .into_iter()
        })
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if disabled {
        parts.push("disabled".into());
    }
    parts.join(" · ")
}

fn description_lines(option: &SelectorOption) -> Vec<String> {
    option
        .description
        .as_deref()
        .into_iter()
        .flat_map(str::lines)
        .map(sanitize_selector_text)
        .filter(|line| !line.is_empty())
        .collect()
}

fn sanitize_selector_text(value: &str) -> String {
    collapse_selector_whitespace(&strip_terminal_sequences(value))
}

fn strip_terminal_sequences(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => skip_escape_sequence(&mut chars),
            '\u{0080}'..='\u{009f}' => {}
            ch if is_unsafe_control(ch) => {}
            ch => output.push(ch),
        }
    }

    output
}

fn skip_escape_sequence<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    match chars.peek().copied() {
        Some('[') => {
            chars.next();
            skip_csi_sequence(chars);
        }
        Some(']') => {
            chars.next();
            skip_control_string(chars);
        }
        Some('P' | 'X' | '^' | '_') => {
            chars.next();
            skip_control_string(chars);
        }
        Some(ch) if is_ascii_escape_byte(ch) => {
            chars.next();
        }
        _ => {}
    }
}

fn skip_csi_sequence<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    for ch in chars.by_ref() {
        if is_csi_final_byte(ch) {
            break;
        }
    }
}

fn skip_control_string<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    while let Some(ch) = chars.next() {
        if ch == '\x07' {
            break;
        }
        if ch == '\x1b' && chars.peek() == Some(&'\\') {
            chars.next();
            break;
        }
    }
}

fn is_ascii_escape_byte(ch: char) -> bool {
    ('\u{0020}'..='\u{007e}').contains(&ch)
}

fn is_csi_final_byte(ch: char) -> bool {
    ('\u{0040}'..='\u{007e}').contains(&ch)
}

fn is_unsafe_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{0000}'..='\u{0008}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'..='\u{001a}'
            | '\u{001c}'..='\u{001f}'
            | '\u{007f}'
    )
}

fn collapse_selector_whitespace(value: &str) -> String {
    let mut collapsed = String::with_capacity(value.len());
    let mut pending_space = false;

    for ch in value.chars() {
        if ch == '\t' || ch == '\n' || ch == '\r' {
            pending_space = true;
            continue;
        }

        if pending_space && !collapsed.is_empty() && !ch.is_whitespace() {
            collapsed.push(' ');
        }
        pending_space = false;
        collapsed.push(ch);
    }

    collapsed.trim().to_string()
}

fn normalized_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|term| !term.is_empty())
        .collect()
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn styled(value: &str, style: Style, decorated: bool) -> String {
    if decorated {
        style.apply_to(value).to_string()
    } else {
        value.to_string()
    }
}

fn accent_style() -> Style {
    Style::new().color256(110).bold()
}

fn selected_style() -> Style {
    Style::new().color256(114).bold()
}

fn cancel_style() -> Style {
    Style::new().red().bold()
}

fn section_style() -> Style {
    Style::new().bold()
}

fn bar_style() -> Style {
    accent_style()
}

fn hint_style() -> Style {
    Style::new().color256(245)
}

fn inactive_style() -> Style {
    Style::new().color256(245).dim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use console::strip_ansi_codes;

    #[test]
    fn movement_skips_section_headers_and_disabled_options() {
        let mut state = SelectorState::single(vec![
            SelectorRow::section("GitHub"),
            SelectorRow::option(0, "Fix editor"),
            SelectorRow::Option(SelectorOption::new(1, "Archived task").disabled(true)),
            SelectorRow::section("Local"),
            SelectorRow::option(2, "Local cleanup"),
        ]);

        assert_eq!(state.active_index(), Some(0));
        state.apply_input(SelectorInput::Down);
        assert_eq!(state.active_index(), Some(2));
        state.apply_input(SelectorInput::Down);
        assert_eq!(state.active_index(), Some(2));
        state.apply_input(SelectorInput::Up);
        assert_eq!(state.active_index(), Some(0));
    }

    #[test]
    fn filtering_matches_label_hint_and_search_only_text() {
        let mut state = SelectorState::single(vec![
            SelectorRow::option_with_hint(0, "Fix editor", "Linear PROJ-123"),
            SelectorRow::Option(
                SelectorOption::new(1, "Local cleanup").search_text("branch tmp-cleanup"),
            ),
        ]);

        type_text(&mut state, "proj");
        assert_eq!(state.query(), "proj");
        assert_eq!(state.active_index(), Some(0));

        type_text(&mut state, " nope");
        assert_eq!(state.active_index(), None);
        assert_eq!(
            state.apply_input(SelectorInput::Enter),
            SelectorTransition::Continue
        );

        for _ in 0.." nope".chars().count() {
            state.apply_input(SelectorInput::Backspace);
        }
        assert_eq!(state.active_index(), Some(0));
    }

    #[test]
    fn filtering_uses_sanitized_text_not_hidden_terminal_payloads() {
        let rows = vec![
            SelectorRow::option_with_hint(
                0,
                "Fix\x1b]0;owned\x07 editor",
                "Linear\x1b[2K PROJ-123",
            ),
            SelectorRow::Option(
                SelectorOption::new(1, "Local cleanup")
                    .search_text("branch\x1b]0;owned\x07 tmp-cleanup"),
            ),
        ];

        let mut hidden_payload = SelectorState::single(rows.clone());
        type_text(&mut hidden_payload, "owned");
        assert_eq!(hidden_payload.active_index(), None);

        let mut visible_label = SelectorState::single(rows.clone());
        type_text(&mut visible_label, "fix editor");
        assert_eq!(visible_label.active_index(), Some(0));

        let mut visible_hint = SelectorState::single(rows.clone());
        type_text(&mut visible_hint, "linear proj");
        assert_eq!(visible_hint.active_index(), Some(0));

        let mut visible_search_text = SelectorState::single(rows);
        type_text(&mut visible_search_text, "branch tmp");
        assert_eq!(visible_search_text.active_index(), Some(1));
    }

    #[test]
    fn filtering_removes_empty_sections_from_visible_rows() {
        let mut state = SelectorState::single(vec![
            SelectorRow::section("GitHub"),
            SelectorRow::option(0, "Fix editor"),
            SelectorRow::section("Local"),
            SelectorRow::option(1, "Local cleanup"),
        ]);

        type_text(&mut state, "local");
        let rendered = render_plain(&state, selector_options("Tasks"));

        assert!(!rendered.contains("GitHub"));
        assert!(rendered.contains("Local"));
        assert!(rendered.contains("Local cleanup"));
    }

    #[test]
    fn multiselect_toggles_only_the_active_option_and_backspace_preserves_selection() {
        let mut state = SelectorState::multi(vec![
            SelectorRow::section("Tasks"),
            SelectorRow::option(0, "Fix editor"),
            SelectorRow::option(1, "Local cleanup"),
        ]);

        state.apply_input(SelectorInput::Space);
        assert_eq!(state.selected_indices(), vec![0]);

        type_text(&mut state, "local");
        assert_eq!(state.active_index(), Some(1));
        state.apply_input(SelectorInput::Space);
        assert_eq!(state.selected_indices(), vec![0, 1]);

        state.apply_input(SelectorInput::Backspace);
        assert_eq!(state.selected_indices(), vec![0, 1]);
    }

    #[test]
    fn enter_and_cancel_return_stable_transitions() {
        let mut single = SelectorState::single(vec![SelectorRow::option(3, "Fix editor")]);
        assert_eq!(
            single.apply_input(SelectorInput::Enter),
            SelectorTransition::Submitted(SelectorSubmission::Single(3))
        );

        let mut multi = SelectorState::multi(vec![
            SelectorRow::Option(SelectorOption::new(1, "Fix editor").selected(true)),
            SelectorRow::Option(SelectorOption::new(2, "Local cleanup").selected(true)),
        ]);
        assert_eq!(
            multi.apply_input(SelectorInput::Enter),
            SelectorTransition::Submitted(SelectorSubmission::Multi(vec![1, 2]))
        );
        assert_eq!(
            multi.apply_input(SelectorInput::Cancel),
            SelectorTransition::Cancelled
        );
    }

    #[test]
    fn render_grouped_multiselect_without_fake_selectable_headers() {
        let mut state = SelectorState::multi(vec![
            SelectorRow::section("GitHub"),
            SelectorRow::Option(SelectorOption::with_hint(0, "Fix", "GitHub #73").selected(true)),
            SelectorRow::option_with_hint(1, "Publish docs", "GitHub #74"),
            SelectorRow::section("Local"),
            SelectorRow::option_with_hint(2, "Local cleanup", "prepared"),
        ]);
        let rendered = render_plain(
            &state,
            selector_options("Tasks to start").selected_summary(true),
        );

        assert_eq!(
            rendered,
            "\
◆ Tasks to start
│ Filter: type to search
│ ↑↓ move, space toggle, enter submit, esc cancel
│
│ GitHub
│ ❯ ●  Fix            GitHub #73
│   ○  Publish docs   GitHub #74
│
│ Local
│   ○  Local cleanup  prepared
│ Selected: Fix
└
"
        );

        state.apply_input(SelectorInput::Down);
        let rendered = render_plain(&state, selector_options("Tasks to start"));
        assert!(rendered.contains("│   ●  Fix"));
        assert!(rendered.contains("│ ❯ ○  Publish docs"));
    }

    #[test]
    fn plain_label_rendering_has_no_fake_hint_column() {
        let state = SelectorState::single(vec![
            SelectorRow::option(0, "Fix"),
            SelectorRow::option(1, "Publish documentation"),
        ]);
        let rendered = render_plain(&state, selector_options("Pick one"));

        assert_eq!(
            rendered,
            "\
◆ Pick one
│ Filter: type to search
│ ↑↓ move, enter select, esc cancel
│
│ ❯ ●  Fix
│   ○  Publish documentation
└
"
        );
    }

    #[test]
    fn option_descriptions_render_under_labels() {
        let state = SelectorState::single(vec![
            SelectorRow::Option(
                SelectorOption::new(0, "감지한 개발 설정 저장")
                    .description("감지한 setup/test 명령과 로컬 파일을 저장합니다."),
            ),
            SelectorRow::Option(
                SelectorOption::new(1, "자동화 없이 최소 설정")
                    .description("setup/test/editor/browser 없이 빈 workspace만 저장합니다."),
            ),
        ]);
        let rendered = render_plain(
            &state,
            selector_options("개발 환경 설정을 어떻게 만들까요?").filter_visible(false),
        );

        assert!(rendered.contains("│ ❯ ●  감지한 개발 설정 저장"));
        assert!(rendered.contains(&format!(
            "│ {}감지한 setup/test 명령과 로컬 파일을 저장합니다.",
            " ".repeat(5)
        )));
        assert!(rendered.contains(
            "│      감지한 setup/test 명령과 로컬 파일을 저장합니다.\n│\n│   ○  자동화 없이 최소 설정"
        ));
        assert!(rendered.contains("│   ○  자동화 없이 최소 설정"));
        assert!(rendered.contains(&format!(
            "│ {}setup/test/editor/browser 없이 빈 workspace만 저장합니다.",
            " ".repeat(5)
        )));
    }

    #[test]
    fn initial_render_shows_filter_and_key_affordances_before_typing() {
        let mut state = SelectorState::single(vec![SelectorRow::option(0, "Fix editor")]);
        let rendered = render_plain(&state, selector_options("Pick one"));

        assert!(rendered.contains("│ Filter: type to search"));
        assert!(rendered.contains("│ ↑↓ move, enter select, esc cancel"));

        type_text(&mut state, "fix");
        let rendered = render_plain(&state, selector_options("Pick one"));
        assert!(rendered.contains("│ Filter: fix"));
    }

    #[test]
    fn filter_can_be_hidden_for_small_fixed_decisions() {
        let mut state = SelectorState::single(vec![
            SelectorRow::option(0, "Personal config"),
            SelectorRow::option(1, "Shared config"),
        ]);
        let rendered = render_plain(
            &state,
            selector_options("Where should wt write config?").filter_visible(false),
        );

        assert!(!rendered.contains("Filter:"));
        assert!(rendered.contains("│ ↑↓ move, enter select, esc cancel"));

        assert_eq!(
            state.apply_input_with_filter(SelectorInput::Char('s'), false),
            SelectorTransition::Continue
        );
        assert_eq!(state.query(), "");
        assert_eq!(state.active_index(), Some(0));
    }

    #[test]
    fn nested_selector_renders_as_field_inside_parent_step() {
        let state = SelectorState::single(vec![
            SelectorRow::option(0, "개인 설정 파일"),
            SelectorRow::option(1, "팀 공유 설정"),
        ]);
        let rendered = render_plain(
            &state,
            selector_options("저장 위치")
                .filter_visible(false)
                .nested(true),
        );

        assert_eq!(
            rendered,
            "
  저장 위치
  │ ↑↓ move, enter select, esc cancel
  │
  │ ❯ ●  개인 설정 파일
  │   ○  팀 공유 설정
  └
"
        );
    }

    #[test]
    fn rendering_sanitizes_untrusted_selector_text() {
        let state = SelectorState::multi(vec![
            SelectorRow::Section(SelectorSection::with_hint(
                "Git\x1b[31mHub\x1b[0m\nIssues",
                "remote\x1b]0;owned\x07 provider",
            )),
            SelectorRow::Option(
                SelectorOption::with_hint(
                    0,
                    "Fix\x1b[2K editor\tissue\nnow\x07",
                    "task | \x1b7save | PR\rtitle | \u{0090}c1",
                )
                .selected(true),
            ),
        ]);
        let rendered = render_plain(&state, selector_options("Tasks").selected_summary(true));

        assert!(rendered.contains("│ GitHub Issues  remote provider"));
        assert!(rendered.contains("│ ❯ ●  Fix editor issue now  task · save · PRtitle · c1"));
        assert!(rendered.contains("│ Selected: Fix editor issue now"));
        assert!(!rendered.contains("owned"));
        assert!(!rendered.contains('\x1b'));
        assert!(!rendered.contains('\x07'));
        assert!(!rendered.contains('\x08'));
        assert!(!rendered.contains('\u{0090}'));
    }

    #[test]
    fn selector_text_sanitization_strips_escape_families_and_collapses_lines() {
        assert_eq!(
            sanitize_selector_text(
                " alpha\tbeta\n\x1b[31mred\x1b[0m\x1b]0;title\x07\x1b7\u{009b}\x08omega "
            ),
            "alpha beta redomega"
        );
    }

    #[test]
    fn hidden_counts_track_matching_option_rows() {
        let mut rows = vec![SelectorRow::section("GitHub")];
        rows.extend((0..6).map(|index| SelectorRow::option(index, format!("Task {index}"))));
        rows.push(SelectorRow::section("Local"));
        rows.extend((6..12).map(|index| SelectorRow::option(index, format!("Task {index}"))));
        let mut state = SelectorState::with_max_visible_rows(SelectorMode::Single, rows, 3);

        for _ in 0..4 {
            state.apply_input(SelectorInput::Down);
        }

        let window = state.visible_window();
        assert_eq!(window.hidden_before, 3);
        assert_eq!(window.hidden_after, 7);

        let rendered = render_plain(&state, selector_options("Tasks"));
        assert!(rendered.contains("│ ↑ 3 more"));
        assert!(rendered.contains("│ ↓ 7 more"));
    }

    #[test]
    fn grouped_selector_keeps_frame_height_stable_while_scrolling() {
        let mut rows = vec![
            SelectorRow::section("not started"),
            SelectorRow::option(0, "Task 0"),
            SelectorRow::option(1, "Task 1"),
            SelectorRow::section("prepared"),
            SelectorRow::option(2, "Task 2"),
            SelectorRow::option(3, "Task 3"),
            SelectorRow::option(4, "Task 4"),
            SelectorRow::section("skipped"),
            SelectorRow::option(5, "Task 5"),
            SelectorRow::section("failed"),
            SelectorRow::option(6, "Task 6"),
            SelectorRow::option(7, "Task 7"),
            SelectorRow::section("running"),
            SelectorRow::option(8, "Task 8"),
            SelectorRow::section("done"),
        ];
        rows.extend((9..28).map(|index| SelectorRow::option(index, format!("Task {index}"))));
        let mut state = SelectorState::multi(rows);
        state.apply_input(SelectorInput::Space);

        for _ in 0..5 {
            state.apply_input(SelectorInput::Down);
        }
        let mixed_groups = render_plain(
            &state,
            selector_options("Tasks to publish").selected_summary(true),
        );

        for _ in 0..8 {
            state.apply_input(SelectorInput::Down);
        }
        let single_group = render_plain(
            &state,
            selector_options("Tasks to publish").selected_summary(true),
        );

        assert!(mixed_groups.contains("│ skipped"));
        assert!(single_group.contains("│ done"));
        assert_eq!(mixed_groups.lines().count(), single_group.lines().count());
    }

    #[test]
    fn selected_summary_collapses_long_selections() {
        let rows = ["One", "Two", "Three", "Four", "Five"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                SelectorRow::Option(SelectorOption::new(index, label).selected(true))
            })
            .collect::<Vec<_>>();
        let state = SelectorState::multi(rows);
        let rendered = render_plain(
            &state,
            selector_options("Tasks")
                .selected_summary(true)
                .summary_label_limit(3),
        );

        assert!(rendered.contains("│ Selected: One, Two, Three +2 more"));
    }

    #[test]
    fn disabled_rows_have_text_state_and_do_not_submit() {
        let state = SelectorState::single(vec![SelectorRow::Option(
            SelectorOption::new(7, "Archived task").disabled(true),
        )]);

        assert_eq!(state.active_index(), None);
        let rendered = render_plain(&state, selector_options("Tasks"));
        assert!(rendered.contains("Archived task  disabled"));

        let mut state = state;
        assert_eq!(
            state.apply_input(SelectorInput::Enter),
            SelectorTransition::Continue
        );
    }

    #[test]
    fn key_mapping_keeps_escape_and_ctrl_c_as_cancel() {
        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        assert_eq!(
            SelectorInput::from_key_event(escape),
            Some(SelectorInput::Cancel)
        );
        assert_eq!(
            SelectorInput::from_key_event(ctrl_c),
            Some(SelectorInput::Cancel)
        );
    }

    #[test]
    fn final_frames_render_submit_and_cancel_without_changing_transitions() {
        let mut state = SelectorState::single(vec![SelectorRow::option(7, "Fix editor")]);
        assert_eq!(
            state.apply_input(SelectorInput::Enter),
            SelectorTransition::Submitted(SelectorSubmission::Single(7))
        );
        assert_eq!(
            render_final_plain(
                &state,
                selector_options("Pick one"),
                SelectorRenderFrame::Submitted
            ),
            "\
◇ Pick one
└ Selected: Fix editor
"
        );

        let mut state = SelectorState::single(vec![SelectorRow::option(7, "Fix editor")]);
        assert_eq!(
            state.apply_input(SelectorInput::Cancel),
            SelectorTransition::Cancelled
        );
        assert_eq!(
            render_final_plain(
                &state,
                selector_options("Pick one"),
                SelectorRenderFrame::Cancelled
            ),
            "\
■ Pick one
└ Cancelled
"
        );
    }

    #[test]
    fn nested_final_frame_renders_selected_label_and_hint() {
        let mut state = SelectorState::single(vec![
            SelectorRow::option_with_hint(0, "개인 설정 파일", "보통 .git/wt/config.toml"),
            SelectorRow::option_with_hint(1, "팀 공유 설정", "./.wt.toml"),
        ]);

        assert_eq!(
            state.apply_input(SelectorInput::Enter),
            SelectorTransition::Submitted(SelectorSubmission::Single(0))
        );
        assert_eq!(
            render_final_plain(
                &state,
                selector_options("저장 위치")
                    .filter_visible(false)
                    .nested(true),
                SelectorRenderFrame::Submitted
            ),
            "\n  저장 위치\n  └ 선택: 개인 설정 파일 (보통 .git/wt/config.toml)\n"
        );
    }

    #[test]
    fn submitted_multiselect_summary_collapses_long_selections() {
        let mut state = SelectorState::multi(
            ["One", "Two", "Three", "Four"]
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    SelectorRow::Option(SelectorOption::new(index, label).selected(true))
                })
                .collect(),
        );

        assert_eq!(
            state.apply_input(SelectorInput::Enter),
            SelectorTransition::Submitted(SelectorSubmission::Multi(vec![0, 1, 2, 3]))
        );
        assert_eq!(
            render_final_plain(
                &state,
                selector_options("Pick many").summary_label_limit(2),
                SelectorRenderFrame::Submitted
            ),
            "\
◇ Pick many
└ Selected: One, Two +2 more
"
        );
    }

    #[test]
    fn visual_row_count_tracks_wrapped_ascii_cjk_emoji_and_ansi_lines() {
        assert_eq!(visual_rows_for_line("abcdefghijk", 10), 2);
        assert_eq!(visual_rows_for_line("인디위키보호", 10), 2);
        assert_eq!(visual_rows_for_line("🙂🙂🙂🙂🙂🙂", 10), 2);
        assert_eq!(visual_rows_for_line("\x1b[31mabcdefghijk\x1b[0m", 10), 2);
        assert_eq!(rendered_visual_rows("abcdefghijk\nshort\n", 10), 3);
    }

    #[test]
    fn raw_terminal_output_returns_to_column_zero_after_each_line() {
        assert_eq!(raw_terminal_output("one\ntwo\n"), "one\r\ntwo\r\n");
    }

    fn type_text(state: &mut SelectorState, text: &str) {
        for ch in text.chars() {
            if ch == ' ' {
                state.apply_input(SelectorInput::Space);
            } else {
                state.apply_input(SelectorInput::Char(ch));
            }
        }
    }

    fn selector_options(prompt: &str) -> SelectorRenderOptions {
        SelectorRenderOptions::new(prompt).decorated(false)
    }

    fn render_plain(state: &SelectorState, options: SelectorRenderOptions) -> String {
        strip_ansi_codes(&render_selector(state, &options)).into_owned()
    }

    fn render_final_plain(
        state: &SelectorState,
        options: SelectorRenderOptions,
        frame: SelectorRenderFrame,
    ) -> String {
        strip_ansi_codes(&render_selector_frame(state, &options, frame)).into_owned()
    }
}
