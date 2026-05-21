use crate::commands::agent_hook;
use crate::config::{AgentCli, Config, IssueProviderType, SiteProvider};
use crate::context::Ctx;
use anyhow::{Result, anyhow};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CODEX_HOOK_INSTALL_HINT: &str =
    "Run cmux hooks codex install --yes to enable reliable Codex status events.";
const CODEX_WT_HOOK_INSTALL_HINT: &str =
    "Run wt hooks setup to enable wt inbox delivery through detected agent hook dispatchers.";
const CODEX_CMUX_HOOK_EVENTS: [(&str, &str); 5] = [
    ("PermissionRequest", "permission_request"),
    ("PreToolUse", "pre_tool_use"),
    ("SessionStart", "session_start"),
    ("Stop", "stop"),
    ("UserPromptSubmit", "user_prompt_submit"),
];

pub fn run(ctx: &Ctx, profile: Option<&str>) -> Result<()> {
    let resolved = resolve_config(ctx, profile)?;
    let config = resolved.config();

    if ctx.is_json() {
        return run_json(ctx, config, profile);
    }

    ctx.ui.print_step("Doctor");
    if let Some(profile) = profile {
        ctx.ui.print_step(&format!("Profile: {profile}"));
    }
    check_issue_provider(ctx, config);
    check_github_cli(ctx, config);
    check_site_provider(ctx, config);
    check_agent_config(ctx, config);
    check_workspace_config(ctx, config);
    check_worktree_naming(ctx, config);
    check_cmux(ctx, config);
    check_codex_hook_readiness(ctx, config);
    Ok(())
}

fn run_json(ctx: &Ctx, config: &Config, profile: Option<&str>) -> Result<()> {
    let report = build_report(ctx, config, profile);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, &report)?;
    writeln!(handle)?;
    Ok(())
}

fn build_report(ctx: &Ctx, config: &Config, profile: Option<&str>) -> DoctorReport {
    let mut checks = Vec::new();
    collect_issue_provider_checks(ctx, config, &mut checks);
    collect_github_cli_check(ctx, config, &mut checks);
    collect_site_provider_checks(ctx, config, &mut checks);
    collect_agent_checks(ctx, config, &mut checks);
    collect_workspace_config_checks(config, &mut checks);
    collect_worktree_naming_checks(ctx, config, &mut checks);
    collect_cmux_check(ctx, config, &mut checks);
    collect_codex_hook_readiness_checks(config, &mut checks);

    DoctorReport {
        profile: profile.map(str::to_string),
        checks,
    }
}

enum ResolvedConfig<'a> {
    Base(&'a Config),
    Profile(Box<Config>),
}

impl<'a> ResolvedConfig<'a> {
    fn config(&self) -> &Config {
        match self {
            ResolvedConfig::Base(config) => config,
            ResolvedConfig::Profile(config) => config,
        }
    }
}

fn resolve_config<'a>(ctx: &'a Ctx, profile: Option<&str>) -> Result<ResolvedConfig<'a>> {
    let Some(profile) = profile else {
        return Ok(ResolvedConfig::Base(&ctx.config));
    };
    let config = Config::load_profile_from_storage(
        &ctx.repo_root,
        &ctx.storage_root,
        profile,
        &ctx.base_config,
    )?
    .ok_or_else(|| anyhow!("Profile '{profile}' not found"))?;
    Ok(ResolvedConfig::Profile(Box::new(config)))
}

#[derive(Serialize)]
struct DoctorReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    checks: Vec<DoctorCheck>,
}

#[derive(Serialize)]
struct DoctorCheck {
    name: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl DoctorCheck {
    fn ok(name: impl Into<String>, message: impl Into<Option<String>>) -> Self {
        Self {
            name: name.into(),
            status: "ok",
            message: message.into(),
        }
    }

    fn warning(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: "warning",
            message: Some(message.into()),
        }
    }
}

fn collect_issue_provider_checks(ctx: &Ctx, config: &Config, checks: &mut Vec<DoctorCheck>) {
    match config.issues.as_ref().map(|issues| &issues.provider) {
        Some(IssueProviderType::Linear) => {
            checks.push(DoctorCheck::ok("issue_provider", Some("linear".into())));
            collect_required_command(
                ctx,
                checks,
                "linear_cli",
                "linear",
                "Install Linear CLI, or change [issues].provider to \"github\"/remove [issues].",
            );
        }
        Some(IssueProviderType::Github) => {
            checks.push(DoctorCheck::ok("issue_provider", Some("github".into())));
            collect_required_command(
                ctx,
                checks,
                "gh_cli",
                "gh",
                "Install GitHub CLI, or change [issues].provider to \"linear\"/remove [issues].",
            );
        }
        None => checks.push(DoctorCheck::ok("issue_provider", Some("none".into()))),
    }
}

fn collect_github_cli_check(ctx: &Ctx, config: &Config, checks: &mut Vec<DoctorCheck>) {
    if config
        .issues
        .as_ref()
        .is_some_and(|issues| issues.provider == IssueProviderType::Github)
    {
        return;
    }

    collect_optional_command(ctx, checks, "gh_cli_for_pr", "gh", "needed for wt run pr");
}

