use crate::context::CommandRunner;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u32,
    pub title: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub state: String,
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
            &[
                "pr",
                "view",
                &number.to_string(),
                "--json",
                "number,title,headRefName,baseRefName,state",
            ],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch PR #{number}");
        }
        let pr: PullRequest = serde_json::from_str(&out.stdout)?;
        Ok(pr)
    }

    pub fn list_prs(&self) -> Result<Vec<PullRequest>> {
        let out = self.runner.run(
            "gh",
            &[
                "pr",
                "list",
                "--json",
                "number,title,headRefName,baseRefName,state",
            ],
            self.cwd,
        )?;
        if !out.success {
            bail!("Failed to fetch PR list");
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
            r#"{"number":42,"title":"Add feature","headRefName":"hoetaek/feature","baseRefName":"main","state":"OPEN"}"#,
            true,
        );

        let svc = GithubService::new(&runner, None);
        let pr = svc.get_pr(42).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.head_ref_name, "hoetaek/feature");
        assert_eq!(pr.state, "OPEN");
    }

    #[test]
    fn list_prs_parses_array() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"number":1,"title":"PR 1","headRefName":"branch-1","baseRefName":"main","state":"OPEN"}]"#,
            true,
        );

        let svc = GithubService::new(&runner, None);
        let prs = svc.list_prs().unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].title, "PR 1");
    }

    #[test]
    fn get_pr_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("not found", false);

        let svc = GithubService::new(&runner, None);
        assert!(svc.get_pr(999).is_err());
    }
}
