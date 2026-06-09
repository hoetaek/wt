use super::merge::merge_config;
use super::*;
use std::collections::HashMap;

fn pathspec_pairs(specs: &[PathSpec]) -> Vec<(&str, &str)> {
    specs.iter().map(|spec| (spec.from(), spec.to())).collect()
}

#[test]
fn parses_full_config() {
    let toml_str = r#"
[worktree]
path = "$HOME/worktrees/{{default_name}}"
copy = [".env", "CLAUDE.local.md", ".claude/settings.local.json"]
link = ["tmp/shared-cache"]
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

[review]
codex_base = "required"

[profile]
name = "codex"

[site]
provider = "valet"
name = "{{repo}}-{{branch_slug}}"
root = "public"
secure = true
url = "https://{{site_name}}.test"
target = "http://127.0.0.1:{{vite_port}}"

[editor]
command = "vi {{path}}"
placement = "cmux_surface"

[workspace]
tabs = ["lazygit"]
post_deps_tabs = ["npm run dev"]
colors = { task = "Blue", issue = "Red", pr = "Green" }

[workspace.browser]
mode = "chrome_devtools"
url = "{{site_url}}"

[workspace.browser.chrome_devtools]
port = 9222
user_data_dir = "{{worktree_parent}}/.chrome-devtools/{{worktree_name}}"

[agent]
cli = "claude"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();

    assert_eq!(
        pathspec_pairs(&config.worktree.copy),
        vec![
            (".env", ".env"),
            ("CLAUDE.local.md", "CLAUDE.local.md"),
            (".claude/settings.local.json", ".claude/settings.local.json")
        ]
    );
    assert_eq!(
        config.worktree.path.as_deref(),
        Some("$HOME/worktrees/{{default_name}}")
    );
    assert_eq!(
        pathspec_pairs(&config.worktree.link),
        vec![("tmp/shared-cache", "tmp/shared-cache")]
    );
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
    assert_eq!(
        workflow_policy.review.codex_base,
        ReviewCodexBasePolicy::Required
    );
    assert_eq!(config.profile.unwrap().name.as_deref(), Some("codex"));

    let site = config.site.unwrap();
    assert_eq!(site.provider, SiteProvider::Valet);
    assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
    assert_eq!(site.root.as_deref(), Some("public"));
    assert_eq!(site.secure, Some(true));
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
    assert_eq!(ws.colors.get("task").unwrap(), "Blue");
    assert_eq!(ws.colors.get("issue").unwrap(), "Red");
    let browser = ws.browser.unwrap();
    assert_eq!(browser.mode, WorkspaceBrowserMode::ChromeDevtools);
    assert_eq!(browser.url.as_deref(), Some("{{site_url}}"));
    assert_eq!(browser.app, None);
    let chrome_devtools = browser.chrome_devtools.unwrap();
    assert_eq!(chrome_devtools.port, Some(9222));
    assert_eq!(
        chrome_devtools.user_data_dir.as_deref(),
        Some("{{worktree_parent}}/.chrome-devtools/{{worktree_name}}")
    );

    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Claude);
}

#[test]
fn language_defaults_to_auto_when_absent() {
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.language, Language::Auto);
}

#[test]
fn language_parses_explicit_values() {
    assert_eq!(
        toml::from_str::<Config>("language = \"ko\"")
            .unwrap()
            .language,
        Language::Ko
    );
    assert_eq!(
        toml::from_str::<Config>("language = \"en\"")
            .unwrap()
            .language,
        Language::En
    );
    assert_eq!(
        toml::from_str::<Config>("language = \"auto\"")
            .unwrap()
            .language,
        Language::Auto
    );
}

#[test]
fn language_rejects_unsupported_value() {
    let err = toml::from_str::<Config>("language = \"fr\"").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("language") || msg.contains("fr"), "got: {msg}");
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
fn rejects_legacy_workspace_chrome_devtools_section_with_guidance() {
    let err = toml::from_str::<Config>(
        r#"
[workspace.chrome_devtools]
debug_port = 9222
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("[workspace.chrome_devtools]"));
    assert!(
        err.to_string()
            .contains("[workspace.browser.chrome_devtools]")
    );
}

