use crate::config::{
    AgentCli, Config, ConfigSource, IssueProviderType, SiteProvider, WorkflowDefaultLandingPolicy,
    WorkflowDefaultPullRequestMode,
};
use crate::config_render::render_effective_config;
use crate::context::{
    CmdOutput, CommandRunner, Ctx, CtxOptions, OutputMode, PromptItem, UserInterface,
};
use crate::task::{self, TaskDocument, TaskOrigin};
use crate::task_run::{self, TaskRunContext, TaskRunRecord, TaskRunStatus};
use crate::workflow::planner::runnable_workflow_info;
use crate::workflow::run::{
    WorkflowTaskState, read_batch_workflow_task_states, read_matrix_workflow_task_states,
    read_single_workflow_task_states, read_stack_workflow_task_states,
};
use crate::workflow::{self, WorkflowMetadata, WorkflowMode};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone)]
pub struct SnapshotState {
    repo_root: PathBuf,
    invocation_root: PathBuf,
    repo_name: String,
    config: Config,
    base_config: Config,
    config_source: ConfigSource,
}

impl SnapshotState {
    pub fn from_ctx(ctx: &Ctx) -> Self {
        Self {
            repo_root: ctx.repo_root.clone(),
            invocation_root: ctx.invocation_root.clone(),
            repo_name: ctx.repo_name.clone(),
            config: ctx.config.clone(),
            base_config: ctx.base_config.clone(),
            config_source: ctx.config_source.clone(),
        }
    }

