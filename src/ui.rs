use crate::context::{PromptItem, UserInterface};
use crate::error::WtError;
use anyhow::{Result, anyhow};
use cliclack::{Theme, ThemeState};
use console::{Style, measure_text_width, style};
use std::io::{self, IsTerminal};

const PROMPT_MAX_ROWS: usize = 10;
const PROMPT_HINT_GAP: usize = 2;
const PROMPT_SEARCH_SEPARATOR: char = '\x1f';
const BAR: &str = "│";
const RADIO_SELECTED: &str = "◉";
const RADIO_UNSELECTED: &str = "○";
const CHECKBOX_SELECTED: &str = "☑";
const CHECKBOX_UNSELECTED: &str = "☐";

pub struct TerminalUi {
    quiet: bool,
    decorated: bool,
}

impl TerminalUi {
    pub fn new(quiet: bool) -> Self {
        Self::with_decoration(quiet, true)
    }

    pub fn with_decoration(quiet: bool, decorated: bool) -> Self {
        cliclack::set_theme(WtPromptTheme);
        Self { quiet, decorated }
    }
}

impl UserInterface for TerminalUi {
    fn select(&self, prompt: &str, items: &[String]) -> Result<usize> {
        let items = items
            .iter()
            .cloned()
            .map(PromptItem::new)
            .collect::<Vec<_>>();
        self.select_items(prompt, &items)
    }

    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
        let items = items
            .iter()
            .cloned()
            .map(PromptItem::new)
            .collect::<Vec<_>>();
        self.multi_select_items(prompt, &items)
    }

    fn can_prompt(&self) -> bool {
        io::stdin().is_terminal()
    }

    fn select_items(&self, prompt: &str, items: &[PromptItem]) -> Result<usize> {
        let items = prompt_entries(items);
        let mut select = cliclack::select(prompt)
            .max_rows(PROMPT_MAX_ROWS)
            .filter_mode();
        for (index, item) in items.iter().enumerate() {
            select = select.item(index, &item.label, &item.hint);
        }
        prompt_result(prompt, select.interact())
    }

    fn multi_select_items(&self, prompt: &str, items: &[PromptItem]) -> Result<Vec<usize>> {
        let items = prompt_entries(items);
        let mut multi_select = cliclack::multiselect(prompt)
            .max_rows(PROMPT_MAX_ROWS)
            .required(false)
            .filter_mode();
        for (index, item) in items.iter().enumerate() {
            multi_select = multi_select.item(index, &item.label, &item.hint);
        }
        prompt_result(prompt, multi_select.interact())
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        let mut confirm = cliclack::confirm(prompt).initial_value(default);
        prompt_result(prompt, confirm.interact())
    }

    fn input(&self, prompt: &str, default: Option<&str>) -> Result<String> {
        let mut input = cliclack::input(prompt);
        if let Some(d) = default {
            input = input.default_input(d);
        }
        prompt_result(prompt, input.interact::<String>())
    }

    fn print_step(&self, msg: &str) {
        if self.quiet {
            return;
        }
        if self.decorated {
            println!("{} {}", style("==>").green(), msg);
        } else {
            println!("{msg}");
        }
    }

    fn print_dim(&self, msg: &str) {
        if self.quiet {
            return;
        }
        if self.decorated {
            println!("{}", style(msg).dim());
        } else {
            println!("{msg}");
        }
    }

    fn print_warning(&self, msg: &str) {
        if self.decorated {
            eprintln!("{} {}", style("WARNING:").yellow(), msg);
        } else {
            eprintln!("WARNING: {msg}");
        }
    }

    fn print_error(&self, msg: &str) {
        if self.decorated {
            eprintln!("{} {}", style("ERROR:").red(), msg);
        } else {
            eprintln!("ERROR: {msg}");
        }
    }
}

struct WtPromptTheme;

struct PromptEntry {
    label: String,
    hint: String,
}

