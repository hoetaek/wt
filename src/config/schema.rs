use anyhow::bail;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use std::borrow::Cow;
use std::collections::HashMap;

pub const RESERVED_PROFILE_NAME: &str = "default";
pub const AGENT_PROMPT_WORKFLOW_SCOPE: &str = "workflow";
const PROMPT_APPEND_PREFIX: &str = "\u{0}append:";
pub(super) const PROMPT_COMMON_SCOPE: &str = "common";
pub(super) const PROMPT_RUNTIME_MODES: [&str; 3] = ["issue", "branch", "pr"];
pub const WORKSPACE_COLOR_KIND_ISSUE: &str = "issue";
pub const WORKSPACE_COLOR_KIND_BRANCH: &str = "branch";
pub const WORKSPACE_COLOR_KIND_PR: &str = "pr";
pub const WORKSPACE_COLOR_KIND_TASK: &str = "task";
pub const WORKSPACE_DEFAULT_COLORS: [(&str, &str); 4] = [
    (WORKSPACE_COLOR_KIND_TASK, "blue"),
    (WORKSPACE_COLOR_KIND_ISSUE, "blue"),
    (WORKSPACE_COLOR_KIND_BRANCH, "green"),
    (WORKSPACE_COLOR_KIND_PR, "magenta"),
];
const DEFAULT_SITE_NAME_TEMPLATE: &str = "{{repo}}-{{branch_slug}}";
const DEFAULT_SITE_ROOT: &str = ".";
const DEFAULT_TRAEFIK_SITE_TARGET_TEMPLATE: &str = "http://127.0.0.1:{{vite_port}}";
const DEFAULT_CHROME_DEVTOOLS_USER_DATA_DIR: &str =
    "{{worktree_parent}}/.chrome-devtools/{{worktree_name}}";
const DEFAULT_WORKSPACE_BROWSER_URL: &str = "{{site_url}}";