#[test]
fn rejects_unknown_workspace_browser_chrome_devtools_field() {
    let err = toml::from_str::<Config>(
        r#"
[workspace.browser]
mode = "chrome_devtools"

[workspace.browser.chrome_devtools]
debug_port = 9222
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("unknown field `debug_port`"));
}

#[test]
fn parses_workspace_browser_system_policy() {
    let config: Config = toml::from_str(
        r#"
[workspace.browser]
mode = "system"
url = "{{site_url}}"
app = "Google Chrome"
"#,
    )
    .unwrap();

    let browser = config.workspace.unwrap().browser.unwrap();
    assert_eq!(browser.mode, WorkspaceBrowserMode::System);
    assert_eq!(browser.effective_url().unwrap().as_ref(), "{{site_url}}");
    assert_eq!(browser.app.as_deref(), Some("Google Chrome"));
}

#[test]
fn parses_workspace_browser_chrome_devtools_policy() {
    let config: Config = toml::from_str(
        r#"
[workspace.browser]
mode = "chrome_devtools"

[workspace.browser.chrome_devtools]
port = 9222
user_data_dir = ".chrome-devtools"
"#,
    )
    .unwrap();

    let browser = config.workspace.unwrap().browser.unwrap();
    assert_eq!(browser.mode, WorkspaceBrowserMode::ChromeDevtools);
    assert_eq!(browser.effective_url().unwrap().as_ref(), "{{site_url}}");
    let chrome_devtools = browser.chrome_devtools.unwrap();
    assert_eq!(chrome_devtools.port, Some(9222));
    assert_eq!(
        chrome_devtools.user_data_dir.as_deref(),
        Some(".chrome-devtools")
    );
}

#[test]
fn workspace_browser_none_rejects_unused_fields() {
    let err = toml::from_str::<Config>(
        r#"
[workspace.browser]
mode = "none"
url = "{{site_url}}"
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("[workspace.browser].url"));

    let err = toml::from_str::<Config>(
        r#"
[workspace.browser]
mode = "none"
app = "Google Chrome"
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("[workspace.browser].app"));
}

#[test]
fn workspace_browser_chrome_devtools_rejects_app() {
    let err = toml::from_str::<Config>(
        r#"
[workspace.browser]
mode = "chrome_devtools"
app = "Google Chrome"
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("[workspace.browser].app"));
}

#[test]
fn workspace_browser_inactive_modes_reject_chrome_devtools_section() {
    let err = toml::from_str::<Config>(
        r#"
[workspace.browser]
mode = "none"

[workspace.browser.chrome_devtools]
port = 9222
"#,
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("[workspace.browser.chrome_devtools]")
    );

    let err = toml::from_str::<Config>(
        r#"
[workspace.browser]
mode = "system"

[workspace.browser.chrome_devtools]
port = 9222
"#,
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("[workspace.browser.chrome_devtools]")
    );
}

#[test]
fn rejects_legacy_workspace_browser_keys() {
    let err = toml::from_str::<Config>(
        r#"
[workspace]
open_url = "{{site_url}}"
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("open_url"));

    let err = toml::from_str::<Config>(
        r#"
[workspace]
open_browser = true
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("open_browser"));

    let err = toml::from_str::<Config>(
        r#"
[workspace]
browser = "Google Chrome"
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("invalid type"));
}

#[test]
fn rejects_legacy_workspace_chrome_devtools_launch_fields() {
    let err = toml::from_str::<Config>(
        r#"
[workspace.chrome_devtools]
enabled = true
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("[workspace.chrome_devtools]"));
    assert!(
        err.to_string()
            .contains("[workspace.browser.chrome_devtools]")
    );

    let err = toml::from_str::<Config>(
        r#"
[workspace.chrome_devtools]
url = "{{site_url}}"
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("[workspace.chrome_devtools]"));
    assert!(
        err.to_string()
            .contains("[workspace.browser.chrome_devtools]")
    );
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
link = ["tmp/shared-cache"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        pathspec_pairs(&config.worktree.copy),
        vec![
            (".env", ".env"),
            (".claude/settings.local.json", ".claude/settings.local.json"),
            (".claude/hooks", ".claude/hooks")
        ]
    );
    assert_eq!(
        pathspec_pairs(&config.worktree.link),
        vec![("tmp/shared-cache", "tmp/shared-cache")]
    );
}

