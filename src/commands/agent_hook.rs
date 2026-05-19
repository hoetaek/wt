use crate::context::Ctx;
use crate::messages::AgentId;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

const CLAUDE_SETTINGS_PATH: &str = ".claude/settings.local.json";
const CLAUDE_HOOK_EVENT: &str = "UserPromptSubmit";
const WT_CLAUDE_HOOK_MARKER: &str = "# wt-agent-hook:claude-inbox";
const CODEX_HOOK_EVENT: &str = "UserPromptSubmit";
const CODEX_HOOK_EVENT_KEY: &str = "user_prompt_submit";
const CODEX_DEFAULT_HOOK_TIMEOUT_SEC: u64 = 600;
const WT_CODEX_HOOK_MARKER: &str = "# wt-agent-hook:codex-inbox";

pub(crate) fn install_claude(ctx: &Ctx, agent: &str) -> Result<()> {
    let agent = AgentId::parse(agent).context("Invalid agent id")?;
    let paths = claude_hook_paths(ctx, true)?;
    ensure_settings_path_is_untracked(ctx, &paths)?;
    ensure_git_excludes(ctx, &paths.exclude_patterns)?;

    let mut settings = read_settings(&paths.settings_path)?;
    remove_managed_claude_hook(&mut settings, agent.as_str())?;
    install_managed_claude_hook(&mut settings, agent.as_str())?;
    write_settings(&paths.settings_path, &settings)?;

    if !ctx.quiet {
        ctx.ui
            .print_step(&format!("Claude hook installed for {}", agent.as_str()));
        ctx.ui
            .print_dim(&format!("  Settings: {}", paths.display_settings));
        ctx.ui.print_dim(&format!(
            "  Command: wt msg check-inbox --agent {}",
            agent.as_str()
        ));
        ctx.ui
            .print_dim(&format!("  Git exclude: {}", paths.exclude_path.display()));
    }

    Ok(())
}

pub(crate) fn uninstall_claude(ctx: &Ctx, agent: &str) -> Result<()> {
    let agent = AgentId::parse(agent).context("Invalid agent id")?;
    let paths = claude_hook_paths(ctx, false)?;
    if !paths.settings_path.exists() {
        if !ctx.quiet {
            ctx.ui
                .print_step(&format!("Claude hook not installed for {}", agent.as_str()));
            ctx.ui
                .print_dim(&format!("  Settings: {}", paths.display_settings));
        }
        return Ok(());
    }

    ensure_settings_path_is_untracked(ctx, &paths)?;
    let mut settings = read_settings(&paths.settings_path)?;
    let removed = remove_managed_claude_hook(&mut settings, agent.as_str())?;
    if removed > 0 {
        if settings_is_empty(&settings) {
            fs::remove_file(&paths.settings_path).with_context(|| {
                format!(
                    "Failed to remove empty Claude local settings: {}",
                    paths.settings_path.display()
                )
            })?;
        } else {
            write_settings(&paths.settings_path, &settings)?;
        }
    }

    if !ctx.quiet {
        let status = if removed == 0 {
            "not installed"
        } else {
            "uninstalled"
        };
        ctx.ui
            .print_step(&format!("Claude hook {status} for {}", agent.as_str()));
        ctx.ui
            .print_dim(&format!("  Settings: {}", paths.display_settings));
    }

    Ok(())
}

