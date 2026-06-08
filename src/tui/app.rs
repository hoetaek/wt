use crate::origin_action_menu::{OriginAction, OriginActionMenu};
use crate::tui::body_markup::{self, LineKind};
use crate::tui::remote_ui::PrintKind;
use crate::ui::selector::strip_terminal_sequences;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserRow {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) run_status: String,
    pub(crate) origin_label: String,
    pub(crate) next_action: String,
    pub(crate) duration: Option<String>,
    pub(crate) size: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) source: String,
    pub(crate) body: String,
    pub(crate) preview_lines: Vec<String>,
    pub(crate) menu: OriginActionMenu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserColumn {
    pub(crate) title: String,
    pub(crate) cell: BrowserCell,
    pub(crate) width: BrowserColumnWidth,
}

impl BrowserColumn {
    pub(crate) fn length(title: impl Into<String>, cell: BrowserCell, width: u16) -> Self {
        Self {
            title: title.into(),
            cell,
            width: BrowserColumnWidth::Length(width),
        }
    }

    pub(crate) fn min(title: impl Into<String>, cell: BrowserCell, width: u16) -> Self {
        Self {
            title: title.into(),
            cell,
            width: BrowserColumnWidth::Min(width),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserColumnWidth {
    Length(u16),
    Min(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserCell {
    Status,
    RunStatus,
    OriginLabel,
    Title,
    Key,
    NextAction,
    Duration,
    Size,
    Branch,
    Source,
    OriginStatus,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyInput {
    Up,
    Down,
    PageUp,
    PageDown,
    Enter,
    Esc,
    Backspace,
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    List,
    FilterInput,
    Menu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Continue,
    Quit,
    Dispatch { key: String, action: OriginAction },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PopupSpec {
    Confirm {
        prompt: String,
        default: bool,
    },
    Select {
        prompt: String,
        items: Vec<String>,
        multi: bool,
    },
    Input {
        prompt: String,
        default: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PopupOutcome {
    Bool(bool),
    Index(usize),
    Indices(Vec<usize>),
    Text(String),
    Cancelled,
}

#[derive(Debug, Clone)]
enum PopupState {
    Confirm {
        prompt: String,
        selected: bool,
    },
    Select {
        prompt: String,
        items: Vec<String>,
        multi: bool,
        cursor: usize,
        selected: Vec<bool>,
    },
    Input {
        prompt: String,
        buffer: String,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PopupView<'a> {
    Confirm {
        prompt: &'a str,
        selected: bool,
    },
    Select {
        prompt: &'a str,
        items: &'a [String],
        multi: bool,
        cursor: usize,
        selected: &'a [bool],
    },
    Input {
        prompt: &'a str,
        buffer: &'a str,
    },
}

#[derive(Debug, Clone, Default)]
struct OutputPanel {
    lines: Vec<(PrintKind, String)>,
    scroll: usize,
}

#[derive(Debug, Clone)]
struct RunningAction {
    key: String,
    progress_label: String,
    spinner_frame: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    rows: Vec<BrowserRow>,
    diagnostics: Vec<String>,
    filter: String,
    selected_index: usize,
    menu_selected_index: usize,
    mode: Mode,
    status_line: String,
    copy: BrowserCopy,
    popup: Option<PopupState>,
    output: OutputPanel,
    running: Option<RunningAction>,
    columns: Vec<BrowserColumn>,
    body_view_open: bool,
    body_scroll: usize,
}

#[derive(Debug, Clone, Copy)]
struct BrowserCopy {
    empty_origin_summary: &'static str,
    origin_count_label: &'static str,
    inventory_title: &'static str,
    empty_inventory_message: &'static str,
    no_selection_message: &'static str,
    no_selection_status: &'static str,
    default_status_line: &'static str,
}

impl BrowserCopy {
    const TASK: Self = Self {
        empty_origin_summary: "no actionable tasks",
        origin_count_label: "actionable tasks",
        inventory_title: "Tasks",
        empty_inventory_message: "No actionable tasks",
        no_selection_message: "No task selected",
        no_selection_status: "no task selected",
        default_status_line: "j/k move  / filter  v body  a archive  Enter actions  q quit",
    };

    const WORKFLOW: Self = Self {
        empty_origin_summary: "no saved workflows",
        origin_count_label: "saved workflows",
        inventory_title: "Workflows",
        empty_inventory_message: "No saved workflows",
        no_selection_message: "No workflow selected",
        no_selection_status: "no workflow selected",
        default_status_line: "j/k move  / filter  v body  Enter actions  q quit",
    };
}

impl AppState {
    #[cfg(test)]
    pub(crate) fn new(rows: Vec<BrowserRow>) -> Self {
        Self::with_diagnostics(rows, Vec::new())
    }

    #[cfg(test)]
    pub(crate) fn with_diagnostics(rows: Vec<BrowserRow>, diagnostics: Vec<String>) -> Self {
        Self::with_copy(rows, diagnostics, BrowserCopy::TASK, default_task_columns())
    }

    pub(crate) fn task_with_columns(
        rows: Vec<BrowserRow>,
        diagnostics: Vec<String>,
        columns: Vec<BrowserColumn>,
    ) -> Self {
        Self::with_copy(rows, diagnostics, BrowserCopy::TASK, columns)
    }

    pub(crate) fn workflow_with_diagnostics(
        rows: Vec<BrowserRow>,
        diagnostics: Vec<String>,
    ) -> Self {
        Self::with_copy(rows, diagnostics, BrowserCopy::WORKFLOW, workflow_columns())
    }

    fn with_copy(
        rows: Vec<BrowserRow>,
        diagnostics: Vec<String>,
        copy: BrowserCopy,
        columns: Vec<BrowserColumn>,
    ) -> Self {
        Self {
            rows,
            diagnostics,
            filter: String::new(),
            selected_index: 0,
            menu_selected_index: 0,
            mode: Mode::List,
            status_line: copy.default_status_line.into(),
            copy,
            popup: None,
            output: OutputPanel::default(),
            running: None,
            columns,
            body_view_open: false,
            body_scroll: 0,
        }
    }

    pub(crate) fn handle(&mut self, key: KeyInput) -> Outcome {
        if self.body_view_open {
            return self.handle_body_view_key(key);
        }

        match self.mode {
            Mode::List => self.handle_list_key(key),
            Mode::FilterInput => self.handle_filter_key(key),
            Mode::Menu => self.handle_menu_key(key),
        }
    }

    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn status_line(&self) -> &str {
        &self.status_line
    }

    pub(crate) fn open_popup(&mut self, spec: PopupSpec) {
        self.popup = Some(match spec {
            PopupSpec::Confirm { prompt, default } => PopupState::Confirm {
                prompt,
                selected: default,
            },
            PopupSpec::Select {
                prompt,
                items,
                multi,
            } => {
                let selected = vec![false; items.len()];
                PopupState::Select {
                    prompt,
                    items,
                    multi,
                    cursor: 0,
                    selected,
                }
            }
            PopupSpec::Input { prompt, default } => PopupState::Input {
                prompt,
                buffer: default.unwrap_or_default(),
            },
        });
    }

    pub(crate) fn has_popup(&self) -> bool {
        self.popup.is_some()
    }

    pub(crate) fn popup_view(&self) -> Option<PopupView<'_>> {
        match self.popup.as_ref()? {
            PopupState::Confirm { prompt, selected } => Some(PopupView::Confirm {
                prompt,
                selected: *selected,
            }),
            PopupState::Select {
                prompt,
                items,
                multi,
                cursor,
                selected,
            } => Some(PopupView::Select {
                prompt,
                items,
                multi: *multi,
                cursor: *cursor,
                selected,
            }),
            PopupState::Input { prompt, buffer } => Some(PopupView::Input { prompt, buffer }),
        }
    }

    pub(crate) fn handle_popup_key(&mut self, key: KeyInput) -> Option<PopupOutcome> {
        let outcome = match self.popup.as_mut()? {
            PopupState::Confirm { selected, .. } => handle_confirm_popup_key(selected, key),
            PopupState::Select {
                items,
                multi,
                cursor,
                selected,
                ..
            } => handle_select_popup_key(items, *multi, cursor, selected, key),
            PopupState::Input { buffer, .. } => handle_input_popup_key(buffer, key),
        };
        if outcome.is_some() {
            self.popup = None;
        }
        outcome
    }

    pub(crate) fn push_output(&mut self, kind: PrintKind, line: String) {
        self.output.lines.push((kind, line));
        self.output.scroll = self
            .output
            .scroll
            .min(self.output.lines.len().saturating_sub(1));
    }

    pub(crate) fn output_lines(&self) -> &[(PrintKind, String)] {
        &self.output.lines
    }

    pub(crate) fn output_scroll(&self) -> usize {
        self.output.scroll
    }

    pub(crate) fn body_view_open(&self) -> bool {
        self.body_view_open
    }

    pub(crate) fn body_scroll(&self) -> usize {
        self.body_scroll
    }

    pub(crate) fn body_lines(&self) -> Vec<(LineKind, String)> {
        self.selected_row()
            .map(|row| body_markup::markup_body(&sanitize_body_for_markup(&row.body)))
            .unwrap_or_default()
    }

    pub(crate) fn begin_action(&mut self, key: &str, verb: &str) {
        if self.running.is_some() {
            return;
        }
        let progress_label = progress_label_for(verb);
        self.running = Some(RunningAction {
            key: key.to_string(),
            progress_label: progress_label.clone(),
            spinner_frame: 0,
        });
        self.status_line = format!("{progress_label} {key}...");
    }

    pub(crate) fn action_in_flight(&self) -> bool {
        self.running.is_some()
    }

    pub(crate) fn spinner_frame(&self) -> Option<usize> {
        self.running.as_ref().map(|running| running.spinner_frame)
    }

    pub(crate) fn running_status_line(&self) -> Option<String> {
        self.running
            .as_ref()
            .map(|running| format!("{} {}...", running.progress_label, running.key))
    }

    pub(crate) fn tick_spinner(&mut self) {
        if let Some(running) = self.running.as_mut() {
            running.spinner_frame = running.spinner_frame.saturating_add(1);
        }
    }

    pub(crate) fn finish_action(&mut self, status: String) {
        self.running = None;
        self.popup = None;
        self.status_line = status;
    }

    pub(crate) fn filter(&self) -> &str {
        &self.filter
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub(crate) fn empty_origin_summary(&self) -> &str {
        self.copy.empty_origin_summary
    }

    pub(crate) fn origin_count_label(&self) -> &str {
        self.copy.origin_count_label
    }

    pub(crate) fn inventory_title(&self) -> &str {
        self.copy.inventory_title
    }

    pub(crate) fn empty_inventory_message(&self) -> &str {
        self.copy.empty_inventory_message
    }

    pub(crate) fn no_selection_message(&self) -> &str {
        self.copy.no_selection_message
    }

    #[cfg(test)]
    pub(crate) fn selected_key(&self) -> Option<&str> {
        self.selected_row().map(|row| row.key.as_str())
    }

    pub(crate) fn selected_row(&self) -> Option<&BrowserRow> {
        self.visible_rows().get(self.selected_index).copied()
    }

    #[cfg(test)]
    pub(crate) fn visible_keys(&self) -> Vec<&str> {
        self.visible_rows()
            .iter()
            .map(|row| row.key.as_str())
            .collect()
    }

    pub(crate) fn visible_rows(&self) -> Vec<&BrowserRow> {
        self.rows
            .iter()
            .filter(|row| row_matches_filter(row, &self.filter))
            .collect()
    }

    pub(crate) fn columns(&self) -> &[BrowserColumn] {
        &self.columns
    }

    pub(crate) fn selected_visible_index(&self) -> Option<usize> {
        if self.visible_rows().is_empty() {
            None
        } else {
            Some(self.selected_index)
        }
    }

    pub(crate) fn selected_menu_index(&self) -> usize {
        self.menu_selected_index
    }

    #[cfg(test)]
    pub(crate) fn menu_selection_is_disabled(&self) -> bool {
        self.selected_menu_item()
            .is_some_and(|item| !item.is_enabled())
    }

    /// Show a status-line dispatch result: close the action menu and put the
    /// one-line message in the status line. No row refresh — status-line
    /// actions do not change on-disk state.
    pub(crate) fn show_dispatch_message(&mut self, message: String) {
        self.mode = Mode::List;
        self.status_line = message;
    }

    pub(crate) fn replace_rows_preserving_selection(
        &mut self,
        rows: Vec<BrowserRow>,
        diagnostics: Vec<String>,
        preferred_key: &str,
    ) {
        self.rows = rows;
        self.diagnostics = diagnostics;
        self.mode = Mode::List;
        self.status_line = self.copy.default_status_line.into();
        self.selected_index = self
            .visible_rows()
            .iter()
            .position(|row| row.key == preferred_key)
            .unwrap_or(self.selected_index);
        self.clamp_selection();
        self.select_first_enabled_menu_item();
        self.clamp_body_scroll();
    }

    pub(crate) fn origin_status_counts(&self) -> Vec<(&str, usize)> {
        let mut counts = Vec::<(&str, usize)>::new();
        for row in &self.rows {
            if let Some((_, count)) = counts
                .iter_mut()
                .find(|(status, _)| *status == row.status.as_str())
            {
                *count += 1;
            } else {
                counts.push((row.status.as_str(), 1));
            }
        }
        counts
    }

    fn handle_list_key(&mut self, key: KeyInput) -> Outcome {
        match key {
            KeyInput::Down | KeyInput::Char('j') => self.move_down(),
            KeyInput::Up | KeyInput::Char('k') => self.move_up(),
            KeyInput::Char('v') => self.open_body_view(),
            KeyInput::Char('/') => {
                self.mode = Mode::FilterInput;
                self.status_line = "type to filter; Esc clears filter".into();
            }
            KeyInput::Enter => {
                self.open_menu();
            }
            KeyInput::Char('q') => return Outcome::Quit,
            KeyInput::Char(ch) => {
                if let Some((key, action)) = self.shortcut_dispatch(ch) {
                    return Outcome::Dispatch { key, action };
                }
            }
            KeyInput::PageUp | KeyInput::PageDown | KeyInput::Esc | KeyInput::Backspace => {}
        }
        Outcome::Continue
    }

    fn handle_filter_key(&mut self, key: KeyInput) -> Outcome {
        match key {
            KeyInput::Esc => {
                self.filter.clear();
                self.mode = Mode::List;
                self.status_line = self.copy.default_status_line.into();
                self.clamp_selection();
            }
            KeyInput::Enter => {
                self.mode = Mode::List;
                self.status_line = self.copy.default_status_line.into();
            }
            KeyInput::Down => self.move_down(),
            KeyInput::Up => self.move_up(),
            KeyInput::Backspace => {
                self.filter.pop();
                self.clamp_selection();
            }
            KeyInput::Char(ch) => {
                self.filter.push(ch);
                self.clamp_selection();
            }
            KeyInput::PageUp | KeyInput::PageDown => {}
        }
        Outcome::Continue
    }

    fn handle_menu_key(&mut self, key: KeyInput) -> Outcome {
        match key {
            KeyInput::Esc => {
                self.mode = Mode::List;
                self.status_line = self.copy.default_status_line.into();
            }
            KeyInput::Down | KeyInput::Char('j') => self.move_menu_down(),
            KeyInput::Up | KeyInput::Char('k') => self.move_menu_up(),
            KeyInput::Enter => return self.menu_enter(),
            KeyInput::Char('q') => return Outcome::Quit,
            KeyInput::PageUp | KeyInput::PageDown | KeyInput::Char(_) | KeyInput::Backspace => {}
        }
        Outcome::Continue
    }

    fn handle_body_view_key(&mut self, key: KeyInput) -> Outcome {
        match key {
            KeyInput::Esc | KeyInput::Char('v') => self.close_body_view(),
            KeyInput::Down | KeyInput::Char('j') => self.scroll_body_down(1),
            KeyInput::Up | KeyInput::Char('k') => self.scroll_body_up(1),
            KeyInput::PageDown => self.scroll_body_down(10),
            KeyInput::PageUp => self.scroll_body_up(10),
            KeyInput::Enter | KeyInput::Backspace | KeyInput::Char(_) => {}
        }
        Outcome::Continue
    }

    fn open_body_view(&mut self) {
        self.body_view_open = true;
        self.body_scroll = 0;
        self.status_line = "j/k scroll  PgUp/PgDn page  v/Esc close".into();
    }

    fn close_body_view(&mut self) {
        self.body_view_open = false;
        self.status_line = self.copy.default_status_line.into();
    }

    fn scroll_body_down(&mut self, amount: usize) {
        if !self.body_lines().is_empty() {
            self.body_scroll = self.body_scroll.saturating_add(amount);
        }
    }

    fn scroll_body_up(&mut self, amount: usize) {
        self.body_scroll = self.body_scroll.saturating_sub(amount);
    }

    fn clamp_body_scroll(&mut self) {
        if self.body_lines().is_empty() {
            self.body_scroll = 0;
        }
    }

    pub(crate) fn clamp_body_scroll_to(&mut self, max_scroll: usize) {
        self.body_scroll = self.body_scroll.min(max_scroll);
    }

    fn move_down(&mut self) {
        let len = self.visible_rows().len();
        if len > 0 {
            self.selected_index = (self.selected_index + 1).min(len - 1);
        }
    }

    fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
    }

    fn clamp_selection(&mut self) {
        let len = self.visible_rows().len();
        if len == 0 {
            self.selected_index = 0;
        } else {
            self.selected_index = self.selected_index.min(len - 1);
        }
    }

    fn open_menu(&mut self) {
        if self.selected_row().is_some() {
            self.mode = Mode::Menu;
            self.select_first_enabled_menu_item();
            self.status_line = "Enter run  Esc back".into();
        } else {
            self.status_line = self.copy.no_selection_status.into();
        }
    }

    fn select_first_enabled_menu_item(&mut self) {
        self.menu_selected_index = self
            .selected_row()
            .and_then(|row| row.menu.first_enabled_index())
            .unwrap_or(0);
    }

    #[cfg(test)]
    fn selected_menu_item(&self) -> Option<&crate::origin_action_menu::OriginActionItem> {
        self.selected_row()
            .and_then(|row| row.menu.item(self.menu_selected_index))
    }

    fn move_menu_down(&mut self) {
        if let Some(row) = self.selected_row() {
            let last = row.menu.items().len().saturating_sub(1);
            self.menu_selected_index = (self.menu_selected_index + 1).min(last);
        }
    }

    fn move_menu_up(&mut self) {
        self.menu_selected_index = self.menu_selected_index.saturating_sub(1);
    }

    fn menu_enter(&mut self) -> Outcome {
        let Some(row) = self.selected_row() else {
            self.mode = Mode::List;
            self.status_line = self.copy.no_selection_status.into();
            return Outcome::Continue;
        };
        let key = row.key.clone();
        let Some(item) = row.menu.item(self.menu_selected_index) else {
            self.status_line = "action unavailable".into();
            return Outcome::Continue;
        };
        if item.is_enabled() {
            return Outcome::Dispatch {
                key,
                action: item.action(),
            };
        }
        self.status_line = item
            .disabled_reason()
            .unwrap_or("action unavailable")
            .to_string();
        Outcome::Continue
    }

    fn shortcut_dispatch(&self, ch: char) -> Option<(String, OriginAction)> {
        let row = self.selected_row()?;
        let shortcut = ch.to_string();
        row.menu
            .action_for_shortcut(&shortcut)
            .map(|action| (row.key.clone(), action))
    }
}

fn handle_confirm_popup_key(selected: &mut bool, key: KeyInput) -> Option<PopupOutcome> {
    match key {
        KeyInput::Enter => Some(PopupOutcome::Bool(*selected)),
        KeyInput::Esc => Some(PopupOutcome::Cancelled),
        KeyInput::Char('y') | KeyInput::Char('Y') => {
            *selected = true;
            None
        }
        KeyInput::Char('n') | KeyInput::Char('N') => {
            *selected = false;
            None
        }
        KeyInput::Up
        | KeyInput::Down
        | KeyInput::PageUp
        | KeyInput::PageDown
        | KeyInput::Backspace
        | KeyInput::Char(_) => None,
    }
}

fn handle_select_popup_key(
    items: &[String],
    multi: bool,
    cursor: &mut usize,
    selected: &mut [bool],
    key: KeyInput,
) -> Option<PopupOutcome> {
    match key {
        KeyInput::Enter if multi => Some(PopupOutcome::Indices(
            selected
                .iter()
                .enumerate()
                .filter_map(|(index, selected)| selected.then_some(index))
                .collect(),
        )),
        KeyInput::Enter => Some(PopupOutcome::Index(*cursor)),
        KeyInput::Esc => Some(PopupOutcome::Cancelled),
        KeyInput::Down | KeyInput::Char('j') => {
            if !items.is_empty() {
                *cursor = (*cursor + 1).min(items.len() - 1);
            }
            None
        }
        KeyInput::Up | KeyInput::Char('k') => {
            *cursor = cursor.saturating_sub(1);
            None
        }
        KeyInput::Char(' ') if multi => {
            if let Some(selected) = selected.get_mut(*cursor) {
                *selected = !*selected;
            }
            None
        }
        KeyInput::PageUp | KeyInput::PageDown | KeyInput::Backspace | KeyInput::Char(_) => None,
    }
}

fn handle_input_popup_key(buffer: &mut String, key: KeyInput) -> Option<PopupOutcome> {
    match key {
        KeyInput::Enter => Some(PopupOutcome::Text(buffer.clone())),
        KeyInput::Esc => Some(PopupOutcome::Cancelled),
        KeyInput::Backspace => {
            buffer.pop();
            None
        }
        KeyInput::Char(ch) => {
            buffer.push(ch);
            None
        }
        KeyInput::Up | KeyInput::Down | KeyInput::PageUp | KeyInput::PageDown => None,
    }
}

fn progress_label_for(verb: &str) -> String {
    match verb {
        "archive" => "archiving".into(),
        _ => format!("{verb}ing"),
    }
}

fn sanitize_body_for_markup(body: &str) -> String {
    strip_terminal_sequences(body).replace('\t', " ")
}

fn row_matches_filter(row: &BrowserRow, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }

    let needle = filter.to_lowercase();
    row.key.to_lowercase().contains(&needle)
        || row.title.to_lowercase().contains(&needle)
        || row.run_status.to_lowercase().contains(&needle)
        || row.origin_label.to_lowercase().contains(&needle)
        || row.next_action.to_lowercase().contains(&needle)
        || row
            .duration
            .as_deref()
            .is_some_and(|duration| duration.to_lowercase().contains(&needle))
        || row
            .size
            .as_deref()
            .is_some_and(|size| size.to_lowercase().contains(&needle))
        || row
            .branch
            .as_deref()
            .is_some_and(|branch| branch.to_lowercase().contains(&needle))
}

#[cfg(test)]
fn default_task_columns() -> Vec<BrowserColumn> {
    vec![
        BrowserColumn::length("status", BrowserCell::Status, 10),
        BrowserColumn::length("origin", BrowserCell::OriginLabel, 18),
        BrowserColumn::min("title", BrowserCell::Title, 18),
        BrowserColumn::length("task", BrowserCell::Key, 24),
        BrowserColumn::length("next", BrowserCell::NextAction, 8),
    ]
}

fn workflow_columns() -> Vec<BrowserColumn> {
    vec![
        BrowserColumn::length("status", BrowserCell::Status, 10),
        BrowserColumn::length("origin", BrowserCell::OriginLabel, 18),
        BrowserColumn::min("title", BrowserCell::Title, 18),
        BrowserColumn::length("workflow", BrowserCell::Key, 24),
        BrowserColumn::length("next", BrowserCell::NextAction, 8),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origin_action_menu::{OriginAction, OriginActionMenu, OriginLabel};

    fn row(key: &str, title: &str, status: &str) -> BrowserRow {
        let menu = if status == "local" {
            OriginActionMenu::for_local_task(key, title)
        } else {
            OriginActionMenu::for_origin_task(key, title, OriginLabel::new("linear", "WT-142"))
        };
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
            preview_lines: vec![format!("Origin      Linear WT-142")],
            menu,
        }
    }

    fn app() -> AppState {
        AppState::new(vec![
            row("origin-sync-tui", "Origin sync TUI", "conflict"),
            row("workflow-docs", "Workflow origin docs", "stale"),
            row("scratch-clean", "Scratch cleanup", "local"),
        ])
    }

    fn app_with_body(body: &str) -> AppState {
        let mut browser_row = row("origin-sync-tui", "Origin sync TUI", "conflict");
        browser_row.body = body.into();
        AppState::new(vec![browser_row])
    }

    #[test]
    fn popup_owns_keys_and_esc_cancels() {
        let mut app = app();
        app.open_popup(PopupSpec::Confirm {
            prompt: "Push?".into(),
            default: false,
        });
        assert!(app.has_popup());
        let outcome = app.handle_popup_key(KeyInput::Esc);
        assert_eq!(outcome, Some(PopupOutcome::Cancelled));
        assert!(!app.has_popup());
    }

    #[test]
    fn confirm_popup_enter_returns_selected_bool() {
        let mut app = app();
        app.open_popup(PopupSpec::Confirm {
            prompt: "Push?".into(),
            default: false,
        });
        app.handle_popup_key(KeyInput::Char('y'));
        let outcome = app.handle_popup_key(KeyInput::Enter);
        assert_eq!(outcome, Some(PopupOutcome::Bool(true)));
    }

    #[test]
    fn multi_select_popup_space_toggles_and_enter_confirms() {
        let mut app = app();
        app.open_popup(PopupSpec::Select {
            prompt: "Pull fields".into(),
            items: vec!["title".into(), "body".into()],
            multi: true,
        });
        app.handle_popup_key(KeyInput::Char(' '));
        app.handle_popup_key(KeyInput::Down);
        app.handle_popup_key(KeyInput::Char(' '));
        let outcome = app.handle_popup_key(KeyInput::Enter);
        assert_eq!(outcome, Some(PopupOutcome::Indices(vec![0, 1])));
    }

    #[test]
    fn select_popup_accepts_vim_j_k_like_list_and_menu_modes() {
        let mut app = app();
        app.open_popup(PopupSpec::Select {
            prompt: "Pull fields".into(),
            items: vec!["title".into(), "body".into()],
            multi: true,
        });
        // j moves the cursor down, then toggle there; k moves back up, toggle.
        app.handle_popup_key(KeyInput::Char('j'));
        app.handle_popup_key(KeyInput::Char(' '));
        app.handle_popup_key(KeyInput::Char('k'));
        app.handle_popup_key(KeyInput::Char(' '));
        let outcome = app.handle_popup_key(KeyInput::Enter);
        assert_eq!(outcome, Some(PopupOutcome::Indices(vec![0, 1])));
    }

    #[test]
    fn input_popup_collects_typed_text() {
        let mut app = app();
        app.open_popup(PopupSpec::Input {
            prompt: "Issue id to attach".into(),
            default: None,
        });
        for ch in "WT-7".chars() {
            app.handle_popup_key(KeyInput::Char(ch));
        }
        app.handle_popup_key(KeyInput::Backspace);
        let outcome = app.handle_popup_key(KeyInput::Enter);
        assert_eq!(outcome, Some(PopupOutcome::Text("WT-".into())));
    }

    #[test]
    fn output_panel_accumulates() {
        let mut app = app();
        assert!(app.output_lines().is_empty());
        for i in 0..50 {
            app.push_output(PrintKind::Plain, format!("line {i}"));
        }
        assert_eq!(app.output_lines().len(), 50);
    }

    #[test]
    fn running_action_sets_spinner_status_and_blocks_second_dispatch() {
        let mut app = app();
        app.begin_action("origin-sync-tui", "pull");
        assert!(app.action_in_flight());
        assert!(app.status_line().contains("pull"));
        app.tick_spinner();
        app.push_output(PrintKind::Plain, "Pull preview - WT-142".into());
        app.finish_action("pulled origin-sync-tui".into());
        assert!(!app.action_in_flight());
        assert_eq!(app.status_line(), "pulled origin-sync-tui");
        assert_eq!(app.output_lines().len(), 1);
    }

    #[test]
    fn archive_running_action_uses_archiving_label() {
        let mut app = app();

        app.begin_action("origin-sync-tui", "archive");

        assert_eq!(app.status_line(), "archiving origin-sync-tui...");
        assert_eq!(
            app.running_status_line(),
            Some("archiving origin-sync-tui...".into())
        );
        assert!(!app.status_line().contains("archiveing"));
    }

    #[test]
    fn v_key_toggles_body_view_and_esc_closes() {
        let mut app = app();
        assert!(!app.body_view_open());
        app.handle(KeyInput::Char('v'));
        assert!(app.body_view_open());
        app.handle(KeyInput::Esc);
        assert!(!app.body_view_open());
    }

    #[test]
    fn body_view_scrolls_and_blocks_dispatch() {
        let mut app = app_with_body("one\ntwo\nthree\nfour");
        app.handle(KeyInput::Char('v'));
        app.handle(KeyInput::Char('j'));
        assert_eq!(app.body_scroll(), 1);

        let outcome = app.handle(KeyInput::Enter);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.handle(KeyInput::Char('d')), Outcome::Continue);
    }

    #[test]
    fn a_key_requests_archive_dispatch_for_selected() {
        let mut app = app();

        assert_eq!(
            app.handle(KeyInput::Char('a')),
            Outcome::Dispatch {
                key: "origin-sync-tui".into(),
                action: OriginAction::Archive
            }
        );
    }

    #[test]
    fn a_key_ignored_during_body_view() {
        let mut app = app();
        app.handle(KeyInput::Char('v'));

        assert_eq!(app.handle(KeyInput::Char('a')), Outcome::Continue);
    }

    #[test]
    fn task_status_line_mentions_archive_shortcut() {
        let app = app();

        assert!(app.status_line().contains("a archive"));
    }

    #[test]
    fn workflow_status_line_omits_archive_shortcut() {
        let app = AppState::workflow_with_diagnostics(Vec::new(), Vec::new());

        assert!(!app.status_line().contains("a archive"));
    }

    #[test]
    fn body_view_page_keys_scroll_by_larger_steps() {
        let body = (0..20)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut app = app_with_body(&body);
        app.handle(KeyInput::Char('v'));
        app.handle(KeyInput::PageDown);
        assert_eq!(app.body_scroll(), 10);
        app.handle(KeyInput::PageUp);
        assert_eq!(app.body_scroll(), 0);
    }

    #[test]
    fn down_and_up_move_selection_within_bounds() {
        let mut app = app();
        assert_eq!(app.selected_key(), Some("origin-sync-tui"));
        app.handle(KeyInput::Down);
        app.handle(KeyInput::Down);
        assert_eq!(app.selected_key(), Some("scratch-clean"));
        app.handle(KeyInput::Down);
        assert_eq!(app.selected_key(), Some("scratch-clean"));
        app.handle(KeyInput::Up);
        assert_eq!(app.selected_key(), Some("workflow-docs"));
    }

    #[test]
    fn filter_mode_narrows_rows_and_esc_restores() {
        let mut app = app();
        app.handle(KeyInput::Char('/'));
        assert_eq!(app.mode(), Mode::FilterInput);
        app.handle(KeyInput::Char('w'));
        app.handle(KeyInput::Char('o'));
        assert_eq!(app.visible_keys(), vec!["workflow-docs"]);
        app.handle(KeyInput::Esc);
        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.visible_keys().len(), 3);
    }

    #[test]
    fn filter_backspace_deletes_last_char_and_rewidens_rows() {
        let mut app = app();
        app.handle(KeyInput::Char('/'));
        app.handle(KeyInput::Char('w'));
        app.handle(KeyInput::Char('o'));
        assert_eq!(app.visible_keys(), vec!["workflow-docs"]);
        app.handle(KeyInput::Backspace);
        assert_eq!(app.filter(), "w");
        assert_eq!(
            app.visible_keys().len(),
            3,
            "w는 origin_label(WT-142)로 모든 행을 다시 보여준다"
        );
        app.handle(KeyInput::Backspace);
        assert_eq!(app.filter(), "");
        assert_eq!(app.visible_keys().len(), 3);
        app.handle(KeyInput::Backspace);
        assert_eq!(app.filter(), "", "빈 filter에서 backspace는 no-op");
    }

    #[test]
    fn show_dispatch_message_closes_menu_and_fills_status_line() {
        let mut app = app();
        app.handle(KeyInput::Enter);
        assert_eq!(app.mode(), Mode::Menu);

        app.show_dispatch_message("Copied reference linear:WT-142".into());

        assert_eq!(app.mode(), Mode::List);
        assert_eq!(app.status_line(), "Copied reference linear:WT-142");
    }

    #[test]
    fn q_requests_quit_only_in_list_mode() {
        let mut app = app();
        app.handle(KeyInput::Char('/'));
        assert_eq!(app.handle(KeyInput::Char('q')), Outcome::Continue);
        app.handle(KeyInput::Esc);
        assert_eq!(app.handle(KeyInput::Char('q')), Outcome::Quit);
    }

    #[test]
    fn enter_shows_menu_status_hint() {
        let mut app = app();
        assert_eq!(app.handle(KeyInput::Enter), Outcome::Continue);
        assert!(app.status_line().contains("Enter run"));
    }

    #[test]
    fn enter_opens_menu_and_esc_returns_to_list() {
        let mut app = app();
        assert_eq!(app.handle(KeyInput::Enter), Outcome::Continue);
        assert_eq!(app.mode(), Mode::Menu);
        app.handle(KeyInput::Esc);
        assert_eq!(app.mode(), Mode::List);
    }

    #[test]
    fn menu_enter_on_enabled_item_requests_dispatch() {
        let mut app = app();
        app.handle(KeyInput::Enter);
        let outcome = app.handle(KeyInput::Enter);
        assert_eq!(
            outcome,
            Outcome::Dispatch {
                key: "origin-sync-tui".into(),
                action: OriginAction::Diff
            }
        );
    }

    #[test]
    fn menu_enter_on_archive_item_requests_dispatch() {
        let mut app = app();
        app.handle(KeyInput::Enter);
        let item_count = app.selected_row().unwrap().menu.items().len();

        for _ in 0..item_count {
            if app
                .selected_menu_item()
                .is_some_and(|item| item.action() == OriginAction::Archive)
            {
                assert_eq!(
                    app.handle(KeyInput::Enter),
                    Outcome::Dispatch {
                        key: "origin-sync-tui".into(),
                        action: OriginAction::Archive
                    }
                );
                return;
            }
            app.handle(KeyInput::Down);
        }

        panic!("archive item not found");
    }

    #[test]
    fn menu_enter_on_disabled_item_shows_reason_and_stays() {
        let mut app = app();
        app.handle(KeyInput::Enter);
        let item_count = app.selected_row().unwrap().menu.items().len();
        for _ in 0..item_count {
            if app.menu_selection_is_disabled() {
                app.handle(KeyInput::Enter);
                if app.status_line().contains("already has origin") {
                    break;
                }
            }
            app.handle(KeyInput::Down);
        }
        assert_eq!(app.mode(), Mode::Menu);
        assert!(app.status_line().contains("already has origin"));
    }

    #[test]
    fn list_shortcut_dispatches_enabled_action_directly() {
        let mut app = app();
        assert_eq!(
            app.handle(KeyInput::Char('d')),
            Outcome::Dispatch {
                key: "origin-sync-tui".into(),
                action: OriginAction::Diff
            }
        );
        app.handle(KeyInput::Down);
        app.handle(KeyInput::Down);
        assert_eq!(app.handle(KeyInput::Char('P')), Outcome::Continue);
    }

    #[test]
    fn empty_inventory_renders_empty_state() {
        let app = AppState::new(vec![]);
        assert!(app.is_empty());
        assert_eq!(app.selected_key(), None);
    }
}
