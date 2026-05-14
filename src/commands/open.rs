use crate::config::{Config, IssueProviderType};
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::services::linear::LinearService;
use crate::{setup, template};
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::path::Path;

pub fn run(ctx: &Ctx, target: Option<&str>) -> Result<()> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    let candidates = load_open_candidates(ctx, &git)?;

    if candidates.is_empty() {
        return Err(anyhow::anyhow!("No workspaces available to open"));
    }

    let idx = match target {
        Some(target) => find_candidate(&candidates, target)?,
        None => {
            let items: Vec<String> = candidates.iter().map(OpenCandidate::label).collect();
            ctx.ui.select("Select a workspace to open", &items)?
        }
    };
    let entry = ensure_candidate_worktree(ctx, &git, &candidates[idx])?;

    open_worktree(ctx, &entry)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OpenCandidate {
    Existing(crate::services::git::WorktreeEntry),
    Local { branch: String },
    Remote { branch: String },
}

impl OpenCandidate {
    fn label(&self) -> String {
        match self {
            Self::Existing(entry) => {
                format!("existing  {}  {}", entry.branch, entry.path.display())
            }
            Self::Local { branch } => format!("local     {branch}"),
            Self::Remote { branch } => format!("remote    origin/{branch}"),
        }
    }

    fn branch(&self) -> &str {
        match self {
            Self::Existing(entry) => &entry.branch,
            Self::Local { branch } | Self::Remote { branch } => branch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchSource {
    Local,
    Remote,
}

fn load_open_candidates(ctx: &Ctx, git: &GitService) -> Result<Vec<OpenCandidate>> {
    let worktrees = git.worktree_list()?;
    let local_branches = git.list_local_branches()?;
    let remote_branches = git.list_remote_branches()?;
    Ok(build_open_candidates(
        worktrees,
        local_branches,
        remote_branches,
        &ctx.invocation_root,
    ))
}

fn build_open_candidates(
    worktrees: Vec<crate::services::git::WorktreeEntry>,
    local_branches: Vec<String>,
    remote_branches: Vec<String>,
    invocation_root: &Path,
) -> Vec<OpenCandidate> {
    let checked_out: HashSet<String> = worktrees.iter().map(|entry| entry.branch.clone()).collect();
    let local_set: HashSet<String> = local_branches.iter().cloned().collect();

    let mut candidates = worktrees
        .into_iter()
        .filter(|entry| !is_current_worktree_path(&entry.path, invocation_root))
        .map(OpenCandidate::Existing)
        .collect::<Vec<_>>();

    let mut local_only = local_branches
        .into_iter()
        .filter(|branch| !checked_out.contains(branch))
        .collect::<Vec<_>>();
    local_only.sort();
    local_only.dedup();
    candidates.extend(
        local_only
            .into_iter()
            .map(|branch| OpenCandidate::Local { branch }),
    );

    let mut remote_only = remote_branches
        .into_iter()
        .filter_map(|remote_ref| origin_branch_name(&remote_ref))
        .filter(|branch| !checked_out.contains(branch) && !local_set.contains(branch))
        .collect::<Vec<_>>();
    remote_only.sort();
    remote_only.dedup();
    candidates.extend(
        remote_only
            .into_iter()
            .map(|branch| OpenCandidate::Remote { branch }),
    );

    candidates
}

fn ensure_candidate_worktree(
    ctx: &Ctx,
    git: &GitService,
    candidate: &OpenCandidate,
) -> Result<crate::services::git::WorktreeEntry> {
    match candidate {
        OpenCandidate::Existing(entry) => Ok(entry.clone()),
        OpenCandidate::Local { branch } => {
            create_branch_worktree(ctx, git, branch, BranchSource::Local)
        }
        OpenCandidate::Remote { branch } => {
            create_branch_worktree(ctx, git, branch, BranchSource::Remote)
        }
    }
}

fn create_branch_worktree(
    ctx: &Ctx,
    git: &GitService,
    branch: &str,
    source: BranchSource,
) -> Result<crate::services::git::WorktreeEntry> {
    let profile_config = load_profile_config_for_branch(ctx, branch)?;
    let config = profile_config.as_ref().unwrap_or(&ctx.config);
    let names = WorktreeNames::new_with_config(
        branch,
        &ctx.parent_dir,
        &ctx.repo_root,
        &ctx.repo_name,
        None,
        config.has_site().then_some(""),
        config.worktree.path.as_deref(),
    )?;

    if names.path.exists() {
        bail!("Worktree path already exists: {}", names.path.display());
    }

    match source {
        BranchSource::Local => {
            ctx.ui
                .print_step(&format!("Creating worktree from local branch: {branch}"));
            git.worktree_add(&names.path, branch)?;
        }
        BranchSource::Remote => {
            ctx.ui.print_step(&format!(
                "Creating worktree from remote branch: origin/{branch}"
            ));
            git.worktree_add_new_branch(&names.path, branch, &format!("origin/{branch}"))?;
        }
    }

    Ok(crate::services::git::WorktreeEntry {
        path: names.path,
        branch: branch.to_string(),
    })
}

fn open_worktree(ctx: &Ctx, entry: &crate::services::git::WorktreeEntry) -> Result<()> {
    let profile_config = load_profile_config_for_branch(ctx, &entry.branch)?;
    let config = profile_config.as_ref().unwrap_or(&ctx.config);
    let title = if matches!(
        config.issues.as_ref().map(|issues| &issues.provider),
        Some(IssueProviderType::Linear)
    ) {
        try_fetch_linear_title(ctx, &entry.path)
    } else {
        None
    };

    let names = WorktreeNames::new(
        &entry.branch,
        &ctx.parent_dir,
        &ctx.repo_name,
        title.as_deref(),
        config.has_site().then_some(""),
    );
    let template_vars = setup::build_template_vars(ctx, &names, title.as_deref());
    let mut template_vars = template_vars;
    let site = setup::apply_site_template_vars(config, &mut template_vars);

    // Open workspace
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui
            .print_step(&format!("Worktree path: {}", entry.path.display()));
        if let Some(site) = site.as_ref() {
            setup::open_site_url(ctx, site, None)?;
        }
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

        let opened_url = setup::open_workspace_url(ctx, config, &template_vars)?;
        if let Some(site) = site.as_ref() {
            setup::open_site_url(ctx, site, opened_url.as_deref())?;
        }
    } else {
        ctx.ui
            .print_step(&format!("Worktree path: {}", entry.path.display()));
        if let Some(site) = site.as_ref() {
            setup::open_site_url(ctx, site, None)?;
        }
    }

    Ok(())
}

fn find_candidate(entries: &[OpenCandidate], target: &str) -> Result<usize> {
    let matches = entries
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate_matches(candidate, target))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [idx] => Ok(*idx),
        [] => bail!("No workspace matches {target:?}"),
        _ => bail!("Multiple workspaces match {target:?}"),
    }
}

