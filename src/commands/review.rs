use crate::commands::{agent_report, task, task_run};
use crate::context::Ctx;
use crate::services::cmux::{CmuxService, CmuxWorkspace};
use crate::services::git::{GitService, WorktreeEntry};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

struct ReviewTarget {
    label: String,
    branch: String,
    worktree: Option<PathBuf>,
    task_run: Option<task_run::TaskRunRecord>,
}

pub fn run(ctx: &Ctx, target: Option<&str>) -> Result<()> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let target = resolve_target(ctx, &git, target)?;
    let parent = git.get_branch_parent(&target.branch)?;
    let status = match target.worktree.as_deref() {
        Some(path) => Some(git.status_porcelain(path)?),
        None => None,
    };

    ctx.ui.print_step(&format!("Review: {}", target.label));
    ctx.ui.print_dim(&format!("  Branch: {}", target.branch));
    match target.worktree.as_deref() {
        Some(path) => ctx.ui.print_dim(&format!("  Worktree: {}", path.display())),
        None => ctx
            .ui
            .print_dim("  Worktree: branch is not checked out in a local worktree"),
    }

    if let Some(record) = latest_task_run_for_target(ctx, &target)? {
        print_task_run(ctx, &record)?;
    } else {
        ctx.ui.print_dim("  TaskRun: none");
    }

    print_worktree_status(ctx, status.as_deref());
    print_cmux_contact(ctx, target.worktree.as_deref());
    print_parent_review(ctx, parent.as_deref(), &target.branch)?;
    print_agent_report_expectation(ctx);
    print_review_checklist(ctx, status.as_deref(), parent.as_deref(), &target.branch)?;

    Ok(())
}

fn resolve_target(ctx: &Ctx, git: &GitService, target: Option<&str>) -> Result<ReviewTarget> {
    let worktrees = git.worktree_list()?;
    match target {
        None => {
            let branch = git.current_branch()?;
            let worktree = worktrees
                .iter()
                .find(|entry| entry.branch == branch)
                .map(|entry| entry.path.clone())
                .or_else(|| Some(ctx.invocation_root.clone()));
            Ok(ReviewTarget {
                label: branch.clone(),
                branch,
                worktree,
                task_run: None,
            })
        }
        Some(raw) => resolve_explicit_target(ctx, git, &worktrees, raw),
    }
}

fn resolve_explicit_target(
    ctx: &Ctx,
    git: &GitService,
    worktrees: &[WorktreeEntry],
    raw: &str,
) -> Result<ReviewTarget> {
    if let Some(path) = existing_directory_target(ctx, raw) {
        let branch = branch_at_path(ctx, &path)?;
        return Ok(ReviewTarget {
            label: raw.to_string(),
            branch,
            worktree: Some(path),
            task_run: None,
        });
    }

    if let Ok(path) = task_run::resolve(ctx, raw) {
        if path.is_file() {
            let run = task_run::read(&path)?;
            let id = task_run_id(&path)?;
            let worktree = worktree_for_branch(worktrees, &run.branch)
                .or_else(|| git.checked_out_path(&run.branch).ok().flatten());
            return Ok(ReviewTarget {
                label: id.clone(),
                branch: run.branch.clone(),
                worktree,
                task_run: Some(task_run::TaskRunRecord { id, path, run }),
            });
        }
    }

    if let Some(entry) = worktrees.iter().find(|entry| worktree_matches(entry, raw)) {
        return Ok(ReviewTarget {
            label: raw.to_string(),
            branch: entry.branch.clone(),
            worktree: Some(entry.path.clone()),
            task_run: None,
        });
    }

    if git.local_branch_exists(raw)? {
        return Ok(ReviewTarget {
            label: raw.to_string(),
            branch: raw.to_string(),
            worktree: git.checked_out_path(raw)?,
            task_run: None,
        });
    }

    bail!("Review target not found: {raw}");
}

fn existing_directory_target(ctx: &Ctx, raw: &str) -> Option<PathBuf> {
    let raw_path = PathBuf::from(raw);
    let mut candidates = Vec::new();
    if raw_path.is_absolute() {
        candidates.push(raw_path);
    } else {
        candidates.push(ctx.invocation_root.join(raw));
        candidates.push(ctx.repo_root.join(raw));
        candidates.push(ctx.parent_dir.join(raw));
    }

    candidates.into_iter().find(|path| path.is_dir())
}

