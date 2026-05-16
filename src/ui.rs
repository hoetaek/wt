use crate::context::UserInterface;
use crate::error::WtError;
use anyhow::{Result, anyhow};
use console::style;
use std::io;

const PROMPT_MAX_ROWS: usize = 10;

pub struct TerminalUi {
    quiet: bool,
    decorated: bool,
}

impl TerminalUi {
    pub fn new(quiet: bool) -> Self {
        Self {
            quiet,
            decorated: true,
        }
    }

    pub fn with_decoration(quiet: bool, decorated: bool) -> Self {
        Self { quiet, decorated }
    }
}

impl UserInterface for TerminalUi {
    fn select(&self, prompt: &str, items: &[String]) -> Result<usize> {
        let mut select = cliclack::select(prompt)
            .max_rows(PROMPT_MAX_ROWS)
            .filter_mode();
        for (index, item) in items.iter().enumerate() {
            select = select.item(index, item, "");
        }
        prompt_result(prompt, select.interact())
    }

    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
        let mut multi_select = cliclack::multiselect(prompt)
            .max_rows(PROMPT_MAX_ROWS)
            .required(false)
            .filter_mode();
        for (index, item) in items.iter().enumerate() {
            multi_select = multi_select.item(index, item, "");
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
}
