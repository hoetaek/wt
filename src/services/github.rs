use crate::context::CommandRunner;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::Path;

const PR_JSON_FIELDS: &str = "number,title,headRefName,baseRefName,state,author";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub state: String,
    pub author: Option<PullRequestAuthor>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestAuthor {
    #[serde(default)]
    pub login: String,
}

impl PullRequest {
    pub fn author_login(&self) -> Option<&str> {
        self.author
            .as_ref()
            .map(|author| author.login.as_str())
            .filter(|login| !login.is_empty())
    }
}

pub struct GithubService<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
}

impl<'a> GithubService<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>) -> Self {
        Self { runner, cwd }
    }

    pub fn get_pr(&self, number: u32) -> Result<PullRequest> {
        let out = self.runner.run(
            "gh",
            &["pr", "view", &number.to_string(), "--json", PR_JSON_FIELDS],
            self.cwd,
        )?;
        if !out.success {
            let detail = if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            };
            bail!("Failed to fetch PR #{number}: {detail}");
        }
        let pr: PullRequest = serde_json::from_str(&out.stdout)?;
        Ok(pr)
    }

    pub fn list_prs(&self) -> Result<Vec<PullRequest>> {
        let out = self
            .runner
            .run("gh", &["pr", "list", "--json", PR_JSON_FIELDS], self.cwd)?;
        if !out.success {
            let detail = if out.stderr.is_empty() {
                &out.stdout
            } else {
                &out.stderr
            };
            bail!("Failed to fetch PR list: {detail}");
        }
        let prs: Vec<PullRequest> = serde_json::from_str(&out.stdout)?;
        Ok(prs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn get_pr_parses_json() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"number":42,"title":"Add feature","headRefName":"alice/feature","baseRefName":"main","state":"OPEN","author":{"login":"alice"}}"#,
            true,
        );

        let svc = GithubService::new(&runner, None);
        let pr = svc.get_pr(42).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.head_ref_name, "alice/feature");
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.author_login(), Some("alice"));
    }

    #[test]
    fn list_prs_parses_array() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"number":1,"title":"PR 1","headRefName":"branch-1","baseRefName":"main","state":"OPEN","author":{"login":"bob"}}]"#,
            true,
        );

        let svc = GithubService::new(&runner, None);
        let prs = svc.list_prs().unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].title, "PR 1");
        assert_eq!(prs[0].author_login(), Some("bob"));
    }

    #[test]
    fn get_pr_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("not found", false);

        let svc = GithubService::new(&runner, None);
        assert!(svc.get_pr(999).is_err());
    }

    #[test]
    fn list_prs_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("error", false);
        let svc = GithubService::new(&runner, None);
        assert!(svc.list_prs().is_err());
    }
}
