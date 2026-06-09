mod agent;
mod loader;
mod merge;
mod profile;
mod schema;

pub use loader::{ConfigSource, InvalidProfileRecord, ProfileInventory, ProfileRecord};
pub use schema::{
    AGENT_PROMPT_WORKFLOW_SCOPE, AgentCli, AgentConfig, AgentConfigPresence, ColumnConfig, Config,
    DepCommand, EditorConfig, EditorPlacement, IssueProviderType, IssuesConfig, Language,
    OriginPolicy, PathSpec, ProfileConfig, RESERVED_PROFILE_NAME, ReadyMode, ReviewCodexBasePolicy,
    ReviewConfig, ReviewDefaultPolicy, SetupConfig, SiteConfig, SiteProvider, SubmitMode,
    TaskListColumns, TaskListConfig, WORKSPACE_COLOR_KIND_BRANCH, WORKSPACE_COLOR_KIND_ISSUE,
    WORKSPACE_COLOR_KIND_PR, WORKSPACE_COLOR_KIND_TASK, WORKSPACE_DEFAULT_COLORS, WorkflowConfig,
    WorkflowDefaultLandingPolicy, WorkflowDefaultPolicy, WorkflowDefaultPullRequestMode,
    WorkspaceBrowserConfig, WorkspaceBrowserMode, WorkspaceChromeDevtoolsConfig, WorkspaceConfig,
    WorktreeConfig, WorktreeNamingConfig, default_workspace_color, validate_profile_name,
};

#[cfg(test)]
mod tests;
