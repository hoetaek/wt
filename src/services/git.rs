use crate::context::{CmdOutput, CommandRunner};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

static REPO_GIT_METADATA_WRITE_LOCK: Mutex<()> = Mutex::new(());
const GIT_CONFIG_LOCK_RETRY_ATTEMPTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: String,
}

/// The result of create_worktree, indicating what branch source was used.
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

    pub fn canonical_repo_root(&self) -> Result<PathBuf> {
        let current_root = self.repo_root()?;
        let git_dir = self.git_dir()?;
        let git_common_dir = self.git_common_dir()?;

        if git_dir == git_common_dir {
            return Ok(current_root);
        }

        git_common_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                anyhow::anyhow!("git common dir has no parent: {}", git_common_dir.display())
            })
    }

    fn git_dir(&self) -> Result<PathBuf> {
        let out = self.git(&["rev-parse", "--path-format=absolute", "--git-dir"])?;
        Ok(PathBuf::from(out.stdout))
    }

    fn git_common_dir(&self) -> Result<PathBuf> {
        let out = self.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
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

    pub fn worktree_add_new_branch(&self, path: &Path, branch: &str, base: &str) -> Result<()> {
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

    pub fn set_branch_parent(&self, branch: &str, parent: &str) -> Result<()> {
        if !self.local_branch_exists(parent)? {
            if self.remote_branch_exists(parent)? {
                self.git(&["branch", "--track", parent, &format!("origin/{parent}")])?;
            } else {
                self.fetch_branch(parent).ok();
                if self.remote_branch_exists(parent)? {
                    self.git(&["branch", "--track", parent, &format!("origin/{parent}")])?;
                }
            }
        }
        self.set_branch_parent_config(branch, parent)?;
        Ok(())
    }

    pub fn get_branch_parent(&self, branch: &str) -> Result<Option<String>> {
        let out = self.runner.run(
            "git",
            &["config", "--get", &format!("branch.{branch}.parentbranch")],
            self.cwd,
        )?;
        if out.success && !out.stdout.is_empty() {
            Ok(Some(out.stdout))
        } else {
            Ok(None)
        }
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
                &format!("refs/heads/{branch}"),
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
                &format!("refs/remotes/origin/{branch}"),
            ],
            self.cwd,
        )?;
        Ok(out.success)
    }

    /// Whether a remote with the given name is configured.
    ///
    /// Reads `.git/config` only (`git remote get-url`); it does not contact the
    /// remote, so it never triggers network or credential/keychain access.
    pub fn has_remote(&self, name: &str) -> Result<bool> {
        let out = self
            .runner
            .run("git", &["remote", "get-url", name], self.cwd)?;
        Ok(out.success)
    }

    pub fn branch_delete(&self, branch: &str) -> Result<CmdOutput> {
        self.runner.run("git", &["branch", "-d", branch], self.cwd)
    }

    pub fn branch_delete_force(&self, branch: &str) -> Result<()> {
        self.git(&["branch", "-D", branch])?;
        Ok(())
    }

    pub fn list_local_branches(&self) -> Result<Vec<String>> {
        let out = self.git(&["branch", "--format=%(refname:short)"])?;
        Ok(out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect())
    }

    pub fn list_remote_branches(&self) -> Result<Vec<String>> {
        let out = self.git(&[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/remotes/origin",
        ])?;
        Ok(out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect())
    }

    pub fn fetch_branch(&self, branch: &str) -> Result<()> {
        self.git(&["fetch", "origin", branch])?;
        Ok(())
    }

    pub fn set_upstream(&self, branch: &str, remote_ref: &str, cwd: &Path) -> Result<()> {
        self.runner
            .run(
                "git",
                &["branch", "--set-upstream-to", remote_ref, branch],
                Some(cwd),
            )
            .ok();
        Ok(())
    }

    /// Find the worktree path where a branch is already checked out.
    pub fn checked_out_path(&self, branch: &str) -> Result<Option<PathBuf>> {
        let entries = self.worktree_list()?;
        Ok(entries
            .into_iter()
            .find(|e| e.branch == branch)
            .map(|e| e.path))
    }

    pub fn status_porcelain(&self, cwd: &Path) -> Result<String> {
        let out = self.runner.run(
            "git",
            &["status", "--porcelain", "--untracked-files=all"],
            Some(cwd),
        )?;
        if !out.success {
            bail!(
                "git status --porcelain failed: {}",
                if out.stderr.is_empty() {
                    &out.stdout
                } else {
                    &out.stderr
                }
            );
        }
        Ok(out.stdout)
    }

    pub fn branch_has_commits_ahead(&self, parent: &str, branch: &str) -> Result<bool> {
        let range = format!("{parent}..{branch}");
        let out = self.git(&["rev-list", "--count", &range])?;
        Ok(out.stdout.trim().parse::<usize>().unwrap_or(0) > 0)
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

    fn set_branch_parent_config(&self, branch: &str, parent: &str) -> Result<()> {
        let key = format!("branch.{branch}.parentbranch");
        let _guard = REPO_GIT_METADATA_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // This guard serializes threads inside one wt process. The retry handles
        // the common cross-process case where another git config writer holds
        // .git/config.lock briefly.
        for attempt in 0..GIT_CONFIG_LOCK_RETRY_ATTEMPTS {
            let out = self
                .runner
                .run("git", &["config", &key, parent], self.cwd)?;
            if out.success {
                return Ok(());
            }

            if attempt + 1 < GIT_CONFIG_LOCK_RETRY_ATTEMPTS && is_git_config_lock_failure(&out) {
                let delay = git_config_lock_retry_delay(attempt);
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
                continue;
            }

            bail!(
                "git config {key} failed: {}",
                if out.stderr.is_empty() {
                    &out.stdout
                } else {
                    &out.stderr
                }
            );
        }

        Ok(())
    }
}

