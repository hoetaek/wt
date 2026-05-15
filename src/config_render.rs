use crate::config::{
    AgentCli, AgentConfig, Config, EditorConfig, EditorPlacement, IssueProviderType, IssuesConfig,
    ReadyMode, SetupConfig, SiteConfig, SiteProvider, SubmitMode, TestConfig, WorkspaceConfig,
    WorktreeConfig,
};

pub fn render_effective_config(config: &Config) -> String {
    let mut s = String::new();

    append_worktree_section(&mut s, &config.worktree);
    append_setup_section(&mut s, &config.setup);
    if let Some(issues) = config.issues.as_ref() {
        append_issues_section(&mut s, issues);
    }
    if let Some(site) = config.site.as_ref() {
        append_site_section(&mut s, site);
    }
    append_editor_section(&mut s, &config.editor);
    if let Some(workspace) = config.workspace.as_ref() {
        append_workspace_section(&mut s, workspace);
    }
    if let Some(agent) = config.agent.as_ref() {
        append_agent_section(&mut s, agent);
    }
    if let Some(test) = config.test.as_ref() {
        append_test_section(&mut s, test);
    }

    if s.starts_with('\n') {
        s.remove(0);
    }
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn append_worktree_section(s: &mut String, worktree: &WorktreeConfig) {
    if worktree == &WorktreeConfig::default() {
        return;
    }

    s.push_str("\n[worktree]\n");
    if let Some(path) = worktree.path.as_deref() {
        s.push_str(&format!("path = {}\n", toml_quote(path)));
    }
    if !worktree.copy.is_empty() {
        s.push_str(&format!("copy = {}\n", toml_array(&worktree.copy)));
    }
    if !worktree.copy_as.is_empty() {
        s.push_str("copy_as = [\n");
        for entry in &worktree.copy_as {
            s.push_str(&format!(
                "    {{ from = {}, to = {} }},\n",
                toml_quote(&entry.from),
                toml_quote(&entry.to)
            ));
        }
        s.push_str("]\n");
    }
    if !worktree.link.is_empty() {
        s.push_str(&format!("link = {}\n", toml_array(&worktree.link)));
    }
    if let Some(context) = worktree.inject_local_context.as_deref() {
        s.push_str(&format!("inject_local_context = {}\n", toml_quote(context)));
    }
    if let Some(naming) = worktree.naming.as_ref() {
        s.push_str("\n[worktree.naming]\n");
        s.push_str(&format!("command = {}\n", toml_quote(&naming.command)));
        s.push_str(&format!("prompt = {}\n", toml_quote(&naming.prompt)));
        if let Some(branch) = naming.branch.as_deref() {
            s.push_str(&format!("branch = {}\n", toml_quote(branch)));
        }
        if let Some(workspace) = naming.workspace.as_deref() {
            s.push_str(&format!("workspace = {}\n", toml_quote(workspace)));
        }
    }
}

fn append_setup_section(s: &mut String, setup: &SetupConfig) {
    if setup == &SetupConfig::default() {
        return;
    }

    s.push_str("\n[setup]\n");
    if !setup.deps.is_empty() {
        s.push_str("deps = [\n");
        for dep in &setup.deps {
            s.push_str("    { ");
            if let Some(working_dir) = dep.working_dir.as_deref() {
                s.push_str(&format!("working_dir = {}, ", toml_quote(working_dir)));
            }
            s.push_str(&format!("run = {}", toml_quote(&dep.run)));
            if let Some(if_exists) = dep.if_exists.as_deref() {
                s.push_str(&format!(", if_exists = {}", toml_quote(if_exists)));
            }
            s.push_str(" },\n");
        }
        s.push_str("]\n");
    }

    if !setup.env.is_empty() {
        s.push_str("\n[setup.env]\n");
        let mut entries = setup.env.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in entries {
            s.push_str(&format!("{} = {}\n", toml_key(key), toml_quote(value)));
        }
    }

    let mut env_files = setup.env_files.iter().collect::<Vec<_>>();
    env_files.sort_by(|a, b| a.0.cmp(b.0));
    for (path, values) in env_files {
        s.push_str(&format!("\n[setup.env_files.{}]\n", toml_key(path)));
        let mut entries = values.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (key, value) in entries {
            s.push_str(&format!("{} = {}\n", toml_key(key), toml_quote(value)));
        }
    }
}

fn append_issues_section(s: &mut String, issues: &IssuesConfig) {
    s.push_str("\n[issues]\n");
    s.push_str(&format!(
        "provider = {}\n",
        toml_quote(issue_provider_name(&issues.provider))
    ));
    if let Some(gh_user) = issues.gh_user.as_deref() {
        s.push_str(&format!("gh_user = {}\n", toml_quote(gh_user)));
    }
}

fn append_site_section(s: &mut String, site: &SiteConfig) {
    s.push_str("\n[site]\n");
    s.push_str(&format!(
        "provider = {}\n",
        toml_quote(site_provider_name(&site.provider))
    ));
    if let Some(name) = site.name.as_deref() {
        s.push_str(&format!("name = {}\n", toml_quote(name)));
    }
    if let Some(root) = site.root.as_deref() {
        s.push_str(&format!("root = {}\n", toml_quote(root)));
    }
    if let Some(secure) = site.secure {
        s.push_str(&format!("secure = {secure}\n"));
    }
    if let Some(open_browser) = site.open_browser {
        s.push_str(&format!("open_browser = {open_browser}\n"));
    }
    if let Some(browser) = site.browser.as_deref() {
        s.push_str(&format!("browser = {}\n", toml_quote(browser)));
    }
    if let Some(url) = site.url.as_deref() {
        s.push_str(&format!("url = {}\n", toml_quote(url)));
    }
    if let Some(target) = site.target.as_deref() {
        s.push_str(&format!("target = {}\n", toml_quote(target)));
    }
}

fn append_editor_section(s: &mut String, editor: &EditorConfig) {
    if editor == &EditorConfig::default() {
        return;
    }

    s.push_str("\n[editor]\n");
    if let Some(command) = editor.command.as_deref() {
        s.push_str(&format!("command = {}\n", toml_quote(command)));
    }
    if let Some(placement) = editor.placement.as_ref() {
        s.push_str(&format!(
            "placement = {}\n",
            toml_quote(editor_placement_name(placement))
        ));
    }
}

fn append_workspace_section(s: &mut String, workspace: &WorkspaceConfig) {
    s.push_str("\n[workspace]\n");
    if !workspace.tabs.is_empty() {
        s.push_str(&format!("tabs = {}\n", toml_array(&workspace.tabs)));
    }
    if !workspace.post_deps_tabs.is_empty() {
        s.push_str(&format!(
            "post_deps_tabs = {}\n",
            toml_array(&workspace.post_deps_tabs)
        ));
    }
    if !workspace.colors.is_empty() {
        s.push_str(&format!(
            "colors = {}\n",
            toml_inline_string_map(&workspace.colors)
        ));
    }
    if let Some(open_url) = workspace.open_url.as_deref() {
        s.push_str(&format!("open_url = {}\n", toml_quote(open_url)));
    }
    if let Some(open_browser) = workspace.open_browser {
        s.push_str(&format!("open_browser = {open_browser}\n"));
    }
    if let Some(browser) = workspace.browser.as_deref() {
        s.push_str(&format!("browser = {}\n", toml_quote(browser)));
    }
}

fn append_agent_section(s: &mut String, agent: &AgentConfig) {
    s.push_str("\n[agent]\n");
    s.push_str(&format!(
        "cli = {}\n",
        toml_quote(agent_cli_name(&agent.cli))
    ));
    if !agent.args.is_empty() {
        s.push_str(&format!("args = {}\n", toml_array(&agent.args)));
    }
    if let Some(command) = agent.command.as_deref() {
        s.push_str(&format!("command = {}\n", toml_quote(command)));
    }
    s.push_str(&format!(
        "ready = {}\n",
        toml_quote(&ready_mode_value(&agent.ready))
    ));
    s.push_str(&format!(
        "submit = {}\n",
        toml_quote(submit_mode_value(&agent.submit))
    ));
    s.push_str(&format!("timeout = {}\n", agent.timeout));
    s.push_str(&format!("send_after = {}\n", agent.send_after));

    if !agent.prompt.is_empty() {
        s.push_str("\n[agent.prompt]\n");
        let mut entries = agent.prompt.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (mode, prompts) in entries {
            s.push_str(&format!("{} = {}\n", toml_key(mode), toml_array(prompts)));
        }
    }
}

fn append_test_section(s: &mut String, test: &TestConfig) {
    if test.commands.is_empty() {
        return;
    }

    s.push_str("\n[test]\ncommands = [\n");
    for command in &test.commands {
        s.push_str("    { ");
        if let Some(label) = command.label.as_deref() {
            s.push_str(&format!("label = {}, ", toml_quote(label)));
        }
        if let Some(working_dir) = command.working_dir.as_deref() {
            s.push_str(&format!("working_dir = {}, ", toml_quote(working_dir)));
        }
        s.push_str(&format!("run = {}", toml_quote(&command.run)));
        if let Some(if_exists) = command.if_exists.as_deref() {
            s.push_str(&format!(", if_exists = {}", toml_quote(if_exists)));
        }
        s.push_str(" },\n");
    }
    s.push_str("]\n");
}

fn agent_cli_name(cli: &AgentCli) -> &'static str {
    match cli {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::None => "none",
    }
}

