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
pub mod worktree_naming;

use anyhow::Result;
use cli::{BatchCommand, Commands};
use context::Ctx;

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Version | Commands::Completion { .. } => Ok(()),
        Commands::Start {
            target,
            base,
            profile,
            parallel,
        } => commands::start::run(ctx, target, base, profile.as_deref(), *parallel),
        Commands::Batch { command } => match command {
            BatchCommand::Prepare {
                issues,
                profile,
                base,
            } => commands::batch::prepare(ctx, issues, profile.as_deref(), base),
            BatchCommand::Run { batch } => commands::batch::run(ctx, batch),
        },
        Commands::List => commands::list::run(ctx),
        Commands::Open { target } => commands::open::run(ctx, target.as_deref()),
        Commands::Done { targets } => commands::done::run(ctx, targets),
        Commands::Doctor => commands::doctor::run(ctx),
        Commands::Profile { name } => commands::profile::run(ctx, name.as_deref()),
        Commands::Traefik { command } => commands::traefik::run(ctx, command),
        Commands::Init {
            local,
            shared,
            agent,
            agent_args,
            agent_command,
            issue_provider,
            site_provider,
            gh_user,
            prompts,
            no_prompts,
            yes,
            force,
        } => commands::init::run(
            ctx,
            commands::init::InitOptions {
                local: *local,
                shared: *shared,
                agent: agent.clone(),
                agent_args: agent_args.clone(),
                agent_command: agent_command.clone(),
                issue_provider: issue_provider.clone(),
                site_provider: site_provider.clone(),
                gh_user: gh_user.clone(),
                prompts: *prompts,
                no_prompts: *no_prompts,
                yes: *yes,
                force: *force,
            },
        ),
    }
}
