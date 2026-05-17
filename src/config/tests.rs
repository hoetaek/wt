use super::merge::merge_config;
use super::*;
use std::collections::HashMap;

#[test]
fn parses_full_config() {
    let toml_str = r#"
[worktree]
path = "$HOME/worktrees/{{default_name}}"
copy = [".env", "CLAUDE.local.md", ".claude/settings.local.json"]
link = [".local"]
inject_local_context = "\n## env\n- parent: `{{parent_branch}}`\n"

[setup]
deps = [
{ run = "composer install" },
{ working_dir = "frontend", run = "npm install" },
{ working_dir = "enterprise", run = "pnpm install", if_exists = "package.json" },
]

[setup.env]
APP_URL = "https://{{site_name}}.test"
APP_NAME = "{{issue_title}}"

[setup.env_files."frontend/.env.development"]
VITE_API_TARGET = "{{api_url}}"

[setup.env_files."backend/.env"]
DJANGO_ENV = "dev"

[workflow]
pull_request = "draft"
landing = "auto"

[profile]
name = "codex"

[site]
provider = "valet"
name = "{{repo}}-{{branch_slug}}"
root = "public"
secure = true
open_browser = true
browser = "Safari"
url = "https://{{site_name}}.test"
target = "http://127.0.0.1:{{vite_port}}"

[editor]
command = "vi {{path}}"
placement = "cmux_surface"

[workspace]
tabs = ["lazygit"]
post_deps_tabs = ["npm run dev"]
colors = { issue = "Red", pr = "Green" }
open_url = "{{site_url}}"
open_browser = true
browser = "Google Chrome"

[agent]
cli = "claude"

[test]
commands = [
{ working_dir = "backend", run = "./vendor/bin/pest", if_exists = "vendor/bin/pest", label = "PHP" },
]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();

    assert_eq!(
        config.worktree.copy,
        vec![".env", "CLAUDE.local.md", ".claude/settings.local.json"]
    );
    assert_eq!(
        config.worktree.path.as_deref(),
        Some("$HOME/worktrees/{{default_name}}")
    );
    assert_eq!(config.worktree.link, vec![".local"]);
    assert!(config.worktree.inject_local_context.is_some());
    assert!(
        config
            .worktree
            .inject_local_context
            .as_deref()
            .unwrap()
            .contains("{{parent_branch}}")
    );
    assert_eq!(config.setup.deps.len(), 3);
    assert_eq!(config.setup.deps[0].run, "composer install");
    assert!(config.setup.deps[0].working_dir.is_none());
    assert_eq!(
        config.setup.deps[1].working_dir.as_deref(),
        Some("frontend")
    );
    assert_eq!(
        config.setup.deps[2].working_dir.as_deref(),
        Some("enterprise")
    );
    assert_eq!(
        config.setup.deps[2].if_exists.as_deref(),
        Some("package.json")
    );
    assert_eq!(
        config.setup.env.get("APP_URL").unwrap(),
        "https://{{site_name}}.test"
    );
    assert_eq!(
        config
            .setup
            .env_files
            .get("frontend/.env.development")
            .unwrap()
            .get("VITE_API_TARGET")
            .unwrap(),
        "{{api_url}}"
    );
    assert_eq!(
        config
            .setup
            .env_files
            .get("backend/.env")
            .unwrap()
            .get("DJANGO_ENV")
            .unwrap(),
        "dev"
    );
    let workflow_policy = config.workflow_default_policy();
    assert_eq!(
        workflow_policy.pull_request,
        WorkflowDefaultPullRequestMode::Draft
    );
    assert_eq!(workflow_policy.landing, WorkflowDefaultLandingPolicy::Auto);
    assert_eq!(config.profile.unwrap().name.as_deref(), Some("codex"));

    let site = config.site.unwrap();
    assert_eq!(site.provider, SiteProvider::Valet);
    assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
    assert_eq!(site.root.as_deref(), Some("public"));
    assert_eq!(site.secure, Some(true));
    assert_eq!(site.open_browser, Some(true));
    assert_eq!(site.browser.as_deref(), Some("Safari"));
    assert_eq!(site.url.as_deref(), Some("https://{{site_name}}.test"));
    assert_eq!(
        site.target.as_deref(),
        Some("http://127.0.0.1:{{vite_port}}")
    );
    assert_eq!(config.editor.command.as_deref(), Some("vi {{path}}"));
    assert_eq!(
        config.editor.placement.as_ref(),
        Some(&EditorPlacement::CmuxSurface)
    );

    let ws = config.workspace.unwrap();
    assert_eq!(ws.tabs, vec!["lazygit"]);
    assert_eq!(ws.post_deps_tabs, vec!["npm run dev"]);
    assert_eq!(ws.colors.get("issue").unwrap(), "Red");
    assert_eq!(ws.open_url.as_deref(), Some("{{site_url}}"));
    assert_eq!(ws.open_browser, Some(true));
    assert_eq!(ws.browser.as_deref(), Some("Google Chrome"));

    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Claude);

    let test = config.test.unwrap();
    assert_eq!(test.commands[0].label.as_deref(), Some("PHP"));
    assert_eq!(test.commands[0].working_dir.as_deref(), Some("backend"));
}

