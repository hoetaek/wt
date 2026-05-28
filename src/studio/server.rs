use crate::config::{Config, ProfileConfig, validate_profile_name};
use crate::context::Ctx;
use crate::storage::StorageRoot;
use crate::studio::auth::{COOKIE_NAME, StudioSession, mint_session_token};
use crate::studio::resource::{
    FileFingerprint, ResourceError, ResourceErrorOrPrecondition, atomic_write, check_precondition,
    diff_text, empty_fingerprint, read_fingerprint,
};
use crate::studio::workflow::{validate_workflow_candidate, validate_workflow_id};
use crate::task::{self, TaskDocument};
use crate::workflow as workflow_model;
use anyhow::{Context, Result, bail};
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

static WEB_DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/studio/web/dist");

#[derive(Debug)]
pub struct ServerOptions {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct StudioState {
    session: Arc<StudioSession>,
    repo_root: Arc<PathBuf>,
    storage_root: StorageRoot,
    task_document_apply_lock: Arc<Mutex<()>>,
    personal_config_apply_lock: Arc<Mutex<()>>,
    profile_config_apply_lock: Arc<Mutex<()>>,
    profile_prompt_apply_lock: Arc<Mutex<()>>,
    workflow_apply_lock: Arc<Mutex<()>>,
}

impl StudioState {
    fn new(ctx: &Ctx, origin: String, token: String) -> Self {
        Self {
            session: Arc::new(StudioSession::new(origin, token)),
            repo_root: Arc::new(ctx.repo_root.clone()),
            storage_root: ctx.storage_root.clone(),
            task_document_apply_lock: Arc::new(Mutex::new(())),
            personal_config_apply_lock: Arc::new(Mutex::new(())),
            profile_config_apply_lock: Arc::new(Mutex::new(())),
            profile_prompt_apply_lock: Arc::new(Mutex::new(())),
            workflow_apply_lock: Arc::new(Mutex::new(())),
        }
    }

    #[cfg(test)]
    fn for_tests(origin: &str, token: &str, repo_root: &Path) -> Self {
        Self {
            session: Arc::new(StudioSession::new(origin.into(), token.into())),
            repo_root: Arc::new(repo_root.to_path_buf()),
            storage_root: StorageRoot::from_git_common_dir(repo_root.join(".git")),
            task_document_apply_lock: Arc::new(Mutex::new(())),
            personal_config_apply_lock: Arc::new(Mutex::new(())),
            profile_config_apply_lock: Arc::new(Mutex::new(())),
            profile_prompt_apply_lock: Arc::new(Mutex::new(())),
            workflow_apply_lock: Arc::new(Mutex::new(())),
        }
    }

    fn token(&self) -> &str {
        self.session.token()
    }

    fn accept_auth_token(&self, token: &str) -> bool {
        self.session.accept_auth_token(token)
    }

    fn api_authorized(&self, headers: &HeaderMap) -> bool {
        self.session.validate_api_headers(headers)
    }

    fn read_api_authorized(&self, headers: &HeaderMap) -> bool {
        self.session.validate_read_headers(headers)
    }
}

pub async fn serve(ctx: &Ctx, options: ServerOptions) -> Result<()> {
    validate_loopback_host(&options.host)?;
    let listener = TcpListener::bind((options.host.as_str(), options.port))
        .await
        .with_context(|| {
            format!(
                "failed to bind wt studio server on {}:{}",
                options.host, options.port
            )
        })?;
    let addr = listener.local_addr()?;
    let origin = format!("http://{addr}");
    let token = mint_session_token()?;
    let auth_url = format!("{origin}/auth?token={token}");

    if ctx.quiet {
        println!("{auth_url}");
    } else {
        ctx.ui.print_step(&format!("wt studio: {auth_url}"));
        ctx.ui.print_dim("Serving wt studio. Press Ctrl-C to stop.");
    }

    maybe_open_browser(ctx, &auth_url, |url| opener::open_browser(url));

    axum::serve(listener, app(StudioState::new(ctx, origin, token)))
        .await
        .context("wt studio server failed")
}

pub fn app(state: StudioState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/auth", get(auth_handler))
        .route("/api/ping", get(api_ping))
        .route(
            "/api/task-documents",
            get(api_task_documents).post(api_task_documents),
        )
        .route("/api/task-documents/plan", post(api_task_document_plan))
        .route("/api/task-documents/apply", post(api_task_document_apply))
        .route("/api/personal-config/plan", post(api_personal_config_plan))
        .route(
            "/api/personal-config/apply",
            post(api_personal_config_apply),
        )
        .route("/api/profiles", get(api_profiles))
        .route("/api/profiles/{name}/plan", post(api_profile_config_plan))
        .route("/api/profiles/{name}/apply", post(api_profile_config_apply))
        .route(
            "/api/profile-prompts/{name}/{mode}/plan",
            post(api_profile_prompt_plan),
        )
        .route(
            "/api/profile-prompts/{name}/{mode}/apply",
            post(api_profile_prompt_apply),
        )
        .route("/api/workflows", get(api_workflows))
        .route("/api/workflows/{id}", get(api_workflow))
        .route("/api/workflows/{id}/plan", post(api_workflow_plan))
        .route("/api/workflows/{id}/apply", post(api_workflow_apply))
        .route("/favicon.ico", get(favicon))
        .fallback(static_or_not_found)
        .with_state(state)
}

fn validate_loopback_host(host: &str) -> Result<()> {
    if host == "127.0.0.1" {
        Ok(())
    } else {
        bail!("wt studio only binds to 127.0.0.1; refusing {host}")
    }
}

fn maybe_open_browser<F, E>(ctx: &Ctx, url: &str, open: F)
where
    F: FnOnce(&str) -> std::result::Result<(), E>,
    E: Display,
{
    if ctx.quiet {
        return;
    }

    if let Err(err) = open(url) {
        ctx.ui
            .print_warning(&format!("Could not open browser automatically: {err}"));
    }
}

async fn index() -> Response {
    embedded_file_response("index.html")
}

#[derive(Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

async fn auth_handler(
    State(state): State<StudioState>,
    Query(query): Query<AuthQuery>,
) -> Response {
    let Some(token) = query.token.as_deref() else {
        return unauthorized();
    };
    if !state.accept_auth_token(token) {
        return unauthorized();
    }

    let cookie = format!(
        "{COOKIE_NAME}={}; HttpOnly; SameSite=Strict; Path=/",
        state.token()
    );
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, "/")
        .header(header::SET_COOKIE, cookie)
        .body(Body::empty())
        .expect("auth redirect response should be valid")
}

async fn api_ping(State(state): State<StudioState>, headers: HeaderMap) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    Json(serde_json::json!({
        "ok": true,
        "surface": "wt studio",
    }))
    .into_response()
}

async fn api_task_documents(State(state): State<StudioState>, headers: HeaderMap) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match task_document_inventory(&state) {
        Ok(response) => Json(response).into_response(),
        Err(err) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{err:#}")).into_response()
        }
    }
}

