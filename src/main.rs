use anyhow::Result;
use clap::Parser;
use std::process;

use wt::cli::{Cli, Commands};
use wt::config::Config;
use wt::context::Ctx;
use wt::error::WtError;
use wt::runner::RealRunner;
use wt::services::git::GitService;
use wt::ui::TerminalUi;

fn main() {
    if let Err(e) = try_main() {
        if let Some(WtError::Cancelled) = e.downcast_ref::<WtError>() {
            process::exit(0);
        }
        eprintln!("\x1b[31mERROR:\x1b[0m {e:#}");
        process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let cli = Cli::parse();
    let runner = RealRunner;

    let git = GitService::new(&runner, None);
    let invocation_root = git.repo_root()?;
    let repo_root = git.canonical_repo_root()?;

    let config = if matches!(cli.command, Commands::Init { .. }) {
        Config::default()
    } else {
        Config::load(&repo_root)?
    };

    let ctx = Ctx::new(
        repo_root,
        invocation_root,
        config,
        Box::new(RealRunner),
        Box::new(TerminalUi),
    );

    wt::dispatch(&ctx, &cli.command)
}