fn collect_site_provider_checks(ctx: &Ctx, config: &Config, checks: &mut Vec<DoctorCheck>) {
    let Some(site) = config.effective_site() else {
        checks.push(DoctorCheck::ok("site_provider", Some("none".into())));
        return;
    };

    match site.provider {
        SiteProvider::None => checks.push(DoctorCheck::ok("site_provider", Some("none".into()))),
        SiteProvider::DockerProxy => {
            checks.push(DoctorCheck::ok(
                "site_provider",
                Some("docker_proxy".into()),
            ));
            checks.push(DoctorCheck::ok("docker_proxy", Some("ok".into())));
        }
        SiteProvider::Herd => {
            checks.push(DoctorCheck::ok("site_provider", Some("herd".into())));
            collect_required_command(
                ctx,
                checks,
                "herd_cli",
                "herd",
                "Install Herd CLI, or change [site].provider to \"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Valet => {
            checks.push(DoctorCheck::ok("site_provider", Some("valet".into())));
            collect_required_command(
                ctx,
                checks,
                "valet_cli",
                "valet",
                "Install Valet CLI, or change [site].provider to \"herd\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Traefik => {
            checks.push(DoctorCheck::ok("site_provider", Some("traefik".into())));
            collect_required_command(
                ctx,
                checks,
                "traefik_cli",
                "traefik",
                "Install Traefik CLI, or change [site].provider to \"herd\"/\"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
    }
}

fn collect_agent_checks(ctx: &Ctx, config: &Config, checks: &mut Vec<DoctorCheck>) {
    let Some(agent) = config.agent.as_ref() else {
        checks.push(DoctorCheck::ok("agent", Some("none".into())));
        return;
    };

    checks.push(DoctorCheck::ok(
        "agent",
        Some(agent_cli_name(&agent.cli).into()),
    ));

    if agent.cli == AgentCli::None {
        if agent.command.is_some() {
            checks.push(DoctorCheck::warning(
                "agent_command",
                "Agent command is ignored when agent.cli is \"none\".",
            ));
        }
        return;
    }

    if agent.command.is_some() {
        checks.push(DoctorCheck::ok("agent_command", Some("override".into())));
        if let Some(command) = agent.command.as_deref() {
            collect_command_string(
                ctx,
                checks,
                command,
                "agent_command",
                "Install the configured agent command, or update [agent].command.",
            );
        }
        if !agent.args.is_empty() {
            checks.push(DoctorCheck::warning(
                "agent_args",
                "Agent args are ignored when agent.command is set.",
            ));
        }
        return;
    }

    if let Some(command) = agent_cli_command(&agent.cli) {
        collect_required_command(
            ctx,
            checks,
            &format!("{command}_cli"),
            command,
            format!("Install {command}, or change [agent].cli/[agent].command."),
        );
    }

    if agent.args.is_empty() {
        checks.push(DoctorCheck::ok("agent_args", Some("none".into())));
        return;
    }

    let mut ok = true;
    for arg in &agent.args {
        if arg.is_empty() {
            ok = false;
            checks.push(DoctorCheck::warning(
                "agent_args",
                "Agent args contain an empty value. Remove it unless you intentionally want to pass ''.",
            ));
            continue;
        }

        if arg.starts_with('-') && arg.chars().any(char::is_whitespace) {
            ok = false;
            checks.push(DoctorCheck::warning(
                "agent_args",
                format!(
                    "Agent arg {arg:?} is a single argument. Split flags and values, e.g. {}.",
                    split_arg_hint(arg)
                ),
            ));
        }
    }

    if ok {
        checks.push(DoctorCheck::ok("agent_args", Some("ok".into())));
    }
}

fn collect_worktree_naming_checks(ctx: &Ctx, config: &Config, checks: &mut Vec<DoctorCheck>) {
    let Some(naming) = config.worktree.naming.as_ref() else {
        return;
    };

    collect_command_string(
        ctx,
        checks,
        &naming.command,
        "worktree_naming_command",
        "Install the configured naming command, or update [worktree.naming].command.",
    );
}

fn collect_workspace_config_checks(config: &Config, checks: &mut Vec<DoctorCheck>) {
    if config.workspace.is_some() {
        checks.push(DoctorCheck::ok(
            "workspace_config",
            Some("configured".into()),
        ));
    } else if agent_launch_requested(config) {
        checks.push(DoctorCheck::warning(
            "workspace_config",
            "missing; add [workspace] to open a cmux workspace and launch the agent.",
        ));
    } else {
        checks.push(DoctorCheck::ok("workspace_config", Some("none".into())));
    }
}

fn collect_cmux_check(ctx: &Ctx, config: &Config, checks: &mut Vec<DoctorCheck>) {
    let needs_cmux = cmux_relevant(config);

    if ctx.runner.has_command("cmux") {
        checks.push(DoctorCheck::ok("cmux_cli", Some("ok".into())));
    } else if needs_cmux {
        collect_required_command(
            ctx,
            checks,
            "cmux_cli",
            "cmux",
            "Install cmux, or remove [workspace]/[agent.prompt] automation.",
        );
    } else {
        checks.push(DoctorCheck::ok(
            "cmux_cli",
            Some("missing; optional for wt agent status/watch, inspect, and send".into()),
        ));
    }
}

fn collect_codex_hook_readiness_checks(config: &Config, checks: &mut Vec<DoctorCheck>) {
    if !codex_agent_configured(config) {
        return;
    }

    let codex_home = match codex_home_dir() {
        Ok(path) => path,
        Err(message) => {
            checks.push(DoctorCheck::warning("codex_home", message));
            return;
        }
    };

    checks.extend(codex_hook_readiness_checks_for_home(&codex_home));
}

fn codex_hook_readiness_checks_for_home(codex_home: &Path) -> Vec<DoctorCheck> {
    let hooks_path = codex_home.join("hooks.json");
    let config_path = codex_home.join("config.toml");
    let mut checks = Vec::new();

    match missing_cmux_codex_hook_events(&hooks_path) {
        Ok(missing) if missing.is_empty() => checks.push(DoctorCheck::ok(
            "codex_hooks_json",
            Some(format!("{} cmux Codex hooks", CODEX_CMUX_HOOK_EVENTS.len())),
        )),
        Ok(missing) => checks.push(DoctorCheck::warning(
            "codex_hooks_json",
            format!(
                "missing cmux hooks for {}. {CODEX_HOOK_INSTALL_HINT}",
                missing.join(", ")
            ),
        )),
        Err(message) => checks.push(DoctorCheck::warning("codex_hooks_json", message)),
    }

    match codex_config_readiness(&config_path, &hooks_path) {
        Ok(readiness) => {
            if readiness.hooks_enabled {
                checks.push(DoctorCheck::ok(
                    "codex_config_hooks",
                    Some("hooks enabled".into()),
                ));
            } else {
                checks.push(DoctorCheck::warning(
                    "codex_config_hooks",
                    format!("hooks feature is not enabled. {CODEX_HOOK_INSTALL_HINT}"),
                ));
            }

            if readiness.missing_trust_events.is_empty() {
                checks.push(DoctorCheck::ok(
                    "codex_hook_trust",
                    Some(format!(
                        "{} trusted hook entries",
                        CODEX_CMUX_HOOK_EVENTS.len()
                    )),
                ));
            } else {
                checks.push(DoctorCheck::warning(
                    "codex_hook_trust",
                    format!(
                        "missing trusted_hash entries for {}. {CODEX_HOOK_INSTALL_HINT}",
                        readiness.missing_trust_events.join(", ")
                    ),
                ));
            }
        }
        Err(message) => checks.push(DoctorCheck::warning("codex_config_hooks", message)),
    }

    checks.extend(codex_wt_inbox_hook_checks_for_home(
        &hooks_path,
        &config_path,
    ));

    checks
}

fn codex_wt_inbox_hook_checks_for_home(hooks_path: &Path, config_path: &Path) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let hooks = match wt_codex_inbox_hooks(hooks_path) {
        Ok(hooks) => hooks,
        Err(message) => {
            checks.push(DoctorCheck::warning("codex_wt_inbox_hook", message));
            return checks;
        }
    };

    let missing_events = missing_wt_codex_inbox_hook_events(&hooks);
    if !missing_events.is_empty() {
        checks.push(DoctorCheck::warning(
            "codex_wt_inbox_hook",
            format!(
                "missing wt-managed Codex inbox hooks for {}. {CODEX_WT_HOOK_INSTALL_HINT}",
                missing_events.join(", ")
            ),
        ));
        if hooks.is_empty() {
            return checks;
        }
    } else {
        checks.push(DoctorCheck::ok(
            "codex_wt_inbox_hook",
            Some(format!("{} wt-managed inbox hook entries", hooks.len())),
        ));
    }

    match missing_wt_codex_inbox_trust(config_path, &hooks) {
        Ok(missing) if missing.is_empty() => checks.push(DoctorCheck::ok(
            "codex_wt_inbox_trust",
            Some(format!("{} trusted inbox hook entries", hooks.len())),
        )),
        Ok(missing) => checks.push(DoctorCheck::warning(
            "codex_wt_inbox_trust",
            format!(
                "missing or stale trusted_hash entries for {}. {CODEX_WT_HOOK_INSTALL_HINT}",
                missing.join(", ")
            ),
        )),
        Err(message) => checks.push(DoctorCheck::warning("codex_wt_inbox_trust", message)),
    }

    checks
}

#[derive(Debug, Clone)]
struct WtCodexInboxHook {
    key: String,
    command: String,
    event_name: &'static str,
    event_key: &'static str,
}

struct CodexConfigReadiness {
    hooks_enabled: bool,
    missing_trust_events: Vec<&'static str>,
}

fn missing_cmux_codex_hook_events(hooks_path: &Path) -> Result<Vec<&'static str>, String> {
    let content = read_codex_file(hooks_path, "hooks.json")?;
    let value = serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|err| format!("invalid JSON: {err}. {CODEX_HOOK_INSTALL_HINT}"))?;
    let hooks = value.get("hooks");
    let mut missing = Vec::new();

    for (event, _) in CODEX_CMUX_HOOK_EVENTS {
        if !hooks
            .and_then(|hooks| hooks.get(event))
            .is_some_and(value_contains_cmux_codex_hook_command)
        {
            missing.push(event);
        }
    }

    Ok(missing)
}

fn codex_config_readiness(
    config_path: &Path,
    hooks_path: &Path,
) -> Result<CodexConfigReadiness, String> {
    let content = read_codex_file(config_path, "config.toml")?;
    let value = toml::from_str::<toml::Value>(&content)
        .map_err(|err| format!("invalid TOML: {err}. {CODEX_HOOK_INSTALL_HINT}"))?;

    Ok(CodexConfigReadiness {
        hooks_enabled: codex_hooks_feature_enabled(&value),
        missing_trust_events: missing_trusted_hook_events(&value, hooks_path),
    })
}

fn read_codex_file(path: &Path, label: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            format!("missing {label}. {CODEX_HOOK_INSTALL_HINT}")
        } else {
            format!("cannot read {label}: {err}. {CODEX_HOOK_INSTALL_HINT}")
        }
    })
}

