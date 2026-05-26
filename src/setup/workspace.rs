use super::{SetupOptions, agent_launch_command};
use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::{CmuxCaller, CmuxService};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

const CMUX_BACKGROUND_AGENT_PROBE_SECS: u64 = 5;
const CMUX_FOCUS_SETTLE_SECS: u64 = 3;

pub(super) struct OpenedWorkspace {
    pub(super) handle: String,
    pub(super) coordinator: Option<CmuxCaller>,
}

pub(super) fn workspace_color(config: &Config, mode: &str) -> String {
    let Some(workspace) = config.workspace.as_ref() else {
        return String::new();
    };

    workspace
        .effective_color(mode)
        .unwrap_or_default()
        .to_string()
}

pub(super) fn insert_cmux_template_vars(
    vars: &mut HashMap<String, String>,
    opened_workspace: Option<&OpenedWorkspace>,
) {
    let Some(opened_workspace) = opened_workspace else {
        return;
    };

    vars.insert(
        "task_agent_cmux_workspace".into(),
        opened_workspace.handle.clone(),
    );

    let Some(coordinator) = opened_workspace.coordinator.as_ref() else {
        return;
    };
    let (Some(workspace), Some(surface)) = (
        coordinator.workspace.as_deref(),
        coordinator.surface.as_deref(),
    ) else {
        return;
    };

    vars.insert("coordinator_cmux_workspace".into(), workspace.into());
    vars.insert("coordinator_cmux_surface".into(), surface.into());
    vars.insert(
        "coordinator_send_command".into(),
        format!(
            "cmux send --workspace {} --surface {} {}",
            shell_arg(workspace),
            shell_arg(surface),
            shell_arg("<message>")
        ),
    );
    vars.insert(
        "coordinator_enter_command".into(),
        format!(
            "cmux send-key --workspace {} --surface {} enter",
            shell_arg(workspace),
            shell_arg(surface)
        ),
    );
}

pub(super) fn open_workspace(
    ctx: &Ctx,
    config: &Config,
    wt_path: &Path,
    names: &WorktreeNames,
    template_vars: &HashMap<String, String>,
    color: &str,
    options: SetupOptions,
) -> Result<Option<OpenedWorkspace>> {
    let cmux = CmuxService::new_with_workspace_focus(ctx.runner.as_ref(), options.focus_workspace);
    if !cmux.is_available() {
        ctx.ui
            .print_step(&format!("Worktree path: {}", wt_path.display()));
        return Ok(None);
    }

    let ws_config = match &config.workspace {
        Some(ws) => ws,
        None => {
            ctx.ui
                .print_step(&format!("Worktree path: {}", wt_path.display()));
            return Ok(None);
        }
    };

    ctx.ui
        .print_step(&format!("Opening cmux workspace: {}", names.workspace));

    let command = agent_launch_command(config.agent.as_ref(), template_vars)?;
    let should_probe_workspace_start = options.focus_restore_if_workspace_cold
        && (!command.trim().is_empty() || !ws_config.tabs.is_empty());
    let identity_context = cmux.identity_context();
    let caller_context = identity_context
        .as_ref()
        .and_then(|identity| identity.caller.clone());
    let focus_restore_target = (options.restore_caller_after_workspace_open
        && (options.focus_workspace || should_probe_workspace_start))
        .then(|| {
            identity_context
                .as_ref()
                .and_then(|identity| identity.focused.clone())
                .or_else(|| caller_context.clone())
        })
        .flatten();
    let ws_handle = cmux.new_workspace_with_caller(
        wt_path,
        &names.workspace,
        &command,
        caller_context.as_ref(),
    )?;
    let mut focus_was_moved = options.focus_workspace;

    if should_probe_workspace_start {
        let ready_marker = config
            .agent
            .as_ref()
            .and_then(|agent| agent.effective_ready());
        ctx.ui.print_step(&format!(
            "Waiting up to {CMUX_BACKGROUND_AGENT_PROBE_SECS}s for cmux to start the agent in the background..."
        ));
        let started = wait_for_workspace_agent_start(
            &cmux,
            &ws_handle,
            ready_marker.as_deref(),
            CMUX_BACKGROUND_AGENT_PROBE_SECS,
        );

        if !started {
            ctx.ui.print_step(
                "cmux did not start the agent in the background; focusing briefly, then returning",
            );
            match cmux.select_workspace(&ws_handle) {
                Ok(()) => {
                    focus_was_moved = true;
                    wait_for_workspace_terminal(&cmux, &ws_handle, CMUX_FOCUS_SETTLE_SECS);
                }
                Err(err) => {
                    ctx.ui.print_warning(&format!(
                        "Failed to focus cmux workspace for PTY initialization: {err}"
                    ));
                }
            }
        }
    }

    if !color.is_empty() {
        cmux.set_color(&ws_handle, color)?;
    }

    let panes = cmux.list_panes(&ws_handle)?;
    if let Some(pane) = panes.first() {
        for tab_cmd in &ws_config.tabs {
            let surface = cmux.new_surface(pane, &ws_handle)?;
            cmux.send(&surface, &ws_handle, &format!("{tab_cmd}\n"))?;
        }
    }

    if options.restore_caller_after_workspace_open && focus_was_moved {
        if let Some(target) = focus_restore_target.as_ref() {
            // Temporary cmux 0.64.x workaround: focus starts the offscreen PTY
            // (manaflow-ai/cmux#4187/#4193), then we restore the prior focus.
            restore_cmux_focus(ctx, &cmux, &ws_handle, target, should_probe_workspace_start);
        }
    }

    Ok(Some(OpenedWorkspace {
        handle: ws_handle,
        coordinator: caller_context,
    }))
}

