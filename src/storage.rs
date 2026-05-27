use crate::context::CommandRunner;
use crate::messages::AgentId;
use anyhow::{Context, Result, bail};
use std::path::{Component, Path, PathBuf};

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
    pub fn resolve(
        runner: &dyn CommandRunner,
        cwd: Option<&Path>,
        repo_root: impl AsRef<Path>,
    ) -> Result<Self> {
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

        Ok(Self::from_git_common_dir_and_repo_root(
            git_common_dir,
            repo_root,
        ))
    }

    pub fn from_git_common_dir(git_common_dir: impl Into<PathBuf>) -> Self {
        let git_common_dir = git_common_dir.into();
        let repo_root = repo_root_from_git_common_dir(&git_common_dir);
        Self::from_git_common_dir_and_repo_root(git_common_dir, repo_root)
    }

    pub fn from_git_common_dir_and_repo_root(
        git_common_dir: impl Into<PathBuf>,
        repo_root: impl AsRef<Path>,
    ) -> Self {
        let git_common_dir = git_common_dir.into();
        let personal_root = repo_root.as_ref().join(".wt");
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

    pub fn config_dir(&self) -> PathBuf {
        self.personal_root.join("config")
    }

    pub fn config_toml(&self) -> PathBuf {
        self.config_dir().join("local.toml")
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.config_dir().join("profiles")
    }

    pub fn planning_dir(&self) -> PathBuf {
        self.personal_root.join("planning")
    }

    pub fn execution_dir(&self) -> PathBuf {
        self.personal_root.join("execution")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.execution_dir().join("tasks")
    }

    pub fn workflows_dir(&self) -> PathBuf {
        self.execution_dir().join("workflows")
    }

    pub fn task_runs_dir(&self) -> PathBuf {
        self.execution_dir().join("task-runs")
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.execution_dir().join("archive")
    }

    pub fn archive_workflows_dir(&self) -> PathBuf {
        self.archive_dir().join("workflows")
    }

    pub fn runtime_dir(&self) -> PathBuf {
        self.personal_root.join("runtime")
    }

    pub fn runtime_agents_dir(&self) -> PathBuf {
        self.runtime_dir().join("agents")
    }

    pub fn runtime_agent_dir(&self, agent_id: &AgentId) -> PathBuf {
        agent_id.runtime_dir(&self.runtime_dir())
    }

    pub fn runtime_agent_observations_dir(&self, agent_id: &AgentId) -> PathBuf {
        self.runtime_agent_dir(agent_id).join("observations")
    }

    pub fn wait_observations_jsonl(&self, agent_id: &AgentId) -> PathBuf {
        self.runtime_agent_observations_dir(agent_id)
            .join(crate::agent_state::WAIT_OBSERVATIONS_FILE)
    }

    pub fn runtime_agent_anchors_dir(&self, agent_id: &AgentId) -> PathBuf {
        self.runtime_agent_dir(agent_id).join("anchors")
    }

    pub fn workflow_archive_dir(&self, id: impl AsRef<str>) -> PathBuf {
        self.archive_workflows_dir().join(id.as_ref())
    }

    pub fn ideas_dir(&self) -> PathBuf {
        self.planning_dir().join("ideas")
    }

    pub fn specs_dir(&self) -> PathBuf {
        self.planning_dir().join("specs")
    }

    pub fn retrospectives_dir(&self) -> PathBuf {
        self.planning_dir().join("retrospectives")
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

    pub fn legacy_messages_dir(&self) -> PathBuf {
        self.personal_root.join("messages")
    }

    pub fn legacy_agent_state_dir(&self) -> PathBuf {
        self.personal_root.join("agent.state")
    }

    pub fn legacy_sessions_dir(&self) -> PathBuf {
        self.personal_root.join("sessions")
    }

    pub fn detect_legacy_local(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        let path = repo_root.as_ref().join(".local");
        if !path.is_dir() || !legacy_local_contains_wt_state(&path) {
            return None;
        }
        Some(LegacyLocalStorage {
            path,
            canonical_root: self.personal_root.clone(),
        })
    }

    pub fn detect_legacy_config(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_personal_file("config.toml", self.config_toml())
            .or_else(|| {
                let path = repo_root.as_ref().join(".local/.wt.toml");
                path.is_file().then_some(LegacyLocalStorage {
                    path,
                    canonical_root: self.config_toml(),
                })
            })
    }

    pub fn detect_legacy_profiles(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "profiles", self.profiles_dir())
    }

    pub fn detect_legacy_tasks(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "tasks", self.tasks_dir())
    }

    pub fn detect_legacy_task_runs(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "task-runs", self.task_runs_dir())
    }

    pub fn detect_legacy_workflows(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "workflows", self.workflows_dir())
    }

    pub fn detect_legacy_ideas(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "ideas", self.ideas_dir())
    }

    pub fn detect_legacy_specs(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "specs", self.specs_dir())
    }

    pub fn detect_legacy_retrospectives(
        &self,
        repo_root: impl AsRef<Path>,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "retrospectives", self.retrospectives_dir())
    }

    pub fn detect_legacy_archive(&self, repo_root: impl AsRef<Path>) -> Option<LegacyLocalStorage> {
        self.detect_legacy_child(repo_root, "archive", self.archive_dir())
    }

    pub fn detect_legacy_messages(&self) -> Option<LegacyLocalStorage> {
        self.legacy_messages_dir()
            .is_dir()
            .then_some(LegacyLocalStorage {
                path: self.legacy_messages_dir(),
                canonical_root: self.runtime_agents_dir(),
            })
    }

    pub fn detect_legacy_agent_state(&self) -> Option<LegacyLocalStorage> {
        self.legacy_agent_state_dir()
            .is_dir()
            .then_some(LegacyLocalStorage {
                path: self.legacy_agent_state_dir(),
                canonical_root: self.runtime_agents_dir(),
            })
    }

    pub fn detect_legacy_sessions(&self) -> Option<LegacyLocalStorage> {
        self.legacy_sessions_dir()
            .is_dir()
            .then_some(LegacyLocalStorage {
                path: self.legacy_sessions_dir(),
                canonical_root: self.runtime_agents_dir(),
            })
    }

    fn detect_legacy_child(
        &self,
        repo_root: impl AsRef<Path>,
        child: &str,
        canonical_root: PathBuf,
    ) -> Option<LegacyLocalStorage> {
        self.detect_legacy_personal_dir(child, canonical_root.clone())
            .or_else(|| {
                let path = repo_root.as_ref().join(".local").join(child);
                path.is_dir().then_some(LegacyLocalStorage {
                    path,
                    canonical_root,
                })
            })
    }

    fn detect_legacy_personal_dir(
        &self,
        child: &str,
        canonical_root: PathBuf,
    ) -> Option<LegacyLocalStorage> {
        let path = self.personal_root.join(child);
        path.is_dir().then_some(LegacyLocalStorage {
            path,
            canonical_root,
        })
    }

    fn detect_legacy_personal_file(
        &self,
        child: &str,
        canonical_root: PathBuf,
    ) -> Option<LegacyLocalStorage> {
        let path = self.personal_root.join(child);
        path.is_file().then_some(LegacyLocalStorage {
            path,
            canonical_root,
        })
    }

    pub fn display_path(&self, path: &Path) -> String {
        if let Ok(relative) = path.strip_prefix(&self.personal_root) {
            let relative = relative.to_string_lossy();
            if relative.is_empty() {
                "<repo-root>/.wt".into()
            } else {
                format!("<repo-root>/.wt/{relative}")
            }
        } else {
            path.display().to_string()
        }
    }
}

