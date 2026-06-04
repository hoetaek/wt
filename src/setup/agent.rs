use super::chrome_devtools::ChromeDevtoolsMcpConfig;
use crate::config::{AgentCli, AgentConfig, SubmitMode};
use crate::context::Ctx;
use crate::services::cmux::CmuxService;
use crate::services::cmux_push::{submit_codex_prompt, submit_pasted_prompt_with_enter};
use crate::template;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const WT_AGENT_ID_TEMPLATE_KEY: &str = "wt_agent_id";
const WT_TASK_RUN_ID_TEMPLATE_KEY: &str = "wt_task_run_id";
const CHROME_DEVTOOLS_MCP_SERVER: &str = "chrome-devtools";
const CHROME_DEVTOOLS_MCP_PACKAGE: &str = "chrome-devtools-mcp@latest";
const CHROME_DEVTOOLS_MCP_CONTEXT: &str =
    "\n이 워크스페이스 브라우저 작업은 chrome-devtools(wt) 서버로 진행하세요.\n";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentLaunchOptions {
    chrome_devtools_mcp: Option<AgentChromeDevtoolsMcp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentChromeDevtoolsMcp {
    browser_url: String,
    claude_config_path: Option<PathBuf>,
}

impl AgentLaunchOptions {
    pub(crate) fn chrome_devtools_context(&self) -> Option<&'static str> {
        self.chrome_devtools_mcp
            .is_some()
            .then_some(CHROME_DEVTOOLS_MCP_CONTEXT)
    }
}

pub(crate) fn agent_launch_command(
    agent: Option<&AgentConfig>,
    vars: &HashMap<String, String>,
) -> Result<String> {
    agent_launch_command_with_options(agent, vars, &AgentLaunchOptions::default())
}

pub(crate) fn prepare_agent_launch_options(
    ctx: &Ctx,
    agent: Option<&AgentConfig>,
    chrome_devtools_mcp: Option<ChromeDevtoolsMcpConfig>,
) -> Result<AgentLaunchOptions> {
    let Some(chrome_devtools_mcp) = chrome_devtools_mcp else {
        return Ok(AgentLaunchOptions::default());
    };
    let Some(agent) = agent else {
        return Ok(AgentLaunchOptions::default());
    };

    match agent.cli {
        AgentCli::None => Ok(AgentLaunchOptions::default()),
        AgentCli::Gemini => {
            ctx.ui.print_step(
                "Chrome DevTools MCP auto-wiring is not supported for agent.cli = \"gemini\"; skipping.",
            );
            Ok(AgentLaunchOptions::default())
        }
        AgentCli::Claude | AgentCli::Codex => {
            if !ctx.runner.has_command("npx") {
                ctx.ui.print_warning(
                    "Chrome DevTools MCP auto-wiring skipped: npx was not found on PATH.",
                );
                return Ok(AgentLaunchOptions::default());
            }

            let claude_config_path = if agent.cli == AgentCli::Claude {
                Some(write_claude_mcp_config(&chrome_devtools_mcp)?)
            } else {
                None
            };

            Ok(AgentLaunchOptions {
                chrome_devtools_mcp: Some(AgentChromeDevtoolsMcp {
                    browser_url: chrome_devtools_mcp.browser_url,
                    claude_config_path,
                }),
            })
        }
    }
}

pub(crate) fn agent_launch_command_with_options(
    agent: Option<&AgentConfig>,
    vars: &HashMap<String, String>,
    options: &AgentLaunchOptions,
) -> Result<String> {
    let Some(agent) = agent else {
        return Ok(String::new());
    };
    let Some(command) = agent.command_line_with_vars(Some(vars))? else {
        return Ok(String::new());
    };

    let command = inject_chrome_devtools_mcp_args(agent, command, options);
    Ok(inject_agent_identity_env(agent, command, vars))
}

fn write_claude_mcp_config(chrome_devtools_mcp: &ChromeDevtoolsMcpConfig) -> Result<PathBuf> {
    if let Some(parent) = chrome_devtools_mcp.claude_config_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("create Chrome DevTools MCP config dir {}", parent.display())
        })?;
    }

    let browser_url_arg = format!("--browser-url={}", chrome_devtools_mcp.browser_url);
    let config = serde_json::json!({
        "mcpServers": {
            CHROME_DEVTOOLS_MCP_SERVER: {
                "command": "npx",
                "args": [
                    CHROME_DEVTOOLS_MCP_PACKAGE,
                    browser_url_arg,
                ],
            },
        },
    });
    let rendered = format!("{}\n", serde_json::to_string_pretty(&config)?);
    fs::write(&chrome_devtools_mcp.claude_config_path, rendered).with_context(|| {
        format!(
            "write Chrome DevTools MCP config {}",
            chrome_devtools_mcp.claude_config_path.display()
        )
    })?;

    Ok(chrome_devtools_mcp.claude_config_path.clone())
}

fn inject_chrome_devtools_mcp_args(
    agent: &AgentConfig,
    command: String,
    options: &AgentLaunchOptions,
) -> String {
    if command.trim().is_empty() {
        return command;
    }
    let Some(chrome_devtools_mcp) = options.chrome_devtools_mcp.as_ref() else {
        return command;
    };

    match agent.cli {
        AgentCli::Claude => {
            let Some(path) = chrome_devtools_mcp.claude_config_path.as_ref() else {
                return command;
            };
            format!(
                "{command} --mcp-config {}",
                shell_arg(&path.to_string_lossy())
            )
        }
        AgentCli::Codex => {
            let command_config =
                format!("mcp_servers.{CHROME_DEVTOOLS_MCP_SERVER}.command=\"npx\"");
            let args_config = format!(
                "mcp_servers.{CHROME_DEVTOOLS_MCP_SERVER}.args=[\"{CHROME_DEVTOOLS_MCP_PACKAGE}\",\"--browser-url={}\"]",
                chrome_devtools_mcp.browser_url
            );
            format!(
                "{command} -c {} -c {}",
                shell_arg(&command_config),
                shell_arg(&args_config)
            )
        }
        AgentCli::Gemini | AgentCli::None => command,
    }
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
            ctx.ui
                .print_step(&format!("Waiting for agent ready marker '{}'...", marker));

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
    submit_codex_prompt(cmux.runner(), surface, Some(ws_handle), prompt)
}

