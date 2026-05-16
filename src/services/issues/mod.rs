use anyhow::Result;

pub mod github;
pub mod linear;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueInfo {
    pub identifier: String,
    pub title: String,
    pub branch_name: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueListItem {
    pub identifier: String,
    pub title: String,
    pub display: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateIssueRequest {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsuredBranch {
    pub name: String,
    pub created: bool,
}

pub trait IssueProvider {
    fn get_issue(&self, id: &str) -> Result<IssueInfo>;
    fn list_issues(&self) -> Result<Vec<IssueListItem>>;
    fn create_issue(&self, request: CreateIssueRequest) -> Result<IssueInfo>;
    fn ensure_branch(
        &self,
        id: &str,
        base: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<EnsuredBranch>;
    fn on_start(&self, id: &str) -> Result<()>;
    fn on_clean(&self, id: &str, branch: &str) -> Result<()>;
}
