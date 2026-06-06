use anyhow::Result;

pub mod capabilities;
pub mod github;
pub mod linear;

pub use capabilities::{IssueCapabilities, IssueOperation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueInfo {
    pub identifier: String,
    pub title: String,
    pub branch_name: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueDetail {
    pub identifier: String,
    pub title: String,
    pub body: Option<String>,
    pub url: Option<String>,
    pub status: Option<String>,
    pub labels: Vec<String>,
    pub comments_count: Option<usize>,
    pub updated_at: Option<String>,
}

impl From<IssueInfo> for IssueDetail {
    fn from(issue: IssueInfo) -> Self {
        Self {
            identifier: issue.identifier,
            title: issue.title,
            body: issue.body,
            url: None,
            status: None,
            labels: Vec::new(),
            comments_count: None,
            updated_at: None,
        }
    }
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
pub struct IssueFieldUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueComment {
    pub id: String,
    pub body: String,
    pub created_at: Option<String>,
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

pub struct IssueProviderFacade<'a> {
    pub provider: Box<dyn IssueProvider + 'a>,
    pub capabilities: IssueCapabilities,
}

impl<'a> IssueProviderFacade<'a> {
    pub fn new(provider: Box<dyn IssueProvider + 'a>, capabilities: IssueCapabilities) -> Self {
        Self {
            provider,
            capabilities,
        }
    }
}

pub trait IssueReader {
    fn get_issue_detail(&self, id: &str) -> Result<IssueDetail>;
}

pub trait IssueUpdater {
    fn update_issue_fields(&self, id: &str, update: IssueFieldUpdate) -> Result<IssueDetail>;
}

pub trait IssueCommenter {
    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment>;
}
