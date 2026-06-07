#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserRow {
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) origin_label: String,
    pub(crate) next_action: String,
    pub(crate) preview_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyInput {
    Up,
    Down,
    Enter,
    Esc,
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    List,
    FilterInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Continue,
    Quit,
}

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    rows: Vec<BrowserRow>,
    diagnostics: Vec<String>,
    filter: String,
    selected_index: usize,
    mode: Mode,
    status_line: String,
}

impl AppState {
    #[cfg(test)]
    pub(crate) fn new(rows: Vec<BrowserRow>) -> Self {
        Self::with_diagnostics(rows, Vec::new())
    }

    pub(crate) fn with_diagnostics(rows: Vec<BrowserRow>, diagnostics: Vec<String>) -> Self {
        Self {
            rows,
            diagnostics,
            filter: String::new(),
            selected_index: 0,
            mode: Mode::List,
            status_line: default_status_line(),
        }
    }

    pub(crate) fn handle(&mut self, key: KeyInput) -> Outcome {
        match self.mode {
            Mode::List => self.handle_list_key(key),
            Mode::FilterInput => self.handle_filter_key(key),
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
                self.status_line = "actions are unavailable in this read-only browser".into();
            }
            KeyInput::Char('q') => return Outcome::Quit,
            KeyInput::Esc | KeyInput::Char(_) => {}
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
            KeyInput::Char(ch) => {
                self.filter.push(ch);
                self.clamp_selection();
            }
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

    fn row(key: &str, title: &str, status: &str) -> BrowserRow {
        BrowserRow {
            key: key.into(),
            title: title.into(),
            status: status.into(),
            origin_label: "Linear WT-142".into(),
            next_action: "diff".into(),
            preview_lines: vec![format!("Origin      Linear WT-142")],
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
    fn q_requests_quit_only_in_list_mode() {
        let mut app = app();
        app.handle(KeyInput::Char('/'));
        assert_eq!(app.handle(KeyInput::Char('q')), Outcome::Continue);
        app.handle(KeyInput::Esc);
        assert_eq!(app.handle(KeyInput::Char('q')), Outcome::Quit);
    }

    #[test]
    fn enter_is_noop_with_status_hint_in_this_slice() {
        let mut app = app();
        assert_eq!(app.handle(KeyInput::Enter), Outcome::Continue);
        assert!(app.status_line().contains("actions"));
    }

    #[test]
    fn empty_inventory_renders_empty_state() {
        let app = AppState::new(vec![]);
        assert!(app.is_empty());
        assert_eq!(app.selected_key(), None);
    }
}
