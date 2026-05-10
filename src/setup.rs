use std::hash::{Hash, Hasher};

use crate::config::{AgentCli, AgentConfig, Config, SiteConfig, SiteProvider, SubmitMode};
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::services::site::{SiteService, provider_label};
use crate::template;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const DEFAULT_SITE_NAME_TEMPLATE: &str = "{{repo}}-{{branch_slug}}";

#[derive(Clone)]
pub(crate) struct SiteDescriptor {
    pub(crate) provider: SiteProvider,
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) root: String,
    pub(crate) secure: bool,
    pub(crate) target: Option<String>,
    pub(crate) open_browser: Option<bool>,
    pub(crate) browser: Option<String>,
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
    let config = config_override.unwrap_or(&ctx.config);

    copy_files(ctx, config, wt_path)?;
    link_files(ctx, config, wt_path)?;

    let mut template_vars = build_template_vars(ctx, names, title);
    if let Some(extra) = extra_vars {
        template_vars.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    let site = apply_site_template_vars(config, &mut template_vars);

    if let Some(ref site) = site {
        let site_service = SiteService::new(ctx.runner.as_ref());
        if site_service.is_available(&site.provider) {
            let site_root = wt_path.join(&site.root);
            let action = if site.provider == SiteProvider::DockerProxy {
                "Using"
            } else {
                "Registering"
            };
            ctx.ui.print_step(&format!(
                "{action} {} site: {}",
                provider_label(&site.provider),
                site.url
            ));
            if let Err(e) = site_service.register(
                &site.provider,
                &site.name,
                &site_root,
                site.secure,
                site.target.as_deref(),
            ) {
                ctx.ui.print_warning(&format!(
                    "{} link failed: {e}",
                    provider_label(&site.provider)
                ));
            }
        } else {
            ctx.ui.print_warning(&format!(
                "{} command not found; skipping site registration",
                provider_label(&site.provider)
            ));
        }
    }

    substitute_env(wt_path, config, &template_vars)?;

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
        &template_vars,
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
    if let Some(ref site) = site {
        open_site_url(ctx, site, None)?;
    }

    open_workspace_url(ctx, config, &template_vars)?;

    if let (Some(handle), Some(agent)) = (&ws_handle, &config.agent) {
        bootstrap_agent(ctx, handle, agent, mode, &template_vars)?;
    }

    run_background_tests(ctx, config, wt_path)?;

    print_summary(ctx, wt_path, names, site.as_ref(), &template_vars);

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

pub(crate) fn build_template_vars(
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
    vars.insert(
        "branch_slug".into(),
        WorktreeNames::build_branch_slug(&names.branch),
    );
    if let Some(t) = title {
        vars.insert("issue_title".into(), t.into());
    }
    if let Some(issue_slug) = WorktreeNames::extract_issue_slug(&names.branch) {
        vars.insert("issue_slug".into(), issue_slug);
    }
    if let Some(issue_key) = WorktreeNames::extract_issue_key(&names.branch) {
        vars.insert("issue_key".into(), issue_key.clone());
        vars.insert("issue_id".into(), issue_key);
    }
    let mut hasher = std::hash::DefaultHasher::new();
    names.branch.hash(&mut hasher);
    let port = 5001 + (hasher.finish() % 4999) as u32;
    let api_port = port + 10000;
    vars.insert("vite_port".into(), port.to_string());
    vars.insert("api_port".into(), api_port.to_string());
    vars.insert("front_port".into(), port.to_string());
    vars.insert("back_port".into(), api_port.to_string());
    vars.insert("site_url".into(), format!("http://127.0.0.1:{port}"));
    vars.insert("api_url".into(), format!("http://127.0.0.1:{api_port}"));
    vars
}

pub(crate) fn apply_site_template_vars(
    config: &Config,
    vars: &mut HashMap<String, String>,
) -> Option<SiteDescriptor> {
    let site = config.effective_site()?;
    let name_template = site.name.as_deref().unwrap_or(DEFAULT_SITE_NAME_TEMPLATE);
    let name = render_site_name(name_template, vars);
    vars.insert("site_name".into(), name.clone());

    let secure = site.secure.unwrap_or(true);
    let url = render_site_url(&site, &name, secure, vars);
    vars.insert("site_url".into(), url.clone());
    let target = render_site_target(&site, vars);

    Some(SiteDescriptor {
        provider: site.provider,
        name,
        url,
        root: site.root.unwrap_or_else(|| ".".into()),
        secure,
        target,
        open_browser: site.open_browser,
        browser: site.browser,
    })
}

fn render_site_name(template_value: &str, vars: &HashMap<String, String>) -> String {
    let rendered = template::render(template_value, vars);
    if rendered.contains("{{") {
        let repo = vars.get("repo").map(String::as_str).unwrap_or("repo");
        let branch_slug = vars
            .get("branch_slug")
            .map(String::as_str)
            .unwrap_or("worktree");
        format!("{repo}-{branch_slug}")
    } else {
        rendered
    }
}

fn render_site_url(
    site: &SiteConfig,
    site_name: &str,
    secure: bool,
    vars: &HashMap<String, String>,
) -> String {
    if let Some(url_template) = site.url.as_deref() {
        return template::render(url_template, vars);
    }

    let scheme = if secure { "https" } else { "http" };
    format!("{scheme}://{site_name}.test")
}

fn render_site_target(site: &SiteConfig, vars: &HashMap<String, String>) -> Option<String> {
    let template_value = match site.target.as_deref() {
        Some(target) => target,
        None if site.provider == SiteProvider::Traefik => "http://127.0.0.1:{{vite_port}}",
        None => return None,
    };
    Some(template::render(template_value, vars))
}

fn substitute_env(wt_path: &Path, config: &Config, vars: &HashMap<String, String>) -> Result<()> {
    let root_env = wt_path.join(".env");

    if !config.setup.env.is_empty() && root_env.exists() {
        for (key, template_val) in &config.setup.env {
            substitute_env_key(&root_env, key, template_val, vars)?;
        }
    }

    for (relative_path, entries) in &config.setup.env_files {
        let env_path = wt_path.join(relative_path);
        if !env_path.exists() {
            continue;
        }

        for (key, template_val) in entries {
            substitute_env_key(&env_path, key, template_val, vars)?;
        }
    }

    Ok(())
}

fn substitute_env_key(
    env_path: &Path,
    key: &str,
    template_val: &str,
    vars: &HashMap<String, String>,
) -> Result<()> {
    let content = fs::read_to_string(env_path)?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    let rendered = template::render(template_val, vars);
    lines.retain(|l| !l.starts_with(&format!("{key}=")));
    // Quote values that contain spaces or special chars
    if rendered.contains(' ') || rendered.contains('"') || rendered.contains('\'') {
        let escaped = rendered.replace('"', "\\\"");
        lines.push(format!("{key}=\"{escaped}\""));
    } else {
        lines.push(format!("{key}={rendered}"));
    }

    fs::write(env_path, lines.join("\n") + "\n")?;
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

    let command = match &config.agent {
        Some(agent) => agent.command_line()?.unwrap_or_default(),
        None => String::new(),
    };
    let ws_handle = cmux.new_workspace(wt_path, &names.workspace, &command)?;

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

fn bootstrap_agent(
    ctx: &Ctx,
    ws_handle: &str,
    agent: &AgentConfig,
    mode: &str,
    vars: &HashMap<String, String>,
) -> Result<()> {
    let prompts = match agent.prompt.get(mode) {
        Some(prompts) if !prompts.is_empty() => prompts,
        _ => return Ok(()),
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let panes = cmux.list_panes(ws_handle)?;
    let pane = match panes.first() {
        Some(pane) => pane,
        None => return Ok(()),
    };
    let surfaces = cmux.list_pane_surfaces(pane, ws_handle)?;
    let surface = match surfaces.first() {
        Some(surface) => surface,
        None => return Ok(()),
    };

    let ready_marker = agent.effective_ready();

    for (i, prompt_template) in prompts.iter().enumerate() {
        if i > 0 {
            let stale_screen = cmux.read_screen(surface, ws_handle).unwrap_or_default();
            let mut screen_changed = false;
            for attempt in 0..agent.timeout {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                if let Ok(current) = cmux.read_screen(surface, ws_handle) {
                    if current != stale_screen {
                        screen_changed = true;
                        break;
                    }
                }
            }
            if !screen_changed {
                ctx.ui.print_warning(&format!(
                    "Screen unchanged — skipping remaining prompts ({}/{})",
                    i + 1,
                    prompts.len()
                ));
                return Ok(());
            }
        }

        if let Some(marker) = &ready_marker {
            ctx.ui.print_step(&format!(
                "Waiting for agent ready marker '{}' ({}s timeout)...",
                marker, agent.timeout
            ));

            let mut ready = false;
            for attempt in 0..agent.timeout {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                if let Ok(screen) = cmux.read_screen(surface, ws_handle) {
                    if screen.contains(marker) {
                        ready = true;
                        break;
                    }
                }
            }

            if !ready {
                ctx.ui.print_warning(&format!(
                    "Timeout waiting for agent ready marker — skipping remaining prompts ({}/{})",
                    i + 1,
                    prompts.len()
                ));
                return Ok(());
            }
        } else if agent.send_after > 0 {
            ctx.ui.print_step(&format!(
                "Waiting {}s before agent prompt...",
                agent.send_after
            ));
            std::thread::sleep(std::time::Duration::from_secs(agent.send_after));
        }

        let rendered = template::render(prompt_template, vars);
        send_agent_prompt(&cmux, surface, ws_handle, agent, rendered)?;
        ctx.ui
            .print_step(&format!("Agent prompt {}/{} sent", i + 1, prompts.len()));
    }

    Ok(())
}

fn send_agent_prompt(
    cmux: &CmuxService,
    surface: &str,
    ws_handle: &str,
    agent: &AgentConfig,
    rendered: String,
) -> Result<()> {
    if should_submit_with_enter_key(agent) {
        let prompt = rendered.trim_end_matches(['\n', '\r']).to_string();
        cmux.send(surface, ws_handle, &prompt)?;
        cmux.send_key(surface, ws_handle, "enter")?;
        return Ok(());
    }

    let prompt = agent.apply_submit_suffix(rendered);
    cmux.send(surface, ws_handle, &prompt)
}

fn should_submit_with_enter_key(agent: &AgentConfig) -> bool {
    matches!(
        (&agent.submit, &agent.cli),
        (SubmitMode::Auto, AgentCli::Codex) | (SubmitMode::CarriageReturn, _)
    )
}

pub(crate) fn open_workspace_url(
    ctx: &Ctx,
    config: &Config,
    vars: &HashMap<String, String>,
) -> Result<Option<String>> {
    let ws = match &config.workspace {
        Some(ws) => ws,
        None => return Ok(None),
    };
    if !ws.open_browser.unwrap_or(true) {
        return Ok(None);
    }
    let url_template = match ws.open_url.as_deref() {
        Some(url) if !url.is_empty() => url,
        _ => return Ok(None),
    };

    let url = template::render(url_template, vars);
    if let Some(browser) = ws.browser.as_deref() {
        ctx.runner.run("open", &["-a", browser, &url], None).ok();
    } else {
        ctx.runner.run("open", &[&url], None).ok();
    }

    Ok(Some(url))
}

pub(crate) fn open_site_url(
    ctx: &Ctx,
    site: &SiteDescriptor,
    already_opened: Option<&str>,
) -> Result<Option<String>> {
    if !site.open_browser.unwrap_or(false) {
        return Ok(None);
    }
    if already_opened == Some(site.url.as_str()) {
        return Ok(None);
    }

    if let Some(app) = site.browser.as_deref() {
        ctx.runner.run("open", &["-a", app, &site.url], None).ok();
    } else {
        ctx.runner.run("open", &[&site.url], None).ok();
    }

    Ok(Some(site.url.clone()))
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
    template_vars: &HashMap<String, String>,
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

    let mut vars = template_vars.clone();
    if let Some(p) = parent {
        vars.insert("parent_branch".into(), p);
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

fn print_summary(
    ctx: &Ctx,
    wt_path: &Path,
    _names: &WorktreeNames,
    site: Option<&SiteDescriptor>,
    vars: &HashMap<String, String>,
) {
    ctx.ui.print_step("Done!");
    ctx.ui.print_step(&format!("  Path: {}", wt_path.display()));
    if let Some(url) = vars.get("site_url") {
        ctx.ui.print_step(&format!("  UI: {url}"));
    }
    if let Some(url) = vars.get("api_url") {
        ctx.ui.print_step(&format!("  API: {url}"));
    }
    if let Some(site) = site {
        ctx.ui.print_step(&format!(
            "  {}: {}",
            provider_label(&site.provider),
            site.url
        ));
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
            path: PathBuf::from("/tmp/sample-app-alice-proj-680"),
            branch: "alice/proj-680-document-editor".into(),
            workspace: "Document editor".into(),
            site: Some("sample-app-proj-680".into()),
        };

        let ctx = Ctx::new(
            PathBuf::from("/home/dev/sample-app"),
            PathBuf::from("/home/dev/sample-app"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let vars = build_template_vars(&ctx, &names, Some("Document editor"));
        assert_eq!(vars.get("repo").unwrap(), "sample-app");
        assert_eq!(vars.get("site_name").unwrap(), "sample-app-proj-680");
        assert_eq!(vars.get("branch_slug").unwrap(), "proj-680-document-editor");
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
    fn run_setup_opens_workspace_url_without_site() {
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

        let repo = std::env::temp_dir().join("wt-test-open-url-repo");
        let wt = std::env::temp_dir().join("wt-test-open-url-worktree");
        fs::create_dir_all(&repo).ok();
        fs::create_dir_all(&wt).ok();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        runner.add_response("", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig {
                open_url: Some("{{site_url}}".into()),
                open_browser: Some(true),
                browser: Some("Google Chrome".into()),
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

        run_setup(&ctx, &wt, &names, None, "new", None, None).unwrap();

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
        let dir = std::env::temp_dir().join("wt-test-env-files-targets");
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
        substitute_env(&dir, &config, &vars).unwrap();

        let root = fs::read_to_string(dir.join(".env")).unwrap();
        let front = fs::read_to_string(dir.join("frontend/.env.development")).unwrap();
        let back = fs::read_to_string(dir.join("backend/.env")).unwrap();

        assert!(root.contains("APP_URL=https://root.test"));
        assert!(front.contains("VITE_API_TARGET=http://127.0.0.1:15001"));
        assert!(front.contains("OTHER=keep"));
        assert!(back.contains("DJANGO_ENV=old"));
        assert!(!back.contains("VITE_API_TARGET="));

        fs::remove_dir_all(&dir).ok();
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

        let vars = build_template_vars(&ctx, &names, None);
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

        let vars = build_template_vars(&ctx, &names, None);
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

        let mut vars = build_template_vars(&ctx, &names, None);
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

        let mut vars = build_template_vars(&ctx, &names, None);
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

        let mut vars = build_template_vars(&ctx, &names, None);
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

        let mut vars = build_template_vars(&ctx, &names, None);
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
        runner.add_response("workspace:1 workspace:1", true);
        runner.add_response("pane:0", true);
        let runner = Arc::new(runner);

        let config = Config {
            workspace: Some(WorkspaceConfig::default()),
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: vec!["--model".into(), "gpt-5.5".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: HashMap::new(),
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

        run_setup(&ctx, &wt, &names, None, "new", None, None).unwrap();

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
            prompt: HashMap::from([("issue".into(), vec!["start {{api_url}}\n".into()])]),
        };
        let vars = HashMap::from([("api_url".into(), "http://127.0.0.1:15001".into())]);

        bootstrap_agent(&ctx, "workspace:1", &agent, "issue", &vars).unwrap();

        let calls = runner.calls.lock().unwrap();
        let send_call = calls
            .iter()
            .find(|(cmd, args, _)| cmd == "cmux" && args.first().is_some_and(|a| a == "send"))
            .expect("expected cmux send call");
        assert_eq!(send_call.1.last().unwrap(), "start http://127.0.0.1:15001");
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
            branch: "alice/proj-680-feature".into(),
            workspace: "feature".into(),
            site: Some("sample-app-proj-680".into()),
        };
        let mut vars = build_template_vars(&ctx, &names, Some("feature"));
        vars.insert("site_url".into(), "https://sample-app-proj-680.test".into());

        inject_claude_local_context(&ctx, &ctx.config, &dir, &names, &vars, Some("workspace:3"))
            .unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        assert!(result.starts_with("# Existing content\n"));
        assert!(result.contains("- parent: `develop`"));
        assert!(result.contains("- site: https://sample-app-proj-680.test"));
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
            branch: "alice/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        inject_claude_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None)
            .unwrap();

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
            branch: "alice/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        assert!(
            inject_claude_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None)
                .is_ok()
        );

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
            branch: "alice/feature".into(),
            workspace: "feature".into(),
            site: None,
        };

        // No site, no workspace handle, no parent
        inject_claude_local_context(&ctx, &ctx.config, &dir, &names, &HashMap::new(), None)
            .unwrap();

        let result = fs::read_to_string(dir.join("CLAUDE.local.md")).unwrap();
        // Unknown vars are left as-is by template::render
        assert!(result.contains("{{parent_branch}}"));
        assert!(result.contains("{{workspace}}"));

        fs::remove_dir_all(&dir).ok();
    }
}
