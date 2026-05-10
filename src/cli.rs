use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "wt", version, about = "Git worktree workspace manager")]
pub struct Cli {
    /// Run as if wt was started in DIR
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// Config file to load instead of .local/.wt.toml or .wt.toml
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
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Commands {
    /// Print wt version
    Version,
    /// Generate shell completion script
    #[command(alias = "completions")]
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Start a ready-to-code workspace from an issue, PR, or branch name
    Start {
        /// Issue number, issue ID, pr [NUMBER], or branch name words
        target: Vec<String>,
        /// Base branch: --base (interactive), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Profile to use for this workspace
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each profile
        #[arg(long)]
        parallel: bool,
    },
    /// Prepare or run issue batches
    Batch {
        #[command(subcommand)]
        command: BatchCommand,
    },
    /// Show worktree, branch, site, and setup state
    List,
    /// Open existing worktree with full workspace
    Open {
        /// Branch, issue number, or worktree directory name to open
        target: Option<String>,
    },
    /// Finish worktrees with cleanup
    Done {
        /// Branch or worktree directory names to finish
        targets: Vec<String>,
    },
    /// Check configured providers and required local tools
    Doctor,
    /// Create or list profile configs
    Profile {
        /// Profile name to create (omit to list existing)
        name: Option<String>,
    },
    /// Create a wt config file
    Init {
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        #[arg(long)]
        shared: bool,
        #[arg(long, value_enum)]
        agent: Option<InitAgent>,
        #[arg(long = "agent-arg", allow_hyphen_values = true)]
        agent_args: Vec<String>,
        #[arg(long)]
        agent_command: Option<String>,
        #[arg(long, value_enum)]
        issue_provider: Option<InitIssueProvider>,
        #[arg(long, value_enum)]
        site_provider: Option<InitSiteProvider>,
        #[arg(long)]
        gh_user: Option<String>,
        #[arg(long, conflicts_with = "no_prompts")]
        prompts: bool,
        #[arg(long)]
        no_prompts: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    /// Inspect helper state for the Traefik site provider
    Traefik {
        #[command(subcommand)]
        command: TraefikCommand,
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
    #[value(name = "docker_proxy", alias = "docker-proxy")]
    DockerProxy,
    Traefik,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum BatchCommand {
    /// Snapshot issues and create a batch file without starting workspaces
    Prepare {
        /// Issue identifiers to snapshot
        issues: Vec<String>,
        /// Profile to use for all issues (defaults to current config)
        #[arg(long)]
        profile: Option<String>,
        /// Base branch: --base (interactive), --base main (explicit)
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
pub enum TraefikCommand {
    /// Check the host setup expected by provider = "traefik"
    Doctor,
    /// Print the paths and defaults used by wt's Traefik provider
    Paths,
    /// Print an example macOS LaunchDaemon plist for host-native Traefik
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
    fn start_no_args() {
        let cli = parse(&["wt", "start"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Start {
                ref target,
                base: None,
                profile: None,
                parallel: false
            }) if target.is_empty()
        ));
    }

    #[test]
    fn start_with_issue_number_target() {
        let cli = parse(&["wt", "start", "680"]);
        if let Some(Commands::Start {
            target,
            base,
            profile,
            parallel,
        }) = cli.command
        {
            assert_eq!(target, vec!["680"]);
            assert_eq!(base, None);
            assert_eq!(profile, None);
            assert!(!parallel);
        } else {
            panic!("expected Start");
        }
    }

    #[test]
    fn start_with_base_interactive() {
        let cli = parse(&["wt", "start", "--base"]);
        if let Some(Commands::Start { base, .. }) = &cli.command {
            assert_eq!(BaseMode::from_raw(base), BaseMode::Interactive);
        } else {
            panic!("expected Start");
        }
    }

    #[test]
    fn start_with_parallel_flag() {
        let cli = parse(&["wt", "start", "680", "--parallel"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Start { parallel: true, .. })
        ));
    }

    #[test]
    fn start_with_profile_flag() {
        let cli = parse(&["wt", "start", "680", "--profile", "codex-yolo"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Start {
                profile: Some(ref profile),
                ..
            }) if profile == "codex-yolo"
        ));
    }

    #[test]
    fn batch_prepare_accepts_default_profile() {
        let cli = parse(&["wt", "batch", "prepare", "PROJ-123", "PROJ-456"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Prepare {
                    ref issues,
                    profile: None,
                    base: None,
                }
            }) if issues == &vec!["PROJ-123".to_string(), "PROJ-456".to_string()]
        ));
    }

    #[test]
    fn batch_prepare_accepts_profile_and_base() {
        let cli = parse(&[
            "wt",
            "batch",
            "prepare",
            "PROJ-123",
            "--profile",
            "codex-yolo",
            "--base",
            "main",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Batch {
                command: BatchCommand::Prepare {
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
    fn start_with_branch_words() {
        let cli = parse(&["wt", "start", "some", "feature", "--parallel"]);
        if let Some(Commands::Start {
            target, parallel, ..
        }) = &cli.command
        {
            assert_eq!(target, &vec!["some".to_string(), "feature".to_string()]);
            assert!(*parallel);
        } else {
            panic!("expected Start");
        }
    }

    #[test]
    fn start_with_pr_target() {
        let cli = parse(&["wt", "start", "pr:42"]);
        if let Some(Commands::Start { target, .. }) = cli.command {
            assert_eq!(target, vec!["pr:42"]);
        } else {
            panic!("expected Start");
        }
    }

    #[test]
    fn start_with_split_pr_target() {
        let cli = parse(&["wt", "start", "pr", "42"]);
        if let Some(Commands::Start { target, .. }) = cli.command {
            assert_eq!(target, vec!["pr", "42"]);
        } else {
            panic!("expected Start");
        }
    }

    #[test]
    fn list_subcommand() {
        let cli = parse(&["wt", "list"]);
        assert!(matches!(cli.command, Some(Commands::List)));
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
    fn init_accepts_docker_proxy_site_provider_aliases() {
        for value in ["docker_proxy", "docker-proxy"] {
            let cli = parse(&["wt", "init", "--site-provider", value]);
            assert!(matches!(
                cli.command,
                Some(Commands::Init {
                    site_provider: Some(InitSiteProvider::DockerProxy),
                    ..
                })
            ));
        }
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
    fn traefik_doctor_subcommand() {
        let cli = parse(&["wt", "traefik", "doctor"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Traefik {
                command: TraefikCommand::Doctor
            })
        ));
    }

    #[test]
    fn traefik_example_launchd_accepts_overrides() {
        let cli = parse(&[
            "wt",
            "traefik",
            "example-launchd",
            "--label",
            "dev.wt-traefik",
            "--bind-ip",
            "127.0.0.3",
        ]);

        assert!(matches!(
            cli.command,
            Some(Commands::Traefik {
                command: TraefikCommand::ExampleLaunchd { label, bind_ip }
            }) if label == "dev.wt-traefik" && bind_ip == "127.0.0.3"
        ));
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
