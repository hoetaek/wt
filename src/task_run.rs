use crate::context::Ctx;
use crate::messages::AgentId;
use crate::task;
use crate::workflow::{self, WorkflowMode};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const STATUS_PREPARED: TaskRunStatus = TaskRunStatus::Prepared;
pub(crate) const STATUS_RUNNING: TaskRunStatus = TaskRunStatus::Running;
pub(crate) const STATUS_DONE: TaskRunStatus = TaskRunStatus::Done;
pub(crate) const STATUS_FAILED: TaskRunStatus = TaskRunStatus::Failed;
pub(crate) const STATUS_SKIPPED: TaskRunStatus = TaskRunStatus::Skipped;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TaskRunStatus {
    Prepared,
    Running,
    Done,
    Failed,
    Skipped,
}

impl TaskRunStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub(crate) fn parse(status: &str) -> Result<Self> {
        match status {
            "prepared" => Ok(Self::Prepared),
            "running" => Ok(Self::Running),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            _ => bail!("Unknown task run status: {status}"),
        }
    }

    pub(crate) fn is_runnable(self) -> bool {
        matches!(self, Self::Prepared | Self::Failed)
    }

    pub(crate) fn is_task_selectable(self) -> bool {
        matches!(self, Self::Prepared | Self::Failed | Self::Skipped)
    }

    pub(crate) fn is_stack_completable(self) -> bool {
        self == Self::Running
    }

    pub(crate) fn is_cleanup_completable(self) -> bool {
        self == Self::Running
    }
}