#[test]
fn worktree_copy_and_link_accept_rename_specs() {
    let config: Config = toml::from_str(
        r#"[worktree]
copy = [".env", { from = ".local/skills", to = ".codex/skills" }]
link = [".local", { from = ".local/skills", to = ".codex/skills" }]
"#,
    )
    .unwrap();

    assert_eq!(config.worktree.copy[0].from(), ".env");
    assert_eq!(config.worktree.copy[0].to(), ".env");
    assert_eq!(config.worktree.copy[1].from(), ".local/skills");
    assert_eq!(config.worktree.copy[1].to(), ".codex/skills");
    assert_eq!(config.worktree.link[1].from(), ".local/skills");
    assert_eq!(config.worktree.link[1].to(), ".codex/skills");
}

#[test]
fn worktree_rejects_removed_copy_as_field() {
    let err = toml::from_str::<Config>(
        r#"[worktree]
copy_as = [{ from = "a", to = "b" }]
"#,
    )
    .unwrap_err();

    assert!(
        err.to_string().contains("copy_as is no longer supported"),
        "unexpected error: {err}"
    );
}

#[test]
fn partial_config_fills_defaults() {
    let toml_str = r#"
[worktree]
copy = [".env"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(
        pathspec_pairs(&config.worktree.copy),
        vec![(".env", ".env")]
    );
    assert!(config.worktree.link.is_empty());
    assert!(config.site.is_none());
    assert!(config.workspace.is_none());
    assert_eq!(config.task_list, TaskListConfig::default());
}

#[test]
fn parses_task_list_column_config() {
    let config: Config = toml::from_str(
        r#"
[task_list.columns.run]
hidden = true
width = 7

[task_list.columns.dur]
width = 6
"#,
    )
    .unwrap();

    assert_eq!(
        config.task_list.columns.run,
        ColumnConfig {
            hidden: Some(true),
            width: Some(7)
        }
    );
    assert_eq!(config.task_list.columns.dur.width, Some(6));
    assert_eq!(config.task_list.columns.dur.hidden, None);
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
    std::fs::create_dir_all(dir.path().join(".wt/config")).ok();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[issues]
provider = "github"

[workflow]
pull_request = "draft"
landing = "manual"

[review]
codex_base = "advisory"

[site]
provider = "herd"
name = "root"

[worktree]
copy = [".env"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
        r#"
[profile.agent]
cli = "codex"

[workflow]
pull_request = "none"
landing = "auto"

[review]
codex_base = "required"

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
    assert_eq!(
        pathspec_pairs(&config.worktree.copy),
        vec![(".env", ".env"), ("CLAUDE.local.md", "CLAUDE.local.md")]
    );
    let workflow_policy = config.workflow_default_policy();
    assert_eq!(
        workflow_policy.pull_request,
        WorkflowDefaultPullRequestMode::None
    );
    assert_eq!(workflow_policy.landing, WorkflowDefaultLandingPolicy::Auto);
    assert_eq!(
        workflow_policy.review.codex_base,
        ReviewCodexBasePolicy::Required
    );
}

#[test]
fn workflow_policy_merges_per_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();

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
        dir.path().join(".wt/config/local.toml"),
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
    std::fs::create_dir_all(dir.path().join(".wt/config/profiles/codex")).unwrap();

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
        dir.path().join(".wt/config/profiles/codex/profile.toml"),
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
fn review_policy_merges_per_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[review]
codex_base = "advisory"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
        r#"
[review]
codex_base = "required"
"#,
    )
    .unwrap();

    let policy = Config::load(dir.path()).unwrap().workflow_default_policy();
    assert_eq!(policy.review.codex_base, ReviewCodexBasePolicy::Required);
}

