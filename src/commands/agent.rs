use crate::agent_state::{
    NewWaitObservation, WaitObservation, WaitObservationStore, WaitObservationSummary,
};
use crate::agents::AgentStatus;
use crate::context::Ctx;
use crate::error::WtError;
use crate::messages::AgentId;
use crate::services::identity_locator::{self, AnchorKey, AnchorKind};
use crate::services::work::{self, WorkSessionState};
use crate::task_run;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub mod supervisor;

pub fn status(ctx: &Ctx, target: Option<&str>) -> Result<()> {
    let (report, exit_code) = observe_status(ctx, target, "status")?;
    if ctx.is_json() {
        print_json(&report)?;
    } else {
        print_text(ctx, &report);
    }

    exit_result(exit_code)
}

pub fn watch(
    ctx: &Ctx,
    target: Option<&str>,
    interval_secs: u64,
    timeout_secs: Option<u64>,
    heartbeat_secs: Option<u64>,
) -> Result<()> {
    watch_with_options(
        ctx,
        target,
        WatchOptions {
            interval: duration_from_secs(interval_secs),
            timeout: timeout_secs.map(duration_from_secs),
            heartbeat: heartbeat_secs.map(duration_from_secs),
        },
    )
}

pub fn wait_stats(ctx: &Ctx) -> Result<()> {
    let paths = wait_observation_paths(ctx)?;
    let summary = WaitObservationStore::summary_all(&paths, wait_observation_summary_path(ctx))?;
    if ctx.is_json() {
        print_json(&summary)?;
    } else {
        print_wait_stats(ctx, &summary);
    }
    Ok(())
}

