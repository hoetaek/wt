use crate::context::{PromptItem, PromptOption, PromptRow, UserInterface, prompt_items_to_rows};
use crate::error::WtError;
use crate::ui::selector::{
    DEFAULT_VISIBLE_OPTIONS, SelectorOption, SelectorRenderOptions, SelectorRow, SelectorSection,
    SelectorState, SelectorSubmission, run_selector_prompt,
};
use anyhow::{Result, anyhow, bail};
use console::style;
use std::io::{self, IsTerminal, Write};

pub(crate) mod selector;

const PROMPT_START: &str = "◆";

pub struct TerminalUi {
    quiet: bool,
    decorated: bool,
}

impl TerminalUi {
    pub fn new(quiet: bool) -> Self {
        Self::with_decoration(quiet, true)
    }

    pub fn with_decoration(quiet: bool, decorated: bool) -> Self {
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
        let rows = prompt_items_to_rows(items);
        self.select_rows(prompt, &rows)
    }

    fn multi_select_items(&self, prompt: &str, items: &[PromptItem]) -> Result<Vec<usize>> {
        let rows = prompt_items_to_rows(items);
        self.multi_select_rows(prompt, &rows)
    }

    fn select_rows(&self, prompt: &str, rows: &[PromptRow]) -> Result<usize> {
        ensure_selectable_rows(prompt, rows)?;
        prompt_result(prompt, require_prompt_terminal())?;

        let mut state = SelectorState::single(selector_rows(rows));
        let options = selector_options(prompt, self.decorated);
        let mut stderr = io::stderr().lock();
        match prompt_result(
            prompt,
            run_selector_prompt(&mut stderr, &mut state, &options),
        )? {
            SelectorSubmission::Single(index) => Ok(index),
            SelectorSubmission::Multi(_) => bail!("prompt '{prompt}' returned multiselect output"),
        }
    }

    fn multi_select_rows(&self, prompt: &str, rows: &[PromptRow]) -> Result<Vec<usize>> {
        ensure_option_rows(prompt, rows)?;
        prompt_result(prompt, require_prompt_terminal())?;

        let mut state = SelectorState::multi(selector_rows(rows));
        let options = selector_options(prompt, self.decorated)
            .selected_summary(should_show_selected_summary(rows));
        let mut stderr = io::stderr().lock();
        match prompt_result(
            prompt,
            run_selector_prompt(&mut stderr, &mut state, &options),
        )? {
            SelectorSubmission::Multi(indices) => Ok(indices),
            SelectorSubmission::Single(_) => bail!("prompt '{prompt}' returned select output"),
        }
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        prompt_result(prompt, run_confirm_prompt(prompt, default, self.decorated))
    }

    fn input(&self, prompt: &str, default: Option<&str>) -> Result<String> {
        prompt_result(prompt, run_input_prompt(prompt, default, self.decorated))
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

fn selector_options(prompt: &str, decorated: bool) -> SelectorRenderOptions {
    SelectorRenderOptions::new(prompt).decorated(decorated)
}

fn selector_rows(rows: &[PromptRow]) -> Vec<SelectorRow> {
    let mut option_index = 0;
    rows.iter()
        .map(|row| match row {
            PromptRow::Section(section) => match section.hint.as_deref() {
                Some(hint) => SelectorRow::Section(SelectorSection::with_hint(
                    section.title.clone(),
                    hint.to_string(),
                )),
                None => SelectorRow::Section(SelectorSection::new(section.title.clone())),
            },
            PromptRow::Option(option) => {
                let row = selector_option(option.value_index.unwrap_or(option_index), option);
                option_index += 1;
                SelectorRow::Option(row)
            }
        })
        .collect()
}

fn selector_option(index: usize, option: &PromptOption) -> SelectorOption {
    let mut row = match option.hint.as_deref() {
        Some(hint) => SelectorOption::with_hint(index, option.label.clone(), hint.to_string()),
        None => SelectorOption::new(index, option.label.clone()),
    };
    for text in &option.search_text {
        row = row.search_text(text.clone());
    }
    row.selected(option.selected).disabled(option.disabled)
}

fn should_show_selected_summary(rows: &[PromptRow]) -> bool {
    let mut options = 0;
    let mut has_section = false;
    for row in rows {
        match row {
            PromptRow::Section(_) => has_section = true,
            PromptRow::Option(_) => options += 1,
        }
    }
    has_section || options > DEFAULT_VISIBLE_OPTIONS
}

fn ensure_selectable_rows(prompt: &str, rows: &[PromptRow]) -> Result<()> {
    if rows.iter().any(|row| {
        matches!(
            row,
            PromptRow::Option(PromptOption {
                disabled: false,
                ..
            })
        )
    }) {
        Ok(())
    } else {
        bail!("prompt '{prompt}' has no selectable items")
    }
}

fn ensure_option_rows(prompt: &str, rows: &[PromptRow]) -> Result<()> {
    if rows.iter().any(|row| matches!(row, PromptRow::Option(_))) {
        Ok(())
    } else {
        bail!("prompt '{prompt}' has no items")
    }
}

fn require_prompt_terminal() -> io::Result<()> {
    if io::stdin().is_terminal() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "not a terminal",
        ))
    }
}