pub(crate) fn install_codex(ctx: &Ctx, agent: Option<&str>) -> Result<()> {
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
    let trust_key = find_managed_codex_hook_key(&hooks, &paths, &command)?
        .ok_or_else(|| anyhow::anyhow!("Failed to locate installed Codex hook trust key"))?;
    let trusted_hash = codex_command_hook_hash(&command);

    validate_codex_config_for_trust(&paths.config_path)?;
    write_codex_hooks(&paths.hooks_path, &hooks)?;
    write_codex_config_trust(
        &paths.config_path,
        CodexTrustUpdate {
            remove_keys: stale_trust_keys,
            install: Some(CodexTrustInstall {
                key: trust_key,
                trusted_hash,
            }),
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

pub(crate) fn uninstall_codex(ctx: &Ctx, agent: Option<&str>) -> Result<()> {
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
                install: None,
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

struct ClaudeHookPaths {
    settings_path: PathBuf,
    display_settings: String,
    exclude_path: PathBuf,
    exclude_patterns: Vec<String>,
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
    install: Option<CodexTrustInstall>,
}

enum CodexHookTarget {
    Dispatcher,
    Agent(AgentId),
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
            Self::Dispatcher => "WT_AGENT_ID dispatcher".into(),
            Self::Agent(agent) => format!("manual override {}", agent.as_str()),
        }
    }
}

enum CodexRemoveTarget {
    AllWtManaged,
    Command(String),
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

fn claude_hook_paths(ctx: &Ctx, create_parent: bool) -> Result<ClaudeHookPaths> {
    let claude_dir = ctx.invocation_root.join(".claude");
    if create_parent {
        ensure_claude_dir(&claude_dir)?;
    }

    let settings_path = ctx.invocation_root.join(CLAUDE_SETTINGS_PATH);
    let mut exclude_patterns = BTreeSet::from([CLAUDE_SETTINGS_PATH.to_string()]);
    if let Some(actual_path) = actual_settings_path(&settings_path) {
        if let Some(relative) = worktree_relative_path(&ctx.invocation_root, &actual_path) {
            exclude_patterns.insert(relative);
        }
    }

    Ok(ClaudeHookPaths {
        settings_path,
        display_settings: CLAUDE_SETTINGS_PATH.into(),
        exclude_path: git_exclude_path(ctx)?,
        exclude_patterns: exclude_patterns.into_iter().collect(),
    })
}

fn ensure_claude_dir(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if path.is_dir() {
                Ok(())
            } else {
                bail!(
                    "Cannot install Claude hook: {} is a symlink that does not resolve to a directory.",
                    path.display()
                );
            }
        }
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => bail!(
            "Cannot install Claude hook: {} exists but is not a directory.",
            path.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)
            .with_context(|| {
                format!(
                    "Failed to create Claude local settings dir: {}",
                    path.display()
                )
            }),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to inspect Claude local settings dir: {}",
                path.display()
            )
        }),
    }
}

fn actual_settings_path(settings_path: &Path) -> Option<PathBuf> {
    let file_name = settings_path.file_name()?;
    let parent = settings_path.parent()?.canonicalize().ok()?;
    Some(parent.join(file_name))
}

fn worktree_relative_path(root: &Path, path: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let relative = path.strip_prefix(root).ok()?;
    Some(path_to_git_pattern(relative))
}

fn git_exclude_path(ctx: &Ctx) -> Result<PathBuf> {
    let out = ctx.runner.run(
        "git",
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ],
        Some(&ctx.invocation_root),
    )?;
    if !out.success {
        bail!(
            "Failed to resolve per-worktree Git exclude file: {}",
            command_error(&out.stdout, &out.stderr)
        );
    }
    Ok(PathBuf::from(out.stdout.trim()))
}

fn ensure_settings_path_is_untracked(ctx: &Ctx, paths: &ClaudeHookPaths) -> Result<()> {
    for pattern in &paths.exclude_patterns {
        if is_tracked_path(ctx, pattern)? {
            bail!(
                "Refusing to modify tracked Claude settings file `{pattern}`. `wt agent hook install claude` only writes worktree-local untracked settings. Remove that path from Git tracking or edit the tracked settings manually if you intentionally want a shared project hook."
            );
        }
    }
    Ok(())
}

fn is_tracked_path(ctx: &Ctx, path: &str) -> Result<bool> {
    let out = ctx.runner.run(
        "git",
        &["ls-files", "--error-unmatch", "--", path],
        Some(&ctx.invocation_root),
    )?;
    Ok(out.success)
}

