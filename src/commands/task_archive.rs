use crate::context::Ctx;
use crate::task;
use crate::task_run::{self, TaskRunRecord};
use crate::workflow::{self as workflow_store, WorkflowMetadata};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn archive(ctx: &Ctx, keys: &[String]) -> Result<()> {
    let plan = plan_archive(ctx, keys)?;
    if plan.candidates.is_empty() {
        let report = ArchiveReport::from_plan(Vec::new(), &plan, false);
        write_report(ctx, &report)?;
        return finish(report);
    }

    if should_confirm(ctx)
        && !ctx.ui.confirm(
            &format!(
                "Archive {} task(s)? moves to archive/tasks",
                plan.candidates.len()
            ),
            false,
        )?
    {
        let report = ArchiveReport::from_plan(Vec::new(), &plan, true);
        write_report(ctx, &report)?;
        return finish(report);
    }

    let mut archived = Vec::new();
    for candidate in &plan.candidates {
        archived.push(move_task_to_archive(ctx, candidate)?);
    }

    let report = ArchiveReport::from_plan(archived, &plan, false);
    write_report(ctx, &report)?;
    finish(report)
}

#[derive(Debug)]
struct ArchivePlan {
    candidates: Vec<ArchiveCandidate>,
    rejected: Vec<RejectedTask>,
    not_found: Vec<String>,
}