async fn api_task_document_plan(
    State(state): State<StudioState>,
    headers: HeaderMap,
    Json(request): Json<TaskDocumentPlanRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match plan_task_document_edit(&state, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_task_document_apply(
    State(state): State<StudioState>,
    headers: HeaderMap,
    Json(request): Json<TaskDocumentApplyRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match apply_task_document_edit(&state, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_personal_config_plan(
    State(state): State<StudioState>,
    headers: HeaderMap,
    Json(request): Json<PersonalConfigPlanRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match plan_personal_config_edit(&state, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_personal_config_apply(
    State(state): State<StudioState>,
    headers: HeaderMap,
    Json(request): Json<PersonalConfigApplyRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match apply_personal_config_edit(&state, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_profiles(State(state): State<StudioState>, headers: HeaderMap) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match profile_inventory(&state) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_profile_config_plan(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
    Json(request): Json<ProfileConfigPlanRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match plan_profile_config_edit(&state, &name, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_profile_config_apply(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath(name): AxumPath<String>,
    Json(request): Json<ProfileConfigApplyRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match apply_profile_config_edit(&state, &name, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_profile_prompt_plan(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath((name, mode)): AxumPath<(String, String)>,
    Json(request): Json<ProfilePromptPlanRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match plan_profile_prompt_edit(&state, &name, &mode, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_profile_prompt_apply(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath((name, mode)): AxumPath<(String, String)>,
    Json(request): Json<ProfilePromptApplyRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match apply_profile_prompt_edit(&state, &name, &mode, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_workflows(State(state): State<StudioState>, headers: HeaderMap) -> Response {
    if !state.read_api_authorized(&headers) {
        return unauthorized();
    }

    match workflow_inventory(&state) {
        Ok(response) => Json(response).into_response(),
        Err(err) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{err:#}")).into_response()
        }
    }
}

async fn api_workflow(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !state.read_api_authorized(&headers) {
        return unauthorized();
    }

    match read_workflow_detail(&state, &id) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_workflow_plan(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<WorkflowPlanRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match plan_workflow_edit(&state, &id, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn api_workflow_apply(
    State(state): State<StudioState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<WorkflowApplyRequest>,
) -> Response {
    if !state.api_authorized(&headers) {
        return unauthorized();
    }

    match apply_workflow_edit(&state, &id, request) {
        Ok(response) => Json(response).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn static_or_not_found(
    State(state): State<StudioState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if uri.path().starts_with("/api/") {
        if !state.api_authorized(&headers) {
            return unauthorized();
        }
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    let path = uri.path().trim_start_matches('/');
    embedded_file_response(path)
}

fn embedded_file_response(path: &str) -> Response {
    let Some(file) = WEB_DIST.get_file(path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(path))
        .body(Body::from(file.contents()))
        .expect("embedded asset response should be valid")
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

const TASK_DOCUMENT_PATH_PREFIX: &str = "<repo-root>/.wt/execution/tasks/";
const WORKFLOW_PATH_PREFIX: &str = "<repo-root>/.wt/execution/workflows/";

#[derive(Debug, Serialize)]
struct TaskDocumentInventoryResponse {
    items: Vec<TaskDocumentInventoryItem>,
    invalid: Vec<InvalidTaskDocument>,
}

#[derive(Debug, Serialize)]
struct TaskDocumentInventoryItem {
    key: String,
    path: String,
    content: String,
    document: TaskDocument,
    fingerprint: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct InvalidTaskDocument {
    path: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct WorkflowInventoryResponse {
    items: Vec<WorkflowInventoryItem>,
}

#[derive(Debug, Serialize)]
struct WorkflowInventoryItem {
    id: String,
    path: String,
    title: Option<String>,
    mode: &'static str,
    color: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct WorkflowDetailResponse {
    id: String,
    path: String,
    #[serde(flatten)]
    workflow: workflow_model::WorkflowMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowPlanRequest {
    candidate: String,
    baseline_fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Serialize)]
struct WorkflowPlanResponse {
    before: String,
    after: String,
    diff: String,
    validation_errors: Vec<String>,
    fingerprint: FileFingerprint,
    baseline_stale: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowApplyRequest {
    candidate: String,
    precondition: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct WorkflowApplyResponse {
    committed_fingerprint: FileFingerprint,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskDocumentPlanMode {
    Create,
    Update,
}

impl TaskDocumentPlanMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskDocumentPlanRequest {
    path: String,
    #[serde(alias = "operation")]
    mode: TaskDocumentPlanMode,
    document: Option<TaskDocument>,
    candidate: Option<String>,
}

#[derive(Debug, Serialize)]
struct TaskDocumentPlanResponse {
    path: String,
    operation: &'static str,
    valid: bool,
    validation_errors: Vec<String>,
    before: String,
    after: String,
    diff: String,
    precondition: FileFingerprint,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskDocumentApplyRequest {
    path: String,
    before: String,
    after: String,
    precondition: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct TaskDocumentApplyResponse {
    path: String,
    fingerprint: FileFingerprint,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalConfigPlanRequest {
    candidate: String,
    baseline_fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Serialize)]
struct PersonalConfigPlanResponse {
    before: String,
    after: String,
    diff: String,
    validation_errors: Vec<String>,
    fingerprint: FileFingerprint,
    baseline_stale: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalConfigApplyRequest {
    candidate: String,
    precondition: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct PersonalConfigApplyResponse {
    committed_fingerprint: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct ProfileInventoryResponse {
    items: Vec<ProfileInventoryItem>,
}

#[derive(Debug, Serialize)]
struct ProfileInventoryItem {
    name: String,
    path: String,
    has_profile_toml: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileConfigPlanRequest {
    candidate: String,
    baseline_fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Serialize)]
struct ProfileConfigPlanResponse {
    before: String,
    after: String,
    diff: String,
    validation_errors: Vec<String>,
    fingerprint: FileFingerprint,
    baseline_stale: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileConfigApplyRequest {
    candidate: String,
    precondition: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct ProfileConfigApplyResponse {
    committed_fingerprint: FileFingerprint,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilePromptPlanRequest {
    candidate: String,
    baseline_fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Serialize)]
struct ProfilePromptPlanResponse {
    before: String,
    after: String,
    diff: String,
    validation_errors: Vec<String>,
    fingerprint: FileFingerprint,
    baseline_stale: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilePromptApplyRequest {
    candidate: String,
    precondition: FileFingerprint,
}

#[derive(Debug, Serialize)]
struct ProfilePromptApplyResponse {
    committed_fingerprint: FileFingerprint,
}

#[derive(Debug)]
struct StudioTaskPath {
    key: String,
    absolute_path: PathBuf,
    display_path: String,
}

#[derive(Debug)]
struct DiskTaskDocument {
    content: String,
    fingerprint: FileFingerprint,
    exists: bool,
}

#[derive(Debug)]
struct StudioApiError {
    status: StatusCode,
    body: serde_json::Value,
}

impl IntoResponse for StudioApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

fn api_error(status: StatusCode, message: impl Into<String>) -> StudioApiError {
    StudioApiError {
        status,
        body: serde_json::json!({ "error": message.into() }),
    }
}

fn resource_error(err: ResourceError) -> StudioApiError {
    api_error(err.status, err.message)
}

fn task_document_inventory(state: &StudioState) -> Result<TaskDocumentInventoryResponse> {
    task::ensure_task_document_store_available(&state.storage_root, &state.repo_root)?;
    let mut items = Vec::new();
    let mut invalid = Vec::new();
    for path in task::task_document_paths_for(&state.storage_root, &state.repo_root)? {
        match task::read_task_document_path_from_store(&state.storage_root, &path) {
            Ok(selected) => {
                let fingerprint = match read_disk_task_document(&path) {
                    Ok(disk) => disk.fingerprint,
                    Err(err) => {
                        invalid.push(InvalidTaskDocument {
                            path: state.storage_root.display_path(&path),
                            error: api_error_message(&err),
                        });
                        continue;
                    }
                };
                items.push(TaskDocumentInventoryItem {
                    key: selected.key,
                    path: selected.path,
                    content: selected.content,
                    document: selected.document,
                    fingerprint,
                });
            }
            Err(err) => invalid.push(InvalidTaskDocument {
                path: state.storage_root.display_path(&path),
                error: format!("{err:#}"),
            }),
        }
    }

    Ok(TaskDocumentInventoryResponse { items, invalid })
}

fn workflow_inventory(state: &StudioState) -> Result<WorkflowInventoryResponse> {
    let workflows_dir = workflow_dir(state);
    if !workflows_dir.exists() {
        return Ok(WorkflowInventoryResponse { items: Vec::new() });
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&workflows_dir).with_context(|| {
        format!(
            "Failed to read workflow directory: {}",
            state.storage_root.display_path(&workflows_dir)
        )
    })? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut items = Vec::new();
    for path in paths {
        let id = workflow_model::id_from_path(&path)?;
        let workflow = workflow_model::read(&path)?;
        items.push(WorkflowInventoryItem {
            id,
            path: state.storage_root.display_path(&path),
            title: workflow.title,
            mode: workflow.mode.as_str(),
            color: workflow.color,
            updated_at: workflow.updated_at,
        });
    }

    Ok(WorkflowInventoryResponse { items })
}

fn read_workflow_detail(
    state: &StudioState,
    id: &str,
) -> std::result::Result<WorkflowDetailResponse, StudioApiError> {
    validate_workflow_id(id)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, format!("{err:#}")))?;
    let path = workflow_path(state, id);
    if !path.exists() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Workflow does not exist: {WORKFLOW_PATH_PREFIX}{id}.toml"),
        ));
    }
    let workflow = workflow_model::read(&path)
        .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, format!("{err:#}")))?;
    Ok(WorkflowDetailResponse {
        id: id.to_string(),
        path: state.storage_root.display_path(&path),
        workflow,
    })
}

fn plan_workflow_edit(
    state: &StudioState,
    id: &str,
    request: WorkflowPlanRequest,
) -> std::result::Result<WorkflowPlanResponse, StudioApiError> {
    let path = existing_workflow_path(state, id)?;
    let display_path = state.storage_root.display_path(&path);
    let before = read_fingerprint(&path, "workflow").map_err(resource_error)?;
    let disk = parse_workflow_metadata(&before.content)?;
    let validation = validate_workflow_candidate(&disk, &request.candidate);
    let baseline_stale = request
        .baseline_fingerprint
        .as_ref()
        .is_some_and(|baseline| baseline != &before.fingerprint);

    let (after, diff, validation_errors) = match validation {
        Ok(mut candidate) => {
            workflow_model::touch(&mut candidate);
            let after = workflow_model::render_workflow_metadata(&candidate);
            let diff = diff_text(&display_path, &before.content, &after);
            (after, diff, Vec::new())
        }
        Err(errors) => (String::new(), String::new(), errors),
    };

    Ok(WorkflowPlanResponse {
        before: before.content,
        after,
        diff,
        validation_errors,
        fingerprint: before.fingerprint,
        baseline_stale,
    })
}

fn apply_workflow_edit(
    state: &StudioState,
    id: &str,
    request: WorkflowApplyRequest,
) -> std::result::Result<WorkflowApplyResponse, StudioApiError> {
    let path = existing_workflow_path(state, id)?;
    let _apply_guard = state.workflow_apply_lock.lock().map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Workflow apply lock is poisoned",
        )
    })?;

    let current = match check_precondition(&path, &request.precondition, "workflow") {
        Ok(current) => current,
        Err(ResourceErrorOrPrecondition::Error(err)) => return Err(resource_error(err)),
        Err(ResourceErrorOrPrecondition::Precondition(stale)) => {
            return Err(StudioApiError {
                status: StatusCode::CONFLICT,
                body: serde_json::json!({
                    "error": "Workflow precondition failed",
                    "current_fingerprint": stale.current.fingerprint,
                }),
            });
        }
    };

    let disk = parse_workflow_metadata(&current.content)?;
    let mut candidate =
        validate_workflow_candidate(&disk, &request.candidate).map_err(|errors| {
            StudioApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                body: serde_json::json!({
                    "error": "Workflow validation failed",
                    "validation_errors": errors,
                }),
            }
        })?;
    workflow_model::touch(&mut candidate);
    let after = workflow_model::render_workflow_metadata(&candidate);
    let committed_fingerprint = atomic_write(&path, &after, "workflow").map_err(resource_error)?;

    Ok(WorkflowApplyResponse {
        committed_fingerprint,
    })
}

fn existing_workflow_path(
    state: &StudioState,
    id: &str,
) -> std::result::Result<PathBuf, StudioApiError> {
    validate_workflow_id(id)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, format!("{err:#}")))?;
    let path = workflow_path(state, id);
    if !path.exists() {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Workflow does not exist: {WORKFLOW_PATH_PREFIX}{id}.toml"),
        ));
    }
    Ok(path)
}

fn parse_workflow_metadata(
    content: &str,
) -> std::result::Result<workflow_model::WorkflowMetadata, StudioApiError> {
    toml::from_str::<workflow_model::WorkflowMetadata>(content)
        .map_err(|err| api_error(StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))
}

fn workflow_dir(state: &StudioState) -> PathBuf {
    state.repo_root.join(".wt/execution/workflows")
}

fn workflow_path(state: &StudioState, id: &str) -> PathBuf {
    workflow_dir(state).join(format!("{id}.toml"))
}

fn api_error_message(err: &StudioApiError) -> String {
    err.body
        .get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("Studio API error")
        .to_string()
}

fn plan_task_document_edit(
    state: &StudioState,
    request: TaskDocumentPlanRequest,
) -> std::result::Result<TaskDocumentPlanResponse, StudioApiError> {
    let resolved = resolve_task_document_path(state, &request.path)?;
    let after = candidate_content(request.document, request.candidate)?;

    let before_disk = match request.mode {
        TaskDocumentPlanMode::Create => {
            let disk = read_disk_task_document(&resolved.absolute_path)?;
            if disk.exists {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    format!("TaskDocument already exists: {}", resolved.display_path),
                ));
            }
            DiskTaskDocument {
                content: String::new(),
                fingerprint: empty_fingerprint(),
                exists: false,
            }
        }
        TaskDocumentPlanMode::Update => {
            let disk = read_disk_task_document(&resolved.absolute_path)?;
            if !disk.exists {
                return Err(api_error(
                    StatusCode::NOT_FOUND,
                    format!("TaskDocument does not exist: {}", resolved.display_path),
                ));
            }
            disk
        }
    };

    let validation_errors = validate_task_document_content(&after);
    let diff = diff_text(&resolved.display_path, &before_disk.content, &after);

    Ok(TaskDocumentPlanResponse {
        path: resolved.display_path,
        operation: request.mode.as_str(),
        valid: validation_errors.is_empty(),
        validation_errors,
        before: before_disk.content,
        after,
        diff,
        precondition: before_disk.fingerprint,
    })
}

fn apply_task_document_edit(
    state: &StudioState,
    request: TaskDocumentApplyRequest,
) -> std::result::Result<TaskDocumentApplyResponse, StudioApiError> {
    let resolved = resolve_task_document_path(state, &request.path)?;
    let validation_errors = validate_task_document_content(&request.after);
    if !validation_errors.is_empty() {
        return Err(StudioApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: serde_json::json!({
                "error": "TaskDocument validation failed",
                "validation_errors": validation_errors,
            }),
        });
    }

    let _apply_guard = state.task_document_apply_lock.lock().map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "TaskDocument apply lock is poisoned",
        )
    })?;
    let current = read_disk_task_document(&resolved.absolute_path)?;
    if current.content != request.before || current.fingerprint != request.precondition {
        return Err(StudioApiError {
            status: StatusCode::CONFLICT,
            body: serde_json::json!({
                "error": "TaskDocument precondition failed",
                "path": resolved.display_path,
                "current": current.content,
                "current_fingerprint": current.fingerprint,
                "diff": diff_text(&resolved.display_path, &request.before, &current.content),
            }),
        });
    }

    task::write_task_document_content_to_store(
        &state.storage_root,
        &state.repo_root,
        &resolved.key,
        &request.after,
    )
    .map_err(|err| api_error(StatusCode::INTERNAL_SERVER_ERROR, format!("{err:#}")))?;

    let written = read_disk_task_document(&resolved.absolute_path)?;
    Ok(TaskDocumentApplyResponse {
        path: resolved.display_path,
        fingerprint: written.fingerprint,
    })
}

fn plan_personal_config_edit(
    state: &StudioState,
    request: PersonalConfigPlanRequest,
) -> std::result::Result<PersonalConfigPlanResponse, StudioApiError> {
    let path = personal_config_path(state);
    let display_path = personal_config_display_path(state);
    let before = read_fingerprint(&path, "personal config").map_err(resource_error)?;
    let validation_errors = validate_personal_config_content(&request.candidate);
    let baseline_stale = request
        .baseline_fingerprint
        .as_ref()
        .is_some_and(|baseline| baseline != &before.fingerprint);

    let (after, diff) = if validation_errors.is_empty() {
        let diff = diff_text(&display_path, &before.content, &request.candidate);
        (request.candidate, diff)
    } else {
        (String::new(), String::new())
    };

    Ok(PersonalConfigPlanResponse {
        before: before.content,
        after,
        diff,
        validation_errors,
        fingerprint: before.fingerprint,
        baseline_stale,
    })
}

fn apply_personal_config_edit(
    state: &StudioState,
    request: PersonalConfigApplyRequest,
) -> std::result::Result<PersonalConfigApplyResponse, StudioApiError> {
    let validation_errors = validate_personal_config_content(&request.candidate);
    if !validation_errors.is_empty() {
        return Err(StudioApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: serde_json::json!({
                "error": "Personal config validation failed",
                "validation_errors": validation_errors,
            }),
        });
    }

    let _apply_guard = state.personal_config_apply_lock.lock().map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Personal config apply lock is poisoned",
        )
    })?;
    let path = personal_config_path(state);
    match check_precondition(&path, &request.precondition, "personal config") {
        Ok(_) => {}
        Err(ResourceErrorOrPrecondition::Error(err)) => return Err(resource_error(err)),
        Err(ResourceErrorOrPrecondition::Precondition(stale)) => {
            return Err(StudioApiError {
                status: StatusCode::CONFLICT,
                body: serde_json::json!({
                    "error": "Personal config precondition failed",
                    "current_fingerprint": stale.current.fingerprint,
                }),
            });
        }
    }

    let committed_fingerprint =
        atomic_write(&path, &request.candidate, "personal config").map_err(resource_error)?;
    Ok(PersonalConfigApplyResponse {
        committed_fingerprint,
    })
}

fn profile_inventory(
    state: &StudioState,
) -> std::result::Result<ProfileInventoryResponse, StudioApiError> {
    let profiles_dir = state.storage_root.profiles_dir();
    if !profiles_dir.exists() {
        return Ok(ProfileInventoryResponse { items: Vec::new() });
    }

    let entries = std::fs::read_dir(&profiles_dir).map_err(|err| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read profile directory: {err}"),
        )
    })?;
    let mut items = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read profile entry: {err}"),
            )
        })?;
        let file_type = entry.file_type().map_err(|err| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to inspect profile entry: {err}"),
            )
        })?;
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        if validate_profile_name(&name).is_err() {
            continue;
        }
        let profile_toml = entry.path().join("profile.toml");
        if !profile_toml.exists() {
            continue;
        }
        items.push(ProfileInventoryItem {
            name,
            path: state.storage_root.display_path(&profile_toml),
            has_profile_toml: true,
        });
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ProfileInventoryResponse { items })
}

fn plan_profile_config_edit(
    state: &StudioState,
    name: &str,
    request: ProfileConfigPlanRequest,
) -> std::result::Result<ProfileConfigPlanResponse, StudioApiError> {
    let path = profile_config_path(state, name)?;
    let display_path = profile_config_display_path(state, name)?;
    let before = read_fingerprint(&path, "profile config").map_err(resource_error)?;
    let validation_errors = validate_profile_config_content(&request.candidate);
    let baseline_stale = request
        .baseline_fingerprint
        .as_ref()
        .is_some_and(|baseline| baseline != &before.fingerprint);

    let (after, diff) = if validation_errors.is_empty() {
        let diff = diff_text(&display_path, &before.content, &request.candidate);
        (request.candidate, diff)
    } else {
        (String::new(), String::new())
    };

    Ok(ProfileConfigPlanResponse {
        before: before.content,
        after,
        diff,
        validation_errors,
        fingerprint: before.fingerprint,
        baseline_stale,
    })
}

fn apply_profile_config_edit(
    state: &StudioState,
    name: &str,
    request: ProfileConfigApplyRequest,
) -> std::result::Result<ProfileConfigApplyResponse, StudioApiError> {
    let validation_errors = validate_profile_config_content(&request.candidate);
    if !validation_errors.is_empty() {
        return Err(StudioApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: serde_json::json!({
                "error": "Profile config validation failed",
                "validation_errors": validation_errors,
            }),
        });
    }

    let _apply_guard = state.profile_config_apply_lock.lock().map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Profile config apply lock is poisoned",
        )
    })?;
    let path = profile_config_path(state, name)?;
    match check_precondition(&path, &request.precondition, "profile config") {
        Ok(_) => {}
        Err(ResourceErrorOrPrecondition::Error(err)) => return Err(resource_error(err)),
        Err(ResourceErrorOrPrecondition::Precondition(stale)) => {
            return Err(StudioApiError {
                status: StatusCode::CONFLICT,
                body: serde_json::json!({
                    "error": "Profile config precondition failed",
                    "current_fingerprint": stale.current.fingerprint,
                }),
            });
        }
    }

    let committed_fingerprint =
        atomic_write(&path, &request.candidate, "profile config").map_err(resource_error)?;
    Ok(ProfileConfigApplyResponse {
        committed_fingerprint,
    })
}