fn branch_at_path(ctx: &Ctx, path: &Path) -> Result<String> {
    let out = ctx
        .runner
        .run("git", &["rev-parse", "--abbrev-ref", "HEAD"], Some(path))?;
    if out.success && !out.stdout.is_empty() {
        Ok(out.stdout)
    } else {
        bail!(
            "Failed to read worktree branch at {}: {}",
            path.display(),
            if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            }
        )
    }
}

fn worktree_for_branch(worktrees: &[WorktreeEntry], branch: &str) -> Option<PathBuf> {
    worktrees
        .iter()
        .find(|entry| entry.branch == branch)
        .map(|entry| entry.path.clone())
}

fn worktree_matches(entry: &WorktreeEntry, raw: &str) -> bool {
    entry.branch == raw
        || entry.path.to_string_lossy() == raw
        || entry
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == raw)
}

fn latest_task_run_for_target(
    ctx: &Ctx,
    target: &ReviewTarget,
) -> Result<Option<task_run::TaskRunRecord>> {
    if let Some(record) = target.task_run.clone() {
        return Ok(Some(record));
    }

    let mut records = task_run::list(ctx)?
        .into_iter()
        .filter(|record| record.run.branch == target.branch)
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.run
            .updated_at
            .cmp(&right.run.updated_at)
            .then_with(|| left.run.created_at.cmp(&right.run.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(records.pop())
}

fn print_task_run(ctx: &Ctx, record: &task_run::TaskRunRecord) -> Result<()> {
    let group = record
        .run
        .group
        .as_deref()
        .map(|group| format!(", group={group}"))
        .unwrap_or_default();
    ctx.ui.print_dim(&format!(
        "  TaskRun: {} (status={}, source={}{})",
        record.id, record.run.status, record.run.source, group
    ));
    if let Some(error) = record.run.error.as_deref() {
        ctx.ui.print_warning(&format!("  TaskRun error: {error}"));
    }

    match task::read_task_document(ctx, &record.run.task) {
        Ok(document) => ctx.ui.print_dim(&format!(
            "  Task: {} ({})",
            task::task_relative_path(&record.run.task),
            document.title_or_key(&record.run.task)
        )),
        Err(err) => ctx.ui.print_warning(&format!(
            "  Task: {} could not be read: {err:#}",
            task::task_relative_path(&record.run.task)
        )),
    }

    Ok(())
}

fn print_worktree_status(ctx: &Ctx, status: Option<&str>) {
    let Some(status) = status else {
        ctx.ui.print_dim("  Worktree status: unavailable");
        return;
    };

    let lines = relevant_status_lines(ctx, status);
    if lines.is_empty() {
        ctx.ui.print_dim("  Worktree status: clean");
        let ignored = ignored_configured_link_lines(ctx, status);
        for line in ignored.iter().take(20) {
            ctx.ui
                .print_dim(&format!("    ignored configured link: {line}"));
        }
        if ignored.len() > 20 {
            ctx.ui
                .print_dim(&format!("    ... {} more ignored", ignored.len() - 20));
        }
    } else {
        ctx.ui
            .print_dim(&format!("  Worktree status: dirty ({} paths)", lines.len()));
        for line in lines.iter().take(20) {
            ctx.ui.print_dim(&format!("    {line}"));
        }
        if lines.len() > 20 {
            ctx.ui
                .print_dim(&format!("    ... {} more", lines.len() - 20));
        }
    }
}

fn print_cmux_contact(ctx: &Ctx, worktree: Option<&Path>) {
    let Some(worktree) = worktree else {
        ctx.ui
            .print_dim("  cmux: unavailable without a checked out worktree");
        return;
    };

    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        ctx.ui.print_dim("  cmux: unavailable");
        return;
    }

    let workspaces = match cmux.list_workspaces() {
        Ok(workspaces) => workspaces,
        Err(err) => {
            ctx.ui
                .print_warning(&format!("  cmux lookup failed: {err:#}"));
            return;
        }
    };

    let matches = workspaces
        .iter()
        .filter(|workspace| cmux_workspace_matches(workspace, worktree))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        ctx.ui.print_dim("  cmux: no workspace found for worktree");
        return;
    }

    for workspace in matches {
        print_cmux_workspace(ctx, &cmux, workspace);
    }
}

