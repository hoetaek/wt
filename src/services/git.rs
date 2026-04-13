use crate::context::{CmdOutput, CommandRunner};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: String,
}

/// The result of create_worktree, indicating what kind of branch was used.
#[derive(Debug, PartialEq)]
pub enum CreateType {
    Local,
    Remote,
    New,
}

pub struct GitService<'a> {
    runner: &'a dyn CommandRunner,
    cwd: Option<&'a Path>,
}

impl<'a> GitService<'a> {
    pub fn new(runner: &'a dyn CommandRunner, cwd: Option<&'a Path>) -> Self {
        Self { runner, cwd }
    }

    pub fn repo_root(&self) -> Result<PathBuf> {
        let out = self.git(&["rev-parse", "--show-toplevel"])?;
        Ok(PathBuf::from(out.stdout))
    }

    pub fn current_branch(&self) -> Result<String> {
        let out = self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        Ok(out.stdout)
    }

    pub fn fetch(&self) -> Result<()> {
        self.git(&["fetch", "origin"])?;
        Ok(())
    }

    pub fn worktree_list(&self) -> Result<Vec<WorktreeEntry>> {
        let out = self.git(&["worktree", "list", "--porcelain"])?;
        parse_worktree_list(&out.stdout)
    }

    pub fn worktree_add(&self, path: &Path, branch: &str) -> Result<()> {
        self.git(&["worktree", "add", &path.to_string_lossy(), branch])?;
        Ok(())
    }

    pub fn worktree_add_new_branch(
        &self,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<()> {
        self.git(&[
            "worktree",
            "add",
            "-b",
            branch,
            &path.to_string_lossy(),
            base,
        ])?;
        Ok(())
    }

    pub fn worktree_remove(&self, path: &Path) -> Result<CmdOutput> {
        self.runner.run(
            "git",
            &["worktree", "remove", &path.to_string_lossy()],
            self.cwd,
        )
    }

    pub fn worktree_remove_force(&self, path: &Path) -> Result<()> {
        self.git(&["worktree", "remove", &path.to_string_lossy(), "--force"])?;
        Ok(())
    }

    pub fn local_branch_exists(&self, branch: &str) -> Result<bool> {
        let out = self.runner.run(
            "git",
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{}", branch),
            ],
            self.cwd,
        )?;
        Ok(out.success)
    }

    pub fn remote_branch_exists(&self, branch: &str) -> Result<bool> {
        let out = self.runner.run(
            "git",
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/remotes/origin/{}", branch),
            ],
            self.cwd,
        )?;
        Ok(out.success)
    }

    pub fn branch_delete(&self, branch: &str) -> Result<CmdOutput> {
        self.runner.run("git", &["branch", "-d", branch], self.cwd)
    }

    pub fn branch_delete_force(&self, branch: &str) -> Result<()> {
        self.git(&["branch", "-D", branch])?;
        Ok(())
    }

    /// Find the worktree path where a branch is already checked out.
    pub fn checked_out_path(&self, branch: &str) -> Result<Option<PathBuf>> {
        let entries = self.worktree_list()?;
        Ok(entries.into_iter().find(|e| e.branch == branch).map(|e| e.path))
    }

    fn git(&self, args: &[&str]) -> Result<CmdOutput> {
        let out = self.runner.run("git", args, self.cwd)?;
        if !out.success {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                if out.stderr.is_empty() {
                    &out.stdout
                } else {
                    &out.stderr
                }
            );
        }
        Ok(out)
    }
}

fn parse_worktree_list(porcelain: &str) -> Result<Vec<WorktreeEntry>> {
    let mut entries = Vec::new();
    let mut current_path: Option<PathBuf> = None;

    for line in porcelain.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path_str));
        } else if let Some(ref_str) = line.strip_prefix("branch refs/heads/") {
            if let Some(path) = current_path.take() {
                entries.push(WorktreeEntry {
                    path,
                    branch: ref_str.to_string(),
                });
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn parse_worktree_list_porcelain() {
        let input = "\
worktree /home/dev/hapjeong
HEAD abc1234
branch refs/heads/main

worktree /home/dev/hapjeong-hoetaek-tech-680
HEAD def5678
branch refs/heads/hoetaek/tech-680-feature

";
        let entries = parse_worktree_list(input).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, PathBuf::from("/home/dev/hapjeong"));
        assert_eq!(entries[0].branch, "main");
        assert_eq!(entries[1].branch, "hoetaek/tech-680-feature");
    }

    #[test]
    fn parse_worktree_list_detached_head_skipped() {
        let input = "\
worktree /home/dev/hapjeong
HEAD abc1234
branch refs/heads/main

worktree /home/dev/hapjeong-detached
HEAD def5678
detached

";
        let entries = parse_worktree_list(input).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].branch, "main");
    }

    #[test]
    fn repo_root_returns_path() {
        let mut runner = MockRunner::new();
        runner.add_response("/home/dev/hapjeong", true);

        let git = GitService::new(&runner, None);
        let root = git.repo_root().unwrap();
        assert_eq!(root, PathBuf::from("/home/dev/hapjeong"));
    }

    #[test]
    fn git_command_failure_returns_error() {
        let mut runner = MockRunner::new();
        runner.add_response("fatal: not a git repository", false);

        let git = GitService::new(&runner, None);
        assert!(git.repo_root().is_err());
    }

    #[test]
    fn checked_out_path_finds_branch() {
        let mut runner = MockRunner::new();
        let porcelain = "\
worktree /home/dev/hapjeong
HEAD abc
branch refs/heads/main

worktree /home/dev/hapjeong-hoetaek-tech-680
HEAD def
branch refs/heads/hoetaek/tech-680

";
        runner.add_response(porcelain, true);

        let git = GitService::new(&runner, None);
        let path = git.checked_out_path("hoetaek/tech-680").unwrap();
        assert_eq!(
            path,
            Some(PathBuf::from("/home/dev/hapjeong-hoetaek-tech-680"))
        );
    }

    #[test]
    fn checked_out_path_returns_none_when_not_found() {
        let mut runner = MockRunner::new();
        let porcelain = "\
worktree /home/dev/hapjeong
HEAD abc
branch refs/heads/main

";
        runner.add_response(porcelain, true);

        let git = GitService::new(&runner, None);
        let path = git.checked_out_path("nonexistent").unwrap();
        assert!(path.is_none());
    }

    #[test]
    fn local_branch_exists_checks_exit_code() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let git = GitService::new(&runner, None);
        assert!(git.local_branch_exists("main").unwrap());

        let calls = runner.calls.borrow();
        assert_eq!(
            calls[0].1,
            vec!["show-ref", "--verify", "--quiet", "refs/heads/main"]
        );
    }
}
