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

        let mut haystack = self.label.to_lowercase();
        if let Some(hint) = self.hint.as_deref() {
            haystack.push(' ');
            haystack.push_str(&hint.to_lowercase());
        }
        for text in &self.search_text {
            haystack.push(' ');
            haystack.push_str(&text.to_lowercase());
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

    pub(crate) fn apply_input(&mut self, input: SelectorInput) -> SelectorTransition {
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

        for option in matching_options.iter().skip(start) {
            let next_rows = option_body_rows(*option, emitted_section, body_rows > 0);
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
    for option in matching_options {
        body_rows += option_body_rows(*option, emitted_section, body_rows > 0);
        if option.section_index.is_some() {
            emitted_section = option.section_index;
        }
    }
    body_rows
}

fn option_body_rows(
    option: MatchingOptionRow,
    emitted_section: Option<usize>,
    has_previous_rows: bool,
) -> usize {
    let section_rows = match option.section_index {
        Some(section_index) if emitted_section != Some(section_index) => {
            1 + usize::from(has_previous_rows)
        }
        _ => 0,
    };
    section_rows + 1
}

#[derive(Debug, Clone)]
pub(crate) struct SelectorRenderOptions {
    prompt: String,
    decorated: bool,
    show_selected_summary: bool,
    summary_label_limit: usize,
}

impl SelectorRenderOptions {
    pub(crate) fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            decorated: true,
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

    #[cfg(test)]
    pub(crate) fn summary_label_limit(mut self, summary_label_limit: usize) -> Self {
        self.summary_label_limit = summary_label_limit.max(1);
        self
    }
}

pub(crate) fn render_selector(state: &SelectorState, options: &SelectorRenderOptions) -> String {
    let window = state.visible_window();
    let hint_label_width = hint_label_width(state, &window);
    let mut lines = Vec::new();

    lines.push(format!(
        "{} {}",
        styled(PROMPT_START, accent_style(), options.decorated),
        options.prompt
    ));
    if state.query().is_empty() && !window.has_hidden_context {
        lines.push(styled(BAR, bar_style(), options.decorated));
    } else if !state.query().is_empty() {
        lines.push(format!(
            "{} Filter: {}",
            styled(BAR, bar_style(), options.decorated),
            state.query()
        ));
    }

    if window.hidden_before > 0 {
        lines.push(format!(
            "{} ↑ {} more",
            styled(BAR, bar_style(), options.decorated),
            window.hidden_before
        ));
    } else if window.has_hidden_context {
        lines.push(styled(BAR, bar_style(), options.decorated));
    }

    let mut body_rows = 0;
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
                }
                SelectorRow::Option(option) => {
                    lines.push(render_option(
                        state,
                        *row_index,
                        option,
                        hint_label_width,
                        options.decorated,
                    ));
                    body_rows += 1;
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
    format!("{}\n", lines.join("\n"))
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

            match state.apply_input(input) {
                SelectorTransition::Continue => {
                    self.draw(&render_selector(state, render_options))?
                }
                SelectorTransition::Submitted(submission) => return Ok(submission),
                SelectorTransition::Cancelled => {
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
        self.rendered_lines = rendered.lines().count().try_into().unwrap_or(u16::MAX);
        Ok(())
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

fn render_section(section: &SelectorSection, decorated: bool) -> String {
    let title = styled(&section.title, section_style(), decorated);
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
) -> String {
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
    let label = styled(
        &option.label,
        option_label_style(active, selected, option.disabled),
        decorated,
    );
    let metadata = metadata_text(option.hint.as_deref(), option.disabled);
    let hint = if metadata.is_empty() {
        String::new()
    } else {
        let label_width = measure_text_width(&option.label);
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

    format!(
        "{} {} {}  {}{}",
        styled(BAR, bar_style(), decorated),
        cursor,
        marker,
        label,
        hint
    )
}

fn hint_label_width(state: &SelectorState, window: &SelectorWindow) -> Option<usize> {
    window
        .row_indices
        .iter()
        .filter_map(|row_index| match &state.rows[*row_index] {
            SelectorRow::Option(option)
                if !metadata_text(option.hint.as_deref(), option.disabled).is_empty() =>
            {
                Some(measure_text_width(&option.label))
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
                Some(option.label.clone())
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

fn option_label_style(active: bool, selected: bool, disabled: bool) -> Style {
    if disabled {
        inactive_style()
    } else if active {
        accent_style()
    } else if selected {
        selected_style()
    } else {
        inactive_style()
    }
}

fn metadata_text(hint: Option<&str>, disabled: bool) -> String {
    let mut parts = hint
        .into_iter()
        .flat_map(|hint| hint.split('|'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if disabled {
        parts.push("disabled".into());
    }
    parts.join(" · ")
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
│
│ ❯ ●  Fix
│   ○  Publish documentation
└
"
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
}
