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

use anyhow::{Result, bail};
use cli::{AgentCommand, Commands, ConfigCommand, TaskCommand, WorkflowCommand};
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
            base,
            profile,
            matrix,
        } => commands::new::run(ctx, name, base, profile.as_deref(), *matrix),
        Commands::Batch { .. } => legacy_batch_command_error(),
        Commands::Task { command } => match command {
            TaskCommand::Run {
                tasks,
                base,
                profile,
                matrix,
            } => commands::task_run_command::run(ctx, tasks, base, profile.as_deref(), *matrix),
            TaskCommand::Publish { tasks } => commands::task_publish::run(ctx, tasks),
        },
        Commands::Workflow { command } => match command {
            WorkflowCommand::Task {
                tasks,
                mode,
                profile,
                base,
                pr,
            } => commands::workflow::task(ctx, tasks, *mode, profile.as_deref(), base, *pr),
            WorkflowCommand::Issue {
                issues,
                mode,
                profile,
                base,
                pr,
            } => commands::workflow::issue(ctx, issues, *mode, profile.as_deref(), base, *pr),
            WorkflowCommand::Run { workflow, jobs } => {
                commands::workflow::run(ctx, workflow.as_deref(), *jobs)
            }
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
        Commands::Stack { .. } => legacy_stack_command_error(),
        Commands::List { wide } => commands::list::run(ctx, *wide),
        Commands::Open { target } => commands::open::run(ctx, target.as_deref()),
        Commands::Done { targets } => commands::done::run(ctx, targets),
        Commands::Inspect { target } => commands::review::run(ctx, target.as_deref()),
        Commands::Review { .. } => legacy_review_command_error(),
        Commands::Agent { command } => match command {
            AgentCommand::Status { target } => commands::agent::status(ctx, target.as_deref()),
            AgentCommand::Watch { target, interval } => {
                commands::agent::watch(ctx, target.as_deref(), *interval)
            }
        },
        Commands::Status { .. } => legacy_status_command_error(),
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

fn legacy_batch_command_error() -> Result<()> {
    bail!(
        "wt batch has been replaced by wt workflow --mode batch. Use `wt workflow task ... --mode batch`, `wt workflow issue ... --mode batch`, `wt workflow run <workflow> --jobs <n>`, and `wt workflow show <workflow>`. Existing .local/batches files are old migration context, not the canonical state surface."
    )
}

fn legacy_stack_command_error() -> Result<()> {
    bail!(
        "wt stack has been replaced by wt workflow --mode stack. Use `wt workflow task ... --mode stack`, `wt workflow issue ... --mode stack`, `wt workflow run <workflow>`, and `wt workflow complete <workflow> <task> [--run-next]`. Existing .local/stacks files are old migration context, not the canonical state surface."
    )
}

fn legacy_review_command_error() -> Result<()> {
    bail!(
        "wt review has been replaced by wt inspect. Use `wt inspect [<target>]` for the read-only work dossier, then complete, land, or clean up explicitly when appropriate."
    )
}

fn legacy_status_command_error() -> Result<()> {
    bail!(
        "wt status has been replaced by wt agent status and wt agent watch. Use `wt agent status <target>` to observe a task agent once, `wt agent watch <target>` to poll, or `wt inspect [<target>]` for the read-only work dossier."
    )
}