fn is_git_config_lock_failure(out: &CmdOutput) -> bool {
    let message = format!("{}\n{}", out.stdout, out.stderr).to_ascii_lowercase();
    message.contains("config.lock")
        || (message.contains("could not lock config file") && message.contains("file exists"))
}

fn git_config_lock_retry_delay(attempt: usize) -> Duration {
    if cfg!(test) {
        Duration::ZERO
    } else {
        Duration::from_millis(25 * 2_u64.pow(attempt as u32))
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
    use crate::context::{CmdOutput, CommandRunner};
    use anyhow::Result;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn parse_worktree_list_porcelain() {
        let input = "\
worktree /home/dev/sample-app
HEAD abc1234
branch refs/heads/main

worktree /home/dev/sample-app-alice-proj-680
HEAD def5678
branch refs/heads/alice/proj-680-feature

";
        let entries = parse_worktree_list(input).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, PathBuf::from("/home/dev/sample-app"));
        assert_eq!(entries[0].branch, "main");
        assert_eq!(entries[1].branch, "alice/proj-680-feature");
    }

    #[test]
    fn parse_worktree_list_detached_head_skipped() {
        let input = "\
worktree /home/dev/sample-app
HEAD abc1234
branch refs/heads/main

worktree /home/dev/sample-app-detached
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
        runner.add_response("/home/dev/sample-app", true);

        let git = GitService::new(&runner, None);
        let root = git.repo_root().unwrap();
        assert_eq!(root, PathBuf::from("/home/dev/sample-app"));
    }

    #[test]
    fn canonical_repo_root_returns_primary_worktree_when_called_inside_linked_worktree() {
        let mut runner = MockRunner::new();
        runner.add_response("/home/dev/sample-app/.claude/worktrees/wt-proj-670", true);
        runner.add_response("/home/dev/sample-app/.git/worktrees/wt-proj-670", true);
        runner.add_response("/home/dev/sample-app/.git", true);

        let cwd = PathBuf::from("/home/dev/sample-app/.claude/worktrees/wt-proj-670");
        let git = GitService::new(&runner, Some(cwd.as_path()));

        assert_eq!(
            git.canonical_repo_root().unwrap(),
            PathBuf::from("/home/dev/sample-app")
        );

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["rev-parse", "--show-toplevel"]);
        assert_eq!(
            calls[1].1,
            vec!["rev-parse", "--path-format=absolute", "--git-dir"]
        );
        assert_eq!(
            calls[2].1,
            vec!["rev-parse", "--path-format=absolute", "--git-common-dir"]
        );
    }

    #[test]
    fn canonical_repo_root_returns_current_root_for_primary_checkout() {
        let mut runner = MockRunner::new();
        runner.add_response("/home/dev/sample-app", true);
        runner.add_response("/home/dev/sample-app/.git", true);
        runner.add_response("/home/dev/sample-app/.git", true);

        let git = GitService::new(&runner, None);

        assert_eq!(
            git.canonical_repo_root().unwrap(),
            PathBuf::from("/home/dev/sample-app")
        );

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["rev-parse", "--show-toplevel"]);
        assert_eq!(
            calls[1].1,
            vec!["rev-parse", "--path-format=absolute", "--git-dir"]
        );
        assert_eq!(
            calls[2].1,
            vec!["rev-parse", "--path-format=absolute", "--git-common-dir"]
        );
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
worktree /home/dev/sample-app
HEAD abc
branch refs/heads/main