fn value_contains_cmux_codex_hook_command(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => {
            (value.contains("cmux hooks")
                && (value.contains("--source codex") || value.contains("hooks codex")))
                || value.contains(" hooks feed --source codex")
                || value.contains(" hooks codex ")
        }
        serde_json::Value::Array(values) => {
            values.iter().any(value_contains_cmux_codex_hook_command)
        }
        serde_json::Value::Object(values) => {
            values.values().any(value_contains_cmux_codex_hook_command)
        }
        _ => false,
    }
}

fn codex_hooks_feature_enabled(config: &toml::Value) -> bool {
    config.get("hooks").and_then(toml::Value::as_bool) == Some(true)
        || config
            .get("features")
            .and_then(|features| features.get("hooks"))
            .and_then(toml::Value::as_bool)
            == Some(true)
}

fn missing_trusted_hook_events(config: &toml::Value, hooks_path: &Path) -> Vec<&'static str> {
    let Some(state) = config
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(toml::Value::as_table)
    else {
        return CODEX_CMUX_HOOK_EVENTS
            .iter()
            .map(|(event, _)| *event)
            .collect();
    };

    let hooks_path = hooks_path.to_string_lossy();
    CODEX_CMUX_HOOK_EVENTS
        .iter()
        .filter_map(|(event, trust_event)| {
            let trusted = state.iter().any(|(key, entry)| {
                codex_trust_key_matches(key, trust_event, &hooks_path)
                    && entry
                        .get("trusted_hash")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|hash| hash.starts_with("sha256:"))
            });
            (!trusted).then_some(*event)
        })
        .collect()
}

