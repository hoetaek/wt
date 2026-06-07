use crate::origin_action_menu::{OriginAction, OriginActionMenu};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserRow {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) origin_label: String,
    pub(crate) next_action: String,
    pub(crate) preview_lines: Vec<String>,
    pub(crate) menu: OriginActionMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyInput {
    Up,
    Down,
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
}

#[derive(Debug, Clone, Copy)]
struct BrowserCopy {
    empty_origin_summary: &'static str,
    origin_count_label: &'static str,
    inventory_title: &'static str,
    empty_inventory_message: &'static str,
    key_column_title: &'static str,
    no_selection_message: &'static str,
    no_selection_status: &'static str,
}

impl BrowserCopy {
    const TASK: Self = Self {
        empty_origin_summary: "no actionable tasks",
        origin_count_label: "actionable tasks",
        inventory_title: "Tasks",
        empty_inventory_message: "No actionable tasks",
        key_column_title: "task",
        no_selection_message: "No task selected",
        no_selection_status: "no task selected",
    };

    const WORKFLOW: Self = Self {
        empty_origin_summary: "no saved workflows",
        origin_count_label: "saved workflows",
        inventory_title: "Workflows",
        empty_inventory_message: "No saved workflows",
        key_column_title: "workflow",
        no_selection_message: "No workflow selected",
        no_selection_status: "no workflow selected",
    };
}

impl AppState {
    #[cfg(test)]
    pub(crate) fn new(rows: Vec<BrowserRow>) -> Self {
        Self::with_diagnostics(rows, Vec::new())
    }

    pub(crate) fn with_diagnostics(rows: Vec<BrowserRow>, diagnostics: Vec<String>) -> Self {
        Self::with_copy(rows, diagnostics, BrowserCopy::TASK)
    }

    pub(crate) fn workflow_with_diagnostics(
        rows: Vec<BrowserRow>,
        diagnostics: Vec<String>,
    ) -> Self {
        Self::with_copy(rows, diagnostics, BrowserCopy::WORKFLOW)
    }

    fn with_copy(rows: Vec<BrowserRow>, diagnostics: Vec<String>, copy: BrowserCopy) -> Self {
        Self {
            rows,
            diagnostics,
            filter: String::new(),
            selected_index: 0,
            menu_selected_index: 0,
            mode: Mode::List,
            status_line: default_status_line(),
            copy,
        }
    }

    pub(crate) fn handle(&mut self, key: KeyInput) -> Outcome {
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

    pub(crate) fn key_column_title(&self) -> &str {
        self.copy.key_column_title
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
        self.status_line = default_status_line();
        self.selected_index = self
            .visible_rows()
            .iter()
            .position(|row| row.key == preferred_key)
            .unwrap_or(self.selected_index);
        self.clamp_selection();
        self.select_first_enabled_menu_item();
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
            KeyInput::Esc | KeyInput::Backspace => {}
        }
        Outcome::Continue
    }

    fn handle_filter_key(&mut self, key: KeyInput) -> Outcome {
        match key {
            KeyInput::Esc => {
                self.filter.clear();
                self.mode = Mode::List;
                self.status_line = default_status_line();
                self.clamp_selection();
            }
            KeyInput::Enter => {
                self.mode = Mode::List;
                self.status_line = default_status_line();
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
        }
        Outcome::Continue
    }

    fn handle_menu_key(&mut self, key: KeyInput) -> Outcome {
        match key {
            KeyInput::Esc => {
                self.mode = Mode::List;
                self.status_line = default_status_line();
            }
            KeyInput::Down | KeyInput::Char('j') => self.move_menu_down(),
            KeyInput::Up | KeyInput::Char('k') => self.move_menu_up(),
            KeyInput::Enter => return self.menu_enter(),
            KeyInput::Char('q') => return Outcome::Quit,
            KeyInput::Char(_) | KeyInput::Backspace => {}
        }
        Outcome::Continue
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

fn default_status_line() -> String {
    "j/k move  / filter  Enter actions  q quit".into()
}

fn row_matches_filter(row: &BrowserRow, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }

    let needle = filter.to_lowercase();
    row.key.to_lowercase().contains(&needle)
        || row.title.to_lowercase().contains(&needle)
        || row.origin_label.to_lowercase().contains(&needle)
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
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
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
    fn menu_enter_on_disabled_item_shows_reason_and_stays() {
        let mut app = app();
        app.handle(KeyInput::Enter);
        for _ in 0..8 {
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
