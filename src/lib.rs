pub mod agent_state;
pub mod agents;
pub mod cli;
pub mod commands;
pub mod config;
pub mod config_render;
pub mod context;
pub mod error;
pub mod local_ui;
pub mod messages;
pub mod names;
pub mod runner;
pub mod services;
pub mod setup;
pub mod storage;
pub mod task;
pub mod task_run;
pub mod template;
pub mod ui;
pub mod workflow;
pub mod worktree_naming;

use anyhow::Result;
use cli::{
    AgentCommand, AgentHookCommand, AgentHookInstallCommand, AgentHookUninstallCommand, Commands,
    ConfigCommand, HookAgent, HookAgentCommand, HooksCommand, MsgCommand, RunCommand, TaskCommand,
    WorkflowCommand,
};
use commands::agent_runtime::KnownAgentCli;
use context::Ctx;

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Version | Commands::Completion { .. } => Ok(()),
        Commands::DeprecatedIssue { .. } => {
            deprecated_start_command_error("wt issue", "wt run issue")
        }
        Commands::DeprecatedPr { .. } => deprecated_start_command_error("wt pr", "wt run pr"),
        Commands::DeprecatedNew { .. } => deprecated_start_command_error("wt new", "wt run branch"),
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
            TaskCommand::DeprecatedRun { .. } => {
                deprecated_start_command_error("wt task run", "wt run task")
            }
            TaskCommand::Publish { tasks } => commands::task_publish::run(ctx, tasks),
        },
        Commands::Workflow { command } => match command {
            WorkflowCommand::List => commands::workflow::list(ctx),
            WorkflowCommand::Archive { workflow } => commands::workflow::archive(ctx, workflow),
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
            WorkflowCommand::DeprecatedRun { .. } => {
                deprecated_start_command_error("wt workflow run", "wt run workflow")
            }
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
        Commands::Inspect { target, pr } => commands::inspect::run(
            ctx,
            target.as_deref(),
            commands::inspect::InspectOptions { pr: *pr },
        ),
        Commands::Agent { command } => match command {
            AgentCommand::Status { target } => commands::agent::status(ctx, target.as_deref()),
            AgentCommand::Watch {
                target,
                interval,
                timeout,
                heartbeat,
            } => commands::agent::watch(ctx, target.as_deref(), *interval, *timeout, *heartbeat),
            AgentCommand::WaitStats => commands::agent::wait_stats(ctx),
            AgentCommand::Hook { command } => match command {
                AgentHookCommand::Install { command } => match command {
                    AgentHookInstallCommand::Claude { agent } => {
                        commands::agent_hook::install_claude(ctx, agent.as_deref())
                    }
                    AgentHookInstallCommand::Codex { agent } => {
                        commands::agent_hook::install_codex(ctx, agent.as_deref())
                    }
                },
                AgentHookCommand::Uninstall { command } => match command {
                    AgentHookUninstallCommand::Claude { agent } => {
                        commands::agent_hook::uninstall_claude(ctx, agent.as_deref())
                    }
                    AgentHookUninstallCommand::Codex { agent } => {
                        commands::agent_hook::uninstall_codex(ctx, agent.as_deref())
                    }
                },
            },
        },
        Commands::Hooks { command } => match command {
            HooksCommand::Setup {
                agent,
                agent_option,
                yes: _,
            } => commands::install::install_selected(ctx, agent.or(*agent_option)),
            HooksCommand::Uninstall {
                agent,
                agent_option,
                yes: _,
            } => commands::install::uninstall_selected(ctx, agent.or(*agent_option)),
            HooksCommand::Codex { command } => match command {
                HookAgentCommand::Install { yes: _ } => {
                    commands::install::install_selected(ctx, Some(HookAgent::Codex))
                }
                HookAgentCommand::Uninstall { yes: _ } => {
                    commands::install::uninstall_selected(ctx, Some(HookAgent::Codex))
                }
            },
            HooksCommand::Claude { command } => match command {
                HookAgentCommand::Install { yes: _ } => {
                    commands::install::install_selected(ctx, Some(HookAgent::Claude))
                }
                HookAgentCommand::Uninstall { yes: _ } => {
                    commands::install::uninstall_selected(ctx, Some(HookAgent::Claude))
                }
            },
        },
        Commands::Install => commands::install::install(ctx),
        Commands::Uninstall => commands::install::uninstall(ctx),
        Commands::Codex { args } => {
            commands::agent_runtime::run_known(ctx, KnownAgentCli::Codex, args)
        }
        Commands::Claude { args } => {
            commands::agent_runtime::run_known(ctx, KnownAgentCli::Claude, args)
        }
        Commands::As { agent, command } => commands::agent_runtime::run_as(ctx, agent, command),
        Commands::Ui { port } => commands::ui::run(ctx, *port),
        Commands::Msg { command } => match command {
            MsgCommand::Send { to, scope, message } => {
                commands::msg::send(ctx, to, scope.as_deref(), message)
            }
            MsgCommand::List { agent } => commands::msg::list(ctx, agent),
            MsgCommand::Read { agent, message_id } => commands::msg::read(ctx, agent, message_id),
            MsgCommand::CheckInbox { agent } => commands::msg::check_inbox(ctx, agent.as_deref()),
        },
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

pub fn deprecated_start_replacement(command: &Commands) -> Option<(&'static str, &'static str)> {
    match command {
        Commands::DeprecatedIssue { .. } => Some(("wt issue", "wt run issue")),
        Commands::DeprecatedPr { .. } => Some(("wt pr", "wt run pr")),
        Commands::DeprecatedNew { .. } => Some(("wt new", "wt run branch")),
        Commands::Task { command } => deprecated_task_start_replacement(command),
        Commands::Workflow { command } => deprecated_workflow_start_replacement(command),
        _ => None,
    }
}

fn deprecated_task_start_replacement(
    command: &TaskCommand,
) -> Option<(&'static str, &'static str)> {
    match command {
        TaskCommand::DeprecatedRun { .. } => Some(("wt task run", "wt run task")),
        _ => None,
    }
}

fn deprecated_workflow_start_replacement(
    command: &WorkflowCommand,
) -> Option<(&'static str, &'static str)> {
    match command {
        WorkflowCommand::DeprecatedRun { .. } => Some(("wt workflow run", "wt run workflow")),
        _ => None,
    }
}

fn deprecated_start_command_error(old: &str, new: &str) -> Result<()> {
    anyhow::bail!(
        "`{old}` has moved. Use `{new}` to start workspace execution. The old command is not an alias."
    )
}
