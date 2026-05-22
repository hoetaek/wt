use crate::context::MachineCtx;
use crate::messages::AgentId;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

const CLAUDE_SETTINGS_PATH: &str = "settings.json";
pub(crate) const CLAUDE_HOOK_EVENTS: &[&str] = &["UserPromptSubmit", "PostToolUse"];
const WT_CLAUDE_HOOK_MARKER: &str = "# wt-agent-hook:claude-inbox";
const WT_CLAUDE_SUPERVISOR_SESSION_END_MARKER: &str =
    "# wt-agent-hook:claude-supervisor-session-end";
pub(crate) const CODEX_HOOK_EVENTS: &[(&str, &str)] = &[
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PostToolUse", "post_tool_use"),
];
const CODEX_DEFAULT_HOOK_TIMEOUT_SEC: u64 = 600;
const WT_CODEX_HOOK_MARKER: &str = "# wt-agent-hook:codex-inbox";

pub(crate) fn install_claude(ctx: &MachineCtx<'_>, agent: Option<&str>) -> Result<()> {
    let target = ClaudeHookTarget::parse(agent)?;
    let settings_path = claude_settings_path(true)?;

    let mut settings = read_settings(&settings_path)?;
    remove_managed_claude_hook(&mut settings, ClaudeRemoveTarget::AllWtManaged)?;
    let command = target.command();
    let session_end_command = target.session_end_command();
    install_managed_claude_hook(&mut settings, &command, &session_end_command)?;
    write_settings(&settings_path, &settings)?;

    if !ctx.quiet {
        ctx.ui
            .print_step(&format!("Claude hook installed for {}", target.label()));
        ctx.ui
            .print_dim(&format!("  Settings: {}", settings_path.display()));
        ctx.ui.print_dim(&format!("  Command: {command}"));
    }

    Ok(())
}

pub(crate) fn uninstall_claude(ctx: &MachineCtx<'_>, agent: Option<&str>) -> Result<()> {
    let target = ClaudeHookTarget::parse(agent)?;
    let settings_path = claude_settings_path(false)?;
    if !settings_path.exists() {
        if !ctx.quiet {
            ctx.ui
                .print_step(&format!("Claude hook not installed for {}", target.label()));
            ctx.ui
                .print_dim(&format!("  Settings: {}", settings_path.display()));
        }
        return Ok(());
    }

    let mut settings = read_settings(&settings_path)?;
    let remove_target = match &target {
        ClaudeHookTarget::Dispatcher => ClaudeRemoveTarget::AllWtManaged,
        ClaudeHookTarget::Agent(agent) => ClaudeRemoveTarget::Commands(vec![
            managed_claude_hook_command(agent.as_str()),
            managed_claude_supervisor_session_end_command(Some(agent.as_str())),
        ]),
    };
    let removed = remove_managed_claude_hook(&mut settings, remove_target)?;
    if removed > 0 {
        if settings_is_empty(&settings) {
            fs::remove_file(&settings_path).with_context(|| {
                format!(
                    "Failed to remove empty Claude settings: {}",
                    settings_path.display()
                )
            })?;
        } else {
            write_settings(&settings_path, &settings)?;
        }
    }

    if !ctx.quiet {
        let status = if removed == 0 {
            "not installed"
        } else {
            "uninstalled"
        };
        ctx.ui
            .print_step(&format!("Claude hook {status} for {}", target.label()));
        ctx.ui
            .print_dim(&format!("  Settings: {}", settings_path.display()));
    }

    Ok(())
}

pub(crate) fn claude_dispatcher_installed() -> Result<bool> {
    let settings_path = claude_settings_path(false)?;
    if !settings_path.exists() {
        return Ok(false);
    }
    let settings = read_settings(&settings_path)?;
    Ok(claude_has_command_for_all_events(
        &settings,
        &managed_claude_dispatcher_command(),
    ))
}

