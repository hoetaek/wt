mod agent;
mod background_tests;
mod browser;
mod chrome_devtools;
mod command;
mod deps;
mod env_template;
mod files;
mod local_context;
mod post_deps;
mod site;
mod summary;
mod workspace;

pub(crate) use agent::agent_launch_command;
use agent::bootstrap_agent;
use background_tests::run_background_tests;
use deps::install_deps;
use env_template::substitute_env;
use files::{copy_files, link_files};
use local_context::inject_local_context;
use post_deps::open_post_deps_tabs;
use summary::print_summary;
use workspace::{insert_cmux_template_vars, open_workspace, workspace_color};

pub(crate) use browser::{launch_browser, prepare_browser_launch};
pub(crate) use env_template::build_template_vars;
pub(crate) use site::apply_site_template_vars;

use crate::config::{Config, SiteProvider};
pub(crate) use crate::config::{
    WORKSPACE_COLOR_KIND_BRANCH, WORKSPACE_COLOR_KIND_ISSUE, WORKSPACE_COLOR_KIND_PR,
    WORKSPACE_COLOR_KIND_TASK,
};
use crate::context::Ctx;
use crate::names::WorktreeNames;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) struct SetupModeKinds<'a> {
    setup_mode: &'a str,
    workspace_color_kind: &'a str,
}

impl<'a> SetupModeKinds<'a> {
    pub(crate) fn new(setup_mode: &'a str, workspace_color_kind: &'a str) -> Self {
        Self {
            setup_mode,
            workspace_color_kind,
        }
    }

    fn same(kind: &'a str) -> Self {
        Self::new(kind, kind)
    }
}

#[cfg(test)]
use crate::config::{AgentCli, AgentConfig, DepCommand, SubmitMode};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use workspace::OpenedWorkspace;

#[derive(Clone, Copy)]
pub(crate) struct SetupOptions {
    pub(crate) focus_workspace: bool,
    pub(crate) restore_caller_after_workspace_open: bool,
    pub(crate) focus_restore_if_workspace_cold: bool,
}

impl Default for SetupOptions {
    fn default() -> Self {
        Self {
            focus_workspace: false,
            restore_caller_after_workspace_open: true,
            focus_restore_if_workspace_cold: true,
        }
    }
}

#[derive(Clone)]
pub(crate) struct SiteDescriptor {
    pub(crate) provider: SiteProvider,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) root: String,
    pub(crate) secure: bool,
    pub(crate) target: Option<String>,
}

/// Run the full setup sequence on a newly created worktree.
pub fn run_setup(
    ctx: &Ctx,
    wt_path: &Path,
    names: &WorktreeNames,
    title: Option<&str>,
    mode: &str,
    extra_vars: Option<&HashMap<String, String>>,
    config_override: Option<&Config>,
) -> Result<()> {
    run_setup_with_workspace_color_kind(
        ctx,
        wt_path,
        names,
        title,
        SetupModeKinds::same(mode),
        extra_vars,
        config_override,
    )
}

