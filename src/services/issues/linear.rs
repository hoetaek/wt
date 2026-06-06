use crate::context::CommandRunner;
use crate::services::issues::{
    CreateIssueRequest, EnsuredBranch, IssueDetail, IssueInfo, IssueListItem, IssueProvider,
    IssueReader,
};
use crate::services::linear::LinearService;
use anyhow::Result;
use std::path::Path;

pub struct LinearIssueProvider<'a> {
    linear: LinearService<'a>,
}

impl<'a> LinearIssueProvider<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>) -> Self {
        Self {
            linear: LinearService::new(runner, cwd),
        }
    }
}

impl IssueProvider for LinearIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        let issue = self.linear.get_issue(id)?;
        Ok(IssueInfo {
            identifier: issue.identifier,
            title: issue.title,
            branch_name: issue.branch_name,
            body: issue.description,
        })
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let issues = self.linear.list_issues()?;
        Ok(issues
            .into_iter()
            .map(|i| {
                let assignee = i
                    .assignee
                    .as_ref()
                    .map(|a| a.display_name.as_str())
                    .unwrap_or("-");
                let hint = if assignee == "-" {
                    i.state.name.clone()
                } else {
                    format!("{} | {}", i.state.name, assignee)
                };
                IssueListItem {
                    display: format!(
                        "{:<9} {:<12} {:<8} {}",
                        i.identifier, i.state.name, assignee, i.title
                    ),
                    identifier: i.identifier,
                    title: i.title,
                    hint: Some(hint),
                }
            })
            .collect())
    }

    fn create_issue(&self, request: CreateIssueRequest) -> Result<IssueInfo> {
        let issue = self.linear.create_issue(&request.title, &request.body)?;
        Ok(IssueInfo {
            identifier: issue.identifier,
            title: issue.title,
            branch_name: issue.branch_name,
            body: issue.description,
        })
    }

    fn ensure_branch(
        &self,
        id: &str,
        _base: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<EnsuredBranch> {
        if let Some(branch_name) = branch_name {
            return Ok(EnsuredBranch {
                name: branch_name.to_string(),
                created: false,
            });
        }

        let issue = self.linear.get_issue(id)?;
        let name = issue.branch_name.ok_or_else(|| {
            anyhow::Error::from(crate::error::WtError::NoBranchName {
                identifier: id.to_string(),
            })
        })?;
        Ok(EnsuredBranch {
            name,
            created: false,
        })
    }

    fn on_start(&self, id: &str) -> Result<()> {
        self.linear.update_status(id, "In Progress")
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}

impl IssueReader for LinearIssueProvider<'_> {
    fn get_issue_detail(&self, id: &str) -> Result<IssueDetail> {
        Ok(self.get_issue(id)?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn get_issue_delegates_numeric_id_to_linear_cli() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680-document-editor"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider.get_issue("680").unwrap();
        assert_eq!(issue.identifier, "PROJ-680");
        assert_eq!(issue.title, "Document editor");
        assert_eq!(
            issue.branch_name.as_deref(),
            Some("alice/proj-680-document-editor")
        );

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["issue", "view", "680", "--json"]);
    }

    #[test]
    fn get_issue_passes_through_full_identifier() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680-document-editor"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider.get_issue("PROJ-680").unwrap();
        assert_eq!(issue.identifier, "PROJ-680");
    }

    #[test]
    fn list_issues_maps_to_display_format() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"PROJ-1","title":"Issue 1","state":{"name":"Started"},"assignee":{"displayName":"alice"}}]"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].identifier, "PROJ-1");
        assert_eq!(items[0].display, "PROJ-1    Started      alice    Issue 1");
        assert_eq!(items[0].hint.as_deref(), Some("Started | alice"));
    }

    #[test]
    fn create_issue_delegates_to_linear_cli_and_parses_identifier() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr(
            "https://linear.app/acme/issue/PROJ-123/add-publish",
            "Created: PROJ-123",
            true,
        );

        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider
            .create_issue(CreateIssueRequest {
                title: "Add publish".into(),
                body: "Create provider issue.".into(),
            })
            .unwrap();

        assert_eq!(issue.identifier, "PROJ-123");
        assert_eq!(issue.title, "Add publish");
        assert_eq!(issue.body.as_deref(), Some("Create provider issue."));
        assert!(issue.branch_name.is_none());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "issue",
                "create",
                "--title",
                "Add publish",
                "--description",
                "Create provider issue."
            ]
        );
    }

    #[test]
    fn create_issue_reports_linear_failure() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("", "--team is required (or set team_id in config)", false);

        let provider = LinearIssueProvider::new(&runner, None);
        let result = provider.create_issue(CreateIssueRequest {
            title: "Add publish".into(),
            body: String::new(),
        });

        let err = result.unwrap_err().to_string();
        assert!(err.contains("Linear issue creation failed"));
        assert!(err.contains("--team is required"));
    }

    #[test]
    fn create_issue_errors_when_linear_output_has_no_identifier() {
        let mut runner = MockRunner::new();
        runner.add_response("https://linear.app/acme/inbox/123", true);

        let provider = LinearIssueProvider::new(&runner, None);
        let result = provider.create_issue(CreateIssueRequest {
            title: "Add publish".into(),
            body: String::new(),
        });

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("did not return a created issue identifier")
        );
    }

    #[test]
    fn ensure_branch_returns_branch_name() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680-document-editor"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let branch = provider.ensure_branch("PROJ-680", None, None).unwrap();
        assert_eq!(branch.name, "alice/proj-680-document-editor");
        assert!(!branch.created);
    }

    #[test]
    fn ensure_branch_uses_requested_branch_name() {
        let runner = MockRunner::new();
        let provider = LinearIssueProvider::new(&runner, None);
        let branch = provider
            .ensure_branch("PROJ-680", None, Some("alice/proj-680-wiki-editor"))
            .unwrap();
        assert_eq!(branch.name, "alice/proj-680-wiki-editor");
        assert!(!branch.created);
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn ensure_branch_errors_when_no_branch_name() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-100","title":"Test","branchName":null}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let result = provider.ensure_branch("PROJ-100", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No branch name"));
    }

    #[test]
    fn on_start_updates_status() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let provider = LinearIssueProvider::new(&runner, None);
        provider.on_start("PROJ-680").unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["issue", "update", "PROJ-680", "--state", "In Progress"]
        );
    }

    #[test]
    fn on_clean_is_noop() {
        let runner = MockRunner::new();
        let provider = LinearIssueProvider::new(&runner, None);
        assert!(provider.on_clean("PROJ-680", "alice/proj-680").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