pub(crate) fn install_codex(ctx: &MachineCtx<'_>, agent: Option<&str>) -> Result<()> {
    let target = CodexHookTarget::parse(agent)?;
    let paths = codex_hook_paths(true)?;
    let command = target.command();

    let mut hooks = read_codex_hooks(&paths.hooks_path)?;
    let remove_target = match &target {
        CodexHookTarget::Dispatcher => CodexRemoveTarget::AllWtManaged,
        CodexHookTarget::Agent(_) => CodexRemoveTarget::Command(command.clone()),
    };
    let stale_trust_keys = remove_managed_codex_hook(&mut hooks, &paths, remove_target)?;
    install_managed_codex_hook(&mut hooks, &command)?;
    let trust_installs = find_managed_codex_hook_trust_installs(&hooks, &paths, &command)?;

    validate_codex_config_for_trust(&paths.config_path)?;
    write_codex_hooks(&paths.hooks_path, &hooks)?;
    write_codex_config_trust(
        &paths.config_path,
        CodexTrustUpdate {
            remove_keys: stale_trust_keys,
            install: trust_installs,
        },
    )?;

    if !ctx.quiet {
        ctx.ui
            .print_step(&format!("Codex hook installed for {}", target.label()));
        ctx.ui
            .print_dim(&format!("  Hooks: {}", paths.hooks_path.display()));
        ctx.ui
            .print_dim(&format!("  Config: {}", paths.config_path.display()));
        ctx.ui.print_dim(&format!("  Command: {command}"));
    }

    Ok(())
}

pub(crate) fn uninstall_codex(ctx: &MachineCtx<'_>, agent: Option<&str>) -> Result<()> {
    let target = CodexHookTarget::parse(agent)?;
    let paths = codex_hook_paths(false)?;
    if !paths.hooks_path.exists() && !paths.config_path.exists() {
        if !ctx.quiet {
            ctx.ui
                .print_step(&format!("Codex hook not installed for {}", target.label()));
            ctx.ui
                .print_dim(&format!("  Hooks: {}", paths.hooks_path.display()));
            ctx.ui
                .print_dim(&format!("  Config: {}", paths.config_path.display()));
        }
        return Ok(());
    }

    let mut hooks = read_codex_hooks(&paths.hooks_path)?;
    let remove_target = match &target {
        CodexHookTarget::Dispatcher => CodexRemoveTarget::AllWtManaged,
        CodexHookTarget::Agent(agent) => {
            CodexRemoveTarget::Command(managed_codex_hook_command(agent.as_str()))
        }
    };
    let trust_keys = remove_managed_codex_hook(&mut hooks, &paths, remove_target)?;
    write_codex_hooks(&paths.hooks_path, &hooks)?;
    if paths.config_path.exists() {
        write_codex_config_trust(
            &paths.config_path,
            CodexTrustUpdate {
                remove_keys: trust_keys.clone(),
                install: Vec::new(),
            },
        )?;
    }

    if !ctx.quiet {
        let status = if trust_keys.is_empty() {
            "not installed"
        } else {
            "uninstalled"
        };
        ctx.ui
            .print_step(&format!("Codex hook {status} for {}", target.label()));
        ctx.ui
            .print_dim(&format!("  Hooks: {}", paths.hooks_path.display()));
        ctx.ui
            .print_dim(&format!("  Config: {}", paths.config_path.display()));
    }

    Ok(())
}