#[test]
fn review_policy_profile_overlay_merges_per_field() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config/profiles/codex")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[review]
codex_base = "advisory"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".wt/config/profiles/codex/profile.toml"),
        r#"
[review]
codex_base = "required"
"#,
    )
    .unwrap();

    let base = Config::load_file(&dir.path().join(".wt.toml")).unwrap();
    let config = Config::load_profile(dir.path(), "codex", &base)
        .unwrap()
        .unwrap();
    let policy = config.workflow_default_policy();
    assert_eq!(policy.review.codex_base, ReviewCodexBasePolicy::Required);
}

#[test]
fn review_policy_rejects_unknown_values() {
    let err = toml::from_str::<Config>("[review]\ncodex_base = \"strict\"\n").unwrap_err();

    assert!(err.to_string().contains("strict"));
    assert!(err.to_string().contains("[review].codex_base"));
}

#[test]
fn review_policy_rejects_boolean_values() {
    let err = toml::from_str::<Config>("[review]\ncodex_base = true\n").unwrap_err();

    assert!(err.to_string().contains("[review].codex_base"));
    assert!(err.to_string().contains("boolean"));
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
    let err = toml::from_str::<Config>(&format!("[workflow]\nlanding = {value:?}\n")).unwrap_err();

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
fn profile_workspace_browser_chrome_devtools_replaces_base_section() {
    let base: Config = toml::from_str(
        r#"
[workspace.browser]
mode = "chrome_devtools"

[workspace.browser.chrome_devtools]
port = 9222
"#,
    )
    .unwrap();
    let profile: Config = toml::from_str(
        r#"
[workspace.browser]
mode = "chrome_devtools"

[workspace.browser.chrome_devtools]
user_data_dir = ".chrome-alt"
"#,
    )
    .unwrap();

    let merged = merge_config(&base, profile);
    let chrome_devtools = merged
        .workspace
        .unwrap()
        .browser
        .unwrap()
        .chrome_devtools
        .unwrap();
    assert_eq!(chrome_devtools.port, None);
    assert_eq!(
        chrome_devtools.user_data_dir.as_deref(),
        Some(".chrome-alt")
    );
}

#[test]
fn profile_workspace_browser_replaces_base_section() {
    let base: Config = toml::from_str(
        r#"
[workspace.browser]
mode = "system"
url = "{{site_url}}"
app = "Google Chrome"
"#,
    )
    .unwrap();
    let profile: Config = toml::from_str(
        r#"
[workspace.browser]
mode = "chrome_devtools"
"#,
    )
    .unwrap();

    let merged = merge_config(&base, profile);
    let browser = merged.workspace.unwrap().browser.unwrap();
    assert_eq!(browser.mode, WorkspaceBrowserMode::ChromeDevtools);
    assert_eq!(browser.url, None);
    assert_eq!(browser.app, None);
}

#[test]
fn prompt_append_layers_extend_effective_prompt_without_redeclaring_agent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();

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
        dir.path().join(".wt/config/local.toml"),
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
fn workflow_prompt_append_layers_extend_workflow_scope() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"

[agent.prompt]
workflow = ["shared workflow\n"]

[agent.prompt.append]
workflow = ["shared workflow append\n"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
        r#"
[agent.prompt.append]
workflow = ["local workflow append\n"]
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(
        agent.prompt.get("workflow").unwrap(),
        &vec!["shared workflow\n\nshared workflow append\n\nlocal workflow append\n".to_string()]
    );
}

#[test]
fn prompt_overwrite_layer_replaces_then_append_extends() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();

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
        dir.path().join(".wt/config/local.toml"),
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
fn named_profile_agent_fields_merge_by_presence_and_prompt_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/codex");
    std::fs::create_dir_all(&profile_dir).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
command = "env WT_AGENT=1 codex"
ready = "BASE_READY"
submit = "newline"
timeout = 99
send_after = 8

[agent.prompt]
common = ["base common\n"]
issue = ["base issue\n"]
branch = ["base branch\n"]
pr = ["base pr\n"]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();
    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
        r#"
[profile]
name = "codex"
"#,
    )
    .unwrap();
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[agent]
args = ["--yolo"]

