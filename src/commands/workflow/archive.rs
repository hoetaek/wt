use crate::context::Ctx;
use crate::task;
use crate::task_run;
use crate::workflow::{self as workflow_store, WorkflowMetadata};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn run(ctx: &Ctx, workflow: &str) -> Result<()> {
    let plan = plan_archive(ctx, workflow)?;
    apply_archive(ctx, &plan)?;
    if ctx.is_json() {
        write_json(&plan.manifest)?;
    } else {
        print_text(ctx, &plan);
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ArchiveManifest {
    workflow_id: String,
    archived_at: String,
    wt_version: String,
    workflow_source_path: String,
    workflow_archive_path: String,
    task_runs: Vec<TaskRunManifestEntry>,
    tasks: Vec<TaskManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct TaskRunManifestEntry {
    id: String,
    status: String,
    result: String,
    source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct TaskManifestEntry {
    key: String,
    result: String,
    source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    referenced_by: Vec<String>,
}

#[derive(Debug)]
struct ArchivePlan {
    manifest: ArchiveManifest,
    archive_dir: PathBuf,
    manifest_path: PathBuf,
    workflow_source: PathBuf,
    workflow_archive: PathBuf,
    task_run_moves: Vec<FileMove>,
    task_moves: Vec<FileMove>,
}

#[derive(Debug)]
struct FileMove {
    source: PathBuf,
    archive: PathBuf,
}

fn plan_archive(ctx: &Ctx, workflow: &str) -> Result<ArchivePlan> {
    let workflow_source = resolve_archive_workflow_key(ctx, workflow)?;
    let workflow_id = workflow_store::id_from_path(&workflow_source)?;
    let metadata = workflow_store::read(&workflow_source)?;
    let archive_dir = ctx.storage_root.workflow_archive_dir(&workflow_id);
    if archive_dir.exists() {
        bail!(
            "Workflow archive already exists: {}",
            ctx.storage_root.display_path(&archive_dir)
        );
    }

    let task_run_refs = workflow_task_run_refs(ctx, &metadata)?;
    let offenders = unfinished_task_runs(&task_run_refs);
    if !offenders.is_empty() {
        bail!(
            "Cannot archive workflow {workflow_id}; linked TaskRuns must be passed or skipped. Offending runs: {}",
            offenders.join(", ")
        );
    }

    let task_refs = workflow_task_refs(ctx, &workflow_id, &metadata)?;
    let workflow_archive = archive_dir.join("workflow.toml");
    let manifest_path = archive_dir.join("manifest.toml");
    let mut task_run_moves = Vec::new();
    let mut task_run_entries = Vec::new();
    for reference in task_run_refs {
        let archive_path = archive_dir
            .join("task-runs")
            .join(format!("{}.toml", reference.id));
        let (result, archive_manifest_path) = if reference.exists {
            task_run_moves.push(FileMove {
                source: reference.source_path.clone(),
                archive: archive_path.clone(),
            });
            (
                "moved".to_string(),
                Some(wt_relative_path(ctx, &archive_path)),
            )
        } else {
            ("missing".to_string(), None)
        };
        task_run_entries.push(TaskRunManifestEntry {
            id: reference.id,
            status: reference.status,
            result,
            source_path: wt_relative_path(ctx, &reference.source_path),
            archive_path: archive_manifest_path,
        });
    }

    let mut task_moves = Vec::new();
    let mut task_entries = Vec::new();
    for reference in task_refs {
        let archive_path = archive_dir
            .join("tasks")
            .join(format!("{}.toml", reference.key));
        let (result, archive_manifest_path) = if !reference.exists {
            ("missing".to_string(), None)
        } else if reference.referenced_by.is_empty() {
            task_moves.push(FileMove {
                source: reference.source_path.clone(),
                archive: archive_path.clone(),
            });
            (
                "moved".to_string(),
                Some(wt_relative_path(ctx, &archive_path)),
            )
        } else {
            ("kept".to_string(), None)
        };
        task_entries.push(TaskManifestEntry {
            key: reference.key,
            result,
            source_path: wt_relative_path(ctx, &reference.source_path),
            archive_path: archive_manifest_path,
            referenced_by: reference.referenced_by,
        });
    }

    let manifest = ArchiveManifest {
        workflow_id: workflow_id.clone(),
        archived_at: current_utc_timestamp(),
        wt_version: env!("CARGO_PKG_VERSION").to_string(),
        workflow_source_path: wt_relative_path(ctx, &workflow_source),
        workflow_archive_path: wt_relative_path(ctx, &workflow_archive),
        task_runs: task_run_entries,
        tasks: task_entries,
    };

    Ok(ArchivePlan {
        manifest,
        archive_dir,
        manifest_path,
        workflow_source,
        workflow_archive,
        task_run_moves,
        task_moves,
    })
}

fn apply_archive(ctx: &Ctx, plan: &ArchivePlan) -> Result<()> {
    fs::create_dir_all(ctx.storage_root.archive_workflows_dir()).with_context(|| {
        format!(
            "Failed to create workflow archive root: {}",
            ctx.storage_root
                .display_path(&ctx.storage_root.archive_workflows_dir())
        )
    })?;
    fs::create_dir(&plan.archive_dir).with_context(|| {
        format!(
            "Failed to create workflow archive directory: {}",
            ctx.storage_root.display_path(&plan.archive_dir)
        )
    })?;
    fs::create_dir_all(plan.archive_dir.join("task-runs")).with_context(|| {
        format!(
            "Failed to create archived TaskRun directory: {}",
            ctx.storage_root
                .display_path(&plan.archive_dir.join("task-runs"))
        )
    })?;
    fs::create_dir_all(plan.archive_dir.join("tasks")).with_context(|| {
        format!(
            "Failed to create archived TaskDocument directory: {}",
            ctx.storage_root
                .display_path(&plan.archive_dir.join("tasks"))
        )
    })?;

    copy_file(&plan.workflow_source, &plan.workflow_archive, "workflow")?;
    for file in plan.task_run_moves.iter().chain(plan.task_moves.iter()) {
        copy_file(&file.source, &file.archive, "archive member")?;
    }

    let manifest = toml::to_string_pretty(&plan.manifest)?;
    fs::write(&plan.manifest_path, manifest).with_context(|| {
        format!(
            "Failed to write archive manifest: {}",
            ctx.storage_root.display_path(&plan.manifest_path)
        )
    })?;

    remove_file_if_present(&plan.workflow_source).with_context(|| {
        format!(
            "Failed to remove archived workflow from active storage: {}",
            ctx.storage_root.display_path(&plan.workflow_source)
        )
    })?;
    for file in &plan.task_run_moves {
        remove_file_if_present(&file.source).with_context(|| {
            format!(
                "Failed to remove archived TaskRun from active storage: {}",
                ctx.storage_root.display_path(&file.source)
            )
        })?;
    }
    for file in &plan.task_moves {
        remove_file_if_present(&file.source).with_context(|| {
            format!(
                "Failed to remove archived TaskDocument from active storage: {}",
                ctx.storage_root.display_path(&file.source)
            )
        })?;
    }

    Ok(())
}

fn copy_file(source: &Path, archive: &Path, label: &str) -> Result<()> {
    if let Some(parent) = archive.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create archive directory: {}", parent.display()))?;
    }
    fs::copy(source, archive).with_context(|| {
        format!(
            "Failed to copy {label} into archive: {} -> {}",
            source.display(),
            archive.display()
        )
    })?;
    Ok(())
}

fn remove_file_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[derive(Debug)]
struct TaskRunRef {
    id: String,
    status: String,
    source_path: PathBuf,
    exists: bool,
}

fn workflow_task_run_refs(ctx: &Ctx, metadata: &WorkflowMetadata) -> Result<Vec<TaskRunRef>> {
    let mut refs = Vec::new();
    for id in workflow_task_run_ids(metadata) {
        let source_path = task_run_source_path(ctx, &id)?;
        if source_path.exists() {
            let run = task_run::read(&source_path)?;
            refs.push(TaskRunRef {
                id,
                status: run.status.as_str().to_string(),
                source_path,
                exists: true,
            });
        } else {
            refs.push(TaskRunRef {
                id,
                status: "missing".into(),
                source_path,
                exists: false,
            });
        }
    }
    Ok(refs)
}

fn unfinished_task_runs(refs: &[TaskRunRef]) -> Vec<String> {
    refs.iter()
        .filter(|reference| {
            reference.exists && !matches!(reference.status.as_str(), "passed" | "skipped")
        })
        .map(|reference| format!("{} ({})", reference.id, reference.status))
        .collect()
}

#[derive(Debug)]
struct TaskRef {
    key: String,
    source_path: PathBuf,
    exists: bool,
    referenced_by: Vec<String>,
}

fn workflow_task_refs(
    ctx: &Ctx,
    workflow_id: &str,
    metadata: &WorkflowMetadata,
) -> Result<Vec<TaskRef>> {
    let other_refs = other_workflow_task_refs(ctx, workflow_id)?;
    let mut refs = Vec::new();
    for key in workflow_task_keys(metadata) {
        let source_path = ctx
            .storage_root
            .tasks_dir()
            .join(format!("{}.toml", task::safe_task_key(&key)));
        refs.push(TaskRef {
            key: key.clone(),
            exists: source_path.exists(),
            source_path,
            referenced_by: other_refs.get(&key).cloned().unwrap_or_default(),
        });
    }
    Ok(refs)
}

fn other_workflow_task_refs(ctx: &Ctx, workflow_id: &str) -> Result<BTreeMap<String, Vec<String>>> {
    let mut refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in workflow_store::workflow_paths(ctx)? {
        let other_id = workflow_store::id_from_path(&path)?;
        if other_id == workflow_id {
            continue;
        }
        let workflow = workflow_store::read(&path).with_context(|| {
            format!(
                "Cannot determine shared TaskDocuments because workflow {} could not be read",
                ctx.storage_root.display_path(&path)
            )
        })?;
        for key in workflow_task_keys(&workflow) {
            refs.entry(key).or_default().push(other_id.clone());
        }
    }
    Ok(refs)
}

fn workflow_task_run_ids(metadata: &WorkflowMetadata) -> Vec<String> {
    let mut ids = BTreeSet::new();
    for row in &metadata.tasks {
        let run = row.run.trim();
        if !run.is_empty() {
            ids.insert(run.to_string());
        }
        for profile_run in &row.runs {
            let run = profile_run.run.trim();
            if !run.is_empty() {
                ids.insert(run.to_string());
            }
        }
    }
    ids.into_iter().collect()
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

fn task_run_source_path(ctx: &Ctx, id: &str) -> Result<PathBuf> {
    ensure_file_stem("TaskRun id", id)?;
    task_run::path_for_id(ctx, id)
}

fn resolve_archive_workflow_key(ctx: &Ctx, workflow: &str) -> Result<PathBuf> {
    if workflow == "latest" {
        bail!("wt workflow archive latest is not supported; pass a workflow key explicitly");
    }
    ensure_file_stem("Workflow key", workflow)?;
    workflow_store::workflow_paths(ctx)?;
    let path = ctx
        .storage_root
        .workflows_dir()
        .join(format!("{workflow}.toml"));
    if path.exists() {
        return Ok(path);
    }
    bail!("Workflow not found: {workflow}");
}

fn ensure_file_stem(label: &str, value: &str) -> Result<()> {
    let path = Path::new(value);
    if path.components().count() == 1 && !value.ends_with(".toml") {
        return Ok(());
    }
    bail!("{label} must be a file stem without path separators or .toml extension: {value}");
}

fn wt_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(ctx.storage_root.personal_root())
        .map(|relative| relative.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ctx.storage_root.display_path(path))
}

fn write_json(manifest: &ArchiveManifest) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, manifest)?;
    writeln!(handle)?;
    Ok(())
}

fn print_text(ctx: &Ctx, plan: &ArchivePlan) {
    let moved_runs = plan
        .manifest
        .task_runs
        .iter()
        .filter(|row| row.result == "moved")
        .count();
    let missing_runs = plan
        .manifest
        .task_runs
        .iter()
        .filter(|row| row.result == "missing")
        .count();
    let moved_tasks = plan
        .manifest
        .tasks
        .iter()
        .filter(|row| row.result == "moved")
        .count();
    let kept_tasks = plan
        .manifest
        .tasks
        .iter()
        .filter(|row| row.result == "kept")
        .count();
    let missing_tasks = plan
        .manifest
        .tasks
        .iter()
        .filter(|row| row.result == "missing")
        .count();

    ctx.ui.print_step(&format!(
        "Archived workflow {} to {}",
        plan.manifest.workflow_id,
        ctx.storage_root.display_path(&plan.archive_dir)
    ));
    ctx.ui.print_dim(&format!(
        "  TaskRuns: {moved_runs} moved, {missing_runs} missing"
    ));
    ctx.ui.print_dim(&format!(
        "  TaskDocuments: {moved_tasks} moved, {kept_tasks} kept shared, {missing_tasks} missing"
    ));
    ctx.ui.print_dim(&format!(
        "  Manifest: {}",
        ctx.storage_root.display_path(&plan.manifest_path)
    ));
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::workflow::{WorkflowMode, WorkflowTask};

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn write_task(root: &Path, key: &str) {
        let tasks_dir = root.join(".git/wt/tasks");
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

    fn write_task_run(root: &Path, id: &str, task: &str, status: &str, group: &str) {
        let task_runs_dir = root.join(".git/wt/task-runs");
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(
            task_runs_dir.join(format!("{id}.toml")),
            format!(
                r#"task = "{task}"
branch = "{task}"
status = "{status}"
group = "{group}"
creation_order = 1
created_at = "2026-05-20T00:00:00Z"
updated_at = "2026-05-20T00:00:00Z"
"#
            ),
        )
        .unwrap();
    }

    fn write_workflow(root: &Path, id: &str, tasks: Vec<WorkflowTask>) {
        let ctx = ctx(root);
        let path = root.join(".git/wt/workflows").join(format!("{id}.toml"));
        let mut workflow =
            WorkflowMetadata::new(WorkflowMode::Batch, "explicit", Some("main".into()), tasks);
        workflow.color = Some("red".into());
        workflow_store::write(&ctx, &path, &mut workflow).unwrap();
    }

    #[test]
    fn archive_allows_only_passed_or_skipped_existing_task_runs() {
        for status in ["passed", "skipped"] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx(dir.path());
            write_task(dir.path(), status);
            write_task_run(dir.path(), &format!("run-{status}"), status, status, "wf");
            write_workflow(
                dir.path(),
                "wf",
                vec![WorkflowTask::new(status, format!("run-{status}"))],
            );

            run(&ctx, "wf").unwrap();
            assert!(
                dir.path()
                    .join(".git/wt/archive/workflows/wf/manifest.toml")
                    .exists()
            );
        }

        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let mut tasks = Vec::new();
        for status in ["prepared", "running", "failed"] {
            write_task(dir.path(), status);
            write_task_run(dir.path(), &format!("run-{status}"), status, status, "wf");
            tasks.push(WorkflowTask::new(status, format!("run-{status}")));
        }
        write_workflow(dir.path(), "wf", tasks);

        let error = run(&ctx, "wf").unwrap_err();
        let report = format!("{error:#}");
        assert!(report.contains("run-prepared (prepared)"));
        assert!(report.contains("run-running (running)"));
        assert!(report.contains("run-failed (failed)"));
        assert!(!dir.path().join(".git/wt/archive/workflows/wf").exists());
    }

    #[test]
    fn archive_requires_explicit_workflow_key() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        write_task(dir.path(), "passed");
        write_task_run(dir.path(), "run-passed", "passed", "passed", "wf");
        write_workflow(
            dir.path(),
            "wf",
            vec![WorkflowTask::new("passed", "run-passed")],
        );

        let latest = format!("{:#}", run(&ctx, "latest").unwrap_err());
        assert!(latest.contains("pass a workflow key explicitly"));
        let with_extension = format!("{:#}", run(&ctx, "wf.toml").unwrap_err());
        assert!(with_extension.contains("Workflow key must be a file stem"));
        let with_path = format!("{:#}", run(&ctx, ".git/wt/workflows/wf.toml").unwrap_err());
        assert!(with_path.contains("Workflow key must be a file stem"));
        assert!(dir.path().join(".git/wt/workflows/wf.toml").exists());
    }

    #[test]
    fn archive_moves_unique_tasks_keeps_shared_tasks_and_records_missing_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        for key in ["unique", "shared"] {
            write_task(dir.path(), key);
        }
        for (run, task) in [
            ("run-unique", "unique"),
            ("run-shared", "shared"),
            ("run-missing", "missing"),
        ] {
            write_task_run(dir.path(), run, task, "passed", "wf");
        }
        write_task_run(dir.path(), "run-other", "shared", "prepared", "other");
        write_workflow(
            dir.path(),
            "wf",
            vec![
                WorkflowTask::new("unique", "run-unique"),
                WorkflowTask::new("shared", "run-shared"),
                WorkflowTask::new("missing", "run-missing"),
            ],
        );
        write_workflow(
            dir.path(),
            "other",
            vec![WorkflowTask::new("shared", "run-other")],
        );

        run(&ctx, "wf").unwrap();

        let archive = dir.path().join(".git/wt/archive/workflows/wf");
        assert!(archive.join("workflow.toml").exists());
        assert!(archive.join("tasks/unique.toml").exists());
        assert!(!archive.join("tasks/shared.toml").exists());
        assert!(!archive.join("tasks/missing.toml").exists());
        assert!(!dir.path().join(".git/wt/tasks/unique.toml").exists());
        assert!(dir.path().join(".git/wt/tasks/shared.toml").exists());

        let manifest: ArchiveManifest =
            toml::from_str(&fs::read_to_string(archive.join("manifest.toml")).unwrap()).unwrap();
        let unique = manifest
            .tasks
            .iter()
            .find(|row| row.key == "unique")
            .unwrap();
        assert_eq!(unique.result, "moved");
        assert_eq!(unique.source_path, "tasks/unique.toml");
        assert_eq!(
            unique.archive_path.as_deref(),
            Some("archive/workflows/wf/tasks/unique.toml")
        );
        let shared = manifest
            .tasks
            .iter()
            .find(|row| row.key == "shared")
            .unwrap();
        assert_eq!(shared.result, "kept");
        assert_eq!(shared.referenced_by, vec!["other"]);
        let missing = manifest
            .tasks
            .iter()
            .find(|row| row.key == "missing")
            .unwrap();
        assert_eq!(missing.result, "missing");
    }
}
