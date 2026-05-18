use crate::commands::agent_report;
use crate::context::Ctx;
use crate::services::git::GitService;
use crate::services::work;
use crate::task;
use crate::task_run;
use crate::workflow::render::{workflow_body_summary, workflow_origin_label, workflow_title_label};
use crate::workflow::{self, WorkflowPullRequestMode, WorkflowRecord};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) type InspectTarget = work::WorkTarget;
pub(crate) type CmuxContact = work::CmuxContact;

pub fn run(ctx: &Ctx, target: Option<&str>) -> Result<()> {
    let selected_target = resolve_inspect_target(ctx, target)?;
    let work = work::observe_target(ctx, selected_target)?;
    let target = &work.target;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let parent = git.get_branch_parent(&target.branch)?;
    let status = match target.worktree.as_deref() {
        Some(path) => Some(git.status_porcelain(path)?),
        None => None,
    };
    let task_runs = task_runs_for_target(ctx, target)?;
    let workflows = workflows_for_task_runs(ctx, &task_runs)?;

    ctx.ui.print_step(&format!("Inspect: {}", target.label));
    print_work_section(ctx, target, &task_runs, &workflows)?;
    print_git_section(ctx, status.as_deref(), parent.as_deref(), &target.branch)?;
    print_agent_section(ctx, &work);
    print_cmux_section(ctx, &work);
    print_agent_report_expectation(ctx);
    print_next_section(ctx, target, &workflows);

    Ok(())
}

fn resolve_inspect_target(ctx: &Ctx, target: Option<&str>) -> Result<InspectTarget> {
    match target {
        Some(target) => work::resolve_target(ctx, Some(target)),
        None => select_inspect_target(ctx),
    }
}

fn select_inspect_target(ctx: &Ctx) -> Result<InspectTarget> {
    work::select_target(
        ctx,
        "Work target to inspect",
        "wt inspect requires TARGET when it cannot open an interactive selector. Pass a branch, worktree path/name, or TaskRun id; or run `wt inspect` in an interactive terminal to choose a work target.",
    )
}
fn task_runs_for_target(ctx: &Ctx, target: &InspectTarget) -> Result<Vec<task_run::TaskRunRecord>> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkflowMatch {
    id: String,
    path: PathBuf,
    mode: String,
    title: String,
    body_summary: Option<String>,
    origin: Option<String>,
    task: String,
    parent: Option<String>,
    pull_request: WorkflowPullRequestMode,
}

fn workflows_for_task_runs(
    ctx: &Ctx,
    records: &[task_run::TaskRunRecord],
) -> Result<Vec<WorkflowMatch>> {
    let run_ids = records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<HashSet<_>>();
    if run_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for path in workflow::workflow_paths(ctx)? {
        let id = workflow::id_from_path(&path)?;
        match workflow::read(&path) {
            Ok(metadata) => {
                let record = WorkflowRecord {
                    id,
                    path,
                    workflow: metadata,
                };
                add_workflow_matches(ctx, &mut matches, &record, &run_ids);
            }
            Err(err) => warn_skipped_workflow(ctx, &path, &err),
        }
    }
    matches.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.task.cmp(&right.task))
    });
    matches.dedup_by(|left, right| left.id == right.id && left.path == right.path);
    Ok(matches)
}

fn add_workflow_matches(
    ctx: &Ctx,
    matches: &mut Vec<WorkflowMatch>,
    record: &WorkflowRecord,
    run_ids: &HashSet<&str>,
) {
    let title = workflow_title_label(ctx, &record.id, &record.workflow);
    let body_summary = workflow_body_summary(&record.workflow);
    let origin = workflow_origin_label(&record.workflow);
    for row in &record.workflow.tasks {
        if run_ids.contains(row.run.as_str()) {
            matches.push(WorkflowMatch {
                id: record.id.clone(),
                path: record.path.clone(),
                mode: record.workflow.mode.as_str().into(),
                title: title.clone(),
                body_summary: body_summary.clone(),
                origin: origin.clone(),
                task: workflow_task_label(&row.task),
                parent: row.parent.clone(),
                pull_request: record.workflow.policy.pull_request,
            });
        }
        for profile_run in &row.runs {
            if !run_ids.contains(profile_run.run.as_str()) {
                continue;
            }
            matches.push(WorkflowMatch {
                id: record.id.clone(),
                path: record.path.clone(),
                mode: record.workflow.mode.as_str().into(),
                title: title.clone(),
                body_summary: body_summary.clone(),
                origin: origin.clone(),
                task: format!("{}:{}", workflow_task_label(&row.task), profile_run.profile),
                parent: row.parent.clone(),
                pull_request: record.workflow.policy.pull_request,
            });
        }
    }
}