fn send_pasted_prompt_then_enter(
    cmux: &CmuxService,
    surface: &str,
    ws_handle: &str,
    buffer_prefix: &str,
    prompt: &str,
) -> Result<()> {
    submit_pasted_prompt_with_enter(
        cmux.runner(),
        surface,
        Some(ws_handle),
        buffer_prefix,
        prompt,
    )
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
    use crate::config::ReadyMode;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn chrome_devtools_mcp() -> ChromeDevtoolsMcpConfig {
        ChromeDevtoolsMcpConfig {
            browser_url: "http://127.0.0.1:9222".into(),
            claude_config_path: PathBuf::from("/tmp/wt-mcp.json"),
        }
    }

    fn agent(cli: AgentCli) -> AgentConfig {
        AgentConfig {
            cli,
            command: None,
            ready: ReadyMode::Auto,
            submit: SubmitMode::Auto,
            ..AgentConfig::default()
        }
    }

    fn ctx_with_runner_and_ui(runner: MockRunner, ui: Arc<MockUi>) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            crate::config::Config::default(),
            Box::new(runner),
            Box::new(ui),
        )
    }

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

    #[test]
    fn agent_launch_command_injects_claude_mcp_config_without_strict() {
        let agent = agent(AgentCli::Claude);
        let options = AgentLaunchOptions {
            chrome_devtools_mcp: Some(AgentChromeDevtoolsMcp {
                browser_url: "http://127.0.0.1:9222".into(),
                claude_config_path: Some(PathBuf::from("/tmp/profile/wt-mcp.json")),
            }),
        };

        let command =
            agent_launch_command_with_options(Some(&agent), &HashMap::new(), &options).unwrap();

        assert_eq!(command, "claude --mcp-config /tmp/profile/wt-mcp.json");
        assert!(!command.contains("--strict-mcp-config"));
    }

    #[test]
    fn agent_launch_command_injects_codex_mcp_config_overrides() {
        let agent = agent(AgentCli::Codex);
        let options = AgentLaunchOptions {
            chrome_devtools_mcp: Some(AgentChromeDevtoolsMcp {
                browser_url: "http://127.0.0.1:9222".into(),
                claude_config_path: None,
            }),
        };

        let command =
            agent_launch_command_with_options(Some(&agent), &HashMap::new(), &options).unwrap();

        assert!(command.starts_with("codex "));
        assert!(command.contains("-c 'mcp_servers.chrome-devtools.command=\"npx\"'"));
        assert!(command.contains(
            "-c 'mcp_servers.chrome-devtools.args=[\"chrome-devtools-mcp@latest\",\"--browser-url=http://127.0.0.1:9222\"]'"
        ));
    }

    #[test]
    fn agent_launch_options_noop_without_chrome_devtools_config() {
        let mut runner = MockRunner::new();
        runner.add_command("npx");
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_and_ui(runner, Arc::clone(&ui));

        let options =
            prepare_agent_launch_options(&ctx, Some(&agent(AgentCli::Codex)), None).unwrap();

        assert_eq!(options, AgentLaunchOptions::default());
        assert!(ui.steps.lock().unwrap().is_empty());
        assert!(ui.warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn agent_launch_options_noop_for_none_cli() {
        let mut runner = MockRunner::new();
        runner.add_command("npx");
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_and_ui(runner, Arc::clone(&ui));

        let options = prepare_agent_launch_options(
            &ctx,
            Some(&agent(AgentCli::None)),
            Some(chrome_devtools_mcp()),
        )
        .unwrap();

        assert_eq!(options, AgentLaunchOptions::default());
        assert!(ui.steps.lock().unwrap().is_empty());
        assert!(ui.warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn agent_launch_options_prints_info_for_unsupported_gemini_cli() {
        let mut runner = MockRunner::new();
        runner.add_command("npx");
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_and_ui(runner, Arc::clone(&ui));

        let options = prepare_agent_launch_options(
            &ctx,
            Some(&agent(AgentCli::Gemini)),
            Some(chrome_devtools_mcp()),
        )
        .unwrap();

        assert_eq!(options, AgentLaunchOptions::default());
        assert!(ui.steps.lock().unwrap().iter().any(|step| step.contains(
            "Chrome DevTools MCP auto-wiring is not supported for agent.cli = \"gemini\""
        )));
        assert!(ui.warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn agent_launch_options_warns_and_skips_when_npx_is_missing() {
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_runner_and_ui(MockRunner::new(), Arc::clone(&ui));

        let options = prepare_agent_launch_options(
            &ctx,
            Some(&agent(AgentCli::Codex)),
            Some(chrome_devtools_mcp()),
        )
        .unwrap();

        assert_eq!(options, AgentLaunchOptions::default());
        assert!(ui.steps.lock().unwrap().is_empty());
        assert!(
            ui.warnings
                .lock()
                .unwrap()
                .iter()
                .any(|warning| warning.contains("npx was not found on PATH"))
        );
    }
}