pub(crate) fn codex_dispatcher_installed() -> Result<bool> {
    let paths = codex_hook_paths(false)?;
    if !paths.hooks_path.exists() || !paths.config_path.exists() {
        return Ok(false);
    }

    let hooks = read_codex_hooks(&paths.hooks_path)?;
    let command = managed_codex_dispatcher_command();
    let trust_installs = match find_managed_codex_hook_trust_installs(&hooks, &paths, &command) {
        Ok(installs) => installs,
        Err(_) => return Ok(false),
    };

    let content = fs::read_to_string(&paths.config_path).with_context(|| {
        format!(
            "Failed to read Codex config: {}",
            paths.config_path.display()
        )
    })?;
    let config = content.parse::<DocumentMut>().with_context(|| {
        format!(
            "Failed to parse Codex config TOML: {}. Fix config.toml before running `wt setup`.",
            paths.config_path.display()
        )
    })?;
    let Some(state) = config
        .get("hooks")
        .and_then(Item::as_table_like)
        .and_then(|hooks| hooks.get("state"))
        .and_then(Item::as_table_like)
    else {
        return Ok(false);
    };

    Ok(trust_installs.iter().all(|install| {
        state
            .get(&install.key)
            .and_then(Item::as_table_like)
            .and_then(|entry| entry.get("trusted_hash"))
            .and_then(Item::as_str)
            == Some(install.trusted_hash.as_str())
    }))
}

pub(crate) fn codex_dispatcher_hook_present() -> Result<bool> {
    let paths = codex_hook_paths(false)?;
    if !paths.hooks_path.exists() {
        return Ok(false);
    }
    let hooks = read_codex_hooks(&paths.hooks_path)?;
    Ok(codex_has_command_for_all_events(
        &hooks,
        &managed_codex_dispatcher_command(),
    ))
}

struct CodexHookPaths {
    hooks_path: PathBuf,
    config_path: PathBuf,
}

struct CodexTrustInstall {
    key: String,
    trusted_hash: String,
}

struct CodexTrustUpdate {
    remove_keys: Vec<String>,
    install: Vec<CodexTrustInstall>,
}

enum CodexHookTarget {
    Dispatcher,
    Agent(AgentId),
}

enum ClaudeHookTarget {
    Dispatcher,
    Agent(AgentId),
}

impl ClaudeHookTarget {
    fn parse(agent: Option<&str>) -> Result<Self> {
        agent
            .map(|agent| AgentId::parse(agent).map(Self::Agent))
            .transpose()
            .context("Invalid agent id")
            .map(|target| target.unwrap_or(Self::Dispatcher))
    }

    fn command(&self) -> String {
        match self {
            Self::Dispatcher => managed_claude_dispatcher_command(),
            Self::Agent(agent) => managed_claude_hook_command(agent.as_str()),
        }
    }

    fn session_end_command(&self) -> String {
        match self {
            Self::Dispatcher => managed_claude_supervisor_session_end_command(None),
            Self::Agent(agent) => {
                managed_claude_supervisor_session_end_command(Some(agent.as_str()))
            }
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Dispatcher => "WT_AGENT_ID/WT_COORDINATOR_AGENT_ID dispatcher".into(),
            Self::Agent(agent) => format!("manual override {}", agent.as_str()),
        }
    }
}

impl CodexHookTarget {
    fn parse(agent: Option<&str>) -> Result<Self> {
        agent
            .map(|agent| AgentId::parse(agent).map(Self::Agent))
            .transpose()
            .context("Invalid agent id")
            .map(|target| target.unwrap_or(Self::Dispatcher))
    }

    fn command(&self) -> String {
        match self {
            Self::Dispatcher => managed_codex_dispatcher_command(),
            Self::Agent(agent) => managed_codex_hook_command(agent.as_str()),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Dispatcher => "WT_AGENT_ID/WT_COORDINATOR_AGENT_ID dispatcher".into(),
            Self::Agent(agent) => format!("manual override {}", agent.as_str()),
        }
    }
}

enum CodexRemoveTarget {
    AllWtManaged,
    Command(String),
}

enum ClaudeRemoveTarget {
    AllWtManaged,
    Commands(Vec<String>),
}

fn codex_hook_paths(create_home: bool) -> Result<CodexHookPaths> {
    let codex_home = codex_home_dir()?;
    match fs::metadata(&codex_home) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => bail!(
            "Cannot install Codex hook: Codex home exists but is not a directory: {}",
            codex_home.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && create_home => {
            fs::create_dir_all(&codex_home).with_context(|| {
                format!("Failed to create Codex home: {}", codex_home.display())
            })?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to inspect Codex home: {}", codex_home.display())
            });
        }
    }

    Ok(CodexHookPaths {
        hooks_path: codex_home.join("hooks.json"),
        config_path: codex_home.join("config.toml"),
    })
}