fn plan_profile_prompt_edit(
    state: &StudioState,
    name: &str,
    mode: &str,
    request: ProfilePromptPlanRequest,
) -> std::result::Result<ProfilePromptPlanResponse, StudioApiError> {
    let resolved = resolve_profile_prompt_path(state, name, mode)?;
    let before = read_fingerprint(&resolved.path, "profile prompt").map_err(resource_error)?;
    let baseline_stale = request
        .baseline_fingerprint
        .as_ref()
        .is_some_and(|baseline| baseline != &before.fingerprint);

    Ok(ProfilePromptPlanResponse {
        before: before.content.clone(),
        after: request.candidate.clone(),
        diff: diff_text(&resolved.display_path, &before.content, &request.candidate),
        validation_errors: Vec::new(),
        fingerprint: before.fingerprint,
        baseline_stale,
    })
}

fn apply_profile_prompt_edit(
    state: &StudioState,
    name: &str,
    mode: &str,
    request: ProfilePromptApplyRequest,
) -> std::result::Result<ProfilePromptApplyResponse, StudioApiError> {
    let resolved = resolve_profile_prompt_path(state, name, mode)?;
    let _apply_guard = state.profile_prompt_apply_lock.lock().map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Profile prompt apply lock is poisoned",
        )
    })?;
    match check_precondition(&resolved.path, &request.precondition, "profile prompt") {
        Ok(_) => {}
        Err(ResourceErrorOrPrecondition::Error(err)) => return Err(resource_error(err)),
        Err(ResourceErrorOrPrecondition::Precondition(stale)) => {
            return Err(StudioApiError {
                status: StatusCode::CONFLICT,
                body: serde_json::json!({
                    "error": "Profile prompt precondition failed",
                    "current_fingerprint": stale.current.fingerprint,
                }),
            });
        }
    }

    let committed_fingerprint = atomic_write(&resolved.path, &request.candidate, "profile prompt")
        .map_err(resource_error)?;
    Ok(ProfilePromptApplyResponse {
        committed_fingerprint,
    })
}

