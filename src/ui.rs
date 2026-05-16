use crate::context::UserInterface;
use anyhow::Result;
use console::style;
use dialoguer::{Confirm, FuzzySelect, Input, MultiSelect};

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
        let selection = FuzzySelect::new()
            .with_prompt(prompt)
            .items(items)
            .max_length(20)
            .interact()?;
        Ok(selection)
    }

    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
        let selections = MultiSelect::new()
            .with_prompt(prompt)
            .items(items)
            .max_length(20)
            .interact()?;
        Ok(selections)
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        let result = Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()?;
        Ok(result)
    }

    fn input(&self, prompt: &str, default: Option<&str>) -> Result<String> {
        let mut builder = Input::<String>::new().with_prompt(prompt);
        if let Some(d) = default {
            builder = builder.default(d.into());
        }
        let result = builder.interact_text()?;
        Ok(result)
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
