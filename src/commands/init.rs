use crate::cli::{InitAgent, InitIssueProvider};
use crate::config::Config;
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
    pub gh_user: Option<String>,
    pub prompts: bool,
    pub no_prompts: bool,
    pub yes: bool,
    pub force: bool,
}

pub fn run(ctx: &Ctx, options: InitOptions) -> Result<()> {
    let target = resolve_target(ctx, &options)?;
    if target.exists() && !options.force {
        bail!(
            "Config already exists: {} (use --force to overwrite)",
            target.display()
        );
    }

    let content = build_config(ctx, &options)?;
    toml::from_str::<Config>(&content)?;

    if !options.yes && !ctx.ui.confirm("Create config?", true)? {
        return Err(WtError::Cancelled.into());
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, content)?;
    ctx.ui
        .print_step(&format!("Created config: {}", target.display()));
    Ok(())
}

fn resolve_target(ctx: &Ctx, options: &InitOptions) -> Result<PathBuf> {
    if options.local {
        return Ok(ctx.repo_root.join(".local/.wt.toml"));
    }
    if options.shared {
        return Ok(ctx.repo_root.join(".wt.toml"));
    }
    if options.yes {
        return Ok(ctx.repo_root.join(".local/.wt.toml"));
    }

    let items = vec![".local/.wt.toml".into(), ".wt.toml".into()];
    match ctx.ui.select("Where should config be created?", &items)? {
        0 => Ok(ctx.repo_root.join(".local/.wt.toml")),
        _ => Ok(ctx.repo_root.join(".wt.toml")),
    }
}

fn build_config(ctx: &Ctx, options: &InitOptions) -> Result<String> {
    let agent = resolve_agent(ctx, options)?;
    let command = resolve_agent_command(ctx, &agent, options)?;
    let args = resolve_agent_args(ctx, &agent, options)?;
    let issue_provider = resolve_issue_provider(ctx, options)?;
    let include_prompts = if agent == InitAgent::None {
        false
    } else {
        resolve_prompts(ctx, options)?
    };

    let mut s = String::new();

    if let Some(provider) = issue_provider {
        s.push_str("[issues]\n");
        s.push_str(&format!(
            "provider = {}\n",
            toml_quote(issue_provider_name(&provider))
        ));
        if provider == InitIssueProvider::Github {
            if let Some(user) = resolve_gh_user(ctx, options)? {
                s.push_str(&format!("gh_user = {}\n", toml_quote(&user)));
            }
        }
        s.push('\n');
    }

    s.push_str("[workspace]\n");
    s.push_str("tabs = [\"lazygit\", \"nvim\"]\n\n");

    s.push_str("[agent]\n");
    s.push_str(&format!("cli = {}\n", toml_quote(agent_name(&agent))));
    if !args.is_empty() {
        s.push_str(&format!("args = {}\n", toml_array(&args)));
    }
    if let Some(command) = command.as_deref() {
        s.push_str(&format!("command = {}\n", toml_quote(command)));
    }

    if include_prompts && agent != InitAgent::None {
        s.push_str("\n[agent.prompt]\n");
        s.push_str(&format!(
            "issue = [{}]\n",
            toml_quote(
                "start 스킬을 사용해서 현재 GitHub 이슈를 읽고 작업 계획을 세운 뒤 바로 시작해줘."
            )
        ));
        s.push_str(&format!(
            "new = [{}]\n",
            toml_quote("start 스킬을 사용해서 현재 작업 컨텍스트를 확인하고 작업 계획을 세운 뒤 바로 시작해줘.")
        ));
    }

    Ok(s)
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
        "custom".into(),
    ];
    let agent = match ctx.ui.select("Select coding agent", &items)? {
        0 => InitAgent::Codex,
        1 => InitAgent::Claude,
        2 => InitAgent::Gemini,
        3 => InitAgent::None,
        _ => InitAgent::Custom,
    };
    Ok(agent)
}

