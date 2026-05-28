use crate::config::{AgentCli, AgentConfig, SubmitMode};
use crate::context::Ctx;
use crate::services::cmux::{CmuxService, PASTE_SUBMIT_SETTLE, unique_cmux_buffer_name};
use crate::services::cmux_push::{CODEX_IN_PROMPT_NEWLINE_KEY, codex_prompt_lines};
use crate::template;
use anyhow::{Result, bail};
use std::collections::HashMap;

const WT_AGENT_ID_TEMPLATE_KEY: &str = "wt_agent_id";
const WT_TASK_RUN_ID_TEMPLATE_KEY: &str = "wt_task_run_id";

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

    let mut exports = Vec::new();
    if let Some(agent_id) = vars
        .get(WT_AGENT_ID_TEMPLATE_KEY)
        .map(String::as_str)
        .filter(|agent_id| !agent_id.trim().is_empty())
    {
        exports.push(format!("WT_AGENT_ID={}", shell_arg(agent_id)));
    }
    if let Some(task_run_id) = vars
        .get(WT_TASK_RUN_ID_TEMPLATE_KEY)
        .map(String::as_str)
        .filter(|task_run_id| !task_run_id.trim().is_empty())
    {
        exports.push(format!("WT_TASK_RUN_ID={}", shell_arg(task_run_id)));
    }
    if exports.is_empty() {
        return command;
    }

    format!("export {}; {command}", exports.join(" "))
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
    if should_submit_codex_with_enter_key(agent) {
        let prompt = rendered.trim_end_matches(['\n', '\r']);
        return send_codex_prompt(cmux, surface, ws_handle, prompt);
    }

    if should_submit_claude_with_enter_key(agent) {
        let prompt = rendered.trim_end_matches(['\n', '\r']);
        return send_pasted_prompt_then_enter(cmux, surface, ws_handle, "wt-claude", prompt);
    }

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

fn send_codex_prompt(
    cmux: &CmuxService,
    surface: &str,
    ws_handle: &str,
    prompt: &str,
) -> Result<()> {
    let lines = codex_prompt_lines(prompt);
    for (i, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            cmux.send(surface, ws_handle, line)?;
        }
        if i + 1 < lines.len() {
            cmux.send_key(surface, ws_handle, CODEX_IN_PROMPT_NEWLINE_KEY)?;
        }
    }
    cmux.send_key(surface, ws_handle, "enter")
}

fn send_pasted_prompt_then_enter(
    cmux: &CmuxService,
    surface: &str,
    ws_handle: &str,
    buffer_prefix: &str,
    prompt: &str,
) -> Result<()> {
    let buffer = unique_cmux_buffer_name(buffer_prefix, surface);
    cmux.set_buffer(&buffer, prompt)?;
    cmux.paste_buffer(surface, ws_handle, &buffer)?;
    std::thread::sleep(PASTE_SUBMIT_SETTLE);
    cmux.send_key(surface, ws_handle, "enter")?;
    Ok(())
}

fn should_submit_with_enter_key(agent: &AgentConfig) -> bool {
    matches!(
        (&agent.submit, &agent.cli),
        (SubmitMode::Auto, AgentCli::Gemini)
            | (
                SubmitMode::CarriageReturn,
                AgentCli::Gemini | AgentCli::None
            )
    )
}

fn should_submit_codex_with_enter_key(agent: &AgentConfig) -> bool {
    matches!(
        (&agent.submit, &agent.cli),
        (SubmitMode::Auto, AgentCli::Codex) | (SubmitMode::CarriageReturn, AgentCli::Codex)
    )
}

fn should_submit_claude_with_enter_key(agent: &AgentConfig) -> bool {
    matches!(
        (&agent.submit, &agent.cli),
        (SubmitMode::Auto, AgentCli::Claude) | (SubmitMode::CarriageReturn, AgentCli::Claude)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_launch_command_injects_task_run_identity_only() {
        let agent = AgentConfig {
            cli: AgentCli::Codex,
            command: Some("codex".into()),
            ..AgentConfig::default()
        };
        let vars = HashMap::from([
            ("wt_agent_id".into(), "agents/run-1-add-schema".into()),
            ("wt_task_run_id".into(), "run-add-schema".into()),
        ]);

        let command = agent_launch_command(Some(&agent), &vars).unwrap();

        assert_eq!(
            command,
            "export WT_AGENT_ID=agents/run-1-add-schema WT_TASK_RUN_ID=run-add-schema; codex"
        );
    }

    #[test]
    fn agent_launch_command_without_identity_returns_bare_command() {
        let agent = AgentConfig {
            cli: AgentCli::Codex,
            command: Some("codex".into()),
            ..AgentConfig::default()
        };

        let command = agent_launch_command(Some(&agent), &HashMap::new()).unwrap();

        assert_eq!(command, "codex");
    }
}