impl fmt::Display for TaskRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<TaskRunStatus> for String {
    fn from(status: TaskRunStatus) -> Self {
        status.as_str().to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskRun {
    pub(crate) task: String,
    pub(crate) branch: String,
    pub(crate) status: TaskRunStatus,
    pub(crate) group: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) creation_order: Option<u64>,
    pub(crate) agent_id: Option<String>,
    pub(crate) coordinator_id: Option<String>,
    pub(crate) coordinator_label: Option<String>,
    pub(crate) last_report_message_id: Option<String>,
    pub(crate) last_reported_at: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl TaskRun {
    pub(crate) fn is_runnable(&self) -> bool {
        self.status.is_runnable()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTaskRun {
    task: String,
    branch: String,
    status: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    creation_order: Option<u64>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    coordinator_id: Option<String>,
    #[serde(default)]
    coordinator_label: Option<String>,
    #[serde(default)]
    last_report_message_id: Option<String>,
    #[serde(default)]
    last_reported_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TryFrom<RawTaskRun> for TaskRun {
    type Error = anyhow::Error;

    fn try_from(raw: RawTaskRun) -> Result<Self> {
        if let Some(source) = raw.source.as_deref() {
            validate_legacy_source(source)?;
        }
        let run = TaskRun {
            task: raw.task,
            branch: raw.branch,
            status: TaskRunStatus::parse(&raw.status)?,
            group: raw.group,
            error: raw.error,
            creation_order: raw.creation_order,
            agent_id: raw.agent_id,
            coordinator_id: raw.coordinator_id,
            coordinator_label: raw.coordinator_label,
            last_report_message_id: raw.last_report_message_id,
            last_reported_at: raw.last_reported_at,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        };
        validate_run(&run)?;
        Ok(run)
    }
}

#[derive(Debug, Deserialize)]
struct TaskRunCreationOrder {
    #[serde(default)]
    creation_order: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TaskRunRoutes<'a> {
    agent_id: Option<&'a str>,
    coordinator_id: Option<&'a str>,
    coordinator_label: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskRunRecord {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) run: TaskRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TaskRunContext {
    Direct,
    WorkflowLinked(WorkflowTaskRunContext),
    UnresolvedWorkflowGroup { group: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkflowTaskRunContext {
    pub(crate) workflow_id: String,
    pub(crate) workflow_path: PathBuf,
    pub(crate) mode: WorkflowMode,
    pub(crate) task: String,
}

impl TaskRunContext {
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Direct => "direct".into(),
            Self::WorkflowLinked(context) => {
                format!(
                    "workflow {} mode {}",
                    context.workflow_id,
                    context.mode.as_str()
                )
            }
            Self::UnresolvedWorkflowGroup { group } => {
                format!("workflow group {group} (not discovered)")
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn create(
    ctx: &Ctx,
    task: &str,
    branch: &str,
    group: Option<&str>,
    status: TaskRunStatus,
) -> Result<TaskRunRecord> {
    create_with_coordinator_id(ctx, task, branch, group, None, status)
}

#[cfg(test)]
pub(crate) fn create_with_coordinator_id(
    ctx: &Ctx,
    task: &str,
    branch: &str,
    group: Option<&str>,
    coordinator_id: Option<&str>,
    status: TaskRunStatus,
) -> Result<TaskRunRecord> {
    create_with_routes(
        ctx,
        task,
        branch,
        group,
        TaskRunRoutes {
            coordinator_id,
            ..TaskRunRoutes::default()
        },
        status,
    )
}

pub(crate) fn create_direct_routed(
    ctx: &Ctx,
    task: &str,
    branch: &str,
    coordinator_id: &str,
    coordinator_label: Option<&str>,
    status: TaskRunStatus,
) -> Result<TaskRunRecord> {
    create_with_routes(
        ctx,
        task,
        branch,
        None,
        TaskRunRoutes {
            coordinator_id: Some(coordinator_id),
            coordinator_label,
            ..TaskRunRoutes::default()
        },
        status,
    )
}

pub(crate) fn create_workflow_routed(
    ctx: &Ctx,
    task: &str,
    branch: &str,
    group: &str,
    coordinator_id: &str,
    coordinator_label: Option<&str>,
    status: TaskRunStatus,
) -> Result<TaskRunRecord> {
    create_with_routes(
        ctx,
        task,
        branch,
        Some(group),
        TaskRunRoutes {
            coordinator_id: Some(coordinator_id),
            coordinator_label,
            ..TaskRunRoutes::default()
        },
        status,
    )
}

fn create_with_routes(
    ctx: &Ctx,
    task: &str,
    branch: &str,
    group: Option<&str>,
    routes: TaskRunRoutes<'_>,
    status: TaskRunStatus,
) -> Result<TaskRunRecord> {
    let now = current_utc_timestamp();
    let creation_order = next_creation_order(ctx)?;
    let task_key = task::safe_task_key(task);
    let agent_id = match routes.agent_id.and_then(optional_string) {
        Some(agent_id) => Some(agent_id),
        None if routes.coordinator_id.is_some() => {
            Some(generated_task_agent_id(creation_order, &task_key)?)
        }
        None => None,
    };
    let run = TaskRun {
        task: task_key,
        branch: branch.to_string(),
        status,
        group: group.and_then(optional_string),
        error: None,
        creation_order: Some(creation_order),
        agent_id,
        coordinator_id: routes.coordinator_id.and_then(optional_string),
        coordinator_label: routes.coordinator_label.and_then(optional_string),
        last_report_message_id: None,
        last_reported_at: None,
        created_at: now.clone(),
        updated_at: now,
    };
    write_new(ctx, &run)
}

pub(crate) fn launch_template_vars(record: &TaskRunRecord) -> HashMap<String, String> {
    launch_template_vars_for(&record.id, &record.run)
}

pub(crate) fn launch_template_vars_for(id: &str, run: &TaskRun) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("wt_task_run_id".into(), id.to_string());
    if let Some(agent_id) = run.agent_id.as_deref() {
        vars.insert("wt_agent_id".into(), agent_id.to_string());
    }
    vars
}

pub(crate) fn ensure_workflow_routes(
    record: &TaskRunRecord,
    coordinator_id: &str,
    coordinator_label: Option<&str>,
) -> Result<TaskRunRecord> {
    let coordinator_id = AgentId::parse(coordinator_id)
        .context("Invalid coordinator id for workflow TaskRun route repair")?;
    let mut run = read(&record.path)?;
    let mut changed = false;

    if run
        .coordinator_id
        .as_deref()
        .and_then(optional_string)
        .is_none()
    {
        run.coordinator_id = Some(coordinator_id.as_str().to_string());
        changed = true;
    }
    if run.agent_id.as_deref().and_then(optional_string).is_none() {
        run.agent_id = Some(generated_task_agent_id_for_record(&run, &record.id)?);
        changed = true;
    }
    if run
        .coordinator_label
        .as_deref()
        .and_then(optional_string)
        .is_none()
    {
        if let Some(label) = coordinator_label.and_then(optional_string) {
            run.coordinator_label = Some(label);
            changed = true;
        }
    }

    if changed {
        run.updated_at = current_utc_timestamp();
        write(&record.path, &run)?;
    }

    Ok(TaskRunRecord {
        id: record.id.clone(),
        path: record.path.clone(),
        run,
    })
}

pub(crate) fn read(path: &Path) -> Result<TaskRun> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read task run: {}", path.display()))?;
    let raw: RawTaskRun = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task run: {}", path.display()))?;
    TaskRun::try_from(raw)
}

pub(crate) fn list(ctx: &Ctx) -> Result<Vec<TaskRunRecord>> {
    task_run_paths(ctx)?
        .into_iter()
        .map(|path| {
            let id = id_from_path(&path)?;
            let run = read(&path)?;
            Ok(TaskRunRecord { id, path, run })
        })
        .collect()
}

pub(crate) fn task_run_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    ensure_no_legacy_task_runs(ctx)?;
    let task_runs_dir = ctx.storage_root.task_runs_dir();
    if !task_runs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&task_runs_dir).with_context(|| {
        format!(
            "Failed to read task run directory: {}",
            ctx.storage_root.display_path(&task_runs_dir)
        )
    })? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn id_from_path(path: &Path) -> Result<String> {
    task_run_id(path)
}

pub(crate) fn path_for_id(ctx: &Ctx, id: &str) -> Result<PathBuf> {
    ensure_no_legacy_task_runs(ctx)?;
    Ok(ctx.storage_root.task_run_path(id))
}

pub(crate) fn resolve(ctx: &Ctx, target: &str) -> Result<PathBuf> {
    ensure_no_legacy_task_runs(ctx)?;

    if target == "latest" {
        return latest_path(ctx);
    }

    let path = storage_display_target(ctx, target).unwrap_or_else(|| PathBuf::from(target));
    if path.is_absolute() && path.exists() {
        return Ok(path);
    }

    let invocation_path = ctx.invocation_root.join(target);
    if invocation_path.exists() {
        return Ok(invocation_path);
    }

    let repo_path = ctx.repo_root.join(target);
    if repo_path.exists() {
        return Ok(repo_path);
    }

    let file_name = if target.ends_with(".toml") {
        target.to_string()
    } else {
        format!("{target}.toml")
    };
    let shorthand = ctx.storage_root.task_runs_dir().join(file_name);
    if shorthand.exists() {
        return Ok(shorthand);
    }

    bail!("Task run not found: {target}");
}

fn storage_display_target(ctx: &Ctx, target: &str) -> Option<PathBuf> {
    target
        .strip_prefix("<git-common-dir>/")
        .map(|relative| ctx.storage_root.git_common_dir().join(relative))
}

pub(crate) fn update(
    ctx: &Ctx,
    target: &str,
    status: TaskRunStatus,
    branch: Option<&str>,
    error: Option<&str>,
) -> Result<TaskRunRecord> {
    let path = resolve(ctx, target)?;
    let mut run = read(&path)?;
    run.status = status;
    if let Some(branch) = branch {
        run.branch = branch.to_string();
    }
    run.error = error.and_then(optional_string);
    run.updated_at = current_utc_timestamp();
    write(&path, &run)?;

    Ok(TaskRunRecord {
        id: task_run_id(&path)?,
        path,
        run,
    })
}

pub(crate) fn update_report_metadata(
    record: &TaskRunRecord,
    message_id: &str,
) -> Result<TaskRunRecord> {
    let mut run = read(&record.path)?;
    let now = current_utc_timestamp();
    run.last_report_message_id = optional_string(message_id);
    run.last_reported_at = Some(now.clone());
    run.updated_at = now;
    write(&record.path, &run)?;

    Ok(TaskRunRecord {
        id: record.id.clone(),
        path: record.path.clone(),
        run,
    })
}

pub(crate) fn delete_record(record: &TaskRunRecord) -> Result<()> {
    match fs::remove_file(&record.path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("Failed to delete task run: {}", record.path.display())),
    }
}

pub(crate) fn latest_for_task(ctx: &Ctx, task: &str) -> Result<Option<TaskRunRecord>> {
    let task = task::safe_task_key(task);
    let mut runs = list(ctx)?
        .into_iter()
        .filter(|record| record.run.task == task)
        .collect::<Vec<_>>();
    runs.sort_by(compare_task_run_records);
    Ok(runs.pop())
}

pub(crate) fn task_is_selectable(ctx: &Ctx, task: &str) -> Result<bool> {
    let Some(record) = latest_for_task(ctx, task)? else {
        return Ok(true);
    };
    Ok(record.run.status.is_task_selectable())
}

pub(crate) fn running_cleanup_matches(ctx: &Ctx, branch: &str) -> Result<Vec<TaskRunRecord>> {
    let mut records = Vec::new();
    for record in list(ctx)? {
        if record.run.branch != branch || !record.run.status.is_cleanup_completable() {
            continue;
        }
        if matches!(resolve_context(ctx, &record), Ok(TaskRunContext::Direct)) {
            records.push(record);
        }
    }
    Ok(records)
}

pub(crate) fn resolve_context(ctx: &Ctx, record: &TaskRunRecord) -> Result<TaskRunContext> {
    let Some(group) = record.run.group.as_deref() else {
        return Ok(TaskRunContext::Direct);
    };

    for path in workflow::workflow_paths(ctx)? {
        let workflow_id = workflow::id_from_path(&path)?;
        if workflow_id != group {
            continue;
        }
        let metadata = workflow::read(&path)?;
        if let Some(row) = metadata.tasks.iter().find(|row| {
            row.task == record.run.task
                && (row.run == record.id || row.runs.iter().any(|run| run.run == record.id))
        }) {
            return Ok(TaskRunContext::WorkflowLinked(WorkflowTaskRunContext {
                workflow_id,
                workflow_path: path,
                mode: metadata.mode,
                task: row.task.clone(),
            }));
        }
    }

    Ok(TaskRunContext::UnresolvedWorkflowGroup {
        group: group.to_string(),
    })
}

pub(crate) fn group_from_path(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Task run group path is missing a file stem"))
}

fn write_new(ctx: &Ctx, run: &TaskRun) -> Result<TaskRunRecord> {
    ensure_no_legacy_task_runs(ctx)?;
    let task_runs_dir = ctx.storage_root.task_runs_dir();
    fs::create_dir_all(&task_runs_dir)?;

    let id_base = task_run_id_base(run);
    let (id, path) = next_available_task_run_path(&task_runs_dir, &id_base);
    write(&path, run)?;
    Ok(TaskRunRecord {
        id,
        path,
        run: run.clone(),
    })
}

fn write(path: &Path, run: &TaskRun) -> Result<()> {
    validate_run(run)?;

    let mut content = String::new();
    content.push_str(&format!("task = {}\n", toml_quote(&run.task)));
    content.push_str(&format!("branch = {}\n", toml_quote(&run.branch)));
    content.push_str(&format!("status = {}\n", toml_quote(run.status.as_str())));
    if let Some(group) = run.group.as_deref() {
        content.push_str(&format!("group = {}\n", toml_quote(group)));
    }
    if let Some(error) = run.error.as_deref() {
        content.push_str(&format!("error = {}\n", toml_quote(error)));
    }
    if let Some(creation_order) = run.creation_order {
        content.push_str(&format!("creation_order = {creation_order}\n"));
    }
    if let Some(agent_id) = run.agent_id.as_deref() {
        content.push_str(&format!("agent_id = {}\n", toml_quote(agent_id)));
    }
    if let Some(coordinator_id) = run.coordinator_id.as_deref() {
        content.push_str(&format!(
            "coordinator_id = {}\n",
            toml_quote(coordinator_id)
        ));
    }
    if let Some(coordinator_label) = run.coordinator_label.as_deref() {
        content.push_str(&format!(
            "coordinator_label = {}\n",
            toml_quote(coordinator_label)
        ));
    }
    if let Some(message_id) = run.last_report_message_id.as_deref() {
        content.push_str(&format!(
            "last_report_message_id = {}\n",
            toml_quote(message_id)
        ));
    }
    if let Some(reported_at) = run.last_reported_at.as_deref() {
        content.push_str(&format!("last_reported_at = {}\n", toml_quote(reported_at)));
    }
    content.push_str(&format!("created_at = {}\n", toml_quote(&run.created_at)));
    content.push_str(&format!("updated_at = {}\n", toml_quote(&run.updated_at)));

    fs::write(path, content)?;
    Ok(())
}

fn validate_run(run: &TaskRun) -> Result<()> {
    if run.task.trim().is_empty() {
        bail!("Task run is missing task");
    }
    if matches!(run.creation_order, Some(0)) {
        bail!("Task run creation_order must be greater than 0");
    }
    if let Some(agent_id) = run.agent_id.as_deref() {
        AgentId::parse(agent_id).context("Invalid TaskRun agent_id")?;
    }
    Ok(())
}

fn latest_path(ctx: &Ctx) -> Result<PathBuf> {
    let mut records = list(ctx)?;
    records.sort_by(compare_task_run_records);
    records
        .pop()
        .map(|record| record.path)
        .ok_or_else(|| anyhow::anyhow!("No task run files found in <git-common-dir>/wt/task-runs"))
}

pub(crate) fn compare_task_run_records(left: &TaskRunRecord, right: &TaskRunRecord) -> Ordering {
    match (left.run.creation_order, right.run.creation_order) {
        (Some(left_order), Some(right_order)) => left_order
            .cmp(&right_order)
            .then_with(|| compare_task_run_record_fallbacks(left, right)),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => compare_task_run_record_fallbacks(left, right),
    }
}

fn compare_task_run_record_fallbacks(left: &TaskRunRecord, right: &TaskRunRecord) -> Ordering {
    normalized_utc_timestamp(&left.run.created_at)
        .cmp(&normalized_utc_timestamp(&right.run.created_at))
        .then_with(|| left.id.cmp(&right.id))
}

fn next_creation_order(ctx: &Ctx) -> Result<u64> {
    ensure_no_legacy_task_runs(ctx)?;
    let task_runs_dir = ctx.storage_root.task_runs_dir();
    if !task_runs_dir.exists() {
        return Ok(1);
    }

    let mut max_order = 0_u64;
    for entry in fs::read_dir(&task_runs_dir).with_context(|| {
        format!(
            "Failed to read task run directory: {}",
            ctx.storage_root.display_path(&task_runs_dir)
        )
    })? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read task run: {}", path.display()))?;
        let order: TaskRunCreationOrder = toml::from_str(&content)
            .with_context(|| format!("Failed to parse task run: {}", path.display()))?;
        if let Some(order) = order.creation_order {
            max_order = max_order.max(order);
        }
    }

    max_order
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("Task run creation_order overflow"))
}

#[cfg(test)]
pub(crate) fn task_run_display_path(ctx: &Ctx, path: &Path) -> String {
    ctx.storage_root.display_path(path)
}

fn ensure_no_legacy_task_runs(ctx: &Ctx) -> Result<()> {
    if let Some(legacy) = ctx.storage_root.detect_legacy_task_runs(&ctx.repo_root) {
        bail!(
            "Found legacy TaskRun storage at {}. Canonical TaskRun storage is {}. wt does not silently read .local/task-runs; import or repair legacy state explicitly before using this command.",
            legacy.path().display(),
            ctx.storage_root.display_path(legacy.canonical_root())
        );
    }
    Ok(())
}

fn next_available_task_run_path(dir: &Path, id_base: &str) -> (String, PathBuf) {
    let mut seq = 1;
    loop {
        let id = if seq == 1 {
            id_base.to_string()
        } else {
            format!("{id_base}-{seq:03}")
        };
        let path = dir.join(format!("{id}.toml"));
        if !path.exists() {
            return (id, path);
        }
        seq += 1;
    }
}

fn task_run_id_base(run: &TaskRun) -> String {
    let mut parts = vec!["run"];
    if let Some(group) = run.group.as_deref() {
        parts.push(group);
    }
    parts.push(&run.task);
    parts
        .into_iter()
        .map(task::safe_task_key)
        .collect::<Vec<_>>()
        .join("-")
}

fn generated_task_agent_id(creation_order: u64, task_key: &str) -> Result<String> {
    let task_key = task::safe_task_key(task_key);
    AgentId::parse(&format!("agents/run-{creation_order}-{task_key}"))
        .map(|agent| agent.as_str().to_string())
        .context("Generated TaskRun agent_id was invalid")
}

fn generated_task_agent_id_for_record(run: &TaskRun, record_id: &str) -> Result<String> {
    match run.creation_order {
        Some(order) => generated_task_agent_id(order, &run.task),
        None => AgentId::parse(&format!("agents/{}", task::safe_task_key(record_id)))
            .map(|agent| agent.as_str().to_string())
            .context("Generated legacy TaskRun agent_id was invalid"),
    }
}

fn validate_legacy_source(source: &str) -> Result<()> {
    match source {
        "new" | "batch" | "stack" => Ok(()),
        _ => bail!("Unknown legacy task run source: {source}"),
    }
}

fn task_run_id(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Task run file is missing an id: {}", path.display()))
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn current_utc_timestamp() -> String {
    let (seconds, nanos) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() as i64, duration.subsec_nanos()))
        .unwrap_or((0, 0));
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
}

fn normalized_utc_timestamp(timestamp: &str) -> Option<String> {
    let without_zone = timestamp.trim().strip_suffix('Z')?;
    let (base, fraction) = without_zone
        .split_once('.')
        .map_or((without_zone, ""), |(base, fraction)| (base, fraction));
    if !is_utc_timestamp_base(base) {
        return None;
    }
    if fraction.len() > 9 || !fraction.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let mut nanos = fraction.to_string();
    while nanos.len() < 9 {
        nanos.push('0');
    }
    Some(format!("{base}.{nanos}Z"))
}

fn is_utc_timestamp_base(base: &str) -> bool {
    let bytes = base.as_bytes();
    bytes.len() == 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes
            .iter()
            .enumerate()
            .all(|(idx, byte)| matches!(idx, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit())
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

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, CtxOptions};
    use crate::storage::StorageRoot;
    use crate::task::{self, TaskDocument};
    use std::fs;
    use std::process::Command;

    const GIT_LOCAL_ENV_KEYS: &[&str] = &[
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ];

    struct CleanGitRunner;

    impl CommandRunner for CleanGitRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            let mut command = clean_command(cmd);
            command.args(args);
            if let Some(cwd) = cwd {
                command.current_dir(cwd);
            }
            let output = command.output()?;
            Ok(CmdOutput {
                stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                success: output.status.success(),
            })
        }

        fn has_command(&self, cmd: &str) -> bool {
            clean_command(cmd)
                .arg("--version")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        }
    }

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_storage(repo: &Path, invocation: &Path, storage_root: StorageRoot) -> Ctx {
        Ctx::new_with_options(
            repo.to_path_buf(),
            invocation.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                storage_root: Some(storage_root),
                ..CtxOptions::default()
            },
        )
    }

