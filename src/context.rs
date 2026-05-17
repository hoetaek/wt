use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

    fn run_with_timeout(
        &self,
        cmd: &str,
        args: &[&str],
        cwd: Option<&Path>,
        _timeout: Duration,
    ) -> Result<CmdOutput> {
        self.run(cmd, args, cwd)
    }

    /// Check if a command exists on PATH.
    fn has_command(&self, cmd: &str) -> bool;
}

/// Trait for user interaction. Injected into Ctx for testability.
pub trait UserInterface: Send + Sync {
    fn select(&self, prompt: &str, items: &[String]) -> Result<usize>;
    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>>;
    fn can_prompt(&self) -> bool {
        true
    }
    fn select_items(&self, prompt: &str, items: &[PromptItem]) -> Result<usize> {
        let rendered = render_prompt_items(items);
        self.select(prompt, &rendered)
    }
    fn multi_select_items(&self, prompt: &str, items: &[PromptItem]) -> Result<Vec<usize>> {
        let rendered = render_prompt_items(items);
        self.multi_select(prompt, &rendered)
    }
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;
    fn input(&self, prompt: &str, default: Option<&str>) -> Result<String>;
    fn print_step(&self, msg: &str);
    fn print_dim(&self, msg: &str);
    fn print_warning(&self, msg: &str);
    fn print_error(&self, msg: &str);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptItem {
    pub label: String,
    pub hint: Option<String>,
}

impl PromptItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: None,
        }
    }

    pub fn with_hint(label: impl Into<String>, hint: impl Into<String>) -> Self {
        let hint = hint.into();
        Self {
            label: label.into(),
            hint: non_empty_hint(hint),
        }
    }

    pub fn from_hint_parts(label: impl Into<String>, parts: Vec<String>) -> Self {
        Self::with_hint(label, join_prompt_hint(parts))
    }

    pub fn render_plain(&self) -> String {
        match self.hint.as_deref() {
            Some(hint) => format!("{}  {}", self.label, hint),
            None => self.label.clone(),
        }
    }
}

impl From<String> for PromptItem {
    fn from(label: String) -> Self {
        Self::new(label)
    }
}

impl From<&str> for PromptItem {
    fn from(label: &str) -> Self {
        Self::new(label)
    }
}

