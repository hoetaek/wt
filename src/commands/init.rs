use crate::cli::{InitAgent, InitIssueProvider, InitSiteProvider};
use crate::commands::profile::{ProfileCreateOptions, create_profile};
use crate::config::{AgentCli, AgentConfig, Config, ReadyMode, SubmitMode};
use crate::context::Ctx;
use crate::error::WtError;
use anyhow::{Result, bail};
use std::path::PathBuf;

#[derive(Debug)]
pub struct InitOptions {
    pub local: bool,
    pub shared: bool,
    pub agent: Option<InitAgent>,
    pub agent_args: Vec<String>,
    pub agent_command: Option<String>,
    pub issue_provider: Option<InitIssueProvider>,
    pub site_provider: Option<InitSiteProvider>,
    pub gh_user: Option<String>,
    pub prompts: bool,
    pub no_prompts: bool,
    pub yes: bool,
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
    name: String,
    agent: AgentConfig,
    include_prompts: bool,
}

#[derive(Debug)]
struct InitPlan {
    content: String,
    config_for_profile: Config,
    profile: Option<InitProfile>,
}

pub fn run(ctx: &Ctx, options: InitOptions) -> Result<()> {
    let target = resolve_target(ctx, &options)?;
    if target.path.exists() && !options.force {
        bail!(
            "Config already exists: {} (use --force to overwrite)",
            target.path.display()
        );
    }

    let plan = build_plan(ctx, &options, target.kind)?;

    if target.kind == InitTargetKind::Shared {
        if let Some(profile) = &plan.profile {
            validate_local_profile_update(ctx, profile, options.force)?;
        }
    }

    if !options.yes && !ctx.ui.confirm("Create config?", true)? {
        return Err(WtError::Cancelled.into());
    }

    if let Some(parent) = target.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target.path, &plan.content)?;
    ctx.ui
        .print_step(&format!("Created config: {}", target.path.display()));

    if let Some(profile) = &plan.profile {
        if target.kind == InitTargetKind::Shared {
            write_local_profile(ctx, profile, options.force)?;
        }

        if profile.include_prompts {
            let profile_toml = ctx
                .repo_root
                .join(".local/profiles")
                .join(&profile.name)
                .join("profile.toml");
            if profile_toml.exists() {
                ctx.ui.print_step(&format!(
                    "Profile '{}' already exists: {}",
                    profile.name,
                    profile_toml.display()
                ));
            } else {
                let created = create_profile(
                    ctx,
                    ProfileCreateOptions {
                        name: &profile.name,
                        base_config: &plan.config_for_profile,
                        agent: Some(profile.agent.clone()),
                        include_prompts: true,
                    },
                )?;
                ctx.ui.print_step(&format!(
                    "Created profile '{}': {}",
                    created.name,
                    created.config_path.display()
                ));
            }
        }
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

fn build_plan(ctx: &Ctx, options: &InitOptions, target_kind: InitTargetKind) -> Result<InitPlan> {
    let agent = resolve_agent(ctx, options)?;
    let command = resolve_agent_command(options)?;
    if agent == InitAgent::None && command.is_some() {
        bail!("--agent-command cannot be used when --agent none");
    }
    let args = resolve_agent_args(ctx, &agent, options)?;
    let issue_provider = resolve_issue_provider(ctx, options)?;
    let gh_user = if issue_provider == Some(InitIssueProvider::Github) {
        resolve_gh_user(ctx, options)?
    } else {
        None
    };
    let site_provider = resolve_site_provider(ctx, options)?;
    if agent == InitAgent::None && options.prompts {
        bail!("--prompts cannot be used when --agent none");
    }
    let include_prompts = if agent == InitAgent::None {
        false
    } else {
        resolve_prompts(ctx, options)?
    };
    let profile = build_profile(&agent, args, command, include_prompts);

    let mut s = String::new();

    if let Some(provider) = issue_provider {
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

    if let (InitTargetKind::Local, Some(profile)) = (target_kind, &profile) {
        append_profile_selection(&mut s, profile);
    }

    append_optional_scaffold(&mut s);

    s.push_str("[workspace]\n");
    s.push_str("tabs = [\"lazygit\", \"nvim\"]\n");
    s.push_str("# post_deps_tabs = [\"npm run dev\"]\n");
    s.push_str("# open_url = \"{{site_url}}\"\n");
    s.push_str("# open_browser = true\n\n");

    let mut config_for_profile = toml::from_str::<Config>(&s)?;
    config_for_profile.profile = None;

    Ok(InitPlan {
        content: s,
        config_for_profile,
        profile,
    })
}

fn append_optional_scaffold(s: &mut String) {
    s.push_str("# Optional worktree behavior. Uncomment and adjust as needed.\n");
    s.push_str("# [worktree]\n");
    s.push_str("# path = \"$HOME/worktrees/{{default_name}}\"\n");
    s.push_str("# copy = [\".env\"]\n");
    s.push_str("# link = [\".local\"]\n\n");

    s.push_str("# Optional AI-assisted naming for issue worktrees.\n");
    s.push_str("# [worktree.naming]\n");
    s.push_str("# command = \"claude -p\"\n");
    s.push_str("# branch = \"{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}\"\n");
    s.push_str("# workspace = \"{{english_title}}\"\n\n");

    s.push_str("# Optional setup commands run inside each worktree.\n");
    s.push_str("# [setup]\n");
    s.push_str("# deps = [\n");
    s.push_str("#     { run = \"npm install\", if_exists = \"package.json\" },\n");
    s.push_str("# ]\n\n");
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

fn resolve_issue_provider(ctx: &Ctx, options: &InitOptions) -> Result<Option<InitIssueProvider>> {
    if let Some(provider) = &options.issue_provider {
        return Ok(match provider {
            InitIssueProvider::Github => Some(InitIssueProvider::Github),
            InitIssueProvider::Linear => Some(InitIssueProvider::Linear),
            InitIssueProvider::None => None,
        });
    }
    if options.yes {
        return Ok(None);
    }

    let items = vec!["github".into(), "linear".into(), "none".into()];
    Ok(match ctx.ui.select("Issue provider", &items)? {
        0 => Some(InitIssueProvider::Github),
        1 => Some(InitIssueProvider::Linear),
        _ => None,
    })
}

fn resolve_site_provider(ctx: &Ctx, options: &InitOptions) -> Result<Option<InitSiteProvider>> {
    if let Some(provider) = &options.site_provider {
        return Ok(match provider {
            InitSiteProvider::None => None,
            InitSiteProvider::Herd => Some(InitSiteProvider::Herd),
            InitSiteProvider::Valet => Some(InitSiteProvider::Valet),
            InitSiteProvider::DockerProxy => Some(InitSiteProvider::DockerProxy),
            InitSiteProvider::Traefik => Some(InitSiteProvider::Traefik),
        });
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

fn resolve_prompts(ctx: &Ctx, options: &InitOptions) -> Result<bool> {
    if options.no_prompts {
        return Ok(false);
    }
    if options.prompts {
        return Ok(true);
    }
    if options.yes {
        return Ok(false);
    }
    ctx.ui.confirm("Create named profile with prompts?", false)
}

fn build_profile(
    agent: &InitAgent,
    args: Vec<String>,
    command: Option<String>,
    include_prompts: bool,
) -> Option<InitProfile> {
    if *agent == InitAgent::None {
        return None;
    }

    let name = agent_name(agent).to_string();
    Some(InitProfile {
        name,
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
        include_prompts,
    })
}

fn append_profile_selection(s: &mut String, profile: &InitProfile) {
    if profile.include_prompts {
        s.push_str("[profile]\n");
        s.push_str(&format!("name = {}\n\n", toml_quote(&profile.name)));
    } else {
        append_inline_agent_section(s, &profile.agent);
        s.push('\n');
    }
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

fn agent_name(agent: &InitAgent) -> &'static str {
    match agent {
        InitAgent::Codex => "codex",
        InitAgent::Claude => "claude",
        InitAgent::Gemini => "gemini",
        InitAgent::None => "none",
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

fn validate_local_profile_update(ctx: &Ctx, profile: &InitProfile, force: bool) -> Result<()> {
    let path = ctx.repo_root.join(".local/.wt.toml");
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;
    set_local_profile_content(&content, profile, force)?;
    Ok(())
}

fn write_local_profile(ctx: &Ctx, profile: &InitProfile, force: bool) -> Result<()> {
    let path = ctx.repo_root.join(".local/.wt.toml");
    let content = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let updated = set_local_profile_content(&content, profile, force)?;
    if updated != content {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, updated)?;
        ctx.ui
            .print_step(&format!("Updated local profile config: {}", path.display()));
    }
    Ok(())
}

fn set_local_profile_content(content: &str, profile: &InitProfile, force: bool) -> Result<String> {
    if !content.trim().is_empty() {
        let config: Config = toml::from_str(content)?;
        if let Some(existing_profile) = config.profile.as_ref() {
            if profile.include_prompts && existing_profile.name.as_deref() == Some(&profile.name) {
                return Ok(content.to_string());
            }
            if !force {
                bail!("Local profile is already configured");
            }
        }
    }

    let mut updated = remove_profile_sections(content);
    updated = updated.trim_end().to_string();
    if !updated.is_empty() {
        updated.push_str("\n\n");
    }
    append_profile_selection(&mut updated, profile);

    toml::from_str::<Config>(&updated)?;
    Ok(updated)
}

fn remove_profile_sections(content: &str) -> String {
    let mut lines = Vec::new();
    let mut skipping_profile = false;

    for line in content.lines() {
        if let Some(header) = table_header(line) {
            skipping_profile = header == "profile" || header.starts_with("profile.");
            if skipping_profile {
                continue;
            }
        }

        if !skipping_profile {
            lines.push(line.to_string());
        }
    }

    lines.join("\n")
}

fn table_header(line: &str) -> Option<&str> {
    let trimmed = line
        .trim()
        .split_once('#')
        .map_or_else(|| line.trim(), |(before, _)| before.trim_end());
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let header = trimmed.trim_start_matches('[').trim_end_matches(']');
    (!header.starts_with('[') && !header.ends_with(']')).then_some(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentCli, IssueProviderType, SiteProvider};
    use crate::context::UserInterface;
    use crate::context::mock::{MockRunner, MockUi};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    fn read_profile_config(root: &Path, name: &str) -> Config {
        let content =
            std::fs::read_to_string(root.join(".local/profiles").join(name).join("profile.toml"))
                .unwrap();
        toml::from_str(&content).unwrap()
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
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                prompts: false,
                no_prompts: false,
                yes: true,
                force: false,
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(content.contains("# path = \"$HOME/worktrees/{{default_name}}\""));
        assert!(content.contains("# [worktree.naming]"));
        assert!(content.contains("# post_deps_tabs = [\"npm run dev\"]"));
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
    fn init_shared_gemini_with_github_options() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
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
                agent: Some(InitAgent::Gemini),
                agent_args: vec!["--model=gemini-pro".into()],
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: Some(InitSiteProvider::None),
                gh_user: Some("alice".into()),
                prompts: true,
                no_prompts: false,
                yes: false,
                force: false,
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

        let local_content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let local_config: Config = toml::from_str(&local_content).unwrap();
        assert_eq!(
            local_config.profile.unwrap().name.as_deref(),
            Some("gemini")
        );
        let profile = read_profile_config(dir.path(), "gemini");
        let agent = profile.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Gemini);
        assert_eq!(agent.args, vec!["--model=gemini-pro"]);
        assert!(
            dir.path()
                .join(".local/profiles/gemini/prompts/issue.md")
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
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: None,
                gh_user: None,
                prompts: false,
                no_prompts: false,
                yes: true,
                force: false,
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
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::Valet),
                gh_user: None,
                prompts: false,
                no_prompts: true,
                yes: false,
                force: false,
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
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::Traefik),
                gh_user: None,
                prompts: false,
                no_prompts: true,
                yes: false,
                force: false,
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
        ui.add_select(1); // claude
        ui.add_select(0); // no agent args
        ui.add_select(2); // no issue provider
        ui.add_select(0); // no site provider
        ui.add_confirm(false); // keep profile inline
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
                prompts: false,
                no_prompts: false,
                yes: false,
                force: false,
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(config.agent.is_none());
        assert!(config.profile.is_none());
        let local_content = std::fs::read_to_string(dir.path().join(".local/.wt.toml")).unwrap();
        let local_config: Config = toml::from_str(&local_content).unwrap();
        let agent = local_config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Claude);
        assert!(agent.prompt.is_empty());
        assert!(
            !dir.path()
                .join(".local/profiles/claude/profile.toml")
                .exists()
        );
        assert!(config.issues.is_none());
    }

    #[test]
    fn init_interactive_flow_accepts_manual_agent_args() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(0); // codex
        ui.add_select(1); // enter agent args
        ui.add_input("--model gpt-5.5");
        ui.add_select(2); // no issue provider
        ui.add_select(0); // no site provider
        ui.add_confirm(false); // keep profile inline
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
                prompts: false,
                no_prompts: false,
                yes: false,
                force: false,
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
            selects: Mutex::new(VecDeque::from([0, 0, 0, 2, 0])),
            confirms: Mutex::new(VecDeque::from([false, true])),
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
                prompts: false,
                no_prompts: false,
                yes: false,
                force: false,
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
        ui.add_select(0); // codex
        ui.add_select(0); // no agent args
        ui.add_select(2); // no issue provider
        ui.add_select(0); // no site provider
        ui.add_confirm(false); // keep profile inline
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
                prompts: false,
                no_prompts: false,
                yes: false,
                force: false,
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
                prompts: false,
                no_prompts: true,
                yes: true,
                force: false,
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
                prompts: false,
                no_prompts: true,
                yes: true,
                force: false,
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
    fn set_local_profile_content_adds_inline_profile_section() {
        let profile = build_profile(&InitAgent::Codex, Vec::new(), None, false).unwrap();
        let updated =
            set_local_profile_content("[workspace]\ntabs = []\n", &profile, false).unwrap();
        let config: Config = toml::from_str(&updated).unwrap();
        assert_eq!(config.profile.unwrap().agent.unwrap().cli, AgentCli::Codex);
        assert!(updated.contains("[workspace]\ntabs = []"));
        assert!(updated.contains("[profile.agent]\ncli = \"codex\""));
    }

    #[test]
    fn set_local_profile_content_rejects_conflict_without_force() {
        let profile = build_profile(&InitAgent::Codex, Vec::new(), None, true).unwrap();
        let result = set_local_profile_content("[profile]\nname = \"claude\"\n", &profile, false);
        assert!(result.is_err());

        let updated =
            set_local_profile_content("[profile]\nname = \"claude\"\n", &profile, true).unwrap();
        let config: Config = toml::from_str(&updated).unwrap();
        assert_eq!(config.profile.unwrap().name.as_deref(), Some("codex"));
    }

    #[test]
    fn init_interactive_flow_respects_create_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(3); // none
        ui.add_select(2); // no issue provider
        ui.add_select(0); // no site provider
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
                prompts: false,
                no_prompts: false,
                yes: false,
                force: false,
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
            prompts: false,
            no_prompts: false,
            yes: true,
            force: false,
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
                    prompts: false,
                    no_prompts: false,
                    yes: true,
                    force: false,
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