fn personal_config_path(state: &StudioState) -> PathBuf {
    state.repo_root.join(".wt/config/local.toml")
}

fn personal_config_display_path(state: &StudioState) -> String {
    state
        .storage_root
        .display_path(&personal_config_path(state))
}

fn validate_personal_config_content(content: &str) -> Vec<String> {
    toml::from_str::<Config>(content)
        .map(|_| Vec::new())
        .unwrap_or_else(|err| vec![err.to_string()])
}

fn profile_config_path(
    state: &StudioState,
    name: &str,
) -> std::result::Result<PathBuf, StudioApiError> {
    validate_profile_name(name)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, format!("{err:#}")))?;
    Ok(state
        .storage_root
        .profiles_dir()
        .join(name)
        .join("profile.toml"))
}

fn profile_config_display_path(
    state: &StudioState,
    name: &str,
) -> std::result::Result<String, StudioApiError> {
    profile_config_path(state, name).map(|path| state.storage_root.display_path(&path))
}

fn validate_profile_config_content(content: &str) -> Vec<String> {
    toml::from_str::<ProfileConfig>(content)
        .map(|_| Vec::new())
        .unwrap_or_else(|err| vec![err.to_string()])
}

#[derive(Debug)]
struct ProfilePromptPath {
    path: PathBuf,
    display_path: String,
}

fn resolve_profile_prompt_path(
    state: &StudioState,
    name: &str,
    mode: &str,
) -> std::result::Result<ProfilePromptPath, StudioApiError> {
    validate_profile_name(name)
        .map_err(|err| api_error(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mode = prompt_mode_file_stem(mode)?;
    let path = state
        .storage_root
        .profiles_dir()
        .join(name)
        .join("prompts")
        .join(format!("{mode}.md"));
    Ok(ProfilePromptPath {
        display_path: state.storage_root.display_path(&path),
        path,
    })
}

fn prompt_mode_file_stem(mode: &str) -> std::result::Result<&'static str, StudioApiError> {
    match mode {
        "workflow" => Ok("workflow"),
        "issue" => Ok("issue"),
        "branch" => Ok("branch"),
        "pr" => Ok("pr"),
        "common" => Ok("common"),
        _ => Err(api_error(
            StatusCode::BAD_REQUEST,
            "Prompt mode must be one of workflow, issue, branch, pr, common",
        )),
    }
}

fn resolve_task_document_path(
    state: &StudioState,
    requested_path: &str,
) -> std::result::Result<StudioTaskPath, StudioApiError> {
    task::ensure_task_document_store_available(&state.storage_root, &state.repo_root)
        .map_err(|err| api_error(StatusCode::CONFLICT, format!("{err:#}")))?;

    let requested_path = requested_path.trim();
    if requested_path.is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "TaskDocument path cannot be empty",
        ));
    }
    let path = Path::new(requested_path);
    if path.is_absolute() || has_disallowed_path_component(path) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "TaskDocument path must stay under <repo-root>/.wt/execution/tasks",
        ));
    }

    let relative = requested_path
        .strip_prefix(TASK_DOCUMENT_PATH_PREFIX)
        .unwrap_or(requested_path);
    if relative.contains('/') || relative.contains('\\') {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "TaskDocument path must be <slug>.toml",
        ));
    }

    let file_name = if relative.ends_with(".toml") {
        relative.to_string()
    } else {
        format!("{relative}.toml")
    };
    let Some(key) = file_name.strip_suffix(".toml") else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "TaskDocument path must end in .toml",
        ));
    };
    if key.is_empty() || task::safe_task_key(key) != key {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "TaskDocument path must be <slug>.toml with a safe slug",
        ));
    }

    let absolute_path = task::task_path_for(&state.storage_root, key);
    Ok(StudioTaskPath {
        key: key.to_string(),
        display_path: state.storage_root.display_path(&absolute_path),
        absolute_path,
    })
}