worktree /home/dev/sample-app-alice-proj-680
HEAD def
branch refs/heads/alice/proj-680

";
        runner.add_response(porcelain, true);

        let git = GitService::new(&runner, None);
        let path = git.checked_out_path("alice/proj-680").unwrap();
        assert_eq!(
            path,
            Some(PathBuf::from("/home/dev/sample-app-alice-proj-680"))
        );
    }

    #[test]
    fn checked_out_path_returns_none_when_not_found() {
        let mut runner = MockRunner::new();
        let porcelain = "\
worktree /home/dev/sample-app
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

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["show-ref", "--verify", "--quiet", "refs/heads/main"]
        );
    }

    #[test]
    fn remote_branch_exists_checks_exit_code() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let git = GitService::new(&runner, None);
        assert!(git.remote_branch_exists("feature").unwrap());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "show-ref",
                "--verify",
                "--quiet",
                "refs/remotes/origin/feature"
            ]
        );
    }

    #[test]
    fn fetch_runs_git_fetch_origin() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let git = GitService::new(&runner, None);
        git.fetch().unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["fetch", "origin"]);
    }

    #[test]
    fn fetch_branch_runs_git_fetch_origin_branch() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let git = GitService::new(&runner, None);
        git.fetch_branch("alice/feature").unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["fetch", "origin", "alice/feature"]);
    }

    #[test]
    fn has_remote_reads_local_config_via_get_url() {
        let mut runner = MockRunner::new();
        runner.add_response("https://example.com/repo.git\n", true);
        let git = GitService::new(&runner, None);
        assert!(git.has_remote("origin").unwrap());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["remote", "get-url", "origin"]);
    }

    #[test]
    fn has_remote_returns_false_when_remote_absent() {
        let mut runner = MockRunner::new();
        runner.add_response("", false);
        let git = GitService::new(&runner, None);
        assert!(!git.has_remote("origin").unwrap());
    }

    #[test]
    fn list_local_branches_parses_output() {
        let mut runner = MockRunner::new();
        runner.add_response("main\ndevelop\nfeature", true);
        let git = GitService::new(&runner, None);
        let branches = git.list_local_branches().unwrap();
        assert_eq!(branches, vec!["main", "develop", "feature"]);
    }

    #[test]
    fn list_remote_branches_parses_origin_refs() {
        let mut runner = MockRunner::new();
        runner.add_response("origin/HEAD\norigin/main\norigin/alice/feature\n", true);
        let git = GitService::new(&runner, None);
        let branches = git.list_remote_branches().unwrap();
        assert_eq!(
            branches,
            vec!["origin/HEAD", "origin/main", "origin/alice/feature"]
        );

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec![
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/remotes/origin"
            ]
        );
    }

    #[test]
    fn worktree_add_passes_correct_args() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let git = GitService::new(&runner, None);
        git.worktree_add(std::path::Path::new("/tmp/wt"), "main")
            .unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["worktree", "add", "/tmp/wt", "main"]);
    }

    #[test]
    fn worktree_add_new_branch_passes_correct_args() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let git = GitService::new(&runner, None);
        git.worktree_add_new_branch(std::path::Path::new("/tmp/wt"), "feature", "main")
            .unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["worktree", "add", "-b", "feature", "/tmp/wt", "main"]
        );
    }

    #[test]
    fn branch_delete_returns_cmd_output() {
        let mut runner = MockRunner::new();
        runner.add_response("Deleted branch feature", true);
        let git = GitService::new(&runner, None);
        let out = git.branch_delete("feature").unwrap();
        assert!(out.success);
    }

    #[test]
    fn set_branch_parent_writes_parentbranch_config() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("", true);

        let git = GitService::new(&runner, None);
        git.set_branch_parent("alice/feature", "main").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["show-ref", "--verify", "--quiet", "refs/heads/main"]
        );
        assert_eq!(
            calls[1].1,
            vec!["config", "branch.alice/feature.parentbranch", "main"]
        );
    }

    #[test]
    fn set_branch_parent_retries_git_config_lock_failure() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response(
            "error: could not lock config file .git/config: File exists",
            false,
        );
        runner.add_response("", true);

        let git = GitService::new(&runner, None);
        git.set_branch_parent("alice/feature", "main").unwrap();

        let calls = runner.calls.lock().unwrap();
        let config_calls = calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "git" && args.first().is_some_and(|arg| arg == "config")
            })
            .count();
        assert_eq!(config_calls, 2);
    }

    #[test]
    fn set_branch_parent_config_writes_are_process_serialized() {
        struct SlowConfigRunner {
            active_config_writes: AtomicUsize,
            overlapped: AtomicBool,
        }

        impl CommandRunner for SlowConfigRunner {
            fn run(&self, cmd: &str, args: &[&str], _cwd: Option<&Path>) -> Result<CmdOutput> {
                assert_eq!(cmd, "git");
                if args.first() == Some(&"config") {
                    if self.active_config_writes.fetch_add(1, Ordering::SeqCst) > 0 {
                        self.overlapped.store(true, Ordering::SeqCst);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                    self.active_config_writes.fetch_sub(1, Ordering::SeqCst);
                }

                Ok(CmdOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                    success: true,
                })
            }

            fn has_command(&self, _cmd: &str) -> bool {
                false
            }
        }

        let runner = SlowConfigRunner {
            active_config_writes: AtomicUsize::new(0),
            overlapped: AtomicBool::new(false),
        };

        std::thread::scope(|scope| {
            scope.spawn(|| {
                let git = GitService::new(&runner, None);
                git.set_branch_parent("alice/feature-a", "main").unwrap();
            });
            scope.spawn(|| {
                let git = GitService::new(&runner, None);
                git.set_branch_parent("alice/feature-b", "main").unwrap();
            });
        });

        assert!(!runner.overlapped.load(Ordering::SeqCst));
    }

    #[test]
    fn status_porcelain_requests_full_untracked_paths() {
        let mut runner = MockRunner::new();
        runner.add_response("?? .codex/skills", true);
        let cwd = PathBuf::from("/home/dev/sample-app-worktree");
        let git = GitService::new(&runner, None);

        assert_eq!(git.status_porcelain(&cwd).unwrap(), "?? .codex/skills");

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["status", "--porcelain", "--untracked-files=all"]
        );
        assert_eq!(calls[0].2.as_ref(), Some(&cwd));
    }

    #[test]
    fn get_branch_parent_returns_value() {
        let mut runner = MockRunner::new();
        runner.add_response("develop", true);
        let git = GitService::new(&runner, None);
        let parent = git.get_branch_parent("alice/proj-680").unwrap();
        assert_eq!(parent, Some("develop".into()));
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["config", "--get", "branch.alice/proj-680.parentbranch"]
        );
    }

    #[test]
    fn get_branch_parent_returns_none_when_missing() {
        let mut runner = MockRunner::new();
        runner.add_response("", false);
        let git = GitService::new(&runner, None);
        let parent = git.get_branch_parent("alice/feature").unwrap();
        assert!(parent.is_none());
    }
}
