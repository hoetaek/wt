use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::services::linear::LinearService;
use crate::{setup, template};
use anyhow::Result;

pub fn run(ctx: &Ctx) -> Result<()> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    let entries = git.worktree_list()?;
    let additional: Vec<_> = entries
        .into_iter()
        .filter(|e| e.path != ctx.repo_root)
        .collect();

    if additional.is_empty() {
        return Err(anyhow::anyhow!("No additional worktrees found"));
    }

    let items: Vec<String> = additional.iter().map(|e| e.branch.clone()).collect();

    let idx = ctx.ui.select("Select a worktree to open", &items)?;
    let entry = &additional[idx];

    // Try to get title from Linear for tech-NNN branches
    let title = try_fetch_linear_title(ctx, &entry.branch);
    let variant_config = load_variant_config_for_branch(ctx, &entry.branch)?;
    let config = variant_config.as_ref().unwrap_or(&ctx.config);

    let names = WorktreeNames::new(
        &entry.branch,
        &ctx.parent_dir,
        &ctx.repo_name,
        title.as_deref(),
        config.herd.as_ref().map(|h| h.site_name.as_str()),
    );
    let template_vars = setup::build_template_vars(ctx, &names, title.as_deref());

    // Open workspace
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui
            .print_step(&format!("Worktree path: {}", entry.path.display()));
        return Ok(());
    }

    if let Some(ref ws_config) = config.workspace {
        ctx.ui
            .print_step(&format!("Opening cmux workspace: {}", names.workspace));
        let command = match &config.agent {
            Some(agent) => agent.command_line()?.unwrap_or_default(),
            None => String::new(),
        };
        let ws_handle = cmux.new_workspace(&entry.path, &names.workspace, &command)?;

        let color = ws_config.colors.get("issue").cloned().unwrap_or_default();
        if !color.is_empty() {
            cmux.set_color(&ws_handle, &color)?;
        }

        let panes = cmux.list_panes(&ws_handle)?;
        if let Some(pane) = panes.first() {
            for tab_cmd in &ws_config.tabs {
                let surface = cmux.new_surface(pane, &ws_handle)?;
                cmux.send(&surface, &ws_handle, &format!("{tab_cmd}\n"))?;
            }
            for tab_cmd in &ws_config.post_deps_tabs {
                let rendered = template::render(tab_cmd, &template_vars);
                let surface = cmux.new_surface(pane, &ws_handle)?;
                cmux.send(&surface, &ws_handle, &format!("{rendered}\n"))?;
            }
        }

        setup::open_workspace_url(ctx, config, &template_vars)?;
    } else {
        ctx.ui
            .print_step(&format!("Worktree path: {}", entry.path.display()));
    }

    Ok(())
}

fn load_variant_config_for_branch(ctx: &Ctx, branch: &str) -> Result<Option<Config>> {
    let Some(name) = infer_variant_name(branch) else {
        return Ok(None);
    };
    Config::load_variant(&ctx.repo_root, name)
}

fn infer_variant_name(branch: &str) -> Option<&str> {
    let short = branch.rsplit('/').next().unwrap_or(branch);
    let (_, suffix) = short.rsplit_once('-')?;
    if suffix.is_empty() {
        None
    } else {
        Some(suffix)
    }
}

fn try_fetch_linear_title(ctx: &Ctx, branch: &str) -> Option<String> {
    let tech_id = WorktreeNames::extract_tech_id(branch)?;
    let identifier = format!("TECH-{}", tech_id.strip_prefix("tech-").unwrap_or(&tech_id));

    let linear = LinearService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let issue = linear.get_issue(&identifier).ok()?;
    Some(issue.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_fetch_linear_title_returns_none_for_non_tech_branch() {
        use crate::config::Config;
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        assert!(try_fetch_linear_title(&ctx, "alice/my-feature").is_none());
    }

    #[test]
    fn open_with_no_worktrees_returns_error() {
        use crate::config::Config;
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No additional worktrees")
        );
    }

    #[test]
    fn open_without_cmux_prints_path() {
        use crate::config::Config;
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let mut runner = MockRunner::new();
        runner.add_response(
            "worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );

        let mut ui = MockUi::new();
        ui.add_select(0);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        assert!(run(&ctx).is_ok());
    }

    #[test]
    fn open_starts_post_deps_tabs_and_opens_workspace_url() {
        use crate::config::{
            AgentCli, AgentConfig, Config, ReadyMode, SubmitMode, WorkspaceConfig,
        };
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
        runner.add_command("cmux");
        runner.add_response(
            "worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/repo-feature\nHEAD def\nbranch refs/heads/alice/feature\n\n",
            true,
        );
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        runner.add_response("surface:1", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0);

        let config = Config {
            workspace: Some(WorkspaceConfig {
                post_deps_tabs: vec!["echo {{site_url}} {{api_url}}".into()],
                open_url: Some("{{site_url}}".into()),
                open_browser: Some(true),
                ..WorkspaceConfig::default()
            }),
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: vec!["--model".into(), "gpt-5.5".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx).unwrap();

        let calls = runner.calls.lock().unwrap();
        let send_call = calls
            .iter()
            .find(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|a| a == "send"))
            .expect("expected post deps command to be sent");
        let sent = send_call.1.last().unwrap();
        assert!(sent.starts_with("echo http://127.0.0.1:"));
        assert!(!sent.contains("{{site_url}}"));
        assert!(!sent.contains("{{api_url}}"));

        let open_call = calls
            .iter()
            .find(|(cmd, _, _)| cmd == "open")
            .expect("expected open command");
        assert!(open_call.1[0].starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn open_uses_matching_variant_config_from_branch_suffix() {
        use crate::config::{
            AgentCli, AgentConfig, Config, ReadyMode, SubmitMode, WorkspaceConfig,
        };
        use crate::context::mock::{MockRunner, MockUi};
        use crate::context::{CmdOutput, CommandRunner, Ctx};
        use anyhow::Result;
        use std::fs;
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

        let repo = tempfile::tempdir().unwrap();
        let local_dir = repo.path().join(".local");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(
            local_dir.join(".wt.codex.toml"),
            r#"
[workspace]
tabs = []

[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
"#,
        )
        .unwrap();

        let worktree = repo.path().with_file_name("repo-feature-codex");
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/feature-codex\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0);

        let root_config = Config {
            workspace: Some(WorkspaceConfig::default()),
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            root_config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx).unwrap();

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
        assert_eq!(command_arg, "codex --model gpt-5.5");
    }

    #[test]
    fn open_falls_back_to_root_config_when_variant_file_is_missing() {
        use crate::config::{
            AgentCli, AgentConfig, Config, ReadyMode, SubmitMode, WorkspaceConfig,
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

        let repo = tempfile::tempdir().unwrap();
        let worktree = repo.path().with_file_name("repo-feature-codex");
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/feature-codex\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0);

        let root_config = Config {
            workspace: Some(WorkspaceConfig::default()),
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            root_config,
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx).unwrap();

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
        assert_eq!(command_arg, "claude");
    }
}