#[test]
fn rejects_unknown_setup_command_field() {
    let err = toml::from_str::<Config>(
        r#"
[setup]
deps = [
{ cwd = "api", run = "uv sync" },
]
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("unknown field `cwd`"));
}

#[test]
fn rejects_unknown_test_command_field() {
    let err = toml::from_str::<Config>(
        r#"
[test]
commands = [
{ cwd = "web", run = "npm test" },
]
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("unknown field `cwd`"));
}

#[test]
fn missing_file_returns_default() {
    let dir = std::env::temp_dir().join("wt-test-no-config");
    std::fs::create_dir_all(&dir).ok();
    let config = Config::load(&dir).unwrap();
    assert_eq!(config, Config::default());
}

#[test]
fn parses_explicit_claude_paths_in_copy() {
    let toml_str = r#"
[worktree]
copy = [".env", ".claude/settings.local.json", ".claude/hooks"]
link = [".local"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        config.worktree.copy,
        vec![".env", ".claude/settings.local.json", ".claude/hooks"]
    );
    assert_eq!(config.worktree.link, vec![".local"]);
}

#[test]
fn partial_config_fills_defaults() {
    let toml_str = r#"
[worktree]
copy = [".env"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.worktree.copy, vec![".env"]);
    assert!(config.worktree.link.is_empty());
    assert!(config.site.is_none());
    assert!(config.workspace.is_none());
}

#[test]
fn rejects_removed_claude_copy_field() {
    let toml_str = r#"
[worktree]
copy = [".env"]
claude_copy = ["settings.local.json"]
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("claude_copy"));
}

#[test]
fn local_config_overrides_root_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).ok();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[issues]
provider = "github"

[workflow]
pull_request = "draft"
landing = "manual"

[site]
provider = "herd"
name = "root"

[worktree]
copy = [".env"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        r#"
[profile.agent]
cli = "codex"

[workflow]
pull_request = "none"
landing = "auto"

[site]
provider = "traefik"
name = "{{repo}}-{{branch_slug}}.l"

[worktree]
copy = ["CLAUDE.local.md"]
"#,
    )
    .unwrap();

    let (config, source) = Config::load_with_source(dir.path()).unwrap();
    assert!(matches!(source, ConfigSource::Files(paths) if paths.len() == 2));
    assert_eq!(config.agent.as_ref().unwrap().cli, AgentCli::Codex);
    assert_eq!(
        config.issues.as_ref().unwrap().provider,
        IssueProviderType::Github
    );
    let site = config.site.as_ref().unwrap();
    assert_eq!(site.provider, SiteProvider::Traefik);
    assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}.l"));
    assert_eq!(config.worktree.copy, vec![".env", "CLAUDE.local.md"]);
    let workflow_policy = config.workflow_default_policy();
    assert_eq!(
        workflow_policy.pull_request,
        WorkflowDefaultPullRequestMode::None
    );
    assert_eq!(workflow_policy.landing, WorkflowDefaultLandingPolicy::Auto);
}

