use crate::config::Config;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::template;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

pub(crate) fn build_template_vars(
    ctx: &Ctx,
    worktree_path: &Path,
    names: &WorktreeNames,
    title: Option<&str>,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("repo".into(), ctx.repo_name.clone());
    vars.insert(
        "repo_root".into(),
        ctx.repo_root.to_string_lossy().into_owned(),
    );
    vars.insert(
        "worktree_path".into(),
        worktree_path.to_string_lossy().into_owned(),
    );
    vars.insert(
        "worktree_parent".into(),
        worktree_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_string_lossy()
            .into_owned(),
    );
    vars.insert(
        "worktree_name".into(),
        worktree_path
            .file_name()
            .unwrap_or(worktree_path.as_os_str())
            .to_string_lossy()
            .into_owned(),
    );
    vars.insert("branch".into(), names.workspace.clone());
    if let Some(site) = &names.site {
        vars.insert("site_name".into(), site.clone());
    }
    let branch_slug = WorktreeNames::build_branch_slug(&names.branch);
    vars.insert("branch_slug".into(), branch_slug.clone());
    vars.insert("wt_agent_id".into(), format!("agents/{branch_slug}"));
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

pub(super) fn substitute_env(
    wt_path: &Path,
    config: &Config,
    vars: &HashMap<String, String>,
) -> Result<()> {
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
    // Quote values that contain spaces or special chars.
    if rendered.contains(' ') || rendered.contains('"') || rendered.contains('\'') {
        let escaped = rendered.replace('"', "\\\"");
        lines.push(format!("{key}=\"{escaped}\""));
    } else {
        lines.push(format!("{key}={rendered}"));
    }

    fs::write(env_path, lines.join("\n") + "\n")?;
    Ok(())
}
