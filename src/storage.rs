use crate::context::CommandRunner;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

const GIT_COMMON_DIR_ARGS: &[&str] = &["rev-parse", "--path-format=absolute", "--git-common-dir"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRoot {
    git_common_dir: PathBuf,
    personal_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyLocalStorage {
    path: PathBuf,
    canonical_root: PathBuf,
}

impl StorageRoot {
    pub fn resolve(runner: &dyn CommandRunner, cwd: Option<&Path>) -> Result<Self> {
        let out = runner
            .run("git", GIT_COMMON_DIR_ARGS, cwd)
            .context("Failed to run git rev-parse --git-common-dir")?;
        if !out.success {
            bail!(
                "Failed to resolve wt personal storage with `git rev-parse --git-common-dir`: {}",
                command_error(&out.stdout, &out.stderr)
            );
        }

        let git_common_dir = out.stdout.trim();
        if git_common_dir.is_empty() {
            bail!("Failed to resolve wt personal storage: git common dir was empty");
        }

        Ok(Self::from_git_common_dir(git_common_dir))
    }

    pub fn from_git_common_dir(git_common_dir: impl Into<PathBuf>) -> Self {
        let git_common_dir = git_common_dir.into();
        let personal_root = git_common_dir.join("wt");
        Self {
            git_common_dir,
            personal_root,
        }
    }

    pub fn git_common_dir(&self) -> &Path {
        &self.git_common_dir
    }

    pub fn personal_root(&self) -> &Path {
        &self.personal_root
    }

    pub fn config_toml(&self) -> PathBuf {
        self.personal_root.join("config.toml")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.personal_root.join("profiles")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.personal_root.join("tasks")
    }

    pub fn workflows_dir(&self) -> PathBuf {
        self.personal_root.join("workflows")
    }

    pub fn task_runs_dir(&self) -> PathBuf {
        self.personal_root.join("task-runs")
    }

    pub fn ideas_dir(&self) -> PathBuf {
        self.personal_root.join("ideas")
    }

    pub fn retrospectives_dir(&self) -> PathBuf {
        self.personal_root.join("retrospectives")
    }

    pub fn task_run_path(&self, id: impl AsRef<str>) -> PathBuf {
        let id = id.as_ref();
        let file_name = if id.ends_with(".toml") {
            id.to_string()
        } else {
            format!("{id}.toml")
        };
        self.task_runs_dir().join(file_name)
    }

    pub fn messages_dir(&self) -> PathBuf {
        self.personal_root.join("messages")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.personal_root.join("worktrees")
    }

    pub fn worktree_dir(&self, id: impl AsRef<Path>) -> PathBuf {
        self.worktrees_dir().join(id)
    }

    pub fn detect_legacy_local(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        let path = repo_root.as_ref().join(".local");
        if path.is_dir() {
            Some(LegacyLocalStorage {
                path,
                canonical_root: self.personal_root.clone(),
            })
        } else {
            None
        }
    }

    pub fn detect_legacy_config(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        let path = repo_root.as_ref().join(".local/.wt.toml");
        if path.is_file() {
            Some(LegacyLocalStorage {
                path,
                canonical_root: self.config_toml(),
            })
        } else {
            None
        }
    }

    pub fn detect_legacy_profiles(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "profiles")
    }

    pub fn detect_legacy_tasks(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "tasks")
    }

    pub fn detect_legacy_task_runs(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "task-runs")
    }

    pub fn detect_legacy_workflows(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "workflows")
    }

    pub fn detect_legacy_ideas(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "ideas")
    }

    pub fn detect_legacy_retrospectives(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "retrospectives")
    }

    fn detect_legacy_child(
        &self,
        repo_root: impl AsRef<Path>,
        child: &str,
    ) -> Option<LegacyLocalStorage> {
        let path = repo_root.as_ref().join(".local").join(child);
        if path.is_dir() {
            Some(LegacyLocalStorage {
                path,
                canonical_root: self.personal_root.join(child),
            })
        } else {
            None
        }
    }

    pub fn display_path(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.git_common_dir) {
            let relative = relative.to_string_lossy();
            if relative.is_empty() {
                "<git-common-dir>".into()
            } else {
                format!("<git-common-dir>/{relative}")
            }
        } else {
            path.display().to_string()
        }
    }
}

impl LegacyLocalStorage {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }

    pub fn error_message(&self) -> String {
        format!(
            "Found legacy repo-root .local storage at {}. Canonical wt personal storage is {}. wt does not silently fall back to .local; import or repair legacy state explicitly before using this command.",
            self.path.display(),
            self.canonical_root.display()
        )
    }

    pub fn error_message_for(&self, state_name: &str) -> String {
        format!(
            "Found legacy repo-root {state_name} at {}. Canonical wt personal {state_name} is {}. wt does not silently fall back to .local; import or repair legacy state explicitly before using this command.",
            self.path.display(),
            self.canonical_root.display()
        )
    }
}

