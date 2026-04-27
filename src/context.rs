use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::config::Config;

/// Output from running an external command.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Trait for running external commands. Injected into Ctx for testability.
pub trait CommandRunner: Send + Sync {
    fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput>;

    /// Check if a command exists on PATH.
    fn has_command(&self, cmd: &str) -> bool;
}

/// Trait for user interaction. Injected into Ctx for testability.
pub trait UserInterface: Send + Sync {
    fn select(&self, prompt: &str, items: &[String]) -> Result<usize>;
    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>>;
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;
    fn input(&self, prompt: &str, default: Option<&str>) -> Result<String>;
    fn print_step(&self, msg: &str);
    fn print_dim(&self, msg: &str);
    fn print_warning(&self, msg: &str);
    fn print_error(&self, msg: &str);
}

/// Context object carrying all side-effect handles.
pub struct Ctx {
    pub repo_root: PathBuf,
    pub parent_dir: PathBuf,
    pub repo_name: String,
    pub config: Config,
    pub runner: Box<dyn CommandRunner>,
    pub ui: Box<dyn UserInterface>,
}

impl Ctx {
    pub fn new(
        repo_root: PathBuf,
        config: Config,
        runner: Box<dyn CommandRunner>,
        ui: Box<dyn UserInterface>,
    ) -> Self {
        let parent_dir = repo_root.parent().unwrap_or(Path::new("/")).to_path_buf();
        let repo_name = repo_root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Self {
            repo_root,
            parent_dir,
            repo_name,
            config,
            runner,
            ui,
        }
    }
}

/// Mock runner for tests. Records calls and returns predefined outputs.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    pub struct MockRunner {
        responses: Mutex<VecDeque<CmdOutput>>,
        pub calls: Mutex<Vec<(String, Vec<String>)>>,
        available_commands: Vec<String>,
    }

    impl Default for MockRunner {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockRunner {
        pub fn new() -> Self {
            Self {
                responses: Mutex::new(VecDeque::new()),
                calls: Mutex::new(Vec::new()),
                available_commands: Vec::new(),
            }
        }

        pub fn add_response(&mut self, stdout: &str, success: bool) {
            self.responses.lock().unwrap().push_back(CmdOutput {
                stdout: stdout.into(),
                stderr: String::new(),
                success,
            });
        }

        pub fn add_command(&mut self, cmd: &str) {
            self.available_commands.push(cmd.into());
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, cmd: &str, args: &[&str], _cwd: Option<&Path>) -> Result<CmdOutput> {
            self.calls.lock().unwrap().push((
                cmd.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockRunner: no more responses queued"))
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.available_commands.contains(&cmd.to_string())
        }
    }

    pub struct MockUi {
        select_responses: Mutex<VecDeque<usize>>,
        multi_select_responses: Mutex<VecDeque<Vec<usize>>>,
        confirm_responses: Mutex<VecDeque<bool>>,
        input_responses: Mutex<VecDeque<String>>,
        pub steps: Mutex<Vec<String>>,
        pub warnings: Mutex<Vec<String>>,
    }

    impl Default for MockUi {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockUi {
        pub fn new() -> Self {
            Self {
                select_responses: Mutex::new(VecDeque::new()),
                multi_select_responses: Mutex::new(VecDeque::new()),
                confirm_responses: Mutex::new(VecDeque::new()),
                input_responses: Mutex::new(VecDeque::new()),
                steps: Mutex::new(Vec::new()),
                warnings: Mutex::new(Vec::new()),
            }
        }

        pub fn add_select(&mut self, index: usize) {
            self.select_responses.lock().unwrap().push_back(index);
        }

        #[allow(dead_code)]
        pub fn add_multi_select(&mut self, indices: Vec<usize>) {
            self.multi_select_responses
                .lock()
                .unwrap()
                .push_back(indices);
        }

        pub fn add_confirm(&mut self, value: bool) {
            self.confirm_responses.lock().unwrap().push_back(value);
        }

        pub fn add_input(&mut self, value: &str) {
            self.input_responses.lock().unwrap().push_back(value.into());
        }
    }

    impl UserInterface for MockUi {
        fn select(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
            self.select_responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockUi: no select response"))
        }

        fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
            self.multi_select_responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockUi: no multi_select response"))
        }

        fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
            self.confirm_responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockUi: no confirm response"))
        }

        fn input(&self, _prompt: &str, default: Option<&str>) -> Result<String> {
            self.input_responses
                .lock()
                .unwrap()
                .pop_front()
                .or_else(|| default.map(String::from))
                .ok_or_else(|| anyhow::anyhow!("MockUi: no input response"))
        }

        fn print_step(&self, msg: &str) {
            self.steps.lock().unwrap().push(msg.into());
        }

        fn print_warning(&self, msg: &str) {
            self.warnings.lock().unwrap().push(msg.into());
        }

        fn print_dim(&self, _msg: &str) {}

        fn print_error(&self, _msg: &str) {}
    }
}

#[cfg(test)]
mod tests {
    use super::mock::*;
    use super::*;
    use assert_fs::TempDir;
    use std::fs;

    #[test]
    fn mock_runner_records_calls_and_returns_responses() {
        let mut runner = MockRunner::new();
        runner.add_response("output1", true);
        runner.add_response("output2", false);

        let r1 = runner.run("git", &["status"], None).unwrap();
        assert!(r1.success);
        assert_eq!(r1.stdout, "output1");

        let r2 = runner.run("git", &["diff"], None).unwrap();
        assert!(!r2.success);

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "git");
        assert_eq!(calls[1].1, vec!["diff"]);
    }

    #[test]
    fn mock_runner_errors_when_no_responses() {
        let runner = MockRunner::new();
        assert!(runner.run("git", &["status"], None).is_err());
    }

    #[test]
    fn mock_ui_returns_queued_responses() {
        let mut ui = MockUi::new();
        ui.add_select(2);
        ui.add_confirm(true);

        let items = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(ui.select("pick", &items).unwrap(), 2);
        assert!(ui.confirm("ok?", false).unwrap());
    }

    #[test]
    fn derives_repo_name_from_canonical_root() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("hapjeong");
        fs::create_dir(&repo_root).unwrap();

        let ctx = Ctx::new(
            repo_root.clone(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(ctx.repo_root, repo_root);
        assert_eq!(ctx.repo_name, "hapjeong");
        assert_eq!(ctx.parent_dir, temp.path());
    }

    #[test]
    fn ctx_derives_parent_and_name() {
        let ctx = Ctx::new(
            PathBuf::from("/home/dev/projects/hapjeong"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        assert_eq!(ctx.parent_dir, PathBuf::from("/home/dev/projects"));
        assert_eq!(ctx.repo_name, "hapjeong");
    }
}