fn issue_provider_name(provider: &IssueProviderType) -> &'static str {
    match provider {
        IssueProviderType::Linear => "linear",
        IssueProviderType::Github => "github",
    }
}

fn site_provider_name(provider: &SiteProvider) -> &'static str {
    match provider {
        SiteProvider::None => "none",
        SiteProvider::Herd => "herd",
        SiteProvider::Valet => "valet",
        SiteProvider::DockerProxy => "docker_proxy",
        SiteProvider::Traefik => "traefik",
    }
}

fn editor_placement_name(placement: &EditorPlacement) -> &'static str {
    match placement {
        EditorPlacement::CmuxSurface => "cmux_surface",
        EditorPlacement::Process => "process",
    }
}

fn ready_mode_value(ready: &ReadyMode) -> String {
    match ready {
        ReadyMode::Auto => "auto".into(),
        ReadyMode::Marker(marker) => marker.clone(),
    }
}

fn submit_mode_value(submit: &SubmitMode) -> &'static str {
    match submit {
        SubmitMode::Auto => "auto",
        SubmitMode::Newline => "newline",
        SubmitMode::CarriageReturn => "carriage_return",
        SubmitMode::None => "none",
    }
}

fn toml_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| toml_quote(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn toml_inline_string_map(values: &std::collections::HashMap<String, String>) -> String {
    let mut entries = values.iter().collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let rendered = entries
        .into_iter()
        .map(|(key, value)| format!("{} = {}", toml_key(key), toml_quote(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {rendered} }}")
}

fn toml_key(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        value.into()
    } else {
        toml_quote(value)
    }
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
