mod agent;
mod loader;
mod merge;
mod profile;
mod schema;

pub use loader::ConfigSource;
pub use schema::{
    AgentCli, AgentConfig, Config, CopyAsEntry, DepCommand, EditorConfig, EditorPlacement,
    IssueProviderType, IssuesConfig, ProfileConfig, RESERVED_PROFILE_NAME, ReadyMode, SetupConfig,
    SiteConfig, SiteProvider, SubmitMode, TestCommand, TestConfig, WORKSPACE_COLOR_KIND_ISSUE,
    WORKSPACE_COLOR_KIND_NEW, WORKSPACE_COLOR_KIND_PR, WORKSPACE_COLOR_KIND_TASK,
    WORKSPACE_DEFAULT_COLORS, WorkflowConfig, WorkflowDefaultLandingPolicy, WorkflowDefaultPolicy,
    WorkflowDefaultPullRequestMode, WorkspaceConfig, WorktreeConfig, WorktreeNamingConfig,
    default_workspace_color, validate_profile_name,
};

#[cfg(test)]
mod tests;
