use anyhow::Result;
use crate::context::CommandRunner;
use crate::services::issues::{IssueInfo, IssueListItem, IssueProvider};
use crate::services::linear::LinearService;
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
        let identifier = if id.chars().all(|c| c.is_ascii_digit()) {
            format!("TECH-{id}")
        } else {
            id.to_string()
        };
        let issue = self.linear.get_issue(&identifier)?;
        Ok(IssueInfo {
            identifier: issue.identifier,
            title: issue.title,
            branch_name: issue.branch_name,
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
                IssueListItem {
                    display: format!("{} {} [{}]", i.identifier, i.title, assignee),
                    identifier: i.identifier,
                    title: i.title,
                }
            })
            .collect())
    }

    fn ensure_branch(&self, id: &str, _base: Option<&str>) -> Result<String> {
        let issue = self.linear.get_issue(id)?;
        issue.branch_name.ok_or_else(|| {
            crate::error::WtError::NoBranchName {
                identifier: id.to_string(),
            }
            .into()
        })
    }

    fn on_start(&self, id: &str) -> Result<()> {
        self.linear.update_status(id, "In Progress")
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn get_issue_normalizes_numeric_id_to_tech_prefix() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"위키 에디터","branchName":"hoetaek/tech-680-위키"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider.get_issue("680").unwrap();
        assert_eq!(issue.identifier, "TECH-680");
        assert_eq!(issue.title, "위키 에디터");
        assert_eq!(issue.branch_name.as_deref(), Some("hoetaek/tech-680-위키"));
    }

    #[test]
    fn get_issue_passes_through_full_identifier() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"위키 에디터","branchName":"hoetaek/tech-680-위키"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let issue = provider.get_issue("TECH-680").unwrap();
        assert_eq!(issue.identifier, "TECH-680");
    }

    #[test]
    fn list_issues_maps_to_display_format() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"TECH-1","title":"Issue 1","state":{"name":"Started"},"assignee":{"displayName":"hoetaek"}}]"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].identifier, "TECH-1");
        assert!(items[0].display.contains("TECH-1"));
        assert!(items[0].display.contains("hoetaek"));
    }

    #[test]
    fn ensure_branch_returns_branch_name() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-680","title":"위키 에디터","branchName":"hoetaek/tech-680-위키"}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let branch = provider.ensure_branch("TECH-680", None).unwrap();
        assert_eq!(branch, "hoetaek/tech-680-위키");
    }

    #[test]
    fn ensure_branch_errors_when_no_branch_name() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"TECH-100","title":"Test","branchName":null}"#,
            true,
        );
        let provider = LinearIssueProvider::new(&runner, None);
        let result = provider.ensure_branch("TECH-100", None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No branch name"));
    }

    #[test]
    fn on_start_updates_status() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let provider = LinearIssueProvider::new(&runner, None);
        provider.on_start("TECH-680").unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["issue", "update", "TECH-680", "--state", "In Progress"]);
    }

    #[test]
    fn on_clean_is_noop() {
        let runner = MockRunner::new();
        let provider = LinearIssueProvider::new(&runner, None);
        assert!(provider.on_clean("TECH-680", "hoetaek/tech-680").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
