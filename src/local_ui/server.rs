use crate::context::Ctx;
use crate::local_ui::snapshot::{self, SnapshotState};
use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use std::fmt::Display;
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
            .print_dim("Serving read-only wt personal state. Press Ctrl-C to stop.");
    }

    maybe_open_browser(ctx, &url, |url| opener::open_browser(url));

    axum::serve(listener, app(state))
        .await
        .context("local UI server failed")
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

pub fn app(state: SnapshotState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/snapshot", get(snapshot_handler))
        .route("/assets/app.css", get(stylesheet))
        .route("/assets/app.js", get(script))
        .route("/favicon.ico", get(favicon))
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

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
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
    use crate::context::CtxOptions;
    use crate::context::mock::{MockRunner, MockUi};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use std::fs;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn config_tab_keeps_source_paths_out_of_repeated_list_rows() {
        assert!(APP_JS.contains("function renderConfig(snapshot)"));
        assert!(APP_JS.contains("relationships: config.paths.slice().sort"));
        assert!(APP_JS.contains("function configSourceLayerPills(config)"));
        assert!(!APP_JS.contains("function settingsFileMasterDetailRecord(row"));
        assert!(APP_JS.contains("function sourceLayerLabel(path"));
        assert!(APP_JS.contains("function settingsFileOrder(path)"));
        assert!(!APP_JS.contains("paths: config.paths,"));
        assert!(!APP_JS.contains("], paths(config.paths)),"));
    }

    #[test]
    fn embedded_frontend_includes_operational_dashboard_shell() {
        assert!(!INDEX_HTML.contains("class=\"state-strip\""));
        assert!(INDEX_HTML.contains("id=\"workspace-label\""));
        assert!(INDEX_HTML.contains("role=\"tablist\""));
        assert!(INDEX_HTML.contains("aria-selected=\"true\""));
        assert!(INDEX_HTML.contains("id=\"tab-overview\""));
        assert!(INDEX_HTML.contains("data-view=\"config\""));
        assert!(INDEX_HTML.contains("data-view=\"retrospecs\""));
        assert!(
            INDEX_HTML.find("class=\"tabs\"").unwrap()
                < INDEX_HTML.find("class=\"top-actions\"").unwrap()
        );
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
        assert!(APP_JS.contains("function compactPath(path)"));
        assert!(APP_JS.contains("function repoContextLabel(repo)"));
        assert!(APP_JS.contains("function attentionStat(count)"));
        assert!(APP_JS.contains("sourceSectionTitle"));
        assert!(APP_JS.contains("설정 목록"));
        assert!(APP_JS.contains("로컬 설정"));
        assert!(APP_JS.contains("공유 설정"));
        assert!(APP_JS.contains("function settingsLayerTone(path)"));
        assert!(APP_JS.contains("function profileEffectiveCards(row)"));
        assert!(APP_JS.contains("function profileTomlCard(card)"));
        assert!(APP_JS.contains("profileOnlyBadge: \"profile.toml\""));
        assert!(APP_JS.contains("listMarker"));
        assert!(APP_CSS.contains(".master-dot"));
        assert!(APP_CSS.contains(".pill.layer-local"));
        assert!(APP_CSS.contains(".detail-card.is-profile-only"));
        assert!(APP_JS.contains("hideSummarySectionTitle"));
        assert!(!APP_JS.contains("Review values"));
        assert!(!APP_JS.contains("승인 기준"));
        assert!(!APP_JS.contains("프로필 영향"));
        assert!(!APP_JS.contains("Applied from"));
        assert!(!APP_JS.contains("Audit source"));
        assert!(!APP_JS.contains("적용 근거"));
        assert!(!APP_JS.contains("검증용 원문"));
        assert!(APP_JS.contains("로컬 TOML 경로"));
        assert!(APP_JS.contains("프로필 TOML 경로"));
        assert!(APP_JS.contains("namingLabel: \"naming\""));
        assert!(APP_JS.contains("depsLabel: \"deps\""));
        assert!(APP_JS.contains("envLabel: \"env\""));
        assert!(APP_JS.contains("chromeDevtoolsLabel: \"chrome_devtools\""));
        assert!(APP_JS.contains("promptModesLabel: \"prompt\""));
        assert!(APP_JS.contains("function agentPromptSummary(agent)"));
        assert!(APP_JS.contains("collapseSources: true"));
        assert!(APP_JS.contains("class=\"detail-source\"><summary>"));
        assert!(APP_JS.contains("detailCards(record.cards"));
        assert!(APP_JS.contains("landingHelp"));
        assert!(APP_JS.contains("postDepsTabsLabel: \"post_deps_tabs\""));
        assert!(!APP_JS.contains("이름 생성"));
        assert!(!APP_JS.contains("의존성 명령"));
        assert!(!APP_JS.contains("프롬프트 범위"));
        assert!(!APP_JS.contains("setup 후 탭"));
        assert!(!APP_JS.contains("의존성 후 탭"));
        assert!(!APP_JS.contains("landingLabel: \"랜딩\""));
        assert!(!APP_JS.contains("동작 요약"));
        assert!(!APP_JS.contains("설정 색인"));
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
        assert!(APP_CSS.contains(".master-list-group"));
        assert!(APP_CSS.contains(".master-list-row"));
        assert!(APP_CSS.contains(".detail-cards"));
        assert!(APP_CSS.contains(".detail-pane"));
        assert!(APP_CSS.contains(".workflow-relationship-summary"));
        assert!(APP_CSS.contains(".relationship-segment"));
        assert!(APP_CSS.contains(".workflow-canvas"));
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
        assert!(APP_JS.contains("workflowMasterDetailRecord"));
        assert!(APP_JS.contains("workflowRelationshipSummary"));
        assert!(APP_JS.contains("workflowCanvasSection"));
        assert!(APP_JS.contains("ideaMasterDetailRecord"));
        assert!(APP_JS.contains("retrospecMasterDetailRecord"));
        assert!(APP_JS.contains("invalidPlanningMasterDetailRecord"));
        assert!(APP_JS.contains("TaskDocument"));
        assert!(APP_JS.contains("agentNotObserved"));
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
        assert!(APP_JS.contains("configSourceLayerPills(config)"));
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
        fs::create_dir_all(dir.path().join(".git/wt/tasks")).unwrap();
        fs::write(
            dir.path().join(".git/wt/tasks/demo.toml"),
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
        assert!(String::from_utf8_lossy(&body).contains("wt ui"));

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
        assert_eq!(
            value["tasks"]["items"][0]["path"],
            "<git-common-dir>/wt/tasks/demo.toml"
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

    #[test]
    fn browser_open_failure_warns_without_failing() {
        let (ctx, ui) = test_ctx(false);

        maybe_open_browser(&ctx, "http://127.0.0.1:8424/", |_| -> Result<()> {
            Err(anyhow::anyhow!("launcher unavailable"))
        });

        assert_eq!(
            ui.warnings.lock().unwrap().as_slice(),
            ["Could not open browser automatically: launcher unavailable"]
        );
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
                quiet,
                ..Default::default()
            },
        );
        (ctx, ui)
    }
}