pub(crate) fn codex_home_dir() -> Result<PathBuf> {
    let path = env::var_os("CODEX_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(path).join(".codex"))
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot resolve Codex home: CODEX_HOME and HOME are unset. Set CODEX_HOME or install Codex so ~/.codex exists."
            )
        })?;

    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .context("Failed to resolve relative CODEX_HOME")
    }
}

pub(crate) fn claude_settings_path(create_home: bool) -> Result<PathBuf> {
    let claude_home = env::var_os("CLAUDE_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(path).join(".claude"))
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot resolve Claude home: CLAUDE_HOME and HOME are unset. Set CLAUDE_HOME or install Claude so ~/.claude exists."
            )
        })?;

    let claude_home = if claude_home.is_absolute() {
        claude_home
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(claude_home))
            .context("Failed to resolve relative CLAUDE_HOME")?
    };

    match fs::metadata(&claude_home) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => bail!(
            "Cannot install Claude hook: Claude home exists but is not a directory: {}",
            claude_home.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && create_home => {
            fs::create_dir_all(&claude_home).with_context(|| {
                format!("Failed to create Claude home: {}", claude_home.display())
            })?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to inspect Claude home: {}", claude_home.display())
            });
        }
    }

    Ok(claude_home.join(CLAUDE_SETTINGS_PATH))
}

fn read_settings(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Claude settings: {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse Claude settings JSON: {}", path.display()))
}

fn write_settings(path: &Path, settings: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create Claude settings dir: {}", parent.display())
        })?;
    }
    let rendered = serde_json::to_string_pretty(settings)?;
    fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("Failed to write Claude settings: {}", path.display()))
}

fn read_codex_hooks(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({ "hooks": {} }));
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Codex hooks file: {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(json!({ "hooks": {} }));
    }
    serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse Codex hooks JSON: {}. Fix hooks.json before running `wt setup`.",
            path.display()
        )
    })
}

fn write_codex_hooks(path: &Path, hooks: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create Codex home: {}", parent.display()))?;
    }
    let rendered = serde_json::to_string_pretty(hooks)?;
    fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("Failed to write Codex hooks file: {}", path.display()))
}

fn install_managed_codex_hook(hooks: &mut Value, command: &str) -> Result<()> {
    let root = codex_hooks_object(hooks)?;
    let events = codex_object_entry(root, "hooks")?;
    for &(event_name, _) in CODEX_HOOK_EVENTS {
        let event = codex_array_entry(events, event_name)?;
        event.push(json!({
            "hooks": [
                {
                    "type": "command",
                    "command": command
                }
            ]
        }));
    }
    Ok(())
}

fn codex_has_command_for_all_events(hooks: &Value, command: &str) -> bool {
    CODEX_HOOK_EVENTS.iter().all(|(event_name, _)| {
        hooks
            .get("hooks")
            .and_then(|hooks| hooks.get(*event_name))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|entry| {
                entry
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|hook| is_managed_command(hook, command))
            })
    })
}

fn codex_remove_target_matches(target: &CodexRemoveTarget, hook: &Value) -> bool {
    match target {
        CodexRemoveTarget::AllWtManaged => hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(is_wt_managed_codex_command),
        CodexRemoveTarget::Command(command) => is_managed_command(hook, command),
    }
}

