use crate::config::{AgentCli, Config};
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use crate::template;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub(super) fn inject_local_context(
    ctx: &Ctx,
    config: &Config,
    wt_path: &Path,
    names: &WorktreeNames,
    template_vars: &HashMap<String, String>,
    ws_handle: Option<&str>,
) -> Result<()> {
    let tmpl = match config.worktree.inject_local_context {
        Some(ref t) => t,
        None => return Ok(()),
    };

    let Some(context_file) = local_context_file(config) else {
        return Ok(());
    };
    let context_path = wt_path.join(context_file);
    if !context_path.exists() {
        return Ok(());
    }

    let git = GitService::new(ctx.runner.as_ref(), Some(wt_path));
    let parent = git.get_branch_parent(&names.branch).unwrap_or(None);

    let mut vars = template_vars.clone();
    if let Some(p) = parent {
        vars.insert("parent_branch".into(), p);
    }
    if let Some(ws) = ws_handle {
        vars.insert("workspace".into(), ws.into());
    }

    let rendered = template::render(tmpl, &vars);

    let mut content = fs::read_to_string(&context_path)?;
    content.push_str(&rendered);
    fs::write(&context_path, content)?;
    Ok(())
}

pub(super) fn append_agent_local_context(
    config: &Config,
    wt_path: &Path,
    context: &str,
) -> Result<()> {
    let Some(context_file) = local_context_file(config) else {
        return Ok(());
    };
    let context_path = wt_path.join(context_file);
    if !context_path.exists() {
        return Ok(());
    }

    let mut content = fs::read_to_string(&context_path)?;
    content.push_str(context);
    fs::write(&context_path, content)?;
    Ok(())
}

fn local_context_file(config: &Config) -> Option<&'static str> {
    match config.agent.as_ref().map(|agent| &agent.cli) {
        Some(AgentCli::Codex) => Some("AGENTS.override.md"),
        Some(AgentCli::Claude) => Some("CLAUDE.local.md"),
        Some(AgentCli::Gemini | AgentCli::None) | None => None,
    }
}
