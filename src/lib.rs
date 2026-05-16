pub mod agents;
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
pub mod workflow;
pub mod worktree_naming;

use anyhow::Result;
use cli::{BatchCommand, Commands, ConfigCommand, StackCommand, TaskCommand, WorkflowCommand};
use context::Ctx;

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Version | Commands::Completion { .. } => Ok(()),
        Commands::Issue {
            target,
            base,
            profile,
            matrix,
        } => commands::issue::run(ctx, target.as_deref(), base, profile.as_deref(), *matrix),
        Commands::Pr { numbers, profile } => commands::pr::run(ctx, numbers, profile.as_deref()),
        Commands::New {
            name,
            task,
            base,
            profile,
            matrix,
        } => commands::new::run(ctx, name, task, base, profile.as_deref(), *matrix),
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
            BatchCommand::Run { batch, jobs } => commands::batch::run(ctx, batch.as_deref(), *jobs),
            BatchCommand::Show { batch } => commands::batch::show(ctx, batch.as_deref()),
            BatchCommand::Edit { batch } => commands::batch::edit(ctx, batch.as_deref()),
            BatchCommand::Clean { batch } => commands::batch::clean(ctx, batch.as_deref()),
        },
        Commands::Task { command } => match command {
            TaskCommand::Publish { tasks } => commands::task_publish::run(ctx, tasks),
        },
        Commands::Workflow { command } => match command {
            WorkflowCommand::Task {
                tasks,
                mode,
                profile,
                base,
                pull_request,
            } => {
                commands::workflow::task(ctx, tasks, *mode, profile.as_deref(), base, *pull_request)
            }
            WorkflowCommand::Issue {
                issues,
                mode,
                profile,
                base,
                pull_request,
            } => commands::workflow::issue(
                ctx,
                issues,
                *mode,
                profile.as_deref(),
                base,
                *pull_request,
            ),
            WorkflowCommand::Run { workflow } => commands::workflow::run(ctx, workflow.as_deref()),
            WorkflowCommand::Show { workflow } => {
                commands::workflow::show(ctx, workflow.as_deref())
            }
            WorkflowCommand::Edit { workflow } => {
                commands::workflow::edit(ctx, workflow.as_deref())
            }
            WorkflowCommand::Complete {
                workflow,
                task,
                run_next,
            } => commands::workflow::complete(ctx, workflow, task.as_deref(), *run_next),
        },
        Commands::Stack { command } => match command {
            StackCommand::Task {
                tasks,
                profile,
                base,
                pull_request,
            } => commands::stack::task(ctx, tasks, profile.as_deref(), base, *pull_request),
            StackCommand::Issue {
                issues,
                profile,
                base,
                pull_request,
            } => commands::stack::issue(ctx, issues, profile.as_deref(), base, *pull_request),
            StackCommand::Run { stack } => commands::stack::run(ctx, stack.as_deref()),
            StackCommand::Show { stack } => commands::stack::show(ctx, stack.as_deref()),
            StackCommand::Edit { stack } => commands::stack::edit(ctx, stack.as_deref()),
            StackCommand::Complete {
                stack,
                task,
                run_next,
            } => commands::stack::complete(ctx, stack, task.as_deref(), *run_next),
        },
        Commands::List { wide } => commands::list::run(ctx, *wide),
        Commands::Open { target } => commands::open::run(ctx, target.as_deref()),
        Commands::Done { targets } => commands::done::run(ctx, targets),
        Commands::Review { target } => commands::review::run(ctx, target.as_deref()),
        Commands::Status { target } => commands::status::run(ctx, target),
        Commands::Send {
            target,
            message,
            no_enter,
        } => commands::send::run(ctx, target, message, *no_enter),
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
            preset,
            minimal,
            agent,
            agent_args,
            agent_command,
            issue_provider,
            site_provider,
            gh_user,
            yes,
            dry_run,
            force,
        } => commands::init::run(
            ctx,
            commands::init::InitOptions {
                local: *local,
                shared: *shared,
                preset: *preset,
                minimal: *minimal,
                agent: agent.clone(),
                agent_args: agent_args.clone(),
                agent_command: agent_command.clone(),
                issue_provider: issue_provider.clone(),
                site_provider: site_provider.clone(),
                gh_user: gh_user.clone(),
                yes: *yes,
                dry_run: *dry_run,
                force: *force,
            },
        ),
    }
}