#[test]
fn workflow_policy_merges_per_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[workflow]
pull_request = "ready"
landing = "manual"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        r#"
[workflow]
landing = "auto"
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let policy = config.workflow_default_policy();
    assert_eq!(policy.pull_request, WorkflowDefaultPullRequestMode::Ready);
    assert_eq!(policy.landing, WorkflowDefaultLandingPolicy::Auto);
}

#[test]
fn workflow_policy_profile_overlay_merges_per_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local/profiles/codex")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[workflow]
pull_request = "draft"
landing = "manual"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".local/profiles/codex/profile.toml"),
        r#"
[workflow]
landing = "auto"
"#,
    )
    .unwrap();

    let base = Config::load_file(&dir.path().join(".wt.toml")).unwrap();
    let config = Config::load_profile(dir.path(), "codex", &base)
        .unwrap()
        .unwrap();
    let policy = config.workflow_default_policy();
    assert_eq!(policy.pull_request, WorkflowDefaultPullRequestMode::Draft);
    assert_eq!(policy.landing, WorkflowDefaultLandingPolicy::Auto);
}

#[test]
fn workflow_policy_rejects_pull_request_aliases() {
    let err = toml::from_str::<Config>(&format!("[workflow]\npull_request = {:?}\n", "open"))
        .unwrap_err();

    assert!(err.to_string().contains("open"));
    assert!(err.to_string().contains("[workflow].pull_request"));
}

#[test]
fn workflow_policy_rejects_nested_defaults_table() {
    let err = toml::from_str::<Config>(&format!(
        "[workflow.{}]\npull_request = \"draft\"\n",
        "defaults"
    ))
    .unwrap_err();

    assert!(err.to_string().contains("[workflow"));
    assert!(err.to_string().contains("pull_request"));
}

#[test]
fn workflow_policy_rejects_legacy_landing_approval_key() {
    let key = format!("landing_requires_{}", "approval");
    let err = toml::from_str::<Config>(&format!("[workflow]\n{key} = false\n")).unwrap_err();

    assert!(err.to_string().contains(&key));
    assert!(err.to_string().contains("[workflow].landing"));
}

#[test]
fn workflow_policy_rejects_legacy_review_landing_value() {
    let value = format!("after_{}", "review");
    let err =
        toml::from_str::<Config>(&format!("[workflow]\nlanding = {:?}\n", value)).unwrap_err();

    assert!(err.to_string().contains(&value));
    assert!(err.to_string().contains("[workflow].landing"));
}

#[test]
fn workflow_policy_rejects_boolean_pull_request_values() {
    let err = toml::from_str::<Config>("[workflow]\npull_request = true\n").unwrap_err();

    assert!(err.to_string().contains("booleans are not aliases"));
}

#[test]
fn local_editor_config_overrides_command_and_preserves_placement() {
    let base: Config = toml::from_str(
        r#"
[editor]
command = "vi {{path}}"
placement = "cmux_surface"
"#,
    )
    .unwrap();
    let profile: Config = toml::from_str(
        r#"
[editor]
command = "code {{path}}"
"#,
    )
    .unwrap();

    let merged = merge_config(&base, profile);

    assert_eq!(merged.editor.command.as_deref(), Some("code {{path}}"));
    assert_eq!(
        merged.editor.placement.as_ref(),
        Some(&EditorPlacement::CmuxSurface)
    );
}

#[test]
fn prompt_append_layers_extend_effective_prompt_without_redeclaring_agent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]

[agent.prompt]
issue = ["shared prompt\n"]

[agent.prompt.append]
issue = ["shared append\n"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        r#"
[agent.prompt.append]
issue = ["local append\n"]
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Codex);
    assert_eq!(agent.args, vec!["--model", "gpt-5.5"]);
    assert_eq!(
        agent.prompt.get("issue").unwrap(),
        &vec!["shared prompt\n\nshared append\n\nlocal append\n".to_string()]
    );
}

#[test]
fn prompt_overwrite_layer_replaces_then_append_extends() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"

[agent.prompt]
issue = ["shared prompt\n"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        r#"
[agent.prompt]
issue = ["local prompt\n"]

[agent.prompt.append]
issue = ["local append\n"]
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Codex);
    assert_eq!(
        agent.prompt.get("issue").unwrap(),
        &vec!["local prompt\n\nlocal append\n".to_string()]
    );
}