#[cfg(test)]
fn watch_with_interval(ctx: &Ctx, target: Option<&str>, interval: Duration) -> Result<()> {
    watch_with_options(
        ctx,
        target,
        WatchOptions {
            interval,
            timeout: None,
            heartbeat: None,
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WatchOptions {
    interval: Duration,
    timeout: Option<Duration>,
    heartbeat: Option<Duration>,
}

fn watch_with_options(ctx: &Ctx, target: Option<&str>, options: WatchOptions) -> Result<()> {
    let target = resolve_agent_target(ctx, target, "watch")?;
    let started_at = Instant::now();
    let mut last_output_at = started_at;
    let mut last_signature = None;

    loop {
        let work = work::observe_target(ctx, target.clone())?;
        let report = AgentStatusReport::from_work(ctx, &work);
        let exit_code = exit_code_for(&work);
        let signature = report.transition_signature();
        let now = Instant::now();
        let elapsed = now.duration_since(started_at);

        let changed = last_signature.as_ref() != Some(&signature);
        if changed {
            if ctx.is_json() {
                print_json_line(&report)?;
            } else {
                print_watch_transition(ctx, &report);
            }
            last_signature = Some(signature);
            last_output_at = now;
        }

        if should_stop_watching(&work, exit_code) {
            return exit_result(exit_code);
        }

        if let Some(timeout) = options.timeout.filter(|timeout| elapsed >= *timeout) {
            if let Some(agent_id) = wait_observation_agent_id(ctx, &work)? {
                record_wait_observation(
                    ctx,
                    &agent_id,
                    &report,
                    "timeout",
                    elapsed,
                    timeout,
                    now.duration_since(last_output_at),
                )?;
            }
            if ctx.is_json() {
                print_json_line(&report_with_warning(
                    &report,
                    watch_timeout_message(&report, timeout, elapsed),
                ))?;
            } else {
                print_watch_timeout(ctx, &report, timeout, elapsed);
            }
            return exit_result(exit_code);
        }

        if !changed
            && options
                .heartbeat
                .is_some_and(|heartbeat| now.duration_since(last_output_at) >= heartbeat)
        {
            let unchanged_duration = now.duration_since(last_output_at);
            let heartbeat = options.heartbeat.expect("heartbeat was checked above");
            if let Some(agent_id) = wait_observation_agent_id(ctx, &work)? {
                record_wait_observation(
                    ctx,
                    &agent_id,
                    &report,
                    "heartbeat",
                    elapsed,
                    heartbeat,
                    unchanged_duration,
                )?;
            }
            if ctx.is_json() {
                print_json_line(&report)?;
            } else {
                print_watch_heartbeat(ctx, &report, elapsed);
            }
            last_output_at = now;
        }

        let sleep_duration = watch_sleep_duration(
            options.interval,
            options.timeout,
            options.heartbeat,
            started_at.elapsed(),
            last_output_at.elapsed(),
        );
        if sleep_duration > Duration::ZERO {
            std::thread::sleep(sleep_duration);
        }
    }
}

fn duration_from_secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn exit_result(exit_code: i32) -> Result<()> {
    if exit_code == 0 {
        Ok(())
    } else {
        Err(WtError::Exit { code: exit_code }.into())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AgentStatusReport {
    target: String,
    branch: String,
    worktree: Option<String>,
    task_run: TaskRunStatusReport,
    agent: AgentObservationReport,
    cmux: CmuxObservationReport,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct TaskRunStatusReport {
    id: Option<String>,
    status: Option<String>,
    context: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AgentObservationReport {
    kind: String,
    state: String,
    last_tool: Option<String>,
    last_event_at: Option<String>,
    session_id: Option<String>,
    needs_input_since: Option<String>,
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct CmuxObservationReport {
    state: String,
    workspace: Option<String>,
    surface: Option<String>,
    workspace_id: Option<String>,
    surface_id: Option<String>,
    pane: Option<String>,
    window: Option<String>,
    workspace_title: Option<String>,
    candidates: Vec<CmuxCandidateReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct CmuxCandidateReport {
    workspace: String,
    surface: String,
    pane: String,
    window: String,
    selected: bool,
    readable: bool,
    agent: String,
    state: String,
    warning: Option<String>,
    live_agent_candidate: bool,
}

fn observe_status(
    ctx: &Ctx,
    target: Option<&str>,
    command: &str,
) -> Result<(AgentStatusReport, i32)> {
    let target = resolve_agent_target(ctx, target, command)?;
    let work = work::observe_target(ctx, target)?;
    let exit_code = exit_code_for(&work);
    Ok((AgentStatusReport::from_work(ctx, &work), exit_code))
}

fn resolve_agent_target(
    ctx: &Ctx,
    target: Option<&str>,
    command: &str,
) -> Result<work::WorkTarget> {
    match target {
        Some(target) => work::resolve_target(ctx, Some(target)),
        None => {
            let guidance = agent_target_required_message(command);
            work::select_target(ctx, "Agent work target", &guidance)
        }
    }
}

fn agent_target_required_message(command: &str) -> String {
    format!(
        "wt agent {command} requires TARGET when it cannot open an interactive selector. Pass a branch, worktree path/name, or TaskRun id; use `wt agent status <target>` to observe once, `wt agent watch <target>` to poll, or `wt inspect [<target>]` for the work dossier."
    )
}

impl AgentStatusReport {
    fn from_work(ctx: &Ctx, work: &work::Work) -> Self {
        let cmux = work.cmux.as_ref();
        let task_run = work.target.task_run.as_ref();
        let task_run_context = task_run.map(|record| {
            task_run::resolve_context(ctx, record)
                .map(|context| context.label())
                .unwrap_or_else(|err| format!("unavailable ({err:#})"))
        });
        Self {
            target: work.target.label.clone(),
            branch: work.target.branch.clone(),
            worktree: work
                .target
                .worktree
                .as_ref()
                .map(|path| path.display().to_string()),
            task_run: TaskRunStatusReport {
                id: task_run.map(|record| record.id.clone()),
                status: task_run.map(|record| record.run.status.to_string()),
                context: task_run_context,
            },
            agent: AgentObservationReport {
                kind: work.state.agent_kind.as_str().to_string(),
                state: work.state.status.as_str().to_string(),
                last_tool: work.state.last_tool.clone(),
                last_event_at: work.state.last_event_at.clone(),
                session_id: work.state.session_id.clone(),
                needs_input_since: work.state.needs_input_since.clone(),
                metadata: work.state.metadata.clone(),
            },
            cmux: CmuxObservationReport {
                state: work.session_state.as_str().to_string(),
                workspace: cmux.map(|cmux| cmux.workspace_ref.clone()),
                surface: cmux.and_then(|cmux| cmux.surface_ref.clone()),
                workspace_id: cmux.map(|cmux| cmux.workspace_id.clone()),
                surface_id: cmux.and_then(|cmux| cmux.surface_id.clone()),
                pane: cmux.and_then(|cmux| cmux.pane_ref.clone()),
                window: cmux.map(|cmux| cmux.window_ref.clone()),
                workspace_title: cmux.map(|cmux| cmux.workspace_title.clone()),
                candidates: work
                    .cmux_contacts
                    .iter()
                    .map(CmuxCandidateReport::from_contact)
                    .collect(),
            },
            warnings: warnings_for_work(work),
        }
    }

    fn transition_signature(&self) -> String {
        format!(
            "{}:{}:{}:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
            self.target,
            self.agent.kind,
            self.agent.state,
            self.cmux.state,
            self.cmux.workspace,
            self.cmux.surface,
            self.cmux.candidates,
            self.task_run.id,
            self.task_run.status,
            self.task_run.context,
            self.agent.last_tool,
            self.agent.last_event_at,
            self.warnings
        )
    }
}

impl CmuxCandidateReport {
    fn from_contact(contact: &work::CmuxContact) -> Self {
        Self {
            workspace: contact.workspace.clone(),
            surface: contact.surface.clone(),
            pane: contact.pane.clone(),
            window: contact.window.clone(),
            selected: contact.selected,
            readable: contact.readable,
            agent: contact.state.agent_kind.as_str().to_string(),
            state: contact.state.status.as_str().to_string(),
            warning: contact.validation_warning.clone(),
            live_agent_candidate: contact.is_live_agent_candidate(),
        }
    }
}

fn warnings_for_work(work: &work::Work) -> Vec<String> {
    let mut warnings = Vec::new();
    for warning in &work.target.warnings {
        if !warnings.contains(warning) {
            warnings.push(warning.clone());
        }
    }
    if let Some(warning) = work.state.warning.as_ref() {
        warnings.push(warning.clone());
    }
    if let Some(cmux) = work.cmux.as_ref() {
        if let Some(warning) = work
            .cmux_contacts
            .iter()
            .find(|contact| {
                contact.workspace == cmux.workspace_ref
                    && Some(contact.surface.as_str()) == cmux.surface_ref.as_deref()
            })
            .and_then(|contact| contact.validation_warning.as_ref())
        {
            if !warnings.contains(warning) {
                warnings.push(warning.clone());
            }
        }
    }
    if work.session_state != WorkSessionState::TerminalSurfaceReady {
        for warning in work
            .cmux_contacts
            .iter()
            .filter_map(|contact| contact.validation_warning.as_ref())
        {
            if !warnings.contains(warning) {
                warnings.push(warning.clone());
            }
        }
    }
    if let Some(message) = work.message.as_ref() {
        warnings.push(message.clone());
    }
    warnings
}

fn exit_code_for(work: &work::Work) -> i32 {
    match work.session_state {
        WorkSessionState::NoLocalWorktree
        | WorkSessionState::CmuxUnavailable
        | WorkSessionState::NoCmuxWorkspace
        | WorkSessionState::NoTerminalSurface
        | WorkSessionState::AmbiguousTerminalSurface => return 1,
        WorkSessionState::TerminalSurfaceReady => {}
    }

    match work.state.status {
        AgentStatus::NeedsInput => 2,
        AgentStatus::Failed => 3,
        _ => 0,
    }
}

fn should_stop_watching(work: &work::Work, exit_code: i32) -> bool {
    exit_code != 0 || work.state.status != AgentStatus::Running
}

fn watch_sleep_duration(
    interval: Duration,
    timeout: Option<Duration>,
    heartbeat: Option<Duration>,
    elapsed: Duration,
    since_last_output: Duration,
) -> Duration {
    let mut sleep = (interval > Duration::ZERO).then_some(interval);
    if let Some(timeout) = timeout {
        sleep = Some(match sleep {
            Some(current) => current.min(timeout.saturating_sub(elapsed)),
            None => timeout.saturating_sub(elapsed),
        });
    }
    if let Some(heartbeat) = heartbeat {
        sleep = Some(match sleep {
            Some(current) => current.min(heartbeat.saturating_sub(since_last_output)),
            None => heartbeat.saturating_sub(since_last_output),
        });
    }
    sleep.unwrap_or(Duration::ZERO)
}

fn report_with_warning(report: &AgentStatusReport, warning: String) -> AgentStatusReport {
    let mut report = report.clone();
    if !report.warnings.contains(&warning) {
        report.warnings.push(warning);
    }
    report
}

fn record_wait_observation(
    ctx: &Ctx,
    agent_id: &AgentId,
    report: &AgentStatusReport,
    wait_reason: &str,
    elapsed_duration: Duration,
    bound_duration: Duration,
    unchanged_duration: Duration,
) -> Result<()> {
    let mut observation = WaitObservation::new_non_idle(NewWaitObservation {
        wait_reason: wait_reason.into(),
        elapsed_seconds: elapsed_duration.as_secs(),
        bound_seconds: bound_duration.as_secs(),
        unchanged_seconds: unchanged_duration.as_secs(),
        target: report.target.clone(),
        branch: report.branch.clone(),
        agent_kind: report.agent.kind.clone(),
        agent_state: report.agent.state.clone(),
    });
    observation.worktree = report.worktree.clone();
    observation.task_run_id = report.task_run.id.clone();
    observation.last_tool = report.agent.last_tool.clone();
    observation.last_event_at = report.agent.last_event_at.clone();
    observation.session_id = report.agent.session_id.clone();

    let store = WaitObservationStore::new(ctx.storage_root.wait_observations_jsonl(agent_id));
    store.append(&observation).with_context(|| {
        format!(
            "Failed to record wait observation in {}",
            ctx.storage_root.display_path(store.path())
        )
    })
}

fn wait_observation_agent_id(ctx: &Ctx, work: &work::Work) -> Result<Option<AgentId>> {
    if let Some(agent_id) = work
        .target
        .task_run
        .as_ref()
        .and_then(|record| record.run.agent_id.as_deref())
    {
        return AgentId::parse(agent_id)
            .with_context(|| format!("Invalid TaskRun agent_id for wait observation: {agent_id}"))
            .map(Some);
    }

    let Some(surface_id) = work
        .cmux
        .as_ref()
        .and_then(|cmux| cmux.surface_id.as_deref())
    else {
        return Ok(None);
    };
    let key = AnchorKey {
        kind: AnchorKind::Surface,
        value: surface_id.to_string(),
    };
    let Some(marker) = identity_locator::read_marker(ctx, &key)? else {
        return Ok(None);
    };
    AgentId::parse(&marker.id)
        .with_context(|| format!("Invalid live surface marker agent id: {}", marker.id))
        .map(Some)
}

fn wait_observation_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let agents_dir = ctx.storage_root.runtime_agents_dir();
    let entries = match fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to read runtime agents directory {}",
                    agents_dir.display()
                )
            });
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "Failed to read runtime agents directory entry in {}",
                agents_dir.display()
            )
        })?;
        if !entry
            .file_type()
            .with_context(|| format!("Failed to inspect {}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let path = entry
            .path()
            .join("observations")
            .join(crate::agent_state::WAIT_OBSERVATIONS_FILE);
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn wait_observation_summary_path(ctx: &Ctx) -> String {
    format!(
        "{}/*/observations/{}",
        ctx.storage_root
            .display_path(&ctx.storage_root.runtime_agents_dir()),
        crate::agent_state::WAIT_OBSERVATIONS_FILE
    )
}

fn print_json<T: Serialize>(report: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

fn print_json_line<T: Serialize>(report: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

fn print_wait_stats(ctx: &Ctx, summary: &WaitObservationSummary) {
    ctx.ui
        .print_plain(&format!("Agent wait stats: {}", summary.path));
    ctx.ui.print_dim(&format!("  Count: {}", summary.count));
    ctx.ui
        .print_dim(&format!("  Sum seconds: {}", summary.sum_seconds));
    ctx.ui.print_dim(&format!(
        "  Average seconds: {}",
        format_average_seconds(summary.average_seconds)
    ));
    ctx.ui.print_dim(&format!(
        "  Min seconds: {}",
        format_optional_seconds(summary.min_seconds)
    ));
    ctx.ui.print_dim(&format!(
        "  Max seconds: {}",
        format_optional_seconds(summary.max_seconds)
    ));
    if summary.count == 0 {
        ctx.ui
            .print_dim("  Empty state: no non-idle wait observations recorded");
    }
    if summary.buckets.is_empty() {
        ctx.ui.print_dim("  Buckets: none");
    } else {
        ctx.ui.print_dim("  Buckets:");
        for (bucket, count) in &summary.buckets {
            ctx.ui.print_dim(&format!("    - {bucket}: {count}"));
        }
    }
    print_wait_stats_groups(ctx, summary);
}

fn format_optional_seconds(value: Option<u64>) -> String {
    value
        .map(|seconds| seconds.to_string())
        .unwrap_or_else(|| "none".into())
}

fn format_average_seconds(value: Option<f64>) -> String {
    match value {
        Some(value) if value.fract().abs() < f64::EPSILON => format!("{value:.0}"),
        Some(value) => format!("{value:.2}"),
        None => "none".into(),
    }
}

fn print_wait_stats_groups(ctx: &Ctx, summary: &WaitObservationSummary) {
    if summary.count == 0 {
        return;
    }
    ctx.ui.print_dim("  Groups:");
    print_wait_stats_group(ctx, "wait_reason", &summary.groups.wait_reason);
    print_wait_stats_group(ctx, "bound_seconds", &summary.groups.bound_seconds);
    print_wait_stats_group(ctx, "agent_kind", &summary.groups.agent_kind);
    print_wait_stats_group(ctx, "agent_state", &summary.groups.agent_state);
}

fn print_wait_stats_group(
    ctx: &Ctx,
    label: &str,
    groups: &BTreeMap<String, crate::agent_state::WaitObservationGroupSummary>,
) {
    ctx.ui.print_dim(&format!("    {label}:"));
    for (value, group) in groups {
        ctx.ui.print_dim(&format!(
            "      - {value}: count {}, avg {}s, min {}, max {}",
            group.count,
            format_average_seconds(group.average_seconds),
            format_optional_seconds(group.min_seconds),
            format_optional_seconds(group.max_seconds)
        ));
    }
}

fn print_text(ctx: &Ctx, report: &AgentStatusReport) {
    ctx.ui
        .print_step(&format!("Agent status: {}", report.target));
    ctx.ui.print_dim(&format!("  Branch: {}", report.branch));
    match report.worktree.as_deref() {
        Some(path) => ctx.ui.print_dim(&format!("  Worktree: {path}")),
        None => ctx
            .ui
            .print_dim("  Worktree: branch is not checked out in a local worktree"),
    }
    print_task_run_text(ctx, report);
    ctx.ui.print_dim(&format!(
        "  Agent: {} ({})",
        report.agent.kind, report.agent.state
    ));
    if let Some(last_tool) = report.agent.last_tool.as_deref() {
        ctx.ui.print_dim(&format!("  Last tool: {last_tool}"));
    }
    if let Some(last_event_at) = report.agent.last_event_at.as_deref() {
        ctx.ui.print_dim(&format!("  Last event: {last_event_at}"));
    }
    if let Some(session_id) = report.agent.session_id.as_deref() {
        ctx.ui.print_dim(&format!("  Session: {session_id}"));
    }
    if let Some(needs_input_since) = report.agent.needs_input_since.as_deref() {
        ctx.ui
            .print_dim(&format!("  Needs input since: {needs_input_since}"));
    }
    print_cmux_text(ctx, report);
    print_cmux_candidates_text(ctx, &report.cmux.candidates);
    for warning in &report.warnings {
        ctx.ui.print_warning(warning);
    }
}

fn print_task_run_text(ctx: &Ctx, report: &AgentStatusReport) {
    match (
        report.task_run.id.as_deref(),
        report.task_run.status.as_deref(),
        report.task_run.context.as_deref(),
    ) {
        (Some(id), Some(status), Some(context)) => ctx.ui.print_dim(&format!(
            "  TaskRun: {id} (status={status}, context={context})"
        )),
        (Some(id), Some(status), None) => ctx
            .ui
            .print_dim(&format!("  TaskRun: {id} (status={status})")),
        _ => ctx.ui.print_dim("  TaskRun: none"),
    }
}

fn print_watch_transition(ctx: &Ctx, report: &AgentStatusReport) {
    ctx.ui.print_step(&format!(
        "Agent watch: {} {} ({})",
        report.target, report.agent.state, report.agent.kind
    ));
    print_watch_details(ctx, report, WatchDetailLevel::Transition);
}

fn print_watch_heartbeat(ctx: &Ctx, report: &AgentStatusReport, elapsed: Duration) {
    ctx.ui.print_step(&format!(
        "Agent watch heartbeat: elapsed {}; {} {} ({})",
        format_duration(elapsed),
        report.target,
        report.agent.state,
        report.agent.kind
    ));
    print_watch_details(ctx, report, WatchDetailLevel::Heartbeat);
}

fn print_watch_timeout(
    ctx: &Ctx,
    report: &AgentStatusReport,
    timeout: Duration,
    elapsed: Duration,
) {
    ctx.ui
        .print_step(&watch_timeout_message(report, timeout, elapsed));
    print_watch_details(ctx, report, WatchDetailLevel::Heartbeat);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchDetailLevel {
    Transition,
    Heartbeat,
}

fn print_watch_details(ctx: &Ctx, report: &AgentStatusReport, detail_level: WatchDetailLevel) {
    ctx.ui.print_dim(&format!("  Branch: {}", report.branch));
    print_task_run_text(ctx, report);
    if detail_level == WatchDetailLevel::Heartbeat {
        if let Some(last_tool) = report.agent.last_tool.as_deref() {
            ctx.ui.print_dim(&format!("  Last tool: {last_tool}"));
        }
    }
    if let Some(last_event_at) = report.agent.last_event_at.as_deref() {
        ctx.ui.print_dim(&format!("  Last event: {last_event_at}"));
    }
    if detail_level == WatchDetailLevel::Heartbeat {
        if let Some(session_id) = report.agent.session_id.as_deref() {
            ctx.ui.print_dim(&format!("  Session: {session_id}"));
        }
    }
    print_cmux_text(ctx, report);
    print_cmux_candidates_text(ctx, &report.cmux.candidates);
    for warning in &report.warnings {
        ctx.ui.print_warning(warning);
    }
}

fn watch_timeout_message(
    report: &AgentStatusReport,
    timeout: Duration,
    elapsed: Duration,
) -> String {
    format!(
        "Agent watch timeout after {}: {} still {} ({}); elapsed {}",
        format_duration(timeout),
        report.target,
        report.agent.state,
        report.agent.kind,
        format_duration(elapsed)
    )
}

fn format_duration(duration: Duration) -> String {
    format!("{}s", duration.as_secs())
}

fn print_cmux_candidates_text(ctx: &Ctx, candidates: &[CmuxCandidateReport]) {
    if candidates.is_empty() {
        return;
    }
    if candidates.len() == 1
        && candidates[0].live_agent_candidate
        && candidates[0].warning.is_none()
    {
        return;
    }

    ctx.ui
        .print_dim(&format!("  cmux candidates: {}", candidates.len()));
    for candidate in candidates {
        let selected = if candidate.selected { " selected" } else { "" };
        let readable = if candidate.readable {
            "readable"
        } else {
            "unreadable"
        };
        let warning = candidate
            .warning
            .as_deref()
            .map(|warning| format!(", warning={warning}"))
            .unwrap_or_default();
        ctx.ui.print_dim(&format!(
            "    - {} {}{} (pane {}, window {}, {}, agent={} state={}{})",
            candidate.workspace,
            candidate.surface,
            selected,
            candidate.pane,
            candidate.window,
            readable,
            candidate.agent,
            candidate.state,
            warning
        ));
    }
}

fn print_cmux_text(ctx: &Ctx, report: &AgentStatusReport) {
    match (
        report.cmux.workspace.as_deref(),
        report.cmux.surface.as_deref(),
    ) {
        (Some(workspace), Some(surface)) => {
            let pane = report.cmux.pane.as_deref().unwrap_or("unknown-pane");
            let window = report.cmux.window.as_deref().unwrap_or("unknown-window");
            ctx.ui.print_dim(&format!(
                "  cmux: {workspace} {surface} (pane {pane}, window {window})"
            ));
        }
        (Some(workspace), None) => {
            ctx.ui.print_dim(&format!(
                "  cmux: {workspace} (terminal surface unavailable)"
            ));
        }
        (None, None) => {
            ctx.ui.print_dim(&format!("  cmux: {}", report.cmux.state));
        }
        (None, Some(surface)) => {
            ctx.ui
                .print_dim(&format!("  cmux: {surface} (workspace unavailable)"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn json_report_includes_status_shape_for_codex_work() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex v0.130.0\nReady", true);
        runner.add_response("codex=Idle icon=pause.circle.fill color=#8E8E93", true);
        runner.add_response(
            r#"{"seq":10,"name":"agent.hook.PreToolUse","occurred_at":"2026-05-16T00:00:10Z","workspace_id":"uuid-workspace-1","surface_id":"uuid-surface-4","payload":{"tool_name":"Bash","session_id":"codex-session"}}"#,
            true,
        );
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.branch, "feature");
        assert_eq!(
            report.worktree.as_deref(),
            Some(fixture.worktree.to_str().unwrap())
        );
        assert_eq!(
            report.cmux.workspace_id.as_deref(),
            Some("uuid-workspace-1")
        );
        assert_eq!(report.cmux.workspace.as_deref(), Some("workspace:1"));
        assert_eq!(report.cmux.surface_id.as_deref(), Some("uuid-surface-4"));
        assert_eq!(report.cmux.surface.as_deref(), Some("surface:4"));
        assert_eq!(report.agent.kind, "codex");
        assert_eq!(report.agent.state, "idle");
        assert_eq!(report.agent.last_tool.as_deref(), Some("Bash"));
        assert_eq!(report.agent.session_id.as_deref(), Some("codex-session"));
        assert_eq!(report.warnings, Vec::<String>::new());

        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["branch"], "feature");
        assert!(value.get("status").is_none());
        assert_eq!(value["task_run"]["status"], serde_json::Value::Null);
        assert_eq!(value["agent"]["kind"], "codex");
        assert_eq!(value["agent"]["state"], "idle");
        assert_eq!(value["cmux"]["workspace"], "workspace:1");
        assert_eq!(value["cmux"]["surface"], "surface:4");
        assert_eq!(value["cmux"]["state"], "terminal_surface_ready");
        assert_eq!(value["cmux"]["candidates"][0]["agent"], "codex");
        assert_eq!(value["cmux"]["candidates"][0]["state"], "idle");
        assert_eq!(value["cmux"]["candidates"][0]["live_agent_candidate"], true);
        assert_eq!(value["warnings"], json!([]));
    }

    #[test]
    fn human_output_names_target_agent_status_and_cmux_contact() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        status(&ctx, Some("feature")).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Agent status: feature"));
        assert!(dims.contains("TaskRun: none"));
        assert!(dims.contains("Agent: codex (idle)"));
        assert!(dims.contains("cmux: workspace:1 surface:4"));
    }

    #[test]
    fn status_branch_target_warns_about_unrelated_invalid_task_run() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".git/wt/execution/task-runs")).unwrap();
        std::fs::write(
            fixture
                .repo
                .join(".git/wt/execution/task-runs/run-broken.toml"),
            "task = \"broken\"\nbranch = \"unrelated\"\nstatus = \"started\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        status(&ctx, Some("feature")).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let warnings = ui.warnings.lock().unwrap().join("\n");
        assert!(steps.contains("Agent status: feature"));
        assert!(warnings.contains("TaskRun inventory skipped invalid record"));
        assert!(warnings.contains("run-broken.toml"));
    }

    #[test]
    fn needs_input_maps_to_exit_code_two_and_records_since_time() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex v0.130.0\nPermission request", true);
        runner.add_response("", true);
        runner.add_response(
            r#"{"seq":10,"name":"agent.hook.PermissionRequest","occurred_at":"2026-05-16T00:00:10Z","workspace_id":"uuid-workspace-1","surface_id":"uuid-surface-4","payload":{"session_id":"codex-session"}}"#,
            true,
        );
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 2);
        assert_eq!(report.agent.state, "needs_input");
        assert_eq!(
            report.agent.needs_input_since.as_deref(),
            Some("2026-05-16T00:00:10Z")
        );
    }

    #[test]
    fn failed_status_maps_to_exit_code_three() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex fatal: command failed", true);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 3);
        assert_eq!(report.agent.state, "failed");
    }

    #[test]
    fn status_keeps_codex_commit_text_surface_out_of_live_candidates() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        runner.add_response(
            "lazygit\n31fdd27 fix: Codex literal screen binding\n3fc13dc fix: Codex model screen binding",
            true,
        );
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.agent.kind, "codex");
        assert_eq!(report.agent.state, "idle");
        assert_eq!(report.cmux.surface.as_deref(), Some("surface:4"));
        assert_eq!(report.cmux.candidates.len(), 2);
        assert!(report.cmux.candidates[0].live_agent_candidate);
        assert_eq!(report.cmux.candidates[0].agent, "codex");
        assert!(!report.cmux.candidates[1].live_agent_candidate);
        assert_eq!(report.cmux.candidates[1].agent, "unknown");
        assert_eq!(
            report.cmux.candidates[1].warning.as_deref(),
            Some("no live agent signal")
        );
    }

    #[test]
    fn missing_target_returns_not_found_error() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let err = observe_status(&ctx, Some("missing"), "status")
            .unwrap_err()
            .to_string();

        assert!(err.contains("Work target not found: missing"));
    }

    #[test]
    fn no_local_worktree_reports_no_session_without_cmux_lookup() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\n",
                fixture.repo.display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\n",
                fixture.repo.display()
            ),
            true,
        );
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 1);
        assert_eq!(report.agent.state, "no_session");
        assert_eq!(report.worktree, None);
        assert_eq!(report.cmux.state, "no_local_worktree");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("not checked out"))
        );
    }

    #[test]
    fn no_cmux_fails_with_clear_warning() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 1);
        assert_eq!(report.agent.state, "no_session");
        assert_eq!(report.cmux.state, "cmux_unavailable");
        assert_eq!(report.warnings, vec!["cmux command not found"]);
    }

    #[test]
    fn no_cmux_workspace_is_a_pollable_no_session_state() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{"id":"uuid-workspace-2","ref":"workspace:2","title":"other","current_directory":"/tmp/other"}]}"#,
            true,
        );
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 1);
        assert_eq!(report.agent.state, "no_session");
        assert_eq!(report.cmux.state, "no_cmux_workspace");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("No cmux workspace found"))
        );
    }

    #[test]
    fn cold_terminal_reports_workspace_and_warning() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Terminal surface not found", false);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 1);
        assert_eq!(report.agent.state, "no_session");
        assert_eq!(report.cmux.workspace.as_deref(), Some("workspace:1"));
        assert_eq!(report.cmux.surface, None);
        assert_eq!(report.cmux.state, "no_terminal_surface");
        assert_eq!(report.cmux.candidates.len(), 1);
        assert_eq!(report.cmux.candidates[0].surface, "surface:4");
        assert!(!report.cmux.candidates[0].readable);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("unreadable cmux surface"))
        );
    }

    #[test]
    fn claude_status_uses_cmux_status_and_hook_events() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("⏵⏵ bypass permissions on\nWorking", true);
        runner.add_response("claude_code=Running", true);
        runner.add_response(
            r#"{"seq":12,"name":"agent.hook.PreToolUse","occurred_at":"2026-05-16T00:00:12Z","workspace_id":"uuid-workspace-1","surface_id":"uuid-surface-4","payload":{"tool_name":"Read","session_id":"claude-session"}}"#,
            true,
        );
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("feature"), "status").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.agent.kind, "claude_code");
        assert_eq!(report.agent.state, "running");
        assert_eq!(report.agent.last_tool.as_deref(), Some("Read"));
        assert_eq!(report.agent.session_id.as_deref(), Some("claude-session"));
    }

    #[test]
    fn task_run_target_adds_task_run_metadata() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".git/wt/execution/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".git/wt/execution/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("run-feature"), "status").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.target, "run-feature");
        assert_eq!(report.branch, "feature");
        assert_eq!(report.task_run.id.as_deref(), Some("run-feature"));
        assert_eq!(report.task_run.status.as_deref(), Some("running"));
        assert_eq!(report.task_run.context.as_deref(), Some("direct"));
    }

    #[test]
    fn status_without_target_selects_interactive_work_target() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".git/wt/execution/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".git/wt/execution/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        std::fs::write(
            fixture
                .repo
                .join(".git/wt/execution/task-runs/run-broken.toml"),
            "task = \"broken\"\nbranch = \"unrelated\"\nstatus = \"started\"\ncreated_at = \"2026-05-16T00:00:01Z\"\nupdated_at = \"2026-05-16T00:00:01Z\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("feature\nmaster\n", true);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let mut ui = MockUi::new();
        ui.add_select(0);
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        status(&ctx, None).unwrap();

        let prompts = ui.prompts.lock().unwrap().join("\n");
        let items = ui.select_items.lock().unwrap();
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(prompts.contains("select: Agent work target"));
        assert!(items[0].iter().any(|item| item.contains("feature")));
        assert!(items[0].iter().any(|item| item.contains("run-feature")));
        assert!(steps.contains("Agent status: feature"));
        let warnings = ui.warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("TaskRun inventory skipped invalid record"));
        assert!(warnings.contains("run-broken.toml"));
    }

    #[test]
    fn status_without_target_requires_explicit_target_for_json() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(MockRunner::new(), OutputMode::Json);

        let err = status(&ctx, None).unwrap_err().to_string();

        assert!(err.contains("wt agent status requires TARGET"));
        assert!(err.contains("wt agent status <target>"));
        assert!(err.contains("wt agent watch <target>"));
        assert!(err.contains("wt inspect [<target>]"));
    }

    #[test]
    fn watch_prints_transitions_and_exits_with_needs_input_code() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_running_observation(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Permission request", true);
        runner.add_response("", true);
        runner.add_response(
            r#"{"seq":11,"name":"agent.hook.PermissionRequest","occurred_at":"2026-05-16T00:00:11Z","workspace_id":"uuid-workspace-1","surface_id":"uuid-surface-4","payload":{"session_id":"codex-session"}}"#,
            true,
        );
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        let err = watch_with_interval(&ctx, Some("feature"), Duration::ZERO).unwrap_err();

        assert!(matches!(
            err.downcast_ref::<WtError>(),
            Some(WtError::Exit { code: 2 })
        ));
        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Agent watch: feature running (codex)"));
        assert!(steps.contains("Agent watch: feature needs_input (codex)"));
    }

    #[test]
    fn watch_timeout_stops_still_running_agent_with_clear_result() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_running_observation(&mut runner, &fixture);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        watch_with_options(
            &ctx,
            Some("feature"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: Some(Duration::ZERO),
                heartbeat: None,
            },
        )
        .unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Agent watch: feature running (codex)"));
        assert!(
            steps.contains("Agent watch timeout after 0s: feature still running (codex); elapsed")
        );
        assert!(dims.contains("Last tool: Bash"));
        assert!(dims.contains("Session: codex-session"));
        assert!(dims.contains("cmux: workspace:1 surface:4"));
    }

    #[test]
    fn watch_recording_timeout_appends_one_non_idle_wait_observation() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_running_observation(&mut runner, &fixture);
        let ctx = fixture.ctx(runner, OutputMode::Text);
        identity_locator::write_marker(
            &ctx,
            &AnchorKey {
                kind: AnchorKind::Surface,
                value: "uuid-surface-4".into(),
            },
            "agents/codex",
            Some("codex"),
        )
        .unwrap();

        watch_with_options(
            &ctx,
            Some("feature"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: Some(Duration::ZERO),
                heartbeat: None,
            },
        )
        .unwrap();

        let observations = read_wait_observations(&fixture);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0]["wait_class"], "non_idle");
        assert_eq!(observations[0]["wait_reason"], "timeout");
        assert_eq!(observations[0]["elapsed_seconds"], 0);
        assert_eq!(observations[0]["bound_seconds"], 0);
        assert_eq!(observations[0]["unchanged_seconds"], 0);
        assert_eq!(observations[0]["agent_state"], "running");
        assert_eq!(observations[0]["agent_kind"], "codex");
        assert_eq!(observations[0]["target"], "feature");
        assert_eq!(observations[0]["last_tool"], "Bash");
        assert_no_cmux_transport_fields(&observations[0]);
    }

    #[test]
    fn wait_observation_record_separates_elapsed_bound_and_unchanged_seconds() {
        let fixture = Fixture::new();
        let ctx = fixture.ctx(MockRunner::new(), OutputMode::Text);
        let report = test_running_report();
        let agent = AgentId::parse("agents/codex").unwrap();

        record_wait_observation(
            &ctx,
            &agent,
            &report,
            "heartbeat",
            Duration::from_secs(125),
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
        .unwrap();
        record_wait_observation(
            &ctx,
            &agent,
            &report,
            "timeout",
            Duration::from_secs(300),
            Duration::from_secs(300),
            Duration::from_secs(140),
        )
        .unwrap();

        let observations = read_wait_observations(&fixture);
        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0]["wait_reason"], "heartbeat");
        assert_eq!(observations[0]["elapsed_seconds"], 125);
        assert_eq!(observations[0]["bound_seconds"], 60);
        assert_eq!(observations[0]["unchanged_seconds"], 60);
        assert_eq!(observations[1]["wait_reason"], "timeout");
        assert_eq!(observations[1]["elapsed_seconds"], 300);
        assert_eq!(observations[1]["bound_seconds"], 300);
        assert_eq!(observations[1]["unchanged_seconds"], 140);
        assert!(observations.iter().all(has_no_cmux_transport_fields));
    }

    #[test]
    fn watch_heartbeat_prints_unchanged_running_observation() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".git/wt/execution/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".git/wt/execution/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nagent_id = \"agents/codex\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        add_running_observation(&mut runner, &fixture);
        add_running_observation(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        watch_with_options(
            &ctx,
            Some("run-feature"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: None,
                heartbeat: Some(Duration::ZERO),
            },
        )
        .unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Agent watch: run-feature running (codex)"));
        assert!(steps.contains("Agent watch heartbeat: elapsed 0s; run-feature running (codex)"));
        assert!(steps.contains("Agent watch: run-feature idle (codex)"));
        assert!(dims.contains("TaskRun: run-feature (status=running, context=direct)"));
        assert!(dims.contains("Last tool: Bash"));
        assert!(dims.contains("Session: codex-session"));
        assert!(dims.contains("cmux: workspace:1 surface:4"));
    }

    #[test]
    fn watch_recording_heartbeat_appends_one_non_idle_wait_observation() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".git/wt/execution/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".git/wt/execution/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nagent_id = \"agents/codex\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        add_running_observation(&mut runner, &fixture);
        add_running_observation(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let ctx = fixture.ctx(runner, OutputMode::Text);

        watch_with_options(
            &ctx,
            Some("run-feature"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: None,
                heartbeat: Some(Duration::ZERO),
            },
        )
        .unwrap();

        let observations = read_wait_observations(&fixture);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0]["wait_class"], "non_idle");
        assert_eq!(observations[0]["wait_reason"], "heartbeat");
        assert_eq!(observations[0]["elapsed_seconds"], 0);
        assert_eq!(observations[0]["bound_seconds"], 0);
        assert_eq!(observations[0]["unchanged_seconds"], 0);
        assert_eq!(observations[0]["agent_state"], "running");
        assert_eq!(observations[0]["task_run_id"], "run-feature");
        assert_no_cmux_transport_fields(&observations[0]);
    }

    #[test]
    fn watch_exits_zero_when_agent_is_no_longer_running() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_running_observation(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            fixture.repo.clone(),
            fixture.repo.clone(),
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        watch_with_interval(&ctx, Some("feature"), Duration::ZERO).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Agent watch: feature running (codex)"));
        assert!(steps.contains("Agent watch: feature idle (codex)"));
    }

    #[test]
    fn watch_recording_idle_observation_does_not_create_non_idle_sample() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        let ctx = fixture.ctx(runner, OutputMode::Text);

        watch_with_options(
            &ctx,
            Some("feature"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: Some(Duration::ZERO),
                heartbeat: None,
            },
        )
        .unwrap();

        assert!(!wait_observations_path(&fixture).exists());
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        repo: PathBuf,
        worktree: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().join("sample");
            let worktree = dir.path().join("sample-feature");
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::create_dir_all(&worktree).unwrap();
            Self {
                _dir: dir,
                repo,
                worktree,
            }
        }

        fn ctx(&self, runner: MockRunner, output_mode: OutputMode) -> Ctx {
            Ctx::new_with_options(
                self.repo.clone(),
                self.repo.clone(),
                Config::default(),
                Box::new(runner),
                Box::new(MockUi::new()),
                CtxOptions {
                    base_config: Config::default(),
                    config_source: crate::config::ConfigSource::Default,
                    storage_root: None,
                    output_mode,
                    verbosity: 0,
                    quiet: false,
                    launcher_coordinator_id: None,
                },
            )
        }
    }

    fn add_worktree_list(runner: &mut MockRunner, fixture: &Fixture) {
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                fixture.repo.display(),
                fixture.worktree.display()
            ),
            true,
        );
    }

    fn add_matching_workspace(runner: &mut MockRunner, fixture: &Fixture) {
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                fixture.worktree.display()
            ),
            true,
        );
    }

    fn add_selected_surface(runner: &mut MockRunner) {
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(runner);
    }

    fn add_no_surface_processes(runner: &mut MockRunner) {
        runner.add_response(
            r#"{"windows":[{"workspaces":[{"panes":[{"surfaces":[]}]}]}]}"#,
            true,
        );
    }

    fn add_running_observation(runner: &mut MockRunner, fixture: &Fixture) {
        add_matching_workspace(runner, fixture);
        add_selected_surface(runner);
        runner.add_response("Codex Working", true);
        runner.add_response("codex=Running", true);
        runner.add_response(
            r#"{"seq":10,"name":"agent.hook.PreToolUse","occurred_at":"2026-05-16T00:00:10Z","workspace_id":"uuid-workspace-1","surface_id":"uuid-surface-4","payload":{"tool_name":"Bash","session_id":"codex-session"}}"#,
            true,
        );
    }

    fn wait_observations_path(fixture: &Fixture) -> PathBuf {
        fixture
            .repo
            .join(".git/wt/runtime/agents/codex/observations/wait-observations.jsonl")
    }

    fn read_wait_observations(fixture: &Fixture) -> Vec<serde_json::Value> {
        std::fs::read_to_string(wait_observations_path(fixture))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn assert_no_cmux_transport_fields(observation: &serde_json::Value) {
        assert!(has_no_cmux_transport_fields(observation));
    }

    fn has_no_cmux_transport_fields(observation: &serde_json::Value) -> bool {
        observation
            .as_object()
            .unwrap()
            .keys()
            .all(|key| !key.starts_with("cmux_"))
    }

    fn test_running_report() -> AgentStatusReport {
        AgentStatusReport {
            target: "feature".into(),
            branch: "feature".into(),
            worktree: Some("/tmp/sample-feature".into()),
            task_run: TaskRunStatusReport {
                id: Some("run-feature".into()),
                status: Some("running".into()),
                context: Some("direct".into()),
            },
            agent: AgentObservationReport {
                kind: "codex".into(),
                state: "running".into(),
                last_tool: Some("Bash".into()),
                last_event_at: Some("2026-05-16T00:00:10Z".into()),
                session_id: Some("codex-session".into()),
                needs_input_since: None,
                metadata: BTreeMap::new(),
            },
            cmux: CmuxObservationReport {
                state: "terminal_surface_ready".into(),
                workspace: Some("workspace:1".into()),
                surface: Some("surface:4".into()),
                workspace_id: Some("uuid-workspace-1".into()),
                surface_id: Some("uuid-surface-4".into()),
                pane: Some("pane:3".into()),
                window: Some("window:1".into()),
                workspace_title: Some("feature".into()),
                candidates: Vec::new(),
            },
            warnings: Vec::new(),
        }
    }
}
