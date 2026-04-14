use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    pub worktree: WorktreeConfig,
    pub setup: SetupConfig,
    pub herd: Option<HerdConfig>,
    pub workspace: Option<WorkspaceConfig>,
    pub test: Option<TestConfig>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct WorktreeConfig {
    pub copy: Vec<String>,
    pub link: Vec<String>,
    pub claude_copy: Vec<String>,
}

#[derive(Debug, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct SetupConfig {
    pub deps: Vec<DepCommand>,
    pub env: HashMap<String, String>,
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
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct WorkspaceConfig {
    pub command: String,
    pub tabs: Vec<String>,
    pub colors: HashMap<String, String>,
    pub post_ready: Option<PostReadyConfig>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PostReadyConfig {
    pub wait_for: String,
    pub send: String,
    pub surface: Option<String>,
    pub timeout: Option<u64>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let toml_str = r#"
[worktree]
copy = [".env", "CLAUDE.local.md"]
link = [".local"]
claude_copy = ["settings.local.json", "hooks"]

[setup]
deps = [
    { run = "composer install", if_exists = "composer.json" },
    { run = "npm install", if_exists = "package.json" },
]

[setup.env]
APP_URL = "https://{{site_name}}.test"
APP_NAME = "{{issue_title}}"

[herd]
site_name = "{{repo}}-{{tech_id}}"
secure = true

[workspace]
command = "claudep"
tabs = ["lazygit", "yazi"]
colors = { issue = "Red", pr = "Green" }

[test]
commands = [
    { run = "./vendor/bin/pest", if_exists = "vendor/bin/pest", label = "PHP" },
]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.worktree.copy, vec![".env", "CLAUDE.local.md"]);
        assert_eq!(config.worktree.link, vec![".local"]);
        assert_eq!(config.setup.deps.len(), 2);
        assert_eq!(config.setup.deps[0].run, "composer install");
        assert_eq!(
            config.setup.env.get("APP_URL").unwrap(),
            "https://{{site_name}}.test"
        );

        let herd = config.herd.unwrap();
        assert_eq!(herd.site_name, "{{repo}}-{{tech_id}}");
        assert_eq!(herd.secure, Some(true));

        let ws = config.workspace.unwrap();
        assert_eq!(ws.command, "claudep");
        assert_eq!(ws.tabs, vec!["lazygit", "yazi"]);
        assert_eq!(ws.colors.get("issue").unwrap(), "Red");

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
}