#[test]
fn common_prompt_scope_expands_after_layers_before_mode_prompt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"

[agent.prompt]
common = ["shared common\n"]
issue = ["shared issue\n"]

[agent.prompt.append]
common = ["shared common append\n"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        r#"
[agent.prompt.append]
common = ["local common append\n"]
issue = ["local issue append\n"]
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let agent = config.agent.unwrap();
    assert!(!agent.prompt.contains_key("common"));
    assert_eq!(
        agent.prompt.get("issue").unwrap(),
        &vec![
            "shared common\n\nshared common append\n\nlocal common append\n".to_string(),
            "shared issue\n\nlocal issue append\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("new").unwrap(),
        &vec!["shared common\n\nshared common append\n\nlocal common append\n".to_string()]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["shared common\n\nshared common append\n\nlocal common append\n".to_string()]
    );
}

#[test]
fn profile_convention_common_prompt_files_expand_after_mode_files() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".local/profiles/codex");
    std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"

[agent.prompt]
common = ["root common\n"]
issue = ["root issue\n"]
"#,
    )
    .unwrap();
    std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
    std::fs::write(profile_dir.join("prompts/common.md"), "file common\n").unwrap();
    std::fs::write(
        profile_dir.join("prompts/common.append.md"),
        "file common append\n",
    )
    .unwrap();
    std::fs::write(profile_dir.join("prompts/issue.md"), "file issue\n").unwrap();
    std::fs::write(
        profile_dir.join("prompts/issue.append.md"),
        "file issue append\n",
    )
    .unwrap();

    let (base, _, _) = Config::load_base_and_effective_with_source(dir.path()).unwrap();
    let config = Config::load_profile(dir.path(), "codex", &base)
        .unwrap()
        .unwrap();
    let agent = config.agent.unwrap();
    assert!(!agent.prompt.contains_key("common"));
    assert_eq!(
        agent.prompt.get("issue").unwrap(),
        &vec![
            "file common\n\nfile common append\n".to_string(),
            "file issue\n\nfile issue append\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("new").unwrap(),
        &vec!["file common\n\nfile common append\n".to_string()]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["file common\n\nfile common append\n".to_string()]
    );
}

#[test]
fn falls_back_to_root_config() {
    let dir = std::env::temp_dir().join("wt-test-root-fallback");
    std::fs::create_dir_all(&dir).ok();

    std::fs::write(
        dir.join(".wt.toml"),
        r#"
[site]
provider = "herd"
name = "root"
"#,
    )
    .unwrap();

    let config = Config::load(&dir).unwrap();
    let site = config.site.unwrap();
    assert_eq!(site.provider, SiteProvider::Herd);
    assert_eq!(site.name.as_deref(), Some("root"));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn load_profiles_discovers_profile_toml_files() {
    let dir = tempfile::tempdir().unwrap();
    let profiles_dir = dir.path().join(".local/profiles");
    let baseline_dir = profiles_dir.join("baseline");
    let tdd_dir = profiles_dir.join("tdd");
    std::fs::create_dir_all(&baseline_dir).unwrap();
    std::fs::create_dir_all(&tdd_dir).unwrap();

    std::fs::write(
        baseline_dir.join("profile.toml"),
        "[worktree]\ncopy = [\".env\"]\n",
    )
    .unwrap();
    std::fs::write(
        tdd_dir.join("profile.toml"),
        "[worktree]\ncopy = [\".env\", \"CLAUDE.local.md\"]\n",
    )
    .unwrap();

    let profiles = Config::load_profiles(dir.path(), &Config::default()).unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].0, "baseline");
    assert_eq!(profiles[1].0, "tdd");
    assert_eq!(profiles[0].1.worktree.copy, vec![".env".to_string()]);
    assert_eq!(
        profiles[1].1.worktree.copy,
        vec![".env".to_string(), "CLAUDE.local.md".to_string()]
    );
}

#[test]
fn load_profiles_returns_empty_when_no_profiles_dir() {
    let dir = tempfile::tempdir().unwrap();
    let profiles = Config::load_profiles(dir.path(), &Config::default()).unwrap();
    assert!(profiles.is_empty());
}