fn candidate_matches(candidate: &OpenCandidate, target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() {
        return false;
    }

    let branch = candidate.branch();
    branch == target
        || branch.rsplit('/').next() == Some(target)
        || matches!(
            candidate,
            OpenCandidate::Remote { branch } if format!("origin/{branch}") == target
        )
        || matches!(
            candidate,
            OpenCandidate::Existing(entry)
                if entry.path.to_string_lossy() == target
                    || entry.path.file_name().and_then(|name| name.to_str()) == Some(target)
        )
}

fn origin_branch_name(remote_ref: &str) -> Option<String> {
    let branch = remote_ref.strip_prefix("origin/")?;
    if branch.is_empty() || branch == "HEAD" || branch.starts_with("HEAD ->") {
        None
    } else {
        Some(branch.to_string())
    }
}

fn is_current_worktree_path(path: &Path, invocation_root: &Path) -> bool {
    if path == invocation_root {
        return true;
    }

    match (path.canonicalize(), invocation_root.canonicalize()) {
        (Ok(path), Ok(invocation_root)) => path == invocation_root,
        _ => false,
    }
}

fn load_profile_config_for_branch(ctx: &Ctx, branch: &str) -> Result<Option<Config>> {
    let short = branch.rsplit('/').next().unwrap_or(branch);
    let mut profiles = Config::load_profiles(&ctx.repo_root, &ctx.base_config)?;
    profiles.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

    Ok(profiles
        .into_iter()
        .find(|(name, _)| {
            short
                .strip_suffix(name)
                .is_some_and(|prefix| prefix.ends_with('-'))
        })
        .map(|(_, config)| config))
}

