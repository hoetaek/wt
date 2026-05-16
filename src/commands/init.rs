use crate::cli::{InitAgent, InitIssueProvider, InitPreset, InitSiteProvider};
use crate::config::{AgentCli, AgentConfig, Config, ReadyMode, SubmitMode};
use crate::context::Ctx;
use crate::error::WtError;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct InitOptions {
    pub local: bool,
    pub shared: bool,
    pub preset: Option<InitPreset>,
    pub minimal: bool,
    pub agent: Option<InitAgent>,
    pub agent_args: Vec<String>,
    pub agent_command: Option<String>,
    pub issue_provider: Option<InitIssueProvider>,
    pub site_provider: Option<InitSiteProvider>,
    pub gh_user: Option<String>,
    pub yes: bool,
    pub dry_run: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitTargetKind {
    Local,
    Shared,
}

#[derive(Debug)]
struct InitTarget {
    path: PathBuf,
    kind: InitTargetKind,
}

#[derive(Debug)]
struct InitProfile {
    agent: AgentConfig,
}

#[derive(Debug)]
struct InitPlan {
    target_path: PathBuf,
    target_kind: InitTargetKind,
    preset: InitPreset,
    sections: Vec<InitSection>,
    content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitSection {
    Issues,
    Site,
    ProfileAgent,
    Worktree,
    Setup,
    Editor,
    Test,
    Workspace,
}

impl InitSection {
    fn name(self) -> &'static str {
        match self {
            InitSection::Issues => "issues",
            InitSection::Site => "site",
            InitSection::ProfileAgent => "profile.agent",
            InitSection::Worktree => "worktree",
            InitSection::Setup => "setup",
            InitSection::Editor => "editor",
            InitSection::Test => "test",
            InitSection::Workspace => "workspace",
        }
    }
}

#[derive(Debug)]
struct InitCommonConfig {
    worktree_path: Option<String>,
    setup_deps: Vec<InitCommand>,
    editor_command: Option<String>,
    test_commands: Vec<InitCommand>,
    workspace_tabs: Vec<String>,
    post_deps_tabs: Vec<String>,
}

#[derive(Debug, Clone)]
struct InitCommand {
    label: Option<String>,
    working_dir: Option<String>,
    run: String,
    if_exists: Option<String>,
    kind: InitCommandKind,
    default_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitCommandKind {
    NodeInstall,
    Other,
}

impl Default for InitCommonConfig {
    fn default() -> Self {
        Self {
            worktree_path: None,
            setup_deps: Vec::new(),
            editor_command: None,
            test_commands: Vec::new(),
            workspace_tabs: vec!["lazygit".into(), "nvim".into()],
            post_deps_tabs: Vec::new(),
        }
    }
}

pub fn run(ctx: &Ctx, options: InitOptions) -> Result<()> {
    validate_options(&options)?;
    let target = resolve_target(ctx, &options)?;
    if target.path.exists() && !options.force && !options.dry_run {
        bail!(
            "Config already exists: {} (use --force to overwrite)",
            target.path.display()
        );
    }

    let plan = build_plan(ctx, &options, target)?;

    if options.dry_run {
        print_plan(ctx, &plan);
        return Ok(());
    }

    if !options.yes {
        print_plan(ctx, &plan);
    }
    if !options.yes && !ctx.ui.confirm("Create config?", true)? {
        return Err(WtError::Cancelled.into());
    }

    if let Some(parent) = plan.target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&plan.target_path, &plan.content)?;
    ctx.ui
        .print_step(&format!("Created config: {}", plan.target_path.display()));

    Ok(())
}

fn validate_options(options: &InitOptions) -> Result<()> {
    if options.minimal && options.preset.is_some() {
        bail!("--minimal cannot be used with --preset");
    }
    Ok(())
}

fn resolve_target(ctx: &Ctx, options: &InitOptions) -> Result<InitTarget> {
    if options.local {
        return Ok(InitTarget {
            path: ctx.repo_root.join(".local/.wt.toml"),
            kind: InitTargetKind::Local,
        });
    }
    if options.shared {
        return Ok(InitTarget {
            path: ctx.repo_root.join(".wt.toml"),
            kind: InitTargetKind::Shared,
        });
    }
    if options.yes {
        return Ok(InitTarget {
            path: ctx.repo_root.join(".local/.wt.toml"),
            kind: InitTargetKind::Local,
        });
    }

    let items = vec![".local/.wt.toml".into(), ".wt.toml".into()];
    match ctx.ui.select("Where should config be created?", &items)? {
        0 => Ok(InitTarget {
            path: ctx.repo_root.join(".local/.wt.toml"),
            kind: InitTargetKind::Local,
        }),
        _ => Ok(InitTarget {
            path: ctx.repo_root.join(".wt.toml"),
            kind: InitTargetKind::Shared,
        }),
    }
}

fn resolve_preset(ctx: &Ctx, options: &InitOptions) -> Result<InitPreset> {
    if options.minimal {
        return Ok(InitPreset::Minimal);
    }
    if let Some(preset) = options.preset {
        return Ok(preset);
    }
    if let Some(preset) = preset_from_explicit_overrides(options) {
        return Ok(preset);
    }
    if options.yes {
        return Ok(InitPreset::Minimal);
    }

    let items = vec![
        "minimal".into(),
        "agent".into(),
        "issue".into(),
        "app".into(),
    ];
    Ok(match ctx.ui.select("Starter preset", &items)? {
        1 => InitPreset::Agent,
        2 => InitPreset::Issue,
        3 => InitPreset::App,
        _ => InitPreset::Minimal,
    })
}

fn preset_from_explicit_overrides(options: &InitOptions) -> Option<InitPreset> {
    if explicit_site_provider(options.site_provider.as_ref()).is_some() {
        return Some(InitPreset::App);
    }
    if explicit_issue_provider(options.issue_provider.as_ref()).is_some() {
        return Some(InitPreset::Issue);
    }
    if explicit_agent_requested(options) {
        return Some(InitPreset::Agent);
    }
    None
}

fn build_plan(ctx: &Ctx, options: &InitOptions, target: InitTarget) -> Result<InitPlan> {
    validate_options(options)?;
    let preset = resolve_preset(ctx, options)?;
    let profile = resolve_profile(ctx, options, preset)?;
    let issue_provider = resolve_issue_provider(ctx, options, preset)?;
    let gh_user = if issue_provider == Some(InitIssueProvider::Github) {
        resolve_gh_user(ctx, options)?
    } else {
        None
    };
    let site_provider = resolve_site_provider(ctx, options, preset)?;
    let common = resolve_common_config(ctx, options, preset)?;

    let mut s = String::new();
    let mut sections = Vec::new();

    if let Some(provider) = issue_provider {
        sections.push(InitSection::Issues);
        s.push_str("[issues]\n");
        s.push_str(&format!(
            "provider = {}\n",
            toml_quote(issue_provider_name(&provider))
        ));
        if provider == InitIssueProvider::Github {
            if let Some(user) = gh_user.as_deref() {
                s.push_str(&format!("gh_user = {}\n", toml_quote(user)));
            }
        }
        s.push('\n');
    }

    if let Some(provider) = site_provider {
        sections.push(InitSection::Site);
        s.push_str("[site]\n");
        s.push_str(&format!(
            "provider = {}\n",
            toml_quote(site_provider_name(&provider))
        ));
        if matches!(provider, InitSiteProvider::Traefik) {
            s.push_str("name = \"{{repo}}-{{branch_slug}}.l\"\n");
            s.push_str("url = \"https://{{site_name}}\"\n");
            s.push_str("target = \"http://127.0.0.1:{{vite_port}}\"\n");
        } else {
            s.push_str("name = \"{{repo}}-{{branch_slug}}\"\n");
        }
        if matches!(
            provider,
            InitSiteProvider::Herd | InitSiteProvider::Valet | InitSiteProvider::Traefik
        ) {
            s.push_str("secure = true\n");
        }
        s.push('\n');
    }

    if let Some(profile) = &profile {
        sections.push(InitSection::ProfileAgent);
        append_profile_selection(&mut s, profile);
    }

    append_active_common_config(&mut s, &common, &mut sections);

    append_optional_scaffold(&mut s);

    toml::from_str::<Config>(&s)?;

    Ok(InitPlan {
        target_path: target.path,
        target_kind: target.kind,
        preset,
        sections,
        content: s,
    })
}

fn print_plan(ctx: &Ctx, plan: &InitPlan) {
    ctx.ui.print_step("Init plan");
    ctx.ui
        .print_dim(&format!("  target: {}", plan.target_path.display()));
    ctx.ui.print_dim(&format!(
        "  target kind: {}",
        target_kind_name(plan.target_kind)
    ));
    ctx.ui
        .print_dim(&format!("  preset: {}", preset_name(plan.preset)));
    let sections = if plan.sections.is_empty() {
        "none".to_string()
    } else {
        plan.sections
            .iter()
            .map(|section| section.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    ctx.ui.print_dim(&format!("  sections: {sections}"));
    ctx.ui.print_dim("  toml:");
    for line in plan.content.lines() {
        ctx.ui.print_dim(&format!("    {line}"));
    }
}

fn append_optional_scaffold(s: &mut String) {
    s.push_str("# Optional worktree behavior. Uncomment and adjust as needed.\n");
    s.push_str("# [worktree]\n");
    s.push_str("# path = \"$HOME/worktrees/{{default_name}}\"\n");
    s.push_str("# copy = [\".env\"]\n");
    s.push_str("# copy_as = [\n");
    s.push_str("#     { from = \".local/templates/.env\", to = \".env\" },\n");
    s.push_str("# ]\n");
    s.push_str("# link = [\".local\"]\n");
    s.push_str("# inject_local_context = \"\"\"\n");
    s.push_str("# ## Local context\n");
    s.push_str("# - site: {{site_url}}\n");
    s.push_str("# - worktree: {{worktree_path}}\n");
    s.push_str("# - parent: {{parent_branch}}\n");
    s.push_str("# \"\"\"\n\n");

    s.push_str("# Optional AI-assisted naming for issue worktrees.\n");
    s.push_str("# [worktree.naming]\n");
    s.push_str("# command = \"claude -p\"\n");
    s.push_str("# branch = \"{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}\"\n");
    s.push('\n');

    s.push_str("# Optional setup commands run inside each worktree.\n");
    s.push_str(
        "# Omit if_exists for required setup; use it only for intentionally optional commands.\n",
    );
    s.push_str("# [setup]\n");
    s.push_str("# deps = [\n");
    s.push_str("#     { run = \"npm install\" },\n");
    s.push_str("#     { working_dir = \"frontend\", run = \"pnpm install\" },\n");
    s.push_str("#     { working_dir = \"api\", run = \"uv sync\" },\n");
    s.push_str(
        "#     { working_dir = \"enterprise\", run = \"pnpm install\", if_exists = \"package.json\" },\n",
    );
    s.push_str("# ]\n\n");
    s.push_str("# Optional environment substitutions for existing env files.\n");
    s.push_str("# [setup.env]\n");
    s.push_str("# APP_ENV = \"local\"\n\n");
    s.push_str("# [setup.env_files.\".env.local\"]\n");
    s.push_str("# APP_URL = \"{{site_url}}\"\n");
    s.push_str("# VITE_PORT = \"{{vite_port}}\"\n\n");

    s.push_str("# Optional editor for wt-managed TOML files.\n");
    s.push_str("# [editor]\n");
    s.push_str("# command = \"nvim {{path}}\"\n");
    s.push_str("# placement = \"cmux_surface\"\n\n");

    s.push_str("# Optional test commands run after setup.\n");
    s.push_str("# [test]\n");
    s.push_str("# commands = [\n");
    s.push_str("#     { label = \"test\", run = \"cargo test\" },\n");
    s.push_str("#     { label = \"lint\", working_dir = \"frontend\", run = \"npm run lint\" },\n");
    s.push_str("#     { label = \"optional-pest\", working_dir = \"backend\", run = \"./vendor/bin/pest\", if_exists = \"vendor/bin/pest\" },\n");
    s.push_str("# ]\n\n");
}

fn append_active_common_config(
    s: &mut String,
    common: &InitCommonConfig,
    sections: &mut Vec<InitSection>,
) {
    if let Some(path) = common.worktree_path.as_deref() {
        sections.push(InitSection::Worktree);
        s.push_str("[worktree]\n");
        s.push_str(&format!("path = {}\n\n", toml_quote(path)));
    }

    if !common.setup_deps.is_empty() {
        sections.push(InitSection::Setup);
        s.push_str("[setup]\n");
        s.push_str("deps = [\n");
        for command in &common.setup_deps {
            append_command_entry(s, command);
        }
        s.push_str("]\n\n");
    }

    if let Some(command) = common.editor_command.as_deref() {
        sections.push(InitSection::Editor);
        s.push_str("[editor]\n");
        s.push_str(&format!("command = {}\n", toml_quote(command)));
        s.push_str("placement = \"cmux_surface\"\n\n");
    }

    if !common.test_commands.is_empty() {
        sections.push(InitSection::Test);
        s.push_str("[test]\n");
        s.push_str("commands = [\n");
        for command in &common.test_commands {
            append_command_entry(s, command);
        }
        s.push_str("]\n\n");
    }

    sections.push(InitSection::Workspace);
    s.push_str("[workspace]\n");
    s.push_str(&format!("tabs = {}\n", toml_array(&common.workspace_tabs)));
    if !common.post_deps_tabs.is_empty() {
        s.push_str(&format!(
            "post_deps_tabs = {}\n",
            toml_array(&common.post_deps_tabs)
        ));
    } else {
        s.push_str("# post_deps_tabs = [\"npm run dev\"]\n");
    }
    s.push_str("# open_url = \"{{site_url}}\"\n");
    s.push_str("# open_browser = true\n");
    s.push_str("# browser = \"Google Chrome\"\n");
    s.push_str("# colors = { issue = \"blue\", pr = \"magenta\", new = \"green\" }\n\n");
}

fn append_command_entry(s: &mut String, command: &InitCommand) {
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

fn resolve_common_config(
    ctx: &Ctx,
    options: &InitOptions,
    preset: InitPreset,
) -> Result<InitCommonConfig> {
    let mut config = InitCommonConfig::default();
    if options.yes {
        if preset == InitPreset::App {
            config.setup_deps = default_enabled_setup_deps(&ctx.repo_root);
            config.test_commands = default_enabled_test_commands(&ctx.repo_root);
        }
        return Ok(config);
    }

    if preset == InitPreset::App {
        return resolve_custom_common_config(ctx, config);
    }

    let items = vec![
        "minimal".into(),
        "customize frequently used settings".into(),
    ];
    if ctx.ui.select("Additional settings", &items)? == 0 {
        return Ok(config);
    }

    resolve_custom_common_config(ctx, config)
}

fn resolve_custom_common_config(
    ctx: &Ctx,
    mut config: InitCommonConfig,
) -> Result<InitCommonConfig> {
    config.worktree_path = resolve_worktree_path(ctx)?;
    config.workspace_tabs = resolve_workspace_tabs(ctx)?;

    config.setup_deps = resolve_setup_deps(ctx)?;

    let post_deps_tabs = detect_post_deps_tabs(&ctx.repo_root);
    if !post_deps_tabs.is_empty()
        && ctx
            .ui
            .confirm("Start detected dev command after deps?", false)?
    {
        config.post_deps_tabs = post_deps_tabs;
    }

    let test_commands = detect_test_commands(&ctx.repo_root);
    if !test_commands.is_empty() && ctx.ui.confirm("Add detected test commands?", true)? {
        config.test_commands = test_commands;
    }

    config.editor_command = resolve_editor_command(ctx)?;
    Ok(config)
}

fn default_enabled_setup_deps(repo_root: &Path) -> Vec<InitCommand> {
    detect_setup_deps(repo_root)
        .into_iter()
        .filter(|command| command.default_enabled)
        .collect()
}

fn default_enabled_test_commands(repo_root: &Path) -> Vec<InitCommand> {
    detect_test_commands(repo_root)
        .into_iter()
        .filter(|command| command.default_enabled)
        .collect()
}

fn resolve_worktree_path(ctx: &Ctx) -> Result<Option<String>> {
    let items = vec![
        "default sibling path".into(),
        "$HOME/worktrees/{{default_name}}".into(),
        "custom".into(),
    ];
    match ctx.ui.select("Worktree path", &items)? {
        0 => Ok(None),
        1 => Ok(Some("$HOME/worktrees/{{default_name}}".into())),
        _ => {
            let input = ctx.ui.input(
                "Worktree path template",
                Some("$HOME/worktrees/{{default_name}}"),
            )?;
            let input = input.trim();
            Ok((!input.is_empty()).then(|| input.to_string()))
        }
    }
}

fn resolve_workspace_tabs(ctx: &Ctx) -> Result<Vec<String>> {
    let input = ctx.ui.input("Workspace tabs", Some("lazygit, nvim"))?;
    let tabs = split_list(&input);
    if tabs.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(tabs)
    }
}

fn resolve_editor_command(ctx: &Ctx) -> Result<Option<String>> {
    let items = vec![
        "default editor".into(),
        "nvim {{path}}".into(),
        "code {{path}}".into(),
        "custom".into(),
    ];
    match ctx.ui.select("Config editor", &items)? {
        0 => Ok(None),
        1 => Ok(Some("nvim {{path}}".into())),
        2 => Ok(Some("code {{path}}".into())),
        _ => {
            let input = ctx.ui.input("Editor command", Some("nvim {{path}}"))?;
            let input = input.trim();
            Ok((!input.is_empty()).then(|| input.to_string()))
        }
    }
}

fn split_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn resolve_setup_deps(ctx: &Ctx) -> Result<Vec<InitCommand>> {
    let mut selected = Vec::new();
    for mut command in detect_setup_deps(&ctx.repo_root) {
        let display = command_display(&command);
        if !ctx.ui.confirm(
            &format!("Add setup command: {display}?"),
            command.default_enabled,
        )? {
            continue;
        }
        if command.kind == InitCommandKind::NodeInstall {
            command.run = resolve_node_install_command(ctx, &command)?;
        }
        selected.push(command);
    }
    Ok(selected)
}

fn resolve_node_install_command(ctx: &Ctx, command: &InitCommand) -> Result<String> {
    let detected = command.run.as_str();
    let mut options = Vec::new();
    push_node_install_option(&mut options, format!("{detected} (detected)"), detected);
    push_node_install_option(&mut options, "npm install".into(), "npm install");
    push_node_install_option(&mut options, "pnpm install".into(), "pnpm install");
    push_node_install_option(&mut options, "yarn install".into(), "yarn install");
    push_node_install_option(&mut options, "bun install".into(), "bun install");
    push_node_install_option(
        &mut options,
        "nvm use && npm install".into(),
        "bash -lc 'source \"$HOME/.nvm/nvm.sh\" && nvm use && npm install'",
    );

    let mut items = options
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    items.push("custom".into());

    let prompt = command.working_dir.as_deref().map_or_else(
        || "Node install command".to_string(),
        |working_dir| format!("Node install command for {working_dir}"),
    );
    let selection = ctx.ui.select(&prompt, &items)?;
    if selection < options.len() {
        return Ok(options[selection].1.clone());
    }

    let input = ctx.ui.input("Custom setup command", Some(detected))?;
    let input = input.trim();
    Ok(if input.is_empty() {
        detected.to_string()
    } else {
        input.to_string()
    })
}

fn push_node_install_option(options: &mut Vec<(String, String)>, label: String, run: &str) {
    if !options.iter().any(|(_, existing)| existing == run) {
        options.push((label, run.to_string()));
    }
}

fn detect_setup_deps(repo_root: &Path) -> Vec<InitCommand> {
    let mut commands = Vec::new();
    for rel_dir in detect_manifest_dirs(
        repo_root,
        &["package.json", "composer.json", "Gemfile", "pyproject.toml"],
    ) {
        let project_root = repo_root.join(&rel_dir);
        let working_dir = relative_dir(&rel_dir);
        if project_root.join("package.json").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir: working_dir.clone(),
                run: format!("{} install", node_package_manager(&project_root)),
                if_exists: None,
                kind: InitCommandKind::NodeInstall,
                default_enabled: true,
            });
        }
        if project_root.join("composer.json").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir: working_dir.clone(),
                run: "composer install".into(),
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: true,
            });
        }
        if project_root.join("Gemfile").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir: working_dir.clone(),
                run: "bundle install".into(),
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: true,
            });
        }
        if project_root.join("pyproject.toml").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir,
                run: "uv sync".into(),
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: is_probable_uv_project(&project_root),
            });
        }
    }
    commands
}