fn repo_root_from_git_common_dir(git_common_dir: &Path) -> PathBuf {
    git_common_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| git_common_dir.to_path_buf())
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
            "Found legacy wt personal {state_name} at {}. Canonical wt personal {state_name} is {}. wt does not silently fall back to legacy storage; import or repair legacy state explicitly before using this command.",
            self.path.display(),
            self.canonical_root.display()
        )
    }
}

pub(crate) fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::Prefix(_)) | Some(Component::RootDir) => {}
                _ => normalized.push(component.as_os_str()),
            },
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

fn legacy_local_contains_wt_state(path: &Path) -> bool {
    path.join(".wt.toml").is_file()
        || [
            "profiles",
            "ideas",
            "specs",
            "retrospectives",
            "tasks",
            "workflows",
            "task-runs",
            "archive",
        ]
        .iter()
        .any(|child| path.join(child).is_dir())
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
        assert_eq!(storage.personal_root(), Path::new("/repo/.wt"));
        assert_eq!(
            storage.config_toml(),
            PathBuf::from("/repo/.wt/config/local.toml")
        );
        assert_eq!(
            storage.profiles_dir(),
            PathBuf::from("/repo/.wt/config/profiles")
        );
        assert_eq!(
            storage.ideas_dir(),
            PathBuf::from("/repo/.wt/planning/ideas")
        );
        assert_eq!(
            storage.specs_dir(),
            PathBuf::from("/repo/.wt/planning/specs")
        );
        assert_eq!(
            storage.retrospectives_dir(),
            PathBuf::from("/repo/.wt/planning/retrospectives")
        );
        assert_eq!(
            storage.tasks_dir(),
            PathBuf::from("/repo/.wt/execution/tasks")
        );
        assert_eq!(
            storage.workflows_dir(),
            PathBuf::from("/repo/.wt/execution/workflows")
        );
        assert_eq!(
            storage.task_runs_dir(),
            PathBuf::from("/repo/.wt/execution/task-runs")
        );
        assert_eq!(
            storage.archive_dir(),
            PathBuf::from("/repo/.wt/execution/archive")
        );
        assert_eq!(storage.runtime_dir(), PathBuf::from("/repo/.wt/runtime"));
        assert_eq!(
            storage.runtime_agents_dir(),
            PathBuf::from("/repo/.wt/runtime/agents")
        );
        assert_eq!(
            storage.legacy_agent_state_dir(),
            PathBuf::from("/repo/.wt/agent.state")
        );
        assert_eq!(
            storage.legacy_sessions_dir(),
            PathBuf::from("/repo/.wt/sessions")
        );
        let agent = AgentId::parse("agents/codex").unwrap();
        assert_eq!(
            storage.runtime_agent_dir(&agent),
            PathBuf::from("/repo/.wt/runtime/agents/codex")
        );
        assert_eq!(
            storage.runtime_agent_observations_dir(&agent),
            PathBuf::from("/repo/.wt/runtime/agents/codex/observations")
        );
        assert_eq!(
            storage.wait_observations_jsonl(&agent),
            PathBuf::from("/repo/.wt/runtime/agents/codex/observations/wait-observations.jsonl")
        );
        assert_eq!(
            storage.runtime_agent_anchors_dir(&agent),
            PathBuf::from("/repo/.wt/runtime/agents/codex/anchors")
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
        let main_storage = StorageRoot::resolve(&runner, Some(&repo), &repo).unwrap();
        let linked_storage = StorageRoot::resolve(&runner, Some(&linked), &repo).unwrap();
        let expected_personal_root = repo.join(".wt");

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
    fn display_path_uses_repo_root_wt_placeholder() {
        let storage = StorageRoot::from_git_common_dir("/repo/.git");

        assert_eq!(
            storage.display_path(Path::new("/repo/.wt/execution/tasks/demo.toml")),
            "<repo-root>/.wt/execution/tasks/demo.toml"
        );
        assert_eq!(
            storage.display_path(Path::new("/repo/.wt")),
            "<repo-root>/.wt"
        );
        assert_eq!(
            storage.display_path(Path::new("/repo/.git/wt/execution/tasks/demo.toml")),
            "/repo/.git/wt/execution/tasks/demo.toml"
        );
    }

    #[test]
    fn detects_legacy_local_without_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        fs::create_dir_all(repo.join(".local/tasks")).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        let legacy = storage.detect_legacy_local(&repo).unwrap();

        assert_eq!(legacy.path(), repo.join(".local").as_path());
        assert_eq!(legacy.canonical_root(), repo.join(".wt").as_path());
        assert!(
            legacy
                .error_message()
                .contains("does not silently fall back")
        );
    }

    #[test]
    fn ignores_missing_or_non_wt_legacy_local_storage() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        assert_eq!(storage.detect_legacy_local(&repo), None);
        fs::create_dir(repo.join(".local")).unwrap();
        fs::create_dir(repo.join(".local/cache")).unwrap();
        fs::write(repo.join(".local/README"), "project-local files\n").unwrap();
        assert_eq!(storage.detect_legacy_local(&repo), None);
    }

    #[test]
    fn normalizes_paths_lexically_for_legacy_prefix_checks() {
        assert_eq!(
            normalize_path_lexically(Path::new("a/./b/../c")),
            PathBuf::from("a/c")
        );
        assert_eq!(
            normalize_path_lexically(Path::new("/repo/.wt/execution/../workflows/new.toml")),
            PathBuf::from("/repo/.wt/workflows/new.toml")
        );
    }

    #[test]
    fn detects_legacy_flat_personal_roots_without_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".wt/tasks")).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        let legacy = storage.detect_legacy_tasks(&repo).unwrap();

        assert_eq!(legacy.path(), repo.join(".wt/tasks").as_path());
        assert_eq!(
            legacy.canonical_root(),
            repo.join(".wt/execution/tasks").as_path()
        );
        assert!(
            legacy
                .error_message_for("TaskDocument storage")
                .contains("does not silently fall back")
        );
    }

    #[test]
    fn detects_legacy_message_storage_without_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".wt/messages/agents/codex/inbox/new")).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        let legacy = storage.detect_legacy_messages().unwrap();

        assert_eq!(legacy.path(), repo.join(".wt/messages").as_path());
        assert_eq!(
            legacy.canonical_root(),
            repo.join(".wt/runtime/agents").as_path()
        );
        assert!(
            legacy
                .error_message_for("message storage")
                .contains("does not silently fall back")
        );
    }

    #[test]
    fn detects_legacy_runtime_actor_roots_without_fallback() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join(".wt/agent.state")).unwrap();
        fs::create_dir_all(repo.join(".wt/sessions")).unwrap();
        let storage = StorageRoot::from_git_common_dir(repo.join(".git"));

        let agent_state = storage.detect_legacy_agent_state().unwrap();
        assert_eq!(agent_state.path(), repo.join(".wt/agent.state").as_path());
        assert_eq!(
            agent_state.canonical_root(),
            repo.join(".wt/runtime/agents").as_path()
        );
        assert!(
            agent_state
                .error_message_for("runtime observation storage")
                .contains("does not silently fall back")
        );

        let sessions = storage.detect_legacy_sessions().unwrap();
        assert_eq!(sessions.path(), repo.join(".wt/sessions").as_path());
        assert_eq!(
            sessions.canonical_root(),
            repo.join(".wt/runtime/agents").as_path()
        );
        assert!(
            sessions
                .error_message_for("session anchor storage")
                .contains("does not silently fall back")
        );
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

        let err = StorageRoot::resolve(&FailingRunner, None, Path::new("/repo")).unwrap_err();

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