[agent.prompt]
issue = ["profile issue\n"]

[agent.prompt.append]
issue = ["profile issue append\n"]
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Codex);
    assert_eq!(agent.args, vec!["--yolo"]);
    assert_eq!(agent.command.as_deref(), Some("env WT_AGENT=1 codex"));
    assert_eq!(agent.ready, ReadyMode::Marker("BASE_READY".into()));
    assert_eq!(agent.submit, SubmitMode::Newline);
    assert_eq!(agent.timeout, 99);
    assert_eq!(agent.send_after, 8);
    assert!(!agent.prompt.contains_key("common"));
    assert_eq!(
        agent.prompt.get("issue").unwrap(),
        &vec![
            "base common\n".to_string(),
            "profile issue\n\nprofile issue append\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("branch").unwrap(),
        &vec!["base common\n".to_string(), "base branch\n".to_string()]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["base common\n".to_string(), "base pr\n".to_string()]
    );
}

#[test]
fn named_profile_empty_args_clears_inherited_args() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/codex");
    std::fs::create_dir_all(&profile_dir).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();
    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
        r#"
[profile]
name = "codex"
"#,
    )
    .unwrap();
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[agent]
args = []
"#,
    )
    .unwrap();

    let config = Config::load(dir.path()).unwrap();
    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, AgentCli::Codex);
    assert!(agent.args.is_empty());
}

#[test]
fn common_prompt_scope_expands_after_layers_before_mode_prompt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();

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
        dir.path().join(".wt/config/local.toml"),
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
        agent.prompt.get("branch").unwrap(),
        &vec!["shared common\n\nshared common append\n\nlocal common append\n".to_string()]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["shared common\n\nshared common append\n\nlocal common append\n".to_string()]
    );
    assert!(!agent.prompt.contains_key("workflow"));
}

#[test]
fn profile_convention_common_prompt_files_expand_after_mode_files() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/codex");
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
    std::fs::write(
        profile_dir.join("profile.toml"),
        r#"
[agent.prompt]
issue = ["profile issue\n"]
"#,
    )
    .unwrap();
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
    std::fs::write(profile_dir.join("prompts/branch.md"), "file branch\n").unwrap();
    std::fs::write(
        profile_dir.join("prompts/branch.append.md"),
        "file branch append\n",
    )
    .unwrap();
    std::fs::write(profile_dir.join("prompts/workflow.md"), "file workflow\n").unwrap();
    std::fs::write(
        profile_dir.join("prompts/workflow.append.md"),
        "file workflow append\n",
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
        agent.prompt.get("branch").unwrap(),
        &vec![
            "file common\n\nfile common append\n".to_string(),
            "file branch\n\nfile branch append\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["file common\n\nfile common append\n".to_string()]
    );
    assert_eq!(
        agent.prompt.get("workflow").unwrap(),
        &vec!["file workflow\n\nfile workflow append\n".to_string()]
    );
}

#[test]
fn rejects_legacy_new_prompt_scope() {
    let err = toml::from_str::<Config>(
        r#"
[agent]
cli = "codex"

[agent.prompt]
new = ["legacy branch prompt\n"]
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("[agent.prompt].new"));
    assert!(err.to_string().contains("[agent.prompt].branch"));
}

