use std::collections::HashMap;

use super::schema::{
    PROMPT_COMMON_SCOPE, PROMPT_RUNTIME_MODES, default_agent_ready, default_agent_send_after,
    default_agent_submit, default_agent_timeout, prompt_append_mode,
};
use super::{
    AgentCli, AgentConfig, Config, CopyAsEntry, EditorConfig, SetupConfig, WorkflowConfig,
    WorkspaceConfig, WorktreeConfig,
};

pub(super) fn merge_config(base: &Config, profile: Config) -> Config {
    let mut merged = base.clone();

    if profile.worktree != WorktreeConfig::default() {
        merge_worktree_config(&mut merged.worktree, profile.worktree);
    }
    if profile.setup != SetupConfig::default() {
        merge_setup_config(&mut merged.setup, profile.setup);
    }
    if profile.workflow != WorkflowConfig::default() {
        merge_workflow_config(&mut merged.workflow, profile.workflow);
    }
    if profile.profile.is_some() {
        merged.profile = profile.profile;
    }
    if profile.site.is_some() {
        merged.site = profile.site;
    }
    if profile.editor != EditorConfig::default() {
        merge_editor_config(&mut merged.editor, profile.editor);
    }
    if profile.workspace.is_some() {
        merged.workspace = match (merged.workspace.take(), profile.workspace) {
            (Some(mut base_workspace), Some(profile_workspace)) => {
                merge_workspace_config(&mut base_workspace, profile_workspace);
                Some(base_workspace)
            }
            (_, profile_workspace) => profile_workspace,
        };
    }
    if let Some(agent) = profile.agent {
        merged.agent = Some(match merged.agent.take() {
            Some(base_agent) => merge_agent_config(base_agent, agent),
            None => {
                let mut agent = agent;
                finalize_agent_prompt_appends(&mut agent);
                agent
            }
        });
    }
    if profile.test.is_some() {
        merged.test = profile.test;
    }
    if profile.issues.is_some() {
        merged.issues = profile.issues;
    }

    merged
}

pub(super) fn finalize_config_prompt_appends(config: &mut Config) {
    if let Some(agent) = config.agent.as_mut() {
        finalize_agent_prompt_appends(agent);
    }
}

pub(super) fn finalize_config_common_prompt_scope(config: &mut Config) {
    if let Some(agent) = config.agent.as_mut() {
        finalize_agent_common_prompt_scope(agent);
    }
}

fn merge_agent_config(mut base: AgentConfig, profile: AgentConfig) -> AgentConfig {
    if is_prompt_only_agent_patch(&profile) {
        apply_prompt_overlay(&mut base.prompt, profile.prompt);
        return base;
    }

    let mut profile = profile;
    finalize_agent_prompt_appends(&mut profile);
    profile
}

fn is_prompt_only_agent_patch(agent: &AgentConfig) -> bool {
    agent.cli == AgentCli::None
        && agent.args.is_empty()
        && agent.command.is_none()
        && agent.ready == default_agent_ready()
        && agent.submit == default_agent_submit()
        && agent.timeout == default_agent_timeout()
        && agent.send_after == default_agent_send_after()
        && !agent.prompt.is_empty()
}

fn finalize_agent_prompt_appends(agent: &mut AgentConfig) {
    let prompt = std::mem::take(&mut agent.prompt);
    apply_prompt_overlay(&mut agent.prompt, prompt);
}

fn finalize_agent_common_prompt_scope(agent: &mut AgentConfig) {
    let Some(common_prompts) = agent.prompt.remove(PROMPT_COMMON_SCOPE) else {
        return;
    };
    if common_prompts.is_empty() {
        return;
    }

    for mode in PROMPT_RUNTIME_MODES {
        let mode_prompts = agent.prompt.remove(mode).unwrap_or_default();
        let mut prompts = common_prompts.clone();
        prompts.extend(mode_prompts);
        agent.prompt.insert(mode.to_string(), prompts);
    }
}

fn apply_prompt_overlay(
    target: &mut HashMap<String, Vec<String>>,
    overlay: HashMap<String, Vec<String>>,
) {
    let mut appends = Vec::new();

    for (mode, prompts) in overlay {
        if let Some(append_mode) = prompt_append_mode(&mode) {
            appends.push((append_mode.to_string(), prompts));
        } else {
            target.insert(mode, prompts);
        }
    }

    appends.sort_by(|a, b| a.0.cmp(&b.0));
    for (mode, prompts) in appends {
        append_prompt_blocks(target.entry(mode).or_default(), prompts);
    }
}

pub(super) fn append_prompt_blocks(target: &mut Vec<String>, additions: Vec<String>) {
    for addition in additions {
        if let Some(first_prompt) = target.first_mut() {
            append_prompt_block(first_prompt, &addition);
        } else {
            target.push(addition);
        }
    }
}

fn append_prompt_block(target: &mut String, addition: &str) {
    let addition = addition.trim_start_matches(['\n', '\r']);
    if addition.is_empty() {
        return;
    }

    if target.is_empty() {
        target.push_str(addition);
        return;
    }

    let trimmed = target.trim_end_matches(['\n', '\r']).to_string();
    target.clear();
    target.push_str(&trimmed);
    target.push_str("\n\n");
    target.push_str(addition);
}

fn merge_worktree_config(base: &mut WorktreeConfig, profile: WorktreeConfig) {
    if profile.path.is_some() {
        base.path = profile.path;
    }
    extend_unique(&mut base.copy, profile.copy);
    extend_copy_as_unique(&mut base.copy_as, profile.copy_as);
    extend_unique(&mut base.link, profile.link);
    if profile.inject_local_context.is_some() {
        base.inject_local_context = profile.inject_local_context;
    }
    if profile.naming.is_some() {
        base.naming = profile.naming;
    }
}

fn merge_setup_config(base: &mut SetupConfig, profile: SetupConfig) {
    base.deps.extend(profile.deps);
    base.env.extend(profile.env);
    for (path, entries) in profile.env_files {
        base.env_files.entry(path).or_default().extend(entries);
    }
}

fn merge_workflow_config(base: &mut WorkflowConfig, profile: WorkflowConfig) {
    if profile.pull_request.is_some() {
        base.pull_request = profile.pull_request;
    }
    if profile.landing.is_some() {
        base.landing = profile.landing;
    }
}

fn merge_workspace_config(base: &mut WorkspaceConfig, profile: WorkspaceConfig) {
    extend_unique(&mut base.tabs, profile.tabs);
    extend_unique(&mut base.post_deps_tabs, profile.post_deps_tabs);
    base.colors.extend(profile.colors);
    if profile.open_url.is_some() {
        base.open_url = profile.open_url;
    }
    if profile.open_browser.is_some() {
        base.open_browser = profile.open_browser;
    }
    if profile.browser.is_some() {
        base.browser = profile.browser;
    }
}

fn merge_editor_config(base: &mut EditorConfig, profile: EditorConfig) {
    if profile.command.is_some() {
        base.command = profile.command;
    }
    if profile.placement.is_some() {
        base.placement = profile.placement;
    }
}

fn extend_unique(target: &mut Vec<String>, additions: Vec<String>) {
    for value in additions {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn extend_copy_as_unique(target: &mut Vec<CopyAsEntry>, additions: Vec<CopyAsEntry>) {
    for value in additions {
        if !target
            .iter()
            .any(|entry| entry.from == value.from && entry.to == value.to)
        {
            target.push(value);
        }
    }
}