fn ensure_git_excludes(ctx: &Ctx, patterns: &[String]) -> Result<()> {
    let exclude_path = git_exclude_path(ctx)?;
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create per-worktree Git exclude dir: {}",
                parent.display()
            )
        })?;
    }

    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    let existing_lines = existing.lines().map(str::trim).collect::<BTreeSet<_>>();
    let missing = patterns
        .iter()
        .filter(|pattern| !existing_lines.contains(pattern.as_str()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.contains("# wt Claude hook adapter") {
        updated.push_str("# wt Claude hook adapter\n");
    }
    for pattern in missing {
        updated.push_str(pattern);
        updated.push('\n');
    }

    fs::write(&exclude_path, updated).with_context(|| {
        format!(
            "Failed to update per-worktree Git exclude file: {}",
            exclude_path.display()
        )
    })
}

fn read_settings(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read Claude local settings: {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse Claude local settings JSON: {}",
            path.display()
        )
    })
}

fn write_settings(path: &Path, settings: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create Claude local settings dir: {}",
                parent.display()
            )
        })?;
    }
    let rendered = serde_json::to_string_pretty(settings)?;
    fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("Failed to write Claude local settings: {}", path.display()))
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
            "Failed to parse Codex hooks JSON: {}. Fix hooks.json before running `wt agent hook install codex`.",
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
    let event = codex_array_entry(events, CODEX_HOOK_EVENT)?;
    event.push(json!({
        "hooks": [
            {
                "type": "command",
                "command": command
            }
        ]
    }));
    Ok(())
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
    let Some(event_value) = events.get_mut(CODEX_HOOK_EVENT) else {
        return Ok(Vec::new());
    };
    let Some(event_entries) = event_value.as_array_mut() else {
        bail!("Cannot update Codex hooks file: `hooks.{CODEX_HOOK_EVENT}` must be an array.");
    };

    let mut removed_keys = Vec::new();
    let mut kept = Vec::with_capacity(event_entries.len());
    for (group_index, mut entry) in std::mem::take(event_entries).into_iter().enumerate() {
        let removed = remove_codex_command_from_event_entry(
            &mut entry,
            paths,
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
        events.remove(CODEX_HOOK_EVENT);
    }
    if events.is_empty() {
        root.remove("hooks");
    }

    Ok(removed_keys)
}

fn remove_codex_command_from_event_entry(
    entry: &mut Value,
    paths: &CodexHookPaths,
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
            removed_keys.push(codex_trust_key(paths, group_index, handler_index));
        } else {
            kept.push(hook);
        }
    }
    *hooks = kept;

    Ok(removed)
}

