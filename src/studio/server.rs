use crate::context::Ctx;
use crate::storage::StorageRoot;
use crate::studio::auth::{COOKIE_NAME, StudioSession, mint_session_token};
use crate::task::{self, TaskDocument};
use anyhow::{Context, Result, bail};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Display;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
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
}

impl StudioState {
    fn new(ctx: &Ctx, origin: String, token: String) -> Self {
        Self {
            session: Arc::new(StudioSession::new(origin, token)),
            repo_root: Arc::new(ctx.repo_root.clone()),
            storage_root: ctx.storage_root.clone(),
            task_document_apply_lock: Arc::new(Mutex::new(())),
        }
    }

    #[cfg(test)]
    fn for_tests(origin: &str, token: &str, repo_root: &Path) -> Self {
        Self {
            session: Arc::new(StudioSession::new(origin.into(), token.into())),
            repo_root: Arc::new(repo_root.to_path_buf()),
            storage_root: StorageRoot::from_git_common_dir(repo_root.join(".git")),
            task_document_apply_lock: Arc::new(Mutex::new(())),
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

const TASK_DOCUMENT_PATH_PREFIX: &str = "<git-common-dir>/wt/execution/tasks/";

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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct FileFingerprint {
    mtime_ns: Option<String>,
    hash: String,
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
    let diff = unified_diff(&resolved.display_path, &before_disk.content, &after);

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
                "diff": unified_diff(&resolved.display_path, &request.before, &current.content),
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
            "TaskDocument path must stay under <git-common-dir>/wt/execution/tasks",
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
    match fs::read(path) {
        Ok(bytes) => {
            let content = String::from_utf8(bytes).map_err(|err| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!("TaskDocument is not UTF-8: {err}"),
                )
            })?;
            let metadata = fs::metadata(path).map_err(|err| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to stat TaskDocument: {err}"),
                )
            })?;
            Ok(DiskTaskDocument {
                fingerprint: fingerprint(&content, Some(&metadata)),
                content,
                exists: true,
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DiskTaskDocument {
            content: String::new(),
            fingerprint: empty_fingerprint(),
            exists: false,
        }),
        Err(err) => Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read TaskDocument: {err}"),
        )),
    }
}

fn empty_fingerprint() -> FileFingerprint {
    fingerprint("", None)
}

fn fingerprint(content: &str, metadata: Option<&fs::Metadata>) -> FileFingerprint {
    FileFingerprint {
        mtime_ns: metadata.and_then(mtime_ns),
        hash: sha256_hex(content.as_bytes()),
    }
}

fn mtime_ns(metadata: &fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let elapsed = modified.duration_since(UNIX_EPOCH).ok()?;
    let nanos = u64::from(elapsed.subsec_nanos());
    elapsed
        .as_secs()
        .checked_mul(1_000_000_000)
        .and_then(|secs| secs.checked_add(nanos))
        .map(|mtime| mtime.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn unified_diff(path: &str, before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }

    let old_lines = split_lines(before);
    let new_lines = split_lines(after);
    let ops = diff_ops(&old_lines, &new_lines);
    let old_start = if old_lines.is_empty() { 0 } else { 1 };
    let new_start = if new_lines.is_empty() { 0 } else { 1 };
    let mut diff = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{old_start},{} +{new_start},{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for op in ops {
        match op {
            DiffOp::Equal(line) => push_diff_line(&mut diff, ' ', line),
            DiffOp::Delete(line) => push_diff_line(&mut diff, '-', line),
            DiffOp::Insert(line) => push_diff_line(&mut diff, '+', line),
        }
    }
    diff
}

fn split_lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        Vec::new()
    } else {
        content.split_inclusive('\n').collect()
    }
}

#[derive(Clone, Copy, Debug)]
enum DiffOp<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

fn diff_ops<'a>(old_lines: &[&'a str], new_lines: &[&'a str]) -> Vec<DiffOp<'a>> {
    let old_len = old_lines.len();
    let new_len = new_lines.len();
    let mut lcs = vec![0usize; (old_len + 1) * (new_len + 1)];
    for old_idx in (0..old_len).rev() {
        for new_idx in (0..new_len).rev() {
            let idx = lcs_index(new_idx, new_len, old_idx);
            lcs[idx] = if old_lines[old_idx] == new_lines[new_idx] {
                lcs[lcs_index(new_idx + 1, new_len, old_idx + 1)] + 1
            } else {
                lcs[lcs_index(new_idx, new_len, old_idx + 1)]
                    .max(lcs[lcs_index(new_idx + 1, new_len, old_idx)])
            };
        }
    }

    let mut ops = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;
    while old_idx < old_len && new_idx < new_len {
        if old_lines[old_idx] == new_lines[new_idx] {
            ops.push(DiffOp::Equal(old_lines[old_idx]));
            old_idx += 1;
            new_idx += 1;
        } else if lcs[lcs_index(new_idx, new_len, old_idx + 1)]
            >= lcs[lcs_index(new_idx + 1, new_len, old_idx)]
        {
            ops.push(DiffOp::Delete(old_lines[old_idx]));
            old_idx += 1;
        } else {
            ops.push(DiffOp::Insert(new_lines[new_idx]));
            new_idx += 1;
        }
    }
    while old_idx < old_len {
        ops.push(DiffOp::Delete(old_lines[old_idx]));
        old_idx += 1;
    }
    while new_idx < new_len {
        ops.push(DiffOp::Insert(new_lines[new_idx]));
        new_idx += 1;
    }
    ops
}

fn lcs_index(new_idx: usize, new_len: usize, old_idx: usize) -> usize {
    old_idx * (new_len + 1) + new_idx
}

fn push_diff_line(diff: &mut String, prefix: char, line: &str) {
    diff.push(prefix);
    diff.push_str(line);
    if !line.ends_with('\n') {
        diff.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CtxOptions, OutputMode};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
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
        let tasks_dir = dir.path().join(".git/wt/execution/tasks");
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
            fs::read_to_string(dir.path().join(".git/wt/execution/tasks/create-me.toml")).unwrap();
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
        let tasks_dir = dir.path().join(".git/wt/execution/tasks");
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
                .join(".git/wt/execution/tasks/bad-schema.toml")
                .exists()
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

    async fn json_body(response: Response) -> serde_json::Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
