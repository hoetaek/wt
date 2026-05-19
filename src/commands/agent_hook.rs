use crate::context::Ctx;
use crate::messages::AgentId;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CLAUDE_SETTINGS_PATH: &str = ".claude/settings.local.json";
const CLAUDE_HOOK_EVENT: &str = "UserPromptSubmit";
const WT_CLAUDE_HOOK_MARKER: &str = "# wt-agent-hook:claude-inbox";

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

struct ClaudeHookPaths {
    settings_path: PathBuf,
    display_settings: String,
    exclude_path: PathBuf,
    exclude_patterns: Vec<String>,
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