fn print_cmux_workspace(ctx: &Ctx, cmux: &CmuxService<'_>, workspace: &CmuxWorkspace) {
    ctx.ui.print_dim(&format!(
        "  cmux workspace: {} \"{}\" (window {})",
        workspace.handle, workspace.title, workspace.window_handle
    ));

    match first_cmux_surface(cmux, &workspace.handle) {
        Ok(Some((pane, surface))) => {
            ctx.ui
                .print_dim(&format!("  cmux surface: {surface} (pane {pane})"));
            ctx.ui.print_dim(&format!(
                "  cmux send: cmux send --workspace {} --surface {} <message>",
                workspace.handle, surface
            ));
            ctx.ui.print_dim(&format!(
                "  cmux enter: cmux send-key --workspace {} --surface {} enter",
                workspace.handle, surface
            ));
        }
        Ok(None) => ctx
            .ui
            .print_dim("  cmux surface: no pane surface found for workspace"),
        Err(err) => ctx
            .ui
            .print_warning(&format!("  cmux surface lookup failed: {err:#}")),
    }
}

fn first_cmux_surface(
    cmux: &CmuxService<'_>,
    workspace_handle: &str,
) -> Result<Option<(String, String)>> {
    let panes = cmux.list_panes(workspace_handle)?;
    let Some(pane) = panes.first() else {
        return Ok(None);
    };
    let surfaces = cmux.list_pane_surfaces(pane, workspace_handle)?;
    let Some(surface) = surfaces.first() else {
        return Ok(None);
    };
    Ok(Some((pane.clone(), surface.clone())))
}

fn cmux_workspace_matches(workspace: &CmuxWorkspace, worktree: &Path) -> bool {
    workspace
        .current_directory
        .as_deref()
        .is_some_and(|cwd| same_path(cwd, worktree))
}

fn print_parent_review(ctx: &Ctx, parent: Option<&str>, branch: &str) -> Result<()> {
    let Some(parent) = parent else {
        ctx.ui
            .print_dim("  Parent: not recorded; committed diff checks skipped");
        return Ok(());
    };

    ctx.ui.print_dim(&format!("  Parent: {parent}"));
    let commit_count = committed_count(ctx, parent, branch)?;
    match commit_count {
        Some(count) => {
            ctx.ui
                .print_dim(&format!("  Commits ahead of parent: {count}"));
            print_commit_log(ctx, parent, branch)?;
            print_diff_stat(ctx, parent, branch)?;
        }
        None => ctx.ui.print_dim(&format!(
            "  Could not compare {branch} against recorded parent {parent}"
        )),
    }

    Ok(())
}

fn print_commit_log(ctx: &Ctx, parent: &str, branch: &str) -> Result<()> {
    let range = format!("{parent}..{branch}");
    let out = ctx.runner.run(
        "git",
        &["log", "--oneline", "--decorate", "--max-count=10", &range],
        Some(&ctx.repo_root),
    )?;
    if out.success && !out.stdout.is_empty() {
        ctx.ui.print_dim("  Recent commits:");
        for line in out.stdout.lines() {
            ctx.ui.print_dim(&format!("    {line}"));
        }
    }
    Ok(())
}

fn print_diff_stat(ctx: &Ctx, parent: &str, branch: &str) -> Result<()> {
    let range = format!("{parent}..{branch}");
    let out = ctx
        .runner
        .run("git", &["diff", "--stat", &range], Some(&ctx.repo_root))?;
    if out.success && !out.stdout.is_empty() {
        ctx.ui.print_dim("  Committed diff stat:");
        for line in out.stdout.lines() {
            ctx.ui.print_dim(&format!("    {line}"));
        }
    } else {
        ctx.ui.print_dim("  Committed diff stat: none");
    }
    Ok(())
}

fn committed_count(ctx: &Ctx, parent: &str, branch: &str) -> Result<Option<usize>> {
    let range = format!("{parent}..{branch}");
    let out = ctx
        .runner
        .run(
            "git",
            &["rev-list", "--count", &range],
            Some(&ctx.repo_root),
        )
        .with_context(|| format!("Failed to compare branch range {range}"))?;
    if !out.success {
        return Ok(None);
    }
    Ok(Some(out.stdout.trim().parse::<usize>().unwrap_or(0)))
}

fn print_agent_report_expectation(ctx: &Ctx) {
    ctx.ui
        .print_step(&format!("Expected {}", agent_report::REPORT_HEADING));
    for item in agent_report::REPORT_ITEMS {
        ctx.ui.print_dim(&format!("  - {item}"));
    }
}

