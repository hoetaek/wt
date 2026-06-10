use crate::commands::agent_report;
use crate::context::Ctx;
use crate::services::git::{GitService, porcelain_status_path};
use crate::services::github_review::{
    GithubReviewService, PullRequestReviewEvidence, PullRequestReviewVerdict,
};
use crate::services::work;
use crate::task;
use crate::task_run;
use crate::workflow::render::{workflow_body_summary, workflow_origin_label, workflow_title_label};
use crate::workflow::{self, WorkflowCodexBaseReview, WorkflowPullRequestMode, WorkflowRecord};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) type InspectTarget = work::WorkTarget;
pub(crate) type CmuxContact = work::CmuxContact;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InspectOptions {
    pub pr: bool,
}

pub fn run(ctx: &Ctx, target: Option<&str>, options: InspectOptions) -> Result<()> {
    let selected_target = resolve_inspect_target(ctx, target)?;
    let work = work::observe_target(ctx, selected_target)?;
    let target = &work.target;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let parent = git.get_branch_parent(&target.branch)?;
    let status = match target.worktree.as_deref() {
        Some(path) => Some(status_porcelain_for_configured_links(ctx, &git, path)?),
        None => None,
    };
    let task_runs = task_runs_for_target(ctx, target)?;
    let workflows = workflows_for_task_runs(ctx, &task_runs.records)?;
    let pull_request_review = if options.pr {
        Some(
            GithubReviewService::new(ctx.runner.as_ref(), Some(&ctx.repo_root))
                .review_for_branch(&target.branch)?,
        )
    } else {
        None
    };

    if ctx.is_json() {
        write_json(&inspect_report(
            ctx,
            &work,
            status.as_deref(),
            parent.as_deref(),
            &task_runs,
            &workflows,
            pull_request_review,
        )?)?;
        return Ok(());
    }

    ctx.ui.print_step(&format!("Inspect: {}", target.label));
    print_work_section(ctx, target, &task_runs.records, &workflows)?;
    print_target_warnings(ctx, target);
    print_git_section(ctx, status.as_deref(), parent.as_deref(), &target.branch)?;
    print_agent_section(ctx, &work);
    print_cmux_section(ctx, &work);
    print_agent_report_expectation(ctx);
    if let Some(evidence) = pull_request_review.as_ref() {
        print_pull_request_review_section(ctx, evidence);
    }
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
#[derive(Clone, Debug, Default)]
struct TargetTaskRunInventory {
    records: Vec<task_run::TaskRunRecord>,
    invalid: Vec<task_run::InvalidTaskRunRecord>,
}

fn task_runs_for_target(ctx: &Ctx, target: &InspectTarget) -> Result<TargetTaskRunInventory> {
    if let Some(record) = target.task_run.clone() {
        return Ok(TargetTaskRunInventory {
            records: vec![record],
            invalid: Vec::new(),
        });
    }

    let inventory = task_run::list_lossy(ctx)?;
    let mut records = inventory
        .records
        .into_iter()
        .filter(|record| record.run.branch == target.branch)
        .collect::<Vec<_>>();
    records.sort_by(task_run::compare_task_run_records);
    Ok(TargetTaskRunInventory {
        records,
        invalid: inventory.invalid,
    })
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
    review_codex_base: WorkflowCodexBaseReview,
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
                review_codex_base: record.workflow.policy.review.codex_base,
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
                review_codex_base: record.workflow.policy.review.codex_base,
            });
        }
    }
}

