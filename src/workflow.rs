use crate::config::validate_profile_name;
use crate::context::Ctx;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) mod planner;
pub(crate) mod render;
pub(crate) mod run;

pub const WORKFLOW_COLOR_ROTATION: &[&str] = &[
    "red", "crimson", "orange", "amber", "olive", "green", "teal", "aqua", "blue", "navy",
    "indigo", "purple", "magenta", "rose", "brown", "charcoal",
];

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowMode {
    Single,
    Batch,
    Stack,
    Matrix,
}

impl WorkflowMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkflowMode::Single => "single",
            WorkflowMode::Batch => "batch",
            WorkflowMode::Stack => "stack",
            WorkflowMode::Matrix => "matrix",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowMetadata {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub origin: Option<WorkflowOrigin>,
    pub mode: WorkflowMode,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    pub base_mode: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub policy: WorkflowPolicy,
    #[serde(default)]
    pub tasks: Vec<WorkflowTask>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowOrigin {
    pub provider: String,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTask {
    pub task: String,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub runs: Vec<WorkflowTaskRun>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTaskRun {
    pub profile: String,
    pub run: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPullRequestMode {
    None,
    Draft,
    Ready,
}

impl WorkflowPullRequestMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowPullRequestMode::None => "none",
            WorkflowPullRequestMode::Draft => "draft",
            WorkflowPullRequestMode::Ready => "ready",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPolicy {
    pub pull_request: WorkflowPullRequestMode,
    pub landing: WorkflowLandingPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowLandingPolicy {
    Manual,
    Auto,
}

impl WorkflowLandingPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowLandingPolicy::Manual => "manual",
            WorkflowLandingPolicy::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowRecord {
    pub id: String,
    pub path: PathBuf,
    pub workflow: WorkflowMetadata,
}

impl WorkflowMetadata {
    pub fn empty(slug: &str) -> Self {
        let mut metadata = Self::new(WorkflowMode::Single, "default", None, Vec::new());
        metadata.title = Some(format!("워크플로우: {slug}"));
        metadata.body = Some(
            "## 목적\n\n\
- \n\n\
## 실행 메모\n\n\
- \n"
                .to_string(),
        );
        metadata
    }

    pub fn new(
        mode: WorkflowMode,
        base_mode: impl Into<String>,
        base: Option<String>,
        tasks: Vec<WorkflowTask>,
    ) -> Self {
        let now = current_utc_timestamp();
        Self {
            title: None,
            body: None,
            origin: None,
            mode,
            profile: None,
            profiles: Vec::new(),
            base_mode: base_mode.into(),
            base,
            color: None,
            created_at: now.clone(),
            updated_at: now,
            policy: WorkflowPolicy::default(),
            tasks,
        }
    }
}

impl Default for WorkflowPolicy {
    fn default() -> Self {
        Self {
            pull_request: WorkflowPullRequestMode::None,
            landing: WorkflowLandingPolicy::Manual,
        }
    }
}

pub fn touch(workflow: &mut WorkflowMetadata) {
    workflow.updated_at = current_utc_timestamp();
}

impl WorkflowTask {
    pub fn new(task: impl Into<String>, run: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            run: run.into(),
            parent: None,
            runs: Vec::new(),
        }
    }

    fn label(&self) -> &str {
        if self.task.trim().is_empty() {
            "workflow-task"
        } else {
            self.task.trim()
        }
    }
}

pub fn create(ctx: &Ctx, mut workflow: WorkflowMetadata) -> Result<WorkflowRecord> {
    let path = next_available_path(ctx)?;
    write(ctx, &path, &mut workflow)?;
    Ok(WorkflowRecord {
        id: workflow_id(&path)?,
        path,
        workflow,
    })
}

pub fn read(path: &Path) -> Result<WorkflowMetadata> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read workflow: {}", path.display()))?;
    let workflow: WorkflowMetadata = match toml::from_str(&content) {
        Ok(workflow) => workflow,
        Err(err) => {
            let err = anyhow::Error::new(err)
                .context(format!("Failed to parse workflow: {}", path.display()));
            if has_removed_objective_field(&content) {
                return Err(err).context(
                    "Workflow uses removed `objective`; rewrite it as top-level `title`, `body`, and optional `[origin]`",
                );
            }
            return Err(err);
        }
    };
    validate_workflow(&workflow)
        .with_context(|| format!("Invalid workflow: {}", path.display()))?;
    Ok(workflow)
}

fn has_removed_objective_field(content: &str) -> bool {
    content.lines().any(|line| {
        let line = line.trim_start();
        !line.starts_with('#')
            && line
                .strip_prefix("objective")
                .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

pub fn list(ctx: &Ctx) -> Result<Vec<WorkflowRecord>> {
    workflow_paths(ctx)?
        .into_iter()
        .map(|path| {
            let id = workflow_id(&path)?;
            let workflow = read(&path)?;
            Ok(WorkflowRecord { id, path, workflow })
        })
        .collect()
}

pub fn resolve(ctx: &Ctx, target: &str) -> Result<PathBuf> {
    ensure_no_legacy_workflows(ctx)?;
    if target == "latest" {
        return latest_path(ctx);
    }

    let path = PathBuf::from(target);
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
    let shorthand = workflows_dir(ctx).join(file_name);
    if shorthand.exists() {
        return Ok(shorthand);
    }

    bail!("Workflow not found: {target}");
}

pub fn latest_path(ctx: &Ctx) -> Result<PathBuf> {
    let mut paths = workflow_paths(ctx)?;
    paths.sort();
    paths.pop().ok_or_else(|| {
        anyhow::anyhow!(
            "No workflow files found in {}",
            ctx.storage_root
                .display_path(&ctx.storage_root.workflows_dir())
        )
    })
}

pub fn write(ctx: &Ctx, path: &Path, workflow: &mut WorkflowMetadata) -> Result<()> {
    ensure_no_legacy_workflows(ctx)?;
    ensure_not_legacy_workflow_path(ctx, path)?;
    ensure_color(ctx, path, workflow)?;
    write_metadata(path, workflow)
}

fn write_metadata(path: &Path, workflow: &WorkflowMetadata) -> Result<()> {
    validate_workflow(workflow)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create workflow directory: {}", parent.display())
        })?;
    }

    fs::write(path, render_workflow_metadata(workflow))
        .with_context(|| format!("Failed to write workflow metadata: {}", path.display()))?;
    Ok(())
}

fn ensure_color(ctx: &Ctx, path: &Path, workflow: &mut WorkflowMetadata) -> Result<()> {
    if workflow
        .color
        .as_deref()
        .is_some_and(|color| !color.trim().is_empty())
    {
        return Ok(());
    }

    workflow.color = Some(next_workflow_color(ctx, Some(path))?);
    Ok(())
}

pub fn next_available_path(ctx: &Ctx) -> Result<PathBuf> {
    ensure_no_legacy_workflows(ctx)?;
    let workflows_dir = workflows_dir(ctx);
    fs::create_dir_all(&workflows_dir).with_context(|| {
        format!(
            "Failed to create workflow directory: {}",
            ctx.storage_root.display_path(&workflows_dir)
        )
    })?;

    let date = current_utc_date();
    let mut seq = 1;
    loop {
        let candidate = workflows_dir.join(format!("{date}-{seq:03}.toml"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        seq += 1;
    }
}

fn next_workflow_color(ctx: &Ctx, excluded_path: Option<&Path>) -> Result<String> {
    let excluded_path = excluded_path.map(|path| comparable_path(ctx, path));
    let mut records = list(ctx)?
        .into_iter()
        .filter(|record| {
            excluded_path
                .as_ref()
                .is_none_or(|excluded| comparable_path(ctx, &record.path) != *excluded)
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.path.cmp(&right.path));

    if let Some(color) = records
        .iter()
        .rev()
        .filter_map(|record| record.workflow.color.as_deref())
        .map(str::trim)
        .find(|color| !color.is_empty())
    {
        if let Some(idx) = palette_index(color) {
            return Ok(WORKFLOW_COLOR_ROTATION[(idx + 1) % WORKFLOW_COLOR_ROTATION.len()].into());
        }
    }

    Ok(WORKFLOW_COLOR_ROTATION[records.len() % WORKFLOW_COLOR_ROTATION.len()].into())
}

fn palette_index(color: &str) -> Option<usize> {
    WORKFLOW_COLOR_ROTATION
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(color))
}

fn validate_workflow(workflow: &WorkflowMetadata) -> Result<()> {
    if workflow.base_mode.trim().is_empty() {
        bail!("Workflow is missing base_mode");
    }
    if workflow.tasks.is_empty() {
        bail!("Workflow has no tasks");
    }
    if workflow
        .title
        .as_deref()
        .is_some_and(|title| title.trim().is_empty())
    {
        bail!("Workflow title cannot be empty");
    }
    if workflow
        .body
        .as_deref()
        .is_some_and(|body| body.trim().is_empty())
    {
        bail!("Workflow body cannot be empty");
    }
    validate_workflow_origin(workflow.origin.as_ref())?;
    if workflow
        .profile
        .as_deref()
        .is_some_and(|profile| profile.trim().is_empty())
    {
        bail!("Workflow profile cannot be empty");
    }
    validate_workflow_profiles(workflow)?;
    if workflow.created_at.trim().is_empty() {
        bail!("Workflow is missing created_at");
    }
    if workflow.updated_at.trim().is_empty() {
        bail!("Workflow is missing updated_at");
    }
    if workflow
        .color
        .as_deref()
        .is_some_and(|color| color.trim().is_empty())
    {
        bail!("Workflow color cannot be empty");
    }
    for item in &workflow.tasks {
        validate_workflow_task(workflow, item)?;
    }
    Ok(())
}

fn validate_workflow_origin(origin: Option<&WorkflowOrigin>) -> Result<()> {
    let Some(origin) = origin else {
        return Ok(());
    };
    if origin.provider.trim().is_empty() {
        bail!("Workflow origin provider cannot be empty");
    }
    if origin.id.trim().is_empty() {
        bail!("Workflow origin id cannot be empty");
    }
    Ok(())
}

fn validate_workflow_profiles(workflow: &WorkflowMetadata) -> Result<()> {
    if !matches!(workflow.mode, WorkflowMode::Matrix) {
        if !workflow.profiles.is_empty() {
            bail!(
                "{} mode workflow cannot store profiles; use mode = \"matrix\"",
                workflow.mode.as_str()
            );
        }
        return Ok(());
    }

    if workflow.profile.is_some() {
        bail!("matrix mode workflow cannot store single profile; use profiles = [...]");
    }
    if workflow.profiles.is_empty() {
        bail!("matrix mode workflow requires at least one profile");
    }
    let mut seen = std::collections::HashSet::new();
    for profile in &workflow.profiles {
        validate_profile_name(profile)?;
        if !seen.insert(profile.as_str()) {
            bail!("Duplicate profile: {profile}");
        }
    }
    if workflow.tasks.len() != 1 {
        bail!("matrix mode workflow requires exactly one task");
    }
    Ok(())
}

fn validate_workflow_task(workflow: &WorkflowMetadata, item: &WorkflowTask) -> Result<()> {
    if item.task.trim().is_empty() {
        bail!("Workflow task is missing task");
    }
    if item
        .parent
        .as_deref()
        .is_some_and(|parent| parent.trim().is_empty())
    {
        bail!("Workflow task {} has an empty parent", item.label());
    }
    if !matches!(workflow.mode, WorkflowMode::Stack) && item.parent.is_some() {
        bail!(
            "{} mode workflow task {} cannot store parent",
            workflow.mode.as_str(),
            item.label()
        );
    }
    if matches!(workflow.mode, WorkflowMode::Matrix) {
        validate_matrix_workflow_task(workflow, item)
    } else {
        if item.run.trim().is_empty() {
            bail!("Workflow task {} is missing TaskRun id", item.label());
        }
        if !item.runs.is_empty() {
            bail!(
                "{} mode workflow task {} cannot store profile runs",
                workflow.mode.as_str(),
                item.label()
            );
        }
        Ok(())
    }
}

fn validate_matrix_workflow_task(workflow: &WorkflowMetadata, item: &WorkflowTask) -> Result<()> {
    if !item.run.trim().is_empty() {
        bail!(
            "matrix mode workflow task {} cannot store run",
            item.label()
        );
    }
    if item.runs.len() != workflow.profiles.len() {
        bail!(
            "matrix mode workflow task {} must store one run for each profile",
            item.label()
        );
    }
    for (idx, run) in item.runs.iter().enumerate() {
        if run.profile != workflow.profiles[idx] {
            bail!(
                "matrix mode workflow task {} run {} must use profile {}",
                item.label(),
                idx + 1,
                workflow.profiles[idx]
            );
        }
        if run.run.trim().is_empty() {
            bail!(
                "matrix mode workflow task {} profile {} is missing TaskRun id",
                item.label(),
                run.profile
            );
        }
    }
    Ok(())
}

pub(crate) fn render_workflow_metadata(workflow: &WorkflowMetadata) -> String {
    let mut content = String::new();
    if let Some(title) = workflow.title.as_deref() {
        content.push_str(&format!("title = {}\n", toml_quote(title)));
    }
    if let Some(body) = workflow.body.as_deref() {
        content.push_str(&format!("body = {}\n", toml_multiline_string(body)));
    }
    content.push_str(&format!("mode = {}\n", toml_quote(workflow.mode.as_str())));
    if let Some(profile) = workflow.profile.as_deref() {
        content.push_str(&format!("profile = {}\n", toml_quote(profile)));
    }
    if !workflow.profiles.is_empty() {
        let profiles = workflow
            .profiles
            .iter()
            .map(|profile| toml_quote(profile))
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("profiles = [{profiles}]\n"));
    }
    content.push_str(&format!(
        "base_mode = {}\n",
        toml_quote(&workflow.base_mode)
    ));
    if let Some(base) = workflow.base.as_deref() {
        content.push_str(&format!("base = {}\n", toml_quote(base)));
    }
    if let Some(color) = workflow.color.as_deref() {
        content.push_str(&format!("color = {}\n", toml_quote(color)));
    }
    content.push_str(&format!(
        "created_at = {}\n",
        toml_quote(&workflow.created_at)
    ));
    content.push_str(&format!(
        "updated_at = {}\n",
        toml_quote(&workflow.updated_at)
    ));
    if let Some(origin) = &workflow.origin {
        content.push_str("\n[origin]\n");
        content.push_str(&format!("provider = {}\n", toml_quote(&origin.provider)));
        content.push_str(&format!("id = {}\n", toml_quote(&origin.id)));
    }
    content.push_str("\n[policy]\n");
    content.push_str(&format!(
        "pull_request = {}\n",
        toml_quote(workflow.policy.pull_request.as_str())
    ));
    content.push_str(&format!(
        "landing = {}\n",
        toml_quote(workflow.policy.landing.as_str())
    ));

    for item in &workflow.tasks {
        content.push_str("\n[[tasks]]\n");
        content.push_str(&format!("task = {}\n", toml_quote(&item.task)));
        if !item.run.trim().is_empty() {
            content.push_str(&format!("run = {}\n", toml_quote(&item.run)));
        }
        if let Some(parent) = item.parent.as_deref() {
            content.push_str(&format!("parent = {}\n", toml_quote(parent)));
        }
        for run in &item.runs {
            content.push_str("\n[[tasks.runs]]\n");
            content.push_str(&format!("profile = {}\n", toml_quote(&run.profile)));
            content.push_str(&format!("run = {}\n", toml_quote(&run.run)));
        }
    }

    content
}

fn toml_multiline_string(value: &str) -> String {
    if value.starts_with(['\n', '\r']) {
        return toml_quote(value);
    }
    let escaped = value
        .replace("\\", "\\\\")
        .replace("\"\"\"", "\\\"\\\"\\\"");
    format!("\"\"\"{escaped}\"\"\"")
}

pub(crate) fn workflow_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    ensure_no_legacy_workflows(ctx)?;
    let workflows_dir = workflows_dir(ctx);
    if !workflows_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&workflows_dir).with_context(|| {
        format!(
            "Failed to read workflow directory: {}",
            ctx.storage_root.display_path(&workflows_dir)
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
    workflow_id(path)
}

fn workflows_dir(ctx: &Ctx) -> PathBuf {
    ctx.storage_root.workflows_dir()
}

fn ensure_no_legacy_workflows(ctx: &Ctx) -> Result<()> {
    if let Some(legacy) = ctx.storage_root.detect_legacy_workflows(&ctx.repo_root) {
        bail!(
            "Found legacy Workflow storage at {}. Canonical Workflow storage is {}. wt does not silently read legacy Workflow storage; import or repair legacy state explicitly before using this command.",
            legacy.path().display(),
            ctx.storage_root.display_path(legacy.canonical_root())
        );
    }
    Ok(())
}

fn ensure_not_legacy_workflow_path(ctx: &Ctx, path: &Path) -> Result<()> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.invocation_root.join(path)
    };
    let legacy_dirs = [
        ctx.storage_root.personal_root().join("workflows"),
        ctx.repo_root.join(".local/workflows"),
    ];
    let normalized_path = crate::storage::normalize_path_lexically(&absolute_path);
    if legacy_dirs
        .iter()
        .map(|legacy_dir| crate::storage::normalize_path_lexically(legacy_dir))
        .any(|legacy_dir| normalized_path.starts_with(legacy_dir))
    {
        bail!(
            "Refusing to write legacy Workflow storage at {}. Canonical Workflow storage is {}.",
            absolute_path.display(),
            ctx.storage_root
                .display_path(&ctx.storage_root.workflows_dir())
        );
    }
    Ok(())
}

fn workflow_id(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Workflow file is missing an id: {}", path.display()))
}

fn comparable_path(ctx: &Ctx, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        ctx.repo_root.join(path)
    }
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

fn current_utc_date() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
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

    fn ctx(root: &Path) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn task(task: &str, run: &str) -> WorkflowTask {
        WorkflowTask::new(task, run)
    }

    #[test]
    fn workflow_write_and_read_round_trip_uses_canonical_task_rows() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let path = dir
            .path()
            .join(".wt/execution/workflows/2026-05-16-001.toml");
        let mut workflow = WorkflowMetadata {
            title: Some("Workflow state model migration".into()),
            body: Some("Ship the workflow state model migration".into()),
            origin: Some(WorkflowOrigin {
                provider: "linear".into(),
                id: "WT-123".into(),
            }),
            mode: WorkflowMode::Stack,
            profile: Some("codex".into()),
            profiles: Vec::new(),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            color: Some("blue".into()),
            created_at: "2026-05-16T00:00:00Z".into(),
            updated_at: "2026-05-16T00:00:00Z".into(),
            policy: WorkflowPolicy {
                pull_request: WorkflowPullRequestMode::Draft,
                landing: WorkflowLandingPolicy::Auto,
            },
            tasks: vec![WorkflowTask {
                task: "add-schema".into(),
                run: "stack-2026-05-16-001-add-schema".into(),
                parent: Some("main".into()),
                runs: Vec::new(),
            }],
        };

        write(&ctx, &path, &mut workflow).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with(
            "title = \"Workflow state model migration\"\nbody = \"\"\"Ship the workflow state model migration\"\"\"\nmode = \"stack\"\nprofile = \"codex\"\nbase_mode = \"explicit\""
        ));
        assert!(content.contains("base = \"main\""));
        assert!(content.contains("color = \"blue\""));
        assert!(content.contains("[origin]\nprovider = \"linear\"\nid = \"WT-123\""));
        assert!(!content.contains("objective ="));
        assert!(content.contains("[[tasks]]"));
        assert!(content.contains("task = \"add-schema\""));
        assert!(content.contains("run = \"stack-2026-05-16-001-add-schema\""));
        assert!(content.contains("parent = \"main\""));
        assert!(content.contains("[policy]"));
        assert!(content.contains("pull_request = \"draft\""));
        assert!(content.contains("landing = \"auto\""));
        assert!(!content.contains("branch ="));
        assert!(!content.contains("status ="));
        assert!(!content.contains("error ="));

        let parsed = read(&path).unwrap();
        assert_eq!(parsed, workflow);
    }

    #[test]
    fn read_rejects_invalid_and_missing_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        fs::write(
            &path,
            r#"mode = "queue"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("mode"));

        fs::write(
            &path,
            r#"base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("mode"));
    }

    #[test]
    fn read_rejects_ambiguous_task_row_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        fs::write(
            &path,
            r#"mode = "batch"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
status = "prepared"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("status"));

        fs::write(
            &path,
            r#"mode = "batch"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
branch = "feature/add-schema"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("branch"));
    }

    #[test]
    fn read_rejects_stack_task_fields_on_non_stack_modes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        fs::write(
            &path,
            r#"mode = "batch"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
parent = "main"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("parent"));

        fs::write(
            &path,
            r#"mode = "single"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
pull_request = "draft"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("pull_request"));
    }

    #[test]
    fn read_rejects_invalid_matrix_workflow_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        fs::write(
            &path,
            r#"mode = "matrix"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"

[[tasks.runs]]
profile = "alpha"
run = "workflow-add-schema-alpha"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("requires at least one profile"));

        fs::write(
            &path,
            r#"mode = "matrix"
profiles = ["alpha"]
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"

[[tasks.runs]]
profile = "alpha"
run = "workflow-add-schema-alpha"

[[tasks]]
task = "wire-api"

[[tasks.runs]]
profile = "alpha"
run = "workflow-wire-api-alpha"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("requires exactly one task"));

        fs::write(
            &path,
            r#"mode = "matrix"
profiles = ["alpha"]
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"

[[tasks.runs]]
profile = "alpha"
run = "workflow-add-schema-alpha"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("cannot store run"));

        fs::write(
            &path,
            r#"mode = "matrix"
profiles = ["alpha", "beta"]
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"

[[tasks.runs]]
profile = "alpha"
run = "workflow-add-schema-alpha"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("one run for each profile"));

        fs::write(
            &path,
            r#"mode = "matrix"
profiles = ["default"]
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"

[[tasks.runs]]
profile = "default"
run = "workflow-add-schema-default"
"#,
        )
        .unwrap();
        assert!(error_report(read(&path)).contains("reserved"));
    }

    #[test]
    fn read_rejects_boolean_pull_request_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        fs::write(
            &path,
            r#"mode = "stack"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
parent = "main"
pull_request = true
"#,
        )
        .unwrap();

        assert!(error_report(read(&path)).contains("pull_request"));
    }

    #[test]
    fn read_rejects_workflow_without_policy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        fs::write(
            &path,
            r#"mode = "stack"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
parent = "main"
"#,
        )
        .unwrap();

        assert!(error_report(read(&path)).contains("policy"));
    }

    #[test]
    fn read_rejects_non_canonical_metadata_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        for field in [
            "objective",
            "description",
            "goal_task",
            "parent_task",
            "subtasks",
        ] {
            fs::write(
                &path,
                format!(
                    r#"mode = "single"
{field} = "Ship the larger goal"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
"#
                ),
            )
            .unwrap();

            let error = error_report(read(&path));
            assert!(error.contains(field));
            if field == "objective" {
                assert!(
                    error.contains(
                        "rewrite it as top-level `title`, `body`, and optional `[origin]`"
                    )
                );
            }
        }
    }

    #[test]
    fn read_rejects_empty_title_body_and_origin_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workflow.toml");

        for (field, value, expected) in [
            ("title", "   ", "Workflow title cannot be empty"),
            ("body", "   ", "Workflow body cannot be empty"),
        ] {
            fs::write(
                &path,
                format!(
                    r#"{field} = "{value}"
mode = "single"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
"#
                ),
            )
            .unwrap();

            assert!(error_report(read(&path)).contains(expected));
        }

        for (origin_field, expected) in [
            (
                "provider = \"\"\nid = \"WT-123\"",
                "Workflow origin provider cannot be empty",
            ),
            (
                "provider = \"linear\"\nid = \"\"",
                "Workflow origin id cannot be empty",
            ),
        ] {
            fs::write(
                &path,
                format!(
                    r#"mode = "single"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[origin]
{origin_field}

[policy]
pull_request = "none"
landing = "manual"

[[tasks]]
task = "add-schema"
run = "workflow-add-schema"
"#
                ),
            )
            .unwrap();

            assert!(error_report(read(&path)).contains(expected));
        }
    }

    #[test]
    fn create_allocates_deterministic_workflow_colors_and_persists_them() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());

        let first = create(
            &ctx,
            WorkflowMetadata::new(
                WorkflowMode::Batch,
                "explicit",
                Some("main".into()),
                vec![task("schema", "workflow-schema")],
            ),
        )
        .unwrap();
        let second = create(
            &ctx,
            WorkflowMetadata::new(
                WorkflowMode::Batch,
                "explicit",
                Some("main".into()),
                vec![task("api", "workflow-api")],
            ),
        )
        .unwrap();

        assert_eq!(first.workflow.color.as_deref(), Some("red"));
        assert_eq!(second.workflow.color.as_deref(), Some("crimson"));
        assert_eq!(
            first.path.parent().unwrap(),
            dir.path().join(".wt/execution/workflows")
        );
        assert!(first.path < second.path);

        let first_content = fs::read_to_string(&first.path).unwrap();
        let second_content = fs::read_to_string(&second.path).unwrap();
        assert!(first_content.contains("color = \"red\""));
        assert!(second_content.contains("color = \"crimson\""));
        assert_eq!(read(&first.path).unwrap().color.as_deref(), Some("red"));
        assert_eq!(
            read(&second.path).unwrap().color.as_deref(),
            Some("crimson")
        );
    }

    #[test]
    fn create_keeps_explicit_workflow_color() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let mut workflow = WorkflowMetadata::new(
            WorkflowMode::Single,
            "explicit",
            Some("main".into()),
            vec![task("schema", "workflow-schema")],
        );
        workflow.color = Some("#ff00aa".into());

        let record = create(&ctx, workflow).unwrap();

        assert_eq!(record.workflow.color.as_deref(), Some("#ff00aa"));
        let content = fs::read_to_string(record.path).unwrap();
        assert!(content.contains("color = \"#ff00aa\""));
    }

    #[test]
    fn resolve_supports_latest_absolute_relative_and_shorthand_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        let old = workflows_dir.join("2026-05-16-001.toml");
        let new = workflows_dir.join("2026-05-16-002.toml");
        fs::write(&old, valid_workflow_toml("old")).unwrap();
        fs::write(&new, valid_workflow_toml("new")).unwrap();

        assert_eq!(resolve(&ctx, "latest").unwrap(), new);
        assert_eq!(resolve(&ctx, old.to_str().unwrap()).unwrap(), old);
        assert_eq!(
            resolve(&ctx, ".wt/execution/workflows/2026-05-16-001.toml").unwrap(),
            dir.path()
                .join(".wt/execution/workflows/2026-05-16-001.toml")
        );
        assert_eq!(resolve(&ctx, "2026-05-16-001").unwrap(), old);
    }

    #[test]
    fn workflow_paths_reject_legacy_local_workflows_without_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let legacy_dir = dir.path().join(".local/workflows");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("2026-05-16-001.toml"),
            valid_workflow_toml("old"),
        )
        .unwrap();

        let message = error_report_paths(workflow_paths(&ctx));

        assert!(message.contains("Found legacy Workflow storage"));
        assert!(
            message.contains("Canonical Workflow storage is <repo-root>/.wt/execution/workflows")
        );
        assert!(message.contains("does not silently read legacy Workflow storage"));
    }

    #[test]
    fn workflow_paths_reject_legacy_git_common_workflows_without_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let legacy_dir = dir.path().join(".git/wt/workflows");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("2026-x.toml"),
            valid_workflow_toml("legacy"),
        )
        .unwrap();

        let message = error_report_paths(workflow_paths(&ctx));

        assert!(message.contains("Found legacy Workflow storage"));
        assert!(message.contains(".git/wt/workflows"));
        assert!(
            message.contains("Canonical Workflow storage is <repo-root>/.wt/execution/workflows")
        );
        assert!(message.contains("does not silently read legacy Workflow storage"));
    }

    #[test]
    fn workflow_write_rejects_normalized_legacy_workflow_path() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let path = dir
            .path()
            .join(".wt/execution/../workflows/2026-05-16-001.toml");
        let mut workflow = WorkflowMetadata::new(
            WorkflowMode::Single,
            "explicit",
            Some("main".into()),
            vec![task("task", "run-task")],
        );

        let error = write(&ctx, &path, &mut workflow).unwrap_err();
        let report = format!("{error:#}");

        assert!(report.contains("Refusing to write legacy Workflow storage"));
        assert!(report.contains(".wt/execution/../workflows/2026-05-16-001.toml"));
        assert!(!dir.path().join(".wt/workflows").exists());
    }

    fn valid_workflow_toml(task_key: &str) -> String {
        format!(
            r#"mode = "single"
base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-16T00:00:00Z"
updated_at = "2026-05-16T00:00:00Z"

[[tasks]]
task = "{task_key}"
run = "workflow-{task_key}"
"#
        )
    }

    fn error_report(result: Result<WorkflowMetadata>) -> String {
        format!("{:#}", result.unwrap_err())
    }

    fn error_report_paths(result: Result<Vec<PathBuf>>) -> String {
        format!("{:#}", result.unwrap_err())
    }
}
