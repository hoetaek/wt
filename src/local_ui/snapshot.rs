use crate::config::{
    AgentCli, AgentConfig, Config, ConfigSource, EditorPlacement, IssueProviderType, ReadyMode,
    ReviewCodexBasePolicy, SetupConfig, SiteProvider, SubmitMode, WorkflowDefaultLandingPolicy,
    WorkflowDefaultPullRequestMode, WorkspaceConfig, WorktreeConfig,
};
use crate::config_render::render_effective_config;
use crate::context::{
    CmdOutput, CommandRunner, Ctx, CtxOptions, OutputMode, PromptItem, UserInterface,
};
use crate::storage::{LegacyLocalStorage, StorageRoot};
use crate::task::{self, TaskDocument, TaskOrigin};
use crate::task_run::{self, TaskRunContext, TaskRunRecord, TaskRunStatus};
use crate::workflow::planner::runnable_workflow_info;
use crate::workflow::render::workflow_title_label;
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
    storage_root: StorageRoot,
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
            storage_root: ctx.storage_root.clone(),
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
        let storage_root = StorageRoot::from_git_common_dir(repo_root.join(".git"));
        Self {
            repo_root,
            invocation_root,
            repo_name,
            config,
            base_config,
            config_source,
            storage_root,
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
                storage_root: Some(self.storage_root.clone()),
                output_mode: OutputMode::Text,
                verbosity: 0,
                quiet: true,
                launcher_coordinator_id: None,
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
    worktree: Option<WorktreeSummary>,
    setup: Option<SetupSummary>,
    workflow: WorkflowDefaultSummary,
    review: ReviewSummary,
    issues: Option<IssuesSummary>,
    site: Option<SiteSummary>,
    editor: Option<EditorSummary>,
    workspace: Option<WorkspaceSummary>,
    agent: Option<AgentSummary>,
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
struct ReviewSummary {
    codex_base: String,
}

#[derive(Debug, Serialize)]
struct IssuesSummary {
    provider: String,
    gh_user: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorktreeSummary {
    path: Option<String>,
    copy: Vec<String>,
    copy_as: Vec<CopyAsSummary>,
    link: Vec<String>,
    inject_local_context: bool,
    naming: Option<WorktreeNamingSummary>,
}

#[derive(Debug, Serialize)]
struct CopyAsSummary {
    from: String,
    to: String,
}

#[derive(Debug, Serialize)]
struct WorktreeNamingSummary {
    command: String,
    branch: Option<String>,
    workspace: Option<String>,
    prompt_configured: bool,
}

#[derive(Debug, Serialize)]
struct SetupSummary {
    deps: Vec<CommandSummary>,
    env: Vec<KeyValueSummary>,
    env_files: Vec<EnvFileSummary>,
}

#[derive(Debug, Serialize)]
struct CommandSummary {
    run: String,
    working_dir: Option<String>,
    if_exists: Option<String>,
    label: Option<String>,
}

#[derive(Debug, Serialize)]
struct KeyValueSummary {
    key: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct EnvFileSummary {
    path: String,
    values: Vec<KeyValueSummary>,
}

#[derive(Debug, Serialize)]
struct SiteSummary {
    provider: String,
    active: bool,
    name: String,
    root: String,
    secure: bool,
    url: String,
    target: Option<String>,
}

#[derive(Debug, Serialize)]
struct EditorSummary {
    command: Option<String>,
    placement: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceSummary {
    tab_count: usize,
    tabs: Vec<String>,
    post_deps_tab_count: usize,
    post_deps_tabs: Vec<String>,
    browser: Option<WorkspaceBrowserSummary>,
    color_count: usize,
    colors: Vec<WorkspaceColorSummary>,
}

#[derive(Debug, Serialize)]
struct WorkspaceBrowserSummary {
    mode: String,
    url: Option<String>,
    app: Option<String>,
    chrome_devtools: Option<WorkspaceChromeDevtoolsSummary>,
}

#[derive(Debug, Serialize)]
struct WorkspaceChromeDevtoolsSummary {
    port: Option<u16>,
    user_data_dir: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceColorSummary {
    kind: String,
    color: String,
}

#[derive(Debug, Serialize)]
struct AgentSummary {
    cli: String,
    args: Vec<String>,
    command: Option<String>,
    ready: String,
    submit: String,
    timeout: u64,
    send_after: u64,
    prompt_modes: Vec<String>,
    prompt_counts: Vec<PromptModeSummary>,
}

#[derive(Debug, Serialize)]
struct PromptModeSummary {
    mode: String,
    count: usize,
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
    title: String,
    mode: String,
    presentation_group: String,
    body_summary: Option<String>,
    body: Option<String>,
    source_text: Option<String>,
    origin: Option<WorkflowOriginSummary>,
    task_count: usize,
    task_runs: TaskRunCounts,
    task_run_groups: Vec<WorkflowTaskRunGroup>,
    relationship_rows: Vec<WorkflowRelationshipRow>,
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
struct WorkflowRelationshipRow {
    index: usize,
    task: String,
    parent: Option<String>,
    profile: Option<String>,
    run_id: String,
    task_document: Option<TaskDocumentLinkSummary>,
    task_document_error: Option<String>,
    task_run: Option<WorkflowRelationshipTaskRunSummary>,
    task_run_path: Option<String>,
    task_run_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowRelationshipTaskRunSummary {
    id: String,
    path: String,
    task: String,
    branch: String,
    status: String,
    group: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct WorkflowTaskRunGroup {
    status: String,
    items: Vec<TaskRunSummary>,
}

#[derive(Debug, Serialize)]
struct WorkflowOriginSummary {
    provider: String,
    id: String,
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
    passed: usize,
    failed: usize,
    skipped: usize,
    missing: usize,
}

#[derive(Debug, Serialize)]
struct WorkflowPolicySummary {
    pull_request: String,
    landing: String,
    review_codex_base: String,
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
    copy: Vec<String>,
    copy_as: Vec<CopyAsSummary>,
    link: Vec<String>,
    agent: String,
    has_site: bool,
    worktree: Option<WorktreeSummary>,
    setup: Option<SetupSummary>,
    site: Option<SiteSummary>,
    workspace: Option<WorkspaceSummary>,
    agent_settings: Option<AgentSummary>,
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
    scope: String,
    spec: Option<String>,
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
            ideas: ctx.storage_root.display_path(&ctx.storage_root.ideas_dir()),
            tasks: ctx.storage_root.display_path(&ctx.storage_root.tasks_dir()),
            workflows: ctx
                .storage_root
                .display_path(&ctx.storage_root.workflows_dir()),
            task_runs: ctx
                .storage_root
                .display_path(&ctx.storage_root.task_runs_dir()),
            profiles: ctx
                .storage_root
                .display_path(&ctx.storage_root.profiles_dir()),
            retrospecs: ctx
                .storage_root
                .display_path(&ctx.storage_root.retrospectives_dir())
                + " + <repo-root>/.wt/planning/specs/*/04-Feedback/10-retrospect.md",
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

    if let Some(legacy) = ctx.storage_root.detect_legacy_ideas(&ctx.repo_root) {
        invalid.push(legacy_state_invalid_record(
            ctx,
            legacy,
            "legacy-ideas",
            "ideas",
        ));
    }

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
    let ideas_dir = ctx.storage_root.ideas_dir();
    if !ideas_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    let display = ctx.storage_root.display_path(&ideas_dir);
    for entry in fs::read_dir(&ideas_dir)
        .with_context(|| format!("Failed to read idea directory: {display}"))?
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
    let relationship_rows = workflow_relationship_rows(ctx, &metadata);
    let title = workflow_title_label(ctx, &id, &metadata);
    let body_summary = metadata.body.as_deref().and_then(body_summary);
    let body = non_empty_string(metadata.body);
    let origin = metadata
        .origin
        .as_ref()
        .map(|origin| WorkflowOriginSummary {
            provider: origin.provider.clone(),
            id: origin.id.clone(),
        });

    WorkflowSummary {
        id,
        path: relative_path(ctx, path),
        title,
        mode: metadata.mode.as_str().into(),
        presentation_group,
        body_summary,
        body,
        source_text: read_known_source_text(ctx, path),
        origin,
        task_count: metadata.tasks.len(),
        task_runs: counts,
        task_run_groups,
        relationship_rows,
        runnable,
        base_mode: metadata.base_mode,
        base: metadata.base,
        profile: metadata.profile,
        profiles: metadata.profiles,
        policy: WorkflowPolicySummary {
            pull_request: metadata.policy.pull_request.as_str().into(),
            landing: metadata.policy.landing.as_str().into(),
            review_codex_base: metadata.policy.review.codex_base.as_str().into(),
        },
        updated_at: metadata.updated_at,
        state_error,
    }
}

fn workflow_relationship_rows(
    ctx: &Ctx,
    metadata: &WorkflowMetadata,
) -> Vec<WorkflowRelationshipRow> {
    if matches!(metadata.mode, WorkflowMode::Matrix) {
        return metadata
            .tasks
            .iter()
            .enumerate()
            .flat_map(|(idx, task)| {
                task.runs
                    .iter()
                    .map(move |run| workflow_relationship_row(ctx, idx + 1, task, Some(run)))
            })
            .collect();
    }

    metadata
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, task)| workflow_relationship_row(ctx, idx + 1, task, None))
        .collect()
}

fn workflow_relationship_row(
    ctx: &Ctx,
    index: usize,
    task: &workflow::WorkflowTask,
    profile_run: Option<&workflow::WorkflowTaskRun>,
) -> WorkflowRelationshipRow {
    let task_key = task::safe_task_key(&task.task);
    let (task_document, task_document_error) = linked_task_document(ctx, &task_key);
    let run_id = profile_run
        .map(|run| run.run.clone())
        .unwrap_or_else(|| task.run.clone());
    let (task_run, task_run_path, task_run_error) = workflow_relationship_task_run(ctx, &run_id);

    WorkflowRelationshipRow {
        index,
        task: task_key,
        parent: task.parent.clone(),
        profile: profile_run.map(|run| run.profile.clone()),
        run_id,
        task_document,
        task_document_error,
        task_run,
        task_run_path,
        task_run_error,
    }
}

fn workflow_relationship_task_run(
    ctx: &Ctx,
    run_id: &str,
) -> (
    Option<WorkflowRelationshipTaskRunSummary>,
    Option<String>,
    Option<String>,
) {
    if run_id.trim().is_empty() {
        return (
            None,
            None,
            Some("Workflow task is missing TaskRun id".into()),
        );
    }

    let path = match task_run::resolve(ctx, run_id) {
        Ok(path) => path,
        Err(err) => return (None, None, Some(format!("{err:#}"))),
    };
    let display_path = relative_path(ctx, &path);
    match task_run::read(&path) {
        Ok(run) => {
            let id = task_run::id_from_path(&path).unwrap_or_else(|_| run_id.to_string());
            (
                Some(WorkflowRelationshipTaskRunSummary {
                    id,
                    path: display_path.clone(),
                    task: run.task,
                    branch: run.branch,
                    status: run.status.as_str().into(),
                    group: run.group,
                    error: run.error,
                }),
                Some(display_path),
                None,
            )
        }
        Err(err) => (None, Some(display_path), Some(format!("{err:#}"))),
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
        passed: 0,
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
        "prepared" => 0,
        "running" => 1,
        "passed" => 2,
        "skipped" => 3,
        "failed" => 4,
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
        TaskRunStatus::Passed => counts.passed += 1,
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
    if counts.total > 0 && counts.passed + counts.skipped == counts.total {
        return "passed".into();
    }
    "waiting".into()
}

fn collect_task_runs(ctx: &Ctx) -> Result<TaskRunCollection> {
    let inventory = task_run::list_lossy(ctx)?;
    let items = inventory
        .records
        .iter()
        .map(|record| task_run_summary(ctx, record))
        .collect();
    let invalid = inventory
        .invalid
        .into_iter()
        .map(|record| InvalidRecord {
            key: record.id,
            path: relative_path(ctx, &record.path),
            error: record.error,
            source_text: read_known_source_text(ctx, &record.path),
        })
        .collect();

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
    let inventory = Config::load_profile_inventory_from_storage(
        &ctx.repo_root,
        &ctx.storage_root,
        &ctx.base_config,
    )?;
    let items = inventory
        .profiles
        .into_iter()
        .map(|profile| {
            let worktree = worktree_summary(&profile.config.worktree);
            let setup = setup_summary(&profile.config.setup);
            let site = site_summary(&profile.config);
            let workspace = workspace_summary(profile.config.workspace.as_ref());
            let agent_settings = agent_summary(profile.config.agent.as_ref());
            let agent = agent_settings
                .as_ref()
                .map(|agent| agent.cli.clone())
                .unwrap_or_else(|| "none".into());
            let has_site = profile.config.has_site();

            ProfileSummary {
                name: profile.name,
                path: relative_path(ctx, &profile.path),
                copy: profile.config.worktree.copy.clone(),
                copy_as: profile
                    .config
                    .worktree
                    .copy_as
                    .iter()
                    .map(|entry| CopyAsSummary {
                        from: entry.from.clone(),
                        to: entry.to.clone(),
                    })
                    .collect(),
                link: profile.config.worktree.link.clone(),
                agent,
                has_site,
                worktree,
                setup,
                site,
                workspace,
                agent_settings,
                source_text: read_known_source_text(ctx, &profile.path),
            }
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

    if let Some(legacy) = ctx
        .storage_root
        .detect_legacy_retrospectives(&ctx.repo_root)
    {
        invalid.push(legacy_state_invalid_record(
            ctx,
            legacy,
            "legacy-retrospectives",
            "retrospectives",
        ));
    }
    if let Some(legacy) = ctx.storage_root.detect_legacy_specs(&ctx.repo_root) {
        invalid.push(legacy_state_invalid_record(
            ctx,
            legacy,
            "legacy-specs",
            "specs",
        ));
    }

    for path in retrospec_paths(ctx)? {
        let (key, scope, spec) = retrospec_identity(ctx, &path);
        let relative_path = relative_path(ctx, &path);
        match read_retrospec(ctx, &path, key.clone(), scope, spec) {
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
    let mut paths = Vec::new();

    let retrospecs_dir = ctx.storage_root.retrospectives_dir();
    if retrospecs_dir.exists() {
        let display = ctx.storage_root.display_path(&retrospecs_dir);
        for entry in fs::read_dir(&retrospecs_dir)
            .with_context(|| format!("Failed to read retrospec directory: {display}"))?
        {
            let path = entry?.path();
            let ext = path.extension().and_then(|ext| ext.to_str());
            if matches!(ext, Some("toml" | "md" | "markdown")) {
                paths.push(path);
            }
        }
    }

    let specs_dir = ctx.storage_root.specs_dir();
    if specs_dir.exists() {
        let display = ctx.storage_root.display_path(&specs_dir);
        for entry in fs::read_dir(&specs_dir)
            .with_context(|| format!("Failed to read specs directory: {display}"))?
        {
            let path = entry?.path();
            if path.is_dir() {
                let retrospec = path.join("04-Feedback/10-retrospect.md");
                if retrospec.exists() {
                    paths.push(retrospec);
                }
            }
        }
    }

    paths.sort();
    Ok(paths)
}

fn retrospec_identity(ctx: &Ctx, path: &Path) -> (String, String, Option<String>) {
    let specs_dir = ctx.storage_root.specs_dir();
    if let Ok(relative) = path.strip_prefix(&specs_dir) {
        let mut components = relative.components();
        if let Some(spec) = components
            .next()
            .and_then(|component| component.as_os_str().to_str())
        {
            return (
                format!("{spec}/10-retrospect"),
                "spec-local".into(),
                Some(spec.to_string()),
            );
        }
    }

    (
        file_stem(path).unwrap_or_else(|| "retrospec".into()),
        "cross-work".into(),
        None,
    )
}

fn read_retrospec(
    ctx: &Ctx,
    path: &Path,
    key: String,
    scope: String,
    spec: Option<String>,
) -> Result<RetrospecSummary> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("toml") => read_toml_retrospec(ctx, path, key, scope, spec),
        Some("md" | "markdown") => read_markdown_retrospec(ctx, path, key, scope, spec),
        _ => bail!("Unsupported retrospec file type: {}", path.display()),
    }
}

fn read_toml_retrospec(
    ctx: &Ctx,
    path: &Path,
    key: String,
    scope: String,
    spec: Option<String>,
) -> Result<RetrospecSummary> {
    let relative_path = relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read retrospec: {relative_path}"))?;
    let value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("Failed to parse retrospec: {relative_path}"))?;
    let title = toml_string(&value, "title").unwrap_or_else(|| key.clone());
    let body = retrospec_body(&value);
    Ok(RetrospecSummary {
        key,
        path: relative_path,
        scope,
        spec,
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

fn read_markdown_retrospec(
    ctx: &Ctx,
    path: &Path,
    key: String,
    scope: String,
    spec: Option<String>,
) -> Result<RetrospecSummary> {
    let relative_path = relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read retrospec: {relative_path}"))?;
    let title = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| key.clone());
    Ok(RetrospecSummary {
        key,
        path: relative_path,
        scope,
        spec,
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
        worktree: worktree_summary(&ctx.config.worktree),
        setup: setup_summary(&ctx.config.setup),
        workflow: WorkflowDefaultSummary {
            pull_request: workflow_default_pull_request(policy.pull_request).into(),
            landing: workflow_default_landing(policy.landing).into(),
        },
        review: ReviewSummary {
            codex_base: review_codex_base(policy.review.codex_base).into(),
        },
        issues: ctx.config.issues.as_ref().map(|issues| IssuesSummary {
            provider: match issues.provider {
                IssueProviderType::Linear => "linear".into(),
                IssueProviderType::Github => "github".into(),
            },
            gh_user: issues.gh_user.clone(),
        }),
        site: site_summary(&ctx.config),
        editor: ctx.config.effective_editor().map(|editor| EditorSummary {
            command: editor.command,
            placement: editor_placement_name(editor.placement.as_ref()).into(),
        }),
        workspace: workspace_summary(ctx.config.workspace.as_ref()),
        agent: agent_summary(ctx.config.agent.as_ref()),
    }
}

fn site_summary(config: &Config) -> Option<SiteSummary> {
    config.site.as_ref().map(|site| {
        let site = site.with_effective_defaults();
        SiteSummary {
            provider: site_provider_name(&site.provider).into(),
            active: config.has_site(),
            name: site.effective_name().into(),
            root: site.effective_root().into(),
            secure: site.effective_secure(),
            url: site.effective_url().into_owned(),
            target: site.effective_target().map(str::to_string),
        }
    })
}

fn workspace_summary(workspace: Option<&WorkspaceConfig>) -> Option<WorkspaceSummary> {
    workspace.map(|workspace| WorkspaceSummary {
        tab_count: workspace.tabs.len(),
        tabs: workspace.tabs.clone(),
        post_deps_tab_count: workspace.post_deps_tabs.len(),
        post_deps_tabs: workspace.post_deps_tabs.clone(),
        browser: workspace
            .browser
            .as_ref()
            .map(|browser| WorkspaceBrowserSummary {
                mode: workspace_browser_mode_name(browser.mode).into(),
                url: browser.effective_url().map(|url| url.into_owned()),
                app: browser.app.clone(),
                chrome_devtools: browser.chrome_devtools.as_ref().map(|chrome| {
                    WorkspaceChromeDevtoolsSummary {
                        port: chrome.port,
                        user_data_dir: chrome.effective_user_data_dir().into(),
                    }
                }),
            }),
        color_count: workspace.effective_colors().len(),
        colors: workspace
            .effective_colors()
            .into_iter()
            .map(|(kind, color)| WorkspaceColorSummary {
                kind: kind.into(),
                color: color.into(),
            })
            .collect(),
    })
}

fn agent_summary(agent: Option<&AgentConfig>) -> Option<AgentSummary> {
    agent.map(|agent| {
        let mut prompt_modes = agent
            .prompt
            .keys()
            .filter_map(|key| {
                prompt_append_mode_name(key)
                    .map(|mode| format!("{mode} append"))
                    .or_else(|| Some(key.clone()))
            })
            .collect::<Vec<_>>();
        prompt_modes.sort();
        let mut prompt_counts = agent
            .prompt
            .iter()
            .map(|(mode, prompts)| PromptModeSummary {
                mode: prompt_append_mode_name(mode)
                    .map(|mode| format!("{mode} append"))
                    .unwrap_or_else(|| mode.clone()),
                count: prompts.len(),
            })
            .collect::<Vec<_>>();
        prompt_counts.sort_by(|a, b| a.mode.cmp(&b.mode));
        AgentSummary {
            cli: agent_cli_name(&agent.cli).into(),
            args: agent.args.clone(),
            command: agent.command.clone(),
            ready: ready_mode_name(&agent.ready),
            submit: submit_mode_name(&agent.submit).into(),
            timeout: agent.timeout,
            send_after: agent.send_after,
            prompt_modes,
            prompt_counts,
        }
    })
}

fn worktree_summary(worktree: &WorktreeConfig) -> Option<WorktreeSummary> {
    if *worktree == WorktreeConfig::default() {
        return None;
    }
    Some(WorktreeSummary {
        path: worktree.path.clone(),
        copy: worktree.copy.clone(),
        copy_as: worktree
            .copy_as
            .iter()
            .map(|entry| CopyAsSummary {
                from: entry.from.clone(),
                to: entry.to.clone(),
            })
            .collect(),
        link: worktree.link.clone(),
        inject_local_context: worktree.inject_local_context.is_some(),
        naming: worktree
            .naming
            .as_ref()
            .map(|naming| WorktreeNamingSummary {
                command: naming.command.clone(),
                branch: naming.branch.clone(),
                workspace: naming.workspace.clone(),
                prompt_configured: !naming.prompt.trim().is_empty(),
            }),
    })
}

fn setup_summary(setup: &SetupConfig) -> Option<SetupSummary> {
    if *setup == SetupConfig::default() {
        return None;
    }
    let mut env_files = setup
        .env_files
        .iter()
        .map(|(path, values)| EnvFileSummary {
            path: path.clone(),
            values: sorted_key_values(values),
        })
        .collect::<Vec<_>>();
    env_files.sort_by(|a, b| a.path.cmp(&b.path));
    Some(SetupSummary {
        deps: setup
            .deps
            .iter()
            .map(|dep| CommandSummary {
                run: dep.run.clone(),
                working_dir: dep.working_dir.clone(),
                if_exists: dep.if_exists.clone(),
                label: None,
            })
            .collect(),
        env: sorted_key_values(&setup.env),
        env_files,
    })
}

fn sorted_key_values(values: &std::collections::HashMap<String, String>) -> Vec<KeyValueSummary> {
    let mut values = values
        .iter()
        .map(|(key, value)| KeyValueSummary {
            key: key.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    values.sort_by(|a, b| a.key.cmp(&b.key));
    values
}

fn prompt_append_mode_name(key: &str) -> Option<&str> {
    key.strip_prefix("\0append:")
}

fn editor_placement_name(placement: Option<&EditorPlacement>) -> &'static str {
    match placement {
        Some(EditorPlacement::Process) => "process",
        Some(EditorPlacement::CmuxSurface) | None => "cmux_surface",
    }
}

fn workspace_browser_mode_name(mode: crate::config::WorkspaceBrowserMode) -> &'static str {
    match mode {
        crate::config::WorkspaceBrowserMode::None => "none",
        crate::config::WorkspaceBrowserMode::System => "system",
        crate::config::WorkspaceBrowserMode::ChromeDevtools => "chrome_devtools",
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

fn review_codex_base(policy: ReviewCodexBasePolicy) -> &'static str {
    match policy {
        ReviewCodexBasePolicy::None => "none",
        ReviewCodexBasePolicy::Advisory => "advisory",
        ReviewCodexBasePolicy::Required => "required",
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

fn ready_mode_name(mode: &ReadyMode) -> String {
    match mode {
        ReadyMode::Auto => "auto".into(),
        ReadyMode::Marker(marker) => marker.clone(),
    }
}

fn submit_mode_name(mode: &SubmitMode) -> &'static str {
    match mode {
        SubmitMode::Auto => "auto",
        SubmitMode::Newline => "newline",
        SubmitMode::CarriageReturn => "carriage_return",
        SubmitMode::None => "none",
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
    if path.starts_with(ctx.storage_root.personal_root()) {
        return ctx.storage_root.display_path(path);
    }

    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn legacy_state_invalid_record(
    ctx: &Ctx,
    legacy: LegacyLocalStorage,
    key: &str,
    state_name: &str,
) -> InvalidRecord {
    InvalidRecord {
        key: key.into(),
        path: relative_path(ctx, legacy.path()),
        error: legacy.error_message_for(state_name),
        source_text: None,
    }
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
        || relative == "<repo-root>/.wt/config/local.toml"
        || (relative.starts_with("<repo-root>/.wt/planning/ideas/")
            && matches!(
                Path::new(relative).extension().and_then(|ext| ext.to_str()),
                Some("toml" | "md" | "markdown")
            ))
        || (relative.starts_with("<repo-root>/.wt/planning/retrospectives/")
            && matches!(
                Path::new(relative).extension().and_then(|ext| ext.to_str()),
                Some("toml" | "md" | "markdown")
            ))
        || (relative.starts_with("<repo-root>/.wt/planning/specs/")
            && relative.ends_with("/04-Feedback/10-retrospect.md"))
        || (relative.starts_with("<repo-root>/.wt/execution/tasks/") && relative.ends_with(".toml"))
        || (relative.starts_with("<repo-root>/.wt/execution/workflows/")
            && relative.ends_with(".toml"))
        || (relative.starts_with("<repo-root>/.wt/execution/task-runs/")
            && relative.ends_with(".toml"))
        || (relative.starts_with("<repo-root>/.wt/config/profiles/")
            && relative.ends_with("/profile.toml"))
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
    use crate::config::IssuesConfig;
    use crate::config::{
        AgentCli, AgentConfig, DepCommand, EditorConfig, EditorPlacement, ReadyMode, SiteConfig,
        SiteProvider, SubmitMode, WorkflowDefaultLandingPolicy, WorkflowDefaultPullRequestMode,
        WorkspaceBrowserConfig, WorkspaceBrowserMode, WorkspaceChromeDevtoolsConfig,
        WorkspaceConfig, WorktreeNamingConfig,
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
        write_profile(
            dir.path(),
            "codex",
            "[worktree]\ncopy = [\".env\", \".linear.toml\"]\ncopy_as = [{ from = \".local/profiles/codex/scaffold\", to = \".\" }]\nlink = [\".local\"]\n\n[agent]\ncli = \"codex\"\n",
        );
        write_profile(dir.path(), "bad name", "[agent]\ncli = \"codex\"\n");
        write_retrospec(
            dir.path(),
            "retro",
            "title = \"Retro\"\ndate = \"2026-05-18\"\noutcome = \"landed\"\ntarget = \"demo\"\ntags = [\"ui\"]\n\n[context]\ngoal = \"Retro goal\"\n\n[keep]\nitems = [\"Keep this\"]\n",
        );
        write_spec_retrospect(
            dir.path(),
            "demo-spec",
            "# Demo spec retro\n\n## 결과\n- result: landed\n\n## 유지할 점\n- Keep spec context\n",
        );
        write_retrospec(dir.path(), "bad", "title = [\n");
        fs::write(
            dir.path().join(".wt.toml"),
            "[workflow]\npull_request = \"ready\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".local")).unwrap();
        fs::write(
            dir.path().join(".wt/config/local.toml"),
            "[workflow]\nlanding = \"auto\"\n",
        )
        .unwrap();

        let mut config = Config::default();
        config.worktree.copy = vec!["AGENTS.override.md".into()];
        config.worktree.link = vec![".local".into()];
        config.worktree.inject_local_context = Some("## Local context\n".into());
        config.worktree.naming = Some(WorktreeNamingConfig {
            command: "claude -p".into(),
            prompt: "Generate a branch slug".into(),
            branch: Some("{{branch_prefix}}{{english_slug}}".into()),
            workspace: Some("{{english_slug}}".into()),
        });
        config.setup.deps = vec![DepCommand {
            working_dir: None,
            run: "npm install".into(),
            if_exists: Some("package.json".into()),
        }];
        config
            .setup
            .env
            .insert("APP_URL".into(), "https://{{site_name}}.test".into());
        config.workflow.pull_request = Some(WorkflowDefaultPullRequestMode::Ready);
        config.workflow.landing = Some(WorkflowDefaultLandingPolicy::Auto);
        config.issues = Some(IssuesConfig {
            provider: IssueProviderType::Github,
            gh_user: Some("alice".into()),
        });
        config.site = Some(SiteConfig {
            provider: SiteProvider::Herd,
            name: Some("{{repo}}-{{branch_slug}}".into()),
            root: Some(".".into()),
            secure: Some(true),
            url: Some("https://{{site_name}}.test".into()),
            target: None,
        });
        config.editor = EditorConfig {
            command: Some("nvim {{path}}".into()),
            placement: Some(EditorPlacement::CmuxSurface),
        };
        config.workspace = Some(WorkspaceConfig {
            tabs: vec!["lazygit".into(), "nvim".into()],
            post_deps_tabs: vec!["npm run dev".into()],
            colors: HashMap::from([("task".into(), "blue".into())]),
            browser: Some(WorkspaceBrowserConfig {
                mode: WorkspaceBrowserMode::ChromeDevtools,
                url: Some("{{site_url}}".into()),
                app: None,
                chrome_devtools: Some(WorkspaceChromeDevtoolsConfig {
                    port: Some(9222),
                    user_data_dir: Some("{{worktree_parent}}/.chrome-devtools".into()),
                }),
            }),
        });
        config.agent = Some(AgentConfig {
            cli: AgentCli::Codex,
            args: vec!["--yolo".into()],
            command: None,
            ready: ReadyMode::Auto,
            submit: SubmitMode::Auto,
            timeout: 30,
            send_after: 2,
            prompt: HashMap::from([
                ("branch".into(), vec!["branch prompt".into()]),
                (
                    "issue".into(),
                    vec!["context prompt".into(), "start prompt".into()],
                ),
            ]),
            ..AgentConfig::default()
        });
        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            config,
            Config::default(),
            ConfigSource::Files(vec![
                dir.path().join(".wt.toml"),
                dir.path().join(".wt/config/local.toml"),
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
        assert_eq!(snapshot.retrospecs.items.len(), 2);
        assert_eq!(snapshot.retrospecs.invalid.len(), 1);
        assert_eq!(snapshot.config.workflow.pull_request, "ready");
        assert_eq!(snapshot.config.workflow.landing, "auto");
        let issues = snapshot.config.issues.as_ref().unwrap();
        assert_eq!(issues.provider, "github");
        assert_eq!(issues.gh_user.as_deref(), Some("alice"));
        let worktree = snapshot.config.worktree.as_ref().unwrap();
        assert_eq!(worktree.copy, vec!["AGENTS.override.md".to_string()]);
        assert_eq!(worktree.link, vec![".local".to_string()]);
        assert!(worktree.inject_local_context);
        let naming = worktree.naming.as_ref().unwrap();
        assert_eq!(naming.command, "claude -p");
        assert!(naming.prompt_configured);
        let setup = snapshot.config.setup.as_ref().unwrap();
        assert_eq!(setup.deps[0].run, "npm install");
        assert_eq!(setup.env[0].key, "APP_URL");
        let site = snapshot.config.site.as_ref().unwrap();
        assert_eq!(site.provider, "herd");
        assert_eq!(site.url, "https://{{site_name}}.test");
        let editor = snapshot.config.editor.as_ref().unwrap();
        assert_eq!(editor.command.as_deref(), Some("nvim {{path}}"));
        assert_eq!(editor.placement, "cmux_surface");
        let workspace = snapshot.config.workspace.as_ref().unwrap();
        assert_eq!(workspace.tabs, vec!["lazygit", "nvim"]);
        let browser = workspace.browser.as_ref().unwrap();
        assert_eq!(browser.mode, "chrome_devtools");
        assert_eq!(
            browser.chrome_devtools.as_ref().unwrap().user_data_dir,
            "{{worktree_parent}}/.chrome-devtools"
        );
        let agent = snapshot.config.agent.as_ref().unwrap();
        assert_eq!(agent.cli, "codex");
        assert_eq!(agent.args, vec!["--yolo".to_string()]);
        assert_eq!(agent.ready, "auto");
        assert_eq!(agent.submit, "auto");
        assert_eq!(agent.timeout, 30);
        assert_eq!(agent.send_after, 2);
        assert_eq!(agent.prompt_counts[0].mode, "branch");
        assert_eq!(agent.prompt_counts[0].count, 1);
        assert_eq!(agent.prompt_counts[1].mode, "issue");
        assert_eq!(agent.prompt_counts[1].count, 2);
        let profile = snapshot.profiles.items.first().unwrap();
        assert_eq!(
            profile.copy,
            vec![".env".to_string(), ".linear.toml".to_string()]
        );
        assert_eq!(profile.copy_as[0].from, ".local/profiles/codex/scaffold");
        assert_eq!(profile.copy_as[0].to, ".");
        assert_eq!(profile.link, vec![".local".to_string()]);
        assert_eq!(
            snapshot.sources.config_paths,
            vec![".wt.toml", "<repo-root>/.wt/config/local.toml"]
        );
        assert_eq!(
            snapshot.tasks.items[0].path,
            "<repo-root>/.wt/execution/tasks/demo.toml"
        );
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
        let relationship = &snapshot.workflows.items[0].relationship_rows[0];
        assert_eq!(relationship.index, 1);
        assert_eq!(relationship.task, "demo");
        assert_eq!(relationship.run_id, "run-demo");
        assert_eq!(relationship.task_document.as_ref().unwrap().title, "Demo");
        assert_eq!(relationship.task_run.as_ref().unwrap().status, "prepared");
        assert_eq!(
            relationship.task_run.as_ref().unwrap().path,
            "<repo-root>/.wt/execution/task-runs/run-demo.toml"
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
        assert_eq!(snapshot.retrospecs.items[0].scope, "cross-work");
        assert_eq!(snapshot.retrospecs.items[1].key, "demo-spec/10-retrospect");
        assert_eq!(snapshot.retrospecs.items[1].scope, "spec-local");
        assert_eq!(
            snapshot.retrospecs.items[1].spec.as_deref(),
            Some("demo-spec")
        );
        assert_eq!(
            snapshot.retrospecs.items[1].path,
            "<repo-root>/.wt/planning/specs/demo-spec/04-Feedback/10-retrospect.md"
        );
        assert_eq!(
            snapshot.workflows.items[0].presentation_group,
            "state_error"
        );
    }

    #[test]
    fn snapshot_reports_legacy_repo_root_ideas_as_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_dir = dir.path().join(".local/ideas");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(legacy_dir.join("old.toml"), "title = \"Old idea\"\n").unwrap();
        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            Config::default(),
            Config::default(),
            ConfigSource::Default,
        );

        let snapshot = build(&state).unwrap();

        assert!(snapshot.ideas.items.is_empty());
        assert_eq!(snapshot.ideas.invalid.len(), 1);
        let invalid = &snapshot.ideas.invalid[0];
        assert_eq!(invalid.key, "legacy-ideas");
        assert_eq!(invalid.path, ".local/ideas");
        assert!(invalid.error.contains("Found legacy wt personal ideas"));
        assert!(invalid.error.contains("wt does not silently fall back"));
        assert_eq!(invalid.source_text, None);
    }

    #[test]
    fn snapshot_reports_legacy_repo_root_retrospectives_as_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_dir = dir.path().join(".local/retrospectives");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(legacy_dir.join("old.toml"), "title = \"Old retro\"\n").unwrap();
        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            Config::default(),
            Config::default(),
            ConfigSource::Default,
        );

        let snapshot = build(&state).unwrap();

        assert!(snapshot.retrospecs.items.is_empty());
        assert_eq!(snapshot.retrospecs.invalid.len(), 1);
        let invalid = &snapshot.retrospecs.invalid[0];
        assert_eq!(invalid.key, "legacy-retrospectives");
        assert_eq!(invalid.path, ".local/retrospectives");
        assert!(
            invalid
                .error
                .contains("Found legacy wt personal retrospectives")
        );
        assert!(invalid.error.contains("wt does not silently fall back"));
        assert_eq!(invalid.source_text, None);
    }

    #[test]
    fn matrix_relationship_rows_include_every_task_row() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "matrix-a",
            "title = \"Matrix A\"\nbranch = \"matrix/a\"\nbody = \"A body\"\n",
        );
        write_task(
            dir.path(),
            "matrix-b",
            "title = \"Matrix B\"\nbranch = \"matrix/b\"\nbody = \"B body\"\n",
        );
        for (run, task, branch) in [
            ("run-a-codex", "matrix-a", "matrix/a-codex"),
            ("run-a-claude", "matrix-a", "matrix/a-claude"),
            ("run-b-codex", "matrix-b", "matrix/b-codex"),
            ("run-b-claude", "matrix-b", "matrix/b-claude"),
        ] {
            write_task_run(
                dir.path(),
                run,
                &format!(
                    "task = \"{task}\"\nbranch = \"{branch}\"\nstatus = \"prepared\"\ncreated_at = \"2026-05-18T00:00:00Z\"\nupdated_at = \"2026-05-18T00:00:00Z\"\n"
                ),
            );
        }

        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            Config::default(),
            Config::default(),
            ConfigSource::Default,
        );
        let ctx = state.ctx();
        let metadata = workflow::WorkflowMetadata {
            title: Some("Matrix demo".into()),
            body: None,
            origin: None,
            mode: workflow::WorkflowMode::Matrix,
            profile: None,
            profiles: vec!["codex".into(), "claude".into()],
            base_mode: "explicit".into(),
            base: Some("main".into()),
            color: None,
            created_at: "2026-05-18T00:00:00Z".into(),
            updated_at: "2026-05-18T00:00:00Z".into(),
            policy: workflow::WorkflowPolicy {
                pull_request: workflow::WorkflowPullRequestMode::None,
                landing: workflow::WorkflowLandingPolicy::Manual,
                review: Default::default(),
            },
            tasks: vec![
                workflow::WorkflowTask {
                    task: "matrix-a".into(),
                    run: String::new(),
                    parent: None,
                    runs: vec![
                        workflow::WorkflowTaskRun {
                            profile: "codex".into(),
                            run: "run-a-codex".into(),
                        },
                        workflow::WorkflowTaskRun {
                            profile: "claude".into(),
                            run: "run-a-claude".into(),
                        },
                    ],
                },
                workflow::WorkflowTask {
                    task: "matrix-b".into(),
                    run: String::new(),
                    parent: None,
                    runs: vec![
                        workflow::WorkflowTaskRun {
                            profile: "codex".into(),
                            run: "run-b-codex".into(),
                        },
                        workflow::WorkflowTaskRun {
                            profile: "claude".into(),
                            run: "run-b-claude".into(),
                        },
                    ],
                },
            ],
        };

        let rows = workflow_relationship_rows(&ctx, &metadata);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.index,
                    row.task.as_str(),
                    row.profile.as_deref(),
                    row.run_id.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                (1, "matrix-a", Some("codex"), "run-a-codex"),
                (1, "matrix-a", Some("claude"), "run-a-claude"),
                (2, "matrix-b", Some("codex"), "run-b-codex"),
                (2, "matrix-b", Some("claude"), "run-b-claude"),
            ]
        );
    }

    #[test]
    fn snapshot_groups_terminal_workflow_as_passed() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "passed-task",
            "title = \"Passed task\"\nbranch = \"feature/passed\"\n",
        );
        write_task_run(
            dir.path(),
            "run-passed",
            "task = \"passed-task\"\nbranch = \"feature/passed\"\nstatus = \"passed\"\ngroup = \"passed-workflow\"\ncreated_at = \"2026-05-18T00:00:00Z\"\nupdated_at = \"2026-05-18T00:00:00Z\"\n",
        );
        write_workflow(
            dir.path(),
            "passed-workflow",
            "title = \"Passed workflow\"\nmode = \"single\"\nbase_mode = \"explicit\"\nbase = \"main\"\ncreated_at = \"2026-05-18T00:00:00Z\"\nupdated_at = \"2026-05-18T00:00:00Z\"\n\n[policy]\npull_request = \"none\"\nlanding = \"manual\"\n\n[[tasks]]\ntask = \"passed-task\"\nrun = \"run-passed\"\n",
        );
        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            Config::default(),
            Config::default(),
            ConfigSource::Default,
        );

        let snapshot = build(&state).unwrap();

        assert_eq!(snapshot.task_runs.items[0].status, "passed");
        assert_eq!(snapshot.workflows.items[0].task_runs.passed, 1);
        assert_eq!(snapshot.workflows.items[0].presentation_group, "passed");
        assert_eq!(
            snapshot.workflows.items[0].task_run_groups[0].status,
            "passed"
        );
    }

    fn write_task(root: &Path, name: &str, content: &str) {
        let dir = root.join(".wt/execution/tasks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_task_run(root: &Path, name: &str, content: &str) {
        let dir = root.join(".wt/execution/task-runs");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_workflow(root: &Path, name: &str, content: &str) {
        let dir = root.join(".wt/execution/workflows");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_idea(root: &Path, name: &str, content: &str) {
        let dir = root.join(".wt/planning/ideas");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_profile(root: &Path, name: &str, content: &str) {
        let dir = root.join(".wt/config/profiles").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("profile.toml"), content).unwrap();
    }

    fn write_retrospec(root: &Path, name: &str, content: &str) {
        let dir = root.join(".wt/planning/retrospectives");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    fn write_spec_retrospect(root: &Path, spec: &str, content: &str) {
        let dir = root
            .join(".wt/planning/specs")
            .join(spec)
            .join("04-Feedback");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("10-retrospect.md"), content).unwrap();
    }
}
