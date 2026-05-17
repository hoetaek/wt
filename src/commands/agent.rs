use crate::agents::AgentStatus;
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::work::{self, WorkSessionState};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

pub fn status(ctx: &Ctx, target: Option<&str>) -> Result<()> {
    let (report, exit_code) = observe_status(ctx, target, "status")?;
    if ctx.is_json() {
        print_json(&report)?;
    } else {
        print_text(ctx, &report);
    }

    exit_result(exit_code)
}

pub fn watch(ctx: &Ctx, target: Option<&str>, interval_secs: u64) -> Result<()> {
    let interval = if interval_secs == 0 {
        Duration::ZERO
    } else {
        Duration::from_secs(interval_secs)
    };
    watch_with_interval(ctx, target, interval)
}

fn watch_with_interval(ctx: &Ctx, target: Option<&str>, interval: Duration) -> Result<()> {
    let target = resolve_agent_target(ctx, target, "watch")?;
    let mut last_signature = None;

    loop {
        let work = work::observe_target(ctx, target.clone())?;
        let report = AgentStatusReport::from_work(&work);
        let exit_code = exit_code_for(&work);
        let signature = report.transition_signature();

        if last_signature.as_ref() != Some(&signature) {
            if ctx.is_json() {
                print_json_line(&report)?;
            } else {
                print_watch_transition(ctx, &report);
            }
            last_signature = Some(signature);
        }

        if should_stop_watching(&work, exit_code) {
            return exit_result(exit_code);
        }

        if interval > Duration::ZERO {
            std::thread::sleep(interval);
        }
    }
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
    source: Option<String>,
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
}

fn observe_status(
    ctx: &Ctx,
    target: Option<&str>,
    command: &str,
) -> Result<(AgentStatusReport, i32)> {
    let target = resolve_agent_target(ctx, target, command)?;
    let work = work::observe_target(ctx, target)?;
    let exit_code = exit_code_for(&work);
    Ok((AgentStatusReport::from_work(&work), exit_code))
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
    fn from_work(work: &work::Work) -> Self {
        let cmux = work.cmux.as_ref();
        let task_run = work.target.task_run.as_ref();
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
                source: task_run.map(|record| record.run.source.to_string()),
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
            },
            warnings: warnings_for_work(work),
        }
    }

    fn transition_signature(&self) -> String {
        format!(
            "{}:{}:{}:{}:{:?}:{:?}:{:?}:{:?}:{:?}",
            self.target,
            self.agent.kind,
            self.agent.state,
            self.cmux.state,
            self.cmux.workspace,
            self.cmux.surface,
            self.agent.last_tool,
            self.agent.last_event_at,
            self.warnings
        )
    }
}

fn warnings_for_work(work: &work::Work) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Some(warning) = work.state.warning.as_ref() {
        warnings.push(warning.clone());
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
        | WorkSessionState::NoTerminalSurface => return 1,
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

fn print_json(report: &AgentStatusReport) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

fn print_json_line(report: &AgentStatusReport) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
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
    for warning in &report.warnings {
        ctx.ui.print_warning(warning);
    }
}

fn print_task_run_text(ctx: &Ctx, report: &AgentStatusReport) {
    match (
        report.task_run.id.as_deref(),
        report.task_run.status.as_deref(),
        report.task_run.source.as_deref(),
    ) {
        (Some(id), Some(status), Some(source)) => ctx.ui.print_dim(&format!(
            "  TaskRun: {id} (status={status}, source={source})"
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
    ctx.ui.print_dim(&format!("  Branch: {}", report.branch));
    print_task_run_text(ctx, report);
    if let Some(last_event_at) = report.agent.last_event_at.as_deref() {
        ctx.ui.print_dim(&format!("  Last event: {last_event_at}"));
    }
    print_cmux_text(ctx, report);
    for warning in &report.warnings {
        ctx.ui.print_warning(warning);
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
        assert_eq!(report.cmux.surface.as_deref(), Some("surface:4"));
        assert_eq!(report.cmux.state, "no_terminal_surface");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("terminal surface is not ready"))
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
        runner.add_response("Claude Code\nWorking", true);
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
        std::fs::create_dir_all(fixture.repo.join(".local/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nsource = \"stack\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        add_matching_workspace(&mut runner, &fixture);
        add_selected_surface(&mut runner);
        runner.add_response("Ready", true);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let (report, exit_code) = observe_status(&ctx, Some("run-feature"), "status").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.target, "run-feature");
        assert_eq!(report.branch, "feature");
        assert_eq!(report.task_run.id.as_deref(), Some("run-feature"));
        assert_eq!(report.task_run.status.as_deref(), Some("running"));
        assert_eq!(report.task_run.source.as_deref(), Some("stack"));
    }

    #[test]
    fn status_without_target_selects_interactive_work_target() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.repo.join(".local/task-runs")).unwrap();
        std::fs::write(
            fixture.repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\nsource = \"stack\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
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
                    output_mode,
                    verbosity: 0,
                    quiet: false,
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
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
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
}
