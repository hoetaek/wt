use crate::context::CmdOutput;
use crate::context::CommandRunner;
use crate::services::issues::{
    CreateIssueRequest, EnsuredBranch, IssueDetail, IssueInfo, IssueListItem, IssueProvider,
    IssueReader,
};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct GhIssue {
    number: u32,
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

pub struct GithubIssueProvider<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
    gh_user: Option<String>,
}

impl<'a> GithubIssueProvider<'a> {
    pub fn new(
        runner: &'a dyn CommandRunner,
        cwd: Option<&'a Path>,
        gh_user: Option<String>,
    ) -> Self {
        Self {
            runner,
            cwd,
            gh_user,
        }
    }

    fn fetch_issue_list(&self, extra_args: &[&str]) -> Result<Vec<GhIssue>> {
        let mut args = vec!["issue", "list", "--json", "number,title", "--state", "open"];
        args.extend_from_slice(extra_args);

        let out = self.runner.run("gh", &args, self.cwd)?;
        if !out.success {
            bail!("Failed to fetch issue list");
        }

        Ok(serde_json::from_str(&out.stdout)?)
    }
}

impl IssueProvider for GithubIssueProvider<'_> {
    fn get_issue(&self, id: &str) -> Result<IssueInfo> {
        let out = self.runner.run(
            "gh",
            &["issue", "view", id, "--json", "number,title,body,url"],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch issue #{id}");
        }
        let gh_issue: GhIssue = serde_json::from_str(&out.stdout)?;

        let list_out = self
            .runner
            .run("gh", &["issue", "develop", "--list", id], self.cwd)?;
        let branch_name = parse_linked_branch(&list_out.stdout);

        Ok(IssueInfo {
            identifier: format!("#{}", gh_issue.number),
            title: gh_issue.title,
            branch_name,
            body: snapshot_body(gh_issue.body, gh_issue.url),
        })
    }

    fn list_issues(&self) -> Result<Vec<IssueListItem>> {
        let issues = if let Some(ref user) = self.gh_user {
            let mut seen = HashSet::new();
            let mut combined = Vec::new();

            for issue in self
                .fetch_issue_list(&["-a", user.as_str()])?
                .into_iter()
                .chain(self.fetch_issue_list(&["-A", user.as_str()])?)
            {
                if seen.insert(issue.number) {
                    combined.push(issue);
                }
            }

            combined
        } else {
            self.fetch_issue_list(&[])?
        };

        Ok(issues
            .into_iter()
            .map(|i| IssueListItem {
                display: format!("#{} {}", i.number, i.title),
                identifier: i.number.to_string(),
                title: i.title,
                hint: Some("GitHub".into()),
            })
            .collect())
    }

    fn create_issue(&self, request: CreateIssueRequest) -> Result<IssueInfo> {
        let out = self.runner.run(
            "gh",
            &[
                "issue",
                "create",
                "--title",
                &request.title,
                "--body",
                &request.body,
            ],
            self.cwd,
        )?;
        if !out.success {
            bail!(
                "GitHub issue creation failed: {}",
                command_failure_detail(&out)
            );
        }

        let Some(number) = parse_created_issue_number(&out.stdout) else {
            bail!("gh issue create did not return an issue URL containing an issue number");
        };

        Ok(IssueInfo {
            identifier: format!("#{number}"),
            title: request.title,
            branch_name: None,
            body: optional_body(request.body),
        })
    }

    fn ensure_branch(
        &self,
        id: &str,
        base: Option<&str>,
        branch_name: Option<&str>,
    ) -> Result<EnsuredBranch> {
        let list_out = self
            .runner
            .run("gh", &["issue", "develop", "--list", id], self.cwd)?;
        if let Some(branch) = parse_linked_branch(&list_out.stdout) {
            return Ok(EnsuredBranch {
                name: branch,
                created: false,
            });
        }

        let mut args = vec!["issue", "develop"];
        if let Some(b) = base {
            args.extend_from_slice(&["--base", b]);
        }
        if let Some(name) = branch_name {
            args.extend_from_slice(&["--name", name]);
        }
        args.push(id);

        let out = self.runner.run("gh", &args, self.cwd)?;
        if !out.success {
            bail!("Failed to create branch for issue #{id}");
        }
        let Some(branch) = parse_created_branch(&out.stdout) else {
            bail!("gh issue develop returned empty branch name for #{id}");
        };
        Ok(EnsuredBranch {
            name: branch,
            created: true,
        })
    }

    fn on_start(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
        Ok(())
    }
}

impl IssueReader for GithubIssueProvider<'_> {
    fn get_issue_detail(&self, id: &str) -> Result<IssueDetail> {
        Ok(self.get_issue(id)?.into())
    }
}

fn parse_linked_branch(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let branch = line.split('\t').next()?.trim();
        if branch.is_empty() {
            None
        } else {
            Some(branch.to_string())
        }
    })
}

fn parse_created_branch(stdout: &str) -> Option<String> {
    stdout.lines().rev().find_map(parse_created_branch_line)
}

fn parse_created_branch_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(branch) = parse_tree_url_branch(trimmed) {
        return Some(branch);
    }

    for token in trimmed.split_whitespace().rev() {
        if let Some(branch) = parse_tree_url_branch(token) {
            return Some(branch);
        }
    }

    if trimmed.contains('\t') {
        return parse_linked_branch(trimmed);
    }

    if trimmed.contains(char::is_whitespace) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_tree_url_branch(value: &str) -> Option<String> {
    let (_, branch) = value.split_once("/tree/")?;
    let branch = branch
        .split(['?', '#'])
        .next()
        .unwrap_or(branch)
        .trim_matches('/');
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
}

