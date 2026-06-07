pub(crate) mod app;
pub(crate) mod browser;
pub(crate) mod dispatch;
pub(crate) mod remote_ui;
pub(crate) mod render;
pub(crate) mod terminal;
pub(crate) mod theme;

pub(crate) fn run_task_browser_with(
    ctx: &crate::context::Ctx,
    app: app::AppState,
    refresh: impl FnMut() -> anyhow::Result<(Vec<app::BrowserRow>, Vec<String>)>,
) -> anyhow::Result<()> {
    browser::run_browser(ctx, app, refresh)
}

pub(crate) fn run_workflow_browser(
    ctx: &crate::context::Ctx,
    app: app::AppState,
    refresh: impl FnMut() -> anyhow::Result<(Vec<app::BrowserRow>, Vec<String>)>,
) -> anyhow::Result<()> {
    browser::run_workflow_browser(ctx, app, refresh)
}

pub(crate) fn terminal_size_allows_task_browser() -> bool {
    browser::terminal_size_allows_browser()
}

pub(crate) fn terminal_size_allows_workflow_browser() -> bool {
    browser::terminal_size_allows_browser()
}