pub fn join_prompt_hint(parts: Vec<String>) -> String {
    parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

pub fn render_prompt_items(items: &[PromptItem]) -> Vec<String> {
    items.iter().map(PromptItem::render_plain).collect()
}

fn non_empty_hint(hint: String) -> Option<String> {
    let hint = hint.trim();
    if hint.is_empty() {
        None
    } else {
        Some(hint.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Text,
    Json,
}

#[derive(Debug, Clone)]
pub struct CtxOptions {
    pub base_config: Config,
    pub output_mode: OutputMode,
    pub verbosity: u8,
    pub quiet: bool,
}

impl Default for CtxOptions {
    fn default() -> Self {
        Self {
            base_config: Config::default(),
            output_mode: OutputMode::Text,
            verbosity: 0,
            quiet: false,
        }
    }
}

/// Context object carrying all side-effect handles.
pub struct Ctx {
    pub repo_root: PathBuf,
    pub invocation_root: PathBuf,
    pub parent_dir: PathBuf,
    pub repo_name: String,
    pub config: Config,
    pub base_config: Config,
    pub runner: Box<dyn CommandRunner>,
    pub ui: Box<dyn UserInterface>,
    pub output_mode: OutputMode,
    pub verbosity: u8,
    pub quiet: bool,
}

impl Ctx {
    pub fn new(
        repo_root: PathBuf,
        invocation_root: PathBuf,
        config: Config,
        runner: Box<dyn CommandRunner>,
        ui: Box<dyn UserInterface>,
    ) -> Self {
        let options = CtxOptions {
            base_config: config.clone(),
            output_mode: OutputMode::Text,
            verbosity: 0,
            quiet: false,
        };
        Self::new_with_options(repo_root, invocation_root, config, runner, ui, options)
    }

    pub fn new_with_options(
        repo_root: PathBuf,
        invocation_root: PathBuf,
        config: Config,
        runner: Box<dyn CommandRunner>,
        ui: Box<dyn UserInterface>,
        options: CtxOptions,
    ) -> Self {
        let parent_dir = repo_root.parent().unwrap_or(Path::new("/")).to_path_buf();
        let repo_name = repo_root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        Self {
            repo_root,
            invocation_root,
            parent_dir,
            repo_name,
            config,
            base_config: options.base_config,
            runner,
            ui,
            output_mode: options.output_mode,
            verbosity: options.verbosity,
            quiet: options.quiet,
        }
    }

    pub fn is_json(&self) -> bool {
        self.output_mode == OutputMode::Json
    }
}

/// Mock runner for tests. Records calls and returns predefined outputs.
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    pub type CommandCall = (String, Vec<String>, Option<PathBuf>);

    pub struct MockRunner {
        responses: Mutex<VecDeque<CmdOutput>>,
        pub calls: Mutex<Vec<CommandCall>>,
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

        pub fn add_response_with_stderr(&mut self, stdout: &str, stderr: &str, success: bool) {
            self.responses.lock().unwrap().push_back(CmdOutput {
                stdout: stdout.into(),
                stderr: stderr.into(),
                success,
            });
        }

        pub fn add_command(&mut self, cmd: &str) {
            self.available_commands.push(cmd.into());
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            self.calls.lock().unwrap().push((
                cmd.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
                cwd.map(Path::to_path_buf),
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
        pub prompts: Mutex<Vec<String>>,
        pub select_items: Mutex<Vec<Vec<String>>>,
        pub multi_select_items: Mutex<Vec<Vec<String>>>,
        pub steps: Mutex<Vec<String>>,
        pub dims: Mutex<Vec<String>>,
        pub warnings: Mutex<Vec<String>>,
        prompt_available: bool,
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
                prompts: Mutex::new(Vec::new()),
                select_items: Mutex::new(Vec::new()),
                multi_select_items: Mutex::new(Vec::new()),
                steps: Mutex::new(Vec::new()),
                dims: Mutex::new(Vec::new()),
                warnings: Mutex::new(Vec::new()),
                prompt_available: true,
            }
        }

        pub fn set_prompt_available(&mut self, prompt_available: bool) {
            self.prompt_available = prompt_available;
        }

        pub fn add_select(&mut self, index: usize) {
            self.select_responses.lock().unwrap().push_back(index);
        }

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
        fn select(&self, prompt: &str, items: &[String]) -> Result<usize> {
            self.prompts
                .lock()
                .unwrap()
                .push(format!("select: {prompt}"));
            self.select_items.lock().unwrap().push(items.to_vec());
            self.select_responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockUi: no select response"))
        }

        fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
            self.prompts
                .lock()
                .unwrap()
                .push(format!("multi_select: {prompt}"));
            self.multi_select_items.lock().unwrap().push(items.to_vec());
            self.multi_select_responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockUi: no multi_select response"))
        }

        fn can_prompt(&self) -> bool {
            self.prompt_available
        }

        fn confirm(&self, prompt: &str, _default: bool) -> Result<bool> {
            self.prompts
                .lock()
                .unwrap()
                .push(format!("confirm: {prompt}"));
            self.confirm_responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("MockUi: no confirm response"))
        }

        fn input(&self, prompt: &str, default: Option<&str>) -> Result<String> {
            self.prompts
                .lock()
                .unwrap()
                .push(format!("input: {prompt}"));
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

        fn print_dim(&self, msg: &str) {
            self.dims.lock().unwrap().push(msg.into());
        }

        fn print_error(&self, _msg: &str) {}
    }

    impl UserInterface for Arc<MockUi> {
        fn select(&self, prompt: &str, items: &[String]) -> Result<usize> {
            self.as_ref().select(prompt, items)
        }

        fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>> {
            self.as_ref().multi_select(prompt, items)
        }

        fn can_prompt(&self) -> bool {
            self.as_ref().can_prompt()
        }

        fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
            self.as_ref().confirm(prompt, default)
        }

        fn input(&self, prompt: &str, default: Option<&str>) -> Result<String> {
            self.as_ref().input(prompt, default)
        }

        fn print_step(&self, msg: &str) {
            self.as_ref().print_step(msg);
        }

        fn print_dim(&self, msg: &str) {
            self.as_ref().print_dim(msg);
        }

        fn print_warning(&self, msg: &str) {
            self.as_ref().print_warning(msg);
        }

        fn print_error(&self, msg: &str) {
            self.as_ref().print_error(msg);
        }
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
        assert_eq!(calls[0].2, None);
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
    fn mock_ui_records_prompt_item_plain_fallbacks() {
        let mut ui = MockUi::new();
        ui.add_select(0);
        ui.add_multi_select(vec![1]);

        let items = vec![
            PromptItem::with_hint("Fix editor", "task PROJ-123 | Linear"),
            PromptItem::new("Local cleanup"),
        ];

        assert_eq!(ui.select_items("pick one", &items).unwrap(), 0);
        assert_eq!(ui.multi_select_items("pick many", &items).unwrap(), vec![1]);
        assert_eq!(
            ui.select_items.lock().unwrap().as_slice(),
            [vec![
                "Fix editor  task PROJ-123 | Linear".to_string(),
                "Local cleanup".to_string()
            ]]
        );
        assert_eq!(
            ui.multi_select_items.lock().unwrap().as_slice(),
            [vec![
                "Fix editor  task PROJ-123 | Linear".to_string(),
                "Local cleanup".to_string()
            ]]
        );
    }

    #[test]
    fn ctx_keeps_canonical_and_invocation_roots_separately() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("sample-app");
        let invocation_root = temp.path().join("sample-app-alice-proj-670");
        fs::create_dir(&repo_root).unwrap();
        fs::create_dir(&invocation_root).unwrap();

        let ctx = Ctx::new(
            repo_root.clone(),
            invocation_root.clone(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(ctx.repo_root, repo_root);
        assert_eq!(ctx.invocation_root, invocation_root);
        assert_eq!(ctx.repo_name, "sample-app");
        assert_eq!(ctx.parent_dir, temp.path());
    }

    #[test]
    fn derives_repo_name_from_canonical_root() {
        let temp = TempDir::new().unwrap();
        let repo_root = temp.path().join("sample-app");
        fs::create_dir(&repo_root).unwrap();

        let ctx = Ctx::new(
            repo_root.clone(),
            repo_root.clone(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(ctx.repo_root, repo_root);
        assert_eq!(ctx.repo_name, "sample-app");
        assert_eq!(ctx.parent_dir, temp.path());
    }

    #[test]
    fn ctx_derives_parent_and_name() {
        let ctx = Ctx::new(
            PathBuf::from("/home/dev/projects/sample-app"),
            PathBuf::from("/home/dev/projects/sample-app"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        assert_eq!(ctx.parent_dir, PathBuf::from("/home/dev/projects"));
        assert_eq!(ctx.repo_name, "sample-app");
    }
}
