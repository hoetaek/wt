use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    pub worktree: WorktreeConfig,
    pub setup: SetupConfig,
    pub herd: Option<HerdConfig>,
    pub workspace: Option<WorkspaceConfig>,
    pub agent: Option<AgentConfig>,
    pub test: Option<TestConfig>,
    pub issues: Option<IssuesConfig>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WorktreeConfig {
    pub copy: Vec<String>,
    pub copy_as: Vec<CopyAsEntry>,
    pub link: Vec<String>,
    pub claude_local_context: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct CopyAsEntry {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct SetupConfig {
    pub deps: Vec<DepCommand>,
    pub env: HashMap<String, String>,
    pub env_files: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct DepCommand {
    pub run: String,
    pub if_exists: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct HerdConfig {
    pub site_name: String,
    pub secure: Option<bool>,
    pub open_browser: Option<bool>,
    pub browser: Option<String>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub tabs: Vec<String>,
    pub post_deps_tabs: Vec<String>,
    pub colors: HashMap<String, String>,
    pub open_url: Option<String>,
    pub open_browser: Option<bool>,
    pub browser: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct AgentConfig {
    pub cli: AgentCli,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default = "default_agent_ready")]
    pub ready: ReadyMode,
    #[serde(default = "default_agent_submit")]
    pub submit: SubmitMode,
    #[serde(default = "default_agent_timeout")]
    pub timeout: u64,
    #[serde(default = "default_agent_send_after")]
    pub send_after: u64,
    #[serde(default)]
    pub prompt: HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AgentCli {
    Codex,
    Claude,
    Gemini,
    Custom,
    None,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ReadyMode {
    Auto,
    Marker(String),
}

impl<'de> Deserialize<'de> for ReadyMode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == "auto" {
            Ok(ReadyMode::Auto)
        } else {
            Ok(ReadyMode::Marker(value))
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SubmitMode {
    Auto,
    Newline,
    CarriageReturn,
    None,
}

fn default_agent_ready() -> ReadyMode {
    ReadyMode::Auto
}

fn default_agent_submit() -> SubmitMode {
    SubmitMode::Auto
}

fn default_agent_timeout() -> u64 {
    15
}

fn default_agent_send_after() -> u64 {
    3
}

impl AgentConfig {
    pub fn command_line(&self) -> anyhow::Result<Option<String>> {
        if self.cli == AgentCli::None {
            return Ok(None);
        }

        if let Some(command) = &self.command {
            return Ok(Some(command.clone()));
        }

        let base = match self.cli {
            AgentCli::Codex => "codex",
            AgentCli::Claude => "claude",
            AgentCli::Gemini => "gemini",
            AgentCli::Custom => anyhow::bail!("agent.command is required when agent.cli is custom"),
            AgentCli::None => unreachable!(),
        };

        if self.args.is_empty() {
            return Ok(Some(base.into()));
        }

        let args = self
            .args
            .iter()
            .map(|arg| shell_escape_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Some(format!("{base} {args}")))
    }

    pub fn effective_ready(&self) -> Option<String> {
        match &self.ready {
            ReadyMode::Marker(marker) => Some(marker.clone()),
            ReadyMode::Auto => match self.cli {
                AgentCli::Codex => Some("›".into()),
                AgentCli::Claude => Some("❯".into()),
                AgentCli::Gemini | AgentCli::Custom | AgentCli::None => None,
            },
        }
    }

    pub fn apply_submit_suffix(&self, mut prompt: String) -> String {
        if prompt.ends_with('\n') || prompt.ends_with('\r') {
            return prompt;
        }

        match self.submit {
            SubmitMode::Auto => match self.cli {
                AgentCli::Codex => prompt.push('\r'),
                AgentCli::Claude | AgentCli::Gemini | AgentCli::Custom => prompt.push('\n'),
                AgentCli::None => {}
            },
            SubmitMode::Newline => prompt.push('\n'),
            SubmitMode::CarriageReturn => prompt.push('\r'),
            SubmitMode::None => {}
        }
        prompt
    }
}

fn shell_escape_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".into();
    }

    if arg.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '@' | '+')
    }) {
        return arg.into();
    }

    format!("'{}'", arg.replace('\'', "'\\''"))
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct TestConfig {
    pub commands: Vec<TestCommand>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct TestCommand {
    pub run: String,
    pub if_exists: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct IssuesConfig {
    pub provider: IssueProviderType,
    pub gh_user: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IssueProviderType {
    Linear,
    Github,
}

impl Config {
    /// Load config with fallback: .local/.wt.toml → .wt.toml → default
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let local_path = repo_root.join(".local/.wt.toml");
        let root_path = repo_root.join(".wt.toml");

        let path = if local_path.exists() {
            local_path
        } else if root_path.exists() {
            root_path
        } else {
            return Ok(Config::default());
        };

        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Discover variant configs: .local/.wt.{name}.toml
    pub fn load_variants(repo_root: &Path) -> anyhow::Result<Vec<(String, Self)>> {
        let local_dir = repo_root.join(".local");
        if !local_dir.exists() {
            return Ok(Vec::new());
        }

        let re = regex::Regex::new(r"^\.wt\.(.+)\.toml$").unwrap();
        let mut variants = Vec::new();

        for entry in std::fs::read_dir(&local_dir)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if let Some(caps) = re.captures(&file_name) {
                let variant_name = caps[1].to_string();
                let content = std::fs::read_to_string(entry.path())?;
                let config: Config = toml::from_str(&content)?;
                variants.push((variant_name, config));
            }
        }

        variants.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(variants)
    }

    pub fn load_variant(repo_root: &Path, name: &str) -> anyhow::Result<Option<Self>> {
        let path = repo_root.join(".local").join(format!(".wt.{name}.toml"));
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path)?;
        Ok(Some(toml::from_str(&content)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let toml_str = r#"
[worktree]
copy = [".env", "CLAUDE.local.md", ".claude/settings.local.json"]
link = [".local"]
claude_local_context = "\n## env\n- parent: `{{parent_branch}}`\n"

[setup]
deps = [
    { run = "composer install", if_exists = "composer.json" },
    { run = "npm install", if_exists = "package.json" },
]

[setup.env]
APP_URL = "https://{{site_name}}.test"
APP_NAME = "{{issue_title}}"

[setup.env_files."frontend/.env.development"]
VITE_API_TARGET = "{{api_url}}"

[setup.env_files."backend/.env"]
DJANGO_ENV = "dev"

[herd]
site_name = "{{repo}}-{{tech_id}}"
secure = true

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
    { run = "./vendor/bin/pest", if_exists = "vendor/bin/pest", label = "PHP" },
]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(
            config.worktree.copy,
            vec![".env", "CLAUDE.local.md", ".claude/settings.local.json"]
        );
        assert_eq!(config.worktree.link, vec![".local"]);
        assert!(config.worktree.claude_local_context.is_some());
        assert!(
            config
                .worktree
                .claude_local_context
                .unwrap()
                .contains("{{parent_branch}}")
        );
        assert_eq!(config.setup.deps.len(), 2);
        assert_eq!(config.setup.deps[0].run, "composer install");
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

        let herd = config.herd.unwrap();
        assert_eq!(herd.site_name, "{{repo}}-{{tech_id}}");
        assert_eq!(herd.secure, Some(true));

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
        assert!(config.herd.is_none());
        assert!(config.workspace.is_none());
    }

    #[test]
    fn rejects_legacy_claude_copy_field() {
        let toml_str = r#"
[worktree]
copy = [".env"]
claude_copy = ["settings.local.json"]
"#;
        let err = toml::from_str::<Config>(toml_str).unwrap_err();
        assert!(err.to_string().contains("claude_copy"));
    }

    #[test]
    fn local_config_takes_precedence() {
        let dir = std::env::temp_dir().join("wt-test-local-precedence");
        std::fs::create_dir_all(dir.join(".local")).ok();

        // Root config
        std::fs::write(
            dir.join(".wt.toml"),
            r#"
[herd]
site_name = "root"
"#,
        )
        .unwrap();

        // Local config (should win)
        std::fs::write(
            dir.join(".local/.wt.toml"),
            r#"
[herd]
site_name = "local"
"#,
        )
        .unwrap();

        let config = Config::load(&dir).unwrap();
        assert_eq!(config.herd.unwrap().site_name, "local");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn falls_back_to_root_config() {
        let dir = std::env::temp_dir().join("wt-test-root-fallback");
        std::fs::create_dir_all(&dir).ok();

        std::fs::write(
            dir.join(".wt.toml"),
            r#"
[herd]
site_name = "root"
"#,
        )
        .unwrap();

        let config = Config::load(&dir).unwrap();
        assert_eq!(config.herd.unwrap().site_name, "root");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_variants_discovers_wt_variant_files() {
        let dir = tempfile::tempdir().unwrap();
        let local_dir = dir.path().join(".local");
        std::fs::create_dir_all(&local_dir).unwrap();

        std::fs::write(
            local_dir.join(".wt.baseline.toml"),
            "[worktree]\ncopy = [\".env\"]\n",
        )
        .unwrap();
        std::fs::write(
            local_dir.join(".wt.tdd.toml"),
            "[worktree]\ncopy = [\".env\", \"CLAUDE.local.md\"]\n",
        )
        .unwrap();
        std::fs::write(local_dir.join(".wt.toml"), "[worktree]\ncopy = []\n").unwrap();

        let variants = Config::load_variants(dir.path()).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].0, "baseline");
        assert_eq!(variants[1].0, "tdd");
        assert_eq!(variants[0].1.worktree.copy, vec![".env".to_string()]);
        assert_eq!(
            variants[1].1.worktree.copy,
            vec![".env".to_string(), "CLAUDE.local.md".to_string()]
        );
    }

    #[test]
    fn load_variants_returns_empty_when_no_local_dir() {
        let dir = tempfile::tempdir().unwrap();
        let variants = Config::load_variants(dir.path()).unwrap();
        assert!(variants.is_empty());
    }

    #[test]
    fn load_variants_returns_empty_when_no_variant_files() {
        let dir = tempfile::tempdir().unwrap();
        let local_dir = dir.path().join(".local");
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(local_dir.join(".wt.toml"), "[worktree]\n").unwrap();
        let variants = Config::load_variants(dir.path()).unwrap();
        assert!(variants.is_empty());
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
    fn parses_agent_ready_marker_submit_and_gemini_cli() {
        let toml_str = r#"
[agent]
cli = "gemini"
ready = "CUSTOM"
submit = "carriage_return"
timeout = 22
send_after = 4
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let agent = config.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Gemini);
        assert_eq!(agent.ready, ReadyMode::Marker("CUSTOM".into()));
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
    fn load_variant_returns_specific_config_or_none() {
        let dir = tempfile::tempdir().unwrap();
        let local_dir = dir.path().join(".local");
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(
            local_dir.join(".wt.codex.toml"),
            r#"
[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
"#,
        )
        .unwrap();

        let variant = Config::load_variant(dir.path(), "codex").unwrap().unwrap();
        assert_eq!(variant.agent.unwrap().cli, AgentCli::Codex);
        assert!(
            Config::load_variant(dir.path(), "missing")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_legacy_workspace_command_and_post_ready() {
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
issue = ["start 스킬을 사용해서 현재 GitHub 이슈를 읽고 작업 계획을 세운 뒤 바로 시작해줘.\n"]
pr = ["/conventional-review {{pr_number}}\n", "/codex:review --background\n"]
"#;
        let err = toml::from_str::<Config>(post_ready_toml).unwrap_err();
        assert!(err.to_string().contains("post_ready"));
    }

    #[test]
    fn parses_open_browser_config() {
        let toml_str = r#"
[herd]
site_name = "test"
open_browser = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let herd = config.herd.unwrap();
        assert_eq!(herd.open_browser, Some(true));
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
    fn issues_section_optional() {
        let toml_str = r#"
[worktree]
copy = [".env"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.issues.is_none());
    }
}