fn parse_created_issue_number(stdout: &str) -> Option<u32> {
    stdout
        .split_whitespace()
        .find_map(parse_created_issue_number_token)
}

fn parse_created_issue_number_token(token: &str) -> Option<u32> {
    let (_, issue) = token.split_once("/issues/")?;
    let issue = issue
        .split(['?', '#', '/', '\t', '\r', '\n'])
        .next()
        .unwrap_or(issue)
        .trim();
    if issue.is_empty() {
        None
    } else {
        issue.parse().ok()
    }
}

fn snapshot_body(body: Option<String>, url: Option<String>) -> Option<String> {
    match (body, url) {
        (Some(body), Some(url)) if !body.trim().is_empty() => Some(format!("{body}\n\nURL: {url}")),
        (Some(body), _) if !body.trim().is_empty() => Some(body),
        (_, Some(url)) => Some(format!("URL: {url}")),
        _ => None,
    }
}

fn optional_body(body: String) -> Option<String> {
    if body.trim().is_empty() {
        None
    } else {
        Some(body)
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
    fn get_issue_parses_gh_json() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"number":42,"title":"Add feature"}"#, true);
        runner.add_response(
            "alice/42-add-feature\thttps://github.com/alice/repo/tree/alice/42-add-feature\n",
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, None);
        let issue = provider.get_issue("42").unwrap();
        assert_eq!(issue.identifier, "#42");
        assert_eq!(issue.title, "Add feature");
        assert_eq!(issue.branch_name.as_deref(), Some("alice/42-add-feature"));
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
        runner.add_response(
            r#"[{"number":2,"title":"Issue 2"},{"number":3,"title":"Issue 3"}]"#,
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, Some("alice".into()));
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].display, "#1 Issue 1");
        assert_eq!(items[1].display, "#2 Issue 2");
        assert_eq!(items[2].display, "#3 Issue 3");
        assert_eq!(items[0].hint.as_deref(), Some("GitHub"));

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].1.contains(&"-a".to_string()));
        assert!(calls[0].1.contains(&"alice".to_string()));
        assert!(calls[1].1.contains(&"-A".to_string()));
        assert!(calls[1].1.contains(&"alice".to_string()));
    }

    #[test]
    fn list_issues_without_gh_user() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"[{"number":1,"title":"Issue 1"}]"#, true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let items = provider.list_issues().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].hint.as_deref(), Some("GitHub"));

        let calls = runner.calls.lock().unwrap();
        assert!(!calls[0].1.contains(&"-a".to_string()));
    }

    #[test]
    fn create_issue_parses_created_issue_url() {
        let mut runner = MockRunner::new();
        runner.add_response("https://github.com/acme/widgets/issues/123", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let issue = provider
            .create_issue(CreateIssueRequest {
                title: "Add publish".into(),
                body: "Create provider issue.".into(),
            })
            .unwrap();

        assert_eq!(issue.identifier, "#123");
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
                "--body",
                "Create provider issue."
            ]
        );
    }

    #[test]
    fn create_issue_reports_gh_failure() {
        let mut runner = MockRunner::new();
        runner.add_response_with_stderr("", "GraphQL: could not create issue", false);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let result = provider.create_issue(CreateIssueRequest {
            title: "Add publish".into(),
            body: String::new(),
        });

        let err = result.unwrap_err().to_string();
        assert!(err.contains("GitHub issue creation failed"));
        assert!(err.contains("GraphQL: could not create issue"));
    }

    #[test]
    fn create_issue_errors_when_gh_output_has_no_issue_number() {
        let mut runner = MockRunner::new();
        runner.add_response("https://github.com/acme/widgets/pulls/123", true);

        let provider = GithubIssueProvider::new(&runner, None, None);
        let result = provider.create_issue(CreateIssueRequest {
            title: "Add publish".into(),
            body: String::new(),
        });

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("did not return an issue URL")
        );
    }

    #[test]
    fn ensure_branch_returns_existing() {
        let mut runner = MockRunner::new();
        runner.add_response(
            "alice/42-add-feature\thttps://github.com/alice/repo/tree/alice/42-add-feature\n",
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", None, None).unwrap();
        assert_eq!(branch.name, "alice/42-add-feature");
        assert!(!branch.created);
    }

    #[test]
    fn ensure_branch_creates_new_without_base() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response(
            "https://github.com/alice/repo/tree/alice/42-add-feature",
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", None, None).unwrap();
        assert_eq!(branch.name, "alice/42-add-feature");
        assert!(branch.created);

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(create_call.1, vec!["issue", "develop", "42"]);
    }

    #[test]
    fn ensure_branch_creates_new_with_base() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response(
            "https://github.com/alice/repo/tree/alice/42-add-feature",
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider.ensure_branch("42", Some("develop"), None).unwrap();
        assert_eq!(branch.name, "alice/42-add-feature");
        assert!(branch.created);

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(
            create_call.1,
            vec!["issue", "develop", "--base", "develop", "42"]
        );
    }

    #[test]
    fn ensure_branch_creates_new_with_name() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response(
            "https://github.com/alice/repo/tree/alice/42-english-title",
            true,
        );

        let provider = GithubIssueProvider::new(&runner, None, None);
        let branch = provider
            .ensure_branch("42", Some("develop"), Some("alice/42-english-title"))
            .unwrap();
        assert_eq!(branch.name, "alice/42-english-title");
        assert!(branch.created);

        let calls = runner.calls.lock().unwrap();
        let create_call = &calls[1];
        assert_eq!(
            create_call.1,
            vec![
                "issue",
                "develop",
                "--base",
                "develop",
                "--name",
                "alice/42-english-title",
                "42"
            ]
        );
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
        assert!(provider.on_clean("42", "alice/42-feature").is_ok());
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