#[derive(Debug)]
struct ArchiveCandidate {
    key: String,
    source_path: PathBuf,
    archive_dir: PathBuf,
    archive_path: PathBuf,
    manifest_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct ArchivedTask {
    key: String,
    source_path: String,
    archive_path: String,
    manifest_path: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct RejectedTask {
    key: String,
    reason: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ArchiveReport {
    archived: Vec<ArchivedTask>,
    rejected: Vec<RejectedTask>,
    not_found: Vec<String>,
    aborted: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ArchiveManifest {
    task_key: String,
    archived_at: u64,
    wt_version: String,
    source_path: String,
    archive_path: String,
}

fn plan_archive(ctx: &Ctx, keys: &[String]) -> Result<ArchivePlan> {
    if keys.is_empty() {
        bail!("wt task archive requires at least one task key");
    }
    if let Some(legacy) = ctx.storage_root.detect_legacy_archive(&ctx.repo_root) {
        bail!("{}", legacy.error_message_for("Task archive storage"));
    }
    task::ensure_task_document_store_available(&ctx.storage_root, &ctx.repo_root)?;

    let inventory = task_run::list_lossy(ctx)?;
    let workflow_refs = active_workflow_task_refs(ctx)?;
    let mut candidates = Vec::new();
    let mut rejected = Vec::new();
    let mut not_found = Vec::new();
    let mut seen = BTreeSet::new();

    for raw_key in keys {
        let key = task::safe_task_key(raw_key);
        if !seen.insert(key.clone()) {
            continue;
        }

        let source_path = ctx.storage_root.tasks_dir().join(format!("{key}.toml"));
        if !source_path.exists() {
            not_found.push(key);
            continue;
        }

        if let Some(workflows) = workflow_refs.get(&key) {
            rejected.push(RejectedTask {
                key,
                reason: format!("referenced by active workflow {}", workflows.join(", ")),
            });
            continue;
        }

        if let Some(record) = latest_valid_task_run_for(&inventory.records, &key)
            && record.run.status == task_run::STATUS_RUNNING
        {
            rejected.push(RejectedTask {
                key,
                reason: format!("latest TaskRun {} is running", record.id),
            });
            continue;
        }

        let archive_dir = ctx.storage_root.task_archive_dir(&key);
        let archive_path = archive_dir.join(format!("{key}.toml"));
        let manifest_path = archive_dir.join("archive.toml");
        if archive_path.exists() || manifest_path.exists() {
            rejected.push(RejectedTask {
                key,
                reason: "archive already exists".to_string(),
            });
            continue;
        }

        candidates.push(ArchiveCandidate {
            key,
            source_path,
            archive_dir,
            archive_path,
            manifest_path,
        });
    }

    Ok(ArchivePlan {
        candidates,
        rejected,
        not_found,
    })
}

fn latest_valid_task_run_for<'a>(
    records: &'a [TaskRunRecord],
    key: &str,
) -> Option<&'a TaskRunRecord> {
    records
        .iter()
        .filter(|record| record.run.task == key)
        .max_by(|left, right| task_run::compare_task_run_records(left, right))
}

fn active_workflow_task_refs(ctx: &Ctx) -> Result<BTreeMap<String, Vec<String>>> {
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in workflow_store::workflow_paths(ctx)? {
        let workflow_id = workflow_store::id_from_path(&path)?;
        let workflow = workflow_store::read(&path).with_context(|| {
            format!(
                "Cannot determine active Workflow TaskDocument references because workflow {} could not be read",
                ctx.storage_root.display_path(&path)
            )
        })?;
        for key in workflow_task_keys(&workflow) {
            refs.entry(key).or_default().push(workflow_id.clone());
        }
    }
    Ok(refs)
}

fn workflow_task_keys(metadata: &WorkflowMetadata) -> Vec<String> {
    metadata
        .tasks
        .iter()
        .map(|row| task::safe_task_key(&row.task))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn should_confirm(ctx: &Ctx) -> bool {
    !ctx.is_json() && !ctx.quiet && ctx.ui.can_prompt()
}

fn move_task_to_archive(ctx: &Ctx, candidate: &ArchiveCandidate) -> Result<ArchivedTask> {
    fs::create_dir_all(&candidate.archive_dir).with_context(|| {
        format!(
            "Failed to create task archive directory: {}",
            ctx.storage_root.display_path(&candidate.archive_dir)
        )
    })?;

    fs::copy(&candidate.source_path, &candidate.archive_path).with_context(|| {
        format!(
            "Failed to copy TaskDocument into archive: {} -> {}",
            ctx.storage_root.display_path(&candidate.source_path),
            ctx.storage_root.display_path(&candidate.archive_path)
        )
    })?;

    let manifest = ArchiveManifest {
        task_key: candidate.key.clone(),
        archived_at: current_epoch_seconds(),
        wt_version: env!("CARGO_PKG_VERSION").to_string(),
        source_path: wt_relative_path(ctx, &candidate.source_path),
        archive_path: wt_relative_path(ctx, &candidate.archive_path),
    };
    let manifest_content = toml::to_string_pretty(&manifest)?;
    if let Err(err) = fs::write(&candidate.manifest_path, manifest_content).with_context(|| {
        format!(
            "Failed to write task archive manifest: {}",
            ctx.storage_root.display_path(&candidate.manifest_path)
        )
    }) {
        let _ = fs::remove_file(&candidate.archive_path);
        return Err(err);
    }

    remove_file_if_present(&candidate.source_path).with_context(|| {
        format!(
            "Failed to remove archived TaskDocument from active storage: {}",
            ctx.storage_root.display_path(&candidate.source_path)
        )
    })?;

    Ok(ArchivedTask {
        key: candidate.key.clone(),
        source_path: manifest.source_path,
        archive_path: manifest.archive_path,
        manifest_path: wt_relative_path(ctx, &candidate.manifest_path),
    })
}

fn remove_file_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn wt_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(ctx.storage_root.personal_root())
        .map(|relative| relative.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ctx.storage_root.display_path(path))
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

impl ArchiveReport {
    fn from_plan(archived: Vec<ArchivedTask>, plan: &ArchivePlan, aborted: bool) -> Self {
        Self {
            archived,
            rejected: plan.rejected.clone(),
            not_found: plan.not_found.clone(),
            aborted,
        }
    }

    fn has_failures(&self) -> bool {
        !self.rejected.is_empty() || !self.not_found.is_empty()
    }
}

fn write_report(ctx: &Ctx, report: &ArchiveReport) -> Result<()> {
    if ctx.is_json() {
        write_json(report)
    } else {
        print_text(ctx, report);
        Ok(())
    }
}

fn write_json(report: &ArchiveReport) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

fn print_text(ctx: &Ctx, report: &ArchiveReport) {
    if report.aborted {
        ctx.ui.print_warning("Skipped task archive");
        return;
    }

    let archived = report.archived.len();
    let rejected = report.rejected.len();
    let missing = report.not_found.len();
    ctx.ui.print_step(&format!(
        "Task archive summary: {archived} archived, {rejected} rejected, {missing} not found"
    ));
    for task in &report.archived {
        ctx.ui
            .print_dim(&format!("  archived {} -> {}", task.key, task.archive_path));
    }
    for task in &report.rejected {
        ctx.ui
            .print_warning(&format!("Rejected {}: {}", task.key, task.reason));
    }
    if !report.not_found.is_empty() {
        ctx.ui
            .print_warning(&format!("Not found: {}", report.not_found.join(", ")));
    }
}

fn finish(report: ArchiveReport) -> Result<()> {
    if report.has_failures() {
        let mut parts = Vec::new();
        if !report.rejected.is_empty() {
            let rejected = report
                .rejected
                .iter()
                .map(|task| format!("{} ({})", task.key, task.reason))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("rejected: {rejected}"));
        }
        if !report.not_found.is_empty() {
            parts.push(format!("not found: {}", report.not_found.join(", ")));
        }
        bail!("Task archive completed with errors: {}", parts.join("; "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ArchiveManifest, archive};
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::task_run;
    use crate::workflow::{self as workflow_store, WorkflowMetadata, WorkflowMode, WorkflowTask};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    fn test_ctx(root: &Path) -> (Ctx, Arc<MockUi>) {
        test_ctx_with_ui(root, Arc::new(MockUi::new()))
    }

    fn test_ctx_with_confirm(root: &Path, confirmed: bool) -> (Ctx, Arc<MockUi>) {
        let mut ui = MockUi::new();
        ui.add_confirm(confirmed);
        test_ctx_with_ui(root, Arc::new(ui))
    }

    fn test_ctx_without_prompt(root: &Path) -> (Ctx, Arc<MockUi>) {
        let mut ui = MockUi::new();
        ui.set_prompt_available(false);
        test_ctx_with_ui(root, Arc::new(ui))
    }

    fn test_ctx_with_ui(root: &Path, ui: Arc<MockUi>) -> (Ctx, Arc<MockUi>) {
        (
            Ctx::new(
                root.to_path_buf(),
                root.to_path_buf(),
                Config::default(),
                Box::new(MockRunner::new()),
                Box::new(ui.clone()),
            ),
            ui,
        )
    }

    fn write_task(root: &Path, key: &str) {
        let tasks_dir = root.join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        fs::write(
            tasks_dir.join(format!("{key}.toml")),
            format!(
                r#"title = "{key}"
branch = "{key}"
body = "Task body"
"#
            ),
        )
        .unwrap();
    }

    fn write_workflow(root: &Path, id: &str, tasks: Vec<WorkflowTask>) {
        let (ctx, _) = test_ctx(root);
        let path = root
            .join(".wt/execution/workflows")
            .join(format!("{id}.toml"));
        let mut workflow =
            WorkflowMetadata::new(WorkflowMode::Batch, "explicit", Some("main".into()), tasks);
        workflow_store::write(&ctx, &path, &mut workflow).unwrap();
    }

    #[test]
    fn archive_moves_task_and_hides_from_list() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "demo");

        archive(&ctx, &["demo".to_string()]).unwrap();

        assert!(!ctx.storage_root.tasks_dir().join("demo.toml").exists());
        assert!(
            ctx.storage_root
                .task_archive_dir("demo")
                .join("demo.toml")
                .exists()
        );
        let manifest: ArchiveManifest = toml::from_str(
            &fs::read_to_string(
                ctx.storage_root
                    .task_archive_dir("demo")
                    .join("archive.toml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.task_key, "demo");
        assert_eq!(manifest.source_path, "execution/tasks/demo.toml");
        assert_eq!(
            manifest.archive_path,
            "execution/archive/tasks/demo/demo.toml"
        );
        assert!(!manifest.wt_version.is_empty());

        let (list_ctx, list_ui) = test_ctx(dir.path());
        crate::commands::task_list::run(&list_ctx, true).unwrap();
        let output = list_ui.steps.lock().unwrap().join("\n");
        assert!(!output.contains("demo"));
    }

    #[test]
    fn archive_rejects_task_with_running_taskrun() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx(dir.path());
        write_task(dir.path(), "busy");
        task_run::create(&ctx, "busy", "busy", None, task_run::STATUS_RUNNING).unwrap();

        let err = archive(&ctx, &["busy".to_string()]).unwrap_err();

        assert!(format!("{err:#}").contains("running"));
        assert!(ctx.storage_root.tasks_dir().join("busy.toml").exists());
    }

    #[test]
    fn archive_rejects_task_referenced_by_active_workflow() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx(dir.path());
        write_task(dir.path(), "workflow-task");
        let run = task_run::create(
            &ctx,
            "workflow-task",
            "workflow-task",
            Some("active-wf"),
            task_run::STATUS_PREPARED,
        )
        .unwrap();
        write_workflow(
            dir.path(),
            "active-wf",
            vec![WorkflowTask::new("workflow-task", run.id)],
        );

        let err = archive(&ctx, &["workflow-task".to_string()]).unwrap_err();

        let report = format!("{err:#}");
        assert!(report.contains("referenced by active workflow active-wf"));
        assert!(
            ctx.storage_root
                .tasks_dir()
                .join("workflow-task.toml")
                .exists()
        );
        assert!(
            !ctx.storage_root
                .task_archive_dir("workflow-task")
                .join("workflow-task.toml")
                .exists()
        );
    }

    #[test]
    fn archive_rejects_legacy_archive_storage() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "demo");
        fs::create_dir_all(dir.path().join(".wt/archive")).unwrap();

        let err = archive(&ctx, &["demo".to_string()]).unwrap_err();

        let report = format!("{err:#}");
        assert!(report.contains("legacy"));
        assert!(report.contains("archive"));
        assert!(ctx.storage_root.tasks_dir().join("demo.toml").exists());
    }

    #[test]
    fn archive_allows_failed_prepared_skipped_passed_and_no_run() {
        for (idx, status) in [
            None,
            Some(task_run::STATUS_PREPARED),
            Some(task_run::STATUS_FAILED),
            Some(task_run::STATUS_SKIPPED),
            Some(task_run::STATUS_PASSED),
        ]
        .into_iter()
        .enumerate()
        {
            let dir = tempfile::tempdir().unwrap();
            let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
            let key = format!("task-{idx}");
            write_task(dir.path(), &key);
            if let Some(status) = status {
                task_run::create(&ctx, &key, &key, None, status).unwrap();
            }

            archive(&ctx, std::slice::from_ref(&key)).unwrap();

            assert!(
                ctx.storage_root
                    .task_archive_dir(&key)
                    .join(format!("{key}.toml"))
                    .exists()
            );
        }
    }

    #[test]
    fn archive_uses_latest_valid_taskrun_status() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "demo");
        task_run::create(&ctx, "demo", "demo", None, task_run::STATUS_RUNNING).unwrap();
        task_run::create(&ctx, "demo", "demo", None, task_run::STATUS_PASSED).unwrap();

