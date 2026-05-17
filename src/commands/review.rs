use crate::commands::{agent_report, task, task_run};
use crate::context::Ctx;
use crate::services::git::GitService;
use crate::services::work;
use anyhow::{Context, Result};
use std::path::Path;

pub(crate) type ReviewTarget = work::WorkTarget;
pub(crate) type CmuxContact = work::CmuxContact;

pub fn run(ctx: &Ctx, target: Option<&str>) -> Result<()> {
    let work = work::observe_work(ctx, target)?;
    let target = &work.target;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
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

    print_task_runs(ctx, &task_runs_for_target(ctx, target)?)?;

    print_worktree_status(ctx, status.as_deref());
    print_cmux_work(ctx, &work);
    print_parent_review(ctx, parent.as_deref(), &target.branch)?;
    print_agent_report_expectation(ctx);
    print_review_checklist(ctx, status.as_deref(), parent.as_deref(), &target.branch)?;

    Ok(())
}

pub(crate) fn resolve_review_target(ctx: &Ctx, target: Option<&str>) -> Result<ReviewTarget> {
    work::resolve_target(ctx, target)
}

fn task_runs_for_target(ctx: &Ctx, target: &ReviewTarget) -> Result<Vec<task_run::TaskRunRecord>> {
    if let Some(record) = target.task_run.clone() {
        return Ok(vec![record]);
    }

    let mut records = task_run::list(ctx)?
        .into_iter()
        .filter(|record| record.run.branch == target.branch)
        .collect::<Vec<_>>();
    records.sort_by(task_run::compare_task_run_records);
    Ok(records)
}