    #[test]
    fn task_run_toml_write_read_list_and_update_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let record = create(
            &ctx,
            "add/schema",
            "add-schema",
            Some("2026-05-16-001"),
            STATUS_RUNNING,
        )
        .unwrap();

        assert_eq!(record.id, "run-2026-05-16-001-add-schema");
        assert_eq!(
            task_run_display_path(&ctx, &record.path),
            "<git-common-dir>/wt/task-runs/run-2026-05-16-001-add-schema.toml"
        );
        let parsed = read(&record.path).unwrap();
        assert_eq!(parsed.task, "add-schema");
        assert_eq!(parsed.branch, "add-schema");
        assert_eq!(parsed.status, STATUS_RUNNING);
        assert_eq!(parsed.group.as_deref(), Some("2026-05-16-001"));
        assert!(parsed.error.is_none());
        assert_eq!(parsed.creation_order, Some(1));

        let records = list(&ctx).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, record.id);

        let updated = update(
            &ctx,
            &record.id,
            STATUS_FAILED,
            Some("alice/add-schema"),
            Some("setup failed"),
        )
        .unwrap();
        assert_eq!(updated.run.status, STATUS_FAILED);
        assert_eq!(updated.run.branch, "alice/add-schema");
        assert_eq!(updated.run.error.as_deref(), Some("setup failed"));

        let content = std::fs::read_to_string(updated.path).unwrap();
        assert!(content.contains("task = \"add-schema\""));
        assert!(!content.contains("source ="));
        assert!(content.contains("group = \"2026-05-16-001\""));
        assert!(content.contains("error = \"setup failed\""));
        assert!(content.contains("creation_order = 1"));
        assert!(!content.contains("cmux"));
        assert!(!content.contains("workspace"));
        assert!(!content.contains("surface"));
    }

    #[test]
    fn task_run_toml_round_trips_coordinator_id_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let record = create_with_coordinator_id(
            &ctx,
            "add-schema",
            "add-schema",
            None,
            Some("agents/coord-a"),
            STATUS_RUNNING,
        )
        .unwrap();

        let parsed = read(&record.path).unwrap();
        assert_eq!(parsed.coordinator_id.as_deref(), Some("agents/coord-a"));
        assert_eq!(parsed.agent_id.as_deref(), Some("agents/run-1-add-schema"));

        let content = std::fs::read_to_string(record.path).unwrap();
        assert!(content.contains("coordinator_id = \"agents/coord-a\""));
        assert!(content.contains("agent_id = \"agents/run-1-add-schema\""));
    }

    #[test]
    fn direct_routed_task_run_round_trips_label_and_report_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator for task \"Add schema\""),
            STATUS_RUNNING,
        )
        .unwrap();

        let updated = update_report_metadata(&record, "msg_123").unwrap();
        let parsed = read(&updated.path).unwrap();

        assert_eq!(parsed.agent_id.as_deref(), Some("agents/run-1-add-schema"));
        assert_eq!(parsed.coordinator_id.as_deref(), Some("agents/coord-a"));
        assert_eq!(
            parsed.coordinator_label.as_deref(),
            Some("Coordinator for task \"Add schema\"")
        );
        assert_eq!(parsed.last_report_message_id.as_deref(), Some("msg_123"));
        assert!(parsed.last_reported_at.is_some());

        let content = std::fs::read_to_string(updated.path).unwrap();
        assert!(content.contains("agent_id = \"agents/run-1-add-schema\""));
        assert!(content.contains("coordinator_id = \"agents/coord-a\""));
        assert!(
            content.contains("coordinator_label = \"Coordinator for task \\\"Add schema\\\"\"")
        );
        assert!(content.contains("last_report_message_id = \"msg_123\""));
        assert!(content.contains("last_reported_at = "));
    }

    #[test]
    fn ensure_workflow_routes_repairs_legacy_run_without_overwriting_coordinator() {
        let dir = tempfile::tempdir().unwrap();
        let task_runs_dir = dir.path().join(".git/wt/task-runs");
        std::fs::create_dir_all(&task_runs_dir).unwrap();
        let path = task_runs_dir.join("run-workflow-legacy.toml");
        let run = TaskRun {
            task: "legacy-task".into(),
            branch: "legacy-task".into(),
            status: STATUS_PREPARED,
            group: Some("workflow-1".into()),
            error: None,
            creation_order: Some(42),
            agent_id: None,
            coordinator_id: Some("agents/coord-existing".into()),
            coordinator_label: None,
            last_report_message_id: None,
            last_reported_at: None,
            created_at: "2026-05-16T00:00:00Z".into(),
            updated_at: "2026-05-16T00:00:00Z".into(),
        };
        write(&path, &run).unwrap();
        let record = TaskRunRecord {
            id: "run-workflow-legacy".into(),
            path,
            run,
        };

        let repaired = ensure_workflow_routes(
            &record,
            "agents/coord-new",
            Some("Coordinator for workflow \"Legacy\""),
        )
        .unwrap();

        assert_eq!(
            repaired.run.agent_id.as_deref(),
            Some("agents/run-42-legacy-task")
        );
        assert_eq!(
            repaired.run.coordinator_id.as_deref(),
            Some("agents/coord-existing")
        );
        assert_eq!(
            repaired.run.coordinator_label.as_deref(),
            Some("Coordinator for workflow \"Legacy\"")
        );
        assert_ne!(repaired.run.updated_at, "2026-05-16T00:00:00Z");

        let content = std::fs::read_to_string(repaired.path).unwrap();
        assert!(content.contains("agent_id = \"agents/run-42-legacy-task\""));
        assert!(content.contains("coordinator_id = \"agents/coord-existing\""));
        assert!(
            content.contains("coordinator_label = \"Coordinator for workflow \\\"Legacy\\\"\"")
        );
    }

    #[test]
    fn task_run_toml_without_coordinator_id_parses_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.toml");

        std::fs::write(
            &path,
            r#"task = "add-schema"
branch = "add-schema"
status = "running"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"
"#,
        )
        .unwrap();

        let parsed = read(&path).unwrap();
        assert!(parsed.coordinator_id.is_none());
        assert!(parsed.agent_id.is_none());
        assert!(parsed.coordinator_label.is_none());
        assert!(parsed.last_report_message_id.is_none());
        assert!(parsed.last_reported_at.is_none());
    }

    #[test]
    fn linked_worktree_reads_common_dir_task_and_task_run_state() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let linked = temp.path().join("linked");
        fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        run_git(
            &repo,
            &["worktree", "add", "-b", "linked", path_str(&linked), "HEAD"],
        );

        let storage_root = StorageRoot::resolve(&CleanGitRunner, Some(&linked)).unwrap();
        let main_ctx = ctx_with_storage(&repo, &repo, storage_root.clone());
        let linked_ctx = ctx_with_storage(&repo, &linked, storage_root);
        let document = TaskDocument {
            title: "Shared task".into(),
            branch: "linked".into(),
            body: "Common dir task body".into(),
            origin: None,
        };

        task::write_task_document(&main_ctx, "shared", &document).unwrap();
        let run = create(&main_ctx, "shared", "linked", None, STATUS_RUNNING).unwrap();

        let selected = task::select_local_task_by_key(&linked_ctx, "shared").unwrap();
        let records = list(&linked_ctx).unwrap();

        assert_eq!(selected.path, "<git-common-dir>/wt/tasks/shared.toml");
        assert_eq!(selected.document.title, "Shared task");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, run.id);
        assert_eq!(records[0].path, run.path);
        assert_eq!(
            task_run_display_path(&linked_ctx, &records[0].path),
            "<git-common-dir>/wt/task-runs/run-shared.toml"
        );
    }

    #[test]
    fn task_run_store_rejects_legacy_local_task_runs_without_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_runs_dir = dir.path().join(".local/task-runs");
        std::fs::create_dir_all(&legacy_runs_dir).unwrap();
        std::fs::write(
            legacy_runs_dir.join("run-legacy.toml"),
            r#"task = "legacy"
branch = "legacy"
status = "running"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"
"#,
        )
        .unwrap();
        let ctx = ctx(dir.path());

        let err = list(&ctx).unwrap_err().to_string();

        assert!(err.contains("Found legacy TaskRun storage"));
        assert!(err.contains(".local/task-runs"));
        assert!(err.contains("<git-common-dir>/wt/task-runs"));
    }

    #[test]
    fn read_rejects_invalid_status_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.toml");

        std::fs::write(
            &path,
            r#"task = "add-schema"
branch = "add-schema"
status = "started"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"
"#,
        )
        .unwrap();
        assert!(read(&path).unwrap_err().to_string().contains("status"));

        std::fs::write(
            &path,
            r#"task = "add-schema"
branch = "add-schema"
status = "running"
source = "queue"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"
"#,
        )
        .unwrap();
        assert!(read(&path).unwrap_err().to_string().contains("source"));
    }

    #[test]
    fn read_accepts_legacy_source_without_rewriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.toml");

        std::fs::write(
            &path,
            r#"task = "add-schema"
branch = "add-schema"
status = "running"
source = "new"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"
"#,
        )
        .unwrap();

        let parsed = read(&path).unwrap();
        assert_eq!(parsed.status, STATUS_RUNNING);

        write(&path, &parsed).unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert!(!content.contains("source ="));
    }

    #[test]
    fn read_rejects_runtime_binding_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.toml");

        std::fs::write(
            &path,
            r#"task = "add-schema"
branch = "add-schema"
status = "running"
cmux_workspace = "workspace:1"
cmux_surface = "surface:4"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"
"#,
        )
        .unwrap();

        let err = format!("{:#}", read(&path).unwrap_err());

        assert!(err.contains("unknown field"));
    }

    #[test]
    fn create_uses_next_id_without_clobbering_existing_run() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let first = create(&ctx, "add-schema", "add-schema", None, STATUS_DONE).unwrap();
        let second = create(&ctx, "add-schema", "add-schema", None, STATUS_RUNNING).unwrap();

        assert_eq!(first.id, "run-add-schema");
        assert_eq!(second.id, "run-add-schema-002");
        assert_eq!(first.run.creation_order, Some(1));
        assert_eq!(second.run.creation_order, Some(2));
        assert!(first.path.exists());
        assert!(second.path.exists());
        assert_eq!(list(&ctx).unwrap().len(), 2);
    }

    #[test]
    fn task_selectability_uses_latest_run_status() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        assert!(task_is_selectable(&ctx, "add-schema").unwrap());
        create(&ctx, "add-schema", "add-schema", None, STATUS_RUNNING).unwrap();
        assert!(!task_is_selectable(&ctx, "add-schema").unwrap());
        create(&ctx, "add-schema", "add-schema", None, STATUS_FAILED).unwrap();
        assert!(task_is_selectable(&ctx, "add-schema").unwrap());
        create(&ctx, "add-schema", "add-schema", None, STATUS_SKIPPED).unwrap();
        assert!(task_is_selectable(&ctx, "add-schema").unwrap());
        create(&ctx, "add-schema", "add-schema", None, STATUS_DONE).unwrap();
        assert!(!task_is_selectable(&ctx, "add-schema").unwrap());
    }

    #[test]
    fn running_cleanup_matches_skips_unreadable_workflow_contexts() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        std::fs::create_dir_all(dir.path().join(".git/wt/workflows")).unwrap();

        let direct = create(&ctx, "direct-task", "feature", None, STATUS_RUNNING).unwrap();
        create(
            &ctx,
            "workflow-task",
            "feature",
            Some("broken-workflow"),
            STATUS_RUNNING,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".git/wt/workflows/broken-workflow.toml"),
            "mode = [",
        )
        .unwrap();

        let records = running_cleanup_matches(&ctx, "feature").unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, direct.id);
    }

    #[test]
    fn latest_for_task_uses_creation_order_when_created_at_ties() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let task_runs_dir = dir.path().join(".git/wt/task-runs");
        std::fs::create_dir_all(&task_runs_dir).unwrap();

        write(
            &task_runs_dir.join("z-earlier-id.toml"),
            &run_with_order("add-schema", STATUS_DONE, Some(1), "2026-05-16T00:00:00Z"),
        )
        .unwrap();
        write(
            &task_runs_dir.join("a-later-id.toml"),
            &run_with_order("add-schema", STATUS_FAILED, Some(2), "2026-05-16T00:00:00Z"),
        )
        .unwrap();

        let latest = latest_for_task(&ctx, "add-schema").unwrap().unwrap();
        assert_eq!(latest.id, "a-later-id");
        assert_eq!(latest.run.status, STATUS_FAILED);
        assert!(task_is_selectable(&ctx, "add-schema").unwrap());
    }

    #[test]
    fn latest_for_task_sorts_fractional_timestamps_after_previous_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let task_runs_dir = dir.path().join(".git/wt/task-runs");
        std::fs::create_dir_all(&task_runs_dir).unwrap();

        write(
            &task_runs_dir.join("z-previous.toml"),
            &run_with_order("add-schema", STATUS_DONE, None, "2026-05-16T00:00:00Z"),
        )
        .unwrap();
        write(
            &task_runs_dir.join("a-fractional.toml"),
            &run_with_order(
                "add-schema",
                STATUS_FAILED,
                None,
                "2026-05-16T00:00:00.000000001Z",
            ),
        )
        .unwrap();

        let latest = latest_for_task(&ctx, "add-schema").unwrap().unwrap();
        assert_eq!(latest.id, "a-fractional");
        assert_eq!(latest.run.status, STATUS_FAILED);
        assert!(task_is_selectable(&ctx, "add-schema").unwrap());
    }

    #[test]
    fn latest_for_task_orders_mixed_previous_and_ordered_records_totally() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let task_runs_dir = dir.path().join(".git/wt/task-runs");
        std::fs::create_dir_all(&task_runs_dir).unwrap();

        write(
            &task_runs_dir.join("a-ordered-first.toml"),
            &run_with_order("add-schema", STATUS_FAILED, Some(1), "2026-05-16T00:00:03Z"),
        )
        .unwrap();
        write(
            &task_runs_dir.join("b-previous.toml"),
            &run_with_order("add-schema", STATUS_DONE, None, "2026-05-16T00:00:02Z"),
        )
        .unwrap();
        write(
            &task_runs_dir.join("c-ordered-second.toml"),
            &run_with_order(
                "add-schema",
                STATUS_RUNNING,
                Some(2),
                "2026-05-16T00:00:01Z",
            ),
        )
        .unwrap();

        let records = list(&ctx).unwrap();
        let previous = record_by_id(&records, "b-previous");
        let ordered_first = record_by_id(&records, "a-ordered-first");
        let ordered_second = record_by_id(&records, "c-ordered-second");
        assert_eq!(
            compare_task_run_records(previous, ordered_first),
            Ordering::Less
        );
        assert_eq!(
            compare_task_run_records(ordered_first, ordered_second),
            Ordering::Less
        );
        assert_eq!(
            compare_task_run_records(previous, ordered_second),
            Ordering::Less
        );

        let latest = latest_for_task(&ctx, "add-schema").unwrap().unwrap();
        assert_eq!(latest.id, "c-ordered-second");
        assert_eq!(latest.run.status, STATUS_RUNNING);
        assert!(!task_is_selectable(&ctx, "add-schema").unwrap());
    }

    fn record_by_id<'a>(records: &'a [TaskRunRecord], id: &str) -> &'a TaskRunRecord {
        records
            .iter()
            .find(|record| record.id == id)
            .unwrap_or_else(|| panic!("missing task run record: {id}"))
    }

    fn run_with_order(
        task: &str,
        status: TaskRunStatus,
        creation_order: Option<u64>,
        created_at: &str,
    ) -> TaskRun {
        TaskRun {
            task: task.into(),
            branch: task.into(),
            status,
            group: None,
            error: None,
            creation_order,
            agent_id: None,
            coordinator_id: None,
            coordinator_label: None,
            last_report_message_id: None,
            last_reported_at: None,
            created_at: created_at.into(),
            updated_at: created_at.into(),
        }
    }

    fn init_repo(repo: &Path) {
        run_git(repo, &["init"]);
        fs::write(repo.join("README.md"), "sample\n").unwrap();
        run_git(repo, &["add", "README.md"]);
        run_git(
            repo,
            &[
                "-c",
                "user.name=wt test",
                "-c",
                "user.email=wt@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = clean_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed\nstdout: {}\nstderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn clean_command(cmd: &str) -> Command {
        let mut command = Command::new(cmd);
        for key in GIT_LOCAL_ENV_KEYS {
            command.env_remove(key);
        }
        command
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().unwrap()
    }
}