fn warn_skipped_workflow(ctx: &Ctx, path: &Path, err: &anyhow::Error) {
    ctx.ui.print_warning(&format!(
        "Skipping workflow {}: {}",
        workflow_relative_path(ctx, path),
        inspect_error_summary(err)
    ));
}

fn inspect_error_summary(err: &anyhow::Error) -> String {
    let message = format!("{err:#}");
    if message.contains("Workflow uses removed `objective`") {
        return "uses removed `objective`; edit the workflow file to use top-level `title`, `body`, and optional `[origin]`".into();
    }
    message
        .lines()
        .next()
        .unwrap_or("workflow could not be read")
        .to_string()
}

fn workflow_task_label(task: &str) -> String {
    let task = task.trim();
    if task.is_empty() {
        "workflow-task".into()
    } else {
        task.into()
    }
}

fn print_work_section(
    ctx: &Ctx,
    target: &InspectTarget,
    records: &[task_run::TaskRunRecord],
    workflows: &[WorkflowMatch],
) -> Result<()> {
    ctx.ui.print_step("Work");
    ctx.ui.print_dim(&format!("  Target: {}", target.label));
    ctx.ui.print_dim(&format!("  Branch: {}", target.branch));
    match target.worktree.as_deref() {
        Some(path) => ctx.ui.print_dim(&format!("  Worktree: {}", path.display())),
        None => ctx
            .ui
            .print_dim("  Worktree: branch is not checked out in a local worktree"),
    }
    print_task_runs(ctx, records)?;
    print_workflows(ctx, workflows);
    Ok(())
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

fn print_workflows(ctx: &Ctx, workflows: &[WorkflowMatch]) {
    match workflows {
        [] => ctx.ui.print_dim("  Workflow: not discovered"),
        [workflow] => print_workflow(ctx, workflow),
        _ => {
            ctx.ui
                .print_dim(&format!("  Workflows: {}", workflows.len()));
            for workflow in workflows {
                print_workflow(ctx, workflow);
            }
        }
    }
}

fn print_workflow(ctx: &Ctx, workflow: &WorkflowMatch) {
    let mut details = vec![
        format!("id={}", workflow.id),
        format!("mode={}", workflow.mode),
        format!("task={}", workflow.task),
        workflow_relative_path(ctx, &workflow.path),
    ];
    if let Some(summary) = workflow.body_summary.as_deref() {
        details.push(format!("body={summary}"));
    }
    if let Some(origin) = workflow.origin.as_deref() {
        details.push(format!("origin={origin}"));
    }
    if let Some(parent) = workflow.parent.as_deref() {
        details.push(format!("parent={parent}"));
    }
    details.push(format!("pull_request={}", workflow.pull_request.as_str()));
    ctx.ui.print_dim(&format!(
        "  Workflow: {} ({})",
        workflow.title,
        details.join(", ")
    ));
}

fn print_git_section(
    ctx: &Ctx,
    status: Option<&str>,
    parent: Option<&str>,
    branch: &str,
) -> Result<()> {
    ctx.ui.print_step("Git");
    print_parent_summary(ctx, parent, branch)?;
    print_worktree_status(ctx, status);
    Ok(())
}

fn print_task_run(ctx: &Ctx, record: &task_run::TaskRunRecord) -> Result<()> {
    let context = task_run::resolve_context(ctx, record)
        .map(|context| context.label())
        .unwrap_or_else(|err| format!("unavailable ({})", inspect_error_summary(&err)));
    ctx.ui.print_dim(&format!(
        "  TaskRun: {} (status={}, context={})",
        record.id, record.run.status, context
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
        ctx.ui.print_dim("  Dirty: unavailable");
        return;
    };

    let lines = relevant_status_lines(ctx, status);
    if lines.is_empty() {
        ctx.ui.print_dim("  Dirty: clean");
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
            .print_dim(&format!("  Dirty: dirty ({} paths)", lines.len()));
        for line in lines.iter().take(20) {
            ctx.ui.print_dim(&format!("    {line}"));
        }
        if lines.len() > 20 {
            ctx.ui
                .print_dim(&format!("    ... {} more", lines.len() - 20));
        }
    }
}

fn print_agent_section(ctx: &Ctx, work: &work::Work) {
    let state = &work.state;
    ctx.ui.print_step("Agent");
    ctx.ui
        .print_dim(&format!("  Kind: {}", state.agent_kind.as_str()));
    ctx.ui
        .print_dim(&format!("  State: {}", state.status.as_str()));
    if let Some(session_id) = state.session_id.as_deref() {
        ctx.ui.print_dim(&format!("  Session: {session_id}"));
    }
    if let Some(tool) = state.last_tool.as_deref() {
        ctx.ui.print_dim(&format!("  Last tool: {tool}"));
    }
    if let Some(last_event_at) = state.last_event_at.as_deref() {
        ctx.ui.print_dim(&format!("  Last event: {last_event_at}"));
    }
    if let Some(needs_input_since) = state.needs_input_since.as_deref() {
        ctx.ui
            .print_dim(&format!("  Needs input since: {needs_input_since}"));
    }
    if let Some(warning) = state.warning.as_deref() {
        ctx.ui.print_warning(&format!("Agent: {warning}"));
    }
}

fn print_cmux_section(ctx: &Ctx, work: &work::Work) {
    ctx.ui.print_step("Cmux");
    ctx.ui
        .print_dim(&format!("  State: {}", work.session_state.as_str()));
    match work.session_state {
        work::WorkSessionState::NoLocalWorktree => ctx
            .ui
            .print_dim("  Contact: unavailable without a checked out worktree"),
        work::WorkSessionState::CmuxUnavailable => ctx.ui.print_dim("  Contact: unavailable"),
        work::WorkSessionState::NoCmuxWorkspace => ctx
            .ui
            .print_dim("  Contact: no workspace found for worktree"),
        work::WorkSessionState::NoTerminalSurface => {
            if let Some(cmux) = work.cmux.as_ref() {
                print_cmux_workspace_ref(ctx, cmux);
            }
            ctx.ui.print_dim("  Contact: terminal surface is not ready");
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
    print_cmux_candidates(ctx, &work.cmux_contacts);
    if let Some(message) = work.message.as_deref() {
        ctx.ui.print_warning(&format!("Cmux: {message}"));
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

fn print_parent_summary(ctx: &Ctx, parent: Option<&str>, branch: &str) -> Result<()> {
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
    ctx.ui.print_step("Expected report");
    ctx.ui.print_dim(&format!(
        "  {}: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>",
        agent_report::REPORT_HEADING
    ));
    for item in agent_report::REPORT_ITEMS {
        ctx.ui.print_dim(&format!("  - {item}"));
    }
}

fn print_next_section(ctx: &Ctx, target: &InspectTarget, workflows: &[WorkflowMatch]) {
    ctx.ui.print_step("Next");
    ctx.ui.print_dim(
        "  Review: compare the report, diff, and checks before changing lifecycle state.",
    );
    for workflow in workflows {
        print_workflow_next_step(ctx, workflow);
    }
    ctx.ui.print_dim(&format!(
        "  Land/cleanup: merge explicitly first; run `wt done {}` only when cleanup is safe.",
        shell_arg(&target.branch)
    ));
}

fn print_workflow_next_step(ctx: &Ctx, workflow: &WorkflowMatch) {
    ctx.ui.print_dim(&format!(
        "  Complete: when accepted, review the worktree, report, and checks, then run `{}`; land when policy and safety checks allow.",
        workflow_complete_command(workflow)
    ));
}

fn workflow_complete_command(workflow: &WorkflowMatch) -> String {
    let mut command = format!(
        "wt workflow complete {}",
        shell_arg(&workflow.path.to_string_lossy())
    );
    if workflow.mode != "single" {
        command.push(' ');
        command.push_str(&shell_arg(&workflow.task));
    }
    if workflow.mode == "stack" {
        command.push_str(" --run-next");
    }
    command
}

fn workflow_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
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
    fn inspect_prints_branch_task_run_status_and_diff() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".local/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(repo.join(".local/workflows")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".local/tasks/feature.toml"),
            "title = \"Feature\"\nbranch = \"feature\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ngroup = \"2026-05-17-001\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/workflows/2026-05-17-001.toml"),
            r#"title = "Ship feature workflow"
body = """Coordinate inspect rendering without letting this deliberately verbose workflow body dominate the inspect dossier output or hide useful metadata. Hidden tail should not render."""
mode = "stack"
base_mode = "explicit"
base = "main"
created_at = "2026-05-17T00:00:00Z"
updated_at = "2026-05-17T00:00:00Z"

[origin]
provider = "linear"
id = "WT-123"

[policy]
pull_request = "draft"
landing = "manual"

[[tasks]]
task = "feature"
run = "run-feature"
parent = "main"
"#,
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/workflows/2026-05-17-099.toml"),
            r#"objective = "Old workflow"
mode = "batch"
base_mode = "explicit"
base = "main"
created_at = "2026-05-17T00:00:00Z"
updated_at = "2026-05-17T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "unrelated"
run = "run-unrelated"
"#,
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
        assert!(steps.contains("Inspect: feature"));
        assert!(steps.contains("Work"));
        assert!(steps.contains("Git"));
        assert!(steps.contains("Agent"));
        assert!(steps.contains("Cmux"));
        assert!(steps.contains("Expected report"));
        assert!(steps.contains("Next"));
        assert!(dims.contains("Agent Completion Report"));
        assert!(dims.contains("PR=<pr>"));
        assert!(dims.contains("TaskRun: run-feature"));
        assert!(dims.contains("Task: .local/tasks/feature.toml (Feature)"));
        assert!(dims.contains("Workflow: Ship feature workflow"));
        assert!(dims.contains("id=2026-05-17-001"));
        assert!(dims.contains("body=Coordinate inspect rendering"));
        assert!(!dims.contains("Hidden tail should not render"));
        assert!(dims.contains("origin=linear:WT-123"));
        assert!(dims.contains("Parent: main"));
        assert!(dims.contains("Commits ahead of parent: 2"));
        assert!(dims.contains("dirty (1 paths)"));
        assert!(dims.contains("PR=<pr>"));
        assert!(dims.contains("wt workflow complete"));
        assert!(dims.contains("--run-next"));
        let warnings = ui.warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Cmux: cmux command not found"));
        assert!(warnings.contains("Skipping workflow .local/workflows/2026-05-17-099.toml"));
        assert!(warnings.contains("uses removed `objective`"));
    }

    #[test]
    fn inspect_next_includes_workflow_complete_for_single_workflow() {
        let dims = inspect_next_section_for_mode("single");

        assert!(dims.contains("Complete: when accepted"));
        assert!(dims.contains("wt workflow complete"));
        assert!(!dims.contains("2026-05-17-001.toml feature"));
        assert!(dims.contains("review the worktree, report, and checks"));
        assert!(dims.contains("land when policy and safety checks allow"));
        assert!(dims.contains("wt done feature"));
        assert!(!dims.contains("--run-next"));
    }

    #[test]
    fn inspect_next_includes_workflow_complete_for_batch_workflow() {
        let dims = inspect_next_section_for_mode("batch");

        assert!(dims.contains("Complete: when accepted"));
        assert!(dims.contains("wt workflow complete"));
        assert!(dims.contains("2026-05-17-001.toml feature"));
        assert!(dims.contains("review the worktree, report, and checks"));
        assert!(dims.contains("land when policy and safety checks allow"));
        assert!(dims.contains("wt done feature"));
        assert!(!dims.contains("--run-next"));
    }

    #[test]
    fn inspect_next_keeps_stack_workflow_completion_guidance() {
        let dims = inspect_next_section_for_mode("stack");

        assert!(dims.contains("Complete: when accepted"));
        assert!(dims.contains("wt workflow complete"));
        assert!(dims.contains("2026-05-17-001.toml feature"));
        assert!(dims.contains("--run-next"));
        assert!(dims.contains("wt done feature"));
    }

    fn inspect_next_section_for_mode(mode: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        let target = InspectTarget {
            label: "feature".into(),
            branch: "feature".into(),
            worktree: None,
            task_run: None,
        };
        let workflow = WorkflowMatch {
            id: "2026-05-17-001".into(),
            path: repo.join(".local/workflows/2026-05-17-001.toml"),
            mode: mode.into(),
            title: "Feature workflow".into(),
            body_summary: None,
            origin: None,
            task: "feature".into(),
            parent: None,
            pull_request: WorkflowPullRequestMode::None,
        };

        print_next_section(&ctx, &target, &[workflow]);

        ui.dims.lock().unwrap().join("\n")
    }

    #[test]
    fn inspect_prints_all_task_runs_for_branch() {
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
            "task = \"add-schema\"\nbranch = \"team-run\"\nstatus = \"running\"\ncreation_order = 1\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-publish-issues.toml"),
            "task = \"publish-issues\"\nbranch = \"team-run\"\nstatus = \"running\"\ncreation_order = 2\ncreated_at = \"2026-05-16T00:00:01Z\"\nupdated_at = \"2026-05-16T00:00:01Z\"\n",
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
    fn inspect_without_target_selects_inspectable_work_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
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
        runner.add_response("feature\nmaster\n", true);
        runner.add_response("main", true);
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response("def add inspect", true);
        runner.add_response(" src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)", true);
        let mut ui = MockUi::new();
        ui.add_select(0);
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, None).unwrap();

        let prompts = ui.prompts.lock().unwrap().join("\n");
        let items = ui.select_items.lock().unwrap();
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(prompts.contains("select: Work target to inspect"));
        assert!(items[0].iter().any(|item| item.contains("feature")));
        assert!(items[0].iter().any(|item| item.contains("run-feature")));
        assert!(steps.contains("Inspect: feature"));
    }

    #[test]
    fn inspect_without_target_requires_tty_selector() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.set_prompt_available(false);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        let err = run(&ctx, None).unwrap_err().to_string();

        assert!(err.contains("wt inspect requires TARGET"));
        assert!(err.contains("branch, worktree path/name, or TaskRun id"));
    }

    #[test]
    fn inspect_accepts_task_run_id_target() {
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
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ngroup = \"stack-1\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
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
        assert!(steps.contains("Inspect: run-feature"));
        assert!(dims.contains("TaskRun: run-feature"));
        assert!(dims.contains("status=running"));
        assert!(dims.contains("context=workflow group stack-1 (not discovered)"));
    }

    #[test]
    fn inspect_keeps_dossier_useful_without_local_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        std::fs::create_dir_all(&repo).unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\n",
                repo.display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo.clone(),
            repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, Some("feature")).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        let warnings = ui.warnings.lock().unwrap().join("\n");
        assert!(dims.contains("Worktree: branch is not checked out"));
        assert!(dims.contains("Dirty: unavailable"));
        assert!(dims.contains("State: no_local_worktree"));
        assert!(warnings.contains("not checked out"));
    }

    #[test]
    fn inspect_prints_matching_cmux_workspace_and_surface() {
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
        runner.add_response(
            r#"{"windows":[{"workspaces":[{"panes":[{"surfaces":[]}]}]}]}"#,
            true,
        );
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
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
    fn inspect_prints_cmux_workspace_without_ready_terminal_surface() {
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
        runner.add_response(
            r#"{"windows":[{"workspaces":[{"panes":[{"surfaces":[]}]}]}]}"#,
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
        assert!(dims.contains("Contact: terminal surface is not ready"));
        assert!(dims.contains("cmux candidates: 1"));
        assert!(dims.contains("unreadable cmux surface"));
        assert!(dims.contains("cmux send --workspace workspace:1 --surface surface:4"));
    }

    #[test]
    fn inspect_clean_check_ignores_configured_worktree_links() {
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
