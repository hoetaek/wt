pub mod cli;
pub mod commands;
pub mod config;
pub mod config_render;
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
use cli::{BatchCommand, Commands, ConfigCommand, StackCommand};
use context::Ctx;

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Version | Commands::Completion { .. } => Ok(()),
        Commands::Issue {
            target,
            base,
            profile,
            parallel,
        } => commands::issue::run(ctx, target.as_deref(), base, profile.as_deref(), *parallel),
        Commands::Pr { number, profile } => commands::pr::run(ctx, *number, profile.as_deref()),
        Commands::New {
            name,
            base,
            profile,
            parallel,
        } => commands::new::run(ctx, name, base, profile.as_deref(), *parallel),
        Commands::Batch { command } => match command {
            BatchCommand::Task {
                tasks,
                profile,
                base,
            } => commands::batch::task(ctx, tasks, profile.as_deref(), base),
            BatchCommand::Issue {
                issues,
                profile,
                base,
            } => commands::batch::issue(ctx, issues, profile.as_deref(), base),
            BatchCommand::Run { batch } => commands::batch::run(ctx, batch),
            BatchCommand::Show { batch } => commands::batch::show(ctx, batch.as_deref()),
            BatchCommand::Edit { batch } => commands::batch::edit(ctx, batch.as_deref()),
        },
        Commands::Stack { command } => match command {
            StackCommand::Task {
                tasks,
                profile,
                base,
            } => commands::stack::task(ctx, tasks, profile.as_deref(), base),
            StackCommand::Issue {
                issues,
                profile,
                base,
            } => commands::stack::issue(ctx, issues, profile.as_deref(), base),
            StackCommand::Run { stack } => commands::stack::run(ctx, stack),
            StackCommand::Show { stack } => commands::stack::show(ctx, stack.as_deref()),
            StackCommand::Edit { stack } => commands::stack::edit(ctx, stack.as_deref()),
            StackCommand::Complete {
                stack,
                item,
                run_next,
            } => commands::stack::complete(ctx, stack, item.as_deref(), *run_next),
        },
        Commands::List { wide } => commands::list::run(ctx, *wide),
        Commands::Open { target } => commands::open::run(ctx, target.as_deref()),
        Commands::Done { targets } => commands::done::run(ctx, targets),
        Commands::Doctor => commands::doctor::run(ctx),
        Commands::Config { profile, command } => match command {
            Some(ConfigCommand::Edit { source }) => {
                commands::config::edit(ctx, profile.as_deref(), source.as_deref())
            }
            Some(ConfigCommand::Extract { source }) => {
                commands::config::extract(ctx, profile.as_deref(), source.as_deref())
            }
            Some(ConfigCommand::Inline { source }) => {
                commands::config::inline(ctx, profile.as_deref(), source.as_deref())
            }
            None => commands::config::effective(ctx, profile.as_deref()),
        },
        Commands::Profile { command } => commands::profile::run(ctx, command.as_ref()),
        Commands::Site { command } => commands::site::run(ctx, command),
        Commands::Init {
            local,
            shared,
            agent,
            agent_args,
            agent_command,
            issue_provider,
            site_provider,
            gh_user,
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
                yes: *yes,
                force: *force,
            },
        ),
    }
}