pub(crate) fn run_setup_with_workspace_color_kind(
    ctx: &Ctx,
    wt_path: &Path,
    names: &WorktreeNames,
    title: Option<&str>,
    modes: SetupModeKinds<'_>,
    extra_vars: Option<&HashMap<String, String>>,
    config_override: Option<&Config>,
) -> Result<()> {
    let options = SetupOptions::default();
    let config = config_override.unwrap_or(&ctx.config);

    copy_files(ctx, config, wt_path)?;
    link_files(ctx, config, wt_path)?;

    let mut template_vars = build_template_vars(ctx, wt_path, names, title);
    if let Some(extra) = extra_vars {
        template_vars.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let site = apply_site_template_vars(config, &mut template_vars);
    let browser_launch = prepare_browser_launch(config, wt_path, &mut template_vars)?;

    if let Some(ref site) = site {
        site::register_site(ctx, wt_path, site);
    }

    substitute_env(wt_path, config, &template_vars)?;

    let ws_color = workspace_color(config, modes.workspace_color_kind);
    let opened_workspace = open_workspace(
        ctx,
        config,
        wt_path,
        names,
        &template_vars,
        &ws_color,
        options,
    )?;
    insert_cmux_template_vars(&mut template_vars, opened_workspace.as_ref());
    let ws_handle = opened_workspace
        .as_ref()
        .map(|workspace| workspace.handle.as_str());

    inject_local_context(ctx, config, wt_path, names, &template_vars, ws_handle)?;

    install_deps(ctx, config, wt_path)?;

    if let Some(handle) = ws_handle {
        open_post_deps_tabs(ctx, config, handle, &template_vars)?;
    }

    // Launch browser after deps (site may need built assets).
    launch_browser(ctx, browser_launch)?;

    if let (Some(handle), Some(agent)) = (ws_handle, &config.agent) {
        bootstrap_agent(ctx, handle, agent, modes.setup_mode, &template_vars)?;
    }

    run_background_tests(ctx, config, wt_path)?;

    print_summary(ctx, wt_path, names, site.as_ref(), &template_vars);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, CtxOptions};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct SharedMockRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedMockRunner {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&std::path::Path>,
        ) -> Result<CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    fn bootstrap_test_ctx(runner: Arc<MockRunner>) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(SharedMockRunner { inner: runner }),
            Box::new(MockUi::new()),
        )
    }

    fn agent_config(cli: AgentCli) -> AgentConfig {
        AgentConfig {
            cli,
            args: Vec::new(),
            command: None,
            ready: crate::config::ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 30,
            send_after: 2,
            prompt: HashMap::new(),
            ..AgentConfig::default()
        }
    }

    #[test]
    fn copy_files_copies_nested_file_into_parent_dirs() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};

        let repo = std::env::temp_dir().join("wt-test-copy-nested-file-repo");
        let wt = std::env::temp_dir().join("wt-test-copy-nested-file-worktree");
        fs::create_dir_all(repo.join(".claude")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(repo.join(".claude/settings.local.json"), "{\"a\":1}\n").unwrap();

        let mut config = Config::default();
        config.worktree.copy = vec![".claude/settings.local.json".into()];

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        copy_files(&ctx, &ctx.config, &wt).unwrap();

        assert_eq!(
            fs::read_to_string(wt.join(".claude/settings.local.json")).unwrap(),
            "{\"a\":1}\n"
        );

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn copy_files_copies_directories_recursively() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};

        let repo = std::env::temp_dir().join("wt-test-copy-dir-repo");
        let wt = std::env::temp_dir().join("wt-test-copy-dir-worktree");
        fs::create_dir_all(repo.join(".claude/hooks/nested")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(repo.join(".claude/hooks/pre-commit"), "hook\n").unwrap();
        fs::write(repo.join(".claude/hooks/nested/config.txt"), "nested\n").unwrap();

        let mut config = Config::default();
        config.worktree.copy = vec![".claude/hooks".into()];

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        copy_files(&ctx, &ctx.config, &wt).unwrap();

        assert_eq!(
            fs::read_to_string(wt.join(".claude/hooks/pre-commit")).unwrap(),
            "hook\n"
        );
        assert_eq!(
            fs::read_to_string(wt.join(".claude/hooks/nested/config.txt")).unwrap(),
            "nested\n"
        );

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn copy_files_copies_copy_as_directory_to_worktree_root() {
        use crate::config::CopyAsEntry;
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};

        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let wt = dir.path().join("worktree");
        let scaffold = repo.join(".git/wt/profiles/codex/scaffold");
        fs::create_dir_all(scaffold.join(".codex/skills/start")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(scaffold.join("AGENTS.override.md"), "instructions\n").unwrap();
        fs::write(
            scaffold.join(".codex/skills/start/SKILL.md"),
            "start skill\n",
        )
        .unwrap();

        let mut config = Config::default();
        config.worktree.copy_as = vec![CopyAsEntry {
            from: ".git/wt/profiles/codex/scaffold".into(),
            to: ".".into(),
        }];

        let ctx = Ctx::new(
            repo.clone(),
            repo,
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        copy_files(&ctx, &ctx.config, &wt).unwrap();

        assert_eq!(
            fs::read_to_string(wt.join("AGENTS.override.md")).unwrap(),
            "instructions\n"
        );
        assert_eq!(
            fs::read_to_string(wt.join(".codex/skills/start/SKILL.md")).unwrap(),
            "start skill\n"
        );
    }

    #[test]
    fn install_deps_uses_working_dir_for_if_exists_and_command() {
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let wt = dir.path().join("worktree");
        fs::create_dir_all(wt.join("frontend")).unwrap();
        fs::write(wt.join("frontend/package.json"), "{}").unwrap();
        fs::write(wt.join("composer.json"), "{}").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut config = Config::default();
        config.setup.deps = vec![
            DepCommand {
                working_dir: Some("frontend".into()),
                run: "npm install".into(),
                if_exists: Some("package.json".into()),
            },
            DepCommand {
                working_dir: None,
                run: "composer install".into(),
                if_exists: Some("composer.json".into()),
            },
            DepCommand {
                working_dir: Some("missing".into()),
                run: "npm install".into(),
                if_exists: Some("package.json".into()),
            },
        ];

        let ctx = Ctx::new(
            repo.clone(),
            repo,
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        install_deps(&ctx, &ctx.config, &wt).unwrap();

        let mut calls = runner.calls.lock().unwrap().clone();
        calls.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "composer");
        assert_eq!(calls[0].1, vec!["install"]);
        assert_eq!(calls[0].2, Some(wt.clone()));
        assert_eq!(calls[1].0, "npm");
        assert_eq!(calls[1].1, vec!["install"]);
        assert_eq!(calls[1].2, Some(wt.join("frontend")));
    }

    #[test]
    fn link_files_creates_parent_dirs_for_nested_destinations() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};

        let repo = std::env::temp_dir().join("wt-test-link-nested-repo");
        let wt = std::env::temp_dir().join("wt-test-link-nested-worktree");
        fs::create_dir_all(repo.join(".config")).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(repo.join(".config/tool.toml"), "name = \"wt\"\n").unwrap();

        let mut config = Config::default();
        config.worktree.link = vec![".config/tool.toml".into()];

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        link_files(&ctx, &ctx.config, &wt).unwrap();

        let dest = wt.join(".config/tool.toml");
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "name = \"wt\"\n");
        assert_eq!(
            fs::read_link(&dest).unwrap(),
            fs::canonicalize(repo.join(".config/tool.toml")).unwrap()
        );

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn build_template_vars_includes_all_fields() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/sample-app-alice-proj-680"),
            branch: "alice/proj-680-document-editor".into(),
            workspace: "Document editor".into(),
            site: Some("sample-app-proj-680".into()),
        };

        let ctx = Ctx::new_with_options(
            PathBuf::from("/home/dev/sample-app"),
            PathBuf::from("/home/dev/sample-app"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                launcher_coordinator_id: Some("agents/coord-a".into()),
                ..CtxOptions::default()
            },
        );

        let actual_worktree_path = PathBuf::from("/tmp/existing-sample-app-proj-680");
        let vars =
            build_template_vars(&ctx, &actual_worktree_path, &names, Some("Document editor"));
        assert_eq!(vars.get("repo").unwrap(), "sample-app");
        assert_eq!(vars.get("repo_root").unwrap(), "/home/dev/sample-app");
        assert_eq!(
            vars.get("worktree_path").unwrap(),
            "/tmp/existing-sample-app-proj-680"
        );
        assert_eq!(vars.get("worktree_parent").unwrap(), "/tmp");
        assert_eq!(
            vars.get("worktree_name").unwrap(),
            "existing-sample-app-proj-680"
        );
        assert_eq!(vars.get("site_name").unwrap(), "sample-app-proj-680");
        assert_eq!(vars.get("branch_slug").unwrap(), "proj-680-document-editor");
        assert_eq!(
            vars.get("wt_agent_id").unwrap(),
            "agents/proj-680-document-editor"
        );
        assert_eq!(
            vars.get("wt_coordinator_agent_id").unwrap(),
            "agents/coord-a"
        );
        assert_eq!(vars.get("coordinator_msg_target").unwrap(), "coordinator");
        assert_eq!(vars.get("issue_title").unwrap(), "Document editor");
        assert!(vars.contains_key("vite_port"));
        assert!(vars.contains_key("api_port"));
        assert_eq!(vars.get("front_port"), vars.get("vite_port"));
        assert_eq!(vars.get("back_port"), vars.get("api_port"));
        assert_eq!(
            vars.get("site_url").unwrap(),
            &format!("http://127.0.0.1:{}", vars.get("vite_port").unwrap())
        );
        assert_eq!(
            vars.get("api_url").unwrap(),
            &format!("http://127.0.0.1:{}", vars.get("api_port").unwrap())
        );
        assert_eq!(
            vars.get("api_port").unwrap().parse::<u32>().unwrap(),
            vars.get("vite_port").unwrap().parse::<u32>().unwrap() + 10000
        );
    }

    #[test]
    fn cmux_template_vars_include_coordinator_and_task_agent_context() {
        use crate::services::cmux::CmuxCaller;

        let mut vars = HashMap::new();
        let opened = OpenedWorkspace {
            handle: "workspace:42".into(),
            coordinator: Some(CmuxCaller {
                window: Some("window:1".into()),
                workspace: Some("workspace:1".into()),
                pane: Some("pane:1".into()),
                surface: Some("surface:128".into()),
            }),
        };

        insert_cmux_template_vars(&mut vars, Some(&opened));

        assert_eq!(
            vars.get("task_agent_cmux_workspace").map(String::as_str),
            Some("workspace:42")
        );
        assert_eq!(
            vars.get("coordinator_cmux_workspace").map(String::as_str),
            Some("workspace:1")
        );
        assert_eq!(
            vars.get("coordinator_cmux_surface").map(String::as_str),
            Some("surface:128")
        );
        assert_eq!(
            vars.get("coordinator_send_command").map(String::as_str),
            Some("cmux send --workspace workspace:1 --surface surface:128 '<message>'")
        );
        assert_eq!(
            vars.get("coordinator_enter_command").map(String::as_str),
            Some("cmux send-key --workspace workspace:1 --surface surface:128 enter")
        );
    }

    #[test]
    fn open_workspace_restores_focused_cmux_surface() {
        use crate::config::{Config, WorkspaceConfig};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-focused-surface-repo");
        let wt = std::env::temp_dir().join("wt-test-focused-surface-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:1","pane_ref":"pane:1","surface_ref":"surface:10"},"focused":{"window_ref":"window:1","workspace_ref":"workspace:1","pane_ref":"pane:1","surface_ref":"surface:20"}}"#,
            true,
        );
        runner.add_response("workspace:2 workspace:2", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig::default()),
            ..Config::default()
        };
        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config.clone(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/focus-restore".into(),
            workspace: "focus restore".into(),
            site: None,
        };
        let options = SetupOptions {
            focus_workspace: true,
            focus_restore_if_workspace_cold: false,
            ..SetupOptions::default()
        };

        let opened = open_workspace(&ctx, &config, &wt, &names, &HashMap::new(), "", options)
            .unwrap()
            .unwrap();

        assert_eq!(
            opened
                .coordinator
                .as_ref()
                .and_then(|caller| caller.surface.as_deref()),
            Some("surface:10")
        );
        let calls = runner.calls.lock().unwrap();
        let focus_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "cmux"
                    && args.first().is_some_and(|arg| arg == "rpc")
                    && args.get(1).is_some_and(|arg| arg == "surface.focus")
            })
            .expect("expected surface.focus call");
        let params: serde_json::Value = serde_json::from_str(&focus_call.1[2]).unwrap();
        assert_eq!(params["surface_id"], "surface:20");
        assert_eq!(params["workspace_id"], "workspace:1");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn run_setup_opens_workspace_url_without_site() {
        use crate::config::{
            Config, WorkspaceBrowserConfig, WorkspaceBrowserMode, WorkspaceConfig,
        };
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-open-url-repo");
        let wt = std::env::temp_dir().join("wt-test-open-url-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(r#"{"caller":{"window_ref":"window:1"}}"#, true);
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig {
                browser: Some(WorkspaceBrowserConfig {
                    mode: WorkspaceBrowserMode::System,
                    url: Some("{{site_url}}".into()),
                    app: Some("Google Chrome".into()),
                }),
                ..WorkspaceConfig::default()
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };

        run_setup(&ctx, &wt, &names, Some("GitHub Issue"), "issue", None, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let open_call = calls
            .iter()
            .find(|(cmd, _, _)| cmd == "open")
            .expect("expected open command");
        assert_eq!(open_call.1[0], "-a");
        assert_eq!(open_call.1[1], "Google Chrome");
        assert!(open_call.1[2].starts_with("http://127.0.0.1:"));

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn run_setup_browser_mode_none_suppresses_browser_and_chrome_launch() {
        use crate::config::{
            Config, WorkspaceBrowserConfig, WorkspaceBrowserMode, WorkspaceChromeDevtoolsConfig,
            WorkspaceConfig,
        };
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-browser-none-repo");
        let wt = std::env::temp_dir().join("wt-test-browser-none-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let config = Config {
            workspace: Some(WorkspaceConfig {
                browser: Some(WorkspaceBrowserConfig {
                    mode: WorkspaceBrowserMode::None,
                    url: None,
                    app: None,
                }),
                chrome_devtools: Some(WorkspaceChromeDevtoolsConfig {
                    port: Some(9222),
                    ..WorkspaceChromeDevtoolsConfig::default()
                }),
                ..WorkspaceConfig::default()
            }),
            ..Config::default()
        };
        let runner = Arc::new(MockRunner::new());
        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };

        run_setup(&ctx, &wt, &names, Some("GitHub Issue"), "issue", None, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(!calls.iter().any(|(cmd, _, _)| cmd == "open" || cmd == "sh"));

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn run_setup_launches_chrome_devtools_and_renders_local_context_vars() {
        use crate::config::{
            Config, WorkspaceBrowserConfig, WorkspaceBrowserMode, WorkspaceChromeDevtoolsConfig,
            WorkspaceConfig,
        };
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-chrome-devtools-repo");
        let wt = std::env::temp_dir().join("wt-test-chrome-devtools-worktree");
        fs::create_dir_all(repo.join(".repo-private")).ok();
        fs::create_dir_all(&wt).ok();
        fs::write(wt.join("CLAUDE.local.md"), "# Existing content\n").unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("google-chrome");
        // get_branch_parent: git config --get
        runner.add_response("", false);
        // Chrome launch
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut config = Config {
            workspace: Some(WorkspaceConfig {
                browser: Some(WorkspaceBrowserConfig {
                    mode: WorkspaceBrowserMode::ChromeDevtools,
                    url: None,
                    app: None,
                }),
                chrome_devtools: Some(WorkspaceChromeDevtoolsConfig {
                    ..WorkspaceChromeDevtoolsConfig::default()
                }),
                ..WorkspaceConfig::default()
            }),
            agent: Some(agent_config(AgentCli::Claude)),
            ..Config::default()
        };
        config.worktree.inject_local_context = Some(
            "\n## chrome\n- debug: {{chrome_debug_url}}\n- port: {{chrome_debug_port}}\n- profile: {{chrome_user_data_dir}}\n".into(),
        );

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };
        let expected_user_data_dir = wt
            .parent()
            .unwrap()
            .join(".chrome-devtools")
            .join(wt.file_name().unwrap());
        let expected_user_data_dir_text = expected_user_data_dir.to_string_lossy().into_owned();

        run_setup(&ctx, &wt, &names, Some("GitHub Issue"), "issue", None, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let launch_call = calls
            .iter()
            .find(|(cmd, _, _)| cmd == "open" || cmd == "sh")
            .expect("expected Chrome launch command");
        let launch_args = launch_call.1.join(" ");
        assert!(launch_args.contains("--remote-debugging-address=127.0.0.1"));
        assert!(launch_args.contains("--remote-debugging-port="));
        assert!(launch_args.contains("--user-data-dir="));
        assert!(launch_args.contains(&expected_user_data_dir_text));
        assert!(!launch_args.contains("{{worktree_"));

        let context = fs::read_to_string(wt.join("CLAUDE.local.md")).unwrap();
        assert!(context.contains("- debug: http://127.0.0.1:"));
        assert!(context.contains("- profile: "));
        assert!(context.contains(&expected_user_data_dir_text));
        assert!(!context.contains("{{chrome_"));
        assert!(!context.contains("{{worktree_"));
        assert!(!context.contains(&repo.join(".repo-private").to_string_lossy().to_string()));

        fs::remove_dir_all(&expected_user_data_dir).ok();
        if let Some(parent) = expected_user_data_dir.parent() {
            fs::remove_dir(parent).ok();
        }
        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn run_setup_renders_chrome_devtools_vars_for_post_deps_tabs() {
        use crate::config::{
            Config, WorkspaceBrowserConfig, WorkspaceBrowserMode, WorkspaceChromeDevtoolsConfig,
            WorkspaceConfig,
        };
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-chrome-devtools-post-tabs-repo");
        let wt = std::env::temp_dir().join("wt-test-chrome-devtools-post-tabs-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_command("google-chrome");
        runner.add_response(r#"{"caller":{"window_ref":"window:1"}}"#, true);
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("", true);
        runner.add_response("pane:0", true);
        runner.add_response("pane:0", true);
        runner.add_response("surface:1", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig {
                post_deps_tabs: vec![
                    "echo {{chrome_debug_url}} {{chrome_debug_port}} {{chrome_user_data_dir}}"
                        .into(),
                ],
                browser: Some(WorkspaceBrowserConfig {
                    mode: WorkspaceBrowserMode::ChromeDevtools,
                    url: None,
                    app: None,
                }),
                chrome_devtools: Some(WorkspaceChromeDevtoolsConfig {
                    ..WorkspaceChromeDevtoolsConfig::default()
                }),
                ..WorkspaceConfig::default()
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };
        let expected_user_data_dir = wt
            .parent()
            .unwrap()
            .join(".chrome-devtools")
            .join(wt.file_name().unwrap());
        let expected_user_data_dir_text = expected_user_data_dir.to_string_lossy().into_owned();

        run_setup(
            &ctx,
            &wt,
            &names,
            Some("GitHub Issue"),
            "branch",
            None,
            None,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        let send_call = calls
            .iter()
            .find(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|arg| arg == "send"))
            .expect("expected post-deps cmux send");
        let sent = send_call.1.last().unwrap();
        assert!(sent.contains("echo http://127.0.0.1:"));
        assert!(sent.contains(&expected_user_data_dir_text));
        assert!(!sent.contains("{{chrome_"));
        assert!(!sent.contains("{{worktree_"));

        fs::remove_dir_all(&expected_user_data_dir).ok();
        if let Some(parent) = expected_user_data_dir.parent() {
            fs::remove_dir(parent).ok();
        }
        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn run_setup_registers_valet_site_with_rendered_branch_slug() {
        use crate::config::{Config, SiteConfig, SiteProvider};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-valet-site-repo");
        let wt = std::env::temp_dir().join("wt-test-valet-site-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(wt.join("public")).ok();

        let mut runner = MockRunner::new();
        runner.add_command("valet");
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Valet,
                root: Some("public".into()),
                secure: Some(true),
                ..SiteConfig::default()
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/my-feature".into(),
            workspace: "my feature".into(),
            site: None,
        };

        run_setup(&ctx, &wt, &names, None, "branch", None, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let expected_site = format!("{}-my-feature", repo.file_name().unwrap().to_string_lossy());
        assert_eq!(calls[0].0, "valet");
        assert_eq!(calls[0].1, vec!["link", expected_site.as_str()]);
        assert_eq!(calls[0].2, Some(wt.join("public")));
        assert_eq!(calls[1].0, "valet");
        assert_eq!(calls[1].1, vec!["secure", expected_site.as_str()]);

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn substitute_env_replaces_keys() {
        let dir = std::env::temp_dir().join("wt-test-env-sub");
        fs::create_dir_all(&dir).ok();
        fs::write(
            dir.join(".env"),
            "APP_URL=http://old\nAPP_NAME=old\nOTHER=keep\n",
        )
        .unwrap();

        let mut config = Config::default();
        config
            .setup
            .env
            .insert("APP_URL".into(), "https://new.test".into());
        config
            .setup
            .env
            .insert("APP_NAME".into(), "New Name".into());

        let vars = HashMap::new();
        substitute_env(&dir, &config, &vars).unwrap();

        let result = fs::read_to_string(dir.join(".env")).unwrap();
        assert!(result.contains("OTHER=keep"));
        assert!(result.contains("APP_URL=https://new.test"));
        assert!(result.contains(r#"APP_NAME="New Name""#));
        assert!(!result.contains("http://old"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn setup_env_does_not_update_nested_or_dotenv_suffix_files() {
        let dir = std::env::temp_dir().join("wt-test-env-root-only");
        fs::create_dir_all(dir.join("frontend")).ok();
        fs::create_dir_all(dir.join("backend")).ok();
        fs::write(dir.join(".env"), "APP_URL=http://old\n").unwrap();
        fs::write(
            dir.join("frontend/.env.development"),
            "VITE_API_TARGET=http://old\nOTHER=keep\n",
        )
        .unwrap();
        fs::write(dir.join("backend/.env"), "DJANGO_ENV=old\n").unwrap();

        let mut config = Config::default();
        config
            .setup
            .env
            .insert("APP_URL".into(), "https://new.test".into());
        config
            .setup
            .env
            .insert("VITE_API_TARGET".into(), "http://127.0.0.1:8000".into());
        config.setup.env.insert("DJANGO_ENV".into(), "dev".into());

        let vars = HashMap::new();
        substitute_env(&dir, &config, &vars).unwrap();

        let root = fs::read_to_string(dir.join(".env")).unwrap();
        let front = fs::read_to_string(dir.join("frontend/.env.development")).unwrap();
        let back = fs::read_to_string(dir.join("backend/.env")).unwrap();

        assert!(root.contains("APP_URL=https://new.test"));
        assert!(root.contains("VITE_API_TARGET=http://127.0.0.1:8000"));
        assert!(root.contains("DJANGO_ENV=dev"));
        assert!(front.contains("VITE_API_TARGET=http://old"));
        assert!(front.contains("OTHER=keep"));
        assert!(back.contains("DJANGO_ENV=old"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn substitute_env_appends_missing_keys_to_root_env_only() {
        let dir = std::env::temp_dir().join("wt-test-env-append-root");
        fs::create_dir_all(dir.join("nested")).ok();
        fs::write(dir.join(".env"), "EXISTING=value\n").unwrap();
        fs::write(dir.join("nested/.env"), "NESTED=value\n").unwrap();

        let mut config = Config::default();
        config
            .setup
            .env
            .insert("NEW_KEY".into(), "new-value".into());

        let vars = HashMap::new();
        substitute_env(&dir, &config, &vars).unwrap();

        let root = fs::read_to_string(dir.join(".env")).unwrap();
        let nested = fs::read_to_string(dir.join("nested/.env")).unwrap();
        assert!(root.contains("NEW_KEY=new-value"));
        assert!(!nested.contains("NEW_KEY="));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn substitute_env_files_updates_configured_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        fs::create_dir_all(dir.join("frontend")).ok();
        fs::create_dir_all(dir.join("backend")).ok();
        fs::write(dir.join(".env"), "APP_URL=http://old\n").unwrap();
        fs::write(
            dir.join("frontend/.env.development"),
            "VITE_API_TARGET=http://old\nOTHER=keep\n",
        )
        .unwrap();
        fs::write(dir.join("backend/.env"), "DJANGO_ENV=old\n").unwrap();

        let mut config = Config::default();
        config
            .setup
            .env
            .insert("APP_URL".into(), "{{site_url}}".into());
        config.setup.env_files.insert(
            "frontend/.env.development".into(),
            HashMap::from([("VITE_API_TARGET".into(), "{{api_url}}".into())]),
        );

        let vars = HashMap::from([
            ("site_url".into(), "https://root.test".into()),
            ("api_url".into(), "http://127.0.0.1:15001".into()),
        ]);
        substitute_env(dir, &config, &vars).unwrap();

        let root = fs::read_to_string(dir.join(".env")).unwrap();
        let front = fs::read_to_string(dir.join("frontend/.env.development")).unwrap();
        let back = fs::read_to_string(dir.join("backend/.env")).unwrap();

        assert!(root.contains("APP_URL=https://root.test"));
        assert!(front.contains("VITE_API_TARGET=http://127.0.0.1:15001"));
        assert!(front.contains("OTHER=keep"));
        assert!(back.contains("DJANGO_ENV=old"));
        assert!(!back.contains("VITE_API_TARGET="));
    }

    #[test]
    fn substitute_env_files_skips_missing_targets() {
        let dir = std::env::temp_dir().join("wt-test-env-files-missing");
        fs::create_dir_all(&dir).ok();

        let mut config = Config::default();
        config.setup.env_files.insert(
            "frontend/.env.development".into(),
            HashMap::from([("VITE_API_TARGET".into(), "{{api_url}}".into())]),
        );

        let vars = HashMap::from([("api_url".into(), "http://127.0.0.1:15001".into())]);
        substitute_env(&dir, &config, &vars).unwrap();

        assert!(!dir.join("frontend/.env.development").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_setup_substitutes_env_without_site() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};

        let repo = std::env::temp_dir().join("wt-test-no-site-env-repo");
        let wt = std::env::temp_dir().join("wt-test-no-site-env-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();
        fs::write(wt.join(".env"), "APP_NAME=old\n").unwrap();

        let mut config = Config::default();
        config
            .setup
            .env
            .insert("APP_NAME".into(), "{{issue_title}}".into());

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: wt.clone(),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };

        run_setup(&ctx, &wt, &names, Some("GitHub Issue"), "issue", None, None).unwrap();

        let env = fs::read_to_string(wt.join(".env")).unwrap();
        assert!(env.contains(r#"APP_NAME="GitHub Issue""#));

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn build_template_vars_extracts_issue_slug_from_branch() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "alice/proj-663-test".into(),
            workspace: "feat: Document reader".into(),
            site: None,
        };

        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let vars = build_template_vars(&ctx, &names.path, &names, None);
        assert_eq!(vars.get("issue_slug").unwrap(), "proj-663");
        assert_eq!(vars.get("branch_slug").unwrap(), "proj-663-test");
    }

    #[test]
    fn build_template_vars_without_title() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "alice/my-feature".into(),
            workspace: "my feature".into(),
            site: None,
        };

        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let vars = build_template_vars(&ctx, &names.path, &names, None);
        assert_eq!(vars.get("repo").unwrap(), "repo");
        assert_eq!(vars.get("branch_slug").unwrap(), "my-feature");
        assert!(!vars.contains_key("issue_title"));
        assert!(!vars.contains_key("site_name"));
        assert!(!vars.contains_key("issue_slug"));
    }

    #[test]
    fn apply_site_template_vars_uses_branch_slug_default() {
        use crate::config::{Config, SiteConfig, SiteProvider};
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "alice/my-feature".into(),
            workspace: "my feature".into(),
            site: None,
        };
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Valet,
                secure: Some(false),
                ..SiteConfig::default()
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let mut vars = build_template_vars(&ctx, &names.path, &names, None);
        let site = apply_site_template_vars(&ctx.config, &mut vars).unwrap();

        assert_eq!(site.name, "repo-my-feature");
        assert_eq!(site.url, "http://repo-my-feature.test");
        assert_eq!(vars.get("site_name").unwrap(), "repo-my-feature");
        assert_eq!(vars.get("site_url").unwrap(), "http://repo-my-feature.test");
    }

    #[test]
    fn apply_site_template_vars_supports_docker_proxy_url_override() {
        use crate::config::{Config, SiteConfig, SiteProvider};
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "alice/my-feature".into(),
            workspace: "my feature".into(),
            site: None,
        };
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::DockerProxy,
                name: Some("{{branch_slug}}.local.test".into()),
                url: Some("https://{{site_name}}".into()),
                ..SiteConfig::default()
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let mut vars = build_template_vars(&ctx, &names.path, &names, None);
        let site = apply_site_template_vars(&ctx.config, &mut vars).unwrap();

        assert_eq!(site.name, "my-feature.local.test");
        assert_eq!(site.url, "https://my-feature.local.test");
    }

    #[test]
    fn apply_site_template_vars_renders_traefik_target() {
        use crate::config::{Config, SiteConfig, SiteProvider};
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "alice/my-feature".into(),
            workspace: "my feature".into(),
            site: None,
        };
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Traefik,
                name: Some("repo-{{branch_slug}}.l".into()),
                url: Some("https://{{site_name}}".into()),
                target: Some("http://127.0.0.1:{{front_port}}".into()),
                ..SiteConfig::default()
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let mut vars = build_template_vars(&ctx, &names.path, &names, None);
        let site = apply_site_template_vars(&ctx.config, &mut vars).unwrap();

        assert_eq!(site.name, "repo-my-feature.l");
        assert_eq!(site.url, "https://repo-my-feature.l");
        let expected = format!("http://127.0.0.1:{}", vars.get("front_port").unwrap());
        assert_eq!(site.target.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn apply_site_template_vars_defaults_traefik_target_to_vite_port() {
        use crate::config::{Config, SiteConfig, SiteProvider};
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "alice/my-feature".into(),
            workspace: "my feature".into(),
            site: None,
        };
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Traefik,
                ..SiteConfig::default()
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let mut vars = build_template_vars(&ctx, &names.path, &names, None);
        let site = apply_site_template_vars(&ctx.config, &mut vars).unwrap();

        let expected = format!("http://127.0.0.1:{}", vars.get("vite_port").unwrap());
        assert_eq!(site.target.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn substitute_env_noop_when_no_env_file() {
        let dir = std::env::temp_dir().join("wt-test-no-env");
        fs::create_dir_all(&dir).ok();

        let mut config = Config::default();
        config.setup.env.insert("KEY".into(), "value".into());

        let vars = HashMap::new();
        assert!(substitute_env(&dir, &config, &vars).is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn substitute_env_noop_when_env_map_empty() {
        let dir = std::env::temp_dir().join("wt-test-empty-env-map");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join(".env"), "KEY=value\n").unwrap();

        let config = Config::default();
        let vars = HashMap::new();
        substitute_env(&dir, &config, &vars).unwrap();

        let result = fs::read_to_string(dir.join(".env")).unwrap();
        assert_eq!(result, "KEY=value\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_setup_opens_workspace_with_agent_command() {
        use crate::config::{AgentCli, AgentConfig, ReadyMode, SubmitMode, WorkspaceConfig};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-agent-command-repo");
        let wt = std::env::temp_dir().join("wt-test-agent-command-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:0"}}"#,
            true,
        );
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("ready ›", true);
        runner.add_response("", true);
        runner.add_response("pane:0", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig::default()),
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: vec![
                    "--model".into(),
                    "{{repo}}-{{branch_slug}}".into(),
                    "--cd".into(),
                    "{{worktree_path}}".into(),
                ],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: HashMap::new(),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: repo.with_file_name("wt-test-agent-command-computed-path"),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };

        run_setup(&ctx, &wt, &names, None, "branch", None, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let workspace_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "cmux" && args.first().is_some_and(|a| a == "new-workspace")
            })
            .expect("expected new-workspace call");
        let command_arg = workspace_call
            .1
            .iter()
            .position(|arg| arg == "--command")
            .and_then(|idx| workspace_call.1.get(idx + 1))
            .unwrap();
        let focus_arg = workspace_call
            .1
            .iter()
            .position(|arg| arg == "--focus")
            .and_then(|idx| workspace_call.1.get(idx + 1))
            .unwrap();
        assert_eq!(
            command_arg,
            &format!(
                "export WT_AGENT_ID=agents/issue-1-test; codex --model wt-test-agent-command-repo-issue-1-test --cd {}",
                wt.display()
            )
        );
        assert_eq!(focus_arg, "false");

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn run_setup_opens_workspace_with_claude_agent_identity_env() {
        use crate::config::{AgentCli, AgentConfig, ReadyMode, SubmitMode, WorkspaceConfig};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::Path;
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let repo = std::env::temp_dir().join("wt-test-claude-agent-env-repo");
        let wt = std::env::temp_dir().join("wt-test-claude-agent-env-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            r#"{"caller":{"window_ref":"window:1","workspace_ref":"workspace:0"}}"#,
            true,
        );
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("ready ❯", true);
        runner.add_response("", true);
        runner.add_response("pane:0", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig::default()),
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: HashMap::new(),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: repo.with_file_name("wt-test-claude-agent-env-computed-path"),
            branch: "alice/issue-1-test".into(),
            workspace: "test".into(),
            site: None,
        };

        run_setup(&ctx, &wt, &names, None, "branch", None, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let workspace_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "cmux" && args.first().is_some_and(|a| a == "new-workspace")
            })
            .expect("expected new-workspace call");
        let command_arg = workspace_call
            .1
            .iter()
            .position(|arg| arg == "--command")
            .and_then(|idx| workspace_call.1.get(idx + 1))
            .unwrap();
        assert_eq!(
            command_arg,
            "export WT_AGENT_ID=agents/issue-1-test; claude"
        );

        fs::remove_dir_all(&repo).ok();
        fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn bootstrap_agent_waits_for_codex_ready_and_submits_with_enter_key() {
        use crate::config::{AgentCli, AgentConfig, ReadyMode, SubmitMode};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let mut runner = MockRunner::new();
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("ready ›", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let agent = AgentConfig {
            cli: AgentCli::Codex,
            args: Vec::new(),
            command: None,
            ready: ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::from([(
                "issue".into(),
                vec!["start {{api_url}} on {{task_agent_cmux_surface}}\n".into()],
            )]),
            ..AgentConfig::default()
        };
        let vars = HashMap::from([("api_url".into(), "http://127.0.0.1:15001".into())]);

        bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &vars).unwrap();

        let calls = runner.calls.lock().unwrap();
        let send_call = calls
            .iter()
            .find(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|a| a == "send"))
            .expect("expected cmux send call");
        assert_eq!(
            send_call.1.last().unwrap(),
            "start http://127.0.0.1:15001 on surface:0"
        );
        let send_key_call = calls
            .iter()
            .find(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|a| a == "send-key"))
            .expect("expected cmux send-key call");
        assert_eq!(
            send_key_call.1,
            vec![
                "send-key",
                "--surface",
                "surface:0",
                "--workspace",
                "workspace:1",
                "enter"
            ]
        );
    }

    #[test]
    fn bootstrap_agent_waits_for_claude_ready_and_submits_with_enter_key() {
        use crate::config::{AgentCli, AgentConfig, ReadyMode, SubmitMode};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let mut runner = MockRunner::new();
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("ready ❯", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let agent = AgentConfig {
            cli: AgentCli::Claude,
            args: Vec::new(),
            command: None,
            ready: ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::from([(
                "issue".into(),
                vec!["claude start {{api_url}} on {{task_agent_cmux_surface}}\n".into()],
            )]),
            ..AgentConfig::default()
        };
        let vars = HashMap::from([("api_url".into(), "http://127.0.0.1:15002".into())]);

        bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &vars).unwrap();

        let calls = runner.calls.lock().unwrap();
        let cmux_calls: Vec<&(String, Vec<String>, Option<PathBuf>)> =
            calls.iter().filter(|(cmd, _, _)| cmd == "cmux").collect();
        let send_idx = cmux_calls
            .iter()
            .position(|(_, args, _)| args.first().is_some_and(|a| a == "send"))
            .expect("expected cmux send call");
        let send_key_idx = cmux_calls
            .iter()
            .position(|(_, args, _)| args.first().is_some_and(|a| a == "send-key"))
            .expect("expected cmux send-key call");
        assert!(
            send_idx < send_key_idx,
            "send must precede send-key for claude auto submit"
        );
        let send_call = cmux_calls[send_idx];
        assert_eq!(
            send_call.1.last().unwrap(),
            "claude start http://127.0.0.1:15002 on surface:0"
        );
        let send_key_call = cmux_calls[send_key_idx];
        assert_eq!(
            send_key_call.1,
            vec![
                "send-key",
                "--surface",
                "surface:0",
                "--workspace",
                "workspace:1",
                "enter"
            ]
        );
    }

    #[test]
    fn bootstrap_agent_waits_for_gemini_ready_and_submits_with_enter_key() {
        use crate::config::{AgentCli, AgentConfig, ReadyMode, SubmitMode};
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        struct SharedRunner {
            inner: Arc<MockRunner>,
        }

        impl CommandRunner for SharedRunner {
            fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
                self.inner.run(cmd, args, cwd)
            }

            fn has_command(&self, cmd: &str) -> bool {
                self.inner.has_command(cmd)
            }
        }

        let mut runner = MockRunner::new();
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("gemini ready", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let agent = AgentConfig {
            cli: AgentCli::Gemini,
            args: Vec::new(),
            command: None,
            ready: ReadyMode::Marker("gemini ready".into()),
            submit: SubmitMode::Auto,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::from([(
                "issue".into(),
                vec!["gemini start {{api_url}} on {{task_agent_cmux_surface}}\n".into()],
            )]),
            ..AgentConfig::default()
        };
        let vars = HashMap::from([("api_url".into(), "http://127.0.0.1:15003".into())]);

        bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &vars).unwrap();

        let calls = runner.calls.lock().unwrap();
        let cmux_calls: Vec<&(String, Vec<String>, Option<PathBuf>)> =
            calls.iter().filter(|(cmd, _, _)| cmd == "cmux").collect();
        let send_idx = cmux_calls
            .iter()
            .position(|(_, args, _)| args.first().is_some_and(|a| a == "send"))
            .expect("expected cmux send call");
        let send_key_idx = cmux_calls
            .iter()
            .position(|(_, args, _)| args.first().is_some_and(|a| a == "send-key"))
            .expect("expected cmux send-key call");
        assert!(
            send_idx < send_key_idx,
            "send must precede send-key for gemini auto submit"
        );
        let send_call = cmux_calls[send_idx];
        assert_eq!(
            send_call.1.last().unwrap(),
            "gemini start http://127.0.0.1:15003 on surface:0"
        );
        let send_key_call = cmux_calls[send_key_idx];
        assert_eq!(
            send_key_call.1,
            vec![
                "send-key",
                "--surface",
                "surface:0",
                "--workspace",
                "workspace:1",
                "enter"
            ]
        );
    }

    #[test]
    fn bootstrap_agent_no_configured_prompts_is_noop() {
        let runner = Arc::new(MockRunner::new());
        let ctx = bootstrap_test_ctx(Arc::clone(&runner));
        let agent = AgentConfig {
            cli: AgentCli::Codex,
            args: Vec::new(),
            command: None,
            ready: crate::config::ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::new(),
            ..AgentConfig::default()
        };

        bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &HashMap::new()).unwrap();

        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn bootstrap_agent_unchanged_screen_before_later_prompt_fails() {
        let mut runner = MockRunner::new();
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("", true);
        runner.add_response("same screen", true);
        runner.add_response("same screen", true);
        let runner = Arc::new(runner);
        let ctx = bootstrap_test_ctx(Arc::clone(&runner));
        let agent = AgentConfig {
            cli: AgentCli::Gemini,
            args: Vec::new(),
            command: None,
            ready: crate::config::ReadyMode::Auto,
            submit: SubmitMode::None,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::from([(
                "issue".into(),
                vec!["first prompt".into(), "second prompt".into()],
            )]),
            ..AgentConfig::default()
        };

        let err = bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &HashMap::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("Agent prompt 2/2 failed"));
        assert!(err.contains("unchanged screen"));
        let send_calls = runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|a| a == "send"))
            .count();
        assert_eq!(send_calls, 1);
    }

    #[test]
    fn bootstrap_agent_ready_marker_timeout_fails() {
        let mut runner = MockRunner::new();
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("not ready", true);
        let runner = Arc::new(runner);
        let ctx = bootstrap_test_ctx(Arc::clone(&runner));
        let agent = AgentConfig {
            cli: AgentCli::Codex,
            args: Vec::new(),
            command: None,
            ready: crate::config::ReadyMode::Marker("READY".into()),
            submit: SubmitMode::Auto,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::from([("issue".into(), vec!["first prompt".into()])]),
            ..AgentConfig::default()
        };

        let err = bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &HashMap::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("Agent prompt 1/1 failed"));
        assert!(err.contains("ready marker timeout"));
        assert!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|(cmd, args, _)| cmd != "cmux" || args.first().is_none_or(|a| a != "send"))
        );
    }

    #[test]
    fn bootstrap_agent_delivers_all_configured_prompts() {
        let mut runner = MockRunner::new();
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("", true);
        runner.add_response("processing first prompt", true);
        runner.add_response("ready for second prompt", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);
        let ctx = bootstrap_test_ctx(Arc::clone(&runner));
        let agent = AgentConfig {
            cli: AgentCli::Gemini,
            args: Vec::new(),
            command: None,
            ready: crate::config::ReadyMode::Auto,
            submit: SubmitMode::None,
            timeout: 1,
            send_after: 0,
            prompt: HashMap::from([(
                "issue".into(),
                vec!["first prompt".into(), "second prompt".into()],
            )]),
            ..AgentConfig::default()
        };

        bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &HashMap::new()).unwrap();

        let calls = runner.calls.lock().unwrap();
        let send_prompts = calls
            .iter()
            .filter(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|a| a == "send"))
            .map(|(_, args, _)| args.last().cloned().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            send_prompts,
            vec!["first prompt".to_string(), "second prompt".to_string()]
        );
    }

    #[test]
    fn inject_local_context_appends_rendered_template() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join("wt-test-inject-context");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("CLAUDE.local.md"), "# Existing content\n").unwrap();

        let mut runner = MockRunner::new();
        // get_branch_parent: git config --get
        runner.add_response("develop", true);

        let mut config = Config::default();
        config.worktree.inject_local_context = Some(
            "\n## env\n- parent: `{{parent_branch}}`\n- site: {{site_url}}\n- ws: `{{workspace}}`\n".into(),
        );
        config.agent = Some(agent_config(AgentCli::Claude));

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "alice/proj-680-feature".into(),
            workspace: "feature".into(),
            site: Some("sample-app-proj-680".into()),
        };
        let mut vars = build_template_vars(&ctx, &names.path, &names, Some("feature"));
        vars.insert("site_url".into(), "https://sample-app-proj-680.test".into());

        inject_local_context(&ctx, &ctx.config, &dir, &names, &vars, Some("workspace:3")).unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        assert!(result.starts_with("# Existing content\n"));
        assert!(result.contains("- parent: `develop`"));
        assert!(result.contains("- site: https://sample-app-proj-680.test"));
        assert!(result.contains("- ws: `workspace:3`"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_local_context_appends_to_codex_context_file() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join("wt-test-inject-codex-context");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("AGENTS.override.md"), "# Codex\n").unwrap();
        fs::write(dir.join("CLAUDE.local.md"), "# Claude\n").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("main", true);

        let mut config = Config::default();
        config.worktree.inject_local_context =
            Some("\n## env\n- parent: `{{parent_branch}}`\n".into());
        config.agent = Some(agent_config(AgentCli::Codex));

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let names = WorktreeNames {
            path: dir.clone(),
            branch: "alice/proj-680-feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        inject_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None).unwrap();

        let codex_result = fs::read_to_string(dir.join("AGENTS.override.md")).unwrap();
        let claude_result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        assert!(codex_result.starts_with("# Codex\n"));
        assert!(codex_result.contains("- parent: `main`"));
        assert_eq!(claude_result, "# Claude\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_local_context_noop_without_config() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join("wt-test-inject-no-config");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("CLAUDE.local.md"), "original\n").unwrap();

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "alice/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        inject_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None).unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        assert_eq!(result, "original\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_local_context_noop_without_file() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join("wt-test-inject-no-file");
        fs::create_dir_all(&dir).ok();
        // No CLAUDE.local.md

        let mut config = Config::default();
        config.worktree.inject_local_context = Some("## env\n".into());
        config.agent = Some(agent_config(AgentCli::Claude));

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "alice/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        assert!(
            inject_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None).is_ok()
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_local_context_handles_missing_vars() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join("wt-test-inject-partial");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("CLAUDE.local.md"), "# test\n").unwrap();

        let mut runner = MockRunner::new();
        // get_branch_parent: not found
        runner.add_response("", false);

        let mut config = Config::default();
        config.worktree.inject_local_context =
            Some("\n## env\n- parent: `{{parent_branch}}`\n- ws: `{{workspace}}`\n".into());
        config.agent = Some(agent_config(AgentCli::Claude));

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "alice/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        // No site, no workspace handle, no parent
        inject_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None).unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        // Unknown vars are left as-is by template::render
        assert!(result.contains("{{parent_branch}}"));
        assert!(result.contains("{{workspace}}"));

        fs::remove_dir_all(&dir).ok();
    }
}
