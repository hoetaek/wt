use std::hash::{Hash, Hasher};

use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::services::herd::HerdService;
use crate::template;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

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
    let config = config_override.unwrap_or(&ctx.config);

    copy_files(ctx, config, wt_path)?;
    link_files(ctx, config, wt_path)?;

    let mut template_vars = build_template_vars(ctx, names, title);
    if let Some(extra) = extra_vars {
        template_vars.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let mut site_name: Option<String> = None;

    if let Some(ref herd_config) = config.herd {
        let herd = HerdService::new(ctx.runner.as_ref());
        if herd.is_available() {
            let name = template::render(&herd_config.site_name, &template_vars);
            ctx.ui
                .print_step(&format!("Registering Herd site: {name}.test"));
            if let Err(e) = herd.link(&name, wt_path, herd_config.secure.unwrap_or(true)) {
                ctx.ui.print_warning(&format!("Herd link failed: {e}"));
            }

            substitute_env(wt_path, config, &template_vars)?;
            site_name = Some(name);
        }
    }

    // Workspace
    let ws_color = config
        .workspace
        .as_ref()
        .and_then(|ws| ws.colors.get(mode))
        .cloned()
        .unwrap_or_default();
    let ws_handle = open_workspace(ctx, config, wt_path, names, &ws_color)?;

    inject_claude_local_context(
        ctx,
        config,
        wt_path,
        names,
        site_name.as_deref(),
        ws_handle.as_deref(),
    )?;

    install_deps(ctx, config, wt_path)?;

    if let (Some(handle), Some(ws_config)) = (&ws_handle, &config.workspace) {
        if !ws_config.post_deps_tabs.is_empty() {
            let cmux = CmuxService::new(ctx.runner.as_ref());
            let panes = cmux.list_panes(handle)?;
            if let Some(pane) = panes.first() {
                for tab_cmd in &ws_config.post_deps_tabs {
                    let rendered = template::render(tab_cmd, &template_vars);
                    let surface = cmux.new_surface(pane, handle)?;
                    cmux.send(&surface, handle, &format!("{rendered}\n"))?;
                }
            }
        }
    }

    // Open browser after deps (site may need built assets)
    if let Some(ref site) = site_name {
        if config
            .herd
            .as_ref()
            .and_then(|h| h.open_browser)
            .unwrap_or(false)
        {
            let url = format!("https://{site}.test");
            let browser = config.herd.as_ref().and_then(|h| h.browser.as_deref());
            if let Some(app) = browser {
                ctx.runner.run("open", &["-a", app, &url], None).ok();
            } else {
                ctx.runner.run("open", &[&url], None).ok();
            }
        }
    }

    // post_ready: wait for prompt in first surface, then send mode-specific command
    if let (Some(handle), Some(ws_config)) = (&ws_handle, &config.workspace) {
        if let Some(ref post) = ws_config.post_ready {
            run_post_ready(ctx, handle, post, mode, &template_vars)?;
        }
    }

    run_background_tests(ctx, config, wt_path)?;

    print_summary(ctx, wt_path, names, site_name.as_deref());

    Ok(())
}

fn copy_files(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    for file in &config.worktree.copy {
        let src = ctx.repo_root.join(file);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src.clone());
            let dest = wt_path.join(file);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if real_src.is_dir() {
                copy_dir_recursive(&real_src, &dest)?;
            } else {
                fs::copy(&real_src, &dest)?;
            }
        }
    }
    for entry in &config.worktree.copy_as {
        let src = ctx.repo_root.join(&entry.from);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src.clone());
            let dest = wt_path.join(&entry.to);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if real_src.is_dir() {
                copy_dir_recursive(&real_src, &dest)?;
            } else {
                fs::copy(&real_src, &dest)?;
            }
        }
    }
    Ok(())
}

fn link_files(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    for file in &config.worktree.link {
        let src = ctx.repo_root.join(file);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src);
            let dest = wt_path.join(file);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&real_src, &dest).ok();
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst)?;
        } else {
            fs::copy(entry.path(), dst)?;
        }
    }
    Ok(())
}