fn detect_post_deps_tabs(repo_root: &Path) -> Vec<String> {
    detect_package_roots(repo_root)
        .into_iter()
        .filter_map(|rel_dir| {
            let project_root = repo_root.join(&rel_dir);
            let working_dir = relative_dir(&rel_dir);
            package_script_command(&project_root, "dev")
                .map(|run| command_for_workspace_tab(working_dir.as_deref(), &run))
        })
        .collect()
}

fn detect_test_commands(repo_root: &Path) -> Vec<InitCommand> {
    let mut commands = Vec::new();
    for rel_dir in detect_manifest_dirs(repo_root, &["Cargo.toml"]) {
        commands.push(InitCommand {
            label: Some("test".into()),
            working_dir: relative_dir(&rel_dir),
            run: "cargo test".into(),
            if_exists: None,
            kind: InitCommandKind::Other,
            default_enabled: true,
        });
    }
    for rel_dir in detect_package_roots(repo_root) {
        let project_root = repo_root.join(&rel_dir);
        let working_dir = relative_dir(&rel_dir);
        if let Some(run) = package_script_command(&project_root, "test") {
            commands.push(InitCommand {
                label: Some("test".into()),
                working_dir: working_dir.clone(),
                run,
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: true,
            });
        }
        if let Some(run) = package_script_command(&project_root, "lint") {
            commands.push(InitCommand {
                label: Some("lint".into()),
                working_dir,
                run,
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: true,
            });
        }
    }
    commands
}

