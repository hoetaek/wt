pub mod cli;
pub mod commands;
pub mod config;
pub mod context;
pub mod error;
pub mod names;
pub mod runner;
pub mod services;
pub mod setup;
pub mod template;
pub mod ui;

use anyhow::Result;
use cli::Commands;
use context::Ctx;

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Issue { number, base } => commands::issue::run(ctx, *number, base),
        Commands::Pr { number } => commands::pr::run(ctx, *number),
        Commands::New { name, base } => commands::new::run(ctx, name, base),
        Commands::Open => commands::open::run(ctx),
        Commands::Clean => commands::clean::run(ctx),
    }
}