fn find_managed_codex_hook_key(
    hooks: &Value,
    paths: &CodexHookPaths,
    command: &str,
) -> Result<Option<String>> {
    let Some(events) = hooks.get("hooks").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(event_entries) = events.get(CODEX_HOOK_EVENT).and_then(Value::as_array) else {
        return Ok(None);
    };
    for (group_index, entry) in event_entries.iter().enumerate() {
        let Some(group_hooks) = entry.get("hooks").and_then(Value::as_array) else {
            bail!("Cannot inspect Codex hooks file: hook event `hooks` must be an array.");
        };
        for (handler_index, hook) in group_hooks.iter().enumerate() {
            if is_managed_command(hook, command) {
                return Ok(Some(codex_trust_key(paths, group_index, handler_index)));
            }
        }
    }

    Ok(None)
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
            "Failed to parse Codex config TOML: {}. Fix config.toml before running `wt agent hook install codex`.",
            path.display()
        )
    })?;

    for key in &update.remove_keys {
        remove_codex_trust_key(&mut document, key);
    }

    if let Some(install) = update.install {
        ensure_codex_hooks_feature(&mut document)?;
        ensure_codex_hooks_table(&mut document)?;
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
            "Failed to parse Codex config TOML: {}. Fix config.toml before running `wt agent hook install codex`.",
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

fn install_managed_claude_hook(settings: &mut Value, agent: &str) -> Result<()> {
    let root = settings_object(settings)?;
    let hooks = object_entry(root, "hooks")?;
    let event = array_entry(hooks, CLAUDE_HOOK_EVENT)?;
    event.push(json!({
        "hooks": [
            {
                "type": "command",
                "command": managed_hook_command(agent)
            }
        ]
    }));
    Ok(())
}

fn remove_managed_claude_hook(settings: &mut Value, agent: &str) -> Result<usize> {
    let root = settings_object(settings)?;
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(hooks) = hooks_value.as_object_mut() else {
        bail!("Cannot update Claude local settings: `hooks` must be a JSON object.");
    };
    let Some(event_value) = hooks.get_mut(CLAUDE_HOOK_EVENT) else {
        return Ok(0);
    };
    let Some(event_entries) = event_value.as_array_mut() else {
        bail!("Cannot update Claude local settings: `hooks.{CLAUDE_HOOK_EVENT}` must be an array.");
    };

    let command = managed_hook_command(agent);
    let mut removed = 0;
    let mut kept = Vec::with_capacity(event_entries.len());
    for mut entry in std::mem::take(event_entries) {
        let removed_from_entry = remove_command_from_event_entry(&mut entry, &command)?;
        removed += removed_from_entry;
        if !(removed_from_entry > 0 && event_entry_has_no_hooks(&entry)) {
            kept.push(entry);
        }
    }
    *event_entries = kept;

    if event_entries.is_empty() {
        hooks.remove(CLAUDE_HOOK_EVENT);
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }

    Ok(removed)
}

fn remove_command_from_event_entry(entry: &mut Value, command: &str) -> Result<usize> {
    let Some(entry) = entry.as_object_mut() else {
        bail!("Cannot update Claude local settings: hook event entries must be JSON objects.");
    };
    let Some(hooks_value) = entry.get_mut("hooks") else {
        return Ok(0);
    };
    let Some(hooks) = hooks_value.as_array_mut() else {
        bail!("Cannot update Claude local settings: hook event `hooks` must be an array.");
    };
    let before = hooks.len();
    hooks.retain(|hook| !is_managed_command(hook, command));
    Ok(before - hooks.len())
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
        anyhow::anyhow!(
            "Cannot update Claude local settings: top-level JSON value must be an object."
        )
    })
}

fn object_entry<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
    value.as_object_mut().ok_or_else(|| {
        anyhow::anyhow!("Cannot update Claude local settings: `{key}` must be a JSON object.")
    })
}

fn array_entry<'a>(object: &'a mut Map<String, Value>, key: &str) -> Result<&'a mut Vec<Value>> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!([]));
    value.as_array_mut().ok_or_else(|| {
        anyhow::anyhow!("Cannot update Claude local settings: `{key}` must be an array.")
    })
}

fn managed_hook_command(agent: &str) -> String {
    format!("wt msg check-inbox --agent {agent} {WT_CLAUDE_HOOK_MARKER}")
}

fn managed_codex_hook_command(agent: &str) -> String {
    format!("wt msg check-inbox --agent {agent} {WT_CODEX_HOOK_MARKER}")
}

fn managed_codex_dispatcher_command() -> String {
    format!(
        "if [ -n \"${{WT_AGENT_ID:-}}\" ]; then wt msg check-inbox --agent \"$WT_AGENT_ID\"; fi {WT_CODEX_HOOK_MARKER}"
    )
}

fn codex_trust_key(paths: &CodexHookPaths, group_index: usize, handler_index: usize) -> String {
    codex_user_prompt_trust_key(&paths.hooks_path, group_index, handler_index)
}

pub(crate) fn codex_user_prompt_trust_key(
    hooks_path: &Path,
    group_index: usize,
    handler_index: usize,
) -> String {
    format!(
        "{}:{CODEX_HOOK_EVENT_KEY}:{group_index}:{handler_index}",
        hooks_path.display()
    )
}

pub(crate) fn is_wt_managed_codex_command(command: &str) -> bool {
    command.contains(WT_CODEX_HOOK_MARKER) && command.contains("wt msg check-inbox --agent ")
}

pub(crate) fn codex_command_hook_hash(command: &str) -> String {
    let identity = json!({
        "event_name": CODEX_HOOK_EVENT_KEY,
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
    format!("sha256:{:x}", hasher.finalize())
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

fn path_to_git_pattern(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn command_error(stdout: &str, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    }
}