fn command_display(command: &InitCommand) -> String {
    let command_part = command.working_dir.as_deref().map_or_else(
        || command.run.clone(),
        |working_dir| format!("{working_dir}: {}", command.run),
    );
    command
        .if_exists
        .as_deref()
        .map_or(command_part.clone(), |guard| {
            let guard = command.working_dir.as_deref().map_or_else(
                || guard.to_string(),
                |working_dir| format!("{working_dir}/{guard}"),
            );
            format!("{command_part} (when {guard} exists)")
        })
}

fn command_for_workspace_tab(working_dir: Option<&str>, run: &str) -> String {
    working_dir.map_or_else(
        || run.to_string(),
        |working_dir| format!("cd {} && {run}", shell_words::quote(working_dir)),
    )
}

fn package_script_command(project_root: &Path, script: &str) -> Option<String> {
    if !package_script_exists(project_root, script) {
        return None;
    }

    let manager = node_package_manager(project_root);
    let command = if script == "test" && manager == "npm" {
        "npm test".into()
    } else {
        format!("{manager} run {script}")
    };
    Some(command)
}

fn package_script_exists(project_root: &Path, script: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(project_root.join("package.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    value
        .get("scripts")
        .and_then(|scripts| scripts.get(script))
        .and_then(|value| value.as_str())
        .is_some()
}

fn detect_package_roots(repo_root: &Path) -> Vec<PathBuf> {
    detect_manifest_dirs(repo_root, &["package.json"])
}

fn detect_manifest_dirs(repo_root: &Path, manifests: &[&str]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    scan_manifest_dirs(repo_root, Path::new(""), 0, manifests, &mut roots);
    roots.sort_by_key(|path| path.to_string_lossy().to_string());
    roots.dedup();
    roots
}

fn scan_manifest_dirs(
    repo_root: &Path,
    rel_dir: &Path,
    depth: usize,
    manifests: &[&str],
    roots: &mut Vec<PathBuf>,
) {
    const MAX_MANIFEST_SCAN_DEPTH: usize = 4;

    let dir = repo_root.join(rel_dir);
    if manifests.iter().any(|manifest| dir.join(manifest).exists()) {
        roots.push(rel_dir.to_path_buf());
    }
    if depth >= MAX_MANIFEST_SCAN_DEPTH {
        return;
    }

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut child_dirs = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then(|| entry.file_name())
        })
        .filter_map(|name| name.into_string().ok())
        .filter(|name| !is_ignored_manifest_dir(name))
        .collect::<Vec<_>>();
    child_dirs.sort();

    for child in child_dirs {
        let child_rel = if rel_dir.as_os_str().is_empty() {
            PathBuf::from(child)
        } else {
            rel_dir.join(child)
        };
        scan_manifest_dirs(repo_root, &child_rel, depth + 1, manifests, roots);
    }
}