fn run_confirm_prompt(prompt: &str, default: bool, decorated: bool) -> io::Result<bool> {
    require_prompt_terminal()?;
    let choices = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        write_prompt(prompt, Some(choices), None, decorated)?;
        let input = read_line()?;
        match input.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {
                eprintln!("Enter y or n.");
            }
        }
    }
}

fn run_input_prompt(prompt: &str, default: Option<&str>, decorated: bool) -> io::Result<String> {
    require_prompt_terminal()?;
    write_prompt(prompt, None, default, decorated)?;
    let input = read_line()?;
    if input.is_empty() {
        Ok(default.unwrap_or_default().to_string())
    } else {
        Ok(input)
    }
}

fn write_prompt(
    prompt: &str,
    suffix: Option<&str>,
    default: Option<&str>,
    decorated: bool,
) -> io::Result<()> {
    let prompt_start = if decorated {
        style(PROMPT_START).color256(110).bold().to_string()
    } else {
        PROMPT_START.to_string()
    };
    let mut stderr = io::stderr().lock();
    write!(stderr, "{prompt_start} {prompt}")?;
    if let Some(suffix) = suffix {
        write!(stderr, " {suffix}")?;
    }
    if let Some(default) = default
        && !default.is_empty()
    {
        write!(stderr, " ({default})")?;
    }
    write!(stderr, " ")?;
    stderr.flush()
}

fn read_line() -> io::Result<String> {
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
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
    fn prompt_rows_map_options_to_return_indices_while_skipping_sections() {
        let rows = vec![
            PromptRow::section("GitHub"),
            PromptRow::option_with_hint("Fix", "GitHub #73"),
            PromptRow::section("Local"),
            PromptRow::option_with_hint("Cleanup", "branch cleanup"),
        ];
        let mut state = SelectorState::single(selector_rows(&rows));

        state.apply_input(crate::ui::selector::SelectorInput::Down);

        assert_eq!(
            state.apply_input(crate::ui::selector::SelectorInput::Enter),
            crate::ui::selector::SelectorTransition::Submitted(SelectorSubmission::Single(1))
        );
    }

    #[test]
    fn prompt_row_hint_remains_metadata_in_owned_selector_rendering() {
        let rows = vec![PromptRow::option_with_hint(
            "Fix editor",
            "task PROJ-123 | Linear",
        )];
        let state = SelectorState::single(selector_rows(&rows));
        let rendered = render_plain(&state, "Task to start");

        assert!(rendered.contains("●  Fix editor"));
        assert!(rendered.contains("task PROJ-123 · Linear"));
    }

    #[test]
    fn task_selector_hint_starts_in_same_rendered_column_for_full_width_titles() {
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
        let rows = prompt_items_to_rows(&[local, provider]);
        let state = SelectorState::multi(selector_rows(&rows));
        let rendered = render_plain(&state, "Tasks to start");
        let local = rendered
            .lines()
            .find(|line| line.contains("짧은 제목"))
            .unwrap();
        let provider = rendered
            .lines()
            .find(|line| line.contains("인디위키 보호된 제목 목록 구현"))
            .unwrap();

        assert_eq!(
            rendered_column(local, "branch a"),
            rendered_column(provider, "Linear PROJ-123")
        );
    }

    #[test]
    fn selected_summary_is_enabled_for_grouped_or_long_multiselects() {
        let grouped = vec![
            PromptRow::section("Local"),
            PromptRow::option("One"),
            PromptRow::option("Two"),
        ];
        let compact = vec![PromptRow::option("One"), PromptRow::option("Two")];
        let long = (0..=DEFAULT_VISIBLE_OPTIONS)
            .map(|index| PromptRow::option(format!("Task {index}")))
            .collect::<Vec<_>>();

        assert!(should_show_selected_summary(&grouped));
        assert!(!should_show_selected_summary(&compact));
        assert!(should_show_selected_summary(&long));
    }

    fn hint_column(row: &str, hint: &str) -> usize {
        row.find(hint)
            .unwrap_or_else(|| panic!("row did not contain hint {hint:?}: {row:?}"))
    }

    fn rendered_column(row: &str, hint: &str) -> usize {
        let byte_index = hint_column(row, hint);
        console::measure_text_width(&row[..byte_index])
    }

    fn render_plain(state: &SelectorState, prompt: &str) -> String {
        let options = SelectorRenderOptions::new(prompt).decorated(false);
        console::strip_ansi_codes(&crate::ui::selector::render_selector(state, &options))
            .into_owned()
    }
}
