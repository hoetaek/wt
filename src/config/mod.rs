mod agent;
mod loader;
mod merge;
mod profile;
mod schema;

pub use loader::ConfigSource;
pub use schema::{
    AgentCli, AgentConfig, Config, CopyAsEntry, DepCommand, EditorConfig, EditorPlacement,
    IssueProviderType, IssuesConfig, ProfileConfig, RESERVED_PROFILE_NAME, ReadyMode, SetupConfig,
    SiteConfig, SiteProvider, SubmitMode, TestCommand, TestConfig, WorkflowConfig,
    WorkflowDefaultLandingPolicy, WorkflowDefaultPolicy, WorkflowDefaultPullRequestMode,
    WorkspaceConfig, WorktreeConfig, WorktreeNamingConfig, validate_profile_name,
};

#[cfg(test)]
mod tests;
