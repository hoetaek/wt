pub mod agents;
pub mod cli;
pub mod commands;
pub mod config;
pub mod config_render;
pub mod context;
pub mod error;
pub mod local_ui;
pub mod names;
pub mod runner;
pub mod services;
pub mod setup;
pub mod task;
pub mod task_run;
pub mod template;
pub mod ui;
pub mod workflow;
pub mod worktree_naming;

use anyhow::Result;
use cli::{AgentCommand, Commands, ConfigCommand, RunCommand, TaskCommand, WorkflowCommand};
use context::Ctx;

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Version | Commands::Completion { .. } => Ok(()),
        Commands::Run { command } => match command {
            RunCommand::Issue {
                target,
                base,
                profile,
                matrix,
            } => commands::issue::run(ctx, target.as_deref(), base, profile.as_deref(), *matrix),
            RunCommand::Pr { numbers, profile } => {
                commands::pr::run(ctx, numbers, profile.as_deref())
            }
            RunCommand::Branch {
                name,
                base,
                profile,
                matrix,
            } => commands::new::run(ctx, name, base, profile.as_deref(), *matrix),
            RunCommand::Task {
                tasks,
                base,
                profile,
            } => commands::task_run_command::run(ctx, tasks, base, profile.as_deref()),
            RunCommand::Workflow { workflow, jobs } => {
                commands::workflow::run(ctx, workflow.as_deref(), *jobs)
            }
        },
        Commands::Task { command } => match command {
            TaskCommand::List => commands::task_list::run(ctx),
            TaskCommand::Import { issues } => commands::task::import(ctx, issues),
            TaskCommand::Publish { tasks } => commands::task_publish::run(ctx, tasks),
        },
        Commands::Workflow { command } => match command {
            WorkflowCommand::List => commands::workflow::list(ctx),
            WorkflowCommand::Task {
                tasks,
                mode,
                profile,
                profiles,
                title,
                body,
                body_file,
                origin_provider,
                origin_id,
                base,
                pr,
            } => commands::workflow::task(
                ctx,
                tasks,
                commands::workflow::TaskOptions {
                    mode: *mode,
                    profile: profile.as_deref(),
                    profiles,
                    title: title.as_deref(),
                    body: body.as_deref(),
                    body_file: body_file.as_deref(),
                    origin_provider: origin_provider.as_deref(),
                    origin_id: origin_id.as_deref(),
                    base,
                    pr: *pr,
                },
            ),
            WorkflowCommand::Issue {
                issues,
                mode,
                profile,
                title,
                body,
                body_file,
                origin_provider,
                origin_id,
                base,
                pr,
            } => commands::workflow::issue(
                ctx,
                issues,
                commands::workflow::IssueOptions {
                    mode: *mode,
                    profile: profile.as_deref(),
                    title: title.as_deref(),
                    body: body.as_deref(),
                    body_file: body_file.as_deref(),
                    origin_provider: origin_provider.as_deref(),
                    origin_id: origin_id.as_deref(),
                    base,
                    pr: *pr,
                },
            ),
            WorkflowCommand::Show { workflow } => {
                commands::workflow::show(ctx, workflow.as_deref())
            }
            WorkflowCommand::Edit { workflow } => {
                commands::workflow::edit(ctx, workflow.as_deref())
            }
            WorkflowCommand::Repair { workflow, apply } => {
                commands::workflow::repair(ctx, workflow, *apply)
            }
            WorkflowCommand::Complete {
                workflow,
                task,
                run_next,
            } => commands::workflow::complete(ctx, workflow, task.as_deref(), *run_next),
        },
        Commands::List { wide } => commands::list::run(ctx, *wide),
        Commands::Open { target } => commands::open::run(ctx, target.as_deref()),
        Commands::Done { targets } => commands::done::run(ctx, targets),
        Commands::Inspect { target } => commands::inspect::run(ctx, target.as_deref()),
        Commands::Agent { command } => match command {
            AgentCommand::Status { target } => commands::agent::status(ctx, target.as_deref()),
            AgentCommand::Watch {
                target,
                interval,
                timeout,
                heartbeat,
            } => commands::agent::watch(ctx, target.as_deref(), *interval, *timeout, *heartbeat),
        },
        Commands::Ui { port } => commands::ui::run(ctx, *port),
        Commands::Send {
            target,
            message,
            no_enter,
        } => commands::send::run(ctx, target, message, *no_enter),
        Commands::Doctor { profile } => commands::doctor::run(ctx, profile.as_deref()),
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