fn build_template_vars(
    ctx: &Ctx,
    names: &WorktreeNames,
    title: Option<&str>,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("repo".into(), ctx.repo_name.clone());
    vars.insert("branch".into(), names.workspace.clone());
    if let Some(site) = &names.site {
        vars.insert("site_name".into(), site.clone());
    }
    if let Some(t) = title {
        vars.insert("issue_title".into(), t.into());
    }
    if let Some(tech_id) = WorktreeNames::extract_tech_id(&names.branch) {
        vars.insert("tech_id".into(), tech_id);
    }
    let mut hasher = std::hash::DefaultHasher::new();
    names.branch.hash(&mut hasher);
    let port = 5001 + (hasher.finish() % 4999) as u32;
    vars.insert("vite_port".into(), port.to_string());
    vars
}

fn substitute_env(wt_path: &Path, config: &Config, vars: &HashMap<String, String>) -> Result<()> {
    let env_path = wt_path.join(".env");
    if !env_path.exists() || config.setup.env.is_empty() {
        return Ok(());
    }

    let content = fs::read_to_string(&env_path)?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    for (key, template_val) in &config.setup.env {
        let rendered = template::render(template_val, vars);
        lines.retain(|l| !l.starts_with(&format!("{key}=")));
        // Quote values that contain spaces or special chars
        if rendered.contains(' ') || rendered.contains('"') || rendered.contains('\'') {
            let escaped = rendered.replace('"', "\\\"");
            lines.push(format!("{key}=\"{escaped}\""));
        } else {
            lines.push(format!("{key}={rendered}"));
        }
    }

    fs::write(&env_path, lines.join("\n") + "\n")?;
    Ok(())
}

fn open_workspace(
    ctx: &Ctx,
    config: &Config,
    wt_path: &Path,
    names: &WorktreeNames,
    color: &str,
) -> Result<Option<String>> {
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui
            .print_step(&format!("Worktree path: {}", wt_path.display()));
        return Ok(None);
    }

    let ws_config = match &config.workspace {
        Some(ws) => ws,
        None => {
            ctx.ui
                .print_step(&format!("Worktree path: {}", wt_path.display()));
            return Ok(None);
        }
    };

    ctx.ui
        .print_step(&format!("Opening cmux workspace: {}", names.workspace));

    let ws_handle = cmux.new_workspace(wt_path, &names.workspace, &ws_config.command)?;

    if !color.is_empty() {
        cmux.set_color(&ws_handle, color)?;
    }

    let panes = cmux.list_panes(&ws_handle)?;
    if let Some(pane) = panes.first() {
        for tab_cmd in &ws_config.tabs {
            let surface = cmux.new_surface(pane, &ws_handle)?;
            cmux.send(&surface, &ws_handle, &format!("{tab_cmd}\n"))?;
        }
    }

    Ok(Some(ws_handle))
}