fn remove_managed_codex_hook(
    hooks: &mut Value,
    paths: &CodexHookPaths,
    target: CodexRemoveTarget,
) -> Result<Vec<String>> {
    let root = codex_hooks_object(hooks)?;
    let Some(events_value) = root.get_mut("hooks") else {
        return Ok(Vec::new());
    };
    let Some(events) = events_value.as_object_mut() else {
        bail!("Cannot update Codex hooks file: `hooks` must be a JSON object.");
    };

    let mut removed_keys = Vec::new();
    let mut empty_events = Vec::new();
    for &(event_name, event_key) in CODEX_HOOK_EVENTS {
        let Some(event_value) = events.get_mut(event_name) else {
            continue;
        };
        let Some(event_entries) = event_value.as_array_mut() else {
            bail!("Cannot update Codex hooks file: `hooks.{event_name}` must be an array.");
        };

        let mut kept = Vec::with_capacity(event_entries.len());
        for (group_index, mut entry) in std::mem::take(event_entries).into_iter().enumerate() {
            let removed = remove_codex_command_from_event_entry(
                &mut entry,
                paths,
                event_key,
                &target,
                group_index,
                &mut removed_keys,
            )?;
            if !(removed > 0 && event_entry_has_no_hooks(&entry)) {
                kept.push(entry);
            }
        }
        *event_entries = kept;

        if event_entries.is_empty() {
            empty_events.push(event_name.to_string());
        }
    }

    for event_name in empty_events {
        events.remove(&event_name);
    }
    if events.is_empty() {
        root.remove("hooks");
    }

    Ok(removed_keys)
}

fn remove_codex_command_from_event_entry(
    entry: &mut Value,
    paths: &CodexHookPaths,
    event_key: &str,
    target: &CodexRemoveTarget,
    group_index: usize,
    removed_keys: &mut Vec<String>,
) -> Result<usize> {
    let Some(entry) = entry.as_object_mut() else {
        bail!("Cannot update Codex hooks file: hook event entries must be JSON objects.");
    };
    let Some(hooks_value) = entry.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(hooks) = hooks_value.as_array_mut() else {
        bail!("Cannot update Codex hooks file: hook event `hooks` must be an array.");
    };

    let mut removed = 0;
    let mut kept = Vec::with_capacity(hooks.len());
    for (handler_index, hook) in std::mem::take(hooks).into_iter().enumerate() {
        if codex_remove_target_matches(target, &hook) {
            removed += 1;
            removed_keys.push(codex_trust_key(
                paths,
                event_key,
                group_index,
                handler_index,
            ));
        } else {
            kept.push(hook);
        }
    }
    *hooks = kept;

    Ok(removed)
}

fn find_managed_codex_hook_trust_installs(
    hooks: &Value,
    paths: &CodexHookPaths,
    command: &str,
) -> Result<Vec<CodexTrustInstall>> {
    let Some(events) = hooks.get("hooks").and_then(Value::as_object) else {
        bail!("Failed to locate installed Codex inbox hooks");
    };

    let mut installs = Vec::new();
    for &(event_name, event_key) in CODEX_HOOK_EVENTS {
        let Some(event_entries) = events.get(event_name).and_then(Value::as_array) else {
            bail!("Failed to locate installed Codex inbox hook for {event_name}");
        };
        let mut found = None;
        for (group_index, entry) in event_entries.iter().enumerate() {
            let Some(group_hooks) = entry.get("hooks").and_then(Value::as_array) else {
                bail!("Cannot inspect Codex hooks file: hook event `hooks` must be an array.");
            };
            for (handler_index, hook) in group_hooks.iter().enumerate() {
                if is_managed_command(hook, command) {
                    found = Some(CodexTrustInstall {
                        key: codex_trust_key(paths, event_key, group_index, handler_index),
                        trusted_hash: codex_command_hook_hash(command, event_key),
                    });
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        installs.push(found.ok_or_else(|| {
            anyhow::anyhow!("Failed to locate installed Codex inbox hook for {event_name}")
        })?);
    }

    Ok(installs)
}

fn codex_hooks_object(hooks: &mut Value) -> Result<&mut Map<String, Value>> {
    hooks.as_object_mut().ok_or_else(|| {
        anyhow::anyhow!("Cannot update Codex hooks file: top-level JSON value must be an object.")
    })
}

fn codex_object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
    value.as_object_mut().ok_or_else(|| {
        anyhow::anyhow!("Cannot update Codex hooks file: `{key}` must be a JSON object.")
    })
}

fn codex_array_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Vec<Value>> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!([]));
    value
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Cannot update Codex hooks file: `{key}` must be an array."))
}

