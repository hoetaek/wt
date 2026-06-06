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
pub mod origin_snapshot;
pub(crate) mod parallel;
pub(crate) mod personal_storage;
pub mod runner;
pub mod scaffold;
pub mod services;
pub mod setup;
pub mod storage;
pub mod studio;
pub mod task;
pub mod task_run;
pub mod template;
pub mod ui;
pub mod workflow;
pub mod worktree_naming;

use anyhow::{Result, bail};
use cli::{
    AgentCommand, AgentSupervisorCommand, Commands, ConfigCommand, MsgCommand, RunCommand,
    SessionCommand, TaskCommand, TaskOriginCommand, WorkflowCommand, WorkflowOriginCommand,
};
use commands::agent_runtime::KnownAgentCli;
use context::{Ctx, MachineCtx};

pub fn dispatch_machine(ctx: &MachineCtx<'_>, command: &Commands) -> Result<()> {
    match command {
        Commands::Setup {
            yes,
            dry_run,
            remove,
        } => commands::setup::run(
            ctx,
            commands::setup::SetupOptions {
                yes: *yes,
                dry_run: *dry_run,
                remove: *remove,
            },
        ),
        _ => bail!("command does not support per-machine context"),
    }
}

pub fn dispatch(ctx: &Ctx, command: &Commands) -> Result<()> {
    match command {
        Commands::Version
        | Commands::Completion { .. }
        | Commands::ShellInit { .. }
        | Commands::Env => Ok(()),
        Commands::Session { command } => match command {
            SessionCommand::Set { id } => commands::session::set(ctx, id),
            SessionCommand::Unset => commands::session::unset(ctx),
            SessionCommand::Show { json } => commands::session::show(ctx, ctx.is_json() || *json),
        },
        Commands::DeprecatedIssue { .. } => {
            deprecated_start_command_error("wt issue", "wt run issue")
        }
        Commands::DeprecatedPr { .. } => deprecated_start_command_error("wt pr", "wt run pr"),
        Commands::DeprecatedNew { .. } => deprecated_start_command_error("wt new", "wt run branch"),
        Commands::Run { command } => {
            personal_storage::ensure_launch_ready(
                ctx.runner.as_ref(),
                &ctx.storage_root,
                &ctx.repo_root,
            )?;
            match command {
                RunCommand::Issue {
                    targets,
                    base,
                    profile,
                    matrix,
                    jobs,
                } => commands::issue::run(ctx, targets, base, profile.as_deref(), *matrix, *jobs),
                RunCommand::Pr {
                    numbers,
                    profile,
                    jobs,
                } => commands::pr::run(ctx, numbers, profile.as_deref(), *jobs),
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
                    jobs,
                } => commands::task_run_command::run(ctx, tasks, base, profile.as_deref(), *jobs),
                RunCommand::Workflow { workflow, jobs } => {
                    commands::workflow::run(ctx, workflow.as_deref(), *jobs)
                }
            }
        }
        Commands::Task { command } => match command {
            TaskCommand::List { all } => commands::task_list::run(ctx, *all),
            TaskCommand::Origin { command } => match command {
                TaskOriginCommand::Import { issues } => commands::task_origin::import(ctx, issues),
                TaskOriginCommand::Publish { tasks } => commands::task_origin::publish(ctx, tasks),
                TaskOriginCommand::Attach { task, issue } => {
                    commands::task_origin::attach(ctx, task, issue)
                }
                TaskOriginCommand::Fetch { tasks } => commands::task_origin::fetch(ctx, tasks),
                TaskOriginCommand::Diff { tasks } => commands::task_origin::diff(ctx, tasks),
                TaskOriginCommand::Pull { tasks } => commands::task_origin::pull(ctx, tasks),
                TaskOriginCommand::Push { tasks } => commands::task_origin::push(ctx, tasks),
            },
            TaskCommand::Import { issues } => commands::task_origin::import(ctx, issues),
            TaskCommand::DeprecatedRun { .. } => {
                deprecated_start_command_error("wt task run", "wt run task")
            }
            TaskCommand::Publish { tasks } => commands::task_origin::publish(ctx, tasks),
            TaskCommand::Report { message } => commands::task_report::run(ctx, message),
            TaskCommand::Review {
                task_run_id,
                accept,
                reject,
                block,
                codex_base,
                message,
            } => {
                let status = match (*accept, *reject, *block) {
                    (true, false, false) => task_run::REVIEW_ACCEPTED,
                    (false, true, false) => task_run::REVIEW_REJECTED,
                    (false, false, true) => task_run::REVIEW_BLOCKED,
                    _ => bail!("Pass exactly one of --accept, --reject, or --block"),
                };
                commands::task_review::run(ctx, task_run_id, status, codex_base.as_deref(), message)
            }
        },
        Commands::Workflow { command } => match command {
            WorkflowCommand::List => commands::workflow::list(ctx),
            WorkflowCommand::Archive { workflow } => commands::workflow::archive(ctx, workflow),
            WorkflowCommand::Task {
                tasks,
                mode,
                profile,
                profiles,
                coordinator,
                id,
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
                    coordinator: coordinator.as_deref(),
                    id: id.as_deref(),
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
                id,
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
                    id: id.as_deref(),
                    title: title.as_deref(),
                    body: body.as_deref(),
                    body_file: body_file.as_deref(),
                    origin_provider: origin_provider.as_deref(),
                    origin_id: origin_id.as_deref(),
                    base,
                    pr: *pr,
                },
            ),
            WorkflowCommand::Origin { command } => match command {
                WorkflowOriginCommand::Attach { workflow, issue } => {
                    commands::workflow::origin_attach(ctx, workflow, issue)
                }
                WorkflowOriginCommand::Fetch { workflows } => {
                    commands::workflow::origin_fetch(ctx, workflows)
                }
                WorkflowOriginCommand::Diff { workflows } => {
                    commands::workflow::origin_diff(ctx, workflows)
                }
                WorkflowOriginCommand::Pull { workflows } => {
                    commands::workflow::origin_pull(ctx, workflows)
                }
                WorkflowOriginCommand::Push { workflows } => {
                    commands::workflow::origin_push(ctx, workflows)
                }
            },
            WorkflowCommand::DeprecatedRun { .. } => {
                deprecated_start_command_error("wt workflow run", "wt run workflow")
            }
            WorkflowCommand::Show { workflow } => {
                commands::workflow::show(ctx, workflow.as_deref())
            }
            WorkflowCommand::Watch {
                workflow,
                interval,
                timeout,
                heartbeat,
            } => {
                commands::workflow::watch(ctx, workflow.as_deref(), *interval, *timeout, *heartbeat)
            }
            WorkflowCommand::Edit { workflow } => {
                commands::workflow::edit(ctx, workflow.as_deref())
            }
            WorkflowCommand::Repair { workflow, apply } => {
                commands::workflow::repair(ctx, workflow, *apply)
            }
            WorkflowCommand::Pass {
                workflow,
                task,
                run_next,
            } => commands::workflow::pass(ctx, workflow, task.as_deref(), *run_next),
            WorkflowCommand::Complete {
                workflow,
                task,
                run_next,
            } => commands::workflow::deprecated_complete(workflow, task.as_deref(), *run_next),
        },
        Commands::Scaffold {
            feature,
            idea,
            spec,
            task,
            workflow,
            retrospect,
            all,
            force,
        } => commands::scaffold::run(
            ctx,
            feature,
            commands::scaffold::ScaffoldFlags {
                idea: *idea,
                spec: *spec,
                task: *task,
                workflow: *workflow,
                retrospect: *retrospect,
                all: *all,
                force: *force,
            },
        ),
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
            AgentCommand::Supervisor { command } => match command {
                AgentSupervisorCommand::Start {
                    agent_id,
                    replace,
                    surface,
                    kind,
                    cleanup_on_session_end,
                    stale_threshold,
                    poll_interval,
                } => commands::agent::supervisor::start(
                    ctx,
                    agent_id,
                    commands::agent::supervisor::StartOptions {
                        replace: *replace,
                        surface: surface.clone(),
                        kind: kind.clone(),
                        cleanup_on_session_end: *cleanup_on_session_end,
                        stale_threshold: stale_threshold.clone(),
                        poll_interval: poll_interval.clone(),
                    },
                ),
                AgentSupervisorCommand::Stop { agent_id, owned_by } => {
                    commands::agent::supervisor::stop(
                        ctx,
                        commands::agent::supervisor::StopOptions {
                            agent_id: agent_id.clone(),
                            owned_by: owned_by.clone(),
                        },
                    )
                }
                AgentSupervisorCommand::Status { agent_id } => {
                    commands::agent::supervisor::status(ctx, agent_id.as_deref())
                }
                AgentSupervisorCommand::Logs { agent_id, follow } => {
                    commands::agent::supervisor::logs(
                        ctx,
                        agent_id,
                        commands::agent::supervisor::LogsOptions { follow: *follow },
                    )
                }
                AgentSupervisorCommand::Run {
                    agent_id,
                    foreground,
                    surface,
                    kind,
                    cleanup_on_session_end,
                    stale_threshold_secs,
                    poll_interval_secs,
                    cycle_cap,
                    payload_cap,
                    log_path,
                } => commands::agent::supervisor::run(
                    ctx,
                    agent_id,
                    commands::agent::supervisor::RunOptions {
                        foreground: *foreground,
                        surface: surface.clone(),
                        kind: kind.clone(),
                        cleanup_on_session_end: *cleanup_on_session_end,
                        stale_threshold_secs: *stale_threshold_secs,
                        poll_interval_secs: *poll_interval_secs,
                        cycle_cap: *cycle_cap,
                        payload_cap: *payload_cap,
                        log_path: log_path.clone(),
                    },
                ),
            },
        },
        Commands::Setup {
            yes,
            dry_run,
            remove,
        } => {
            let machine_ctx = ctx.machine_ctx();
            commands::setup::run(
                &machine_ctx,
                commands::setup::SetupOptions {
                    yes: *yes,
                    dry_run: *dry_run,
                    remove: *remove,
                },
            )
        }
        Commands::Codex { args } => {
            commands::agent_runtime::run_known(ctx, KnownAgentCli::Codex, args)
        }
        Commands::Claude { args } => {
            commands::agent_runtime::run_known(ctx, KnownAgentCli::Claude, args)
        }
        Commands::As { agent, command } => commands::agent_runtime::run_as(ctx, agent, command),
        Commands::Ui { port } => commands::ui::run(ctx, *port),
        Commands::Studio {
            port,
            dev,
            dev_origin,
        } => commands::studio::run(ctx, *port, *dev, dev_origin.clone()),
        Commands::Msg { command } => match command {
            MsgCommand::Send { to, scope, message } => {
                commands::msg::send(ctx, to, scope.as_deref(), message)
            }
            MsgCommand::List { agent } => commands::msg::list(ctx, agent),
            MsgCommand::Read { agent, message_id } => commands::msg::read(ctx, agent, message_id),
            MsgCommand::CheckInbox {
                agent,
                hook_event_name,
                silent: _,
            } => commands::msg::check_inbox(ctx, agent.as_deref(), hook_event_name.as_deref()),
            MsgCommand::Watch {
                agent,
                timeout,
                json,
            } => commands::msg::watch(
                ctx,
                agent.as_deref(),
                std::time::Duration::from_secs(*timeout),
                ctx.is_json() || *json,
            ),
        },
        Commands::Send {
            target,
            message,
            no_enter,
        } => commands::send::run(ctx, target, message, *no_enter),
        Commands::Doctor {
            profile,
            prune_env_anchors,
        } => commands::doctor::run(ctx, profile.as_deref(), prune_env_anchors.as_deref()),
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
            dry_run,
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
