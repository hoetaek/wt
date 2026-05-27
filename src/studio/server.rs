use crate::context::Ctx;
use crate::studio::auth::{COOKIE_NAME, StudioSession, mint_session_token};
use anyhow::{Context, Result, bail};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use std::fmt::Display;
use std::sync::Arc;
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
}

impl StudioState {
    fn new(origin: String, token: String) -> Self {
        Self {
            session: Arc::new(StudioSession::new(origin, token)),
        }
    }

    #[cfg(test)]
    fn for_tests(origin: &str, token: &str) -> Self {
        Self::new(origin.into(), token.into())
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
        ctx.ui
            .print_dim("Serving the wt studio skeleton. Press Ctrl-C to stop.");
    }

    maybe_open_browser(ctx, &auth_url, |url| opener::open_browser(url));

    axum::serve(listener, app(StudioState::new(origin, token)))
        .await
        .context("wt studio server failed")
}

pub fn app(state: StudioState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/auth", get(auth_handler))
        .route("/api/ping", get(api_ping))
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
    async fn app_serves_stub_page() {
        let response = app(StudioState::for_tests("http://127.0.0.1:8424", "secret"))
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("Studio (stub)"));
    }

    #[tokio::test]
    async fn auth_sets_http_only_cookie_and_redirects() {
        let app = app(StudioState::for_tests("http://127.0.0.1:8424", "secret"));

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
        let response = app(StudioState::for_tests("http://127.0.0.1:8424", "secret"))
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
        let response = app(StudioState::for_tests("http://127.0.0.1:8424", "secret"))
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
        let response = app(StudioState::for_tests("http://127.0.0.1:8424", "secret"))
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
}
