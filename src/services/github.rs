use crate::context::CommandRunner;
use anyhow::{bail, Result};
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

    pub fn get_pr(&self, number: u32) -> Result<PullRequest> { todo!() }
    pub fn list_prs(&self) -> Result<Vec<PullRequest>> { todo!() }
}