fn write_codex_config_trust(path: &Path, update: CodexTrustUpdate) -> Result<()> {
    let content = if path.exists() {
        fs::read_to_string(path)
            .with_context(|| format!("Failed to read Codex config: {}", path.display()))?
    } else {
        String::new()
    };
    let mut document = content.parse::<DocumentMut>().with_context(|| {
        format!(
            "Failed to parse Codex config TOML: {}. Fix config.toml before running `wt setup`.",
            path.display()
        )
    })?;

    for key in &update.remove_keys {
        remove_codex_trust_key(&mut document, key);
    }

    if !update.install.is_empty() {
        ensure_codex_hooks_feature(&mut document)?;
        ensure_codex_hooks_table(&mut document)?;
    }

    for install in update.install {
        set_codex_trust_key(&mut document, &install.key, &install.trusted_hash)?;
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create Codex home: {}", parent.display()))?;
    }
    fs::write(path, document.to_string())
        .with_context(|| format!("Failed to write Codex config: {}", path.display()))
}

fn validate_codex_config_for_trust(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Codex config: {}", path.display()))?;
    content.parse::<DocumentMut>().with_context(|| {
        format!(
            "Failed to parse Codex config TOML: {}. Fix config.toml before running `wt setup`.",
            path.display()
        )
    })?;
    Ok(())
}

fn ensure_codex_hooks_feature(document: &mut DocumentMut) -> Result<()> {
    if !document
        .get("features")
        .and_then(Item::as_table_like)
        .is_some_and(|features| {
            features
                .get("hooks")
                .and_then(Item::as_bool)
                .is_some_and(|enabled| enabled)
        })
    {
        if document.get("features").is_none() {
            document["features"] = Item::Table(Table::new());
        }
        let Some(features) = document["features"].as_table_like_mut() else {
            bail!("Cannot update Codex config: `features` must be a TOML table.");
        };
        features.insert("hooks", value(true));
    }

    Ok(())
}

fn ensure_codex_hooks_table(document: &mut DocumentMut) -> Result<()> {
    if document.get("hooks").and_then(Item::as_bool).is_some() {
        document.as_table_mut().remove("hooks");
    }
    if document.get("hooks").is_none() {
        document["hooks"] = Item::Table(Table::new());
    }
    if document["hooks"].as_table_like_mut().is_none() {
        bail!(
            "Cannot update Codex config: `hooks` must be a TOML table to store hook trust state."
        );
    }
    Ok(())
}

fn remove_codex_trust_key(document: &mut DocumentMut, key: &str) {
    let Some(hooks) = document.get_mut("hooks").and_then(Item::as_table_like_mut) else {
        return;
    };
    let Some(state) = hooks.get_mut("state").and_then(Item::as_table_like_mut) else {
        return;
    };
    state.remove(key);
}

fn set_codex_trust_key(document: &mut DocumentMut, key: &str, trusted_hash: &str) -> Result<()> {
    let Some(hooks) = document["hooks"].as_table_like_mut() else {
        bail!("Cannot update Codex config: `hooks` must be a TOML table.");
    };
    if hooks.get("state").is_none() {
        hooks.insert("state", Item::Table(Table::new()));
    }
    let Some(state) = hooks.get_mut("state").and_then(Item::as_table_like_mut) else {
        bail!("Cannot update Codex config: `hooks.state` must be a TOML table.");
    };
    if state.get(key).is_none() {
        state.insert(key, Item::Table(Table::new()));
    }
    let Some(entry) = state.get_mut(key).and_then(Item::as_table_like_mut) else {
        bail!("Cannot update Codex config: hook state entry `{key}` must be a TOML table.");
    };
    entry.insert("enabled", value(true));
    entry.insert("trusted_hash", value(trusted_hash));
    Ok(())
}