fn is_ignored_manifest_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".local"
            | ".next"
            | ".nuxt"
            | ".turbo"
            | ".cache"
            | "node_modules"
            | "vendor"
            | "target"
            | "dist"
            | "build"
            | "coverage"
            | "tmp"
    )
}

fn relative_dir(rel_dir: &Path) -> Option<String> {
    (!rel_dir.as_os_str().is_empty()).then(|| rel_dir.to_string_lossy().replace('\\', "/"))
}

fn is_probable_uv_project(project_root: &Path) -> bool {
    if project_root.join("uv.lock").exists() {
        return true;
    }
    let Ok(pyproject) = std::fs::read_to_string(project_root.join("pyproject.toml")) else {
        return false;
    };
    pyproject.contains("[tool.uv") || pyproject.contains("[dependency-groups]")
}

fn node_package_manager(project_root: &Path) -> &'static str {
    package_manager_from_package_json(project_root).unwrap_or_else(|| {
        if project_root.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else if project_root.join("bun.lock").exists() || project_root.join("bun.lockb").exists()
        {
            "bun"
        } else if project_root.join("yarn.lock").exists() {
            "yarn"
        } else {
            "npm"
        }
    })
}

fn package_manager_from_package_json(project_root: &Path) -> Option<&'static str> {
    let content = std::fs::read_to_string(project_root.join("package.json")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    let package_manager = value.get("packageManager")?.as_str()?;
    let name = package_manager.split('@').next().unwrap_or(package_manager);
    known_node_package_manager(name)
}