fn restore_cmux_focus(
    ctx: &Ctx,
    cmux: &CmuxService<'_>,
    opened_workspace: &str,
    target: &CmuxCaller,
    print_returned_step: bool,
) {
    let workspace = target
        .workspace
        .as_deref()
        .filter(|workspace| !workspace.trim().is_empty());
    let surface = target
        .surface
        .as_deref()
        .filter(|surface| !surface.trim().is_empty());

    if let Some(surface) = surface {
        match cmux.focus_surface(surface, workspace) {
            Ok(()) => {
                if print_returned_step {
                    ctx.ui.print_step("Returned to previous cmux surface");
                }
                return;
            }
            Err(err) => {
                ctx.ui.print_warning(&format!(
                    "Failed to restore cmux surface focus: {err}; falling back to workspace focus"
                ));
            }
        }
    }

    if let Some(workspace) = workspace.filter(|workspace| *workspace != opened_workspace) {
        if let Err(err) = cmux.select_workspace(workspace) {
            ctx.ui
                .print_warning(&format!("Failed to restore cmux workspace focus: {err}"));
        } else if print_returned_step {
            ctx.ui.print_step("Returned to previous cmux workspace");
        }
    }
}

fn workspace_terminal_ready(cmux: &CmuxService<'_>, ws_handle: &str) -> bool {
    let Some(surface) = first_workspace_surface(cmux, ws_handle) else {
        return false;
    };
    cmux.read_screen(&surface, ws_handle).is_ok()
}

fn workspace_agent_started(
    cmux: &CmuxService<'_>,
    ws_handle: &str,
    ready_marker: Option<&str>,
) -> bool {
    let Some(surface) = first_workspace_surface(cmux, ws_handle) else {
        return false;
    };
    let Ok(screen) = cmux.read_screen(&surface, ws_handle) else {
        return false;
    };

    ready_marker.is_none_or(|marker| screen.contains(marker))
}

fn wait_for_workspace_agent_start(
    cmux: &CmuxService<'_>,
    ws_handle: &str,
    ready_marker: Option<&str>,
    timeout_secs: u64,
) -> bool {
    let attempts = timeout_secs.saturating_mul(4).max(1);
    for attempt in 0..attempts {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if workspace_agent_started(cmux, ws_handle, ready_marker) {
            return true;
        }
    }
    false
}

fn wait_for_workspace_terminal(cmux: &CmuxService<'_>, ws_handle: &str, timeout_secs: u64) -> bool {
    let attempts = timeout_secs.saturating_mul(4).max(1);
    for attempt in 0..attempts {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if workspace_terminal_ready(cmux, ws_handle) {
            return true;
        }
    }
    false
}

fn first_workspace_surface(cmux: &CmuxService<'_>, ws_handle: &str) -> Option<String> {
    let pane = cmux.list_panes(ws_handle).ok()?.into_iter().next()?;
    cmux.list_pane_surfaces(&pane, ws_handle)
        .ok()?
        .into_iter()
        .next()
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
    use super::workspace_color;
    use crate::config::{Config, WorkspaceConfig};
    use crate::setup::{
        WORKSPACE_COLOR_KIND_BRANCH, WORKSPACE_COLOR_KIND_ISSUE, WORKSPACE_COLOR_KIND_PR,
        WORKSPACE_COLOR_KIND_TASK,
    };
    use std::collections::HashMap;

    #[test]
    fn workspace_color_uses_builtin_defaults_when_workspace_is_configured() {
        let config = Config {
            workspace: Some(WorkspaceConfig::default()),
            ..Config::default()
        };

        assert_eq!(workspace_color(&config, WORKSPACE_COLOR_KIND_TASK), "blue");
        assert_eq!(workspace_color(&config, WORKSPACE_COLOR_KIND_ISSUE), "blue");
        assert_eq!(
            workspace_color(&config, WORKSPACE_COLOR_KIND_BRANCH),
            "green"
        );
        assert_eq!(workspace_color(&config, WORKSPACE_COLOR_KIND_PR), "magenta");
    }

    #[test]
    fn workspace_color_prefers_configured_colors() {
        let config = Config {
            workspace: Some(WorkspaceConfig {
                colors: HashMap::from([(WORKSPACE_COLOR_KIND_TASK.into(), "cyan".into())]),
                ..WorkspaceConfig::default()
            }),
            ..Config::default()
        };

        assert_eq!(workspace_color(&config, WORKSPACE_COLOR_KIND_TASK), "cyan");
    }

    #[test]
    fn workspace_color_allows_empty_color_override() {
        let config = Config {
            workspace: Some(WorkspaceConfig {
                colors: HashMap::from([(WORKSPACE_COLOR_KIND_TASK.into(), String::new())]),
                ..WorkspaceConfig::default()
            }),
            ..Config::default()
        };

        assert_eq!(workspace_color(&config, WORKSPACE_COLOR_KIND_TASK), "");
    }

    #[test]
    fn workspace_color_has_no_default_without_workspace_config() {
        assert_eq!(
            workspace_color(&Config::default(), WORKSPACE_COLOR_KIND_TASK),
            ""
        );
    }

    #[test]
    fn workspace_color_has_no_default_for_unknown_kind() {
        let config = Config {
            workspace: Some(WorkspaceConfig::default()),
            ..Config::default()
        };

        assert_eq!(workspace_color(&config, "workflow"), "");
    }
}