fn install_managed_claude_hook(
    settings: &mut Value,
    command: &str,
    session_end_command: &str,
) -> Result<()> {
    let root = settings_object(settings)?;
    let hooks = object_entry(root, "hooks")?;
    for &event_name in CLAUDE_HOOK_EVENTS {
        let event = array_entry(hooks, event_name)?;
        event.push(json!({
            "hooks": [
                {
                    "type": "command",
                    "command": command
                }
            ]
        }));
    }
    let session_end = array_entry(hooks, "SessionEnd")?;
    session_end.push(json!({
    "hooks": [
            {
                "type": "command",
                "command": session_end_command
            }
        ]
    }));
    Ok(())
}

fn claude_has_command_for_all_events(settings: &Value, command: &str) -> bool {
    CLAUDE_HOOK_EVENTS.iter().all(|event_name| {
        settings
            .get("hooks")
            .and_then(|hooks| hooks.get(*event_name))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|entry| {
                entry
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .any(|hook| is_managed_command(hook, command))
            })
    })
}

fn remove_managed_claude_hook(settings: &mut Value, target: ClaudeRemoveTarget) -> Result<usize> {
    let root = settings_object(settings)?;
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(hooks) = hooks_value.as_object_mut() else {
        bail!("Cannot update Claude settings: `hooks` must be a JSON object.");
    };

    let mut removed = 0;
    let mut empty_events = Vec::new();
    for event_name in CLAUDE_HOOK_EVENTS
        .iter()
        .copied()
        .chain(std::iter::once("SessionEnd"))
    {
        let Some(event_value) = hooks.get_mut(event_name) else {
            continue;
        };
        let Some(event_entries) = event_value.as_array_mut() else {
            bail!("Cannot update Claude settings: `hooks.{event_name}` must be an array.");
        };

        let mut kept = Vec::with_capacity(event_entries.len());
        for mut entry in std::mem::take(event_entries) {
            let removed_from_entry = remove_claude_command_from_event_entry(&mut entry, &target)?;
            removed += removed_from_entry;
            if !(removed_from_entry > 0 && event_entry_has_no_hooks(&entry)) {
                kept.push(entry);
            }
        }
        *event_entries = kept;

        if event_entries.is_empty() {
            empty_events.push(event_name.to_string());
        }
    }

    for event_name in empty_events {
        hooks.remove(&event_name);
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }

    Ok(removed)
}

fn remove_claude_command_from_event_entry(
    entry: &mut Value,
    target: &ClaudeRemoveTarget,
) -> Result<usize> {
    let Some(entry) = entry.as_object_mut() else {
        bail!("Cannot update Claude settings: hook event entries must be JSON objects.");
    };
    let Some(hooks_value) = entry.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(hooks) = hooks_value.as_array_mut() else {
        bail!("Cannot update Claude settings: hook event `hooks` must be an array.");
    };
    let before = hooks.len();
    hooks.retain(|hook| !claude_remove_target_matches(target, hook));
    Ok(before - hooks.len())
}

fn claude_remove_target_matches(target: &ClaudeRemoveTarget, hook: &Value) -> bool {
    match target {
        ClaudeRemoveTarget::AllWtManaged => hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(is_wt_managed_claude_command),
        ClaudeRemoveTarget::Commands(commands) => commands
            .iter()
            .any(|command| is_managed_command(hook, command)),
    }
}

fn is_managed_command(hook: &Value, command: &str) -> bool {
    hook.get("type").and_then(Value::as_str) == Some("command")
        && hook.get("command").and_then(Value::as_str) == Some(command)
}

fn event_entry_has_no_hooks(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
}