fn warn_skipped_workflow(ctx: &Ctx, path: &Path, err: &anyhow::Error) {
    if ctx.is_json() {
        return;
    }
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

#[derive(Debug, Serialize)]
struct InspectReport {
    target: InspectTargetReport,
    git: InspectGitReport,
    task_runs: Vec<InspectTaskRunReport>,
    invalid_task_runs: Vec<InspectInvalidTaskRunReport>,
    workflows: Vec<InspectWorkflowReport>,
    agent: InspectAgentReport,
    cmux: InspectCmuxReport,
    expected_report: InspectExpectedReport,
    pull_request_review: Option<PullRequestReviewEvidence>,
}

#[derive(Debug, Serialize)]
struct InspectTargetReport {
    label: String,
    branch: String,
    worktree: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectGitReport {
    parent: Option<String>,
    dirty: String,
    dirty_paths: Vec<String>,
    ignored_configured_links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InspectTaskRunReport {
    id: String,
    task: String,
    branch: String,
    status: String,
    context: String,
    group: Option<String>,
    error: Option<String>,
    route: InspectTaskRunRouteReport,
    report: InspectTaskRunReportState,
    review: InspectTaskRunReviewState,
    task_path: String,
    task_title: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectInvalidTaskRunReport {
    id: String,
    path: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct InspectTaskRunRouteReport {
    agent_id: Option<String>,
    coordinator_id: Option<String>,
    coordinator_label: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectTaskRunReportState {
    last_message_id: Option<String>,
    last_reported_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectTaskRunReviewState {
    last_status: Option<String>,
    last_message_id: Option<String>,
    last_reviewed_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectWorkflowReport {
    id: String,
    path: String,
    mode: String,
    title: String,
    body_summary: Option<String>,
    origin: Option<String>,
    task: String,
    parent: Option<String>,
    pull_request: String,
    review_codex_base: String,
}

#[derive(Debug, Serialize)]
struct InspectAgentReport {
    kind: String,
    state: String,
    session_id: Option<String>,
    last_tool: Option<String>,
    last_event_at: Option<String>,
    needs_input_since: Option<String>,
    warning: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectCmuxReport {
    state: String,
    contact: Option<InspectCmuxContactReport>,
    candidates: Vec<InspectCmuxContactReport>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectCmuxContactReport {
    workspace: String,
    surface: String,
    pane: String,
    title: String,
    selected: bool,
    readable: bool,
    agent_kind: String,
    agent_state: String,
    validation_warning: Option<String>,
}

#[derive(Debug, Serialize)]
struct InspectExpectedReport {
    heading: &'static str,
    shape: &'static str,
    items: Vec<&'static str>,
}

fn inspect_report(
    ctx: &Ctx,
    work: &work::Work,
    status: Option<&str>,
    parent: Option<&str>,
    task_runs: &TargetTaskRunInventory,
    workflows: &[WorkflowMatch],
    pull_request_review: Option<PullRequestReviewEvidence>,
) -> Result<InspectReport> {
    Ok(InspectReport {
        target: InspectTargetReport {
            label: work.target.label.clone(),
            branch: work.target.branch.clone(),
            worktree: work
                .target
                .worktree
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        git: inspect_git_report(ctx, status, parent),
        task_runs: task_runs
            .records
            .iter()
            .map(|record| inspect_task_run_report(ctx, record))
            .collect::<Result<Vec<_>>>()?,
        invalid_task_runs: task_runs
            .invalid
            .iter()
            .map(|record| InspectInvalidTaskRunReport {
                id: record.id.clone(),
                path: ctx.storage_root.display_path(&record.path),
                error: record.error.clone(),
            })
            .collect(),
        workflows: workflows
            .iter()
            .map(|workflow| InspectWorkflowReport {
                id: workflow.id.clone(),
                path: workflow_relative_path(ctx, &workflow.path),
                mode: workflow.mode.clone(),
                title: workflow.title.clone(),
                body_summary: workflow.body_summary.clone(),
                origin: workflow.origin.clone(),
                task: workflow.task.clone(),
                parent: workflow.parent.clone(),
                pull_request: workflow.pull_request.as_str().into(),
                review_codex_base: workflow.review_codex_base.as_str().into(),
            })
            .collect(),
        agent: InspectAgentReport {
            kind: work.state.agent_kind.as_str().into(),
            state: work.state.status.as_str().into(),
            session_id: work.state.session_id.clone(),
            last_tool: work.state.last_tool.clone(),
            last_event_at: work.state.last_event_at.clone(),
            needs_input_since: work.state.needs_input_since.clone(),
            warning: work.state.warning.clone(),
        },
        cmux: inspect_cmux_report(work),
        expected_report: InspectExpectedReport {
            heading: agent_report::REPORT_HEADING,
            shape: "Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>",
            items: agent_report::REPORT_ITEMS.to_vec(),
        },
        pull_request_review,
    })
}

fn inspect_git_report(ctx: &Ctx, status: Option<&str>, parent: Option<&str>) -> InspectGitReport {
    let dirty_paths = status
        .map(|status| {
            relevant_status_lines(ctx, status)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let ignored_configured_links = status
        .map(|status| {
            ignored_configured_link_lines(ctx, status)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dirty = match status {
        None => "unavailable",
        Some(_) if dirty_paths.is_empty() => "clean",
        Some(_) => "dirty",
    };
    InspectGitReport {
        parent: parent.map(str::to_string),
        dirty: dirty.into(),
        dirty_paths,
        ignored_configured_links,
    }
}

fn inspect_task_run_report(
    ctx: &Ctx,
    record: &task_run::TaskRunRecord,
) -> Result<InspectTaskRunReport> {
    let context = task_run::resolve_context(ctx, record)
        .map(|context| context.label())
        .unwrap_or_else(|err| format!("unavailable ({})", inspect_error_summary(&err)));
    let task_path = task::task_relative_path(&record.run.task);
    let task_title = task::read_task_document(ctx, &record.run.task)
        .ok()
        .map(|document| document.title_or_key(&record.run.task).to_string());
    Ok(InspectTaskRunReport {
        id: record.id.clone(),
        task: record.run.task.clone(),
        branch: record.run.branch.clone(),
        status: record.run.status.as_str().into(),
        context,
        group: record.run.group.clone(),
        error: record.run.error.clone(),
        route: InspectTaskRunRouteReport {
            agent_id: record.run.agent_id.clone(),
            coordinator_id: record.run.coordinator_id.clone(),
            coordinator_label: record.run.coordinator_label.clone(),
        },
        report: InspectTaskRunReportState {
            last_message_id: record.run.last_report_message_id.clone(),
            last_reported_at: record.run.last_reported_at.clone(),
        },
        review: InspectTaskRunReviewState {
            last_status: record
                .run
                .last_review_status
                .map(|status| status.as_str().into()),
            last_message_id: record.run.last_review_message_id.clone(),
            last_reviewed_at: record.run.last_reviewed_at.clone(),
        },
        task_path,
        task_title,
    })
}

fn inspect_cmux_report(work: &work::Work) -> InspectCmuxReport {
    InspectCmuxReport {
        state: work.session_state.as_str().into(),
        contact: selected_work_contact(work)
            .or_else(|| work.cmux_contacts.iter().find(|contact| contact.selected))
            .map(inspect_cmux_contact_report),
        candidates: work
            .cmux_contacts
            .iter()
            .map(inspect_cmux_contact_report)
            .collect(),
        message: work.message.clone(),
    }
}

fn inspect_cmux_contact_report(contact: &CmuxContact) -> InspectCmuxContactReport {
    InspectCmuxContactReport {
        workspace: contact.workspace.clone(),
        surface: contact.surface.clone(),
        pane: contact.pane.clone(),
        title: contact.title.clone(),
        selected: contact.selected,
        readable: contact.readable,
        agent_kind: contact.state.agent_kind.as_str().into(),
        agent_state: contact.state.status.as_str().into(),
        validation_warning: contact.validation_warning.clone(),
    }
}

fn write_json(report: &InspectReport) -> Result<()> {
    let mut handle = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    println!();
    Ok(())
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

fn print_target_warnings(ctx: &Ctx, target: &InspectTarget) {
    for warning in &target.warnings {
        ctx.ui.print_warning(warning);
    }
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
    details.push(format!(
        "review_codex_base={}",
        workflow.review_codex_base.as_str()
    ));
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
    print_task_run_route(ctx, record);
    print_task_run_report_state(ctx, record);
    print_task_run_review_state(ctx, record);

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

fn print_task_run_route(ctx: &Ctx, record: &task_run::TaskRunRecord) {
    let task_agent = record.run.agent_id.as_deref().unwrap_or("missing");
    let coordinator = record.run.coordinator_id.as_deref().unwrap_or("missing");
    let label = record
        .run
        .coordinator_label
        .as_deref()
        .map(|label| format!(", coordinator_label={label}"))
        .unwrap_or_default();
    ctx.ui.print_dim(&format!(
        "  TaskRun route: task_agent={task_agent}, coordinator={coordinator}{label}"
    ));
}

fn print_task_run_report_state(ctx: &Ctx, record: &task_run::TaskRunRecord) {
    match (
        record.run.last_report_message_id.as_deref(),
        record.run.last_reported_at.as_deref(),
    ) {
        (Some(message_id), Some(reported_at)) => ctx.ui.print_dim(&format!(
            "  TaskRun report: message={message_id}, reported_at={reported_at}"
        )),
        (Some(message_id), None) => ctx.ui.print_dim(&format!(
            "  TaskRun report: message={message_id}, reported_at=missing"
        )),
        _ => ctx.ui.print_dim("  TaskRun report: not reported"),
    }
}

fn print_task_run_review_state(ctx: &Ctx, record: &task_run::TaskRunRecord) {
    match (
        record.run.last_review_status,
        record.run.last_review_message_id.as_deref(),
        record.run.last_reviewed_at.as_deref(),
    ) {
        (Some(status), Some(message_id), Some(reviewed_at)) => ctx.ui.print_dim(&format!(
            "  TaskRun review: status={status}, message={message_id}, reviewed_at={reviewed_at}"
        )),
        (Some(status), message_id, reviewed_at) => ctx.ui.print_dim(&format!(
            "  TaskRun review: status={status}, message={}, reviewed_at={}",
            message_id.unwrap_or("missing"),
            reviewed_at.unwrap_or("missing")
        )),
        _ => ctx.ui.print_dim("  TaskRun review: not reviewed"),
    }
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

fn print_pull_request_review_section(ctx: &Ctx, evidence: &PullRequestReviewEvidence) {
    ctx.ui.print_step("Pull Request Review");
    let Some(pr) = evidence.pr.as_ref() else {
        ctx.ui.print_warning("Pull request review: none detected");
        for warning in &evidence.warnings {
            ctx.ui.print_warning(&format!("  {warning}"));
        }
        return;
    };

    ctx.ui.print_dim(&format!(
        "  PR: #{} {} ({}, {} -> {})",
        pr.number, pr.title, pr.state, pr.head_ref_name, pr.base_ref_name
    ));
    if let Some(url) = pr.url.as_deref() {
        ctx.ui.print_dim(&format!("  URL: {url}"));
    }
    ctx.ui
        .print_dim(&format!("  Verdict: {}", evidence.verdict.as_str()));
    ctx.ui.print_dim(&format!("  Head: {}", pr.head_ref_oid));
    ctx.ui.print_dim(&format!(
        "  Checks: {} passed, {} pending, {} blocked, {} warnings",
        count_verdict(&evidence.checks, PullRequestReviewVerdict::Passed),
        count_verdict(&evidence.checks, PullRequestReviewVerdict::Pending),
        count_verdict(&evidence.checks, PullRequestReviewVerdict::Blocked),
        count_verdict(&evidence.checks, PullRequestReviewVerdict::Warning)
            + count_verdict(&evidence.checks, PullRequestReviewVerdict::Unavailable),
    ));
    let current_reviews = evidence
        .reviews
        .iter()
        .filter(|review| review.covers_head)
        .count();
    let stale_reviews = evidence.reviews.len().saturating_sub(current_reviews);
    ctx.ui.print_dim(&format!(
        "  Reviews: {current_reviews} current-head, {stale_reviews} stale/unsynchronized"
    ));
    let unresolved_threads = evidence
        .threads
        .iter()
        .filter(|thread| !thread.is_resolved)
        .count();
    let outdated_threads = evidence
        .threads
        .iter()
        .filter(|thread| thread.is_outdated)
        .count();
    ctx.ui.print_dim(&format!(
        "  Threads: {unresolved_threads} unresolved, {outdated_threads} outdated"
    ));
    if !evidence.review_requests.is_empty() {
        let reviewers = evidence
            .review_requests
            .iter()
            .map(|request| request.reviewer.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        ctx.ui.print_dim(&format!("  Review requests: {reviewers}"));
    }
    if !evidence.suggested_triggers.is_empty() {
        ctx.ui.print_dim(&format!(
            "  Suggested re-review: {}",
            evidence.suggested_triggers.join("; ")
        ));
    }
    for warning in &evidence.warnings {
        ctx.ui
            .print_warning(&format!("Pull request review: {warning}"));
    }
}

fn count_verdict(
    checks: &[crate::services::github_review::PullRequestReviewCheck],
    verdict: PullRequestReviewVerdict,
) -> usize {
    checks
        .iter()
        .filter(|check| check.verdict == verdict)
        .count()
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
        "  Pass: when accepted, review the worktree, report, and checks, then run `{}`; land when policy and safety checks allow.",
        workflow_pass_command(workflow)
    ));
}

fn workflow_pass_command(workflow: &WorkflowMatch) -> String {
    let mut command = format!(
        "wt workflow pass {}",
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
    ctx.storage_root.display_path(path)
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

fn status_porcelain_for_configured_links(
    ctx: &Ctx,
    git: &GitService<'_>,
    path: &Path,
) -> Result<String> {
    let status = git.status_porcelain(path)?;
    if ctx.config.worktree.link.is_empty() || !status_may_hide_configured_link(ctx, &status) {
        return Ok(status);
    }
    git.status_porcelain_untracked_files_all(path)
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
    let Some(path) = porcelain_status_path(line) else {
        return false;
    };
    let path = path.as_ref();
    ctx.config
        .worktree
        .link
        .iter()
        .map(|linked| linked.to().trim_end_matches('/'))
        .any(|linked| path == linked || path.starts_with(&format!("{linked}/")))
}

fn status_may_hide_configured_link(ctx: &Ctx, status: &str) -> bool {
    status_lines(status)
        .into_iter()
        .any(|line| status_line_may_hide_configured_link(ctx, line))
}

fn status_line_may_hide_configured_link(ctx: &Ctx, line: &str) -> bool {
    if !line.starts_with("?? ") {
        return false;
    }
    let Some(path) = porcelain_status_path(line) else {
        return false;
    };
    let path = path.as_ref().trim_end_matches('/');
    ctx.config
        .worktree
        .link
        .iter()
        .map(|linked| linked.to().trim_end_matches('/'))
        .any(|linked| linked != path && linked.starts_with(&format!("{path}/")))
}

// porcelain 경로 추출/unquote는 services::git::porcelain_status_path 공유 구현을 쓴다.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, PathSpec};
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::sync::Arc;

    #[test]
    fn inspect_prints_branch_task_run_status_and_diff() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".wt/execution/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".wt/execution/task-runs")).unwrap();
        std::fs::create_dir_all(repo.join(".wt/execution/workflows")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".wt/execution/tasks/feature.toml"),
            "title = \"Feature\"\nbranch = \"feature\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ngroup = \"2026-05-17-001\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/workflows/2026-05-17-001.toml"),
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
            repo.join(".wt/execution/workflows/2026-05-17-099.toml"),
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

        run(&ctx, Some("feature"), InspectOptions::default()).unwrap();

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
        assert!(dims.contains("TaskRun route: task_agent=missing, coordinator=missing"));
        assert!(dims.contains("TaskRun report: not reported"));
        assert!(dims.contains("TaskRun review: not reviewed"));
        assert!(dims.contains("Task: <repo-root>/.wt/execution/tasks/feature.toml (Feature)"));
        assert!(dims.contains("Workflow: Ship feature workflow"));
        assert!(dims.contains("id=2026-05-17-001"));
        assert!(dims.contains("body=Coordinate inspect rendering"));
        assert!(!dims.contains("Hidden tail should not render"));
        assert!(dims.contains("origin=linear:WT-123"));
        assert!(dims.contains("Parent: main"));
        assert!(dims.contains("Commits ahead of parent: 2"));
        assert!(dims.contains("dirty (1 paths)"));
        assert!(dims.contains("PR=<pr>"));
        assert!(dims.contains("wt workflow pass"));
        assert!(dims.contains("--run-next"));
        let warnings = ui.warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Cmux: cmux command not found"));
        assert!(
            warnings.contains(
                "Skipping workflow <repo-root>/.wt/execution/workflows/2026-05-17-099.toml"
            )
        );
        assert!(warnings.contains("uses removed `objective`"));
    }

    #[test]
    fn inspect_next_includes_workflow_pass_for_single_workflow() {
        let dims = inspect_next_section_for_mode("single");

        assert!(dims.contains("Pass: when accepted"));
        assert!(dims.contains("wt workflow pass"));
        assert!(!dims.contains("2026-05-17-001.toml feature"));
        assert!(dims.contains("review the worktree, report, and checks"));
        assert!(dims.contains("land when policy and safety checks allow"));
        assert!(dims.contains("wt done feature"));
        assert!(!dims.contains("--run-next"));
    }

    #[test]
    fn inspect_next_includes_workflow_pass_for_batch_workflow() {
        let dims = inspect_next_section_for_mode("batch");

        assert!(dims.contains("Pass: when accepted"));
        assert!(dims.contains("wt workflow pass"));
        assert!(dims.contains("2026-05-17-001.toml feature"));
        assert!(dims.contains("review the worktree, report, and checks"));
        assert!(dims.contains("land when policy and safety checks allow"));
        assert!(dims.contains("wt done feature"));
        assert!(!dims.contains("--run-next"));
    }

    #[test]
    fn inspect_next_keeps_stack_workflow_pass_guidance() {
        let dims = inspect_next_section_for_mode("stack");

        assert!(dims.contains("Pass: when accepted"));
        assert!(dims.contains("wt workflow pass"));
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
            warnings: Vec::new(),
        };
        let workflow = WorkflowMatch {
            id: "2026-05-17-001".into(),
            path: repo.join(".wt/execution/workflows/2026-05-17-001.toml"),
            mode: mode.into(),
            title: "Feature workflow".into(),
            body_summary: None,
            origin: None,
            task: "feature".into(),
            parent: None,
            pull_request: WorkflowPullRequestMode::None,
            review_codex_base: WorkflowCodexBaseReview::None,
        };

        print_next_section(&ctx, &target, &[workflow]);

        ui.dims.lock().unwrap().join("\n")
    }

    #[test]
    fn inspect_prints_all_task_runs_for_branch() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-workspace");
        std::fs::create_dir_all(repo.join(".wt/execution/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".wt/execution/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".wt/execution/tasks/add-schema.toml"),
            "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/tasks/publish-issues.toml"),
            "title = \"Publish issues\"\nbranch = \"publish-issues\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/task-runs/run-add-schema.toml"),
            "task = \"add-schema\"\nbranch = \"team-run\"\nstatus = \"running\"\ncreation_order = 1\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/task-runs/run-publish-issues.toml"),
            "task = \"publish-issues\"\nbranch = \"team-run\"\nstatus = \"running\"\ncreation_order = 2\ncreated_at = \"2026-05-16T00:00:01Z\"\nupdated_at = \"2026-05-16T00:00:01Z\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/task-runs/run-broken.toml"),
            "task = \"broken\"\nbranch = \"unrelated\"\nstatus = \"started\"\ncreated_at = \"2026-05-16T00:00:02Z\"\nupdated_at = \"2026-05-16T00:00:02Z\"\n",
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

        run(&ctx, Some("team-run"), InspectOptions::default()).unwrap();

        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(dims.contains("TaskRuns: 2"));
        assert!(dims.contains("TaskRun: run-add-schema"));
        assert!(dims.contains("TaskRun: run-publish-issues"));
        assert!(
            dims.contains("Task: <repo-root>/.wt/execution/tasks/add-schema.toml (Add schema)")
        );
        assert!(dims.contains(
            "Task: <repo-root>/.wt/execution/tasks/publish-issues.toml (Publish issues)"
        ));
        let warnings = ui.warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("TaskRun inventory skipped invalid record"));
        assert!(warnings.contains("run-broken.toml"));
    }

    #[test]
    fn inspect_without_target_selects_inspectable_work_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".wt/execution/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".wt/execution/task-runs/run-feature.toml"),
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

        run(&ctx, None, InspectOptions::default()).unwrap();

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

        let err = run(&ctx, None, InspectOptions::default())
            .unwrap_err()
            .to_string();

        assert!(err.contains("wt inspect requires TARGET"));
        assert!(err.contains("branch, worktree path/name, or TaskRun id"));
    }

    #[test]
    fn inspect_accepts_task_run_id_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".wt/execution/tasks")).unwrap();
        std::fs::create_dir_all(repo.join(".wt/execution/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".wt/execution/tasks/feature.toml"),
            "title = \"Feature\"\nbranch = \"feature\"\n",
        )
        .unwrap();
        std::fs::write(
            repo.join(".wt/execution/task-runs/run-feature.toml"),
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

        run(&ctx, Some("run-feature"), InspectOptions::default()).unwrap();

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

        run(&ctx, Some("feature"), InspectOptions::default()).unwrap();

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
        std::fs::create_dir_all(repo.join(".wt/execution/task-runs")).unwrap();
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

        run(&ctx, Some("feature"), InspectOptions::default()).unwrap();

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
        std::fs::create_dir_all(repo.join(".wt/execution/task-runs")).unwrap();
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

        run(&ctx, Some("feature"), InspectOptions::default()).unwrap();

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
        config.worktree.link = vec!["tmp/shared-cache".into()];
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(
            relevant_status_lines(&ctx, "?? tmp/shared-cache\n?? tmp/shared-cache/a.toml"),
            Vec::<&str>::new()
        );
        assert_eq!(
            relevant_status_lines(&ctx, "?? tmp/shared-cache\n M src/lib.rs"),
            vec!["M src/lib.rs"]
        );
    }

    #[test]
    fn inspect_status_expands_untracked_paths_only_when_link_is_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.worktree.link = vec![PathSpec::Rename {
            from: ".local/skills".into(),
            to: ".codex/skills".into(),
        }];
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let mut runner = MockRunner::new();
        runner.add_response("?? .codex/", true);
        runner.add_response("?? .codex/skills\n?? src/lib.rs", true);
        let git = GitService::new(&runner, Some(&ctx.repo_root));

        let status = status_porcelain_for_configured_links(&ctx, &git, dir.path()).unwrap();

        assert_eq!(status, "?? .codex/skills\n?? src/lib.rs");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["status", "--porcelain"]);
        assert_eq!(
            calls[1].1,
            vec!["status", "--porcelain", "--untracked-files=all"]
        );
    }

    #[test]
    fn porcelain_status_path_returns_plain_paths() {
        assert_eq!(
            porcelain_status_path("?? src/app.rs").as_deref(),
            Some("src/app.rs")
        );
        assert_eq!(
            porcelain_status_path("R  old.rs -> new.rs").as_deref(),
            Some("new.rs")
        );
    }

    #[test]
    fn porcelain_status_path_splits_rename_only_for_rename_statuses() {
        // 비rename 상태에서는 " -> "가 경로의 일부다 (git은 공백/화살표만으로는 인용하지 않음)
        assert_eq!(
            porcelain_status_path("?? a -> b.md").as_deref(),
            Some("a -> b.md")
        );
        // rename은 따옴표 밖 첫 구분자에서 자르고 대상 경로를 취한다
        assert_eq!(
            porcelain_status_path(r#"R  "old -> x.md" -> "new -> y.md""#).as_deref(),
            Some("new -> y.md")
        );
        assert_eq!(
            porcelain_status_path("R  old.rs -> new.rs").as_deref(),
            Some("new.rs")
        );
    }

    #[test]
    fn porcelain_status_path_unquotes_git_escaped_paths() {
        assert_eq!(
            porcelain_status_path(r#"?? "src/tab\there.txt""#).as_deref(),
            Some("src/tab\there.txt")
        );
        assert_eq!(
            porcelain_status_path(r#"?? "back\\slash""#).as_deref(),
            Some("back\\slash")
        );
        assert_eq!(
            porcelain_status_path(r#"?? "\355\225\234\352\270\200.md""#).as_deref(),
            Some("한글.md")
        );
    }

    #[test]
    fn configured_link_detection_matches_git_quoted_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.worktree.link = vec![PathSpec::Same("한글".into())];
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        // git이 core.quotePath로 "한글/note.md"를 octal escape한 형태
        assert!(
            is_configured_link_status_line(&ctx, r#"?? "\355\225\234\352\270\200/note.md""#),
            "escape된 configured link 경로도 link 라인으로 인식해야 한다"
        );
        assert!(!is_configured_link_status_line(
            &ctx,
            r#"?? "\355\225\234\352\270\200-else/note.md""#
        ));
    }
}