fn print_task_runs(ctx: &Ctx, records: &[task_run::TaskRunRecord]) -> Result<()> {
    match records {
        [] => ctx.ui.print_dim("  TaskRun: none"),
        [record] => print_task_run(ctx, record)?,
        _ => {
            ctx.ui.print_dim(&format!("  TaskRuns: {}", records.len()));
            for record in records {
                print_task_run(ctx, record)?;
            }
        }
    }
    Ok(())
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

fn print_cmux_work(ctx: &Ctx, work: &work::Work) {
    match work.session_state {
        work::WorkSessionState::NoLocalWorktree => ctx
            .ui
            .print_dim("  cmux: unavailable without a checked out worktree"),
        work::WorkSessionState::CmuxUnavailable => ctx.ui.print_dim("  cmux: unavailable"),
        work::WorkSessionState::NoCmuxWorkspace => {
            ctx.ui.print_dim("  cmux: no workspace found for worktree")
        }
        work::WorkSessionState::NoTerminalSurface => {
            if let Some(cmux) = work.cmux.as_ref() {
                print_cmux_workspace_ref(ctx, cmux);
            }
            ctx.ui.print_dim("  cmux: terminal surface is not ready");
        }
        work::WorkSessionState::AmbiguousTerminalSurface => {
            if let Some(cmux) = work.cmux.as_ref() {
                print_cmux_workspace_ref(ctx, cmux);
            }
            ctx.ui.print_dim("  cmux: terminal surface is ambiguous");
        }
        work::WorkSessionState::TerminalSurfaceReady => {
            if let Some(contact) = selected_work_contact(work) {
                print_cmux_workspace(ctx, contact);
            } else if let Some(contact) =
                work.cmux.as_ref().and_then(work::WorkCmuxSurface::contact)
            {
                print_cmux_workspace(ctx, &contact);
            }
        }
    }
    print_agent_state(ctx, &work.state);
    print_cmux_candidates(ctx, &work.cmux_contacts);
}

fn print_agent_state(ctx: &Ctx, state: &work::WorkState) {
    ctx.ui.print_dim(&format!(
        "  Agent: {} ({})",
        state.agent_kind.as_str(),
        state.status.as_str()
    ));
    if let Some(tool) = state.last_tool.as_deref() {
        ctx.ui.print_dim(&format!("  Agent last tool: {tool}"));
    }
    if let Some(warning) = state.warning.as_deref() {
        ctx.ui.print_warning(&format!("  Agent warning: {warning}"));
    }
}

fn print_cmux_workspace(ctx: &Ctx, contact: &CmuxContact) {
    ctx.ui.print_dim(&format!(
        "  cmux workspace: {} \"{}\" (window {})",
        contact.workspace, contact.title, contact.window
    ));
    ctx.ui.print_dim(&format!(
        "  cmux surface: {} (pane {})",
        contact.surface, contact.pane
    ));
    ctx.ui.print_dim(&format!(
        "  cmux send: cmux send --workspace {} --surface {} <message>",
        contact.workspace, contact.surface
    ));
    ctx.ui.print_dim(&format!(
        "  cmux enter: cmux send-key --workspace {} --surface {} enter",
        contact.workspace, contact.surface
    ));
}

fn selected_work_contact(work: &work::Work) -> Option<&CmuxContact> {
    let cmux = work.cmux.as_ref()?;
    work.cmux_contacts.iter().find(|contact| {
        contact.workspace == cmux.workspace_ref
            && Some(contact.surface.as_str()) == cmux.surface_ref.as_deref()
    })
}

fn print_cmux_candidates(ctx: &Ctx, contacts: &[CmuxContact]) {
    if contacts.is_empty() {
        return;
    }
    if contacts.len() == 1
        && contacts[0].is_live_agent_candidate()
        && contacts[0].validation_warning.is_none()
    {
        return;
    }

    ctx.ui
        .print_dim(&format!("  cmux candidates: {}", contacts.len()));
    for contact in contacts {
        let selected = if contact.selected { " selected" } else { "" };
        let readable = if contact.readable {
            "readable"
        } else {
            "unreadable"
        };
        let warning = contact
            .validation_warning
            .as_deref()
            .map(|warning| format!(", warning={warning}"))
            .unwrap_or_default();
        ctx.ui.print_dim(&format!(
            "    - {} {}{} (pane {}, window {}, {}, agent={} status={}{})",
            contact.workspace,
            contact.surface,
            selected,
            contact.pane,
            contact.window,
            readable,
            contact.state.agent_kind.as_str(),
            contact.state.status.as_str(),
            warning
        ));
        ctx.ui.print_dim(&format!(
            "      cmux send --workspace {} --surface {} <message>",
            contact.workspace, contact.surface
        ));
        ctx.ui.print_dim(&format!(
            "      cmux send-key --workspace {} --surface {} enter",
            contact.workspace, contact.surface
        ));
    }
}

fn print_cmux_workspace_ref(ctx: &Ctx, cmux: &work::WorkCmuxSurface) {
    ctx.ui.print_dim(&format!(
        "  cmux workspace: {} \"{}\" (window {})",
        cmux.workspace_ref, cmux.workspace_title, cmux.window_ref
    ));
}

pub(crate) fn cmux_contacts(ctx: &Ctx, worktree: &Path) -> Result<Vec<CmuxContact>> {
    work::cmux_contacts(ctx, worktree)
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
    fn review_prints_all_task_runs_for_branch() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-workspace");
        std::fs::create_dir_all(repo.join(".local/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".local/tasks/add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/tasks/publish-issues.toml"),
            "title = \"Publish issues\"\nbranch = \"publish-issues\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-add-schema.toml"),
            "task = \"add-schema\"\nbranch = \"team-run\"\nstatus = \"running\"\nsource = \"new\"\ncreation_order = 1\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-publish-issues.toml"),
            "task = \"publish-issues\"\nbranch = \"team-run\"\nstatus = \"running\"\nsource = \"new\"\ncreation_order = 2\ncreated_at = \"2026-05-16T00:00:01Z\"\nupdated_at = \"2026-05-16T00:00:01Z\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/team-run\n\n",
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

        run(&ctx, Some("team-run")).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("TaskRuns: 2"));
        assert!(dims.contains("TaskRun: run-add-schema"));
        assert!(dims.contains("TaskRun: run-publish-issues"));
        assert!(dims.contains("Task: .local/tasks/add-schema.toml (Add schema)"));
        assert!(dims.contains("Task: .local/tasks/publish-issues.toml (Publish issues)"));
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
        runner.add_response("", false);
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
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        runner.add_response("ready", true);
        runner.add_response("", true);
        runner.add_response("", true);
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

        run(&ctx, Some("feature")).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("cmux workspace: workspace:1 \"feature\" (window window:1)"));
        assert!(dims.contains("cmux surface: surface:4 (pane pane:3)"));
        assert!(dims.contains("cmux send --workspace workspace:1 --surface surface:4"));
        assert!(dims.contains("cmux send-key --workspace workspace:1 --surface surface:4 enter"));
    }

    #[test]
    fn review_prints_cmux_workspace_without_ready_terminal_surface() {
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
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        runner.add_response("Terminal surface not found", false);
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

        run(&ctx, Some("feature")).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("cmux workspace: workspace:1 \"feature\" (window window:1)"));
        assert!(dims.contains("cmux: terminal surface is not ready"));
        assert!(dims.contains("cmux candidates: 1"));
        assert!(dims.contains("unreadable cmux surface"));
        assert!(dims.contains("cmux send --workspace workspace:1 --surface surface:4"));
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
