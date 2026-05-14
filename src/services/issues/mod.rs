use anyhow::Result;

pub mod github;
pub mod linear;

pub struct IssueInfo {
    pub identifier: String,
    pub title: String,
    pub branch_name: Option<String>,
    pub body: Option<String>,
}

pub struct IssueListItem {
    pub identifier: String,
    pub title: String,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsuredBranch {
    pub name: String,
    pub created: bool,
}

pub trait IssueProvider {
    fn get_issue(&self, id: &str) -> Result<IssueInfo>;
    fn list_issues(&self) -> Result<Vec<IssueListItem>>;
    fn ensure_branch(
        &self,
        id: &str,
        base: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<EnsuredBranch>;
    fn on_start(&self, id: &str) -> Result<()>;
    fn on_clean(&self, id: &str, branch: &str) -> Result<()>;
}
