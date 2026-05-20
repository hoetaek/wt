use anyhow::{Context, Result, bail};
use clap::{Command as ClapCommand, CommandFactory, Parser};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;

use wt::cli::{AgentCommand, Cli, ColorMode, Commands, MsgCommand, TaskCommand, WorkflowCommand};
use wt::config::{Config, ConfigSource};
use wt::context::{Ctx, CtxOptions, OutputMode};
use wt::error::WtError;
use wt::runner::RealRunner;
use wt::services::git::GitService;
use wt::ui::TerminalUi;

fn main() {
    if let Err(e) = try_main() {
        if let Some(WtError::Cancelled) = e.downcast_ref::<WtError>() {
            process::exit(0);
        }
        if let Some(WtError::Exit { code }) = e.downcast_ref::<WtError>() {
            process::exit(*code);
        }
        eprintln!("{} {e:#}", console::style("ERROR:").red());
        process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let cli = Cli::parse();
    apply_color(effective_color(&cli));

    let Some(command) = cli.command.as_ref() else {
        let mut command = Cli::command();
        command.print_help()?;
        println!();
        return Ok(());
    };

    if cli.json && !supports_json(command) {
        bail!(
            "JSON output is supported for: wt version, wt list, wt inspect, wt task list, wt workflow list, wt agent status, wt agent watch, wt msg list, wt msg read, wt msg check-inbox, wt doctor, wt profile"
        );
    }

    match command {
        Commands::Version => {
            print_version(cli.json);
            return Ok(());
        }
        Commands::Completion { shell } => {
            let mut command = completion_command();
            let bin_name = command.get_name().to_string();
            let mut buffer = Vec::new();
            clap_complete::generate(*shell, &mut command, bin_name, &mut buffer);
            let script = String::from_utf8(buffer)?;
            io::stdout().write_all(strip_removed_completion_entries(*shell, &script).as_bytes())?;
            return Ok(());
        }
        _ => {}
    }

    if let Some((old, new)) = wt::deprecated_start_replacement(command) {
        bail!(
            "`{old}` has moved. Use `{new}` to start workspace execution. The old command is not an alias."
        );
    }

    let runner = RealRunner;
    let current_dir = std::env::current_dir()?;
    let working_dir = resolve_directory(&current_dir, cli.directory.as_deref())?;

    let git = GitService::new(&runner, working_dir.as_deref());
    let invocation_root = git.repo_root()?;
    let repo_root = git.canonical_repo_root()?;
    let storage_root = wt::storage::StorageRoot::resolve(&runner, Some(&invocation_root))?;
    let config_base = working_dir.as_deref().unwrap_or(&current_dir);

    let (base_config, config, config_source) = if matches!(command, Commands::Init { .. }) {
        (Config::default(), Config::default(), ConfigSource::Default)
    } else if let Some(path) = cli.config.as_deref() {
        let path = resolve_path(config_base, path);
        let (base_config, config) =
            Config::load_file_for_repo_with_storage_root(&path, &repo_root, &storage_root)
                .with_context(|| format!("failed to load config: {}", path.display()))?;
        (base_config, config, ConfigSource::File(path))
    } else {
        Config::load_base_and_effective_with_source_and_storage_root(&repo_root, &storage_root)?
    };

    let output_mode = if cli.json {
        OutputMode::Json
    } else {
        OutputMode::Text
    };

    let ctx = Ctx::new_with_options(
        repo_root,
        invocation_root,
        config,
        Box::new(RealRunner),
        Box::new(TerminalUi::with_decoration(
            cli.quiet,
            use_decorative_output(&cli),
        )),
        CtxOptions {
            base_config,
            config_source: config_source.clone(),
            storage_root: Some(storage_root),
            output_mode,
            verbosity: cli.verbose,
            quiet: cli.quiet,
        },
    );

    if cli.verbose > 0 && !ctx.is_json() {
        ctx.ui
            .print_dim(&format!("repo: {}", ctx.repo_root.display()));
        ctx.ui
            .print_dim(&format!("invocation: {}", ctx.invocation_root.display()));
        match config_source {
            ConfigSource::Default => ctx.ui.print_dim("config: default"),
            ConfigSource::File(path) => ctx.ui.print_dim(&format!("config: {}", path.display())),
            ConfigSource::Files(paths) => {
                let rendered = paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" + ");
                ctx.ui.print_dim(&format!("config: {rendered}"));
            }
        }
    }

    wt::dispatch(&ctx, command)
}

const HIDDEN_COMPLETION_PREFIX: &str = "__wt_removed_";

fn completion_command() -> ClapCommand {
    let mut hidden_index = 0;
    rename_hidden_subcommands(Cli::command(), &mut hidden_index)
}

fn rename_hidden_subcommands(command: ClapCommand, hidden_index: &mut usize) -> ClapCommand {
    command.mut_subcommands(|subcommand| {
        let is_hidden = subcommand.is_hide_set();
        let subcommand = rename_hidden_subcommands(subcommand, hidden_index);
        if is_hidden {
            // AOT completion scripts still walk hidden parser traps; keep them
            // unreachable without teaching removed command names to shells.
            // Clap stores command names as borrowed values; completion runs once
            // and exits, so leaking this generated hidden name is intentional.
            let hidden_name: &'static str =
                Box::leak(format!("{HIDDEN_COMPLETION_PREFIX}{hidden_index}").into_boxed_str());
            *hidden_index += 1;
            subcommand.name(hidden_name)
        } else {
            subcommand
        }
    })
}

