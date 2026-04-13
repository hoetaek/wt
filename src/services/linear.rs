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

    pub fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let out = self.runner.run(
            "linear",
            &[
                "issue",
                "list",
                "--state",
                "backlog",
                "--state",
                "unstarted",
                "--state",
                "started",
                "--json",
            ],
            self.cwd,
        )?;
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
            r#"{"identifier":"TECH-680","title":"C11S09. 위키 에디터","branchName":"hoetaek/tech-680-c11s09-위키"}"#,
            true,
        );

        let svc = LinearService::new(&runner, None);
        let issue = svc.get_issue("TECH-680").unwrap();
        assert_eq!(issue.identifier, "TECH-680");
        assert_eq!(issue.title, "C11S09. 위키 에디터");
        assert_eq!(
            issue.branch_name.as_deref(),
            Some("hoetaek/tech-680-c11s09-위키")
        );
    }

    #[test]
    fn get_issue_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("not found", false);

        let svc = LinearService::new(&runner, None);
        assert!(svc.get_issue("TECH-999").is_err());
    }

    #[test]
    fn list_issues_parses_array() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"TECH-1","title":"Issue 1","state":{"name":"Started"},"assignee":{"displayName":"hoetaek"}}]"#,
            true,
        );

        let svc = LinearService::new(&runner, None);
        let issues = svc.list_issues().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].assignee.as_ref().unwrap().display_name, "hoetaek");
    }
}