fn known_node_package_manager(name: &str) -> Option<&'static str> {
    match name {
        "npm" => Some("npm"),
        "pnpm" => Some("pnpm"),
        "yarn" => Some("yarn"),
        "bun" => Some("bun"),
        _ => None,
    }
}

fn resolve_profile(
    ctx: &Ctx,
    options: &InitOptions,
    preset: InitPreset,
) -> Result<Option<InitProfile>> {
    if !preset_includes_agent(preset) && !explicit_agent_requested(options) {
        return Ok(None);
    }

    let agent = resolve_agent(ctx, options)?;
    let command = resolve_agent_command(options)?;
    if agent == InitAgent::None {
        if command.is_some() {
            bail!("--agent-command cannot be used when --agent none");
        }
        if !options.agent_args.is_empty() {
            bail!("--agent-arg cannot be used when --agent none");
        }
        return Ok(None);
    }
    let args = resolve_agent_args(ctx, &agent, options)?;
    Ok(build_profile(&agent, args, command))
}

fn resolve_agent(ctx: &Ctx, options: &InitOptions) -> Result<InitAgent> {
    if let Some(agent) = &options.agent {
        return Ok(agent.clone());
    }
    if options.yes {
        return Ok(InitAgent::Codex);
    }

    let items = vec![
        "codex".into(),
        "claude".into(),
        "gemini".into(),
        "none".into(),
    ];
    let agent = match ctx.ui.select("Select coding agent", &items)? {
        0 => InitAgent::Codex,
        1 => InitAgent::Claude,
        2 => InitAgent::Gemini,
        _ => InitAgent::None,
    };
    Ok(agent)
}

fn resolve_agent_command(options: &InitOptions) -> Result<Option<String>> {
    if let Some(command) = &options.agent_command {
        return Ok(Some(command.clone()));
    }
    Ok(None)
}

fn resolve_agent_args(ctx: &Ctx, agent: &InitAgent, options: &InitOptions) -> Result<Vec<String>> {
    if !options.agent_args.is_empty() {
        return Ok(options.agent_args.clone());
    }
    if options.yes {
        return Ok(Vec::new());
    }
    if *agent == InitAgent::None {
        return Ok(Vec::new());
    }

    let items = vec!["none".into(), "enter args".into()];
    match ctx.ui.select("Agent args", &items)? {
        0 => Ok(Vec::new()),
        _ => {
            let input = ctx.ui.input("Agent args", None)?;
            Ok(input
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>())
        }
    }
}

fn resolve_issue_provider(
    ctx: &Ctx,
    options: &InitOptions,
    preset: InitPreset,
) -> Result<Option<InitIssueProvider>> {
    if let Some(provider) = explicit_issue_provider(options.issue_provider.as_ref()) {
        return Ok(Some(provider));
    }
    if matches!(options.issue_provider, Some(InitIssueProvider::None)) {
        return Ok(None);
    }
    if !preset_includes_issue_provider(preset) {
        return Ok(None);
    }
    if options.yes {
        return Ok(Some(InitIssueProvider::Github));
    }

    let items = vec!["github".into(), "linear".into(), "none".into()];
    Ok(match ctx.ui.select("Issue provider", &items)? {
        0 => Some(InitIssueProvider::Github),
        1 => Some(InitIssueProvider::Linear),
        _ => None,
    })
}

fn resolve_site_provider(
    ctx: &Ctx,
    options: &InitOptions,
    preset: InitPreset,
) -> Result<Option<InitSiteProvider>> {
    if let Some(provider) = explicit_site_provider(options.site_provider.as_ref()) {
        return Ok(Some(provider));
    }
    if matches!(options.site_provider, Some(InitSiteProvider::None)) {
        return Ok(None);
    }
    if !preset_includes_site_provider(preset) {
        return Ok(None);
    }
    if options.yes {
        return Ok(None);
    }

    let items = vec![
        "none".into(),
        "herd".into(),
        "valet".into(),
        "docker_proxy".into(),
        "traefik".into(),
    ];
    Ok(match ctx.ui.select("Local site provider", &items)? {
        1 => Some(InitSiteProvider::Herd),
        2 => Some(InitSiteProvider::Valet),
        3 => Some(InitSiteProvider::DockerProxy),
        4 => Some(InitSiteProvider::Traefik),
        _ => None,
    })
}