fn strip_removed_completion_entries(shell: clap_complete::Shell, script: &str) -> String {
    let mut output = String::new();
    let mut skipping_removed_case_arm_depth: Option<usize> = None;

    for line in script.lines() {
        let trimmed = line.trim_start();
        if let Some(case_depth) = skipping_removed_case_arm_depth.as_mut() {
            if trimmed.starts_with("case ") {
                *case_depth += 1;
            } else if trimmed == "esac" {
                *case_depth = case_depth.saturating_sub(1);
            } else if *case_depth == 0 && trimmed == ";;" {
                skipping_removed_case_arm_depth = None;
            }
            continue;
        }

        if matches!(
            shell,
            clap_complete::Shell::Fish
                | clap_complete::Shell::Elvish
                | clap_complete::Shell::PowerShell
        ) && contains_hidden_completion_name(trimmed)
        {
            continue;
        }

        if matches!(shell, clap_complete::Shell::Zsh)
            && contains_hidden_completion_name(trimmed)
            && (trimmed.starts_with('\'') || trimmed.starts_with('"'))
        {
            continue;
        }

        if contains_hidden_completion_name(trimmed)
            && (trimmed.starts_with(HIDDEN_COMPLETION_PREFIX)
                || trimmed
                    .strip_prefix("case ")
                    .is_some_and(|rest| rest.starts_with(HIDDEN_COMPLETION_PREFIX)))
        {
            continue;
        }

        if matches!(
            shell,
            clap_complete::Shell::Bash | clap_complete::Shell::Zsh
        ) && contains_hidden_completion_name(trimmed)
            && trimmed.ends_with(')')
        {
            skipping_removed_case_arm_depth = Some(0);
            continue;
        }

        if !trimmed.contains(' ') && contains_hidden_completion_name(trimmed) {
            continue;
        }

        let cleaned = strip_removed_completion_tokens(line);
        if cleaned.trim().is_empty() && contains_hidden_completion_name(line) {
            continue;
        }
        output.push_str(&cleaned);
        output.push('\n');
    }

    output
}

fn contains_hidden_completion_name(line: &str) -> bool {
    line.contains(HIDDEN_COMPLETION_PREFIX)
}

fn strip_removed_completion_tokens(line: &str) -> String {
    let mut cleaned = line.to_string();

    while let Some(start) = cleaned.find(HIDDEN_COMPLETION_PREFIX) {
        let end = hidden_completion_name_end(&cleaned, start);
        let (start, end) = expand_hidden_completion_token_range(&cleaned, start, end);
        cleaned.replace_range(start..end, "");
    }

    cleaned
}

fn hidden_completion_name_end(line: &str, start: usize) -> usize {
    let mut end = start + HIDDEN_COMPLETION_PREFIX.len();
    for (offset, ch) in line[end..].char_indices() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            end = start + HIDDEN_COMPLETION_PREFIX.len() + offset + ch.len_utf8();
        } else {
            return end;
        }
    }
    line.len()
}

fn expand_hidden_completion_token_range(line: &str, start: usize, end: usize) -> (usize, usize) {
    if start > 0 {
        let previous = line[..start].chars().next_back().unwrap();
        if previous.is_whitespace() {
            return (start - previous.len_utf8(), end);
        }
    }

    if end < line.len() {
        let next = line[end..].chars().next().unwrap();
        if next.is_whitespace() {
            return (start, end + next.len_utf8());
        }
    }

    (start, end)
}

fn effective_color(cli: &Cli) -> ColorMode {
    if cli.no_color {
        ColorMode::Never
    } else {
        cli.color
    }
}

fn apply_color(color: ColorMode) {
    match color {
        ColorMode::Auto => {}
        ColorMode::Always => {
            console::set_colors_enabled(true);
            console::set_colors_enabled_stderr(true);
        }
        ColorMode::Never => {
            console::set_colors_enabled(false);
            console::set_colors_enabled_stderr(false);
        }
    }
}

fn use_decorative_output(cli: &Cli) -> bool {
    !cli.quiet
        && !cli.json
        && !cli.no_color
        && cli.color != ColorMode::Never
        && io::stdout().is_terminal()
}

fn supports_json(command: &Commands) -> bool {
    wt::deprecated_start_replacement(command).is_some()
        || matches!(
            command,
            Commands::Version
                | Commands::List { .. }
                | Commands::Inspect { .. }
                | Commands::Workflow {
                    command: WorkflowCommand::List,
                }
                | Commands::Task {
                    command: TaskCommand::List,
                }
                | Commands::Agent {
                    command: AgentCommand::Status { .. },
                }
                | Commands::Agent {
                    command: AgentCommand::Watch { .. },
                }
                | Commands::Agent {
                    command: AgentCommand::WaitStats,
                }
                | Commands::Msg {
                    command: MsgCommand::List { .. }
                        | MsgCommand::Read { .. }
                        | MsgCommand::CheckInbox { .. },
                }
                | Commands::Doctor { .. }
                | Commands::Profile { .. }
        )
}

fn print_version(json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "name": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION"),
            })
        );
    } else {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    }
}

fn resolve_directory(current_dir: &Path, directory: Option<&Path>) -> Result<Option<PathBuf>> {
    let Some(directory) = directory else {
        return Ok(None);
    };

    let path = resolve_path(current_dir, directory);
    if !path.is_dir() {
        bail!("directory does not exist: {}", path.display());
    }
    Ok(Some(path))
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