fn wt_codex_inbox_hooks(hooks_path: &Path) -> Result<Vec<WtCodexInboxHook>, String> {
    let content = read_codex_file(hooks_path, "hooks.json")
        .map_err(|message| message.replace(CODEX_HOOK_INSTALL_HINT, CODEX_WT_HOOK_INSTALL_HINT))?;
    let value = serde_json::from_str::<serde_json::Value>(&content)
        .map_err(|err| format!("invalid JSON: {err}. {CODEX_WT_HOOK_INSTALL_HINT}"))?;
    let mut hooks = Vec::new();
    for &(event_name, event_key) in agent_hook::CODEX_HOOK_EVENTS {
        let Some(event_entries) = value
            .get("hooks")
            .and_then(|hooks| hooks.get(event_name))
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };

        for (group_index, group) in event_entries.iter().enumerate() {
            let Some(group_hooks) = group.get("hooks").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for (handler_index, hook) in group_hooks.iter().enumerate() {
                let Some(command) = hook.get("command").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if agent_hook::is_wt_managed_codex_command(command) {
                    hooks.push(WtCodexInboxHook {
                        key: agent_hook::codex_event_trust_key(
                            hooks_path,
                            event_key,
                            group_index,
                            handler_index,
                        ),
                        command: command.to_string(),
                        event_name,
                        event_key,
                    });
                }
            }
        }
    }

    Ok(hooks)
}

fn missing_wt_codex_inbox_hook_events(hooks: &[WtCodexInboxHook]) -> Vec<&'static str> {
    agent_hook::CODEX_HOOK_EVENTS
        .iter()
        .filter_map(|(event_name, _)| {
            let installed = hooks.iter().any(|hook| hook.event_name == *event_name);
            (!installed).then_some(*event_name)
        })
        .collect()
}

fn missing_wt_codex_inbox_trust(
    config_path: &Path,
    hooks: &[WtCodexInboxHook],
) -> Result<Vec<String>, String> {
    let content = read_codex_file(config_path, "config.toml")
        .map_err(|message| message.replace(CODEX_HOOK_INSTALL_HINT, CODEX_WT_HOOK_INSTALL_HINT))?;
    let value = toml::from_str::<toml::Value>(&content)
        .map_err(|err| format!("invalid TOML: {err}. {CODEX_WT_HOOK_INSTALL_HINT}"))?;
    let state = value
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(toml::Value::as_table);

    Ok(hooks
        .iter()
        .filter_map(|hook| {
            let trusted_hash = state
                .and_then(|state| state.get(&hook.key))
                .and_then(|entry| entry.get("trusted_hash"))
                .and_then(toml::Value::as_str);
            let expected_hash = agent_hook::codex_command_hook_hash(&hook.command, hook.event_key);
            (trusted_hash != Some(expected_hash.as_str())).then_some(hook.key.clone())
        })
        .collect())
}

fn codex_trust_key_matches(key: &str, event: &str, hooks_path: &str) -> bool {
    key.contains(&format!(":{event}:")) && (key.contains(hooks_path) || key.contains("hooks.json"))
}

fn codex_home_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEX_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    std::env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(path).join(".codex"))
        .ok_or_else(|| format!("CODEX_HOME and HOME are unset. {CODEX_HOOK_INSTALL_HINT}"))
}

fn codex_agent_configured(config: &Config) -> bool {
    config
        .agent
        .as_ref()
        .is_some_and(|agent| agent.cli == AgentCli::Codex)
}

fn collect_optional_command(
    ctx: &Ctx,
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    cmd: &str,
    message: &str,
) {
    if ctx.runner.has_command(cmd) {
        checks.push(DoctorCheck::ok(name, Some("ok".into())));
    } else {
        checks.push(DoctorCheck::warning(name, format!("missing; {message}")));
    }
}

fn collect_required_command(
    ctx: &Ctx,
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    cmd: &str,
    fix: impl Into<String>,
) {
    if ctx.runner.has_command(cmd) {
        checks.push(DoctorCheck::ok(name, Some("ok".into())));
    } else {
        checks.push(DoctorCheck::warning(
            name,
            format!("missing. {}", fix.into()),
        ));
    }
}

fn collect_command_string(
    ctx: &Ctx,
    checks: &mut Vec<DoctorCheck>,
    command: &str,
    label: &str,
    fix: &str,
) {
    match first_command_token(command) {
        Ok(Some(cmd)) => collect_required_command(ctx, checks, label, &cmd, fix),
        Ok(None) => checks.push(DoctorCheck::warning(label, format!("empty command. {fix}"))),
        Err(e) => checks.push(DoctorCheck::warning(
            label,
            format!("invalid command ({e}). {fix}"),
        )),
    }
}

fn check_issue_provider(ctx: &Ctx, config: &Config) {
    match config.issues.as_ref().map(|issues| &issues.provider) {
        Some(IssueProviderType::Linear) => {
            ctx.ui.print_step("Issue provider: linear");
            check_required_command(
                ctx,
                "linear",
                "linear CLI",
                "Install Linear CLI, or change [issues].provider to \"github\"/remove [issues].",
            );
        }
        Some(IssueProviderType::Github) => {
            ctx.ui.print_step("Issue provider: github");
            check_required_command(
                ctx,
                "gh",
                "gh CLI",
                "Install GitHub CLI, or change [issues].provider to \"linear\"/remove [issues].",
            );
        }
        None => ctx.ui.print_step("Issue provider: none"),
    }
}