#[test]
fn load_profiles_returns_empty_when_no_profile_toml_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local/profiles/empty")).unwrap();
    let profiles = Config::load_profiles(dir.path(), &Config::default()).unwrap();
    assert!(profiles.is_empty());
}

#[test]
fn load_with_source_applies_inline_profile_to_effective_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).unwrap();
    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        r#"
[profile.agent]
cli = "codex"
args = ["--yolo"]
"#,
    )
    .unwrap();

    let (base, effective, source) =
        Config::load_base_and_effective_with_source(dir.path()).expect("config should load");

    assert!(matches!(source, ConfigSource::File(_)));
    assert!(base.profile.is_some());
    let agent = effective.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Codex);
    assert_eq!(agent.args, vec!["--yolo"]);
    assert!(effective.profile.is_none());
}

#[test]
fn load_with_source_resolves_named_profile_without_polluting_base_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local/profiles/codex")).unwrap();
    std::fs::write(
        dir.path().join(".wt.toml"),
        "[issues]\nprovider = \"github\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        "[profile]\nname = \"codex\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".local/profiles/codex/profile.toml"),
        "[agent]\ncli = \"codex\"\n",
    )
    .unwrap();

    let (base, effective, _) =
        Config::load_base_and_effective_with_source(dir.path()).expect("config should load");

    assert_eq!(base.profile.unwrap().name.as_deref(), Some("codex"));
    assert!(base.agent.is_none());
    assert_eq!(effective.agent.unwrap().cli, AgentCli::Codex);
    assert!(effective.profile.is_none());
}

#[test]
fn rejects_reserved_default_profile_name() {
    let err = toml::from_str::<Config>("[profile]\nname = \"default\"\n").unwrap_err();
    assert!(err.to_string().contains("reserved"));
}

#[test]
fn rejects_profile_name_combined_with_inline_profile_settings() {
    let err = toml::from_str::<Config>(
        r#"
[profile]
name = "codex"

[profile.agent]
cli = "codex"
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("cannot be combined"));
}

#[test]
fn parses_agent_config_with_defaults() {
    let toml_str = r#"
[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]

[agent.prompt]
issue = ["start\n"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Codex);
    assert_eq!(agent.args, vec!["--model", "gpt-5.5"]);
    assert_eq!(agent.command, None);
    assert_eq!(agent.ready, ReadyMode::Auto);
    assert_eq!(agent.submit, SubmitMode::Auto);
    assert_eq!(agent.timeout, 15);
    assert_eq!(agent.send_after, 3);
    assert_eq!(agent.prompt.get("issue").unwrap(), &vec!["start\n"]);
}

#[test]
fn rejects_append_as_agent_prompt_mode_name() {
    let toml_str = r#"
[agent]
cli = "codex"

[agent.prompt]
append = ["ambiguous\n"]
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("agent.prompt.append"));
}

#[test]
fn rejects_partial_agent_without_prompt_patch() {
    let toml_str = r#"
[agent]
args = ["--yolo"]
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("agent.cli is required"));
}

#[test]
fn rejects_unknown_agent_fields() {
    let toml_str = r#"
[agent]
driver = "codex"
cli = "codex"
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("driver"));
}

#[test]
fn rejects_unknown_agent_cli() {
    let toml_str = r#"
[agent]
cli = "other"
command = "env CODEX_HOME=.codex codex"
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("other"));
}

#[test]
fn parses_agent_ready_marker_submit_and_gemini_cli() {
    let toml_str = r#"
[agent]
cli = "gemini"
ready = "READY_MARKER"
submit = "carriage_return"
timeout = 22
send_after = 4
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Gemini);
    assert_eq!(agent.ready, ReadyMode::Marker("READY_MARKER".into()));
    assert_eq!(agent.submit, SubmitMode::CarriageReturn);
    assert_eq!(agent.timeout, 22);
    assert_eq!(agent.send_after, 4);
}