fn resolve_agent_command(
    ctx: &Ctx,
    agent: &InitAgent,
    options: &InitOptions,
) -> Result<Option<String>> {
    if let Some(command) = &options.agent_command {
        return Ok(Some(command.clone()));
    }
    if *agent != InitAgent::Custom {
        return Ok(None);
    }
    if options.yes {
        bail!("--agent-command is required when --agent custom");
    }

    let command = ctx.ui.input("Agent command", None)?;
    if command.trim().is_empty() {
        bail!("agent command cannot be empty");
    }
    Ok(Some(command))
}

fn resolve_agent_args(ctx: &Ctx, agent: &InitAgent, options: &InitOptions) -> Result<Vec<String>> {
    if !options.agent_args.is_empty() {
        return Ok(options.agent_args.clone());
    }
    if options.yes {
        return Ok(default_agent_args(agent));
    }
    if *agent == InitAgent::None {
        return Ok(Vec::new());
    }

    let items = vec!["default".into(), "none".into(), "custom".into()];
    match ctx.ui.select("Agent args", &items)? {
        0 => Ok(default_agent_args(agent)),
        1 => Ok(Vec::new()),
        _ => {
            let input = ctx.ui.input("Agent args", None)?;
            Ok(input
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>())
        }
    }
}

fn default_agent_args(agent: &InitAgent) -> Vec<String> {
    match agent {
        InitAgent::Codex
        | InitAgent::Claude
        | InitAgent::Gemini
        | InitAgent::Custom
        | InitAgent::None => Vec::new(),
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
    if options.prompts || options.yes {
        return Ok(true);
    }
    ctx.ui.confirm("Add default start prompts?", true)
}

fn agent_name(agent: &InitAgent) -> &'static str {
    match agent {
        InitAgent::Codex => "codex",
        InitAgent::Claude => "claude",
        InitAgent::Gemini => "gemini",
        InitAgent::Custom => "custom",
        InitAgent::None => "none",
    }
}

fn issue_provider_name(provider: &InitIssueProvider) -> &'static str {
    match provider {
        InitIssueProvider::Github => "github",
        InitIssueProvider::Linear => "linear",
        InitIssueProvider::None => "none",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentCli, IssueProviderType};
    use crate::context::mock::{MockRunner, MockUi};

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
        let agent = config.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(agent.args.is_empty());
        assert!(agent.prompt.contains_key("issue"));
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
        assert_eq!(config.agent.unwrap().cli, AgentCli::Gemini);
        let issues = config.issues.unwrap();
        assert_eq!(issues.provider, IssueProviderType::Github);
        assert_eq!(issues.gh_user.as_deref(), Some("alice"));
    }

    #[test]
    fn init_github_without_user_omits_personal_gh_user() {
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
    fn init_interactive_flow_uses_ui_answers() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1); // .wt.toml
        ui.add_select(1); // claude
        ui.add_select(0); // default agent args
        ui.add_select(2); // no issue provider
        ui.add_confirm(false); // no default prompts
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
        let agent = config.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Claude);
        assert!(agent.prompt.is_empty());
        assert!(config.issues.is_none());
    }

    #[test]
    fn init_interactive_flow_accepts_custom_agent_args() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(0); // codex
        ui.add_select(2); // custom agent args
        ui.add_input("--model gpt-5.5");
        ui.add_select(2); // no issue provider
        ui.add_confirm(false); // no default prompts
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
        let agent = config.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(agent.args, vec!["--model", "gpt-5.5"]);
    }

    #[test]
    fn init_interactive_flow_accepts_custom_agent_command() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(4); // custom
        ui.add_input("my-agent --flag");
        ui.add_select(0); // default agent args
        ui.add_select(2); // no issue provider
        ui.add_confirm(false); // no default prompts
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
        let agent = config.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Custom);
        assert_eq!(agent.command.as_deref(), Some("my-agent --flag"));
    }

    #[test]
    fn init_interactive_flow_respects_create_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .local/.wt.toml
        ui.add_select(3); // none
        ui.add_select(2); // no issue provider
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
        assert_eq!(
            toml::from_str::<Config>(&content)
                .unwrap()
                .agent
                .unwrap()
                .cli,
            AgentCli::Claude
        );
    }

    #[test]
    fn custom_agent_requires_command() {
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
                agent: Some(InitAgent::Custom),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                gh_user: None,
                prompts: false,
                no_prompts: false,
                yes: true,
                force: false,
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--agent-command"));
    }
}