#[test]
fn rejects_legacy_new_prompt_append_scope() {
    let err = toml::from_str::<Config>(
        r#"
[agent]
cli = "codex"

[agent.prompt.append]
new = ["legacy branch append\n"]
"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("[agent.prompt.append].new"));
    assert!(err.to_string().contains("[agent.prompt.append].branch"));
}

#[test]
fn rejects_legacy_new_profile_prompt_file() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/codex");
    std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();

    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
cli = "codex"
"#,
    )
    .unwrap();
    std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
    std::fs::write(profile_dir.join("prompts/new.md"), "legacy prompt\n").unwrap();

    let (base, _, _) = Config::load_base_and_effective_with_source(dir.path()).unwrap();
    let err = Config::load_profile(dir.path(), "codex", &base)
        .unwrap_err()
        .to_string();

    assert!(err.contains("prompts/new.md"));
    assert!(err.contains("prompts/branch.md"));
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
    let profiles_dir = dir.path().join(".wt/config/profiles");
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
    assert_eq!(
        pathspec_pairs(&profiles[0].1.worktree.copy),
        vec![(".env", ".env")]
    );
    assert_eq!(
        pathspec_pairs(&profiles[1].1.worktree.copy),
        vec![(".env", ".env"), ("CLAUDE.local.md", "CLAUDE.local.md")]
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
    std::fs::create_dir_all(dir.path().join(".wt/config/profiles/empty")).unwrap();
    let profiles = Config::load_profiles(dir.path(), &Config::default()).unwrap();
    assert!(profiles.is_empty());
}

#[test]
fn rejects_legacy_repo_root_personal_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local")).unwrap();
    std::fs::write(
        dir.path().join(".local/.wt.toml"),
        "[agent]\ncli = \"codex\"\n",
    )
    .unwrap();

    let err = Config::load(dir.path()).unwrap_err().to_string();

    assert!(err.contains("legacy wt personal config"));
    assert!(err.contains(".local/.wt.toml"));
    assert!(err.contains(".wt/config/local.toml"));
    assert!(err.contains("does not silently fall back"));
}

#[test]
fn reports_legacy_repo_root_profile_storage() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".local/profiles/codex")).unwrap();
    std::fs::write(
        dir.path().join(".local/profiles/codex/profile.toml"),
        "[agent]\ncli = \"codex\"\n",
    )
    .unwrap();

    let inventory = Config::load_profile_inventory(dir.path(), &Config::default()).unwrap();

    assert!(inventory.profiles.is_empty());
    assert_eq!(inventory.invalid_profiles.len(), 1);
    let invalid = &inventory.invalid_profiles[0];
    assert_eq!(invalid.name, "<legacy>");
    assert!(invalid.path.ends_with(".local/profiles"));
    assert!(invalid.error.contains("legacy wt personal profile storage"));
    assert!(invalid.error.contains(".wt/config/profiles"));
    assert!(invalid.error.contains("does not silently fall back"));
}

#[test]
fn load_with_source_applies_inline_profile_to_effective_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".wt/config")).unwrap();
    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
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
    std::fs::create_dir_all(dir.path().join(".wt/config/profiles/codex")).unwrap();
    std::fs::write(
        dir.path().join(".wt.toml"),
        "[issues]\nprovider = \"github\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".wt/config/local.toml"),
        "[profile]\nname = \"codex\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".wt/config/profiles/codex/profile.toml"),
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
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".wt.toml"),
        r#"
[agent]
args = ["--yolo"]
"#,
    )
    .unwrap();

    let err = Config::load(dir.path()).unwrap_err().to_string();
    assert!(err.contains("agent.cli is required"));
    assert!(err.contains("inheriting agent.cli"));
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
        ..AgentConfig::default()
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
        ..AgentConfig::default()
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
        ..AgentConfig::default()
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
        ..AgentConfig::default()
    };

    assert_eq!(agent.command_line().unwrap(), None);
}

#[test]
fn load_profile_returns_specific_config_or_none() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/codex");
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
    let profile_dir = dir.path().join(".wt/config/profiles/codex-yolo");
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
    base.worktree.link = vec!["tmp/shared-cache".into()];
    base.worktree.path = Some("worktrees/{{default_name}}".into());

    let profile = Config::load_profile(dir.path(), "codex-yolo", &base)
        .unwrap()
        .unwrap();

    assert!(
        profile
            .worktree
            .copy
            .iter()
            .any(|entry| entry.from() == ".env" && entry.to() == ".env")
    );
    assert_eq!(
        pathspec_pairs(&profile.worktree.link),
        vec![("tmp/shared-cache", "tmp/shared-cache")]
    );
    assert_eq!(
        profile.worktree.path.as_deref(),
        Some("worktrees/{{default_name}}")
    );
    let agent = profile.agent.unwrap();
    assert_eq!(agent.args, vec!["--yolo"]);
    assert_eq!(agent.prompt.get("issue").unwrap(), &vec!["handle issue\n"]);
    let scaffold = profile_dir.join("scaffold").display().to_string();
    assert!(
        profile
            .worktree
            .copy
            .iter()
            .any(|entry| entry.from() == scaffold.as_str() && entry.to() == ".")
    );
}

