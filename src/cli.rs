use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "wt", about = "Git worktree workspace manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Commands {
    /// Create worktree from Linear issue
    Issue {
        /// Issue number (e.g. 680 → TECH-680)
        number: Option<u32>,
        /// Base branch: --base (interactive), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        #[arg(long)]
        parallel: bool,
    },
    /// Create worktree from GitHub PR
    Pr {
        /// PR number
        number: Option<u32>,
    },
    /// Create worktree with custom branch
    New {
        /// Branch name words (joined as kebab-case)
        name: Vec<String>,
        /// Base branch: --base (interactive), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        #[arg(long)]
        parallel: bool,
    },
    /// Open existing worktree with full workspace
    Open,
    /// Remove worktrees with cleanup
    Clean,
    /// Create or list variant configs
    Variant {
        /// Variant name to create (omit to list existing)
        name: Option<String>,
    },
}

/// Parsed base branch mode, derived from the raw --base flag.
#[derive(Debug, PartialEq)]
pub enum BaseMode {
    /// No --base flag: prompt user to confirm current branch
    Default,
    /// --base with no value: interactive select via dialoguer
    Interactive,
    /// --base <branch>: use the explicit branch
    Explicit(String),
}

impl BaseMode {
    pub fn from_raw(raw: &Option<String>) -> Self {
        match raw {
            None => BaseMode::Default,
            Some(s) if s.is_empty() => BaseMode::Interactive,
            Some(s) => BaseMode::Explicit(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }

    #[test]
    fn issue_no_args() {
        let cli = parse(&["wt", "issue"]);
        assert!(matches!(
            cli.command,
            Commands::Issue {
                number: None,
                base: None,
                parallel: false
            }
        ));
    }

    #[test]
    fn issue_with_number() {
        let cli = parse(&["wt", "issue", "680"]);
        assert!(matches!(
            cli.command,
            Commands::Issue {
                number: Some(680),
                base: None,
                parallel: false
            }
        ));
    }

    #[test]
    fn issue_with_base_interactive() {
        let cli = parse(&["wt", "issue", "--base"]);
        if let Commands::Issue { base, .. } = &cli.command {
            assert_eq!(BaseMode::from_raw(base), BaseMode::Interactive);
        } else {
            panic!("expected Issue");
        }
    }

    #[test]
    fn issue_with_parallel_flag() {
        let cli = parse(&["wt", "issue", "680", "--parallel"]);
        assert!(matches!(
            cli.command,
            Commands::Issue {
                number: Some(680),
                parallel: true,
                ..
            }
        ));
    }

    #[test]
    fn new_parallel_flag() {
        let cli = parse(&["wt", "new", "some", "feature", "--parallel"]);
        if let Commands::New { parallel, .. } = &cli.command {
            assert!(*parallel);
        } else {
            panic!("expected New");
        }
    }

    #[test]
    fn pr_with_number() {
        let cli = parse(&["wt", "pr", "42"]);
        assert!(matches!(cli.command, Commands::Pr { number: Some(42) }));
    }

    #[test]
    fn clean_subcommand() {
        let cli = parse(&["wt", "clean"]);
        assert!(matches!(cli.command, Commands::Clean));
    }

    #[test]
    fn open_subcommand() {
        let cli = parse(&["wt", "open"]);
        assert!(matches!(cli.command, Commands::Open));
    }

    #[test]
    fn base_mode_from_none() {
        assert_eq!(BaseMode::from_raw(&None), BaseMode::Default);
    }

    #[test]
    fn base_mode_from_empty() {
        assert_eq!(BaseMode::from_raw(&Some("".into())), BaseMode::Interactive);
    }

    #[test]
    fn base_mode_from_value() {
        assert_eq!(
            BaseMode::from_raw(&Some("develop".into())),
            BaseMode::Explicit("develop".into())
        );
    }
}
