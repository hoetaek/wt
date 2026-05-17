use crate::agents::AgentStatus;
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::work::{self, WorkSessionState};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;

pub fn run(ctx: &Ctx, target: &str) -> Result<()> {
    let (report, exit_code) = observe_status(ctx, target)?;
    if ctx.is_json() {
        print_json(&report)?;
    } else {
        print_text(ctx, &report);
    }

    if exit_code == 0 {
        Ok(())
    } else {
        Err(WtError::Exit { code: exit_code }.into())
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct StatusReport {
    target: String,
    branch: String,
    worktree_path: Option<String>,
    cmux_workspace_id: Option<String>,
    cmux_workspace_ref: Option<String>,
    cmux_workspace_title: Option<String>,
    cmux_window_id: Option<String>,
    cmux_window_ref: Option<String>,
    cmux_pane_id: Option<String>,
    cmux_pane_ref: Option<String>,
    cmux_surface_id: Option<String>,
    cmux_surface_ref: Option<String>,
    agent: String,
    status: String,
    last_tool: Option<String>,
    last_event_at: Option<String>,
    session_id: Option<String>,
    needs_input_since: Option<String>,
    meta: BTreeMap<String, String>,
    warnings: Vec<String>,
}

fn observe_status(ctx: &Ctx, target: &str) -> Result<(StatusReport, i32)> {
    let work = work::observe_work(ctx, Some(target))?;
    let exit_code = exit_code_for(&work);
    Ok((StatusReport::from_work(&work), exit_code))
}

impl StatusReport {
    fn from_work(work: &work::Work) -> Self {
        let mut meta = work.state.metadata.clone();
        meta.insert(
            "session_state".into(),
            work.session_state.as_str().to_string(),
        );
        if let Some(record) = work.target.task_run.as_ref() {
            meta.insert("task_run_id".into(), record.id.clone());
            meta.insert("task_run_status".into(), record.run.status.to_string());
            meta.insert("task_run_source".into(), record.run.source.to_string());
        }

        let cmux = work.cmux.as_ref();
        Self {
            target: work.target.label.clone(),
            branch: work.target.branch.clone(),
            worktree_path: work
                .target
                .worktree
                .as_ref()
                .map(|path| path.display().to_string()),
            cmux_workspace_id: cmux.map(|cmux| cmux.workspace_id.clone()),
            cmux_workspace_ref: cmux.map(|cmux| cmux.workspace_ref.clone()),
            cmux_workspace_title: cmux.map(|cmux| cmux.workspace_title.clone()),
            cmux_window_id: cmux.map(|cmux| cmux.window_id.clone()),
            cmux_window_ref: cmux.map(|cmux| cmux.window_ref.clone()),
            cmux_pane_id: cmux.and_then(|cmux| cmux.pane_id.clone()),
            cmux_pane_ref: cmux.and_then(|cmux| cmux.pane_ref.clone()),
            cmux_surface_id: cmux.and_then(|cmux| cmux.surface_id.clone()),
            cmux_surface_ref: cmux.and_then(|cmux| cmux.surface_ref.clone()),
            agent: work.state.agent_kind.as_str().to_string(),
            status: work.state.status.as_str().to_string(),
            last_tool: work.state.last_tool.clone(),
            last_event_at: work.state.last_event_at.clone(),
            session_id: work.state.session_id.clone(),
            needs_input_since: work.state.needs_input_since.clone(),
            meta,
            warnings: warnings_for_work(work),
        }
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
    if work.session_state == WorkSessionState::CmuxUnavailable {
        return 1;
    }

    match work.state.status {
        AgentStatus::NeedsInput => 2,
        AgentStatus::Failed => 3,
        _ => 0,
    }
}

fn print_json(report: &StatusReport) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

fn print_text(ctx: &Ctx, report: &StatusReport) {
    ctx.ui.print_step(&format!("Status: {}", report.target));
    ctx.ui.print_dim(&format!("  Branch: {}", report.branch));
    match report.worktree_path.as_deref() {
        Some(path) => ctx.ui.print_dim(&format!("  Worktree: {path}")),
        None => ctx
            .ui
            .print_dim("  Worktree: branch is not checked out in a local worktree"),
    }
    ctx.ui
        .print_dim(&format!("  Agent: {} ({})", report.agent, report.status));
    if let Some(last_tool) = report.last_tool.as_deref() {
        ctx.ui.print_dim(&format!("  Last tool: {last_tool}"));
    }
    if let Some(last_event_at) = report.last_event_at.as_deref() {
        ctx.ui.print_dim(&format!("  Last event: {last_event_at}"));
    }
    if let Some(session_id) = report.session_id.as_deref() {
        ctx.ui.print_dim(&format!("  Session: {session_id}"));
    }
    if let Some(needs_input_since) = report.needs_input_since.as_deref() {
        ctx.ui
            .print_dim(&format!("  Needs input since: {needs_input_since}"));
    }
    print_cmux_text(ctx, report);
    for warning in &report.warnings {
        ctx.ui.print_warning(warning);
    }
}

fn print_cmux_text(ctx: &Ctx, report: &StatusReport) {
    match (
        report.cmux_workspace_ref.as_deref(),
        report.cmux_surface_ref.as_deref(),
    ) {
        (Some(workspace), Some(surface)) => {
            let pane = report.cmux_pane_ref.as_deref().unwrap_or("unknown-pane");
            let window = report
                .cmux_window_ref
                .as_deref()
                .unwrap_or("unknown-window");
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
            let state = report
                .meta
                .get("session_state")
                .map(String::as_str)
                .unwrap_or("unknown");
            ctx.ui.print_dim(&format!("  cmux: {state}"));
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.branch, "feature");
        assert_eq!(
            report.worktree_path.as_deref(),
            Some(fixture.worktree.to_str().unwrap())
        );
        assert_eq!(
            report.cmux_workspace_id.as_deref(),
            Some("uuid-workspace-1")
        );
        assert_eq!(report.cmux_workspace_ref.as_deref(), Some("workspace:1"));
        assert_eq!(report.cmux_surface_id.as_deref(), Some("uuid-surface-4"));
        assert_eq!(report.cmux_surface_ref.as_deref(), Some("surface:4"));
        assert_eq!(report.agent, "codex");
        assert_eq!(report.status, "idle");
        assert_eq!(report.last_tool.as_deref(), Some("Bash"));
        assert_eq!(report.session_id.as_deref(), Some("codex-session"));
        assert_eq!(report.warnings, Vec::<String>::new());

        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["branch"], "feature");
        assert_eq!(value["cmux_workspace_ref"], "workspace:1");
        assert_eq!(value["cmux_surface_ref"], "surface:4");
        assert_eq!(value["meta"]["session_state"], "terminal_surface_ready");
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

        run(&ctx, "feature").unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let dims = ui.dims.lock().unwrap().join("\n");
        assert!(steps.contains("Status: feature"));
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 2);
        assert_eq!(report.status, "needs_input");
        assert_eq!(
            report.needs_input_since.as_deref(),
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 3);
        assert_eq!(report.status, "failed");
    }

    #[test]
    fn missing_target_returns_not_found_error() {
        let fixture = Fixture::new();
        let mut runner = MockRunner::new();
        add_worktree_list(&mut runner, &fixture);
        runner.add_response("", false);
        let ctx = fixture.ctx(runner, OutputMode::Json);

        let err = observe_status(&ctx, "missing").unwrap_err().to_string();

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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.status, "no_session");
        assert_eq!(report.worktree_path, None);
        assert_eq!(report.meta["session_state"], "no_local_worktree");
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 1);
        assert_eq!(report.status, "no_session");
        assert_eq!(report.meta["session_state"], "cmux_unavailable");
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.status, "no_session");
        assert_eq!(report.meta["session_state"], "no_cmux_workspace");
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.status, "no_session");
        assert_eq!(report.cmux_workspace_ref.as_deref(), Some("workspace:1"));
        assert_eq!(report.cmux_surface_ref.as_deref(), Some("surface:4"));
        assert_eq!(report.meta["session_state"], "no_terminal_surface");
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

        let (report, exit_code) = observe_status(&ctx, "feature").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.agent, "claude_code");
        assert_eq!(report.status, "running");
        assert_eq!(report.last_tool.as_deref(), Some("Read"));
        assert_eq!(report.session_id.as_deref(), Some("claude-session"));
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

        let (report, exit_code) = observe_status(&ctx, "run-feature").unwrap();

        assert_eq!(exit_code, 0);
        assert_eq!(report.target, "run-feature");
        assert_eq!(report.branch, "feature");
        assert_eq!(report.meta["task_run_id"], "run-feature");
        assert_eq!(report.meta["task_run_status"], "running");
        assert_eq!(report.meta["task_run_source"], "stack");
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
}
