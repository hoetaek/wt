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

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct WorkspaceConfig {
    pub command: String,
    pub tabs: Vec<String>,
    pub post_deps_tabs: Vec<String>,
    pub colors: HashMap<String, String>,
    pub post_ready: Option<PostReadyConfig>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PostReadyConfig {
    pub wait_for: String,
    pub send: HashMap<String, Vec<String>>,
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

[herd]
site_name = "{{repo}}-{{tech_id}}"
secure = true

[workspace]
command = "claudep"
tabs = ["lazygit"]
post_deps_tabs = ["npm run dev"]
colors = { issue = "Red", pr = "Green" }

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

        let herd = config.herd.unwrap();
        assert_eq!(herd.site_name, "{{repo}}-{{tech_id}}");
        assert_eq!(herd.secure, Some(true));

        let ws = config.workspace.unwrap();
        assert_eq!(ws.command, "claudep");
        assert_eq!(ws.tabs, vec!["lazygit"]);
        assert_eq!(ws.post_deps_tabs, vec!["npm run dev"]);
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
    fn parses_post_ready_config() {
        let toml_str = r#"
[workspace]
command = "bash"
tabs = []

[workspace.post_ready]
wait_for = "❯"
timeout = 10

[workspace.post_ready.send]
issue = ["/start\n"]
pr = ["/conventional-review {{pr_number}}\n", "/codex:review --background\n"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let post = config.workspace.unwrap().post_ready.unwrap();
        assert_eq!(post.wait_for, "❯");
        assert_eq!(post.send.get("issue").unwrap(), &vec!["/start\n"]);
        assert_eq!(post.send.get("pr").unwrap().len(), 2);
        assert_eq!(
            post.send.get("pr").unwrap()[1],
            "/codex:review --background\n"
        );
        assert!(!post.send.contains_key("new"));
        assert_eq!(post.timeout, Some(10));
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
gh_user = "hoetaek"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let issues = config.issues.unwrap();
        assert_eq!(issues.provider, IssueProviderType::Github);
        assert_eq!(issues.gh_user.as_deref(), Some("hoetaek"));
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