fn resolve_gh_user(ctx: &Ctx, options: &InitOptions) -> Result<Option<String>> {
    if let Some(user) = options.gh_user.as_deref() {
        let user = user.trim();
        return Ok((!user.is_empty()).then(|| user.to_string()));
    }
    if options.yes {
        return Ok(None);
    }

    let user = ctx.ui.input("GitHub username (optional)", Some(""))?;
    let user = user.trim();
    Ok((!user.is_empty()).then(|| user.to_string()))
}

fn build_profile(
    agent: &InitAgent,
    args: Vec<String>,
    command: Option<String>,
) -> Option<InitProfile> {
    (*agent != InitAgent::None).then(|| InitProfile {
        agent: AgentConfig {
            cli: init_agent_cli(agent),
            args,
            command,
            ready: ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 30,
            send_after: 2,
            prompt: Default::default(),
        },
    })
}

fn append_profile_selection(s: &mut String, profile: &InitProfile) {
    append_inline_agent_section(s, &profile.agent);
    s.push('\n');
}

fn append_inline_agent_section(s: &mut String, agent: &AgentConfig) {
    s.push_str("[profile.agent]\n");
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
        "timeout = {}\nsend_after = {}\n",
        agent.timeout, agent.send_after
    ));
}

fn init_agent_cli(agent: &InitAgent) -> AgentCli {
    match agent {
        InitAgent::Codex => AgentCli::Codex,
        InitAgent::Claude => AgentCli::Claude,
        InitAgent::Gemini => AgentCli::Gemini,
        InitAgent::None => AgentCli::None,
    }
}

fn preset_includes_agent(preset: InitPreset) -> bool {
    preset == InitPreset::Agent
}

fn preset_includes_issue_provider(preset: InitPreset) -> bool {
    preset == InitPreset::Issue
}

fn preset_includes_site_provider(preset: InitPreset) -> bool {
    preset == InitPreset::App
}

fn explicit_agent_requested(options: &InitOptions) -> bool {
    matches!(
        options.agent.as_ref(),
        Some(InitAgent::Codex | InitAgent::Claude | InitAgent::Gemini)
    ) || options.agent_command.is_some()
        || !options.agent_args.is_empty()
}

fn explicit_issue_provider(provider: Option<&InitIssueProvider>) -> Option<InitIssueProvider> {
    match provider {
        Some(InitIssueProvider::Github) => Some(InitIssueProvider::Github),
        Some(InitIssueProvider::Linear) => Some(InitIssueProvider::Linear),
        Some(InitIssueProvider::None) | None => None,
    }
}

fn explicit_site_provider(provider: Option<&InitSiteProvider>) -> Option<InitSiteProvider> {
    match provider {
        Some(InitSiteProvider::Herd) => Some(InitSiteProvider::Herd),
        Some(InitSiteProvider::Valet) => Some(InitSiteProvider::Valet),
        Some(InitSiteProvider::DockerProxy) => Some(InitSiteProvider::DockerProxy),
        Some(InitSiteProvider::Traefik) => Some(InitSiteProvider::Traefik),
        Some(InitSiteProvider::None) | None => None,
    }
}

fn agent_cli_name(agent: &AgentCli) -> &'static str {
    match agent {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::None => "none",
    }
}

fn preset_name(preset: InitPreset) -> &'static str {
    match preset {
        InitPreset::Minimal => "minimal",
        InitPreset::Agent => "agent",
        InitPreset::Issue => "issue",
        InitPreset::App => "app",
    }
}

fn target_kind_name(kind: InitTargetKind) -> &'static str {
    match kind {
        InitTargetKind::Local => "local",
        InitTargetKind::Shared => "shared",
    }
}

fn issue_provider_name(provider: &InitIssueProvider) -> &'static str {
    match provider {
        InitIssueProvider::Github => "github",
        InitIssueProvider::Linear => "linear",
        InitIssueProvider::None => "none",
    }
}