        archive(&ctx, &["demo".to_string()]).unwrap();

        assert!(
            ctx.storage_root
                .task_archive_dir("demo")
                .join("demo.toml")
                .exists()
        );
    }

    #[test]
    fn archive_does_not_crash_on_malformed_task_run_and_allows_when_no_valid_running() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "demo");
        let task_runs_dir = ctx.storage_root.task_runs_dir();
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(
            task_runs_dir.join("run-broken.toml"),
            "task = \"demo\"\nstatus = \"running\"\ncreated_at =",
        )
        .unwrap();

        archive(&ctx, &["demo".to_string()]).unwrap();

        assert!(
            ctx.storage_root
                .task_archive_dir("demo")
                .join("demo.toml")
                .exists()
        );
    }

    #[test]
    fn archive_missing_key_reports_not_found_without_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx(dir.path());

        let err = archive(&ctx, &["nope".to_string()]).unwrap_err();

        let report = format!("{err:#}");
        assert!(report.contains("not found") || report.contains("nope"));
    }

    #[test]
    fn archive_continues_other_keys_when_some_are_rejected_or_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "ok");
        write_task(dir.path(), "busy");
        task_run::create(&ctx, "busy", "busy", None, task_run::STATUS_RUNNING).unwrap();

        let err = archive(
            &ctx,
            &["missing".to_string(), "ok".to_string(), "busy".to_string()],
        )
        .unwrap_err();

        let report = format!("{err:#}");
        assert!(report.contains("missing"));
        assert!(report.contains("running"));
        assert!(
            ctx.storage_root
                .task_archive_dir("ok")
                .join("ok.toml")
                .exists()
        );
        assert!(ctx.storage_root.tasks_dir().join("busy.toml").exists());
    }

    #[test]
    fn archive_asks_confirm_and_aborts_on_no() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), false);
        write_task(dir.path(), "demo");

        archive(&ctx, &["demo".to_string()]).unwrap();

        assert!(ctx.storage_root.tasks_dir().join("demo.toml").exists());
        assert!(
            !ctx.storage_root
                .task_archive_dir("demo")
                .join("demo.toml")
                .exists()
        );
    }

    #[cfg(unix)]
    #[test]
    fn archive_preserves_active_task_when_manifest_write_fails() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = test_ctx_with_confirm(dir.path(), true);
        write_task(dir.path(), "demo");
        let archive_dir = ctx.storage_root.task_archive_dir("demo");
        fs::create_dir_all(&archive_dir).unwrap();
        symlink(
            archive_dir.join("missing-parent").join("archive.toml"),
            archive_dir.join("archive.toml"),
        )
        .unwrap();

        let err = archive(&ctx, &["demo".to_string()]).unwrap_err();

        let report = format!("{err:#}");
        assert!(report.contains("Failed to write task archive manifest"));
        assert!(ctx.storage_root.tasks_dir().join("demo.toml").exists());
        assert!(!archive_dir.join("demo.toml").exists());
    }

    #[test]
    fn archive_skips_confirm_when_ui_cannot_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, ui) = test_ctx_without_prompt(dir.path());
        write_task(dir.path(), "demo");

        archive(&ctx, &["demo".to_string()]).unwrap();

        assert!(
            ctx.storage_root
                .task_archive_dir("demo")
                .join("demo.toml")
                .exists()
        );
        assert!(ui.prompts.lock().unwrap().is_empty());
    }
}