    pub fn new(
        repo_root: PathBuf,
        invocation_root: PathBuf,
        repo_name: String,
        config: Config,
        base_config: Config,
        config_source: ConfigSource,
    ) -> Self {
        Self {
            repo_root,
            invocation_root,
            repo_name,
            config,
            base_config,
            config_source,
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx::new_with_options(
            self.repo_root.clone(),
            self.invocation_root.clone(),
            self.config.clone(),
            Box::new(NoopRunner),
            Box::new(NoopUi),
            CtxOptions {
                base_config: self.base_config.clone(),
                config_source: self.config_source.clone(),
                output_mode: OutputMode::Text,
                verbosity: 0,
                quiet: true,
            },
        )
    }
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    repo: RepoSummary,
    sources: SourceSummary,
    config: ConfigSummary,
    ideas: IdeaCollection,
    tasks: TaskCollection,
    workflows: WorkflowCollection,
    task_runs: TaskRunCollection,
    profiles: ProfileCollection,
    retrospecs: RetrospecCollection,
}

#[derive(Debug, Serialize)]
struct RepoSummary {
    name: String,
    root: String,
}

#[derive(Debug, Serialize)]
struct SourceSummary {
    ideas: String,
    tasks: String,
    workflows: String,
    task_runs: String,
    profiles: String,
    retrospecs: String,
    config_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ConfigSummary {
    source: String,
    paths: Vec<String>,
    effective_text: String,
    source_files: Vec<SourceFileSummary>,
    selected_profile: Option<String>,
    workflow: WorkflowDefaultSummary,
    agent: Option<String>,
    issues: Option<String>,
    site: Option<SiteSummary>,
    workspace: Option<WorkspaceSummary>,
}

#[derive(Debug, Serialize)]
struct SourceFileSummary {
    path: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct WorkflowDefaultSummary {
    pull_request: String,
    landing: String,
}

#[derive(Debug, Serialize)]
struct SiteSummary {
    provider: String,
    active: bool,
}

#[derive(Debug, Serialize)]
struct WorkspaceSummary {
    tab_count: usize,
    post_deps_tab_count: usize,
    open_url: Option<String>,
    open_browser: Option<bool>,
    color_count: usize,
}

#[derive(Debug, Serialize)]
struct IdeaCollection {
    items: Vec<IdeaSummary>,
    invalid: Vec<InvalidRecord>,
}

#[derive(Debug, Serialize)]
struct IdeaSummary {
    key: String,
    path: String,
    kind: String,
    title: String,
    status: Option<String>,
    source: Option<String>,
    tags: Vec<String>,
    updated_at: Option<String>,
    body_summary: Option<String>,
    body: Option<String>,
    source_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskCollection {
    items: Vec<TaskSummary>,
    invalid: Vec<InvalidRecord>,
}

#[derive(Debug, Serialize)]
struct TaskSummary {
    key: String,
    path: String,
    title: String,
    branch: Option<String>,
    origin: Option<TaskOriginSummary>,
    source: String,
    body_summary: Option<String>,
    body: Option<String>,
    source_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskOriginSummary {
    provider: String,
    id: String,
}

#[derive(Debug, Serialize)]
struct WorkflowCollection {
    items: Vec<WorkflowSummary>,
    invalid: Vec<InvalidRecord>,
}

#[derive(Debug, Serialize)]
struct WorkflowSummary {
    id: String,
    path: String,
    title: Option<String>,
    mode: String,
    presentation_group: String,
    body_summary: Option<String>,
    body: Option<String>,
    source_text: Option<String>,
    task_count: usize,
    task_runs: TaskRunCounts,
    task_run_groups: Vec<WorkflowTaskRunGroup>,
    runnable: RunnableSummary,
    base_mode: String,
    base: Option<String>,
    profile: Option<String>,
    profiles: Vec<String>,
    policy: WorkflowPolicySummary,
    updated_at: String,
    state_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowTaskRunGroup {
    status: String,
    items: Vec<TaskRunSummary>,
}

#[derive(Debug, Serialize)]
struct RunnableSummary {
    runnable: bool,
    runnable_count: usize,
    next_task: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskRunCounts {
    total: usize,
    prepared: usize,
    running: usize,
    done: usize,
    failed: usize,
    skipped: usize,
    missing: usize,
}

#[derive(Debug, Serialize)]
struct WorkflowPolicySummary {
    pull_request: String,
    landing: String,
}

#[derive(Debug, Serialize)]
struct TaskRunCollection {
    items: Vec<TaskRunSummary>,
    invalid: Vec<InvalidRecord>,
}

#[derive(Debug, Serialize)]
struct TaskRunSummary {
    id: String,
    path: String,
    task: String,
    branch: String,
    status: String,
    group: Option<String>,
    context: TaskRunContextSummary,
    error: Option<String>,
    creation_order: Option<u64>,
    created_at: String,
    updated_at: String,
    source_text: Option<String>,
    task_document: Option<TaskDocumentLinkSummary>,
    task_document_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskDocumentLinkSummary {
    key: String,
    path: String,
    title: String,
    branch: Option<String>,
    origin: Option<TaskOriginSummary>,
    body_summary: Option<String>,
    body: Option<String>,
    source_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskRunContextSummary {
    kind: String,
    label: String,
    workflow_id: Option<String>,
    workflow_path: Option<String>,
    mode: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProfileCollection {
    items: Vec<ProfileSummary>,
    invalid: Vec<InvalidRecord>,
}

#[derive(Debug, Serialize)]
struct ProfileSummary {
    name: String,
    path: String,
    copy_count: usize,
    link_count: usize,
    agent: String,
    has_site: bool,
    test_count: usize,
    source_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct RetrospecCollection {
    items: Vec<RetrospecSummary>,
    invalid: Vec<InvalidRecord>,
}

#[derive(Debug, Serialize)]
struct RetrospecSummary {
    key: String,
    path: String,
    kind: String,
    title: String,
    outcome: Option<String>,
    target: Option<String>,
    tags: Vec<String>,
    date: Option<String>,
    body_summary: Option<String>,
    body: Option<String>,
    source_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct InvalidRecord {
    key: String,
    path: String,
    error: String,
    source_text: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct IdeaToml {
    title: Option<String>,
    status: Option<String>,
    source: Option<String>,
    tags: Option<Vec<String>>,
    updated_at: Option<String>,
    body: Option<String>,
}

pub fn build(state: &SnapshotState) -> Result<Snapshot> {
    let ctx = state.ctx();
    Ok(Snapshot {
        repo: RepoSummary {
            name: state.repo_name.clone(),
            root: state.repo_root.display().to_string(),
        },
        sources: SourceSummary {
            ideas: ".local/ideas".into(),
            tasks: ".local/tasks".into(),
            workflows: ".local/workflows".into(),
            task_runs: ".local/task-runs".into(),
            profiles: ".local/profiles".into(),
            retrospecs: ".local/retrospectives".into(),
            config_paths: config_source_paths(&ctx),
        },
        config: config_summary(&ctx),
        ideas: collect_ideas(&ctx)?,
        tasks: collect_tasks(&ctx)?,
        workflows: collect_workflows(&ctx)?,
        task_runs: collect_task_runs(&ctx)?,
        profiles: collect_profiles(&ctx)?,
        retrospecs: collect_retrospecs(&ctx)?,
    })
}

fn collect_ideas(ctx: &Ctx) -> Result<IdeaCollection> {
    let mut items = Vec::new();
    let mut invalid = Vec::new();

    for path in idea_paths(ctx)? {
        let key = file_stem(&path).unwrap_or_else(|| "idea".into());
        let relative_path = relative_path(ctx, &path);
        match read_idea(ctx, &path) {
            Ok(summary) => items.push(summary),
            Err(err) => invalid.push(InvalidRecord {
                key,
                path: relative_path,
                error: format!("{err:#}"),
                source_text: read_known_source_text(ctx, &path),
            }),
        }
    }

    Ok(IdeaCollection { items, invalid })
}

fn idea_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let ideas_dir = ctx.repo_root.join(".local/ideas");
    if !ideas_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in
        fs::read_dir(&ideas_dir).with_context(|| "Failed to read idea directory: .local/ideas")?
    {
        let path = entry?.path();
        let ext = path.extension().and_then(|ext| ext.to_str());
        if matches!(ext, Some("toml" | "md" | "markdown")) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_idea(ctx: &Ctx, path: &Path) -> Result<IdeaSummary> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => read_toml_idea(ctx, path),
        Some("md" | "markdown") => read_markdown_idea(ctx, path),
        _ => bail!("Unsupported idea file type: {}", path.display()),
    }
}

fn read_toml_idea(ctx: &Ctx, path: &Path) -> Result<IdeaSummary> {
    let relative_path = relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read idea: {relative_path}"))?;
    let idea: IdeaToml = toml::from_str(&content)
        .with_context(|| format!("Failed to parse idea: {relative_path}"))?;
    let key = file_stem(path).unwrap_or_else(|| "idea".into());
    let title = non_empty_string(idea.title).unwrap_or_else(|| key.clone());
    Ok(IdeaSummary {
        key,
        path: relative_path,
        kind: "toml".into(),
        title,
        status: non_empty_string(idea.status),
        source: non_empty_string(idea.source),
        tags: idea.tags.unwrap_or_default(),
        updated_at: non_empty_string(idea.updated_at),
        body_summary: idea.body.as_deref().and_then(body_summary),
        body: non_empty_string(idea.body),
        source_text: non_empty_body(&content),
    })
}

fn read_markdown_idea(ctx: &Ctx, path: &Path) -> Result<IdeaSummary> {
    let relative_path = relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read idea: {relative_path}"))?;
    let key = file_stem(path).unwrap_or_else(|| "idea".into());
    let title = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| key.clone());
    Ok(IdeaSummary {
        key,
        path: relative_path,
        kind: "markdown".into(),
        title,
        status: None,
        source: None,
        tags: Vec::new(),
        updated_at: None,
        body_summary: body_summary(&content),
        body: non_empty_body(&content),
        source_text: non_empty_body(&content),
    })
}

fn collect_tasks(ctx: &Ctx) -> Result<TaskCollection> {
    let mut items = Vec::new();
    let mut invalid = Vec::new();

    for path in task::task_document_paths(ctx)? {
        let key = task::task_key_from_path(&path).unwrap_or_else(|_| "task".into());
        let relative_path = relative_path(ctx, &path);
        match task::read_task_document_path(ctx, &path) {
            Ok(selected) => items.push(task_summary(
                selected.key,
                selected.path,
                selected.content,
                selected.document,
            )),
            Err(err) => invalid.push(InvalidRecord {
                key,
                path: relative_path,
                error: format!("{err:#}"),
                source_text: read_known_source_text(ctx, &path),
            }),
        }
    }

    Ok(TaskCollection { items, invalid })
}

fn task_summary(key: String, path: String, content: String, document: TaskDocument) -> TaskSummary {
    let origin = document.origin.as_ref().map(task_origin_summary);
    TaskSummary {
        key: key.clone(),
        path,
        title: document.title_or_key(&key),
        branch: task::prepared_branch_name(&document.branch).map(str::to_string),
        source: if origin.is_some() {
            "provider-origin".into()
        } else {
            "local".into()
        },
        origin,
        body_summary: body_summary(&document.body),
        body: non_empty_body(&document.body),
        source_text: non_empty_body(&content),
    }
}

fn task_origin_summary(origin: &TaskOrigin) -> TaskOriginSummary {
    TaskOriginSummary {
        provider: origin.provider.clone(),
        id: origin.id.clone(),
    }
}

fn collect_workflows(ctx: &Ctx) -> Result<WorkflowCollection> {
    let mut items = Vec::new();
    let mut invalid = Vec::new();

    for path in workflow::workflow_paths(ctx)? {
        let id = workflow::id_from_path(&path).unwrap_or_else(|_| "workflow".into());
        let relative_path = relative_path(ctx, &path);
        match workflow::read(&path) {
            Ok(metadata) => items.push(workflow_summary(ctx, &path, id, metadata)),
            Err(err) => invalid.push(InvalidRecord {
                key: id,
                path: relative_path,
                error: format!("{err:#}"),
                source_text: read_known_source_text(ctx, &path),
            }),
        }
    }

    Ok(WorkflowCollection { items, invalid })
}

fn workflow_summary(
    ctx: &Ctx,
    path: &Path,
    id: String,
    metadata: WorkflowMetadata,
) -> WorkflowSummary {
    let counts = workflow_task_run_counts(ctx, &metadata);
    let (runnable, state_error) = workflow_runnable(ctx, path, &metadata);
    let presentation_group = workflow_presentation_group(&counts, &runnable, state_error.as_ref());
    let task_run_groups = workflow_task_run_groups(ctx, &metadata);

    WorkflowSummary {
        id,
        path: relative_path(ctx, path),
        title: metadata.title,
        mode: metadata.mode.as_str().into(),
        presentation_group,
        body_summary: metadata.body.as_deref().and_then(short_summary),
        body: non_empty_string(metadata.body),
        source_text: read_known_source_text(ctx, path),
        task_count: metadata.tasks.len(),
        task_runs: counts,
        task_run_groups,
        runnable,
        base_mode: metadata.base_mode,
        base: metadata.base,
        profile: metadata.profile,
        profiles: metadata.profiles,
        policy: WorkflowPolicySummary {
            pull_request: metadata.policy.pull_request.as_str().into(),
            landing: metadata.policy.landing.as_str().into(),
        },
        updated_at: metadata.updated_at,
        state_error,
    }
}

fn workflow_runnable(
    ctx: &Ctx,
    path: &Path,
    metadata: &WorkflowMetadata,
) -> (RunnableSummary, Option<String>) {
    match read_workflow_states(ctx, path, metadata) {
        Ok(states) => {
            if let Some(info) = runnable_workflow_info(&metadata.mode, &states) {
                let next_task = info
                    .next_idx
                    .and_then(|idx| states.get(idx))
                    .map(|state| state.row.task.clone());
                (
                    RunnableSummary {
                        runnable: true,
                        runnable_count: info.runnable_count,
                        next_task,
                    },
                    None,
                )
            } else {
                (
                    RunnableSummary {
                        runnable: false,
                        runnable_count: 0,
                        next_task: None,
                    },
                    None,
                )
            }
        }
        Err(err) => (
            RunnableSummary {
                runnable: false,
                runnable_count: 0,
                next_task: None,
            },
            Some(format!("{err:#}")),
        ),
    }
}

fn read_workflow_states(
    ctx: &Ctx,
    path: &Path,
    metadata: &WorkflowMetadata,
) -> Result<Vec<WorkflowTaskState>> {
    match metadata.mode {
        WorkflowMode::Single => read_single_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Batch => read_batch_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Stack => read_stack_workflow_task_states(ctx, path, metadata),
        WorkflowMode::Matrix => read_matrix_workflow_task_states(ctx, path, metadata),
    }
}

fn workflow_task_run_counts(ctx: &Ctx, metadata: &WorkflowMetadata) -> TaskRunCounts {
    let mut counts = TaskRunCounts {
        total: workflow_run_ids(metadata).len(),
        prepared: 0,
        running: 0,
        done: 0,
        failed: 0,
        skipped: 0,
        missing: 0,
    };

    for run_id in workflow_run_ids(metadata) {
        match task_run::resolve(ctx, &run_id).and_then(|path| task_run::read(&path)) {
            Ok(run) => increment_status_count(&mut counts, run.status),
            Err(_) => counts.missing += 1,
        }
    }

    counts
}

fn workflow_task_run_groups(ctx: &Ctx, metadata: &WorkflowMetadata) -> Vec<WorkflowTaskRunGroup> {
    let runs = workflow_run_ids(metadata)
        .into_iter()
        .filter_map(|run_id| {
            let path = task_run::resolve(ctx, &run_id).ok()?;
            let run = task_run::read(&path).ok()?;
            let id = task_run::id_from_path(&path).unwrap_or(run_id);
            let record = TaskRunRecord { id, path, run };
            Some(task_run_summary(ctx, &record))
        })
        .collect();
    grouped_task_runs(runs)
}

fn grouped_task_runs(mut runs: Vec<TaskRunSummary>) -> Vec<WorkflowTaskRunGroup> {
    runs.sort_by(|left, right| {
        task_run_status_order(&left.status)
            .cmp(&task_run_status_order(&right.status))
            .then_with(|| {
                right
                    .creation_order
                    .unwrap_or_default()
                    .cmp(&left.creation_order.unwrap_or_default())
            })
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut groups: Vec<WorkflowTaskRunGroup> = Vec::new();
    for run in runs {
        if let Some(group) = groups.last_mut()
            && group.status == run.status
        {
            group.items.push(run);
        } else {
            groups.push(WorkflowTaskRunGroup {
                status: run.status.clone(),
                items: vec![run],
            });
        }
    }
    groups
}

fn task_run_status_order(status: &str) -> usize {
    match status {
        "failed" => 0,
        "running" => 1,
        "prepared" => 2,
        "skipped" => 3,
        "done" => 4,
        _ => 5,
    }
}

fn workflow_run_ids(metadata: &WorkflowMetadata) -> Vec<String> {
    if matches!(metadata.mode, WorkflowMode::Matrix) {
        return metadata
            .tasks
            .iter()
            .flat_map(|row| row.runs.iter().map(|run| run.run.clone()))
            .collect();
    }
    metadata.tasks.iter().map(|row| row.run.clone()).collect()
}

fn increment_status_count(counts: &mut TaskRunCounts, status: TaskRunStatus) {
    match status {
        TaskRunStatus::Prepared => counts.prepared += 1,
        TaskRunStatus::Running => counts.running += 1,
        TaskRunStatus::Done => counts.done += 1,
        TaskRunStatus::Failed => counts.failed += 1,
        TaskRunStatus::Skipped => counts.skipped += 1,
    }
}

fn workflow_presentation_group(
    counts: &TaskRunCounts,
    runnable: &RunnableSummary,
    state_error: Option<&String>,
) -> String {
    if state_error.is_some() {
        return "state_error".into();
    }
    if runnable.runnable {
        return "runnable".into();
    }
    if counts.total > 0 && counts.done + counts.skipped == counts.total {
        return "done".into();
    }
    "waiting".into()
}

fn collect_task_runs(ctx: &Ctx) -> Result<TaskRunCollection> {
    let mut items = Vec::new();
    let mut invalid = Vec::new();

    for path in task_run::task_run_paths(ctx)? {
        let id = task_run::id_from_path(&path).unwrap_or_else(|_| "task-run".into());
        let relative_path = relative_path(ctx, &path);
        match task_run::read(&path) {
            Ok(run) => {
                let record = TaskRunRecord { id, path, run };
                items.push(task_run_summary(ctx, &record));
            }
            Err(err) => invalid.push(InvalidRecord {
                key: id,
                path: relative_path,
                error: format!("{err:#}"),
                source_text: read_known_source_text(ctx, &path),
            }),
        }
    }

    Ok(TaskRunCollection { items, invalid })
}

fn task_run_summary(ctx: &Ctx, record: &TaskRunRecord) -> TaskRunSummary {
    let (task_document, task_document_error) = linked_task_document(ctx, &record.run.task);
    TaskRunSummary {
        id: record.id.clone(),
        path: relative_path(ctx, &record.path),
        task: record.run.task.clone(),
        branch: record.run.branch.clone(),
        status: record.run.status.as_str().into(),
        group: record.run.group.clone(),
        context: task_run_context_summary(ctx, record),
        error: record.run.error.clone(),
        creation_order: record.run.creation_order,
        created_at: record.run.created_at.clone(),
        updated_at: record.run.updated_at.clone(),
        source_text: read_known_source_text(ctx, &record.path),
        task_document,
        task_document_error,
    }
}

fn linked_task_document(
    ctx: &Ctx,
    task_key: &str,
) -> (Option<TaskDocumentLinkSummary>, Option<String>) {
    let key = task::safe_task_key(task_key);
    match task::read_task_file(ctx, &key) {
        Ok((document, path, content)) => {
            let origin = document.origin.as_ref().map(task_origin_summary);
            (
                Some(TaskDocumentLinkSummary {
                    key: key.clone(),
                    path,
                    title: document.title_or_key(&key),
                    branch: task::prepared_branch_name(&document.branch).map(str::to_string),
                    origin,
                    body_summary: body_summary(&document.body),
                    body: non_empty_body(&document.body),
                    source_text: non_empty_body(&content),
                }),
                None,
            )
        }
        Err(err) => (None, Some(format!("{err:#}"))),
    }
}

fn task_run_context_summary(ctx: &Ctx, record: &TaskRunRecord) -> TaskRunContextSummary {
    match task_run::resolve_context(ctx, record) {
        Ok(TaskRunContext::Direct) => TaskRunContextSummary {
            kind: "direct".into(),
            label: "direct".into(),
            workflow_id: None,
            workflow_path: None,
            mode: None,
            error: None,
        },
        Ok(TaskRunContext::WorkflowLinked(context)) => TaskRunContextSummary {
            kind: "workflow".into(),
            label: format!(
                "workflow {} mode {}",
                context.workflow_id,
                context.mode.as_str()
            ),
            workflow_id: Some(context.workflow_id),
            workflow_path: Some(relative_path(ctx, &context.workflow_path)),
            mode: Some(context.mode.as_str().into()),
            error: None,
        },
        Ok(TaskRunContext::UnresolvedWorkflowGroup { group }) => TaskRunContextSummary {
            kind: "unresolved_workflow".into(),
            label: format!("workflow group {group} (not discovered)"),
            workflow_id: Some(group),
            workflow_path: None,
            mode: None,
            error: None,
        },
        Err(err) => TaskRunContextSummary {
            kind: "error".into(),
            label: "context unavailable".into(),
            workflow_id: record.run.group.clone(),
            workflow_path: None,
            mode: None,
            error: Some(format!("{err:#}")),
        },
    }
}

fn collect_profiles(ctx: &Ctx) -> Result<ProfileCollection> {
    let inventory = Config::load_profile_inventory(&ctx.repo_root, &ctx.base_config)?;
    let items = inventory
        .profiles
        .into_iter()
        .map(|profile| ProfileSummary {
            name: profile.name,
            path: relative_path(ctx, &profile.path),
            copy_count: profile.config.worktree.copy.len() + profile.config.worktree.copy_as.len(),
            link_count: profile.config.worktree.link.len(),
            agent: profile
                .config
                .agent
                .as_ref()
                .map(|agent| agent_cli_name(&agent.cli))
                .unwrap_or("none")
                .into(),
            has_site: profile.config.has_site(),
            test_count: profile
                .config
                .test
                .map(|test| test.commands.len())
                .unwrap_or(0),
            source_text: read_known_source_text(ctx, &profile.path),
        })
        .collect();
    let invalid = inventory
        .invalid_profiles
        .into_iter()
        .map(|profile| InvalidRecord {
            key: profile.name,
            path: relative_path(ctx, &profile.path),
            error: profile.error,
            source_text: read_known_source_text(ctx, &profile.path),
        })
        .collect();

    Ok(ProfileCollection { items, invalid })
}

fn collect_retrospecs(ctx: &Ctx) -> Result<RetrospecCollection> {
    let mut items = Vec::new();
    let mut invalid = Vec::new();

    for path in retrospec_paths(ctx)? {
        let key = file_stem(&path).unwrap_or_else(|| "retrospec".into());
        let relative_path = relative_path(ctx, &path);
        match read_retrospec(ctx, &path) {
            Ok(summary) => items.push(summary),
            Err(err) => invalid.push(InvalidRecord {
                key,
                path: relative_path,
                error: format!("{err:#}"),
                source_text: read_known_source_text(ctx, &path),
            }),
        }
    }

    Ok(RetrospecCollection { items, invalid })
}

fn retrospec_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let retrospecs_dir = ctx.repo_root.join(".local/retrospectives");
    if !retrospecs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&retrospecs_dir)
        .with_context(|| "Failed to read retrospec directory: .local/retrospectives")?
    {
        let path = entry?.path();
        let ext = path.extension().and_then(|ext| ext.to_str());
        if matches!(ext, Some("toml" | "md" | "markdown")) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_retrospec(ctx: &Ctx, path: &Path) -> Result<RetrospecSummary> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => read_toml_retrospec(ctx, path),
        Some("md" | "markdown") => read_markdown_retrospec(ctx, path),
        _ => bail!("Unsupported retrospec file type: {}", path.display()),
    }
}

fn read_toml_retrospec(ctx: &Ctx, path: &Path) -> Result<RetrospecSummary> {
    let relative_path = relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read retrospec: {relative_path}"))?;
    let value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse retrospec: {relative_path}"))?;
    let key = file_stem(path).unwrap_or_else(|| "retrospec".into());
    let title = toml_string(&value, "title").unwrap_or_else(|| key.clone());
    let body = retrospec_body(&value);
    Ok(RetrospecSummary {
        key,
        path: relative_path,
        kind: "toml".into(),
        title,
        outcome: toml_string(&value, "outcome"),
        target: toml_string(&value, "target"),
        tags: toml_string_array(&value, "tags"),
        date: toml_string(&value, "date"),
        body_summary: body.as_deref().and_then(body_summary),
        body,
        source_text: non_empty_body(&content),
    })
}

fn read_markdown_retrospec(ctx: &Ctx, path: &Path) -> Result<RetrospecSummary> {
    let relative_path = relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read retrospec: {relative_path}"))?;
    let key = file_stem(path).unwrap_or_else(|| "retrospec".into());
    let title = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| key.clone());
    Ok(RetrospecSummary {
        key,
        path: relative_path,
        kind: "markdown".into(),
        title,
        outcome: None,
        target: None,
        tags: Vec::new(),
        date: None,
        body_summary: body_summary(&content),
        body: non_empty_body(&content),
        source_text: non_empty_body(&content),
    })
}

fn retrospec_body(value: &toml::Value) -> Option<String> {
    let mut blocks = Vec::new();

    if let Some(context) = value.get("context").and_then(toml::Value::as_table) {
        let mut lines = Vec::new();
        for key in ["goal", "scope", "integration_branch"] {
            if let Some(text) = table_string(context, key) {
                lines.push(format!("{}: {text}", label(key)));
            }
        }
        if !lines.is_empty() {
            blocks.push(format!("Context\n{}", lines.join("\n")));
        }
    }

    for section in ["keep", "problem", "try"] {
        if let Some(table) = value.get(section).and_then(toml::Value::as_table) {
            let items = table_string_array(table, "items");
            if !items.is_empty() {
                blocks.push(format!(
                    "{}\n{}",
                    label(section),
                    items
                        .into_iter()
                        .map(|item| format!("- {item}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
    }

    if let Some(candidates) = value
        .get("action_candidates")
        .and_then(toml::Value::as_array)
    {
        let items = candidates
            .iter()
            .filter_map(toml::Value::as_table)
            .filter_map(|table| table_string(table, "summary"))
            .map(|summary| format!("- {summary}"))
            .collect::<Vec<_>>();
        if !items.is_empty() {
            blocks.push(format!("Action candidates\n{}", items.join("\n")));
        }
    }

    non_empty_body(&blocks.join("\n\n"))
}

fn toml_string(value: &toml::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .and_then(|value| non_empty_string(Some(value.to_string())))
}

fn toml_string_array(value: &toml::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .filter_map(|value| non_empty_string(Some(value.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn table_string(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .and_then(|value| non_empty_string(Some(value.to_string())))
}

fn table_string_array(table: &toml::map::Map<String, toml::Value>, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .filter_map(|value| non_empty_string(Some(value.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn config_summary(ctx: &Ctx) -> ConfigSummary {
    let policy = ctx.config.workflow_default_policy();
    ConfigSummary {
        source: config_source_label(&ctx.config_source).into(),
        paths: config_source_paths(ctx),
        effective_text: render_effective_config(&ctx.config),
        source_files: config_source_files(ctx),
        selected_profile: ctx
            .base_config
            .profile
            .as_ref()
            .and_then(|profile| profile.name.clone()),
        workflow: WorkflowDefaultSummary {
            pull_request: workflow_default_pull_request(policy.pull_request).into(),
            landing: workflow_default_landing(policy.landing).into(),
        },
        agent: ctx
            .config
            .agent
            .as_ref()
            .map(|agent| agent_cli_name(&agent.cli).into()),
        issues: ctx
            .config
            .issues
            .as_ref()
            .map(|issues| match issues.provider {
                IssueProviderType::Linear => "linear".into(),
                IssueProviderType::Github => "github".into(),
            }),
        site: ctx.config.site.as_ref().map(|site| SiteSummary {
            provider: site_provider_name(&site.provider).into(),
            active: ctx.config.has_site(),
        }),
        workspace: ctx
            .config
            .workspace
            .as_ref()
            .map(|workspace| WorkspaceSummary {
                tab_count: workspace.tabs.len(),
                post_deps_tab_count: workspace.post_deps_tabs.len(),
                open_url: workspace.open_url.clone(),
                open_browser: workspace.open_browser,
                color_count: workspace.effective_colors().len(),
            }),
    }
}

fn config_source_label(source: &ConfigSource) -> &'static str {
    match source {
        ConfigSource::Default => "default",
        ConfigSource::File(_) => "file",
        ConfigSource::Files(_) => "files",
    }
}

fn config_source_paths(ctx: &Ctx) -> Vec<String> {
    match &ctx.config_source {
        ConfigSource::Default => Vec::new(),
        ConfigSource::File(path) => vec![relative_path(ctx, path)],
        ConfigSource::Files(paths) => paths.iter().map(|path| relative_path(ctx, path)).collect(),
    }
}

fn config_source_files(ctx: &Ctx) -> Vec<SourceFileSummary> {
    config_source_path_bufs(&ctx.config_source)
        .into_iter()
        .filter_map(|path| {
            read_known_source_text(ctx, &path).map(|text| SourceFileSummary {
                path: relative_path(ctx, &path),
                text,
            })
        })
        .collect()
}

fn config_source_path_bufs(source: &ConfigSource) -> Vec<PathBuf> {
    match source {
        ConfigSource::Default => Vec::new(),
        ConfigSource::File(path) => vec![path.clone()],
        ConfigSource::Files(paths) => paths.clone(),
    }
}

fn workflow_default_pull_request(mode: WorkflowDefaultPullRequestMode) -> &'static str {
    match mode {
        WorkflowDefaultPullRequestMode::None => "none",
        WorkflowDefaultPullRequestMode::Draft => "draft",
        WorkflowDefaultPullRequestMode::Ready => "ready",
    }
}

fn workflow_default_landing(policy: WorkflowDefaultLandingPolicy) -> &'static str {
    match policy {
        WorkflowDefaultLandingPolicy::Manual => "manual",
        WorkflowDefaultLandingPolicy::Auto => "auto",
    }
}

fn agent_cli_name(cli: &AgentCli) -> &'static str {
    match cli {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::None => "none",
    }
}

fn site_provider_name(provider: &SiteProvider) -> &'static str {
    match provider {
        SiteProvider::None => "none",
        SiteProvider::Herd => "herd",
        SiteProvider::Valet => "valet",
        SiteProvider::DockerProxy => "docker-proxy",
        SiteProvider::Traefik => "traefik",
    }
}

fn relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map(str::to_string)
}

fn label(value: &str) -> String {
    value
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn non_empty_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn non_empty_body(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn read_known_source_text(ctx: &Ctx, path: &Path) -> Option<String> {
    let relative = relative_path(ctx, path);
    if !is_known_state_or_config_path(&relative) {
        return None;
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|content| non_empty_body(&content))
}

fn is_known_state_or_config_path(relative: &str) -> bool {
    relative == ".wt.toml"
        || relative == ".local/.wt.toml"
        || (relative.starts_with(".local/ideas/")
            && matches!(
                Path::new(relative).extension().and_then(|ext| ext.to_str()),
                Some("toml" | "md" | "markdown")
            ))
        || (relative.starts_with(".local/retrospectives/")
            && matches!(
                Path::new(relative).extension().and_then(|ext| ext.to_str()),
                Some("toml" | "md" | "markdown")
            ))
        || (relative.starts_with(".local/tasks/") && relative.ends_with(".toml"))
        || (relative.starts_with(".local/workflows/") && relative.ends_with(".toml"))
        || (relative.starts_with(".local/task-runs/") && relative.ends_with(".toml"))
        || (relative.starts_with(".local/profiles/") && relative.ends_with("/profile.toml"))
}

fn body_summary(value: &str) -> Option<String> {
    short_summary(value)
}

fn short_summary(value: &str) -> Option<String> {
    let summary = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        None
    } else {
        Some(truncate_chars(&summary, 140))
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

struct NoopUi;

impl UserInterface for NoopUi {
    fn select(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
        bail!("local UI snapshot cannot prompt")
    }

    fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
        bail!("local UI snapshot cannot prompt")
    }

    fn can_prompt(&self) -> bool {
        false
    }

    fn select_items(&self, _prompt: &str, _items: &[PromptItem]) -> Result<usize> {
        bail!("local UI snapshot cannot prompt")
    }

    fn multi_select_items(&self, _prompt: &str, _items: &[PromptItem]) -> Result<Vec<usize>> {
        bail!("local UI snapshot cannot prompt")
    }

    fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
        bail!("local UI snapshot cannot prompt")
    }

    fn input(&self, _prompt: &str, _default: Option<&str>) -> Result<String> {
        bail!("local UI snapshot cannot prompt")
    }

    fn print_step(&self, _msg: &str) {}
    fn print_dim(&self, _msg: &str) {}
    fn print_warning(&self, _msg: &str) {}
    fn print_error(&self, _msg: &str) {}
}

struct NoopRunner;

impl CommandRunner for NoopRunner {
    fn run(&self, _cmd: &str, _args: &[&str], _cwd: Option<&Path>) -> Result<CmdOutput> {
        bail!("local UI snapshot cannot run commands")
    }

    fn run_with_timeout(
        &self,
        _cmd: &str,
        _args: &[&str],
        _cwd: Option<&Path>,
        _timeout: Duration,
    ) -> Result<CmdOutput> {
        bail!("local UI snapshot cannot run commands")
    }

    fn has_command(&self, _cmd: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentCli, AgentConfig, ReadyMode, SubmitMode, WorkflowDefaultLandingPolicy,
        WorkflowDefaultPullRequestMode,
    };
    use std::collections::HashMap;

    #[test]
    fn snapshot_reports_valid_and_invalid_records() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "demo",
            "title = \"Demo\"\nbranch = \"feature/demo\"\nbody = \"Demo body\"\n",
        );
        write_task(dir.path(), "bad", "unknown = true\n");
        write_task_run(
            dir.path(),
            "run-demo",
            "task = \"demo\"\nbranch = \"feature/demo\"\nstatus = \"prepared\"\ncreation_order = 1\ncreated_at = \"2026-05-18T00:00:00Z\"\nupdated_at = \"2026-05-18T00:00:00Z\"\n",
        );
        write_task_run(
            dir.path(),
            "run-bad",
            "task = \"demo\"\nbranch = \"feature/demo\"\nstatus = \"unknown\"\ncreated_at = \"2026-05-18T00:00:00Z\"\nupdated_at = \"2026-05-18T00:00:00Z\"\n",
        );
        write_workflow(
            dir.path(),
            "2026-05-18-001",
            "title = \"Workflow demo\"\nbody = \"Workflow body\"\nmode = \"batch\"\nbase_mode = \"explicit\"\nbase = \"main\"\ncreated_at = \"2026-05-18T00:00:00Z\"\nupdated_at = \"2026-05-18T00:00:00Z\"\n\n[policy]\npull_request = \"none\"\nlanding = \"manual\"\n\n[[tasks]]\ntask = \"demo\"\nrun = \"run-demo\"\n",
        );
        write_workflow(dir.path(), "bad", "mode = \"batch\"\n");
        write_idea(
            dir.path(),
            "idea",
            "title = \"Idea\"\nstatus = \"ready\"\ntags = [\"ui\"]\nbody = \"Idea body\"\n",
        );
        write_idea(dir.path(), "bad", "title = [\n");
        write_profile(dir.path(), "codex", "[agent]\ncli = \"codex\"\n");
        write_profile(dir.path(), "bad name", "[agent]\ncli = \"codex\"\n");
        write_retrospec(
            dir.path(),
            "retro",
            "title = \"Retro\"\ndate = \"2026-05-18\"\noutcome = \"landed\"\ntarget = \"demo\"\ntags = [\"ui\"]\n\n[context]\ngoal = \"Retro goal\"\n\n[keep]\nitems = [\"Keep this\"]\n",
        );
        write_retrospec(dir.path(), "bad", "title = [\n");
        fs::write(
            dir.path().join(".wt.toml"),
            "[workflow]\npull_request = \"ready\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".local")).unwrap();
        fs::write(
            dir.path().join(".local/.wt.toml"),
            "[workflow]\nlanding = \"auto\"\n",
        )
        .unwrap();

        let mut config = Config::default();
        config.workflow.pull_request = Some(WorkflowDefaultPullRequestMode::Ready);
        config.workflow.landing = Some(WorkflowDefaultLandingPolicy::Auto);
        config.agent = Some(AgentConfig {
            cli: AgentCli::Codex,
            args: Vec::new(),
            command: None,
            ready: ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 15,
            send_after: 3,
            prompt: HashMap::new(),
        });
        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            config,
            Config::default(),
            ConfigSource::Files(vec![
                dir.path().join(".wt.toml"),
                dir.path().join(".local/.wt.toml"),
            ]),
        );

        let snapshot = build(&state).unwrap();
        assert_eq!(snapshot.tasks.items.len(), 1);
        assert_eq!(snapshot.tasks.invalid.len(), 1);
        assert_eq!(snapshot.workflows.items.len(), 1);
        assert_eq!(snapshot.workflows.invalid.len(), 1);
        assert_eq!(snapshot.task_runs.items.len(), 1);
        assert_eq!(snapshot.task_runs.invalid.len(), 1);
        assert_eq!(snapshot.ideas.items.len(), 1);
        assert_eq!(snapshot.ideas.invalid.len(), 1);
        assert_eq!(snapshot.profiles.items.len(), 1);
        assert_eq!(snapshot.profiles.invalid.len(), 1);
        assert_eq!(snapshot.retrospecs.items.len(), 1);
        assert_eq!(snapshot.retrospecs.invalid.len(), 1);
        assert_eq!(snapshot.config.workflow.pull_request, "ready");
        assert_eq!(snapshot.config.workflow.landing, "auto");
        assert_eq!(snapshot.config.agent.as_deref(), Some("codex"));
        assert_eq!(
            snapshot.sources.config_paths,
            vec![".wt.toml", ".local/.wt.toml"]
        );
        assert_eq!(snapshot.tasks.items[0].path, ".local/tasks/demo.toml");
        assert_eq!(snapshot.tasks.items[0].body.as_deref(), Some("Demo body"));
        assert!(
            snapshot.tasks.items[0]
                .source_text
                .as_deref()
                .unwrap()
                .contains("branch = \"feature/demo\"")
        );
        assert_eq!(snapshot.ideas.items[0].body.as_deref(), Some("Idea body"));
        assert!(
            snapshot.task_runs.items[0]
                .source_text
                .as_deref()
                .unwrap()
                .contains("status = \"prepared\"")
        );
        assert_eq!(
            snapshot.task_runs.items[0]
                .task_document
                .as_ref()
                .map(|document| document.title.as_str()),
            Some("Demo")
        );
        assert_eq!(
            snapshot.workflows.items[0].task_run_groups[0].status,
            "prepared"
        );
        assert_eq!(
            snapshot.workflows.items[0].task_run_groups[0].items[0]
                .task_document
                .as_ref()
                .map(|document| document.body.as_deref()),
            Some(Some("Demo body"))
        );
        assert_eq!(
            snapshot.workflows.items[0].body.as_deref(),
            Some("Workflow body")
        );
        assert!(
            snapshot.workflows.items[0]
                .source_text
                .as_deref()
                .unwrap()
                .contains("[[tasks]]")
        );
        assert!(
            snapshot.profiles.items[0]
                .source_text
                .as_deref()
                .unwrap()
                .contains("cli = \"codex\"")
        );
        assert!(snapshot.config.effective_text.contains("[workflow]"));
        assert_eq!(snapshot.config.source_files.len(), 2);
        assert_eq!(
            snapshot.retrospecs.items[0].body.as_deref(),
            Some("Context\nGoal: Retro goal\n\nKeep\n- Keep this")
        );
        assert_eq!(
            snapshot.workflows.items[0].presentation_group,
            "state_error"
        );
    }

    fn write_task(root: &Path, name: &str, content: &str) {
        let dir = root.join(".local/tasks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_task_run(root: &Path, name: &str, content: &str) {
        let dir = root.join(".local/task-runs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_workflow(root: &Path, name: &str, content: &str) {
        let dir = root.join(".local/workflows");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_idea(root: &Path, name: &str, content: &str) {
        let dir = root.join(".local/ideas");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_profile(root: &Path, name: &str, content: &str) {
        let dir = root.join(".local/profiles").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("profile.toml"), content).unwrap();
    }

    fn write_retrospec(root: &Path, name: &str, content: &str) {
        let dir = root.join(".local/retrospectives");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }
}