fn try_fetch_linear_title(ctx: &Ctx, worktree_path: &Path) -> Option<String> {
    let linear = LinearService::new(ctx.runner.as_ref(), Some(worktree_path));
    let identifier = linear.current_issue_id().ok()?;
    let issue = linear.get_issue(&identifier).ok()?;
    Some(issue.title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_fetch_linear_title_returns_none_without_issue_id() {
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

        assert!(try_fetch_linear_title(&ctx, Path::new("/tmp/repo")).is_none());
    }

    #[test]
    fn try_fetch_linear_title_uses_linear_cli_issue_id() {
        use crate::config::Config;
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let mut runner = MockRunner::new();
        runner.add_response("PROJ-680", true);
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680-document-editor"}"#,
            true,
        );
        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let title = try_fetch_linear_title(&ctx, Path::new("/tmp/repo-feature"));
        assert_eq!(title.as_deref(), Some("Document editor"));
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
        runner.add_response("main\n", true);
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No workspaces available to open")
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
        runner.add_response("main\nalice/feature\n", true);
        runner.add_response("", true);

        let mut ui = MockUi::new();
        ui.add_select(0);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        assert!(run(&ctx, None).is_ok());
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
        runner.add_response("main\nalice/feature\n", true);
        runner.add_response("", true);
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

        run(&ctx, None).unwrap();

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
    fn open_uses_matching_profile_config_from_branch_suffix() {
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
        let profile_dir = repo.path().join(".local/profiles/codex");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("profile.toml"),
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
        runner.add_response("main\nalice/feature-codex\n", true);
        runner.add_response("", true);
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

        run(&ctx, None).unwrap();

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
    fn open_uses_hyphenated_profile_config_from_branch_suffix() {
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
        let profile_dir = repo.path().join(".local/profiles/codex-yolo");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("profile.toml"),
            r#"
[workspace]
tabs = []

[agent]
cli = "codex"
args = ["--yolo"]
"#,
        )
        .unwrap();

        let worktree = repo.path().with_file_name("repo-feature-codex-yolo");
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}\nHEAD def\nbranch refs/heads/alice/feature-codex-yolo\n\n",
                repo.path().display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("main\nalice/feature-codex-yolo\n", true);
        runner.add_response("", true);
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

        run(&ctx, None).unwrap();

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
        assert_eq!(command_arg, "codex --yolo");
    }

    #[test]
    fn open_falls_back_to_root_config_when_profile_file_is_missing() {
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
        runner.add_response("main\nalice/feature-codex\n", true);
        runner.add_response("", true);
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

        run(&ctx, None).unwrap();

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

    #[test]
    fn open_candidates_exclude_current_and_group_branch_state() {
        let worktrees = vec![
            crate::services::git::WorktreeEntry {
                path: "/tmp/repo".into(),
                branch: "main".into(),
            },
            crate::services::git::WorktreeEntry {
                path: "/tmp/repo-feature".into(),
                branch: "alice/feature".into(),
            },
        ];

        let candidates = build_open_candidates(
            worktrees,
            vec!["main".into(), "alice/feature".into(), "local-only".into()],
            vec![
                "origin/HEAD".into(),
                "origin/main".into(),
                "origin/alice/feature".into(),
                "origin/remote-only".into(),
            ],
            Path::new("/tmp/repo"),
        );
        let labels = candidates
            .iter()
            .map(OpenCandidate::label)
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec![
                "existing  alice/feature  /tmp/repo-feature",
                "local     local-only",
                "remote    origin/remote-only"
            ]
        );
    }

    #[test]
    fn candidate_matching_uses_branch_or_path_not_issue_number() {
        let candidate = OpenCandidate::Existing(crate::services::git::WorktreeEntry {
            path: "/tmp/sample-app-proj-123-fix-editor".into(),
            branch: "alice/proj-123-fix-editor".into(),
        });

        assert!(candidate_matches(&candidate, "alice/proj-123-fix-editor"));
        assert!(candidate_matches(&candidate, "proj-123-fix-editor"));
        assert!(candidate_matches(
            &candidate,
            "sample-app-proj-123-fix-editor"
        ));
        assert!(!candidate_matches(&candidate, "123"));
        assert!(!candidate_matches(&candidate, "PROJ-123"));
    }

    #[test]
    fn open_creates_worktree_for_selected_local_branch() {
        use crate::config::Config;
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

        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let expected = parent.path().join("repo-feature");

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.display()
            ),
            true,
        );
        runner.add_response("main\nfeature\n", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0);

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let worktree_add = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.first().is_some_and(|arg| arg == "worktree")
                    && args.get(1).is_some_and(|arg| arg == "add")
            })
            .expect("expected git worktree add");
        assert_eq!(
            worktree_add.1,
            vec![
                "worktree",
                "add",
                expected.to_string_lossy().as_ref(),
                "feature"
            ]
        );
    }

    #[test]
    fn open_creates_worktree_for_selected_remote_branch() {
        use crate::config::Config;
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

        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let expected = parent.path().join("repo-feature");

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.display()
            ),
            true,
        );
        runner.add_response("main\n", true);
        runner.add_response("origin/HEAD\norigin/main\norigin/feature\n", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0);

        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run(&ctx, None).unwrap();

        let calls = runner.calls.lock().unwrap();
        let worktree_add = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.first().is_some_and(|arg| arg == "worktree")
                    && args.get(1).is_some_and(|arg| arg == "add")
            })
            .expect("expected git worktree add");
        assert_eq!(
            worktree_add.1,
            vec![
                "worktree",
                "add",
                "-b",
                "feature",
                expected.to_string_lossy().as_ref(),
                "origin/feature"
            ]
        );
    }
}
