use crate::context::CmdOutput;
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

    pub fn create_issue(&self, title: &str, description: &str) -> Result<Issue> {
        let out = self.runner.run(
            "linear",
            &[
                "issue",
                "create",
                "--title",
                title,
                "--description",
                description,
            ],
            self.cwd,
        )?;
        if !out.success {
            bail!(
                "Linear issue creation failed: {}",
                command_failure_detail(&out)
            );
        }

        let Some(identifier) = parse_created_identifier(&out.stdout, &out.stderr) else {
            bail!("linear issue create did not return a created issue identifier");
        };

        Ok(Issue {
            identifier,
            title: title.to_string(),
            branch_name: None,
            description: optional_description(description),
        })
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

fn parse_created_identifier(stdout: &str, stderr: &str) -> Option<String> {
    stderr
        .lines()
        .chain(stdout.lines())
        .find_map(parse_created_identifier_line)
}

fn parse_created_identifier_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if let Some(identifier) = trimmed.strip_prefix("Created:") {
        return normalize_identifier(identifier.trim());
    }

    trimmed
        .split_whitespace()
        .find_map(parse_created_identifier_token)
}

fn parse_created_identifier_token(token: &str) -> Option<String> {
    let (_, issue) = token.split_once("/issue/")?;
    let issue = issue
        .split(['?', '#', '/', '\t', '\r', '\n'])
        .next()
        .unwrap_or(issue)
        .trim();
    normalize_identifier(issue)
}

fn normalize_identifier(identifier: &str) -> Option<String> {
    let (team, number) = identifier.split_once('-')?;
    if team.is_empty()
        || number.is_empty()
        || !team.chars().all(|ch| ch.is_ascii_alphanumeric())
        || !number.chars().all(|ch| ch.is_ascii_digit())
    {
        None
    } else {
        Some(format!("{}-{number}", team.to_ascii_uppercase()))
    }
}

fn optional_description(description: &str) -> Option<String> {
    if description.trim().is_empty() {
        None
    } else {
        Some(description.to_string())
    }
}

fn command_failure_detail(out: &CmdOutput) -> &str {
    if out.stderr.trim().is_empty() {
        out.stdout.trim()
    } else {
        out.stderr.trim()
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