fn settings_object(settings: &mut Value) -> Result<&mut Map<String, Value>> {
    settings.as_object_mut().ok_or_else(|| {
        anyhow::anyhow!("Cannot update Claude settings: top-level JSON value must be an object.")
    })
}

fn object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
    value.as_object_mut().ok_or_else(|| {
        anyhow::anyhow!("Cannot update Claude settings: `{key}` must be a JSON object.")
    })
}

fn array_entry<'a>(object: &'a mut Map<String, Value>, key: &str) -> Result<&'a mut Vec<Value>> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!([]));
    value
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Cannot update Claude settings: `{key}` must be an array."))
}

fn managed_claude_hook_command(agent: &str) -> String {
    format!("wt msg check-inbox --agent {agent} --silent {WT_CLAUDE_HOOK_MARKER}")
}

fn managed_claude_dispatcher_command() -> String {
    format!("wt msg check-inbox --silent {WT_CLAUDE_HOOK_MARKER}")
}

fn managed_claude_supervisor_session_end_command(owner: Option<&str>) -> String {
    match owner {
        Some(owner) => {
            format!(
                "wt agent supervisor stop --owned-by {owner} {WT_CLAUDE_SUPERVISOR_SESSION_END_MARKER}"
            )
        }
        None => format!(
            "if [ -n \"${{WT_AGENT_ID:-}}\" ]; then wt agent supervisor stop --owned-by \"$WT_AGENT_ID\"; fi {WT_CLAUDE_SUPERVISOR_SESSION_END_MARKER}"
        ),
    }
}

fn managed_codex_hook_command(agent: &str) -> String {
    format!("wt msg check-inbox --agent {agent} --silent {WT_CODEX_HOOK_MARKER}")
}

fn managed_codex_dispatcher_command() -> String {
    format!("wt msg check-inbox --silent {WT_CODEX_HOOK_MARKER}")
}

fn codex_trust_key(
    paths: &CodexHookPaths,
    event_key: &str,
    group_index: usize,
    handler_index: usize,
) -> String {
    codex_event_trust_key(&paths.hooks_path, event_key, group_index, handler_index)
}

pub(crate) fn codex_event_trust_key(
    hooks_path: &Path,
    event_key: &str,
    group_index: usize,
    handler_index: usize,
) -> String {
    format!(
        "{}:{event_key}:{group_index}:{handler_index}",
        hooks_path.display()
    )
}

pub(crate) fn is_wt_managed_codex_command(command: &str) -> bool {
    command.contains(WT_CODEX_HOOK_MARKER) && command.contains("wt msg check-inbox")
}

fn is_wt_managed_claude_command(command: &str) -> bool {
    (command.contains(WT_CLAUDE_HOOK_MARKER) && command.contains("wt msg check-inbox"))
        || (command.contains(WT_CLAUDE_SUPERVISOR_SESSION_END_MARKER)
            && command.contains("wt agent supervisor stop"))
}

pub(crate) fn codex_command_hook_hash(command: &str, event_key: &str) -> String {
    let identity = json!({
        "event_name": event_key,
        "hooks": [
            {
                "async": false,
                "command": command,
                "timeout": CODEX_DEFAULT_HOOK_TIMEOUT_SEC,
                "type": "command",
            }
        ]
    });
    version_for_canonical_json(&identity)
}

fn version_for_canonical_json(value: &Value) -> String {
    let canonical = canonical_json(value);
    let serialized = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(serialized);
    sha256_version(hasher.finalize().as_ref())
}

fn sha256_version(bytes: &[u8]) -> String {
    let mut version = String::with_capacity("sha256:".len() + bytes.len() * 2);
    version.push_str("sha256:");
    for byte in bytes {
        let _ = write!(&mut version, "{byte:02x}");
    }
    version
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                if let Some(value) = map.get(key) {
                    sorted.insert(key.clone(), canonical_json(value));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn settings_is_empty(settings: &Value) -> bool {
    settings.as_object().is_some_and(Map::is_empty)
}
