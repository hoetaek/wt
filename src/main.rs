use anyhow::{Context, Result, bail};
use clap::{Command as ClapCommand, CommandFactory, Parser};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process;

use wt::cli::{AgentCommand, Cli, ColorMode, Commands, TaskCommand, WorkflowCommand};
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
            "JSON output is supported for: wt version, wt list, wt task list, wt workflow list, wt agent status, wt agent watch, wt doctor, wt profile"
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
            io::stdout().write_all(strip_removed_completion_entries(&script).as_bytes())?;
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
    let config_base = working_dir.as_deref().unwrap_or(&current_dir);

    let (base_config, config, config_source) = if matches!(command, Commands::Init { .. }) {
        (Config::default(), Config::default(), ConfigSource::Default)
    } else if let Some(path) = cli.config.as_deref() {
        let path = resolve_path(config_base, path);
        let (base_config, config) = Config::load_file_for_repo(&path, &repo_root)
            .with_context(|| format!("failed to load config: {}", path.display()))?;
        (base_config, config, ConfigSource::File(path))
    } else {
        Config::load_base_and_effective_with_source(&repo_root)?
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

const HIDDEN_COMPLETION_NAMES: [&str; 16] = [
    "__wt_removed_0",
    "__wt_removed_1",
    "__wt_removed_2",
    "__wt_removed_3",
    "__wt_removed_4",
    "__wt_removed_5",
    "__wt_removed_6",
    "__wt_removed_7",
    "__wt_removed_8",
    "__wt_removed_9",
    "__wt_removed_10",
    "__wt_removed_11",
    "__wt_removed_12",
    "__wt_removed_13",
    "__wt_removed_14",
    "__wt_removed_15",
];

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
            let hidden_name = HIDDEN_COMPLETION_NAMES
                .get(*hidden_index)
                .copied()
                .unwrap_or("__wt_removed_extra");
            *hidden_index += 1;
            subcommand.name(hidden_name)
        } else {
            subcommand
        }
    })
}

fn strip_removed_completion_entries(script: &str) -> String {
    let mut output = String::new();
    let mut skipping_removed_block = false;

    for line in script.lines() {
        let trimmed = line.trim_start();
        if !skipping_removed_block
            && HIDDEN_COMPLETION_NAMES
                .iter()
                .any(|name| trimmed.contains(name))
            && trimmed.ends_with(')')
        {
            skipping_removed_block = true;
            continue;
        }

        if skipping_removed_block {
            if trimmed == ";;" {
                skipping_removed_block = false;
            }
            continue;
        }

        let cleaned = strip_removed_completion_tokens(line);
        if cleaned.trim().is_empty()
            && HIDDEN_COMPLETION_NAMES
                .iter()
                .any(|name| line.contains(name))
        {
            continue;
        }
        output.push_str(&cleaned);
        output.push('\n');
    }

    output
}

fn strip_removed_completion_tokens(line: &str) -> String {
    let mut cleaned = line.to_string();
    for name in HIDDEN_COMPLETION_NAMES {
        cleaned = cleaned.replace(&format!(" {name}"), "");
        cleaned = cleaned.replace(&format!("{name} "), "");
        cleaned = cleaned.replace(name, "");
    }
    cleaned
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
