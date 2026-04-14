use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::CmuxService;
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
) -> Result<()> {
    copy_files(ctx, wt_path)?;
    link_files(ctx, wt_path)?;
    copy_claude_files(ctx, wt_path)?;

    let template_vars = build_template_vars(ctx, names, title);
    let mut site_name: Option<String> = None;

    if let Some(ref herd_config) = ctx.config.herd {
        let herd = HerdService::new(ctx.runner.as_ref());
        if herd.is_available() {
            let name = template::render(&herd_config.site_name, &template_vars);
            ctx.ui
                .print_step(&format!("Registering Herd site: {name}.test"));
            herd.link(&name, wt_path, herd_config.secure.unwrap_or(true))?;

            substitute_env(wt_path, &ctx.config, &template_vars)?;
            site_name = Some(name);
        }
    }

    // Workspace
    let ws_color = ctx
        .config
        .workspace
        .as_ref()
        .and_then(|ws| ws.colors.get(mode))
        .cloned()
        .unwrap_or_default();
    let ws_handle = open_workspace(ctx, wt_path, names, &ws_color)?;

    install_deps(ctx, wt_path)?;

    // Open browser after deps (site may need built assets)
    if let Some(ref site) = site_name {
        if ctx
            .config
            .herd
            .as_ref()
            .and_then(|h| h.open_browser)
            .unwrap_or(false)
        {
            let url = format!("https://{site}.test");
            let browser = ctx.config.herd.as_ref().and_then(|h| h.browser.as_deref());
            if let Some(app) = browser {
                ctx.runner.run("open", &["-a", app, &url], None).ok();
            } else {
                ctx.runner.run("open", &[&url], None).ok();
            }
        }
    }

    // post_ready: wait for prompt in first surface, then send mode-specific command
    if let (Some(handle), Some(ws_config)) = (&ws_handle, &ctx.config.workspace) {
        if let Some(ref post) = ws_config.post_ready {
            run_post_ready(ctx, handle, post, mode)?;
        }
    }

    run_background_tests(ctx, wt_path)?;

    print_summary(ctx, wt_path, names, site_name.as_deref());

    Ok(())
}

fn copy_files(ctx: &Ctx, wt_path: &Path) -> Result<()> {
    for file in &ctx.config.worktree.copy {
        let src = ctx.repo_root.join(file);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src.clone());
            fs::copy(&real_src, wt_path.join(file))?;
        }
    }
    Ok(())
}

fn link_files(ctx: &Ctx, wt_path: &Path) -> Result<()> {
    for file in &ctx.config.worktree.link {
        let src = ctx.repo_root.join(file);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src);
            let dest = wt_path.join(file);
            #[cfg(unix)]
            std::os::unix::fs::symlink(&real_src, &dest).ok();
        }
    }
    Ok(())
}

fn copy_claude_files(ctx: &Ctx, wt_path: &Path) -> Result<()> {
    for item in &ctx.config.worktree.claude_copy {
        let src = ctx.repo_root.join(".claude").join(item);
        if src.exists() {
            let dest = wt_path.join(".claude").join(item);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if src.is_dir() {
                copy_dir_recursive(&src, &dest)?;
            } else {
                fs::copy(&src, &dest)?;
            }
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

    let ws_config = match &ctx.config.workspace {
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

fn install_deps(ctx: &Ctx, wt_path: &Path) -> Result<()> {
    let applicable: Vec<_> = ctx
        .config
        .setup
        .deps
        .iter()
        .filter(|dep| {
            dep.if_exists
                .as_ref()
                .map_or(true, |f| wt_path.join(f).exists())
        })
        .collect();

    if applicable.is_empty() {
        return Ok(());
    }
    ctx.ui.print_step("Installing dependencies...");

    let warnings = Arc::new(Mutex::new(Vec::new()));

    std::thread::scope(|s| {
        let handles: Vec<_> = applicable
            .iter()
            .map(|dep| {
                let warnings = Arc::clone(&warnings);
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
                        Ok(o) if !o.success => {
                            warnings
                                .lock()
                                .unwrap()
                                .push(format!("Dependency command failed: {run_str}"));
                        }
                        Err(e) => {
                            warnings
                                .lock()
                                .unwrap()
                                .push(format!("Dependency command error: {run_str}: {e}"));
                        }
                        _ => {}
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().ok();
        }
    });

    for w in warnings.lock().unwrap().iter() {
        ctx.ui.print_warning(w);
    }

    Ok(())
}

fn run_post_ready(
    ctx: &Ctx,
    ws_handle: &str,
    post: &crate::config::PostReadyConfig,
    mode: &str,
) -> Result<()> {
    let send_cmd = match post.send.get(mode) {
        Some(cmd) => cmd.clone(),
        None => return Ok(()),
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let timeout_secs = post.timeout.unwrap_or(15);

    // Find the target surface (first surface of first pane if not specified)
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

    ctx.ui.print_step(&format!(
        "Waiting for '{}' ({}s timeout)...",
        post.wait_for, timeout_secs
    ));

    for _ in 0..timeout_secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if let Ok(screen) = cmux.read_screen(&surface, ws_handle) {
            if screen.contains(&post.wait_for) {
                cmux.send(&surface, ws_handle, &send_cmd)?;
                ctx.ui.print_step("Post-ready command sent");
                return Ok(());
            }
        }
    }

    ctx.ui.print_warning("Post-ready timeout — skipped");
    Ok(())
}

fn run_background_tests(ctx: &Ctx, wt_path: &Path) -> Result<()> {
    let test_config = match &ctx.config.test {
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
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let vars = build_template_vars(&ctx, &names, None);
        assert_eq!(vars.get("repo").unwrap(), "repo");
        assert!(vars.get("issue_title").is_none());
        assert!(vars.get("site_name").is_none());
        assert!(vars.get("tech_id").is_none());
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
}
