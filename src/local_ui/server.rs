use crate::context::Ctx;
use crate::local_ui::snapshot::{self, SnapshotState};
use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_CSS: &str = include_str!("assets/app.css");
const APP_JS: &str = include_str!("assets/app.js");

pub struct ServerOptions {
    pub host: String,
    pub port: u16,
}

pub async fn serve(ctx: &Ctx, options: ServerOptions) -> Result<()> {
    let state = SnapshotState::from_ctx(ctx);
    let listener = TcpListener::bind((options.host.as_str(), options.port))
        .await
        .with_context(|| {
            format!(
                "failed to bind local UI server on {}:{}",
                options.host, options.port
            )
        })?;
    let addr = listener.local_addr()?;
    let url = format!("http://{addr}/");

    if ctx.quiet {
        println!("{url}");
    } else {
        ctx.ui.print_step(&format!("Local wt UI: {url}"));
        ctx.ui
            .print_dim("Serving read-only wt local state. Press Ctrl-C to stop.");
    }

    axum::serve(listener, app(state))
        .await
        .context("local UI server failed")
}

pub fn app(state: SnapshotState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/snapshot", get(snapshot_handler))
        .route("/assets/app.css", get(stylesheet))
        .route("/assets/app.js", get(script))
        .fallback(not_found)
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn snapshot_handler(State(state): State<SnapshotState>) -> Response {
    match snapshot::build(&state) {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("{err:#}"),
            })),
        )
            .into_response(),
    }
}

async fn stylesheet() -> Response {
    static_response("text/css; charset=utf-8", APP_CSS)
}

async fn script() -> Response {
    static_response("application/javascript; charset=utf-8", APP_JS)
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn static_response(content_type: &'static str, body: &'static str) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("static response should be valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigSource};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use std::fs;
    use tower::ServiceExt;

    #[test]
    fn config_tab_passes_source_paths_to_card_renderer() {
        assert!(APP_JS.contains("function renderConfig(snapshot)"));
        assert!(APP_JS.contains("paths: config.paths,"));
        assert!(!APP_JS.contains("], paths(config.paths)),"));
    }

    #[test]
    fn embedded_frontend_includes_operational_dashboard_shell() {
        assert!(!INDEX_HTML.contains("class=\"state-strip\""));
        assert!(INDEX_HTML.contains("role=\"tablist\""));
        assert!(INDEX_HTML.contains("aria-selected=\"true\""));
        assert!(INDEX_HTML.contains("id=\"tab-overview\""));
        assert!(INDEX_HTML.contains("data-view=\"config\""));
        assert!(INDEX_HTML.contains("data-view=\"retrospecs\""));
        assert!(!INDEX_HTML.contains("data-view=\"profiles\""));
        assert!(!INDEX_HTML.contains("data-view=\"tasks\""));
        assert!(INDEX_HTML.contains("role=\"tabpanel\""));
        assert!(INDEX_HTML.contains("id=\"language-toggle\""));
        assert!(INDEX_HTML.contains("class=\"language-switch\""));
        assert!(!INDEX_HTML.contains("id=\"refresh\""));
        assert!(APP_CSS.contains(".section-heading"));
        assert!(APP_CSS.contains(".jump-nav"));
        assert!(APP_CSS.contains(".top-actions .language-switch"));
        assert!(APP_CSS.contains(".language-switch[data-current=\"en\"]"));
        assert!(!APP_CSS.contains(".state-strip"));
        assert!(APP_CSS.contains(".metrics[data-view]:not([data-view=\"overview\"])"));
        assert!(APP_CSS.contains(".focus-panel"));
        assert!(APP_CSS.contains(".focus-inspector"));
        assert!(APP_CSS.contains(".source-panel"));
        assert!(APP_CSS.contains(".status-strip-inline"));
        assert!(APP_CSS.contains(".scan-list"));
        assert!(APP_CSS.contains(".scan-row"));
        assert!(APP_CSS.contains(".scan-meta"));
        assert!(APP_CSS.contains(".master-detail-shell"));
        assert!(APP_CSS.contains(".master-list-row"));
        assert!(APP_CSS.contains(".detail-pane"));
        assert!(APP_CSS.contains(".record-list"));
        assert!(APP_CSS.contains(".record-card.tone-green::before"));
        assert!(APP_CSS.contains(".read-more.is-open"));
        assert!(APP_CSS.contains(".summary-full"));
        assert!(!APP_CSS.contains(".collapse-inline"));
        assert!(APP_CSS.contains(".mobile-meta"));
        assert!(APP_CSS.contains(".full-text pre"));
        assert!(APP_CSS.contains(".markdown-body h1"));
        assert!(APP_JS.contains("metric invalid"));
        assert!(APP_JS.contains("Focus inspector"));
        assert!(APP_JS.contains("overviewFocusModel"));
        assert!(APP_JS.contains("Config cockpit"));
        assert!(APP_JS.contains("masterDetailPanel"));
        assert!(APP_JS.contains("handleMasterDetailKeydown"));
        assert!(APP_JS.contains("configMasterDetailRecords"));
        assert!(APP_JS.contains("workflowScanRow"));
        assert!(APP_JS.contains("taskRunScanRow"));
        assert!(APP_JS.contains("data-read-toggle"));
        assert!(APP_JS.contains("summary-full full-text"));
        assert!(APP_JS.contains("source-panel full-text"));
        assert!(!APP_JS.contains("</summary><div class=\"full-text\""));
        assert!(APP_JS.contains("전문 보기"));
        assert!(APP_JS.contains("확인 필요"));
        assert!(APP_JS.contains("tabWorkflows: \"워크플로우\""));
        assert!(APP_JS.contains("tabTaskRuns: \"작업 실행\""));
        assert!(APP_JS.contains("workflowUiGroup"));
        assert!(APP_JS.contains("taskRunNeedsAttention"));
        assert!(APP_JS.contains("config.source_files || []"));
        assert!(APP_JS.contains("effective_text"));
        assert!(APP_JS.contains("localStorage.setItem(LOCALE_KEY"));
        assert!(!APP_JS.contains("data-collapse"));
        assert!(APP_JS.contains("aria-selected"));
        assert!(APP_JS.contains("formatInlineMarkdown"));
        assert!(APP_JS.contains("TaskRun status"));
        assert!(APP_JS.contains("formatTaskRunState"));
        assert!(!APP_JS.contains("Linked TaskDocument"));
        assert!(!APP_JS.contains("TaskRun TOML"));
        assert!(APP_JS.contains("formatWorkflowTaskRuns"));
        assert!(APP_JS.contains("retrospecs"));
    }

    #[tokio::test]
    async fn app_serves_static_assets_and_snapshot_route() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".local/tasks")).unwrap();
        fs::write(
            dir.path().join(".local/tasks/demo.toml"),
            "title = \"Demo\"\nbranch = \"feature/demo\"\nbody = \"Demo body\"\n",
        )
        .unwrap();

        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            Config::default(),
            Config::default(),
            ConfigSource::Default,
        );
        let app = app(state);

        let response = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("wt local state"));

        let response = app
            .clone()
            .oneshot(Request::get("/assets/app.js").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(Request::get("/api/snapshot").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["tasks"]["items"][0]["path"], ".local/tasks/demo.toml");
    }

    #[tokio::test]
    async fn app_does_not_serve_repo_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".env"), "SECRET=value\n").unwrap();
        let state = SnapshotState::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            "repo".into(),
            Config::default(),
            Config::default(),
            ConfigSource::Default,
        );

        let response = app(state)
            .oneshot(Request::get("/.env").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