#[test]
fn agent_command_line_escapes_args_and_respects_override() {
    let agent = AgentConfig {
        cli: AgentCli::Codex,
        args: vec!["--prompt".into(), "hello world".into(), "it's ok".into()],
        command: None,
        ready: ReadyMode::Auto,
        submit: SubmitMode::Auto,
        timeout: 15,
        send_after: 3,
        prompt: HashMap::new(),
    };
    assert_eq!(
        agent.command_line().unwrap(),
        Some("codex --prompt 'hello world' 'it'\\''s ok'".into())
    );

    let override_agent = AgentConfig {
        command: Some("env FOO=1 codex --model gpt-5.5".into()),
        ..agent
    };
    assert_eq!(
        override_agent.command_line().unwrap(),
        Some("env FOO=1 codex --model gpt-5.5".into())
    );

    let templated_agent = AgentConfig {
        command: Some("env CHROME_DIR={{repo_root}}/profiles/{{branch_slug}} codex".into()),
        args: vec![
            "--cd".into(),
            "{{worktree_path}}".into(),
            "{{repo_root}}/profiles/{{branch_slug}}".into(),
        ],
        ..override_agent
    };
    let vars = HashMap::from([
        ("repo_root".into(), "/tmp/repo".into()),
        ("worktree_path".into(), "/tmp/repo-feature".into()),
        ("branch_slug".into(), "feature".into()),
    ]);
    assert_eq!(
        templated_agent.command_line_with_vars(Some(&vars)).unwrap(),
        Some("env CHROME_DIR=/tmp/repo/profiles/feature codex".into())
    );

    let templated_args_agent = AgentConfig {
        cli: AgentCli::Codex,
        args: vec![
            "--cd".into(),
            "{{worktree_path}}".into(),
            "{{repo_root}}/profiles/{{branch_slug}}".into(),
        ],
        command: None,
        ready: ReadyMode::Auto,
        submit: SubmitMode::Auto,
        timeout: 15,
        send_after: 3,
        prompt: HashMap::new(),
    };
    assert_eq!(
        templated_args_agent
            .command_line_with_vars(Some(&vars))
            .unwrap(),
        Some("codex --cd /tmp/repo-feature /tmp/repo/profiles/feature".into())
    );
}

#[test]
fn agent_helpers_pick_ready_and_submit_by_cli() {
    let codex = AgentConfig {
        cli: AgentCli::Codex,
        args: Vec::new(),
        command: None,
        ready: ReadyMode::Auto,
        submit: SubmitMode::Auto,
        timeout: 15,
        send_after: 3,
        prompt: HashMap::new(),
    };
    let claude = AgentConfig {
        cli: AgentCli::Claude,
        ..codex.clone()
    };
    let gemini = AgentConfig {
        cli: AgentCli::Gemini,
        ..codex.clone()
    };
    let none = AgentConfig {
        cli: AgentCli::None,
        ..codex.clone()
    };

    assert_eq!(codex.effective_ready(), Some("›".into()));
    assert_eq!(claude.effective_ready(), Some("❯".into()));
    assert_eq!(gemini.effective_ready(), None);
    assert_eq!(none.command_line().unwrap(), None);
    assert_eq!(codex.apply_submit_suffix("go".into()), "go\r");
    assert_eq!(claude.apply_submit_suffix("go".into()), "go\n");
    assert_eq!(gemini.apply_submit_suffix("go".into()), "go\n");
    assert_eq!(codex.apply_submit_suffix("go\n".into()), "go\n");
}

#[test]
fn agent_none_disables_command_even_with_override() {
    let agent = AgentConfig {
        cli: AgentCli::None,
        args: Vec::new(),
        command: Some("codex --model gpt-5.5".into()),
        ready: ReadyMode::Auto,
        submit: SubmitMode::Auto,
        timeout: 15,
        send_after: 3,
        prompt: HashMap::new(),
    };

    assert_eq!(agent.command_line().unwrap(), None);
}