fn print_review_checklist(
    ctx: &Ctx,
    status: Option<&str>,
    parent: Option<&str>,
    branch: &str,
) -> Result<()> {
    ctx.ui.print_step("Review checklist");
    let clean = status.is_some_and(|status| relevant_status_lines(ctx, status).is_empty());
    print_check(ctx, clean, "worktree is clean");

    if let Some(parent) = parent {
        let ahead = committed_count(ctx, parent, branch)?.unwrap_or(0) > 0;
        print_check(ctx, ahead, "branch has committed work ahead of parent");
    } else {
        ctx.ui
            .print_dim("  [?] branch has committed work ahead of parent");
    }

    print_check(ctx, false, "human or agent review has checked the report");
    Ok(())
}

fn print_check(ctx: &Ctx, ok: bool, label: &str) {
    let mark = if ok { "x" } else { " " };
    ctx.ui.print_dim(&format!("  [{mark}] {label}"));
}

fn status_lines(status: &str) -> Vec<&str> {
    status
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn relevant_status_lines<'a>(ctx: &Ctx, status: &'a str) -> Vec<&'a str> {
    status_lines(status)
        .into_iter()
        .filter(|line| !is_configured_link_status_line(ctx, line))
        .collect()
}

fn ignored_configured_link_lines<'a>(ctx: &Ctx, status: &'a str) -> Vec<&'a str> {
    status_lines(status)
        .into_iter()
        .filter(|line| is_configured_link_status_line(ctx, line))
        .collect()
}

fn is_configured_link_status_line(ctx: &Ctx, line: &str) -> bool {
    let path = line.get(3..).unwrap_or("").trim();
    ctx.config
        .worktree
        .link
        .iter()
        .map(|linked| linked.trim_end_matches('/'))
        .any(|linked| path == linked || path.starts_with(&format!("{linked}/")))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn task_run_id(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("TaskRun path is missing a file stem: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::sync::Arc;

    #[test]
    fn review_prints_branch_task_run_status_and_diff() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".local/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".local/tasks/feature.toml"),
            "title = \"Feature\"\nbranch = \"feature\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nsource = \"new\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("main", true);
        runner.add_response(" M src/lib.rs", true);
        runner.add_response("2", true);
        runner.add_response("def add review\nabc add task", true);
        runner.add_response(
            " src/lib.rs | 12 ++++++++++++\n 1 file changed, 12 insertions(+)",
            true,
        );
        runner.add_response("2", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo.clone(),
            worktree.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, Some("feature")).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Review: feature"));
        assert!(steps.contains("Expected Agent Completion Report"));
        assert!(dims.contains("TaskRun: run-feature"));
        assert!(dims.contains("Task: .local/tasks/feature.toml (Feature)"));
        assert!(dims.contains("Parent: main"));
        assert!(dims.contains("Commits ahead of parent: 2"));
        assert!(dims.contains("dirty (1 paths)"));
    }

    #[test]
    fn review_accepts_task_run_id_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".local/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".local/tasks/feature.toml"),
            "title = \"Feature\"\nbranch = \"feature\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nsource = \"stack\"\ngroup = \"stack-1\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response("def add review", true);
        runner.add_response(" src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)", true);
        runner.add_response("1", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, Some("run-feature")).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Review: run-feature"));
        assert!(dims.contains("TaskRun: run-feature"));
        assert!(dims.contains("status=running"));
        assert!(dims.contains("source=stack"));
    }

    #[test]
    fn review_prints_matching_cmux_workspace_and_surface() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response("1", true);
        runner.add_response("def add review", true);
        runner.add_response(" src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)", true);
        runner.add_response("1", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, Some("feature")).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("cmux workspace: workspace:1 \"feature\" (window window:1)"));
        assert!(dims.contains("cmux surface: surface:4 (pane pane:3)"));
        assert!(dims.contains("cmux send --workspace workspace:1 --surface surface:4"));
        assert!(dims.contains("cmux send-key --workspace workspace:1 --surface surface:4 enter"));
    }

    #[test]
    fn review_clean_check_ignores_configured_worktree_links() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.worktree.link = vec![".local".into()];
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(
            relevant_status_lines(&ctx, "?? .local\n?? .local/tasks/a.toml"),
            Vec::<&str>::new()
        );
        assert_eq!(
            relevant_status_lines(&ctx, "?? .local\n M src/lib.rs"),
            vec!["M src/lib.rs"]
        );
    }
}
