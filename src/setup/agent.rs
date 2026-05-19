use crate::config::{AgentCli, AgentConfig, SubmitMode};
use crate::context::Ctx;
use crate::services::cmux::CmuxService;
use crate::template;
use anyhow::{Result, bail};
use std::collections::HashMap;

const WT_AGENT_ID_TEMPLATE_KEY: &str = "wt_agent_id";

pub(crate) fn agent_launch_command(
    agent: Option<&AgentConfig>,
    vars: &HashMap<String, String>,
) -> Result<String> {
    let Some(agent) = agent else {
        return Ok(String::new());
    };
    let Some(command) = agent.command_line_with_vars(Some(vars))? else {
        return Ok(String::new());
    };

    Ok(inject_agent_identity_env(agent, command, vars))
}

fn inject_agent_identity_env(
    agent: &AgentConfig,
    command: String,
    vars: &HashMap<String, String>,
) -> String {
    if agent.cli == AgentCli::None || command.trim().is_empty() {
        return command;
    }

    let Some(agent_id) = vars
        .get(WT_AGENT_ID_TEMPLATE_KEY)
        .map(String::as_str)
        .filter(|agent_id| !agent_id.trim().is_empty())
    else {
        return command;
    };

    format!("export WT_AGENT_ID={}; {command}", shell_arg(agent_id))
}

pub(super) fn bootstrap_agent(
    ctx: &Ctx,
    ws_handle: &str,
    agent: &AgentConfig,
    mode: &str,
    vars: &HashMap<String, String>,
) -> Result<()> {
    let prompts = match agent.prompt.get(mode) {
        Some(prompts) if !prompts.is_empty() => prompts,
        _ => return Ok(()),
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let panes = cmux.list_panes(ws_handle)?;
    let pane = match panes.first() {
        Some(pane) => pane,
        None => bail!(
            "Agent prompt 1/{} failed: no cmux pane found in workspace {}",
            prompts.len(),
            ws_handle
        ),
    };
    let surfaces = cmux.list_pane_surfaces(pane, ws_handle)?;
    let surface = match surfaces.first() {
        Some(surface) => surface,
        None => bail!(
            "Agent prompt 1/{} failed: no cmux surface found for pane {} in workspace {}",
            prompts.len(),
            pane,
            ws_handle
        ),
    };
    let mut vars = vars.clone();
    vars.insert("task_agent_cmux_workspace".into(), ws_handle.into());
    vars.insert("task_agent_cmux_surface".into(), surface.into());

    let ready_marker = agent.effective_ready();

    for (i, prompt_template) in prompts.iter().enumerate() {
        if i > 0 {
            let stale_screen = cmux.read_screen(surface, ws_handle).map_err(|err| {
                anyhow::anyhow!(
                    "Agent prompt {}/{} failed: screen read failed before delivery: {err:#}",
                    i + 1,
                    prompts.len()
                )
            })?;
            let mut screen_changed = false;
            for attempt in 0..agent.timeout {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                if let Ok(current) = cmux.read_screen(surface, ws_handle) {
                    if current != stale_screen {
                        screen_changed = true;
                        break;
                    }
                }
            }
            if !screen_changed {
                bail!(
                    "Agent prompt {}/{} failed: unchanged screen before delivery",
                    i + 1,
                    prompts.len()
                );
            }
        }

        if let Some(marker) = &ready_marker {
            ctx.ui.print_step(&format!(
                "Waiting for agent ready marker '{}' ({}s timeout)...",
                marker, agent.timeout
            ));

            let mut ready = false;
            for attempt in 0..agent.timeout {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                if let Ok(screen) = cmux.read_screen(surface, ws_handle) {
                    if screen.contains(marker) {
                        ready = true;
                        break;
                    }
                }
            }

            if !ready {
                bail!(
                    "Agent prompt {}/{} failed: ready marker timeout waiting for '{}'",
                    i + 1,
                    prompts.len(),
                    marker
                );
            }
        } else if agent.send_after > 0 {
            ctx.ui.print_step(&format!(
                "Waiting {}s before agent prompt...",
                agent.send_after
            ));
            std::thread::sleep(std::time::Duration::from_secs(agent.send_after));
        }

        let rendered = template::render(prompt_template, &vars);
        send_agent_prompt(&cmux, surface, ws_handle, agent, rendered)?;
        ctx.ui
            .print_step(&format!("Agent prompt {}/{} sent", i + 1, prompts.len()));
    }

    Ok(())
}

fn send_agent_prompt(
    cmux: &CmuxService,
    surface: &str,
    ws_handle: &str,
    agent: &AgentConfig,
    rendered: String,
) -> Result<()> {
    if should_submit_with_enter_key(agent) {
        let prompt = rendered.trim_end_matches(['\n', '\r']).to_string();
        cmux.send(surface, ws_handle, &prompt)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        cmux.send_key(surface, ws_handle, "enter")?;
        return Ok(());
    }

    let prompt = agent.apply_submit_suffix(rendered);
    cmux.send(surface, ws_handle, &prompt)
}

fn should_submit_with_enter_key(agent: &AgentConfig) -> bool {
    matches!(
        (&agent.submit, &agent.cli),
        (
            SubmitMode::Auto,
            AgentCli::Codex | AgentCli::Claude | AgentCli::Gemini,
        ) | (SubmitMode::CarriageReturn, _)
    )
}

fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}