#[test]
fn load_profile_returns_specific_config_or_none() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".local/profiles/codex");
    std::fs::create_dir_all(&profile_dir).unwrap();
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
"#,
    )
    .unwrap();

    let profile = Config::load_profile(dir.path(), "codex", &Config::default())
        .unwrap()
        .unwrap();
    assert_eq!(profile.agent.unwrap().cli, AgentCli::Codex);
    assert!(
        Config::load_profile(dir.path(), "missing", &Config::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn load_profile_overlays_base_config_and_applies_conventions() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".local/profiles/codex-yolo");
    std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();
    std::fs::create_dir_all(profile_dir.join("scaffold/.codex/skills")).unwrap();
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[agent]
cli = "codex"
args = ["--yolo"]
"#,
    )
    .unwrap();
    std::fs::write(profile_dir.join("prompts/issue.md"), "handle issue\n").unwrap();
    std::fs::write(
        profile_dir.join("scaffold/AGENTS.override.md"),
        "codex override\n",
    )
    .unwrap();
    std::fs::write(
        profile_dir.join("scaffold/.codex/skills/README.md"),
        "skills\n",
    )
    .unwrap();

    let mut base = Config::default();
    base.worktree.copy = vec![".env".into()];
    base.worktree.link = vec![".local".into()];
    base.worktree.path = Some("worktrees/{{default_name}}".into());

    let profile = Config::load_profile(dir.path(), "codex-yolo", &base)
        .unwrap()
        .unwrap();

    assert_eq!(profile.worktree.copy, vec![".env"]);
    assert_eq!(profile.worktree.link, vec![".local"]);
    assert_eq!(
        profile.worktree.path.as_deref(),
        Some("worktrees/{{default_name}}")
    );
    let agent = profile.agent.unwrap();
    assert_eq!(agent.args, vec!["--yolo"]);
    assert_eq!(agent.prompt.get("issue").unwrap(), &vec!["handle issue\n"]);
    assert!(
        profile.worktree.copy_as.iter().any(|entry| {
            entry.from == ".local/profiles/codex-yolo/scaffold" && entry.to == "."
        })
    );
}

#[test]
fn load_profile_merges_worktree_fields_without_dropping_base_lists() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".local/profiles/alternate-path");
    std::fs::create_dir_all(&profile_dir).unwrap();
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[worktree]
path = "profiles/{{default_name}}"
"#,
    )
    .unwrap();

    let mut base = Config::default();
    base.worktree.copy = vec![".env".into()];
    base.worktree.link = vec![".local".into()];

    let profile = Config::load_profile(dir.path(), "alternate-path", &base)
        .unwrap()
        .unwrap();

    assert_eq!(
        profile.worktree.path.as_deref(),
        Some("profiles/{{default_name}}")
    );
    assert_eq!(profile.worktree.copy, vec![".env"]);
    assert_eq!(profile.worktree.link, vec![".local"]);
}

#[test]
fn load_profile_applies_profile_scaffold_root() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".local/profiles/claude-plan");
    std::fs::create_dir_all(profile_dir.join("scaffold/.claude/commands")).unwrap();
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[agent]
cli = "claude"
"#,
    )
    .unwrap();
    std::fs::write(profile_dir.join("scaffold/CLAUDE.local.md"), "claude\n").unwrap();
    std::fs::write(
        profile_dir.join("scaffold/.claude/commands/start.md"),
        "start\n",
    )
    .unwrap();

    let profile = Config::load_profile(dir.path(), "claude-plan", &Config::default())
        .unwrap()
        .unwrap();

    assert!(
        profile.worktree.copy_as.iter().any(|entry| {
            entry.from == ".local/profiles/claude-plan/scaffold" && entry.to == "."
        })
    );
}

#[test]
fn rejects_removed_workspace_command_and_post_ready() {
    let command_toml = r#"
[workspace]
command = "bash"
tabs = []
"#;
    let err = toml::from_str::<Config>(command_toml).unwrap_err();
    assert!(err.to_string().contains("command"));

    let post_ready_toml = r#"
[workspace]
tabs = []

[workspace.post_ready]
wait_for = "❯"
timeout = 10
send_after = 2

[workspace.post_ready.send]
issue = ["start 스킬을 사용해서 현재 이슈/작업 컨텍스트를 확인하고 작업 계획을 세운 뒤 바로 시작해줘.\n"]
pr = ["/conventional-review {{pr_number}}\n", "/codex:review --background\n"]
"#;
    let err = toml::from_str::<Config>(post_ready_toml).unwrap_err();
    assert!(err.to_string().contains("post_ready"));
}