fn site_provider_name(provider: &InitSiteProvider) -> &'static str {
    match provider {
        InitSiteProvider::None => "none",
        InitSiteProvider::Herd => "herd",
        InitSiteProvider::Valet => "valet",
        InitSiteProvider::DockerProxy => "docker_proxy",
        InitSiteProvider::Traefik => "traefik",
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

fn toml_array(values: &[String]) -> String {
    let rendered = values
        .iter()
        .map(|value| toml_quote(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentCli, IssueProviderType, SiteProvider};
    use crate::context::UserInterface;
    use crate::context::mock::{MockRunner, MockUi};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn local_target(dir: &tempfile::TempDir) -> InitTarget {
        InitTarget {
            path: dir.path().join(".local/.wt.toml"),
            kind: InitTargetKind::Local,
        }
    }

    fn ctx_for_dir(dir: &tempfile::TempDir) -> Ctx {
        Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    #[test]
    fn init_minimal_preset_plan_records_target_preset_and_sections() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                preset: Some(InitPreset::Minimal),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(plan.target_path, dir.path().join(".local/.wt.toml"));
        assert_eq!(plan.target_kind, InitTargetKind::Local);
        assert_eq!(plan.preset, InitPreset::Minimal);
        assert_eq!(plan.sections, vec![InitSection::Workspace]);
        assert!(!plan.content.contains("[profile.agent]"));
        assert!(!plan.content.contains("[issues]"));
        assert!(!plan.content.contains("[site]"));
    }

    #[test]
    fn init_agent_preset_plan_writes_agent_section() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                preset: Some(InitPreset::Agent),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(
            plan.sections,
            vec![InitSection::ProfileAgent, InitSection::Workspace]
        );
        assert!(plan.content.contains("[profile.agent]"));
        assert!(plan.content.contains("cli = \"codex\""));
        assert!(!plan.content.contains("[issues]"));
    }

    #[test]
    fn init_issue_preset_plan_writes_issue_section() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                preset: Some(InitPreset::Issue),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(
            plan.sections,
            vec![InitSection::Issues, InitSection::Workspace]
        );
        assert!(plan.content.contains("[issues]"));
        assert!(plan.content.contains("provider = \"github\""));
        assert!(!plan.content.contains("[profile.agent]"));
    }

    #[test]
    fn init_app_preset_plan_writes_detected_app_sections() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                preset: Some(InitPreset::App),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(
            plan.sections,
            vec![
                InitSection::Setup,
                InitSection::Test,
                InitSection::Workspace
            ]
        );
        assert!(plan.content.contains("[setup]"));
        assert!(plan.content.contains("run = \"npm install\""));
        assert!(plan.content.contains("[test]"));
        assert!(plan.content.contains("run = \"npm test\""));
    }

    #[test]
    fn init_local_codex_yes_creates_parseable_config() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Agent),
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(content.contains("# path = \"$HOME/worktrees/{{default_name}}\""));
        assert!(content.contains("# copy_as = ["));
        assert!(content.contains("# inject_local_context = \"\"\""));
        assert!(content.contains("# [worktree.naming]"));
        assert!(
            content
                .contains("# branch = \"{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}\"")
        );
        assert!(!content.contains("{{english_title}}"));
        assert!(content.contains("# [setup.env]"));
        assert!(content.contains("# [setup.env_files.\".env.local\"]"));
        assert!(content.contains("# [editor]"));
        assert!(content.contains("# [test]"));
        assert!(content.contains("# post_deps_tabs = [\"npm run dev\"]"));
        assert!(
            content.contains("# colors = { issue = \"blue\", pr = \"magenta\", new = \"green\" }")
        );
        assert!(config.worktree.path.is_none());
        assert!(config.worktree.naming.is_none());
        let profile = config.profile.unwrap();
        let agent = profile.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(config.agent.is_none());
        assert!(!content.contains("[agent]"));
        assert!(agent.args.is_empty());
        assert!(!content.contains("args ="));
        assert!(
            !dir.path()
                .join(".local/profiles/codex/profile.toml")
                .exists()
        );
        assert!(!content.contains("현재 GitHub 이슈"));
    }

    #[test]
    fn init_shared_with_github_options_creates_only_shared_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(true); // create config
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: true,
                preset: Some(InitPreset::Issue),
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: Some(InitSiteProvider::None),
                gh_user: Some("alice".into()),
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(config.agent.is_none());
        assert!(config.profile.is_none());
        assert!(!content.contains("현재 이슈/작업 컨텍스트"));
        assert!(!content.contains("현재 GitHub 이슈"));
        let issues = config.issues.unwrap();
        assert_eq!(issues.provider, IssueProviderType::Github);
        assert_eq!(issues.gh_user.as_deref(), Some("alice"));

        assert!(!dir.path().join(".local/.wt.toml").exists());
        assert!(
            !dir.path()
                .join(".local/profiles/gemini/profile.toml")
                .exists()
        );
    }

    #[test]
    fn init_github_without_user_omits_gh_user() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Issue),
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let issues = config.issues.unwrap();
        assert_eq!(issues.provider, IssueProviderType::Github);
        assert!(issues.gh_user.is_none());
        assert!(!content.contains("alice"));
    }

    #[test]
    fn init_with_site_provider_writes_site_section() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // no agent args
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(true); // create config
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Agent),
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::Valet),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let site = config.site.unwrap();
        assert_eq!(site.provider, SiteProvider::Valet);
        assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
        assert_eq!(site.secure, Some(true));
        assert!(!content.contains("[herd]"));
    }

    #[test]
    fn init_with_traefik_site_provider_writes_traefik_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // no agent args
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(true); // create config
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Agent),
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::Traefik),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let site = config.site.unwrap();
        assert_eq!(site.provider, SiteProvider::Traefik);
        assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}.l"));
        assert_eq!(site.url.as_deref(), Some("https://{{site_name}}"));
        assert_eq!(
            site.target.as_deref(),
            Some("http://127.0.0.1:{{vite_port}}")
        );
        assert_eq!(site.secure, Some(true));
    }

    #[test]
    fn init_interactive_flow_uses_ui_answers() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1); // .wt.toml
        ui.add_select(1); // agent preset
        ui.add_select(0); // codex
        ui.add_select(0); // no agent args
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(true); // create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(config.agent.is_none());
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(!dir.path().join(".local/.wt.toml").exists());
        assert!(config.issues.is_none());
    }

    #[test]
    fn init_interactive_flow_accepts_manual_agent_args() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(1); // agent preset
        ui.add_select(0); // codex
        ui.add_select(1); // enter agent args
        ui.add_input("--model gpt-5.5");
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(true); // create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(agent.args, vec!["--model", "gpt-5.5"]);
    }

    #[test]
    fn init_interactive_custom_common_config_writes_selected_settings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        let mut ui = MockUi::new();
        ui.add_select(1); // customize frequently used settings
        ui.add_select(1); // $HOME/worktrees/{{default_name}}
        ui.add_input("lazygit, nvim, pnpm run dev");
        ui.add_confirm(true); // add detected setup commands
        ui.add_select(0); // detected pnpm install
        ui.add_confirm(true); // start detected dev command after deps
        ui.add_confirm(true); // add detected test commands
        ui.add_select(1); // nvim {{path}}
        ui.add_confirm(true); // create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Minimal),
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(
            config.worktree.path.as_deref(),
            Some("$HOME/worktrees/{{default_name}}")
        );
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].run, "pnpm install");
        assert!(config.setup.deps[0].working_dir.is_none());
        assert!(config.setup.deps[0].if_exists.is_none());
        assert_eq!(config.editor.command.as_deref(), Some("nvim {{path}}"));

        let workspace = config.workspace.unwrap();
        assert_eq!(
            workspace.tabs,
            vec![
                "lazygit".to_string(),
                "nvim".to_string(),
                "pnpm run dev".to_string()
            ]
        );
        assert_eq!(workspace.post_deps_tabs, vec!["pnpm run dev".to_string()]);

        let test = config.test.unwrap();
        assert!(test.commands.iter().any(|command| {
            command.label.as_deref() == Some("test")
                && command.run == "pnpm run test"
                && command.working_dir.is_none()
                && command.if_exists.is_none()
        }));
        assert!(test.commands.iter().any(|command| {
            command.label.as_deref() == Some("lint")
                && command.run == "pnpm run lint"
                && command.working_dir.is_none()
                && command.if_exists.is_none()
        }));
    }

    #[test]
    fn init_interactive_setup_deps_supports_root_polyglot_and_subdir_override() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{}}"#).unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::write(
            dir.path().join("apps/web/package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_select(1); // customize frequently used settings
        ui.add_select(0); // default worktree path
        ui.add_input("lazygit, nvim");
        ui.add_confirm(true); // root npm install
        ui.add_select(0); // detected npm install
        ui.add_confirm(true); // root composer install
        ui.add_confirm(true); // apps/web pnpm install
        ui.add_select(4); // nvm use && npm install
        ui.add_confirm(false); // do not start detected dev command
        ui.add_select(0); // default editor
        ui.add_confirm(true); // create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Minimal),
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.setup.deps.len(), 3);
        assert_eq!(config.setup.deps[0].run, "npm install");
        assert!(config.setup.deps[0].working_dir.is_none());
        assert_eq!(config.setup.deps[1].run, "composer install");
        assert!(config.setup.deps[1].working_dir.is_none());
        assert_eq!(
            config.setup.deps[2].working_dir.as_deref(),
            Some("apps/web")
        );
        assert_eq!(
            config.setup.deps[2].run,
            "bash -lc 'source \"$HOME/.nvm/nvm.sh\" && nvm use && npm install'"
        );
        assert!(content.contains("working_dir = \"apps/web\""));
    }

    #[test]
    fn init_interactive_setup_deps_detects_uv_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/pyproject.toml"),
            "[project]\nname = \"api\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("api/uv.lock"), "").unwrap();

        let mut ui = MockUi::new();
        ui.add_select(1); // customize frequently used settings
        ui.add_select(0); // default worktree path
        ui.add_input("lazygit, nvim");
        ui.add_confirm(true); // api uv sync
        ui.add_select(0); // default editor
        ui.add_confirm(true); // create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                preset: Some(InitPreset::Minimal),
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].working_dir.as_deref(), Some("api"));
        assert_eq!(config.setup.deps[0].run, "uv sync");
        assert!(config.setup.deps[0].if_exists.is_none());
    }

    #[test]
    fn init_interactive_agent_args_options_do_not_include_default() {
        struct CapturingUi {
            selects: Mutex<VecDeque<usize>>,
            confirms: Mutex<VecDeque<bool>>,
            agent_args_items: Arc<Mutex<Option<Vec<String>>>>,
        }

        impl UserInterface for CapturingUi {
            fn select(&self, prompt: &str, items: &[String]) -> Result<usize> {
                if prompt == "Agent args" {
                    *self.agent_args_items.lock().unwrap() = Some(items.to_vec());
                }
                self.selects
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("no select response"))
            }

            fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
                unreachable!()
            }

            fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
                self.confirms
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or_else(|| anyhow::anyhow!("no confirm response"))
            }

            fn input(&self, _prompt: &str, _default: Option<&str>) -> Result<String> {
                unreachable!()
            }

            fn print_step(&self, _msg: &str) {}

            fn print_dim(&self, _msg: &str) {}

            fn print_warning(&self, _msg: &str) {}

            fn print_error(&self, _msg: &str) {}
        }

        let dir = tempfile::tempdir().unwrap();
        let agent_args_items = Arc::new(Mutex::new(None));
        let ui = CapturingUi {
            selects: Mutex::new(VecDeque::from([0, 1, 0, 0, 0])),
            confirms: Mutex::new(VecDeque::from([true])),
            agent_args_items: Arc::clone(&agent_args_items),
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            agent_args_items.lock().unwrap().as_ref().unwrap(),
            &vec!["none".to_string(), "enter args".to_string()]
        );
    }

    #[test]
    fn init_interactive_codex_none_agent_args_omits_args() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(1); // agent preset
        ui.add_select(0); // codex
        ui.add_select(0); // no agent args
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(true); // create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(agent.args.is_empty());
        assert!(!content.contains("args ="));
    }

    #[test]
    fn init_agent_command_overrides_selected_cli() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: Some("sandvault run -- codex --yolo".into()),
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(
            agent.command.as_deref(),
            Some("sandvault run -- codex --yolo")
        );
    }

    #[test]
    fn init_none_agent_rejects_command_override() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: Some("codex".into()),
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--agent-command cannot be used when --agent none")
        );
    }

    #[test]
    fn init_shared_accepts_agent_runtime_in_selected_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: true,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.profile.unwrap().agent.unwrap().cli, AgentCli::Codex);
        assert!(!dir.path().join(".local/.wt.toml").exists());
    }

    #[test]
    fn init_none_agent_rejects_agent_args() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: vec!["--model".into()],
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--agent-arg cannot be used when --agent none")
        );
    }

    #[test]
    fn init_interactive_flow_respects_create_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(0); // minimal preset
        ui.add_select(0); // minimal additional settings
        ui.add_confirm(false); // do not create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        let result = run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        );

        assert!(result.is_err());
        assert!(!dir.path().join(".local/.wt.toml").exists());
    }

    #[test]
    fn init_refuses_existing_file_without_force_and_overwrites_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(".local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join(".wt.toml"), "[workspace]\ntabs = []\n").unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let options = InitOptions {
            local: true,
            shared: false,
            agent: Some(InitAgent::Claude),
            agent_args: Vec::new(),
            agent_command: None,
            issue_provider: None,
            site_provider: None,
            gh_user: None,
            yes: true,
            force: false,
            ..InitOptions::default()
        };
        assert!(run(&ctx, options).is_err());

        run(
            &ctx,
            InitOptions {
                force: true,
                ..InitOptions {
                    local: true,
                    shared: false,
                    agent: Some(InitAgent::Claude),
                    agent_args: Vec::new(),
                    agent_command: None,
                    issue_provider: None,
                    site_provider: None,
                    gh_user: None,
                    yes: true,
                    force: false,
                    ..InitOptions::default()
                }
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(local.join(".wt.toml")).unwrap();
        let config = toml::from_str::<Config>(&content).unwrap();
        assert_eq!(config.profile.unwrap().agent.unwrap().cli, AgentCli::Claude);
        assert!(
            !dir.path()
                .join(".local/profiles/claude/profile.toml")
                .exists()
        );
    }
}
