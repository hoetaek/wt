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

    if let Some(ref herd_config) = ctx.config.herd {
        let herd = HerdService::new(ctx.runner.as_ref());
        if herd.is_available() {
            let site_name = template::render(&herd_config.site_name, &template_vars);
            ctx.ui
                .print_step(&format!("Registering Herd site: {site_name}.test"));
            herd.link(&site_name, wt_path, herd_config.secure.unwrap_or(true))?;

            substitute_env(wt_path, &ctx.config, &template_vars)?;
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
    open_workspace(ctx, wt_path, names, &ws_color)?;

    install_deps(ctx, wt_path)?;
    run_background_tests(ctx, wt_path)?;

    print_summary(ctx, wt_path, names);

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
    if let Some(tech_id) = WorktreeNames::extract_tech_id(&names.workspace) {
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
        lines.push(format!("{key}={rendered}"));
    }

    fs::write(&env_path, lines.join("\n") + "\n")?;
    Ok(())
}

fn open_workspace(ctx: &Ctx, wt_path: &Path, names: &WorktreeNames, color: &str) -> Result<()> {
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui
            .print_step(&format!("Worktree path: {}", wt_path.display()));
        return Ok(());
    }

    let ws_config = match &ctx.config.workspace {
        Some(ws) => ws,
        None => {
            ctx.ui
                .print_step(&format!("Worktree path: {}", wt_path.display()));
            return Ok(());
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

    Ok(())
}

fn install_deps(ctx: &Ctx, wt_path: &Path) -> Result<()> {
    if ctx.config.setup.deps.is_empty() {
        return Ok(());
    }
    ctx.ui.print_step("Installing dependencies...");

    for dep in &ctx.config.setup.deps {
        if let Some(ref check_file) = dep.if_exists {
            if !wt_path.join(check_file).exists() {
                continue;
            }
        }
        let parts: Vec<&str> = dep.run.split_whitespace().collect();
        if let Some((cmd, args)) = parts.split_first() {
            let out = ctx.runner.run(cmd, args, Some(wt_path))?;
            if !out.success {
                ctx.ui
                    .print_warning(&format!("Dependency command failed: {}", dep.run));
            }
        }
    }

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
        let parts: Vec<&str> = test_cmd.run.split_whitespace().collect();
        if let Some((cmd, args)) = parts.split_first() {
            let out = ctx.runner.run(cmd, args, Some(wt_path))?;
            let label = test_cmd.label.as_deref().unwrap_or("test");
            if out.success {
                ctx.ui.print_step(&format!("{label}: PASSED"));
            } else {
                ctx.ui.print_warning(&format!("{label}: FAILED"));
            }
        }
    }

    Ok(())
}

fn print_summary(ctx: &Ctx, wt_path: &Path, names: &WorktreeNames) {
    ctx.ui.print_step("Done!");
    ctx.ui.print_step(&format!("  Path: {}", wt_path.display()));
    if let Some(ref site) = names.site {
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
        assert!(result.contains("APP_NAME=New Name"));
        assert!(!result.contains("http://old"));

        fs::remove_dir_all(&dir).ok();
    }
}