#[test]
fn rejects_removed_claude_local_context_field() {
    let toml_str = r#"
[worktree]
claude_local_context = "old"
"#;

    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("claude_local_context"));
}

#[test]
fn parses_site_open_browser_config() {
    let toml_str = r#"
[site]
provider = "herd"
name = "test"
open_browser = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let site = config.site.unwrap();
    assert_eq!(site.open_browser, Some(true));
}

#[test]
fn rejects_herd_config_section() {
    let toml_str = r#"
[herd]
site_name = "{{repo}}-{{branch_slug}}"
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("herd"));
}

#[test]
fn parses_traefik_site_provider() {
    let toml_str = r#"
[site]
provider = "traefik"
name = "istat-{{branch_slug}}.l"
url = "https://{{site_name}}"
target = "http://127.0.0.1:{{front_port}}"
secure = true
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let site = config.site.unwrap();
    assert_eq!(site.provider, SiteProvider::Traefik);
    assert_eq!(site.name.as_deref(), Some("istat-{{branch_slug}}.l"));
    assert_eq!(site.url.as_deref(), Some("https://{{site_name}}"));
    assert_eq!(
        site.target.as_deref(),
        Some("http://127.0.0.1:{{front_port}}")
    );
    assert_eq!(site.secure, Some(true));
}

#[test]
fn effective_site_uses_site_provider_herd() {
    let toml_str = r#"
[site]
provider = "herd"
name = "{{repo}}-{{branch_slug}}"
secure = true
open_browser = true
browser = "Google Chrome"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let site = config.effective_site().unwrap();
    assert_eq!(site.provider, SiteProvider::Herd);
    assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
    assert_eq!(site.secure, Some(true));
    assert_eq!(site.open_browser, Some(true));
    assert_eq!(site.browser.as_deref(), Some("Google Chrome"));
}

#[test]
fn site_provider_none_disables_effective_site() {
    let toml_str = r#"
[site]
provider = "none"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.effective_site().is_none());
    assert!(!config.has_site());
}

#[test]
fn parses_issues_config_github() {
    let toml_str = r#"
[issues]
provider = "github"
gh_user = "alice"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let issues = config.issues.unwrap();
    assert_eq!(issues.provider, IssueProviderType::Github);
    assert_eq!(issues.gh_user.as_deref(), Some("alice"));
}

#[test]
fn parses_issues_config_linear() {
    let toml_str = r#"
[issues]
provider = "linear"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let issues = config.issues.unwrap();
    assert_eq!(issues.provider, IssueProviderType::Linear);
    assert!(issues.gh_user.is_none());
}

#[test]
fn parses_named_profile_selector_config() {
    let toml_str = r#"
[profile]
name = "codex"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.profile.unwrap().name.as_deref(), Some("codex"));
}

#[test]
fn parses_worktree_naming_config_with_defaults() {
    let toml_str = r#"
[worktree.naming]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let naming = config.worktree.naming.unwrap();
    assert_eq!(naming.command, "claude -p");
    assert!(naming.prompt.contains("{{issue_title}}"));
    assert_eq!(
        naming.branch.as_deref(),
        Some("{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}")
    );
    assert!(naming.workspace.is_none());
}

#[test]
fn parses_worktree_naming_config_overrides() {
    let toml_str = r#"
[worktree.naming]
command = "claude -p --model sonnet"
prompt = "title={{issue_title}}"
branch = "feat/{{issue_number}}-{{english_slug}}"
workspace = "{{english_title}}"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let naming = config.worktree.naming.unwrap();
    assert_eq!(naming.command, "claude -p --model sonnet");
    assert_eq!(naming.prompt, "title={{issue_title}}");
    assert_eq!(
        naming.branch.as_deref(),
        Some("feat/{{issue_number}}-{{english_slug}}")
    );
    assert_eq!(naming.workspace.as_deref(), Some("{{english_title}}"));
}

#[test]
fn issues_section_optional() {
    let toml_str = r#"
[worktree]
copy = [".env"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.issues.is_none());
}