fn check_github_cli(ctx: &Ctx, config: &Config) {
    if config
        .issues
        .as_ref()
        .is_some_and(|issues| issues.provider == IssueProviderType::Github)
    {
        return;
    }

    let status = if ctx.runner.has_command("gh") {
        "ok"
    } else {
        "missing"
    };
    ctx.ui
        .print_step(&format!("gh CLI: {status} (needed for wt run pr)"));
}

fn check_site_provider(ctx: &Ctx, config: &Config) {
    let Some(site) = config.effective_site() else {
        ctx.ui.print_step("Site provider: none");
        return;
    };

    match site.provider {
        SiteProvider::None => ctx.ui.print_step("Site provider: none"),
        SiteProvider::DockerProxy => {
            ctx.ui.print_step("Site provider: docker_proxy");
            ctx.ui.print_step("Docker proxy: ok");
        }
        SiteProvider::Herd => {
            ctx.ui.print_step("Site provider: herd");
            check_required_command(
                ctx,
                "herd",
                "herd CLI",
                "Install Herd CLI, or change [site].provider to \"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Valet => {
            ctx.ui.print_step("Site provider: valet");
            check_required_command(
                ctx,
                "valet",
                "valet CLI",
                "Install Valet CLI, or change [site].provider to \"herd\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Traefik => {
            ctx.ui.print_step("Site provider: traefik");
            check_required_command(
                ctx,
                "traefik",
                "Traefik CLI",
                "Install Traefik CLI, or change [site].provider to \"herd\"/\"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
    }
}

fn check_required_command(ctx: &Ctx, cmd: &str, label: &str, fix: &str) {
    if ctx.runner.has_command(cmd) {
        ctx.ui.print_step(&format!("{label}: ok"));
    } else {
        ctx.ui.print_warning(&format!("{label}: missing. {fix}"));
    }
}

fn check_agent_config(ctx: &Ctx, config: &Config) {
    let Some(agent) = config.agent.as_ref() else {
        ctx.ui.print_step("Agent: none");
        return;
    };

    ctx.ui
        .print_step(&format!("Agent: {}", agent_cli_name(&agent.cli)));

    if agent.cli == AgentCli::None {
        if agent.command.is_some() {
            ctx.ui
                .print_warning("Agent command is ignored when agent.cli is \"none\".");
        }
        return;
    }

    if agent.command.is_some() {
        ctx.ui.print_step("Agent command: override");
        if let Some(command) = agent.command.as_deref() {
            check_command_string(
                ctx,
                command,
                "Agent command",
                "Install the configured agent command, or update [agent].command.",
            );
        }
        if !agent.args.is_empty() {
            ctx.ui
                .print_warning("Agent args are ignored when agent.command is set.");
        }
        return;
    }

    if let Some(command) = agent_cli_command(&agent.cli) {
        check_required_command(
            ctx,
            command,
            &format!("{command} CLI"),
            &format!("Install {command}, or change [agent].cli/[agent].command."),
        );
    }

    if agent.args.is_empty() {
        ctx.ui.print_step("Agent args: none");
        return;
    }

    let mut ok = true;
    for arg in &agent.args {
        if arg.is_empty() {
            ok = false;
            ctx.ui.print_warning(
                "Agent args contain an empty value. Remove it unless you intentionally want to pass ''.",
            );
            continue;
        }

        if arg.starts_with('-') && arg.chars().any(char::is_whitespace) {
            ok = false;
            ctx.ui.print_warning(&format!(
                "Agent arg {arg:?} is a single argument. Split flags and values, e.g. {}.",
                split_arg_hint(arg)
            ));
        }
    }

    if ok {
        ctx.ui.print_step("Agent args: ok");
    }
}

fn check_worktree_naming(ctx: &Ctx, config: &Config) {
    let Some(naming) = config.worktree.naming.as_ref() else {
        return;
    };

    check_command_string(
        ctx,
        &naming.command,
        "worktree.naming.command",
        "Install the configured naming command, or update [worktree.naming].command.",
    );
}

fn check_workspace_config(ctx: &Ctx, config: &Config) {
    if config.workspace.is_some() {
        ctx.ui.print_step("Workspace config: configured");
    } else if agent_launch_requested(config) {
        ctx.ui.print_warning(
            "Workspace config: missing. Add [workspace] to open a cmux workspace and launch the agent.",
        );
    } else {
        ctx.ui.print_step("Workspace config: none");
    }
}

fn check_cmux(ctx: &Ctx, config: &Config) {
    let needs_cmux = cmux_relevant(config);

    if ctx.runner.has_command("cmux") {
        ctx.ui.print_step("cmux CLI: ok");
    } else if needs_cmux {
        check_required_command(
            ctx,
            "cmux",
            "cmux CLI",
            "Install cmux, or remove [workspace]/[agent.prompt] automation.",
        );
    } else {
        ctx.ui.print_step(
            "cmux CLI: missing (optional for wt agent status/watch, inspect, and send)",
        );
    }
}

fn check_codex_hook_readiness(ctx: &Ctx, config: &Config) {
    if !codex_agent_configured(config) {
        return;
    }

    let checks = match codex_home_dir() {
        Ok(path) => codex_hook_readiness_checks_for_home(&path),
        Err(message) => vec![DoctorCheck::warning("codex_home", message)],
    };

    for check in checks {
        let label = codex_readiness_label(&check.name);
        let message = check.message.as_deref().unwrap_or(check.status);
        if check.status == "ok" {
            ctx.ui.print_step(&format!("{label}: {message}"));
        } else {
            ctx.ui.print_warning(&format!("{label}: {message}"));
        }
    }
}

fn codex_readiness_label(name: &str) -> &str {
    match name {
        "codex_home" => "Codex home",
        "codex_hooks_json" => "Codex hooks.json",
        "codex_config_hooks" => "Codex config hooks",
        "codex_hook_trust" => "Codex hook trust",
        "codex_wt_inbox_hook" => "Codex wt inbox hook",
        "codex_wt_inbox_trust" => "Codex wt inbox trust",
        _ => name,
    }
}

fn cmux_relevant(config: &Config) -> bool {
    config.workspace.is_some()
        || agent_launch_requested(config)
        || config
            .agent
            .as_ref()
            .is_some_and(|agent| !agent.prompt.is_empty())
}

fn agent_launch_requested(config: &Config) -> bool {
    config
        .agent
        .as_ref()
        .is_some_and(|agent| agent.cli != AgentCli::None)
}

fn agent_cli_name(cli: &AgentCli) -> &'static str {
    match cli {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::None => "none",
    }
}

fn agent_cli_command(cli: &AgentCli) -> Option<&'static str> {
    match cli {
        AgentCli::Codex => Some("codex"),
        AgentCli::Claude => Some("claude"),
        AgentCli::Gemini => Some("gemini"),
        AgentCli::None => None,
    }
}

fn check_command_string(ctx: &Ctx, command: &str, label: &str, fix: &str) {
    match first_command_token(command) {
        Ok(Some(cmd)) => check_required_command(ctx, &cmd, label, fix),
        Ok(None) => ctx
            .ui
            .print_warning(&format!("{label}: empty command. {fix}")),
        Err(e) => ctx
            .ui
            .print_warning(&format!("{label}: invalid command ({e}). {fix}")),
    }
}

fn first_command_token(command: &str) -> Result<Option<String>> {
    let mut parts = shell_words::split(command)?;
    Ok(parts.drain(..).next())
}

fn split_arg_hint(arg: &str) -> String {
    let parts = arg
        .split_whitespace()
        .map(toml_quote)
        .collect::<Vec<_>>()
        .join(", ");
    format!("args = [{parts}]")
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentConfig, Config, IssuesConfig, ReadyMode, SiteConfig, SubmitMode, WorktreeNamingConfig,
    };
    use crate::context::mock::MockRunner;
    use crate::context::{Ctx, UserInterface};
    use anyhow::{Result, bail};
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingUi {
        steps: Arc<Mutex<Vec<String>>>,
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingUi {
        fn new() -> Self {
            Self::default()
        }
    }

    impl UserInterface for RecordingUi {
        fn select(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
            bail!("unexpected select")
        }

        fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
            bail!("unexpected multi_select")
        }

        fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
            bail!("unexpected confirm")
        }

        fn input(&self, _prompt: &str, _default: Option<&str>) -> Result<String> {
            bail!("unexpected input")
        }

        fn print_step(&self, msg: &str) {
            self.steps.lock().unwrap().push(msg.into());
        }

        fn print_dim(&self, _msg: &str) {}

        fn print_warning(&self, msg: &str) {
            self.warnings.lock().unwrap().push(msg.into());
        }

        fn print_error(&self, _msg: &str) {}
    }

    fn ctx_with(config: Config, runner: MockRunner, ui: RecordingUi) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(ui),
        )
    }

    fn ctx_with_root(
        repo_root: PathBuf,
        config: Config,
        runner: MockRunner,
        ui: RecordingUi,
    ) -> Ctx {
        Ctx::new(
            repo_root.clone(),
            repo_root,
            config,
            Box::new(runner),
            Box::new(ui),
        )
    }

    fn write_codex_hooks(home: &Path) {
        fs::create_dir_all(home).unwrap();
        let hooks = CODEX_CMUX_HOOK_EVENTS
            .iter()
            .map(|(event, _)| {
                let command = match *event {
                    "PermissionRequest" | "PreToolUse" => format!(
                        "cmux_cli=\"${{CMUX_BUNDLED_CLI_PATH:-}}\"; if [ -n \"$CMUX_SURFACE_ID\" ] && [ -n \"$cmux_cli\" ]; then \"$cmux_cli\" hooks feed --source codex --event {event}; else echo '{{}}'; fi"
                    ),
                    "SessionStart" => String::from(
                        "cmux_cli=\"${CMUX_BUNDLED_CLI_PATH:-}\"; if [ -n \"$CMUX_SURFACE_ID\" ] && [ -n \"$cmux_cli\" ]; then \"$cmux_cli\" hooks codex session-start; else echo '{}'; fi",
                    ),
                    "Stop" => String::from(
                        "cmux_cli=\"${CMUX_BUNDLED_CLI_PATH:-}\"; if [ -n \"$CMUX_SURFACE_ID\" ] && [ -n \"$cmux_cli\" ]; then \"$cmux_cli\" hooks codex stop; else echo '{}'; fi",
                    ),
                    "UserPromptSubmit" => String::from(
                        "cmux_cli=\"${CMUX_BUNDLED_CLI_PATH:-}\"; if [ -n \"$CMUX_SURFACE_ID\" ] && [ -n \"$cmux_cli\" ]; then \"$cmux_cli\" hooks codex prompt-submit; else echo '{}'; fi",
                    ),
                    _ => unreachable!(),
                };
                (
                    event.to_string(),
                    json!([
                        {
                            "hooks": [
                                {
                                    "command": command
                                }
                            ]
                        }
                    ]),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        fs::write(
            home.join("hooks.json"),
            serde_json::to_string_pretty(&json!({ "hooks": hooks })).unwrap(),
        )
        .unwrap();
    }

    fn write_codex_config(home: &Path, include_trust: bool) {
        fs::create_dir_all(home).unwrap();
        let hooks_path = home.join("hooks.json");
        let mut content = String::from("[features]\nhooks = true\n");

        if include_trust {
            for (_, trust_event) in CODEX_CMUX_HOOK_EVENTS {
                content.push_str(&format!(
                    "\n[hooks.state.\"{}:{trust_event}:0:0\"]\ntrusted_hash = \"sha256:test\"\n",
                    hooks_path.display()
                ));
            }
        }

        fs::write(home.join("config.toml"), content).unwrap();
    }

    fn write_wt_codex_inbox_hook(home: &Path, include_trust: bool) {
        fs::create_dir_all(home).unwrap();
        let hooks_path = home.join("hooks.json");
        let command = "if [ -n \"${WT_AGENT_ID:-}\" ]; then wt msg check-inbox --agent \"$WT_AGENT_ID\"; fi # wt-agent-hook:codex-inbox";
        fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [{
                            "type": "command",
                            "command": command,
                        }],
                    }],
                    "PostToolUse": [{
                        "hooks": [{
                            "type": "command",
                            "command": command,
                        }],
                    }],
                },
            }))
            .unwrap(),
        )
        .unwrap();

        let mut content = String::from("[features]\nhooks = true\n");
        if include_trust {
            for &(_, event_key) in crate::commands::agent_hook::CODEX_HOOK_EVENTS {
                content.push_str(&format!(
                    "\n[hooks.state.\"{}:{event_key}:0:0\"]\nenabled = true\ntrusted_hash = \"{}\"\n",
                    hooks_path.display(),
                    crate::commands::agent_hook::codex_command_hook_hash(command, event_key)
                ));
            }
        }
        fs::write(home.join("config.toml"), content).unwrap();
    }

    fn check_by_name<'a>(checks: &'a [DoctorCheck], name: &str) -> &'a DoctorCheck {
        checks
            .iter()
            .find(|check| check.name == name)
            .unwrap_or_else(|| panic!("missing doctor check {name}"))
    }

    #[test]
    fn warns_when_linear_provider_cli_is_missing() {
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("linear CLI: missing"));
        assert!(warnings.contains("[issues].provider"));
    }

    #[test]
    fn reports_github_issue_provider_when_gh_is_available() {
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Github,
                gh_user: Some("alice".into()),
            }),
            ..Default::default()
        };
        let mut runner = MockRunner::new();
        runner.add_command("gh");
        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, runner, ui);

        run(&ctx, None).unwrap();

        let steps = steps.lock().unwrap().join("\n");
        assert!(steps.contains("Issue provider: github"));
        assert!(steps.contains("gh CLI: ok"));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn warns_when_configured_site_provider_cli_is_missing() {
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Valet,
                ..Default::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("valet CLI: missing"));
        assert!(warnings.contains("[site].provider"));
    }

    #[test]
    fn docker_proxy_site_provider_does_not_require_cli() {
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::DockerProxy,
                ..Default::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let steps = steps.lock().unwrap().join("\n");
        assert!(steps.contains("Site provider: docker_proxy"));
        assert!(steps.contains("Docker proxy: ok"));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn warns_when_agent_arg_combines_flag_and_value() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: vec!["--plugin-dir .plugin-cache".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("claude CLI: missing"));
        assert!(warnings.contains("--plugin-dir .plugin-cache"));
        assert!(warnings.contains("args = [\"--plugin-dir\", \".plugin-cache\"]"));
    }

    #[test]
    fn reports_agent_args_ok_when_flag_and_value_are_split() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: vec!["--plugin-dir".into(), ".plugin-cache".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            workspace: Some(Default::default()),
            ..Default::default()
        };
        let mut runner = MockRunner::new();
        runner.add_command("claude");
        runner.add_command("cmux");
        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, runner, ui);

        run(&ctx, None).unwrap();

        let steps = steps.lock().unwrap().join("\n");
        assert!(steps.contains("claude CLI: ok"));
        assert!(steps.contains("Agent args: ok"));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn warns_when_configured_agent_cli_is_missing() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("claude CLI: missing"));
        assert!(warnings.contains("[agent].cli"));
    }

    #[test]
    fn warns_when_agent_command_override_is_missing() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: Some("my-agent --flag".into()),
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Agent command: missing"));
        assert!(warnings.contains("[agent].command"));
    }

    #[test]
    fn warns_when_none_agent_has_command_override() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::None,
                args: Vec::new(),
                command: Some("codex".into()),
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Agent command is ignored"));
        assert!(warnings.contains("agent.cli"));
    }

    #[test]
    fn checks_worktree_naming_command() {
        let config = Config {
            worktree: crate::config::WorktreeConfig {
                naming: Some(WorktreeNamingConfig::default()),
                ..Default::default()
            },
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("worktree.naming.command: missing"));
        assert!(warnings.contains("[worktree.naming].command"));
    }

    #[test]
    fn checks_cmux_when_workspace_is_configured() {
        let config = Config {
            workspace: Some(Default::default()),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("cmux CLI: missing"));
        assert!(warnings.contains("[workspace]"));
    }

    #[test]
    fn codex_hook_readiness_warns_when_codex_config_is_missing() {
        let temp = TempDir::new().unwrap();

        let checks = codex_hook_readiness_checks_for_home(temp.path());

        let hooks = check_by_name(&checks, "codex_hooks_json");
        assert_eq!(hooks.status, "warning");
        assert!(
            hooks
                .message
                .as_deref()
                .unwrap()
                .contains("missing hooks.json")
        );

        let config = check_by_name(&checks, "codex_config_hooks");
        assert_eq!(config.status, "warning");
        assert!(
            config
                .message
                .as_deref()
                .unwrap()
                .contains("missing config.toml")
        );
    }

    #[test]
    fn codex_hook_readiness_reports_installed_hooks() {
        let temp = TempDir::new().unwrap();
        write_codex_hooks(temp.path());
        write_codex_config(temp.path(), true);

        let checks = codex_hook_readiness_checks_for_home(temp.path());

        assert_eq!(check_by_name(&checks, "codex_hooks_json").status, "ok");
        assert_eq!(check_by_name(&checks, "codex_config_hooks").status, "ok");
        assert_eq!(check_by_name(&checks, "codex_hook_trust").status, "ok");
    }

    #[test]
    fn codex_hook_readiness_warns_when_trust_is_missing() {
        let temp = TempDir::new().unwrap();
        write_codex_hooks(temp.path());
        write_codex_config(temp.path(), false);

        let checks = codex_hook_readiness_checks_for_home(temp.path());

        assert_eq!(check_by_name(&checks, "codex_hooks_json").status, "ok");
        assert_eq!(check_by_name(&checks, "codex_config_hooks").status, "ok");
        let trust = check_by_name(&checks, "codex_hook_trust");
        assert_eq!(trust.status, "warning");
        assert!(
            trust
                .message
                .as_deref()
                .unwrap()
                .contains("PermissionRequest")
        );
    }

    #[test]
    fn codex_hook_readiness_reports_wt_inbox_hook_trust() {
        let temp = TempDir::new().unwrap();
        write_wt_codex_inbox_hook(temp.path(), true);

        let checks = codex_hook_readiness_checks_for_home(temp.path());

        assert_eq!(check_by_name(&checks, "codex_wt_inbox_hook").status, "ok");
        assert_eq!(check_by_name(&checks, "codex_wt_inbox_trust").status, "ok");
    }

    #[test]
    fn codex_hook_readiness_warns_when_wt_inbox_trust_is_missing() {
        let temp = TempDir::new().unwrap();
        write_wt_codex_inbox_hook(temp.path(), false);

        let checks = codex_hook_readiness_checks_for_home(temp.path());

        assert_eq!(check_by_name(&checks, "codex_wt_inbox_hook").status, "ok");
        let trust = check_by_name(&checks, "codex_wt_inbox_trust");
        assert_eq!(trust.status, "warning");
        assert!(
            trust
                .message
                .as_deref()
                .unwrap()
                .contains("user_prompt_submit")
        );
    }

    #[test]
    fn non_codex_agent_does_not_report_codex_hook_readiness() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            ..Default::default()
        };
        let mut checks = Vec::new();

        collect_codex_hook_readiness_checks(&config, &mut checks);

        assert!(checks.is_empty());
    }

    #[test]
    fn warns_when_agent_launch_has_no_workspace_config() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: vec!["--yolo".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
                ..AgentConfig::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx, None).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Workspace config: missing"));
        assert!(warnings.contains("launch the agent"));
        assert!(warnings.contains("cmux CLI: missing"));
    }

    fn write_profile_toml(repo_root: &Path, name: &str, body: &str) {
        let profile_dir = repo_root.join(".git/wt/profiles").join(name);
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(profile_dir.join("profile.toml"), body).unwrap();
    }

    #[test]
    fn text_output_shows_selected_profile_and_uses_profile_config() {
        let temp = TempDir::new().unwrap();
        write_profile_toml(
            temp.path(),
            "codex",
            r#"
[issues]
provider = "linear"
"#,
        );

        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with_root(
            temp.path().to_path_buf(),
            Config::default(),
            MockRunner::new(),
            ui,
        );

        run(&ctx, Some("codex")).unwrap();

        let steps_joined = steps.lock().unwrap().join("\n");
        let warnings_joined = warnings.lock().unwrap().join("\n");
        assert!(
            steps_joined.contains("Profile: codex"),
            "expected text output to surface selected profile, got steps:\n{steps_joined}"
        );
        assert!(
            steps_joined.contains("Issue provider: linear"),
            "expected linear provider from profile config, got steps:\n{steps_joined}"
        );
        assert!(
            warnings_joined.contains("linear CLI: missing"),
            "expected linear cli warning, got warnings:\n{warnings_joined}"
        );
    }

    #[test]
    fn run_fails_when_named_profile_is_missing() {
        let temp = TempDir::new().unwrap();
        let ctx = ctx_with_root(
            temp.path().to_path_buf(),
            Config::default(),
            MockRunner::new(),
            RecordingUi::new(),
        );

        let err = run(&ctx, Some("missing")).unwrap_err();
        assert!(
            err.to_string().contains("Profile 'missing' not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn run_rejects_reserved_profile_name() {
        let temp = TempDir::new().unwrap();
        let ctx = ctx_with_root(
            temp.path().to_path_buf(),
            Config::default(),
            MockRunner::new(),
            RecordingUi::new(),
        );

        let err = run(&ctx, Some("default")).unwrap_err();
        assert!(
            err.to_string().contains("reserved"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn json_report_includes_selected_profile_and_profile_checks() {
        let temp = TempDir::new().unwrap();
        write_profile_toml(
            temp.path(),
            "codex",
            r#"
[issues]
provider = "linear"
"#,
        );
        let ctx = ctx_with_root(
            temp.path().to_path_buf(),
            Config::default(),
            MockRunner::new(),
            RecordingUi::new(),
        );

        let config = Config::load_profile_from_storage(
            &ctx.repo_root,
            &ctx.storage_root,
            "codex",
            &ctx.base_config,
        )
        .unwrap()
        .unwrap();
        let report = build_report(&ctx, &config, Some("codex"));

        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["profile"], serde_json::Value::String("codex".into()));

        let checks = value["checks"].as_array().unwrap();
        let issue_provider = checks
            .iter()
            .find(|c| c["name"] == "issue_provider")
            .unwrap();
        assert_eq!(issue_provider["message"], "linear");

        let linear_cli = checks.iter().find(|c| c["name"] == "linear_cli").unwrap();
        assert_eq!(linear_cli["status"], "warning");
    }

    #[test]
    fn json_report_omits_profile_when_none_selected() {
        let temp = TempDir::new().unwrap();
        let ctx = ctx_with_root(
            temp.path().to_path_buf(),
            Config::default(),
            MockRunner::new(),
            RecordingUi::new(),
        );

        let report = build_report(&ctx, &ctx.config, None);
        let value = serde_json::to_value(&report).unwrap();
        assert!(
            value.get("profile").is_none(),
            "expected no profile field when none selected, got {value}"
        );
        assert!(value["checks"].is_array());
    }
}