pub fn default_workspace_color(kind: &str) -> Option<&'static str> {
    WORKSPACE_DEFAULT_COLORS
        .iter()
        .find_map(|(default_kind, color)| (*default_kind == kind).then_some(*color))
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub worktree: WorktreeConfig,
    pub setup: SetupConfig,
    pub workflow: WorkflowConfig,
    pub review: ReviewConfig,
    pub profile: Option<ProfileConfig>,
    pub site: Option<SiteConfig>,
    pub editor: EditorConfig,
    pub workspace: Option<WorkspaceConfig>,
    pub agent: Option<AgentConfig>,
    pub issues: Option<IssuesConfig>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkflowConfig {
    pub pull_request: Option<WorkflowDefaultPullRequestMode>,
    pub landing: Option<WorkflowDefaultLandingPolicy>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewConfig {
    pub codex_base: Option<ReviewCodexBasePolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowDefaultPullRequestMode {
    None,
    Draft,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowDefaultLandingPolicy {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowDefaultPolicy {
    pub pull_request: WorkflowDefaultPullRequestMode,
    pub landing: WorkflowDefaultLandingPolicy,
    pub review: ReviewDefaultPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCodexBasePolicy {
    None,
    Advisory,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewDefaultPolicy {
    pub codex_base: ReviewCodexBasePolicy,
}

impl<'de> Deserialize<'de> for WorkflowConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = HashMap::<String, toml::Value>::deserialize(deserializer)?;
        let mut config = WorkflowConfig::default();

        for (key, value) in raw {
            match key.as_str() {
                "pull_request" => {
                    config.pull_request = Some(parse_workflow_pull_request::<D::Error>(&value)?);
                }
                "landing" => {
                    config.landing = Some(parse_workflow_landing::<D::Error>(&value)?);
                }
                "defaults" => {
                    return Err(D::Error::custom(format!(
                        "{} is not supported; set [workflow].pull_request and [workflow].landing directly",
                        legacy_workflow_defaults_table()
                    )));
                }
                key if key == legacy_landing_approval_key() => {
                    return Err(D::Error::custom(format!(
                        "[workflow].{} is not supported; use [workflow].landing = \"manual\" or \"auto\"",
                        legacy_landing_approval_key()
                    )));
                }
                other => {
                    return Err(D::Error::custom(format!(
                        "unknown [workflow] field `{other}`; expected `pull_request` or `landing`"
                    )));
                }
            }
        }

        Ok(config)
    }
}

impl<'de> Deserialize<'de> for ReviewConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = HashMap::<String, toml::Value>::deserialize(deserializer)?;
        let mut config = ReviewConfig::default();

        for (key, value) in raw {
            match key.as_str() {
                "codex_base" => {
                    config.codex_base = Some(parse_review_codex_base::<D::Error>(&value)?);
                }
                other => {
                    return Err(D::Error::custom(format!(
                        "unknown [review] field `{other}`; expected `codex_base`"
                    )));
                }
            }
        }

        Ok(config)
    }
}

fn parse_workflow_pull_request<E>(
    value: &toml::Value,
) -> std::result::Result<WorkflowDefaultPullRequestMode, E>
where
    E: DeError,
{
    match value.as_str() {
        Some("none") => Ok(WorkflowDefaultPullRequestMode::None),
        Some("draft") => Ok(WorkflowDefaultPullRequestMode::Draft),
        Some("ready") => Ok(WorkflowDefaultPullRequestMode::Ready),
        Some(other) => Err(E::custom(format!(
            "[workflow].pull_request must be one of \"none\", \"draft\", or \"ready\"; `{other}` is not an alias"
        ))),
        None => Err(E::custom(format!(
            "[workflow].pull_request must be a string: \"none\", \"draft\", or \"ready\"; booleans are not aliases ({})",
            workflow_value_type(value)
        ))),
    }
}

fn parse_workflow_landing<E>(
    value: &toml::Value,
) -> std::result::Result<WorkflowDefaultLandingPolicy, E>
where
    E: DeError,
{
    match value.as_str() {
        Some("manual") => Ok(WorkflowDefaultLandingPolicy::Manual),
        Some("auto") => Ok(WorkflowDefaultLandingPolicy::Auto),
        Some(other) if other == legacy_review_landing_value() => Err(E::custom(format!(
            "[workflow].landing uses \"manual\" or \"auto\"; `{}` is not supported",
            legacy_review_landing_value()
        ))),
        Some(other) => Err(E::custom(format!(
            "[workflow].landing must be \"manual\" or \"auto\"; `{other}` is not supported"
        ))),
        None => Err(E::custom(format!(
            "[workflow].landing must be a string: \"manual\" or \"auto\" ({})",
            workflow_value_type(value)
        ))),
    }
}

fn parse_review_codex_base<E>(value: &toml::Value) -> std::result::Result<ReviewCodexBasePolicy, E>
where
    E: DeError,
{
    match value.as_str() {
        Some("none") => Ok(ReviewCodexBasePolicy::None),
        Some("advisory") => Ok(ReviewCodexBasePolicy::Advisory),
        Some("required") => Ok(ReviewCodexBasePolicy::Required),
        Some(other) => Err(E::custom(format!(
            "[review].codex_base must be one of \"none\", \"advisory\", or \"required\"; `{other}` is not supported"
        ))),
        None => Err(E::custom(format!(
            "[review].codex_base must be a string: \"none\", \"advisory\", or \"required\" ({})",
            workflow_value_type(value)
        ))),
    }
}

fn workflow_value_type(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

fn legacy_workflow_defaults_table() -> String {
    format!("[workflow.{}]", "defaults")
}

fn legacy_landing_approval_key() -> String {
    format!("landing_requires_{}", "approval")
}

fn legacy_review_landing_value() -> String {
    format!("after_{}", "review")
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WorktreeConfig {
    pub path: Option<String>,
    pub copy: Vec<String>,
    pub copy_as: Vec<CopyAsEntry>,
    pub link: Vec<String>,
    pub inject_local_context: Option<String>,
    pub naming: Option<WorktreeNamingConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CopyAsEntry {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SetupConfig {
    pub deps: Vec<DepCommand>,
    pub env: HashMap<String, String>,
    pub env_files: HashMap<String, HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DepCommand {
    pub working_dir: Option<String>,
    pub run: String,
    pub if_exists: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    pub provider: SiteProvider,
    pub name: Option<String>,
    pub root: Option<String>,
    pub secure: Option<bool>,
    pub url: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EditorConfig {
    pub command: Option<String>,
    pub placement: Option<EditorPlacement>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditorPlacement {
    CmuxSurface,
    Process,
}

impl EditorConfig {
    pub fn effective_placement(&self) -> EditorPlacement {
        self.placement
            .clone()
            .unwrap_or(EditorPlacement::CmuxSurface)
    }
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            provider: SiteProvider::None,
            name: None,
            root: None,
            secure: None,
            url: None,
            target: None,
        }
    }
}

impl SiteConfig {
    pub fn with_effective_defaults(&self) -> Self {
        let mut site = self.clone();
        site.name
            .get_or_insert_with(|| DEFAULT_SITE_NAME_TEMPLATE.into());
        site.root.get_or_insert_with(|| DEFAULT_SITE_ROOT.into());
        site.secure.get_or_insert(true);
        if site.url.is_none() {
            site.url = Some(default_site_url(site.secure.unwrap_or(true)));
        }
        if site.target.is_none() && site.provider == SiteProvider::Traefik {
            site.target = Some(DEFAULT_TRAEFIK_SITE_TARGET_TEMPLATE.into());
        }
        site
    }

    pub fn effective_name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_SITE_NAME_TEMPLATE)
    }

    pub fn effective_root(&self) -> &str {
        self.root.as_deref().unwrap_or(DEFAULT_SITE_ROOT)
    }

    pub fn effective_secure(&self) -> bool {
        self.secure.unwrap_or(true)
    }

    pub fn effective_url(&self) -> Cow<'_, str> {
        match self.url.as_deref() {
            Some(url) => Cow::Borrowed(url),
            None => Cow::Owned(default_site_url(self.effective_secure())),
        }
    }

    pub fn effective_target(&self) -> Option<&str> {
        self.target.as_deref().or_else(|| {
            (self.provider == SiteProvider::Traefik).then_some(DEFAULT_TRAEFIK_SITE_TARGET_TEMPLATE)
        })
    }
}

fn default_site_url(secure: bool) -> String {
    let scheme = if secure { "https" } else { "http" };
    format!("{scheme}://{{{{site_name}}}}.test")
}

#[derive(Debug, Deserialize, Default, PartialEq, Clone)]
#[serde(rename_all = "snake_case")]
pub enum SiteProvider {
    #[default]
    None,
    Herd,
    Valet,
    DockerProxy,
    Traefik,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceConfig {
    pub tabs: Vec<String>,
    pub post_deps_tabs: Vec<String>,
    pub colors: HashMap<String, String>,
    pub browser: Option<WorkspaceBrowserConfig>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct WorkspaceConfigRaw {
    tabs: Vec<String>,
    post_deps_tabs: Vec<String>,
    colors: HashMap<String, String>,
    browser: Option<WorkspaceBrowserConfig>,
    chrome_devtools: Option<toml::Value>,
}

impl<'de> Deserialize<'de> for WorkspaceConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = WorkspaceConfigRaw::deserialize(deserializer)?;
        if raw.colors.contains_key("new") {
            return Err(D::Error::custom(
                "[workspace].colors.new is no longer supported; use [workspace].colors.branch for wt run branch",
            ));
        }
        if raw.chrome_devtools.is_some() {
            return Err(D::Error::custom(
                "[workspace.chrome_devtools] is no longer supported; move it to [workspace.browser.chrome_devtools]",
            ));
        }

        Ok(Self {
            tabs: raw.tabs,
            post_deps_tabs: raw.post_deps_tabs,
            colors: raw.colors,
            browser: raw.browser,
        })
    }
}

impl WorkspaceConfig {
    pub fn effective_color(&self, kind: &str) -> Option<&str> {
        self.colors
            .get(kind)
            .map(String::as_str)
            .or_else(|| default_workspace_color(kind))
    }

    pub fn effective_colors(&self) -> Vec<(&str, &str)> {
        let mut colors = WORKSPACE_DEFAULT_COLORS
            .iter()
            .map(|(kind, default_color)| {
                (
                    *kind,
                    self.colors
                        .get(*kind)
                        .map(String::as_str)
                        .unwrap_or(default_color),
                )
            })
            .collect::<Vec<_>>();

        let mut custom_colors = self
            .colors
            .iter()
            .filter(|(kind, _)| {
                !WORKSPACE_DEFAULT_COLORS
                    .iter()
                    .any(|(default_kind, _)| default_kind == &kind.as_str())
            })
            .map(|(kind, color)| (kind.as_str(), color.as_str()))
            .collect::<Vec<_>>();
        custom_colors.sort_by(|a, b| a.0.cmp(b.0));
        colors.extend(custom_colors);
        colors
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceBrowserConfig {
    pub mode: WorkspaceBrowserMode,
    pub url: Option<String>,
    pub app: Option<String>,
    pub chrome_devtools: Option<WorkspaceChromeDevtoolsConfig>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceBrowserMode {
    None,
    System,
    ChromeDevtools,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceBrowserConfigRaw {
    mode: WorkspaceBrowserMode,
    url: Option<String>,
    app: Option<String>,
    chrome_devtools: Option<WorkspaceChromeDevtoolsConfig>,
}

impl<'de> Deserialize<'de> for WorkspaceBrowserConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = WorkspaceBrowserConfigRaw::deserialize(deserializer)?;

        match raw.mode {
            WorkspaceBrowserMode::None => {
                if raw.url.is_some() {
                    return Err(D::Error::custom(
                        "[workspace.browser].url is not valid when mode = \"none\"",
                    ));
                }
                if raw.app.is_some() {
                    return Err(D::Error::custom(
                        "[workspace.browser].app is not valid when mode = \"none\"",
                    ));
                }
                if raw.chrome_devtools.is_some() {
                    return Err(D::Error::custom(
                        "[workspace.browser.chrome_devtools] is only valid when mode = \"chrome_devtools\"",
                    ));
                }
            }
            WorkspaceBrowserMode::System => {
                if raw.chrome_devtools.is_some() {
                    return Err(D::Error::custom(
                        "[workspace.browser.chrome_devtools] is only valid when mode = \"chrome_devtools\"",
                    ));
                }
            }
            WorkspaceBrowserMode::ChromeDevtools => {
                if raw.app.is_some() {
                    return Err(D::Error::custom(
                        "[workspace.browser].app is only valid when mode = \"system\"",
                    ));
                }
            }
        }

        Ok(Self {
            mode: raw.mode,
            url: raw.url,
            app: raw.app,
            chrome_devtools: raw.chrome_devtools,
        })
    }
}

impl WorkspaceBrowserConfig {
    pub fn effective_url(&self) -> Option<Cow<'_, str>> {
        match self.mode {
            WorkspaceBrowserMode::None => None,
            WorkspaceBrowserMode::System | WorkspaceBrowserMode::ChromeDevtools => {
                Some(match self.url.as_deref() {
                    Some(url) => Cow::Borrowed(url),
                    None => Cow::Borrowed(DEFAULT_WORKSPACE_BROWSER_URL),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceChromeDevtoolsConfig {
    pub port: Option<u16>,
    pub user_data_dir: Option<String>,
}

impl WorkspaceChromeDevtoolsConfig {
    pub fn effective_user_data_dir(&self) -> &str {
        self.user_data_dir
            .as_deref()
            .unwrap_or(DEFAULT_CHROME_DEVTOOLS_USER_DATA_DIR)
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub cli: AgentCli,
    pub args: Vec<String>,
    pub command: Option<String>,
    pub ready: ReadyMode,
    pub submit: SubmitMode,
    pub timeout: u64,
    pub send_after: u64,
    pub prompt: HashMap<String, Vec<String>>,
    #[doc(hidden)]
    pub presence: AgentConfigPresence,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            cli: AgentCli::None,
            args: Vec::new(),
            command: None,
            ready: default_agent_ready(),
            submit: default_agent_submit(),
            timeout: default_agent_timeout(),
            send_after: default_agent_send_after(),
            prompt: HashMap::new(),
            presence: AgentConfigPresence::default(),
        }
    }
}

impl PartialEq for AgentConfig {
    fn eq(&self, other: &Self) -> bool {
        self.cli == other.cli
            && self.args == other.args
            && self.command == other.command
            && self.ready == other.ready
            && self.submit == other.submit
            && self.timeout == other.timeout
            && self.send_after == other.send_after
            && self.prompt == other.prompt
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentConfigPresence {
    pub cli: bool,
    pub args: bool,
    pub command: bool,
    pub ready: bool,
    pub submit: bool,
    pub timeout: bool,
    pub send_after: bool,
}

impl AgentConfigPresence {
    pub fn has_runtime_fields(self) -> bool {
        self.cli
            || self.args
            || self.command
            || self.ready
            || self.submit
            || self.timeout
            || self.send_after
    }

    pub fn has_runtime_fields_without_cli(self) -> bool {
        self.args || self.command || self.ready || self.submit || self.timeout || self.send_after
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct AgentConfigRaw {
    cli: Option<AgentCli>,
    args: Option<Vec<String>>,
    command: Option<String>,
    ready: Option<ReadyMode>,
    submit: Option<SubmitMode>,
    timeout: Option<u64>,
    send_after: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_agent_prompts")]
    prompt: HashMap<String, Vec<String>>,
}

impl<'de> Deserialize<'de> for AgentConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = AgentConfigRaw::deserialize(deserializer)?;
        let presence = AgentConfigPresence {
            cli: raw.cli.is_some(),
            args: raw.args.is_some(),
            command: raw.command.is_some(),
            ready: raw.ready.is_some(),
            submit: raw.submit.is_some(),
            timeout: raw.timeout.is_some(),
            send_after: raw.send_after.is_some(),
        };

        if !presence.has_runtime_fields() && raw.prompt.is_empty() {
            return Err(D::Error::custom(
                "agent.cli is required unless the section only contains agent.prompt or agent.prompt.append, or inherits agent.cli from a lower-precedence config layer",
            ));
        }

        Ok(Self {
            cli: raw.cli.unwrap_or(AgentCli::None),
            args: raw.args.unwrap_or_default(),
            command: raw.command,
            ready: raw.ready.unwrap_or_else(default_agent_ready),
            submit: raw.submit.unwrap_or_else(default_agent_submit),
            timeout: raw.timeout.unwrap_or_else(default_agent_timeout),
            send_after: raw.send_after.unwrap_or_else(default_agent_send_after),
            prompt: raw.prompt,
            presence,
        })
    }
}

fn deserialize_agent_prompts<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = HashMap::<String, toml::Value>::deserialize(deserializer)?;
    let mut prompts = HashMap::new();

    for (mode, value) in raw {
        if mode == "append" {
            let append = value.as_table().ok_or_else(|| {
                D::Error::custom("[agent.prompt.append] must be a table of mode prompt arrays")
            })?;
            for (append_mode, append_value) in append {
                reject_legacy_agent_prompt_mode::<D::Error>(append_mode, true)?;
                let prompts_to_append = parse_prompt_values::<D::Error>(
                    append_value.clone(),
                    &format!("agent.prompt.append.{append_mode}"),
                )?;
                prompts.insert(prompt_append_key(append_mode), prompts_to_append);
            }
            continue;
        }

        reject_legacy_agent_prompt_mode::<D::Error>(&mode, false)?;
        let mode_prompts = parse_prompt_values::<D::Error>(value, &format!("agent.prompt.{mode}"))?;
        prompts.insert(mode, mode_prompts);
    }

    Ok(prompts)
}

fn reject_legacy_agent_prompt_mode<E>(mode: &str, append: bool) -> std::result::Result<(), E>
where
    E: DeError,
{
    if mode != "new" {
        return Ok(());
    }

    let key = if append {
        "[agent.prompt.append].new"
    } else {
        "[agent.prompt].new"
    };
    Err(E::custom(format!(
        "{key} is no longer supported; use [agent.prompt].branch or [agent.prompt.append].branch for wt run branch"
    )))
}

fn parse_prompt_values<E>(value: toml::Value, key: &str) -> std::result::Result<Vec<String>, E>
where
    E: DeError,
{
    value
        .try_into::<Vec<String>>()
        .map_err(|err| E::custom(format!("{key} must be an array of strings: {err}")))
}

pub(super) fn prompt_append_key(mode: &str) -> String {
    format!("{PROMPT_APPEND_PREFIX}{mode}")
}

pub(super) fn prompt_append_mode(key: &str) -> Option<&str> {
    key.strip_prefix(PROMPT_APPEND_PREFIX)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileConfig {
    pub name: Option<String>,
    pub worktree: WorktreeConfig,
    pub setup: SetupConfig,
    pub site: Option<SiteConfig>,
    pub workspace: Option<WorkspaceConfig>,
    pub agent: Option<AgentConfig>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct ProfileConfigRaw {
    name: Option<String>,
    worktree: WorktreeConfig,
    setup: SetupConfig,
    site: Option<SiteConfig>,
    workspace: Option<WorkspaceConfig>,
    agent: Option<AgentConfig>,
}

impl<'de> Deserialize<'de> for ProfileConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = ProfileConfigRaw::deserialize(deserializer)?;
        let profile = ProfileConfig {
            name: raw.name,
            worktree: raw.worktree,
            setup: raw.setup,
            site: raw.site,
            workspace: raw.workspace,
            agent: raw.agent,
        };
        profile
            .validate()
            .map_err(|err| serde::de::Error::custom(err.to_string()))?;
        Ok(profile)
    }
}

impl ProfileConfig {
    pub fn has_inline_settings(&self) -> bool {
        self.worktree != WorktreeConfig::default()
            || self.setup != SetupConfig::default()
            || self.site.is_some()
            || self.workspace.is_some()
            || self.agent.is_some()
    }

    fn validate(&self) -> anyhow::Result<()> {
        if let Some(name) = self.name.as_deref() {
            validate_profile_name(name)?;
        }

        if self.name.is_some() && self.has_inline_settings() {
            bail!(
                "[profile] name cannot be combined with inline [profile.agent], [profile.worktree], [profile.setup], [profile.workspace], or [profile.site] sections"
            );
        }

        Ok(())
    }

    pub(super) fn into_config(self) -> Config {
        Config {
            worktree: self.worktree,
            setup: self.setup,
            site: self.site,
            workspace: self.workspace,
            agent: self.agent,
            ..Config::default()
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentCli {
    Codex,
    Claude,
    Gemini,
    #[default]
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

pub(super) fn default_agent_ready() -> ReadyMode {
    ReadyMode::Auto
}

pub(super) fn default_agent_submit() -> SubmitMode {
    SubmitMode::Auto
}

pub(super) fn default_agent_timeout() -> u64 {
    15
}

pub(super) fn default_agent_send_after() -> u64 {
    3
}

pub fn validate_profile_name(name: &str) -> anyhow::Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Profile name cannot be empty");
    }
    if trimmed != name {
        bail!("Profile name cannot contain leading or trailing whitespace: {name:?}");
    }
    if name == RESERVED_PROFILE_NAME {
        bail!("'{RESERVED_PROFILE_NAME}' is reserved and cannot be used as a profile name");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("Profile name must contain only ASCII letters, digits, '-' or '_': {name}");
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct IssuesConfig {
    pub provider: IssueProviderType,
    pub gh_user: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IssueProviderType {
    Linear,
    Github,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct WorktreeNamingConfig {
    pub command: String,
    pub prompt: String,
    pub branch: Option<String>,
    pub workspace: Option<String>,
}

impl Default for WorktreeNamingConfig {
    fn default() -> Self {
        Self {
            command: "claude -p".into(),
            prompt: default_worktree_naming_prompt(),
            branch: Some("{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}".into()),
            workspace: None,
        }
    }
}

fn default_worktree_naming_prompt() -> String {
    r#"Generate concise English naming variables for a worktree issue.

Issue identifier: {{issue_identifier}}
Issue title: {{issue_title}}
Suggested branch: {{suggested_branch}}

Return only JSON with string values:
{"english_slug":"..."}

Rules:
- english_slug: lowercase ASCII kebab-case, 3-8 words, no issue identifier.
- Do not include markdown or extra text.
"#
    .into()
}

impl Config {
    pub(crate) fn validate_effective_agent(&self) -> anyhow::Result<()> {
        let Some(agent) = self.agent.as_ref() else {
            return Ok(());
        };

        if agent.cli == AgentCli::None
            && !agent.presence.cli
            && agent.presence.has_runtime_fields_without_cli()
        {
            bail!(
                "agent.cli is required when [agent] sets args, command, ready, submit, timeout, or send_after without inheriting agent.cli from a lower-precedence config layer"
            );
        }

        Ok(())
    }

    pub fn workflow_default_policy(&self) -> WorkflowDefaultPolicy {
        WorkflowDefaultPolicy {
            pull_request: self
                .workflow
                .pull_request
                .unwrap_or(WorkflowDefaultPullRequestMode::None),
            landing: self
                .workflow
                .landing
                .unwrap_or(WorkflowDefaultLandingPolicy::Manual),
            review: self.review_default_policy(),
        }
    }

    pub fn review_default_policy(&self) -> ReviewDefaultPolicy {
        ReviewDefaultPolicy {
            codex_base: self
                .review
                .codex_base
                .unwrap_or(ReviewCodexBasePolicy::None),
        }
    }

    pub fn effective_site(&self) -> Option<SiteConfig> {
        let site = self.site.as_ref()?;
        if site.provider == SiteProvider::None {
            return None;
        }
        Some(site.with_effective_defaults())
    }

    pub fn effective_editor(&self) -> Option<EditorConfig> {
        if self.editor == EditorConfig::default() {
            return None;
        }
        Some(EditorConfig {
            command: self.editor.command.clone(),
            placement: Some(self.editor.effective_placement()),
        })
    }

    pub fn has_site(&self) -> bool {
        self.effective_site().is_some()
    }
}