fn command_error(stdout: &str, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{CmdOutput, CommandRunner};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    const GIT_LOCAL_ENV_KEYS: &[&str] = &[
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ];

    struct CleanGitRunner;

    impl CommandRunner for CleanGitRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            let mut command = clean_command(cmd);
            command.args(args);
            if let Some(cwd) = cwd {
                command.current_dir(cwd);
            }
            let output = command.output()?;
            Ok(CmdOutput {
                stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                success: output.status.success(),
            })
        }

        fn has_command(&self, cmd: &str) -> bool {
            clean_command(cmd)
                .arg("--version")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
    }

    #[test]
    fn exposes_typed_personal_state_paths() {
        let git_common_dir = PathBuf::from("/repo/.git");
        let storage = StorageRoot::from_git_common_dir(&git_common_dir);

        assert_eq!(storage.git_common_dir(), git_common_dir.as_path());
        assert_eq!(storage.personal_root(), Path::new("/repo/.git/wt"));
        assert_eq!(
            storage.config_toml(),
            PathBuf::from("/repo/.git/wt/config.toml")
        );
        assert_eq!(
            storage.profiles_dir(),
            PathBuf::from("/repo/.git/wt/profiles")
        );
        assert_eq!(storage.tasks_dir(), PathBuf::from("/repo/.git/wt/tasks"));
        assert_eq!(
            storage.workflows_dir(),
            PathBuf::from("/repo/.git/wt/workflows")
        );
        assert_eq!(
            storage.task_runs_dir(),
            PathBuf::from("/repo/.git/wt/task-runs")
        );
        assert_eq!(
            storage.messages_dir(),
            PathBuf::from("/repo/.git/wt/messages")
        );
        assert_eq!(storage.ideas_dir(), PathBuf::from("/repo/.git/wt/ideas"));
        assert_eq!(
            storage.retrospectives_dir(),
            PathBuf::from("/repo/.git/wt/retrospectives")
        );
        assert_eq!(
            storage.worktrees_dir(),
            PathBuf::from("/repo/.git/wt/worktrees")
        );
        assert_eq!(
            storage.worktree_dir("wt_123"),
            PathBuf::from("/repo/.git/wt/worktrees/wt_123")
        );
    }

    #[test]
    fn main_and_linked_worktrees_resolve_the_same_personal_root() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let linked = temp.path().join("linked");
        fs::create_dir(&repo).unwrap();
        init_repo(&repo);

        run_git(
            &repo,
            &["worktree", "add", "-b", "linked", path_str(&linked), "HEAD"],
        );

        let runner = CleanGitRunner;
        let main_storage = StorageRoot::resolve(&runner, Some(&repo)).unwrap();
        let linked_storage = StorageRoot::resolve(&runner, Some(&linked)).unwrap();
        let expected_personal_root = fs::canonicalize(repo.join(".git")).unwrap().join("wt");

        assert_eq!(
            main_storage.personal_root(),
            expected_personal_root.as_path()
        );
        assert_eq!(
            linked_storage.personal_root(),
            expected_personal_root.as_path()
        );
        assert_eq!(main_storage, linked_storage);
    }

    #[test]
    fn detects_legacy_local_without_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(repo.join(".local")).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        let legacy = storage.detect_legacy_local(&repo).unwrap();

        assert_eq!(legacy.path(), repo.join(".local").as_path());
        assert_eq!(legacy.canonical_root(), repo.join(".git/wt").as_path());
        assert!(
            legacy
                .error_message()
                .contains("does not silently fall back")
        );
    }

    #[test]
    fn ignores_missing_legacy_local_storage() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        assert_eq!(storage.detect_legacy_local(&repo), None);
    }

    #[test]
    fn resolve_reports_git_failure_without_local_fallback() {
        struct FailingRunner;
        impl CommandRunner for FailingRunner {
            fn run(&self, _cmd: &str, _args: &[&str], _cwd: Option<&Path>) -> Result<CmdOutput> {
                Ok(CmdOutput {
                    stdout: String::new(),
                    stderr: "fatal: not a git repository".into(),
                    success: false,
                })
            }

            fn has_command(&self, _cmd: &str) -> bool {
                true
            }
        }

        let err = StorageRoot::resolve(&FailingRunner, None).unwrap_err();

        assert!(err.to_string().contains("git rev-parse --git-common-dir"));
        assert!(err.to_string().contains("fatal: not a git repository"));
    }

    fn init_repo(repo: &Path) {
        run_git(repo, &["init"]);
        fs::write(repo.join("README.md"), "sample\n").unwrap();
        run_git(repo, &["add", "README.md"]);
        run_git(
            repo,
            &[
                "-c",
                "user.name=wt test",
                "-c",
                "user.email=wt@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = clean_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn clean_command(cmd: &str) -> Command {
        let mut command = Command::new(cmd);
        for key in GIT_LOCAL_ENV_KEYS {
            command.env_remove(key);
        }
        command
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().unwrap()
    }
}
