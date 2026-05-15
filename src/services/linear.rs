use crate::context::CommandRunner;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub identifier: String,
    pub title: String,
    pub branch_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueListItem {
    pub identifier: String,
    pub title: String,
    pub state: IssueState,
    pub assignee: Option<Assignee>,
}

#[derive(Debug, Deserialize)]
pub struct IssueState {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignee {
    pub display_name: String,
}

pub struct LinearService<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
}

impl<'a> LinearService<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>) -> Self {
        Self { runner, cwd }
    }

    pub fn get_issue(&self, identifier: &str) -> Result<Issue> {
        let out = self
            .runner
            .run("linear", &["issue", "view", identifier, "--json"], self.cwd)?;
        if !out.success {
            bail!("Failed to fetch issue {identifier}");
        }
        let issue: Issue = serde_json::from_str(&out.stdout)?;
        Ok(issue)
    }

    pub fn current_issue_id(&self) -> Result<String> {
        let out = self.runner.run("linear", &["issue", "id"], self.cwd)?;
        if !out.success || out.stdout.trim().is_empty() {
            bail!("Failed to resolve current Linear issue id");
        }
        Ok(out.stdout.trim().to_string())
    }

    pub fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let out = self
            .runner
            .run("linear", &["issue", "list", "--json"], self.cwd)?;
        if !out.success {
            bail!("Failed to fetch issue list");
        }
        let issues: Vec<IssueListItem> = serde_json::from_str(&out.stdout)?;
        Ok(issues)
    }

    pub fn update_status(&self, identifier: &str, state: &str) -> Result<()> {
        let out = self.runner.run(
            "linear",
            &["issue", "update", identifier, "--state", state],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to update issue {identifier} to {state}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn get_issue_parses_json() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680-document-editor"}"#,
            true,
        );

        let svc = LinearService::new(&runner, None);
        let issue = svc.get_issue("PROJ-680").unwrap();
        assert_eq!(issue.identifier, "PROJ-680");
        assert_eq!(issue.title, "Document editor");
        assert_eq!(
            issue.branch_name.as_deref(),
            Some("alice/proj-680-document-editor")
        );
    }

    #[test]
    fn get_issue_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("not found", false);

        let svc = LinearService::new(&runner, None);
        assert!(svc.get_issue("PROJ-999").is_err());
    }

    #[test]
    fn list_issues_parses_array() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"PROJ-1","title":"Issue 1","state":{"name":"Started"},"assignee":{"displayName":"alice"}}]"#,
            true,
        );

        let svc = LinearService::new(&runner, None);
        let issues = svc.list_issues().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].assignee.as_ref().unwrap().display_name, "alice");

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["issue", "list", "--json"]);
    }

    #[test]
    fn list_issues_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("error", false);
        let svc = LinearService::new(&runner, None);
        assert!(svc.list_issues().is_err());
    }

    #[test]
    fn current_issue_id_uses_linear_cli_branch_resolution() {
        let mut runner = MockRunner::new();
        runner.add_response("PROJ-680", true);

        let svc = LinearService::new(&runner, None);
        assert_eq!(svc.current_issue_id().unwrap(), "PROJ-680");

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["issue", "id"]);
    }

    #[test]
    fn update_status_success() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let svc = LinearService::new(&runner, None);
        svc.update_status("PROJ-1", "In Progress").unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["issue", "update", "PROJ-1", "--state", "In Progress"]
        );
    }

    #[test]
    fn update_status_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("", false);
        let svc = LinearService::new(&runner, None);
        assert!(svc.update_status("PROJ-1", "Done").is_err());
    }
}