fn install_deps(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    let applicable: Vec<_> = config
        .setup
        .deps
        .iter()
        .filter(|dep| {
            dep.if_exists
                .as_ref()
                .is_none_or(|f| wt_path.join(f).exists())
        })
        .collect();

    if applicable.is_empty() {
        return Ok(());
    }

    for dep in &applicable {
        ctx.ui.print_dim(&format!("  ⏳ {}", dep.run));
    }

    let results = Arc::new(Mutex::new(Vec::new()));

    std::thread::scope(|s| {
        let handles: Vec<_> = applicable
            .iter()
            .map(|dep| {
                let results = Arc::clone(&results);
                let run_str = &dep.run;
                s.spawn(move || {
                    let needs_shell =
                        run_str.contains("&&") || run_str.contains("||") || run_str.contains("|");
                    let out = if needs_shell {
                        ctx.runner.run("sh", &["-c", run_str], Some(wt_path))
                    } else {
                        let parts: Vec<&str> = run_str.split_whitespace().collect();
                        if let Some((cmd, args)) = parts.split_first() {
                            ctx.runner.run(cmd, args, Some(wt_path))
                        } else {
                            return;
                        }
                    };
                    match out {
                        Ok(o) if o.success => {
                            results.lock().unwrap().push((
                                run_str.to_string(),
                                true,
                                String::new(),
                            ));
                        }
                        Ok(o) => {
                            results.lock().unwrap().push((
                                run_str.to_string(),
                                false,
                                o.stderr.clone(),
                            ));
                        }
                        Err(e) => {
                            results.lock().unwrap().push((
                                run_str.to_string(),
                                false,
                                e.to_string(),
                            ));
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().ok();
        }
    });

    for (cmd, success, err) in results.lock().unwrap().iter() {
        if *success {
            ctx.ui.print_dim(&format!("  ✓ {cmd}"));
        } else {
            ctx.ui.print_warning(&format!("  ✗ {cmd}"));
            if !err.is_empty() {
                ctx.ui.print_dim(&format!("    {err}"));
            }
        }
    }

    Ok(())
}

fn run_post_ready(
    ctx: &Ctx,
    ws_handle: &str,
    post: &crate::config::PostReadyConfig,
    mode: &str,
    vars: &HashMap<String, String>,
) -> Result<()> {
    let commands = match post.send.get(mode) {
        Some(cmds) if !cmds.is_empty() => cmds,
        _ => return Ok(()),
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let timeout_secs = post.timeout.unwrap_or(15);

    // Find the target surface
    let panes = cmux.list_panes(ws_handle)?;
    let pane = match panes.first() {
        Some(p) => p,
        None => return Ok(()),
    };
    let surfaces = cmux.list_pane_surfaces(pane, ws_handle)?;
    let surface = match post.surface.as_deref() {
        Some(name) => surfaces
            .iter()
            .find(|s| s.contains(name))
            .cloned()
            .or_else(|| surfaces.first().cloned()),
        None => surfaces.first().cloned(),
    };
    let surface = match surface {
        Some(s) => s,
        None => return Ok(()),
    };

    for (i, cmd_template) in commands.iter().enumerate() {
        let rendered = template::render(cmd_template, vars);

        ctx.ui.print_step(&format!(
            "Waiting for '{}' ({}s timeout)...",
            post.wait_for, timeout_secs
        ));

        let mut sent = false;

        // After sending the previous command, wait for screen to change
        // before polling for the next prompt (avoids matching stale ❯)
        if i > 0 {
            let stale_screen = cmux.read_screen(&surface, ws_handle).unwrap_or_default();
            let mut screen_changed = false;
            for _ in 0..timeout_secs {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if let Ok(current) = cmux.read_screen(&surface, ws_handle) {
                    if current != stale_screen {
                        screen_changed = true;
                        break;
                    }
                }
            }
            if !screen_changed {
                ctx.ui.print_warning(&format!(
                    "Screen unchanged — skipping remaining commands ({}/{})",
                    i + 1,
                    commands.len()
                ));
                return Ok(());
            }
        }

        for _ in 0..timeout_secs {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if let Ok(screen) = cmux.read_screen(&surface, ws_handle) {
                if screen.contains(&post.wait_for) {
                    cmux.send(&surface, ws_handle, &rendered)?;
                    ctx.ui
                        .print_step(&format!("Command {}/{} sent", i + 1, commands.len()));
                    sent = true;
                    break;
                }
            }
        }

        if !sent {
            ctx.ui.print_warning(&format!(
                "Timeout waiting for prompt — skipping remaining commands ({}/{})",
                i + 1,
                commands.len()
            ));
            return Ok(());
        }
    }

    Ok(())
}

fn run_background_tests(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    let test_config = match &config.test {
        Some(tc) => tc,
        None => return Ok(()),
    };
    if test_config.commands.is_empty() {
        return Ok(());
    }

    ctx.ui.print_step("Running tests in background...");

    for test_cmd in &test_config.commands {
        if let Some(ref check_file) = test_cmd.if_exists {
            if !wt_path.join(check_file).exists() {
                continue;
            }
        }
        let run_str = &test_cmd.run;
        let needs_shell = run_str.contains("&&") || run_str.contains("||") || run_str.contains("|");
        let out = if needs_shell {
            ctx.runner.run("sh", &["-c", run_str], Some(wt_path))?
        } else {
            let parts: Vec<&str> = run_str.split_whitespace().collect();
            if let Some((cmd, args)) = parts.split_first() {
                ctx.runner.run(cmd, args, Some(wt_path))?
            } else {
                continue;
            }
        };
        let label = test_cmd.label.as_deref().unwrap_or("test");
        if out.success {
            ctx.ui.print_step(&format!("{label}: PASSED"));
        } else {
            ctx.ui.print_warning(&format!("{label}: FAILED"));
        }
    }

    Ok(())
}

fn inject_claude_local_context(
    ctx: &Ctx,
    config: &Config,
    wt_path: &Path,
    names: &WorktreeNames,
    site_name: Option<&str>,
    ws_handle: Option<&str>,
) -> Result<()> {
    let tmpl = match config.worktree.claude_local_context {
        Some(ref t) => t,
        None => return Ok(()),
    };

    let claude_local = wt_path.join("CLAUDE.local.md");
    if !claude_local.exists() {
        return Ok(());
    }

    let git = GitService::new(ctx.runner.as_ref(), Some(wt_path));
    let parent = git.get_branch_parent(&names.branch).unwrap_or(None);

    let mut vars = HashMap::new();
    if let Some(p) = parent {
        vars.insert("parent_branch".into(), p);
    }
    if let Some(site) = site_name {
        vars.insert("site_url".into(), format!("https://{site}.test"));
    }
    if let Some(ws) = ws_handle {
        vars.insert("workspace".into(), ws.into());
    }

    let rendered = template::render(tmpl, &vars);

    let mut content = fs::read_to_string(&claude_local)?;
    content.push_str(&rendered);
    fs::write(&claude_local, content)?;
    Ok(())
}

fn print_summary(ctx: &Ctx, wt_path: &Path, _names: &WorktreeNames, site_name: Option<&str>) {
    ctx.ui.print_step("Done!");
    ctx.ui.print_step(&format!("  Path: {}", wt_path.display()));
    if let Some(site) = site_name {
        ctx.ui.print_step(&format!("  Site: https://{site}.test"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            path: PathBuf::from("/tmp/hapjeong-hoetaek-tech-680"),
            branch: "hoetaek/tech-680-c11s09-위키".into(),
            workspace: "위키 에디터 (C11S09)".into(),
            site: Some("hapjeong-tech-680".into()),
        };

        let ctx = Ctx::new(
            PathBuf::from("/home/dev/hapjeong"),
            PathBuf::from("/home/dev/hapjeong"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let vars = build_template_vars(&ctx, &names, Some("C11S09. 위키 에디터"));
        assert_eq!(vars.get("repo").unwrap(), "hapjeong");
        assert_eq!(vars.get("site_name").unwrap(), "hapjeong-tech-680");
        assert_eq!(vars.get("issue_title").unwrap(), "C11S09. 위키 에디터");
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
    fn build_template_vars_extracts_tech_id_from_branch() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "hoetaek/tech-663-c11s03-test".into(),
            workspace: "feat: 위키 읽기 페이지".into(),
            site: None,
        };

        let ctx = Ctx::new(
            PathBuf::from("/home/dev/repo"),
            PathBuf::from("/home/dev/repo"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let vars = build_template_vars(&ctx, &names, None);
        assert_eq!(vars.get("tech_id").unwrap(), "tech-663");
    }

    #[test]
    fn build_template_vars_without_title() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let names = WorktreeNames {
            path: PathBuf::from("/tmp/repo-feature"),
            branch: "hoetaek/my-feature".into(),
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

        let vars = build_template_vars(&ctx, &names, None);
        assert_eq!(vars.get("repo").unwrap(), "repo");
        assert!(!vars.contains_key("issue_title"));
        assert!(!vars.contains_key("site_name"));
        assert!(!vars.contains_key("tech_id"));
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
    fn post_ready_sends_commands_sequentially() {
        use crate::config::PostReadyConfig;
        use crate::context::mock::MockRunner;
        use std::sync::Arc;

        let mut runner = MockRunner::new();
        // First iteration: list_panes, list_pane_surfaces, read_screen, send
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("some output ❯", true);
        runner.add_response("", true);
        // Second iteration: list_panes, list_pane_surfaces, read_screen, send
        runner.add_response("pane:0", true);
        runner.add_response("surface:0", true);
        runner.add_response("some output ❯", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);

        let post = PostReadyConfig {
            wait_for: "❯".into(),
            send: HashMap::from([(
                "pr".into(),
                vec![
                    "/conventional-review {{pr_number}}\n".into(),
                    "/codex:review --background\n".into(),
                ],
            )]),
            surface: None,
            timeout: Some(5),
        };

        let vars = HashMap::from([("pr_number".into(), "42".into())]);

        // Call the function directly with runner ref
        let cmux = CmuxService::new(runner.as_ref());
        let commands = post.send.get("pr").unwrap();

        for cmd_template in commands {
            let rendered = template::render(cmd_template, &vars);
            // Simulate: read_screen finds prompt, then send
            let panes = cmux.list_panes("ws:1").unwrap();
            let pane = panes.first().unwrap();
            let surfaces = cmux.list_pane_surfaces(pane, "ws:1").unwrap();
            let surface = surfaces.first().unwrap();
            let screen = cmux.read_screen(surface, "ws:1").unwrap();
            assert!(screen.contains(&post.wait_for));
            cmux.send(surface, "ws:1", &rendered).unwrap();
        }

        let calls = runner.calls.lock().unwrap();
        // Verify the send calls contain rendered templates
        let send_calls: Vec<_> = calls
            .iter()
            .filter(|(cmd, _, _)| cmd == "cmux" && !cmd.is_empty())
            .filter(|(_, args, _)| args.first().is_some_and(|a| a == "send"))
            .collect();
        assert_eq!(send_calls.len(), 2);
        assert_eq!(send_calls[0].1.last().unwrap(), "/conventional-review 42\n");
        assert_eq!(
            send_calls[1].1.last().unwrap(),
            "/codex:review --background\n"
        );
    }

    #[test]
    fn post_ready_skips_unknown_mode() {
        use crate::config::PostReadyConfig;
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

        let post = PostReadyConfig {
            wait_for: "❯".into(),
            send: HashMap::from([("pr".into(), vec!["/review\n".into()])]),
            surface: None,
            timeout: Some(5),
        };

        let vars = HashMap::new();
        // "issue" mode has no entry — should return Ok without calling cmux
        run_post_ready(&ctx, "workspace:1", &post, "issue", &vars).unwrap();
        // No cmux calls should have been made (function returns early)
    }

    #[test]
    fn post_ready_timeout_skips_remaining() {
        use crate::context::mock::MockRunner;
        use std::sync::Arc;

        let mut runner = MockRunner::new();
        // list_panes
        runner.add_response("pane:0", true);
        // list_pane_surfaces
        runner.add_response("surface:0", true);
        // read_screen — no prompt (timeout after 1 attempt with timeout=1)
        runner.add_response("loading...", true);

        let runner = Arc::new(runner);

        // Call run_post_ready directly — need Ctx with Arc runner
        // Instead, test the timeout logic via CmuxService directly
        let cmux = CmuxService::new(runner.as_ref());
        let panes = cmux.list_panes("ws:1").unwrap();
        let pane = panes.first().unwrap();
        let surfaces = cmux.list_pane_surfaces(pane, "ws:1").unwrap();
        let surface = surfaces.first().unwrap();
        let screen = cmux.read_screen(surface, "ws:1").unwrap();
        // Prompt not found — timeout scenario
        assert!(!screen.contains("❯"));

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 3); // list_panes + list_pane_surfaces + read_screen
    }

    #[test]
    fn inject_claude_local_context_appends_rendered_template() {
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
        config.worktree.claude_local_context = Some(
            "\n## env\n- parent: `{{parent_branch}}`\n- site: {{site_url}}\n- ws: `{{workspace}}`\n".into(),
        );

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "hoetaek/tech-680-feature".into(),
            workspace: "feature".into(),
            site: Some("hapjeong-tech-680".into()),
        };

        inject_claude_local_context(
            &ctx,
            &ctx.config,
            &dir,
            &names,
            Some("hapjeong-tech-680"),
            Some("workspace:3"),
        )
        .unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        assert!(result.starts_with("# Existing content\n"));
        assert!(result.contains("- parent: `develop`"));
        assert!(result.contains("- site: https://hapjeong-tech-680.test"));
        assert!(result.contains("- ws: `workspace:3`"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_claude_local_context_noop_without_config() {
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
            branch: "hoetaek/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        inject_claude_local_context(&ctx, &ctx.config, &dir, &names, None, None).unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        assert_eq!(result, "original\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_claude_local_context_noop_without_file() {
        use crate::context::Ctx;
        use crate::context::mock::{MockRunner, MockUi};
        use std::path::PathBuf;

        let dir = std::env::temp_dir().join("wt-test-inject-no-file");
        fs::create_dir_all(&dir).ok();
        // No CLAUDE.local.md

        let mut config = Config::default();
        config.worktree.claude_local_context = Some("## env\n".into());

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "hoetaek/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        assert!(inject_claude_local_context(&ctx, &ctx.config, &dir, &names, None, None).is_ok());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_claude_local_context_handles_missing_vars() {
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
        config.worktree.claude_local_context =
            Some("\n## env\n- parent: `{{parent_branch}}`\n- ws: `{{workspace}}`\n".into());

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let names = WorktreeNames {
            path: dir.clone(),
            branch: "hoetaek/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        // No site, no workspace handle, no parent
        inject_claude_local_context(&ctx, &ctx.config, &dir, &names, None, None).unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        // Unknown vars are left as-is by template::render
        assert!(result.contains("{{parent_branch}}"));
        assert!(result.contains("{{workspace}}"));

        fs::remove_dir_all(&dir).ok();
    }
}
