use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::cmux::CmuxService;
use crate::services::git::GitService;
use crate::services::linear::LinearService;
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

    let items: Vec<String> = additional
        .iter()
        .map(|e| format!("{} [{}]", e.path.display(), e.branch))
        .collect();

    let idx = ctx.ui.select("Select a worktree to open", &items)?;
    let entry = &additional[idx];

    // Try to get title from Linear for tech-NNN branches
    let title = try_fetch_linear_title(ctx, &entry.branch);

    let names = WorktreeNames::new(
        &entry.branch,
        &ctx.parent_dir,
        &ctx.repo_name,
        title.as_deref(),
        ctx.config.herd.as_ref().map(|h| h.site_name.as_str()),
    );

    // Open workspace
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui
            .print_step(&format!("Worktree path: {}", entry.path.display()));
        return Ok(());
    }

    if let Some(ref ws_config) = ctx.config.workspace {
        ctx.ui
            .print_step(&format!("Opening cmux workspace: {}", names.workspace));
        let ws_handle = cmux.new_workspace(&entry.path, &names.workspace, &ws_config.command)?;

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
        }
    } else {
        ctx.ui
            .print_step(&format!("Worktree path: {}", entry.path.display()));
    }

    Ok(())
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
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        assert!(try_fetch_linear_title(&ctx, "hoetaek/my-feature").is_none());
    }
}