fn prompt_entries(items: &[PromptItem]) -> Vec<PromptEntry> {
    let hint_label_width = items
        .iter()
        .filter(|item| has_prompt_hint(item))
        .map(|item| measure_text_width(&item.label))
        .max();

    items
        .iter()
        .map(|item| {
            let hint = item.hint.as_deref().unwrap_or("").trim().to_string();
            let mut label = item.label.clone();
            if !hint.is_empty() {
                if let Some(target_width) = hint_label_width {
                    label.push_str(
                        &" ".repeat(target_width.saturating_sub(measure_text_width(&label))),
                    );
                }
                // Cliclack filters only labels, so carry hint text in a suffix
                // that the theme strips before rendering.
                label.push(PROMPT_SEARCH_SEPARATOR);
                label.push_str(&hint);
            }
            PromptEntry { label, hint }
        })
        .collect()
}

fn has_prompt_hint(item: &PromptItem) -> bool {
    item.hint
        .as_deref()
        .is_some_and(|hint| !hint.trim().is_empty())
}

fn display_label(label: &str) -> &str {
    label
        .split_once(PROMPT_SEARCH_SEPARATOR)
        .map_or(label, |(display, _)| display)
}

impl Theme for WtPromptTheme {
    fn bar_color(&self, state: &ThemeState) -> Style {
        match state {
            ThemeState::Active => accent_style(),
            ThemeState::Cancel => Style::new().red(),
            ThemeState::Submit => muted_style(),
            ThemeState::Error(_) => Style::new().yellow(),
        }
    }

    fn state_symbol_color(&self, state: &ThemeState) -> Style {
        match state {
            ThemeState::Submit => selected_style(),
            _ => self.bar_color(state),
        }
    }

    fn radio_symbol(&self, state: &ThemeState, selected: bool) -> String {
        match state {
            ThemeState::Active | ThemeState::Error(_) if selected => {
                accent_style().apply_to(RADIO_SELECTED)
            }
            ThemeState::Active | ThemeState::Error(_) => muted_style().apply_to(RADIO_UNSELECTED),
            ThemeState::Submit if selected => selected_style().apply_to(RADIO_SELECTED),
            ThemeState::Cancel if selected => muted_style().apply_to(RADIO_SELECTED),
            _ => Style::new().apply_to(""),
        }
        .to_string()
    }

    fn checkbox_symbol(&self, state: &ThemeState, selected: bool, active: bool) -> String {
        match state {
            ThemeState::Active | ThemeState::Error(_) if selected => {
                selected_style().apply_to(CHECKBOX_SELECTED)
            }
            ThemeState::Active | ThemeState::Error(_) if active => {
                accent_style().apply_to(CHECKBOX_UNSELECTED)
            }
            ThemeState::Active | ThemeState::Error(_) => {
                muted_style().apply_to(CHECKBOX_UNSELECTED)
            }
            ThemeState::Submit if selected => selected_style().apply_to(CHECKBOX_SELECTED),
            ThemeState::Cancel if selected => muted_style().apply_to(CHECKBOX_SELECTED),
            _ => Style::new().apply_to(""),
        }
        .to_string()
    }

    fn checkbox_style(&self, state: &ThemeState, selected: bool, active: bool) -> Style {
        match state {
            ThemeState::Cancel if selected => Style::new().dim().strikethrough(),
            ThemeState::Submit if selected => muted_style(),
            ThemeState::Active | ThemeState::Error(_) if active => Style::new().bold(),
            ThemeState::Active | ThemeState::Error(_) if selected => Style::new(),
            _ => muted_style(),
        }
    }

    fn format_select_item(
        &self,
        state: &ThemeState,
        selected: bool,
        label: &str,
        hint: &str,
    ) -> String {
        match state {
            ThemeState::Cancel | ThemeState::Submit if !selected => return String::new(),
            _ => {}
        }

        format_prompt_row(
            state,
            self.bar_color(state),
            self.radio_symbol(state, selected),
            if selected {
                Style::new().bold()
            } else {
                muted_style()
            },
            label,
            hint,
        )
    }

    fn format_multiselect_item(
        &self,
        state: &ThemeState,
        selected: bool,
        active: bool,
        label: &str,
        hint: &str,
    ) -> String {
        match state {
            ThemeState::Cancel | ThemeState::Submit if !selected => return String::new(),
            _ => {}
        }

        format_prompt_row(
            state,
            self.bar_color(state),
            self.checkbox_symbol(state, selected, active),
            self.checkbox_style(state, selected, active),
            label,
            hint,
        )
    }
}

fn accent_style() -> Style {
    Style::new().color256(110).bold()
}

