use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::git::{GitService, WorktreeEntry};
use crate::setup;
use anyhow::Result;
use serde::Serialize;
use std::io::Write;

pub fn run(ctx: &Ctx) -> Result<()> {
    let items = collect(ctx)?;
    if ctx.is_json() {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer_pretty(&mut handle, &items)?;
        writeln!(handle)?;
    } else {
        print_table(&items)?;
    }
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
struct WorktreeRow {
    branch: String,
    path: String,
    current: bool,
    dirty: bool,
    parent: Option<String>,
    ahead: Option<u32>,
    behind: Option<u32>,
    site_url: Option<String>,
}

fn collect(ctx: &Ctx) -> Result<Vec<WorktreeRow>> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));
    let current = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root))
        .current_branch()
        .ok();
    let entries = git.worktree_list()?;

    entries
        .iter()
        .map(|entry| build_row(ctx, &git, entry, current.as_deref()))
        .collect()
}

fn build_row(
    ctx: &Ctx,
    git: &GitService,
    entry: &WorktreeEntry,
    current: Option<&str>,
) -> Result<WorktreeRow> {
    let parent = git.get_branch_parent(&entry.branch)?;
    let (ahead, behind) = parent
        .as_deref()
        .and_then(|parent| ahead_behind(ctx, &entry.branch, parent))
        .unwrap_or((None, None));

    Ok(WorktreeRow {
        branch: entry.branch.clone(),
        path: entry.path.display().to_string(),
        current: current == Some(entry.branch.as_str()),
        dirty: is_dirty(ctx, entry),
        parent,
        ahead,
        behind,
        site_url: site_url(ctx, entry),
    })
}

fn is_dirty(ctx: &Ctx, entry: &WorktreeEntry) -> bool {
    ctx.runner
        .run("git", &["status", "--porcelain"], Some(&entry.path))
        .map(|out| out.success && !out.stdout.trim().is_empty())
        .unwrap_or(false)
}

fn ahead_behind(ctx: &Ctx, branch: &str, parent: &str) -> Option<(Option<u32>, Option<u32>)> {
    let range = format!("{parent}...{branch}");
    let out = ctx
        .runner
        .run(
            "git",
            &["rev-list", "--left-right", "--count", &range],
            Some(&ctx.repo_root),
        )
        .ok()?;
    if !out.success {
        return None;
    }
    let mut parts = out.stdout.split_whitespace();
    let behind = parts.next()?.parse::<u32>().ok()?;
    let ahead = parts.next()?.parse::<u32>().ok()?;
    Some((Some(ahead), Some(behind)))
}

fn site_url(ctx: &Ctx, entry: &WorktreeEntry) -> Option<String> {
    if !ctx.config.has_site() {
        return None;
    }

    let names = WorktreeNames::new(
        &entry.branch,
        &ctx.parent_dir,
        &ctx.repo_name,
        None,
        Some(""),
    );
    let mut vars = setup::build_template_vars(ctx, &names, None);
    setup::apply_site_template_vars(&ctx.config, &mut vars);
    vars.remove("site_url")
}

fn print_table(items: &[WorktreeRow]) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(
        out,
        "{:<2} {:<34} {:<7} {:<18} {:<9} PATH",
        "", "BRANCH", "STATE", "PARENT", "AHEAD"
    )?;
    for item in items {
        let marker = if item.current { "*" } else { " " };
        let state = if item.dirty { "dirty" } else { "clean" };
        let parent = item.parent.as_deref().unwrap_or("-");
        let ahead = match (item.ahead, item.behind) {
            (Some(ahead), Some(behind)) => format!("+{ahead}/-{behind}"),
            _ => "-".into(),
        };
        writeln!(
            out,
            "{:<2} {:<34} {:<7} {:<18} {:<9} {}",
            marker, item.branch, state, parent, ahead, item.path
        )?;
        if let Some(site_url) = item.site_url.as_deref() {
            writeln!(out, "   site: {site_url}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx};
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

    #[test]
    fn collect_reports_dirty_and_ahead_behind_state() {
        let mut runner = MockRunner::new();
        runner.add_response("feature", true); // current branch
        runner.add_response(
            "worktree /tmp/repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/repo-feature\nHEAD def\nbranch refs/heads/feature\n\n",
            true,
        );
        runner.add_response("", false); // main parent
        runner.add_response("", true); // main status
        runner.add_response("main", true); // feature parent
        runner.add_response("1 2", true); // feature ahead/behind
        runner.add_response(" M src/lib.rs", true); // feature status
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo-feature"),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let rows = collect(&ctx).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].branch, "feature");
        assert!(rows[1].current);
        assert!(rows[1].dirty);
        assert_eq!(rows[1].parent.as_deref(), Some("main"));
        assert_eq!(rows[1].ahead, Some(2));
        assert_eq!(rows[1].behind, Some(1));
    }
}
