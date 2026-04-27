use anyhow::{Result, bail};
use crate::context::CommandRunner;
use crate::services::issues::{IssueInfo, IssueListItem, IssueProvider};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct GhIssue {
    number: u32,
    title: String,
}

pub struct GithubIssueProvider<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
    gh_user: Option<String>,
}

impl<'a> GithubIssueProvider<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>, gh_user: Option<String>) -> Self {
        Self { runner, cwd, gh_user }
    }
}

impl IssueProvider for GithubIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        let out = self.runner.run(
            "gh",
            &["issue", "view", id, "--json", "number,title"],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch issue #{id}");
        }
        let gh_issue: GhIssue = serde_json::from_str(&out.stdout)?;

        let list_out = self.runner.run(
            "gh",
            &["issue", "develop", "--list", id],
            self.cwd,
        )?;
        let branch_name = list_out
            .stdout
            .lines()
            .next()
            .and_then(|line| line.split('\t').nth(1))
            .filter(|s| !s.is_empty())
            .map(String::from);

        Ok(IssueInfo {
            identifier: format!("#{}", gh_issue.number),
            title: gh_issue.title,
            branch_name,
        })
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let mut args = vec!["issue", "list", "--json", "number,title", "--state", "open"];
        let gh_user_str;
        if let Some(ref user) = self.gh_user {
            gh_user_str = user.clone();
            args.extend_from_slice(&["-a", &gh_user_str]);
        }
        let out = self.runner.run("gh", &args, self.cwd)?;
        if !out.success {
            bail!("Failed to fetch issue list");
        }
        let issues: Vec<GhIssue> = serde_json::from_str(&out.stdout)?;
        Ok(issues
            .into_iter()
            .map(|i| IssueListItem {
                display: format!("#{} {}", i.number, i.title),
                identifier: i.number.to_string(),
                title: i.title,
            })
            .collect())
    }

    fn ensure_branch(&self, id: &str, base: Option<&str>) -> Result<String> {
        let list_out = self.runner.run(
            "gh",
            &["issue", "develop", "--list", id],
            self.cwd,
        )?;
        if let Some(branch) = list_out
            .stdout
            .lines()
            .next()
            .and_then(|line| line.split('\t').nth(1))
            .filter(|s| !s.is_empty())
        {
            return Ok(branch.to_string());
        }

        let mut args = vec!["issue", "develop"];
        if let Some(b) = base {
            args.extend_from_slice(&["--base", b]);
        }
        args.push(id);

        let out = self.runner.run("gh", &args, self.cwd)?;
        if !out.success {
            bail!("Failed to create branch for issue #{id}");
        }
        let branch = out.stdout.trim().to_string();
        if branch.is_empty() {
            bail!("gh issue develop returned empty branch name for #{id}");
        }
        Ok(branch)
    }

    fn on_start(&self, _id: &str) -> Result<()> {
        Ok(())
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
    fn get_issue_parses_gh_json() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"number":42,"title":"Add feature"}"#, true);
        runner.add_response("42\thoetaek/42-add-feature\n", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let issue = provider.get_issue("42").unwrap();
        assert_eq!(issue.identifier, "#42");
        assert_eq!(issue.title, "Add feature");
        assert_eq!(issue.branch_name.as_deref(), Some("hoetaek/42-add-feature"));
    }

    #[test]
    fn get_issue_no_existing_branch() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"number":42,"title":"Add feature"}"#, true);
        runner.add_response("", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let issue = provider.get_issue("42").unwrap();
        assert_eq!(issue.identifier, "#42");
        assert!(issue.branch_name.is_none());
    }

    #[test]
    fn list_issues_with_gh_user_filter() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"number":1,"title":"Issue 1"},{"number":2,"title":"Issue 2"}]"#,
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, Some("hoetaek".into()));
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].display, "#1 Issue 1");

        let calls = runner.calls.lock().unwrap();
        assert!(calls[0].1.contains(&"-a".to_string()));
        assert!(calls[0].1.contains(&"hoetaek".to_string()));
    }

    #[test]
    fn list_issues_without_gh_user() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"[{"number":1,"title":"Issue 1"}]"#, true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 1);

        let calls = runner.calls.lock().unwrap();
        assert!(!calls[0].1.contains(&"-a".to_string()));
    }

    #[test]
    fn ensure_branch_returns_existing() {
        let mut runner = MockRunner::new();
        runner.add_response("42\thoetaek/42-add-feature\n", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", None).unwrap();
        assert_eq!(branch, "hoetaek/42-add-feature");
    }

    #[test]
    fn ensure_branch_creates_new_without_base() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("hoetaek/42-add-feature", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", None).unwrap();
        assert_eq!(branch, "hoetaek/42-add-feature");

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(create_call.1, vec!["issue", "develop", "42"]);
    }

    #[test]
    fn ensure_branch_creates_new_with_base() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("hoetaek/42-add-feature", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", Some("develop")).unwrap();
        assert_eq!(branch, "hoetaek/42-add-feature");

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(create_call.1, vec!["issue", "develop", "--base", "develop", "42"]);
    }

    #[test]
    fn on_start_is_noop() {
        let runner = MockRunner::new();
        let provider = GithubIssueProvider::new(&runner, None, None);
        assert!(provider.on_start("42").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn on_clean_is_noop() {
        let runner = MockRunner::new();
        let provider = GithubIssueProvider::new(&runner, None, None);
        assert!(provider.on_clean("42", "hoetaek/42-feature").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
