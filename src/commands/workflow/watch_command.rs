use super::show_command::{
    WorkflowShowSnapshot, WorkflowShowTaskSnapshot, collect_workflow_snapshot,
    write_workflow_snapshot_json,
};
use crate::context::{Ctx, PromptItem, PromptRow};
use crate::error::WtError;
use crate::task_run::{STATUS_FAILED, STATUS_PASSED, STATUS_SKIPPED};
use crate::workflow as workflow_store;
use crate::workflow::render::{workflow_relative_path, workflow_title_label};
use anyhow::{Result, bail};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) fn run(
    ctx: &Ctx,
    workflow: Option<&str>,
    interval_secs: u64,
    timeout_secs: Option<u64>,
    heartbeat_secs: Option<u64>,
) -> Result<()> {
    watch_with_options(
        ctx,
        workflow,
        WatchOptions {
            interval: Duration::from_secs(interval_secs),
            timeout: timeout_secs.map(Duration::from_secs),
            heartbeat: heartbeat_secs.map(Duration::from_secs),
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WatchOptions {
    interval: Duration,
    timeout: Option<Duration>,
    heartbeat: Option<Duration>,
}

fn watch_with_options(ctx: &Ctx, workflow: Option<&str>, options: WatchOptions) -> Result<()> {
    let path = resolve_watch_target(ctx, workflow)?;
    let id = workflow_store::id_from_path(&path)?;
    let metadata = workflow_store::read(&path)?;
    let mut last_signature = None;
    let started_at = Instant::now();
    let mut last_output_at = started_at;

    if !ctx.is_json() {
        ctx.ui.print_step(&format!(
            "Workflow watch: {id} ({}, {} tasks) waiting for terminal",
            metadata.mode.as_str(),
            metadata.tasks.len()
        ));
    }

    loop {
        let snapshot = collect_workflow_snapshot(ctx, &path, &metadata)?;
        let signature = transition_signature(&snapshot);
        let changed = last_signature.as_ref() != Some(&signature);
        let now = Instant::now();
        let elapsed = now.duration_since(started_at);
        let verdict = WatchVerdict::from_snapshot(&snapshot);

        if changed {
            if !ctx.is_json() {
                print_transition(ctx, &snapshot, &verdict);
            }
            last_signature = Some(signature);
            last_output_at = now;
        }

        if verdict.all_terminal {
            if ctx.is_json() {
                write_workflow_snapshot_json(&snapshot)?;
            } else {
                print_done(ctx, &snapshot, &verdict);
            }
            return exit_result(verdict.exit_code());
        }

        if let Some(timeout) = options.timeout.filter(|timeout| elapsed >= *timeout) {
            if ctx.is_json() {
                write_workflow_snapshot_json(&snapshot)?;
            } else {
                print_timeout(ctx, &snapshot, &verdict, timeout, elapsed);
            }
            return Ok(());
        }

        if !changed
            && options
                .heartbeat
                .is_some_and(|heartbeat| now.duration_since(last_output_at) >= heartbeat)
        {
            if !ctx.is_json() {
                print_heartbeat(ctx, &snapshot, &verdict, elapsed);
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

fn resolve_watch_target(ctx: &Ctx, workflow: Option<&str>) -> Result<PathBuf> {
    match workflow {
        Some(workflow) => workflow_store::resolve(ctx, workflow),
        None if ctx.is_json() || ctx.quiet || !ctx.ui.can_prompt() => {
            bail!("{}", watch_target_required_message())
        }
        None => select_watch_workflow(ctx),
    }
}

fn watch_target_required_message() -> &'static str {
    "wt workflow watch requires WORKFLOW when it cannot open an interactive selector. Pass a workflow path or id; use `wt workflow show <workflow> --json` to observe once, `wt workflow watch <workflow>` to block until every workflow task is terminal, or `wt agent watch <target>` for one task agent's runtime state."
}

fn select_watch_workflow(ctx: &Ctx) -> Result<PathBuf> {
    let candidates = watch_workflow_candidates(ctx)?;
    if candidates.is_empty() {
        bail!(
            "No workflows found in {}",
            ctx.storage_root
                .display_path(&ctx.storage_root.workflows_dir())
        );
    }
    let rows = candidates
        .iter()
        .enumerate()
        .map(|(idx, candidate)| PromptRow::from_indexed_item(idx, candidate.item.clone()))
        .collect::<Vec<_>>();
    let idx = ctx.ui.select_rows("Workflow to watch", &rows)?;
    let candidate = candidates
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("Selected workflow index out of range: {idx}"))?;
    Ok(candidate.path.clone())
}

struct WatchWorkflowCandidate {
    path: PathBuf,
    item: PromptItem,
}

fn watch_workflow_candidates(ctx: &Ctx) -> Result<Vec<WatchWorkflowCandidate>> {
    let mut candidates = Vec::new();
    for path in workflow_store::workflow_paths(ctx)? {
        let id = workflow_store::id_from_path(&path)?;
        match workflow_store::read(&path) {
            Ok(metadata) => {
                let item = PromptItem::from_hint_parts(
                    workflow_title_label(ctx, &id, &metadata),
                    vec![
                        metadata.mode.as_str().to_string(),
                        workflow_relative_path(ctx, &path),
                    ],
                );
                candidates.push(WatchWorkflowCandidate { path, item });
            }
            Err(err) => ctx.ui.print_warning(&format!(
                "Skipping unreadable workflow {}: {}",
                workflow_relative_path(ctx, &path),
                first_error_line(&err)
            )),
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(candidates)
}

fn first_error_line(err: &anyhow::Error) -> String {
    format!("{err:#}")
        .lines()
        .next()
        .unwrap_or("unknown error")
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchVerdict {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    all_terminal: bool,
}

impl WatchVerdict {
    fn from_snapshot(snapshot: &WorkflowShowSnapshot) -> Self {
        let passed = status_count(snapshot, STATUS_PASSED.as_str());
        let failed = status_count(snapshot, STATUS_FAILED.as_str());
        let skipped = status_count(snapshot, STATUS_SKIPPED.as_str());
        let total = snapshot.tasks.len();
        let all_terminal = snapshot
            .tasks
            .iter()
            .all(|task| is_terminal_status(&task.status));
        Self {
            total,
            passed,
            failed,
            skipped,
            all_terminal,
        }
    }

    fn terminal_count(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    fn exit_code(&self) -> i32 {
        if self.failed > 0 { 3 } else { 0 }
    }
}

fn status_count(snapshot: &WorkflowShowSnapshot, status: &str) -> usize {
    snapshot
        .tasks
        .iter()
        .filter(|task| task.status == status)
        .count()
}

fn is_terminal_status(status: &str) -> bool {
    matches!(status, "passed" | "failed" | "skipped")
}

fn transition_signature(snapshot: &WorkflowShowSnapshot) -> String {
    snapshot
        .tasks
        .iter()
        .map(task_signature)
        .collect::<Vec<_>>()
        .join("|")
}

fn task_signature(task: &WorkflowShowTaskSnapshot) -> String {
    format!("{}={}", task.task, task.status)
}

fn print_transition(ctx: &Ctx, snapshot: &WorkflowShowSnapshot, verdict: &WatchVerdict) {
    ctx.ui.print_step(&format!(
        "Workflow watch: {}; {}",
        terminal_summary(verdict),
        task_status_summary(snapshot)
    ));
}

fn print_heartbeat(
    ctx: &Ctx,
    snapshot: &WorkflowShowSnapshot,
    verdict: &WatchVerdict,
    elapsed: Duration,
) {
    ctx.ui.print_step(&format!(
        "Workflow watch heartbeat: elapsed {}; {}; {}",
        format_duration(elapsed),
        terminal_summary(verdict),
        task_status_summary(snapshot)
    ));
}

fn print_timeout(
    ctx: &Ctx,
    snapshot: &WorkflowShowSnapshot,
    verdict: &WatchVerdict,
    timeout: Duration,
    elapsed: Duration,
) {
    ctx.ui.print_step(&format!(
        "Workflow watch timeout after {}: {}; {}; elapsed {}",
        format_duration(timeout),
        terminal_summary(verdict),
        non_terminal_summary(snapshot),
        format_duration(elapsed)
    ));
}

fn print_done(ctx: &Ctx, snapshot: &WorkflowShowSnapshot, verdict: &WatchVerdict) {
    if verdict.failed > 0 {
        ctx.ui.print_step(&format!(
            "[done] terminal with failure: {} ({})",
            failed_task_summary(snapshot),
            status_count_summary(verdict)
        ));
    } else if verdict.skipped > 0 {
        ctx.ui.print_step(&format!(
            "[done] all passed/skipped ({}/{})",
            verdict.terminal_count(),
            verdict.total
        ));
    } else {
        ctx.ui.print_step(&format!(
            "[done] all passed ({}/{})",
            verdict.passed, verdict.total
        ));
    }
}

fn task_status_summary(snapshot: &WorkflowShowSnapshot) -> String {
    if snapshot.tasks.is_empty() {
        return "no tasks".into();
    }
    snapshot
        .tasks
        .iter()
        .map(task_signature)
        .collect::<Vec<_>>()
        .join(", ")
}

fn non_terminal_summary(snapshot: &WorkflowShowSnapshot) -> String {
    let summary = snapshot
        .tasks
        .iter()
        .filter(|task| !is_terminal_status(&task.status))
        .map(task_signature)
        .collect::<Vec<_>>()
        .join(", ");
    if summary.is_empty() {
        "no non-terminal tasks".into()
    } else {
        summary
    }
}

fn failed_task_summary(snapshot: &WorkflowShowSnapshot) -> String {
    snapshot
        .tasks
        .iter()
        .filter(|task| task.status == STATUS_FAILED.as_str())
        .map(task_signature)
        .collect::<Vec<_>>()
        .join(", ")
}

fn terminal_summary(verdict: &WatchVerdict) -> String {
    format!("{}/{} terminal", verdict.terminal_count(), verdict.total)
}

fn status_count_summary(verdict: &WatchVerdict) -> String {
    let mut parts = Vec::new();
    if verdict.passed > 0 {
        parts.push(format!("{} passed", verdict.passed));
    }
    if verdict.failed > 0 {
        parts.push(format!("{} failed", verdict.failed));
    }
    if verdict.skipped > 0 {
        parts.push(format!("{} skipped", verdict.skipped));
    }
    if parts.is_empty() {
        "0 tasks".into()
    } else {
        parts.join(", ")
    }
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

fn exit_result(exit_code: i32) -> Result<()> {
    if exit_code == 0 {
        Ok(())
    } else {
        Err(WtError::Exit { code: exit_code }.into())
    }
}

fn format_duration(duration: Duration) -> String {
    format!("{}s", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_ui(root: &Path, ui: MockUi) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        )
    }

    fn write_task_document(root: &Path, key: &str, branch: &str) {
        let dir = root.join(".wt/execution/tasks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{key}.toml")),
            format!("title = \"{key}\"\nbranch = \"{branch}\"\nbody = \"Task body\"\n"),
        )
        .unwrap();
    }

    fn write_task_run(root: &Path, id: &str, task: &str, branch: &str, status: &str, group: &str) {
        let dir = root.join(".wt/execution/task-runs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{id}.toml")),
            format!(
                "task = \"{task}\"\n\
                 branch = \"{branch}\"\n\
                 status = \"{status}\"\n\
                 group = \"{group}\"\n\
                 creation_order = 1\n\
                 created_at = \"2026-05-18T00:00:00.000000000Z\"\n\
                 updated_at = \"2026-05-18T00:00:00.000000000Z\"\n"
            ),
        )
        .unwrap();
    }

    fn write_workflow(root: &Path, id: &str, tasks: &str) {
        let dir = root.join(".wt/execution/workflows");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{id}.toml")),
            format!(
                "mode = \"stack\"\n\
                 base_mode = \"explicit\"\n\
                 base = \"main\"\n\
                 created_at = \"2026-05-18T00:00:00Z\"\n\
                 updated_at = \"2026-05-18T00:00:00Z\"\n\n\
                 [policy]\n\
                 pull_request = \"none\"\n\
                 landing = \"manual\"\n\n\
                 {tasks}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn all_passed_exits_zero() {
        let temp = TempDir::new().unwrap();
        write_task_document(temp.path(), "api", "feature/api");
        write_task_run(
            temp.path(),
            "run-api",
            "api",
            "feature/api",
            "passed",
            "2026-05-18-010",
        );
        write_workflow(
            temp.path(),
            "2026-05-18-010",
            "[[tasks]]\ntask = \"api\"\nrun = \"run-api\"\n",
        );

        watch_with_options(
            &ctx(temp.path()),
            Some("2026-05-18-010"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: None,
                heartbeat: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn failed_task_exits_three() {
        let temp = TempDir::new().unwrap();
        write_task_document(temp.path(), "api", "feature/api");
        write_task_run(
            temp.path(),
            "run-api",
            "api",
            "feature/api",
            "failed",
            "2026-05-18-010",
        );
        write_workflow(
            temp.path(),
            "2026-05-18-010",
            "[[tasks]]\ntask = \"api\"\nrun = \"run-api\"\n",
        );

        let err = watch_with_options(
            &ctx(temp.path()),
            Some("2026-05-18-010"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: None,
                heartbeat: None,
            },
        )
        .unwrap_err();

        assert!(matches!(
            err.downcast_ref::<WtError>(),
            Some(WtError::Exit { code: 3 })
        ));
    }

    #[test]
    fn timeout_while_non_terminal_exits_zero_without_wait_observations() {
        let temp = TempDir::new().unwrap();
        write_task_document(temp.path(), "api", "feature/api");
        write_task_run(
            temp.path(),
            "run-api",
            "api",
            "feature/api",
            "running",
            "2026-05-18-010",
        );
        write_workflow(
            temp.path(),
            "2026-05-18-010",
            "[[tasks]]\ntask = \"api\"\nrun = \"run-api\"\n",
        );

        watch_with_options(
            &ctx(temp.path()),
            Some("2026-05-18-010"),
            WatchOptions {
                interval: Duration::ZERO,
                timeout: Some(Duration::ZERO),
                heartbeat: None,
            },
        )
        .unwrap();

        assert!(!temp.path().join(".wt/runtime/agents").exists());
    }

    #[test]
    fn omitted_target_requires_selector_capable_context() {
        let temp = TempDir::new().unwrap();
        let mut ui = MockUi::new();
        ui.set_prompt_available(false);
        let err = watch_with_options(
            &ctx_with_ui(temp.path(), ui),
            None,
            WatchOptions {
                interval: Duration::ZERO,
                timeout: Some(Duration::ZERO),
                heartbeat: None,
            },
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("wt workflow watch requires WORKFLOW"));
        assert!(format!("{err:#}").contains("wt workflow watch <workflow>"));
        assert!(format!("{err:#}").contains("wt agent watch <target>"));
    }

    #[test]
    fn omitted_target_uses_interactive_selector_when_available() {
        let temp = TempDir::new().unwrap();
        write_task_document(temp.path(), "api", "feature/api");
        write_task_run(
            temp.path(),
            "run-api",
            "api",
            "feature/api",
            "passed",
            "2026-05-18-010",
        );
        write_workflow(
            temp.path(),
            "2026-05-18-010",
            "[[tasks]]\ntask = \"api\"\nrun = \"run-api\"\n",
        );
        let mut ui = MockUi::new();
        ui.add_select(0);
        let ctx = ctx_with_ui(temp.path(), ui);

        watch_with_options(
            &ctx,
            None,
            WatchOptions {
                interval: Duration::ZERO,
                timeout: None,
                heartbeat: None,
            },
        )
        .unwrap();

        assert!(ctx.ui.can_prompt());
    }
}
