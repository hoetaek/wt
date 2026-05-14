use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "wt", version, about = "Git worktree workspace manager")]
pub struct Cli {
    /// Run wt from DIR
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// Config file to load for commands that read wt config
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Increase diagnostic output (-v, -vv)
    #[arg(short, long, action = ArgAction::Count, global = true, conflicts_with = "quiet")]
    pub verbose: u8,
    /// Suppress normal status output
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// When to use terminal colors
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true)]
    pub color: ColorMode,
    /// Disable terminal colors
    #[arg(long = "no-color", global = true, conflicts_with = "color")]
    pub no_color: bool,
    /// Emit machine-readable JSON for supported commands
    #[arg(long, global = true, hide = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Commands {
    /// Print wt version
    Version,
    /// Generate shell completion script
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Start a workspace from an issue
    Issue {
        /// Issue number or provider-specific key
        target: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled issue worktree from .local/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each profile
        #[arg(long, conflicts_with = "profile")]
        parallel: bool,
    },
    /// Start a workspace from a pull request
    Pr {
        /// Pull request number (omit to select from the open PR list)
        number: Option<u32>,
        /// Apply config from .local/profiles/<name> to the PR worktree
        #[arg(long)]
        profile: Option<String>,
    },
    /// Start a workspace from branch-name text
    New {
        /// Branch name words
        #[arg(required = true)]
        name: Vec<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled branch worktree from .local/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each profile
        #[arg(long, conflicts_with = "profile")]
        parallel: bool,
    },
    /// Create or run issue batches
    Batch {
        #[command(subcommand)]
        command: BatchCommand,
    },
    /// Create or run branch stacks
    Stack {
        #[command(subcommand)]
        command: StackCommand,
    },
    /// Show worktree, branch, site, and setup state
    List {
        /// Do not truncate table columns to fit the terminal
        #[arg(long)]
        wide: bool,
    },
    /// Open a workspace from an existing worktree or branch
    Open {
        /// Branch or worktree directory name to open directly
        target: Option<String>,
    },
    /// Finish worktrees with cleanup
    Done {
        /// Branch, issue number/key, or worktree directory names to finish
        targets: Vec<String>,
    },
    /// Check configured providers and required local tools
    Doctor,
    /// Print the effective config
    Config {
        /// Use a named profile from .local/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
    },
    /// List or manage named profile configs
    Profile {
        #[command(subcommand)]
        command: Option<ProfileCommand>,
    },
    /// Create a wt config file
    Init {
        /// Write private config to .local/.wt.toml
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        /// Write shared project config to .wt.toml
        #[arg(long)]
        shared: bool,
        /// Agent runtime for omitted --profile runs
        #[arg(long, value_enum)]
        agent: Option<InitAgent>,
        /// Extra argument for the generated agent command
        #[arg(long = "agent-arg", allow_hyphen_values = true)]
        agent_args: Vec<String>,
        /// Override the generated agent command
        #[arg(long)]
        agent_command: Option<String>,
        /// Issue provider to configure
        #[arg(long, value_enum)]
        issue_provider: Option<InitIssueProvider>,
        /// Local site provider to configure
        #[arg(long, value_enum)]
        site_provider: Option<InitSiteProvider>,
        /// GitHub user for issue list filtering
        #[arg(long)]
        gh_user: Option<String>,
        /// Create a named profile with prompt scaffold
        #[arg(long, conflicts_with = "no_prompts")]
        prompts: bool,
        /// Keep profile settings inline
        #[arg(long)]
        no_prompts: bool,
        /// Skip confirmation prompts
        #[arg(long)]
        yes: bool,
        /// Overwrite existing config/profile settings
        #[arg(long)]
        force: bool,
    },
    /// Inspect and manage local site provider helpers
    Site {
        #[command(subcommand)]
        command: SiteCommand,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum ProfileCommand {
    /// Create a named profile scaffold
    Create {
        /// New profile name
        name: String,
    },
    /// Move inline [profile.*] settings into a named profile
    Promote {
        /// New profile name
        name: String,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum InitAgent {
    Codex,
    Claude,
    Gemini,
    None,
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum InitIssueProvider {
    Github,
    Linear,
    None,
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum InitSiteProvider {
    None,
    Herd,
    Valet,
    #[value(name = "docker_proxy")]
    DockerProxy,
    Traefik,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum BatchCommand {
    /// Snapshot issues and create a batch file without starting workspaces
    #[command(alias = "prepare")]
    Issue {
        /// Issue identifiers to snapshot (omit to select interactively)
        issues: Vec<String>,
        /// Named profile from .local/profiles/<name> for all issues
        #[arg(long)]
        profile: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
    },
    /// Run prepared or failed items from a batch file
    Run {
        /// Batch TOML path, or "latest" for the newest local batch
        batch: String,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum StackCommand {
    /// Create a stack file from branch-name text without starting workspaces
    New {
        /// Branch-name text items to create in base-to-top order
        #[arg(required = true)]
        items: Vec<String>,
        /// Named profile from .local/profiles/<name> for all items
        #[arg(long)]
        profile: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
    },
    /// Create a stack file from issues without starting workspaces
    Issue {
        /// Issue identifiers to snapshot in base-to-top order (omit to select interactively)
        issues: Vec<String>,
        /// Named profile from .local/profiles/<name> for all issues
        #[arg(long)]
        profile: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
    },
    /// Start the next prepared or failed item from a stack file
    Run {
        /// Stack TOML path, or "latest" for the newest local stack
        stack: String,
    },
    /// Mark the running item in a stack as complete
    Complete {
        /// Stack TOML path, or "latest" for the newest local stack
        stack: String,
        /// Running stack item identifier to complete
        item: Option<String>,
        /// Start the next stack item after marking this one complete
        #[arg(long)]
        run_next: bool,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum SiteCommand {
    /// Check the configured local site provider setup
    Doctor,
    /// Print provider-managed site paths and defaults
    Paths,
    /// Print an example macOS LaunchDaemon plist for provider = "traefik"
    ExampleLaunchd {
        /// LaunchDaemon label to render
        #[arg(long, default_value = "wt.traefik")]
        label: String,
        /// IP address Traefik should bind for wt sites
        #[arg(long, default_value = "127.0.0.2")]
        bind_ip: String,
    },
}

/// Parsed base branch mode, derived from the raw --base flag.
#[derive(Debug, PartialEq)]
pub enum BaseMode {
    /// No --base flag: prompt user to confirm current branch
    Default,
    /// --base with no value: interactive select via dialoguer
    Interactive,
    /// --base .: use current branch without prompting
    Current,
    /// --base <branch>: use the explicit branch
    Explicit(String),
}

impl BaseMode {
    pub fn from_raw(raw: &Option<String>) -> Self {
        match raw {
            None => BaseMode::Default,
            Some(s) if s.is_empty() => BaseMode::Interactive,
            Some(s) if s == "." => BaseMode::Current,
            Some(s) => BaseMode::Explicit(s.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }

    #[test]
    fn no_subcommand_is_allowed_for_manual_help_handling() {
        let cli = parse(&["wt"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn version_subcommand() {
        let cli = parse(&["wt", "version"]);
        assert!(matches!(cli.command, Some(Commands::Version)));
    }

    #[test]
    fn completion_subcommand() {
        let cli = parse(&["wt", "completion", "bash"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Completion {
                shell: clap_complete::Shell::Bash
            })
        ));
    }

    #[test]
    fn global_options_parse_before_subcommand() {
        let cli = parse(&[
            "wt",
            "-C",
            "/tmp/repo",
            "--config",
            "/tmp/wt.toml",
            "--color",
            "always",
            "-vv",
            "doctor",
        ]);
        assert_eq!(
            cli.directory.as_deref(),
            Some(std::path::Path::new("/tmp/repo"))
        );
        assert_eq!(
            cli.config.as_deref(),
            Some(std::path::Path::new("/tmp/wt.toml"))
        );
        assert_eq!(cli.color, ColorMode::Always);
        assert_eq!(cli.verbose, 2);
        assert!(matches!(cli.command, Some(Commands::Doctor)));
    }

    #[test]
    fn no_color_flag() {
        let cli = parse(&["wt", "--no-color", "doctor"]);
        assert!(cli.no_color);
        assert!(matches!(cli.command, Some(Commands::Doctor)));
    }

    #[test]
    fn issue_no_args_starts_interactive_issue_flow() {
        let cli = parse(&["wt", "issue"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Issue {
                target: None,
                base: None,
                profile: None,
                parallel: false
            })
        ));
    }

    #[test]
    fn issue_with_target() {
        let cli = parse(&["wt", "issue", "PROJ-680"]);
        if let Some(Commands::Issue {
            target,
            base,
            profile,
            parallel,
        }) = cli.command
        {
            assert_eq!(target.as_deref(), Some("PROJ-680"));
            assert_eq!(base, None);
            assert_eq!(profile, None);
            assert!(!parallel);
        } else {
            panic!("expected Issue");
        }
    }

    #[test]
    fn issue_with_base_interactive() {
        let cli = parse(&["wt", "issue", "--base"]);
        if let Some(Commands::Issue { base, .. }) = &cli.command {
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
            Some(Commands::Issue { parallel: true, .. })
        ));
    }

    #[test]
    fn issue_with_profile_flag() {
        let cli = parse(&["wt", "issue", "680", "--profile", "codex-yolo"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Issue {
                profile: Some(ref profile),
                ..
            }) if profile == "codex-yolo"
        ));
    }

    #[test]
    fn issue_rejects_parallel_with_profile() {
        let result =
            Cli::try_parse_from(["wt", "issue", "680", "--parallel", "--profile", "codex"]);
        assert!(result.is_err());
    }

    #[test]
    fn pr_no_args_starts_interactive_pr_flow() {
        let cli = parse(&["wt", "pr"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Pr {
                number: None,
                profile: None
            })
        ));
    }

    #[test]
    fn pr_with_number_and_profile() {
        let cli = parse(&["wt", "pr", "42", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Pr {
                number: Some(42),
                profile: Some(ref profile),
            }) if profile == "codex"
        ));
    }

    #[test]
    fn batch_issue_accepts_default_profile() {
        let cli = parse(&["wt", "batch", "issue", "PROJ-123", "PROJ-456"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Issue {
                    ref issues,
                    profile: None,
                    base: None,
                }
            }) if issues == &vec!["PROJ-123".to_string(), "PROJ-456".to_string()]
        ));
    }

    #[test]
    fn batch_issue_accepts_no_issue_args_for_interactive_selection() {
        let cli = parse(&["wt", "batch", "issue"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Issue {
                    ref issues,
                    profile: None,
                    base: None,
                }
            }) if issues.is_empty()
        ));
    }

    #[test]
    fn batch_issue_accepts_profile_and_base() {
        let cli = parse(&[
            "wt",
            "batch",
            "issue",
            "PROJ-123",
            "--profile",
            "codex-yolo",
            "--base",
            "main",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Issue {
                    ref issues,
                    profile: Some(ref profile),
                    base: Some(ref base),
                }
            }) if issues == &vec!["PROJ-123".to_string()]
                && profile == "codex-yolo"
                && base == "main"
        ));
    }

    #[test]
    fn batch_prepare_alias_parses_as_issue_but_is_hidden_from_help() {
        let cli = parse(&["wt", "batch", "prepare", "PROJ-123"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Issue { ref issues, .. }
            }) if issues == &vec!["PROJ-123".to_string()]
        ));

        let mut command = Cli::command();
        let batch = command.find_subcommand_mut("batch").unwrap();
        let help = batch.render_help().to_string();
        assert!(help.contains("issue"));
        assert!(!help.contains("  prepare"));
    }

    #[test]
    fn batch_run_accepts_latest() {
        let cli = parse(&["wt", "batch", "run", "latest"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Run { ref batch }
            }) if batch == "latest"
        ));
    }

    #[test]
    fn stack_issue_accepts_ordered_issues_profile_and_base() {
        let cli = parse(&[
            "wt",
            "stack",
            "issue",
            "PROJ-123",
            "PROJ-456",
            "--profile",
            "codex",
            "--base",
            "main",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Stack {
                command: StackCommand::Issue {
                    ref issues,
                    profile: Some(ref profile),
                    base: Some(ref base),
                }
            }) if issues == &vec!["PROJ-123".to_string(), "PROJ-456".to_string()]
                && profile == "codex"
                && base == "main"
        ));
    }

    #[test]
    fn stack_new_accepts_items_profile_and_base() {
        let cli = parse(&[
            "wt",
            "stack",
            "new",
            "add-schema",
            "wire-api",
            "--profile",
            "codex",
            "--base",
            "main",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Stack {
                command: StackCommand::New {
                    ref items,
                    profile: Some(ref profile),
                    base: Some(ref base),
                }
            }) if items == &vec!["add-schema".to_string(), "wire-api".to_string()]
                && profile == "codex"
                && base == "main"
        ));
    }

    #[test]
    fn stack_issue_accepts_no_issue_args_for_interactive_selection() {
        let cli = parse(&["wt", "stack", "issue"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Stack {
                command: StackCommand::Issue {
                    ref issues,
                    profile: None,
                    base: None,
                }
            }) if issues.is_empty()
        ));
    }

    #[test]
    fn stack_run_accepts_latest() {
        let cli = parse(&["wt", "stack", "run", "latest"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Stack {
                command: StackCommand::Run { ref stack }
            }) if stack == "latest"
        ));
    }

    #[test]
    fn stack_complete_accepts_stack_and_item() {
        let cli = parse(&["wt", "stack", "complete", "latest", "PROJ-123"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Stack {
                command: StackCommand::Complete {
                    ref stack,
                    item: Some(ref item),
                    run_next: false,
                }
            }) if stack == "latest" && item == "PROJ-123"
        ));
    }

    #[test]
    fn stack_complete_accepts_run_next() {
        let cli = parse(&[
            "wt",
            "stack",
            "complete",
            "latest",
            "PROJ-123",
            "--run-next",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Stack {
                command: StackCommand::Complete {
                    ref stack,
                    item: Some(ref item),
                    run_next: true,
                }
            }) if stack == "latest" && item == "PROJ-123"
        ));
    }

    #[test]
    fn new_with_branch_words() {
        let cli = parse(&["wt", "new", "some", "feature", "--parallel"]);
        if let Some(Commands::New { name, parallel, .. }) = &cli.command {
            assert_eq!(name, &vec!["some".to_string(), "feature".to_string()]);
            assert!(*parallel);
        } else {
            panic!("expected New");
        }
    }

    #[test]
    fn new_with_base_and_profile() {
        let cli = parse(&[
            "wt",
            "new",
            "some",
            "feature",
            "--base",
            "main",
            "--profile",
            "codex",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::New {
                ref name,
                base: Some(ref base),
                profile: Some(ref profile),
                parallel: false,
            }) if name == &vec!["some".to_string(), "feature".to_string()]
                && base == "main"
                && profile == "codex"
        ));
    }

    #[test]
    fn new_rejects_parallel_with_profile() {
        let result = Cli::try_parse_from([
            "wt",
            "new",
            "some",
            "feature",
            "--parallel",
            "--profile",
            "codex",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn start_subcommand_is_removed() {
        let result = Cli::try_parse_from(["wt", "start"]);
        assert!(result.is_err());
    }

    #[test]
    fn list_subcommand() {
        let cli = parse(&["wt", "list"]);
        assert!(matches!(cli.command, Some(Commands::List { wide: false })));
    }

    #[test]
    fn list_accepts_wide_flag() {
        let cli = parse(&["wt", "list", "--wide"]);
        assert!(matches!(cli.command, Some(Commands::List { wide: true })));
    }

    #[test]
    fn done_subcommand() {
        let cli = parse(&["wt", "done", "feature"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Done { ref targets }) if targets == &vec!["feature".to_string()]
        ));
    }

    #[test]
    fn doctor_subcommand() {
        let cli = parse(&["wt", "doctor"]);
        assert!(matches!(cli.command, Some(Commands::Doctor)));
    }

    #[test]
    fn config_subcommand() {
        let cli = parse(&["wt", "config"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config { profile: None })
        ));
    }

    #[test]
    fn config_accepts_profile_flag() {
        let cli = parse(&["wt", "config", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: Some(ref profile)
            }) if profile == "codex"
        ));
    }

    #[test]
    fn open_subcommand() {
        let cli = parse(&["wt", "open", "feature"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Open {
                target: Some(ref target)
            }) if target == "feature"
        ));
    }

    #[test]
    fn init_local_codex_yes() {
        let cli = parse(&["wt", "init", "--local", "--agent", "codex", "--yes"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                yes: true,
                ..
            })
        ));
    }

    #[test]
    fn init_parses_agent_args_and_prompts() {
        let cli = parse(&[
            "wt",
            "init",
            "--shared",
            "--agent",
            "gemini",
            "--agent-arg",
            "--model=gemini-pro",
            "--issue-provider",
            "github",
            "--site-provider",
            "valet",
            "--gh-user",
            "alice",
            "--prompts",
            "--force",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                shared: true,
                agent: Some(InitAgent::Gemini),
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: Some(InitSiteProvider::Valet),
                prompts: true,
                force: true,
                ..
            })
        ));
        if let Some(Commands::Init {
            agent_args,
            site_provider,
            gh_user,
            ..
        }) = cli.command
        {
            assert_eq!(agent_args, vec!["--model=gemini-pro"]);
            assert_eq!(site_provider, Some(InitSiteProvider::Valet));
            assert_eq!(gh_user.as_deref(), Some("alice"));
        }
    }

    #[test]
    fn init_accepts_docker_proxy_site_provider() {
        let cli = parse(&["wt", "init", "--site-provider", "docker_proxy"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                site_provider: Some(InitSiteProvider::DockerProxy),
                ..
            })
        ));
    }

    #[test]
    fn init_rejects_docker_proxy_kebab_alias() {
        let result = Cli::try_parse_from(["wt", "init", "--site-provider", "docker-proxy"]);
        assert!(result.is_err());
    }

    #[test]
    fn init_accepts_traefik_site_provider() {
        let cli = parse(&["wt", "init", "--site-provider", "traefik"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                site_provider: Some(InitSiteProvider::Traefik),
                ..
            })
        ));
    }

    #[test]
    fn site_doctor_subcommand() {
        let cli = parse(&["wt", "site", "doctor"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Site {
                command: SiteCommand::Doctor
            })
        ));
    }

    #[test]
    fn site_example_launchd_accepts_overrides() {
        let cli = parse(&[
            "wt",
            "site",
            "example-launchd",
            "--label",
            "dev.wt-traefik",
            "--bind-ip",
            "127.0.0.3",
        ]);

        assert!(matches!(
            cli.command,
            Some(Commands::Site {
                command: SiteCommand::ExampleLaunchd { label, bind_ip }
            }) if label == "dev.wt-traefik" && bind_ip == "127.0.0.3"
        ));
    }

    #[test]
    fn traefik_subcommand_is_removed() {
        let result = Cli::try_parse_from(["wt", "traefik", "doctor"]);
        assert!(result.is_err());
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
    fn base_mode_from_dot() {
        assert_eq!(BaseMode::from_raw(&Some(".".into())), BaseMode::Current);
    }

    #[test]
    fn base_mode_from_value() {
        assert_eq!(
            BaseMode::from_raw(&Some("develop".into())),
            BaseMode::Explicit("develop".into())
        );
    }
}