#[test]
fn load_profile_merges_worktree_fields_without_dropping_base_lists() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/alternate-path");
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
    base.worktree.link = vec!["tmp/shared-cache".into()];

    let profile = Config::load_profile(dir.path(), "alternate-path", &base)
        .unwrap()
        .unwrap();

    assert_eq!(
        profile.worktree.path.as_deref(),
        Some("profiles/{{default_name}}")
    );
    assert_eq!(
        pathspec_pairs(&profile.worktree.copy),
        vec![(".env", ".env")]
    );
    assert_eq!(
        pathspec_pairs(&profile.worktree.link),
        vec![("tmp/shared-cache", "tmp/shared-cache")]
    );
}

#[test]
fn load_profile_applies_profile_scaffold_root() {
    let dir = tempfile::tempdir().unwrap();
    let profile_dir = dir.path().join(".wt/config/profiles/claude-plan");
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

    let scaffold = profile_dir.join("scaffold").display().to_string();
    assert!(
        profile
            .worktree
            .copy
            .iter()
            .any(|entry| entry.from() == scaffold.as_str() && entry.to() == ".")
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
fn rejects_legacy_site_open_browser_config() {
    let toml_str = r#"
[site]
provider = "herd"
name = "test"
open_browser = true
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("open_browser"));
}

#[test]
fn rejects_legacy_site_browser_config() {
    let toml_str = r#"
[site]
provider = "herd"
name = "test"
browser = "Google Chrome"
"#;
    let err = toml::from_str::<Config>(toml_str).unwrap_err();
    assert!(err.to_string().contains("browser"));
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
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let site = config.effective_site().unwrap();
    assert_eq!(site.provider, SiteProvider::Herd);
    assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
    assert_eq!(site.secure, Some(true));
}

#[test]
fn effective_site_materializes_runtime_defaults() {
    let toml_str = r#"
[site]
provider = "herd"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let site = config.effective_site().unwrap();
    assert_eq!(site.provider, SiteProvider::Herd);
    assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
    assert_eq!(site.root.as_deref(), Some("."));
    assert_eq!(site.secure, Some(true));
    assert_eq!(site.url.as_deref(), Some("https://{{site_name}}.test"));
    assert_eq!(site.target, None);
}

#[test]
fn effective_traefik_site_materializes_target_default() {
    let toml_str = r#"
[site]
provider = "traefik"
secure = false
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let site = config.effective_site().unwrap();
    assert_eq!(site.provider, SiteProvider::Traefik);
    assert_eq!(site.secure, Some(false));
    assert_eq!(site.url.as_deref(), Some("http://{{site_name}}.test"));
    assert_eq!(
        site.target.as_deref(),
        Some("http://127.0.0.1:{{vite_port}}")
    );
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
fn issues_origin_policy_defaults_to_provider_preferred() {
    let config: Config = toml::from_str(
        r#"
[issues]
provider = "linear"
"#,
    )
    .unwrap();

    assert_eq!(
        config.issues.unwrap().origin_policy,
        OriginPolicy::ProviderPreferred
    );
}

#[test]
fn issues_origin_policy_parses_required_and_local_only() {
    let required: Config = toml::from_str(
        r#"
[issues]
provider = "github"
origin_policy = "provider-required"
"#,
    )
    .unwrap();
    assert_eq!(
        required.issues.unwrap().origin_policy,
        OriginPolicy::ProviderRequired
    );

    let local_only: Config = toml::from_str(
        r#"
[issues]
provider = "linear"
origin_policy = "local-only"
"#,
    )
    .unwrap();
    assert_eq!(
        local_only.issues.unwrap().origin_policy,
        OriginPolicy::LocalOnly
    );
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