fn selected_style() -> Style {
    Style::new().color256(114).bold()
}

fn muted_style() -> Style {
    Style::new().color256(245)
}

fn hint_style(state: &ThemeState) -> Style {
    match state {
        ThemeState::Cancel => Style::new().dim().strikethrough(),
        ThemeState::Submit => Style::new().dim(),
        _ => muted_style(),
    }
}

fn format_prompt_row(
    state: &ThemeState,
    bar_style: Style,
    marker: String,
    label_style: Style,
    label: &str,
    hint: &str,
) -> String {
    let label = display_label(label);
    let hint = format_hint(state, hint);
    format!(
        "{bar}  {marker}  {label}{hint}\n",
        bar = bar_style.apply_to(BAR),
        label = label_style.apply_to(label),
    )
}

fn format_hint(state: &ThemeState, hint: &str) -> String {
    let hint = hint.trim();
    if hint.is_empty() {
        String::new()
    } else {
        let hint = hint.replace(" | ", " · ");
        format!(
            "{}{}",
            " ".repeat(PROMPT_HINT_GAP),
            hint_style(state).apply_to(hint)
        )
    }
}

fn prompt_result<T>(prompt: &str, result: io::Result<T>) -> Result<T> {
    result.map_err(|err| match err.kind() {
        io::ErrorKind::Interrupted => anyhow::Error::new(WtError::Cancelled),
        io::ErrorKind::NotConnected => anyhow!(
            "interactive prompt '{prompt}' requires a terminal; rerun in an interactive shell or pass the value explicitly"
        ),
        io::ErrorKind::InvalidInput => anyhow!("prompt '{prompt}' cannot be shown: {err}"),
        _ => anyhow!("interactive prompt '{prompt}' failed: {err}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{self, TaskDocument, TaskOrigin};
    use console::strip_ansi_codes;

    #[test]
    fn interrupted_prompt_maps_to_cancelled_error() {
        let err = prompt_result::<()>(
            "Select item",
            Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")),
        )
        .unwrap_err();

        assert!(matches!(
            err.downcast_ref::<WtError>(),
            Some(WtError::Cancelled)
        ));
    }

    #[test]
    fn non_tty_prompt_error_is_actionable() {
        let err = prompt_result::<()>(
            "Select item",
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "not a terminal",
            )),
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "interactive prompt 'Select item' requires a terminal; rerun in an interactive shell or pass the value explicitly"
        );
    }

    #[test]
    fn prompt_theme_remains_readable_without_color() {
        console::set_colors_enabled(false);
        let rendered = WtPromptTheme.format_select_item(
            &ThemeState::Active,
            true,
            "Fix editor",
            "task PROJ-123 | Linear",
        );
        console::set_colors_enabled(true);

        let plain = strip_ansi_codes(&rendered);
        assert!(plain.contains("◉  Fix editor"));
        assert!(plain.contains("task PROJ-123 · Linear"));
    }

    #[test]
    fn prompt_theme_renders_checkbox_rows_without_color() {
        console::set_colors_enabled(false);
        let rendered = WtPromptTheme.format_multiselect_item(
            &ThemeState::Active,
            true,
            true,
            "Publish docs",
            "PROJ-123 | Todo | alice",
        );
        console::set_colors_enabled(true);

        let plain = strip_ansi_codes(&rendered);
        assert!(plain.contains("☑  Publish docs"));
        assert!(plain.contains("PROJ-123 · Todo · alice"));
    }

    #[test]
    fn prompt_theme_aligns_select_hint_columns_after_stripping_ansi() {
        console::set_colors_enabled(true);
        let items = prompt_entries(&[
            PromptItem::with_hint("Fix", "task PROJ-123"),
            PromptItem::with_hint("Publish documentation", "task PROJ-456"),
        ]);
        let short = strip_ansi_codes(&WtPromptTheme.format_select_item(
            &ThemeState::Active,
            true,
            &items[0].label,
            &items[0].hint,
        ))
        .into_owned();
        let long = strip_ansi_codes(&WtPromptTheme.format_select_item(
            &ThemeState::Active,
            false,
            &items[1].label,
            &items[1].hint,
        ))
        .into_owned();
        console::set_colors_enabled(true);

        assert_eq!(
            hint_column(&short, "task PROJ-123"),
            hint_column(&long, "task PROJ-456")
        );
    }

    #[test]
    fn prompt_theme_aligns_multiselect_hint_columns_after_stripping_ansi() {
        console::set_colors_enabled(true);
        let items = prompt_entries(&[
            PromptItem::with_hint("Fix", "task PROJ-123"),
            PromptItem::with_hint("Publish documentation", "task PROJ-456"),
        ]);
        let short = strip_ansi_codes(&WtPromptTheme.format_multiselect_item(
            &ThemeState::Active,
            true,
            true,
            &items[0].label,
            &items[0].hint,
        ))
        .into_owned();
        let long = strip_ansi_codes(&WtPromptTheme.format_multiselect_item(
            &ThemeState::Active,
            false,
            false,
            &items[1].label,
            &items[1].hint,
        ))
        .into_owned();
        console::set_colors_enabled(true);

        assert_eq!(
            hint_column(&short, "task PROJ-123"),
            hint_column(&long, "task PROJ-456")
        );
    }

    #[test]
    fn task_selector_origin_state_starts_in_same_rendered_column_for_full_width_titles() {
        console::set_colors_enabled(true);
        let local = task::task_resource_item(
            "a",
            &TaskDocument {
                title: "짧은 제목".into(),
                branch: "a".into(),
                body: String::new(),
                origin: None,
            },
            "origin:none",
        );
        let provider = task::task_resource_item(
            "very-long-task-key",
            &TaskDocument {
                title: "인디위키 보호된 제목 목록 구현".into(),
                branch: "team/very-long-task-key".into(),
                body: String::new(),
                origin: Some(TaskOrigin {
                    provider: "linear".into(),
                    id: "PROJ-123".into(),
                }),
            },
            "origin:linear:PROJ-123",
        );
        let items = prompt_entries(&[local, provider]);

        let local = strip_ansi_codes(&WtPromptTheme.format_multiselect_item(
            &ThemeState::Active,
            true,
            true,
            &items[0].label,
            &items[0].hint,
        ))
        .into_owned();
        let provider = strip_ansi_codes(&WtPromptTheme.format_multiselect_item(
            &ThemeState::Active,
            false,
            false,
            &items[1].label,
            &items[1].hint,
        ))
        .into_owned();
        console::set_colors_enabled(true);

        assert_eq!(
            rendered_column(&local, "not published"),
            rendered_column(&provider, "Linear PROJ-123")
        );
        assert!(items[0].label.contains("not published | task a | branch a"));
        assert!(items[1].label.contains(
            "Linear PROJ-123 | task very-long-task-key | branch team/very-long-task-key"
        ));
    }

    #[test]
    fn prompt_entries_keep_hint_text_searchable_without_rendering_search_suffix() {
        let items = prompt_entries(&[PromptItem::with_hint(
            "Fix editor",
            "task PROJ-123 | Linear",
        )]);

        assert!(items[0].label.contains("Fix editor"));
        assert!(items[0].label.contains("task PROJ-123 | Linear"));

        let rendered = WtPromptTheme.format_select_item(
            &ThemeState::Active,
            true,
            &items[0].label,
            &items[0].hint,
        );
        let plain = strip_ansi_codes(&rendered);
        assert!(!plain.contains(PROMPT_SEARCH_SEPARATOR));
        assert!(!plain.contains("task PROJ-123 | Linear"));
        assert!(plain.contains("task PROJ-123 · Linear"));
    }

    #[test]
    fn prompt_entries_leave_plain_labels_unpadded() {
        let items = prompt_entries(&[
            PromptItem::new("Fix"),
            PromptItem::new("Publish documentation"),
        ]);

        assert_eq!(items[0].label, "Fix");
        assert_eq!(items[0].hint, "");
        assert_eq!(items[1].label, "Publish documentation");
        assert_eq!(items[1].hint, "");
    }

    fn hint_column(row: &str, hint: &str) -> usize {
        row.find(hint)
            .unwrap_or_else(|| panic!("row did not contain hint {hint:?}: {row:?}"))
    }

    fn rendered_column(row: &str, hint: &str) -> usize {
        let byte_index = hint_column(row, hint);
        measure_text_width(&row[..byte_index])
    }
}