fn has_disallowed_path_component(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn candidate_content(
    document: Option<TaskDocument>,
    candidate: Option<String>,
) -> std::result::Result<String, StudioApiError> {
    match (document, candidate) {
        (Some(document), None) => Ok(task::render_task_document(&document)),
        (None, Some(candidate)) => Ok(candidate),
        (Some(_), Some(_)) => Err(api_error(
            StatusCode::BAD_REQUEST,
            "Plan request must include document or candidate, not both",
        )),
        (None, None) => Err(api_error(
            StatusCode::BAD_REQUEST,
            "Plan request must include document or candidate",
        )),
    }
}

fn validate_task_document_content(content: &str) -> Vec<String> {
    toml::from_str::<TaskDocument>(content)
        .map(|_| Vec::new())
        .unwrap_or_else(|err| vec![err.to_string()])
}

fn read_disk_task_document(path: &Path) -> std::result::Result<DiskTaskDocument, StudioApiError> {
    read_fingerprint(path, "TaskDocument")
        .map(|snapshot| DiskTaskDocument {
            content: snapshot.content,
            fingerprint: snapshot.fingerprint,
            exists: snapshot.exists,
        })
        .map_err(resource_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CtxOptions, OutputMode};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use std::fs;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn refuses_non_loopback_bind_host() {
        assert!(validate_loopback_host("127.0.0.1").is_ok());
        let err = validate_loopback_host("0.0.0.0").unwrap_err();
        assert!(format!("{err:#}").contains("only binds to 127.0.0.1"));
    }

    #[tokio::test]
    async fn app_serves_studio_page() {
        let dir = tempfile::tempdir().unwrap();
        let response = app(test_state(dir.path()))
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("TaskDocument authoring"));
    }

    #[tokio::test]
    async fn auth_sets_http_only_cookie_and_redirects() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(test_state(dir.path()));

        let response = app
            .oneshot(
                Request::get("/auth?token=secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[header::LOCATION], "/");
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("wt_studio_session=secret"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
    }

    #[tokio::test]
    async fn api_requires_cookie() {
        let dir = tempfile::tempdir().unwrap();
        let response = app(test_state(dir.path()))
            .oneshot(
                Request::get("/api/ping")
                    .header(header::ORIGIN, "http://127.0.0.1:8424")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn profile_api_requires_cookie_and_matching_origin() {
        let dir = tempfile::tempdir().unwrap();
        let app = app(test_state(dir.path()));

        let missing_cookie = app
            .clone()
            .oneshot(
                Request::get("/api/profiles")
                    .header(header::ORIGIN, "http://127.0.0.1:8424")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_cookie.status(), StatusCode::UNAUTHORIZED);

        let origin_mismatch = app
            .oneshot(
                Request::get("/api/profiles")
                    .header(header::ORIGIN, "http://127.0.0.1:9999")
                    .header(header::COOKIE, "wt_studio_session=secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(origin_mismatch.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_rejects_origin_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let response = app(test_state(dir.path()))
            .oneshot(
                Request::get("/api/ping")
                    .header(header::ORIGIN, "http://127.0.0.1:9999")
                    .header(header::COOKIE, "wt_studio_session=secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_ping_accepts_cookie_and_matching_origin() {
        let dir = tempfile::tempdir().unwrap();
        let response = app(test_state(dir.path()))
            .oneshot(
                Request::get("/api/ping")
                    .header(header::ORIGIN, "http://127.0.0.1:8424")
                    .header(header::COOKIE, "wt_studio_session=secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["surface"], "wt studio");
    }

    #[tokio::test]
    async fn task_document_plan_create_uses_empty_before_and_normalized_after() {
        let dir = tempfile::tempdir().unwrap();
        let response = app(test_state(dir.path()))
            .oneshot(authorized_json_request(
                "/api/task-documents/plan",
                serde_json::json!({
                    "path": "new-task.toml",
                    "mode": "create",
                    "document": {
                        "title": "New task",
                        "branch": "new-task",
                        "body": "Do the work."
                    }
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["operation"], "create");
        assert_eq!(value["valid"], true);
        assert_eq!(value["before"], "");
        assert_eq!(
            value["after"],
            "title = \"New task\"\nbranch = \"new-task\"\nbody = \"\"\"Do the work.\"\"\"\n"
        );
        assert_eq!(value["precondition"]["mtime_ns"], serde_json::Value::Null);
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+title = \"New task\"")
        );
    }

    #[tokio::test]
    async fn task_document_plan_update_uses_disk_before_and_candidate_after() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        let before = "title = \"Old\"\nbranch = \"edit-task\"\nbody = \"\"\"old\"\"\"\n";
        fs::write(tasks_dir.join("edit-task.toml"), before).unwrap();
        let after = "title = \"Edited\"\nbranch = \"edit-task\"\nbody = \"\"\"new\"\"\"\n";

        let response = app(test_state(dir.path()))
            .oneshot(authorized_json_request(
                "/api/task-documents/plan",
                serde_json::json!({
                    "path": "edit-task.toml",
                    "mode": "update",
                    "candidate": after
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["operation"], "update");
        assert_eq!(value["before"], before);
        assert_eq!(value["after"], after);
        assert!(value["diff"].as_str().unwrap().contains("-title = \"Old\""));
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+title = \"Edited\"")
        );
    }

    #[tokio::test]
    async fn task_document_apply_writes_file_and_returns_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/task-documents/plan",
                serde_json::json!({
                    "path": "create-me.toml",
                    "mode": "create",
                    "document": {
                        "title": "Create me",
                        "branch": "create-me",
                        "body": "Create through studio."
                    }
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/task-documents/apply",
                serde_json::json!({
                    "path": plan["path"],
                    "before": plan["before"],
                    "after": plan["after"],
                    "precondition": plan["precondition"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::OK);
        let applied = json_body(apply_response).await;
        assert!(applied["fingerprint"]["mtime_ns"].is_string());
        assert_eq!(applied["fingerprint"]["hash"].as_str().unwrap().len(), 64);
        let written =
            fs::read_to_string(dir.path().join(".wt/execution/tasks/create-me.toml")).unwrap();
        assert_eq!(written, plan["after"].as_str().unwrap());

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let selected = task::select_local_task_by_key(&ctx, "create-me").unwrap();
        assert_eq!(selected.document.title, "Create me");
        assert_eq!(selected.document.branch, "create-me");
    }

    #[tokio::test]
    async fn task_document_apply_rejects_stale_precondition_with_current_diff() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        fs::create_dir_all(&tasks_dir).unwrap();
        let original = "title = \"Original\"\nbranch = \"stale\"\n";
        fs::write(tasks_dir.join("stale.toml"), original).unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/task-documents/plan",
                serde_json::json!({
                    "path": "stale.toml",
                    "mode": "update",
                    "candidate": "title = \"Planned\"\nbranch = \"stale\"\n"
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;
        fs::write(
            tasks_dir.join("stale.toml"),
            "title = \"External\"\nbranch = \"stale\"\n",
        )
        .unwrap();

        let response = server
            .oneshot(authorized_json_request(
                "/api/task-documents/apply",
                serde_json::json!({
                    "path": plan["path"],
                    "before": plan["before"],
                    "after": plan["after"],
                    "precondition": plan["precondition"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let value = json_body(response).await;
        assert_eq!(value["error"], "TaskDocument precondition failed");
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+title = \"External\"")
        );
    }

    #[tokio::test]
    async fn task_document_plan_rejects_paths_outside_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        for path in ["../escape.toml", "/tmp/escape.toml"] {
            let response = server
                .clone()
                .oneshot(authorized_json_request(
                    "/api/task-documents/plan",
                    serde_json::json!({
                        "path": path,
                        "mode": "create",
                        "candidate": "title = \"Bad\"\nbranch = \"bad\"\n"
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn invalid_task_document_plan_cannot_be_applied() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/task-documents/plan",
                serde_json::json!({
                    "path": "bad-schema.toml",
                    "mode": "create",
                    "candidate": "title = \"Bad\"\nbranch = \"bad-schema\"\nunknown = true\n"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(plan_response.status(), StatusCode::OK);
        let plan = json_body(plan_response).await;
        assert_eq!(plan["valid"], false);
        assert!(
            plan["validation_errors"][0]
                .as_str()
                .unwrap()
                .contains("unknown field")
        );

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/task-documents/apply",
                serde_json::json!({
                    "path": plan["path"],
                    "before": plan["before"],
                    "after": plan["after"],
                    "precondition": plan["precondition"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            !dir.path()
                .join(".wt/execution/tasks/bad-schema.toml")
                .exists()
        );
    }

    #[tokio::test]
    async fn personal_config_plan_reads_hardcoded_local_toml_and_reports_diff() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".wt/config");
        fs::create_dir_all(&config_dir).unwrap();
        let before = "[workflow]\npull_request = \"draft\"\n";
        fs::write(config_dir.join("local.toml"), before).unwrap();

        let response = app(test_state(dir.path()))
            .oneshot(authorized_json_request(
                "/api/personal-config/plan",
                serde_json::json!({
                    "candidate": "[workflow]\npull_request = \"ready\"\nlanding = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["before"], before);
        assert_eq!(
            value["after"],
            "[workflow]\npull_request = \"ready\"\nlanding = \"auto\"\n"
        );
        assert_eq!(value["baseline_stale"], false);
        assert!(value["fingerprint"]["mtime_ns"].is_string());
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+landing = \"auto\"")
        );
    }

    #[tokio::test]
    async fn personal_config_apply_writes_only_local_toml_with_matching_precondition() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".wt/config");
        fs::create_dir_all(config_dir.join("profiles")).unwrap();
        fs::write(
            config_dir.join("local.toml"),
            "[workflow]\nlanding = \"manual\"\n",
        )
        .unwrap();
        fs::write(
            config_dir.join("profiles/dev.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/personal-config/plan",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/personal-config/apply",
                serde_json::json!({
                    "candidate": plan["after"],
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::OK);
        let applied = json_body(apply_response).await;
        assert!(applied["committed_fingerprint"]["mtime_ns"].is_string());
        assert_eq!(
            fs::read_to_string(config_dir.join("local.toml")).unwrap(),
            "[workflow]\nlanding = \"auto\"\n"
        );
        assert_eq!(
            fs::read_to_string(config_dir.join("profiles/dev.toml")).unwrap(),
            "[agent]\ncli = \"codex\"\n"
        );
    }

    #[tokio::test]
    async fn personal_config_apply_rejects_stale_precondition() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".wt/config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("local.toml"),
            "[workflow]\nlanding = \"manual\"\n",
        )
        .unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/personal-config/plan",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;
        fs::write(
            config_dir.join("local.toml"),
            "[workflow]\npull_request = \"ready\"\n",
        )
        .unwrap();

        let response = server
            .oneshot(authorized_json_request(
                "/api/personal-config/apply",
                serde_json::json!({
                    "candidate": plan["after"],
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let value = json_body(response).await;
        assert_eq!(value["error"], "Personal config precondition failed");
        assert!(value["current_fingerprint"]["mtime_ns"].is_string());
        assert_eq!(
            fs::read_to_string(config_dir.join("local.toml")).unwrap(),
            "[workflow]\npull_request = \"ready\"\n"
        );
    }

    #[tokio::test]
    async fn personal_config_validation_errors_do_not_apply() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/personal-config/plan",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"after_review\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();

        assert_eq!(plan_response.status(), StatusCode::OK);
        let plan = json_body(plan_response).await;
        assert_eq!(plan["after"], "");
        assert_eq!(plan["diff"], "");
        assert!(
            plan["validation_errors"][0]
                .as_str()
                .unwrap()
                .contains("manual")
        );

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/personal-config/apply",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"after_review\"\n",
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!dir.path().join(".wt/config/local.toml").exists());
    }

    #[tokio::test]
    async fn personal_config_plan_marks_baseline_stale() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".wt/config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("local.toml"),
            "[workflow]\nlanding = \"manual\"\n",
        )
        .unwrap();
        let server = app(test_state(dir.path()));
        let first_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/personal-config/plan",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let first = json_body(first_response).await;
        fs::write(
            config_dir.join("local.toml"),
            "[workflow]\npull_request = \"ready\"\n",
        )
        .unwrap();

        let stale_response = server
            .oneshot(authorized_json_request(
                "/api/personal-config/plan",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"auto\"\n",
                    "baseline_fingerprint": first["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(stale_response.status(), StatusCode::OK);
        let stale = json_body(stale_response).await;
        assert_eq!(stale["baseline_stale"], true);
    }

    #[tokio::test]
    async fn profile_inventory_lists_only_valid_profile_toml_records() {
        let dir = tempfile::tempdir().unwrap();
        let profiles_dir = dir.path().join(".wt/config/profiles");
        fs::create_dir_all(profiles_dir.join("codex/prompts")).unwrap();
        fs::create_dir_all(profiles_dir.join("claude")).unwrap();
        fs::create_dir_all(profiles_dir.join("prompt-only/prompts")).unwrap();
        fs::create_dir_all(profiles_dir.join("default")).unwrap();
        fs::create_dir_all(profiles_dir.join("bad name")).unwrap();
        fs::write(
            profiles_dir.join("codex/profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();
        fs::write(
            profiles_dir.join("claude/profile.toml"),
            "[agent]\ncli = \"claude\"\n",
        )
        .unwrap();
        fs::write(profiles_dir.join("codex/prompts/workflow.md"), "work").unwrap();
        fs::write(profiles_dir.join("codex/prompts/common.md"), "common").unwrap();
        fs::write(profiles_dir.join("prompt-only/prompts/workflow.md"), "work").unwrap();

        let response = app(test_state(dir.path()))
            .oneshot(authorized_get_request("/api/profiles"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        let items = value["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "claude");
        assert_eq!(items[1]["name"], "codex");
        assert_eq!(items[1]["has_profile_toml"], true);
        assert!(items[1].get("has_prompts").is_none());
    }

    #[tokio::test]
    async fn profile_config_plan_reads_named_profile_toml_and_reports_diff() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".wt/config/profiles/codex");
        fs::create_dir_all(&profile_dir).unwrap();
        let before = "[agent]\ncli = \"codex\"\n";
        fs::write(profile_dir.join("profile.toml"), before).unwrap();

        let response = app(test_state(dir.path()))
            .oneshot(authorized_json_request(
                "/api/profiles/codex/plan",
                serde_json::json!({
                    "candidate": "[agent]\ncli = \"codex\"\nargs = [\"--sandbox\", \"danger-full-access\"]\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["before"], before);
        assert_eq!(value["baseline_stale"], false);
        assert!(
            value["diff"]
                .as_str()
                .unwrap()
                .contains("+args = [\"--sandbox\", \"danger-full-access\"]")
        );
    }

    #[tokio::test]
    async fn profile_config_apply_writes_only_selected_profile_toml() {
        let dir = tempfile::tempdir().unwrap();
        let profiles_dir = dir.path().join(".wt/config/profiles");
        fs::create_dir_all(profiles_dir.join("codex")).unwrap();
        fs::create_dir_all(profiles_dir.join("claude")).unwrap();
        fs::write(
            profiles_dir.join("codex/profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();
        fs::write(
            profiles_dir.join("claude/profile.toml"),
            "[agent]\ncli = \"claude\"\n",
        )
        .unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/profiles/codex/plan",
                serde_json::json!({
                    "candidate": "[agent]\ncli = \"codex\"\nready = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/profiles/codex/apply",
                serde_json::json!({
                    "candidate": plan["after"],
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::OK);
        let applied = json_body(apply_response).await;
        assert!(applied["committed_fingerprint"]["mtime_ns"].is_string());
        assert_eq!(
            fs::read_to_string(profiles_dir.join("codex/profile.toml")).unwrap(),
            "[agent]\ncli = \"codex\"\nready = \"auto\"\n"
        );
        assert_eq!(
            fs::read_to_string(profiles_dir.join("claude/profile.toml")).unwrap(),
            "[agent]\ncli = \"claude\"\n"
        );
    }

    #[tokio::test]
    async fn profile_config_apply_rejects_stale_precondition() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".wt/config/profiles/codex");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/profiles/codex/plan",
                serde_json::json!({
                    "candidate": "[agent]\ncli = \"codex\"\nready = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;
        fs::write(
            profile_dir.join("profile.toml"),
            "[agent]\ncli = \"codex\"\nsubmit = \"newline\"\n",
        )
        .unwrap();

        let response = server
            .oneshot(authorized_json_request(
                "/api/profiles/codex/apply",
                serde_json::json!({
                    "candidate": plan["after"],
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let value = json_body(response).await;
        assert_eq!(value["error"], "Profile config precondition failed");
        assert_eq!(
            fs::read_to_string(profile_dir.join("profile.toml")).unwrap(),
            "[agent]\ncli = \"codex\"\nsubmit = \"newline\"\n"
        );
    }

    #[tokio::test]
    async fn profile_config_rejects_invalid_or_reserved_profile_name() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        for path in [
            &format!(
                "/api/profiles/{}/plan",
                crate::config::RESERVED_PROFILE_NAME
            ),
            "/api/profiles/bad%20name/plan",
            "/api/profiles/../plan",
            "/api/profiles/..%2Fescape/plan",
            "/api/profiles/codex%2Fescape/plan",
            "/api/profiles/codex..escape/plan",
            "/api/profiles/codex%2E%2Eescape/plan",
        ] {
            let response = server
                .clone()
                .oneshot(authorized_json_request(
                    path,
                    serde_json::json!({
                        "candidate": "[agent]\ncli = \"codex\"\n",
                        "baseline_fingerprint": null
                    }),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn profile_config_validation_errors_do_not_apply() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/profiles/codex/plan",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"auto\"\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();

        assert_eq!(plan_response.status(), StatusCode::OK);
        let plan = json_body(plan_response).await;
        assert_eq!(plan["after"], "");
        assert_eq!(plan["diff"], "");
        assert!(
            plan["validation_errors"][0]
                .as_str()
                .unwrap()
                .contains("unknown field")
        );

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/profiles/codex/apply",
                serde_json::json!({
                    "candidate": "[workflow]\nlanding = \"auto\"\n",
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            !dir.path()
                .join(".wt/config/profiles/codex/profile.toml")
                .exists()
        );
    }

    #[tokio::test]
    async fn profile_prompt_plan_routes_all_modes_and_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let prompts_dir = dir.path().join(".wt/config/profiles/codex/prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        for mode in ["workflow", "issue", "branch", "pr"] {
            fs::write(
                prompts_dir.join(format!("{mode}.md")),
                format!("{mode} prompt\n"),
            )
            .unwrap();
        }
        let server = app(test_state(dir.path()));

        for mode in ["workflow", "issue", "branch", "pr", "common"] {
            let candidate = format!("updated {mode}\n");
            let response = server
                .clone()
                .oneshot(authorized_json_request(
                    &format!("/api/profile-prompts/codex/{mode}/plan"),
                    serde_json::json!({
                        "candidate": candidate,
                        "baseline_fingerprint": null
                    }),
                ))
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            let value = json_body(response).await;
            assert_eq!(value["validation_errors"].as_array().unwrap().len(), 0);
            assert_eq!(value["after"], format!("updated {mode}\n"));
            if mode == "common" {
                assert_eq!(value["before"], "");
                assert_eq!(value["fingerprint"]["mtime_ns"], serde_json::Value::Null);
            } else {
                assert_eq!(value["before"], format!("{mode} prompt\n"));
            }
            assert!(
                value["diff"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("prompts/{mode}.md"))
            );
        }
    }

    #[tokio::test]
    async fn profile_prompt_apply_creates_parent_dir_for_missing_mode_file() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/profile-prompts/codex/common/plan",
                serde_json::json!({
                    "candidate": "shared prompt\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/profile-prompts/codex/common/apply",
                serde_json::json!({
                    "candidate": plan["after"],
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::OK);
        let applied = json_body(apply_response).await;
        assert!(applied["committed_fingerprint"]["mtime_ns"].is_string());
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join(".wt/config/profiles/codex/prompts/common.md")
            )
            .unwrap(),
            "shared prompt\n"
        );
    }

    #[tokio::test]
    async fn profile_prompt_rejects_invalid_mode_and_profile_name() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));

        let invalid_mode = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/profile-prompts/codex/new/plan",
                serde_json::json!({
                    "candidate": "legacy new prompt\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_mode.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_body(invalid_mode).await["error"]
                .as_str()
                .unwrap()
                .contains("workflow, issue, branch, pr, common")
        );

        let invalid_profile = server
            .oneshot(authorized_json_request(
                "/api/profile-prompts/default/common/plan",
                serde_json::json!({
                    "candidate": "default prompt\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_profile.status(), StatusCode::BAD_REQUEST);
        assert!(
            json_body(invalid_profile).await["error"]
                .as_str()
                .unwrap()
                .contains("reserved")
        );
    }

    #[tokio::test]
    async fn profile_prompt_apply_rejects_stale_precondition() {
        let dir = tempfile::tempdir().unwrap();
        let prompts_dir = dir.path().join(".wt/config/profiles/codex/prompts");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::write(prompts_dir.join("issue.md"), "original\n").unwrap();
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/profile-prompts/codex/issue/plan",
                serde_json::json!({
                    "candidate": "planned\n",
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;
        fs::write(prompts_dir.join("issue.md"), "external\n").unwrap();

        let response = server
            .oneshot(authorized_json_request(
                "/api/profile-prompts/codex/issue/apply",
                serde_json::json!({
                    "candidate": plan["after"],
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let value = json_body(response).await;
        assert_eq!(value["error"], "Profile prompt precondition failed");
        assert!(value["current_fingerprint"]["mtime_ns"].is_string());
        assert_eq!(
            fs::read_to_string(prompts_dir.join("issue.md")).unwrap(),
            "external\n"
        );
    }

    #[tokio::test]
    async fn workflow_list_reads_workflow_store() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        fs::write(
            workflows_dir.join("2026-05-28-001.toml"),
            sample_workflow("Workflow one", "single", "run-one"),
        )
        .unwrap();

        let response = app(test_state(dir.path()))
            .oneshot(authorized_get_request("/api/workflows"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["items"][0]["id"], "2026-05-28-001");
        assert_eq!(value["items"][0]["title"], "Workflow one");
        assert_eq!(value["items"][0]["mode"], "single");
    }

    #[tokio::test]
    async fn workflow_list_accepts_cookie_only_same_origin_get() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        fs::write(
            workflows_dir.join("2026-05-28-004.toml"),
            sample_workflow("Workflow four", "single", "run-four"),
        )
        .unwrap();

        let response = app(test_state(dir.path()))
            .oneshot(cookie_get_request("/api/workflows"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["items"][0]["id"], "2026-05-28-004");
    }

    #[tokio::test]
    async fn workflow_detail_reads_valid_id_only() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        fs::write(
            workflows_dir.join("2026-05-28-002.toml"),
            sample_workflow("Workflow two", "single", "run-two"),
        )
        .unwrap();

        let response = app(test_state(dir.path()))
            .oneshot(authorized_get_request("/api/workflows/2026-05-28-002"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let value = json_body(response).await;
        assert_eq!(value["id"], "2026-05-28-002");
        assert_eq!(value["title"], "Workflow two");
        assert_eq!(value["tasks"][0]["run"], "run-two");
    }

    #[tokio::test]
    async fn workflow_detail_rejects_traversal_and_missing_ids() {
        let dir = tempfile::tempdir().unwrap();
        let server = app(test_state(dir.path()));

        let invalid = server
            .clone()
            .oneshot(authorized_get_request("/api/workflows/abc"))
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let traversal = server
            .clone()
            .oneshot(authorized_get_request("/api/workflows/abc%2F..%2Fetc"))
            .await
            .unwrap();
        assert_eq!(traversal.status(), StatusCode::BAD_REQUEST);

        let missing = server
            .oneshot(authorized_get_request("/api/workflows/2026-05-28-003"))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn workflow_plan_and_apply_update_hot_fields_only() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        let path = workflows_dir.join("2026-05-28-005.toml");
        let before = sample_workflow("Workflow five", "single", "run-five");
        fs::write(&path, &before).unwrap();
        let candidate = before
            .replace("title = \"Workflow five\"", "title = \"Workflow edited\"")
            .replace("color = \"blue\"", "color = \"green\"")
            .replace("pull_request = \"ready\"", "pull_request = \"draft\"")
            .replace("landing = \"auto\"", "landing = \"manual\"");
        let server = app(test_state(dir.path()));

        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/workflows/2026-05-28-005/plan",
                serde_json::json!({
                    "candidate": candidate,
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();

        assert_eq!(plan_response.status(), StatusCode::OK);
        let plan = json_body(plan_response).await;
        assert_eq!(plan["before"], before);
        assert_eq!(plan["validation_errors"].as_array().unwrap().len(), 0);
        assert_eq!(plan["baseline_stale"], false);
        assert!(
            plan["diff"]
                .as_str()
                .unwrap()
                .contains("+title = \"Workflow edited\"")
        );
        assert!(
            plan["diff"]
                .as_str()
                .unwrap()
                .contains("-updated_at = \"2026-05-28T00:00:00Z\"")
        );

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/workflows/2026-05-28-005/apply",
                serde_json::json!({
                    "candidate": candidate,
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::OK);
        let applied = json_body(apply_response).await;
        assert!(applied["committed_fingerprint"]["mtime_ns"].is_string());
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("title = \"Workflow edited\""));
        assert!(written.contains("color = \"green\""));
        assert!(written.contains("pull_request = \"draft\""));
        assert!(written.contains("landing = \"manual\""));
        assert!(!written.contains("updated_at = \"2026-05-28T00:00:00Z\""));
        let workflow = workflow_model::read(&path).unwrap();
        assert_eq!(workflow.tasks[0].run, "run-five");
    }

    #[tokio::test]
    async fn workflow_apply_rejects_stale_precondition() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        let path = workflows_dir.join("2026-05-28-006.toml");
        let before = sample_workflow("Workflow six", "single", "run-six");
        fs::write(&path, &before).unwrap();
        let candidate = before.replace("title = \"Workflow six\"", "title = \"Workflow stale\"");
        let server = app(test_state(dir.path()));
        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/workflows/2026-05-28-006/plan",
                serde_json::json!({
                    "candidate": candidate,
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();
        let plan = json_body(plan_response).await;
        fs::write(
            &path,
            sample_workflow("Workflow external", "single", "run-six"),
        )
        .unwrap();

        let response = server
            .oneshot(authorized_json_request(
                "/api/workflows/2026-05-28-006/apply",
                serde_json::json!({
                    "candidate": candidate,
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let value = json_body(response).await;
        assert_eq!(value["error"], "Workflow precondition failed");
        assert!(value["current_fingerprint"]["mtime_ns"].is_string());
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("Workflow external")
        );
    }

    #[tokio::test]
    async fn workflow_plan_rejects_read_only_field_changes() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        let before = sample_workflow("Workflow seven", "single", "run-seven");
        fs::write(workflows_dir.join("2026-05-28-007.toml"), &before).unwrap();
        let server = app(test_state(dir.path()));
        let cases = [
            (
                "created_at",
                before.replace(
                    "created_at = \"2026-05-28T00:00:00Z\"",
                    "created_at = \"2026-05-28T00:00:01Z\"",
                ),
            ),
            (
                "updated_at",
                before.replace(
                    "updated_at = \"2026-05-28T00:00:00Z\"",
                    "updated_at = \"2026-05-28T00:00:01Z\"",
                ),
            ),
            (
                "mode",
                before.replace("mode = \"single\"", "mode = \"batch\""),
            ),
            (
                "base_mode",
                before.replace("base_mode = \"explicit\"", "base_mode = \"default\""),
            ),
            (
                "base",
                before.replace("base = \"develop\"", "base = \"main\""),
            ),
            (
                "profile",
                before.replace(
                    "mode = \"single\"\n",
                    "mode = \"single\"\nprofile = \"codex\"\n",
                ),
            ),
            (
                "profiles",
                before.replace(
                    "mode = \"single\"\n",
                    "mode = \"single\"\nprofiles = [\"codex\"]\n",
                ),
            ),
            (
                "tasks",
                before.replace("run = \"run-seven\"", "run = \"run-other\""),
            ),
            (
                "origin",
                before.replace(
                    "\n[policy]\n",
                    "\n[origin]\nprovider = \"github\"\nid = \"1\"\n\n[policy]\n",
                ),
            ),
        ];

        for (field, candidate) in cases {
            let response = server
                .clone()
                .oneshot(authorized_json_request(
                    "/api/workflows/2026-05-28-007/plan",
                    serde_json::json!({
                        "candidate": candidate,
                        "baseline_fingerprint": null
                    }),
                ))
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            let value = json_body(response).await;
            assert!(
                value["validation_errors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|error| error == &format!("field '{field}' is read-only in studio")),
                "{field} should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn workflow_apply_rejects_read_only_and_invalid_color_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        fs::create_dir_all(&workflows_dir).unwrap();
        let before = sample_workflow("Workflow eight", "single", "run-eight");
        fs::write(workflows_dir.join("2026-05-28-008.toml"), &before).unwrap();
        let server = app(test_state(dir.path()));
        let invalid_color = before.replace("color = \"blue\"", "color = \"ultraviolet\"");

        let plan_response = server
            .clone()
            .oneshot(authorized_json_request(
                "/api/workflows/2026-05-28-008/plan",
                serde_json::json!({
                    "candidate": invalid_color,
                    "baseline_fingerprint": null
                }),
            ))
            .await
            .unwrap();

        assert_eq!(plan_response.status(), StatusCode::OK);
        let plan = json_body(plan_response).await;
        assert_eq!(plan["validation_errors"][0], "invalid color: ultraviolet");
        assert_eq!(plan["after"], "");
        assert_eq!(plan["diff"], "");

        let apply_response = server
            .oneshot(authorized_json_request(
                "/api/workflows/2026-05-28-008/apply",
                serde_json::json!({
                    "candidate": before.replace("mode = \"single\"", "mode = \"batch\""),
                    "precondition": plan["fingerprint"]
                }),
            ))
            .await
            .unwrap();

        assert_eq!(apply_response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let value = json_body(apply_response).await;
        assert_eq!(value["error"], "Workflow validation failed");
        assert_eq!(
            value["validation_errors"][0],
            "field 'mode' is read-only in studio"
        );
    }

    #[test]
    fn browser_open_is_skipped_in_quiet_mode() {
        let (ctx, ui) = test_ctx(true);
        let mut opened = false;

        maybe_open_browser(&ctx, "http://127.0.0.1:8424/", |_| -> Result<()> {
            opened = true;
            Ok(())
        });

        assert!(!opened);
        assert!(ui.warnings.lock().unwrap().is_empty());
    }

    fn test_ctx(quiet: bool) -> (Ctx, Arc<MockUi>) {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new_with_options(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
            CtxOptions {
                output_mode: OutputMode::Text,
                quiet,
                ..Default::default()
            },
        );
        (ctx, ui)
    }

    fn test_state(repo_root: &Path) -> StudioState {
        StudioState::for_tests("http://127.0.0.1:8424", "secret", repo_root)
    }

    fn authorized_json_request(path: &str, value: serde_json::Value) -> Request<Body> {
        Request::post(path)
            .header(header::ORIGIN, "http://127.0.0.1:8424")
            .header(header::COOKIE, "wt_studio_session=secret")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&value).unwrap()))
            .unwrap()
    }

    fn authorized_get_request(path: &str) -> Request<Body> {
        Request::get(path)
            .header(header::ORIGIN, "http://127.0.0.1:8424")
            .header(header::COOKIE, "wt_studio_session=secret")
            .body(Body::empty())
            .unwrap()
    }

    fn cookie_get_request(path: &str) -> Request<Body> {
        Request::get(path)
            .header(header::COOKIE, "wt_studio_session=secret")
            .body(Body::empty())
            .unwrap()
    }

    fn sample_workflow(title: &str, mode: &str, run: &str) -> String {
        format!(
            "title = \"{title}\"\nmode = \"{mode}\"\nbase_mode = \"explicit\"\nbase = \"develop\"\ncolor = \"blue\"\ncreated_at = \"2026-05-28T00:00:00Z\"\nupdated_at = \"2026-05-28T00:00:00Z\"\n\n[policy]\npull_request = \"ready\"\nlanding = \"auto\"\n\n[[tasks]]\ntask = \"sample-task\"\nrun = \"{run}\"\n"
        )
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
