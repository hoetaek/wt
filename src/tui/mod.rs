pub(crate) mod app;
pub(crate) mod browser;
pub(crate) mod render;
pub(crate) mod terminal;

pub(crate) fn run_task_browser_with(rows: Vec<app::BrowserRow>) -> anyhow::Result<()> {
    browser::run_browser(rows)
}

pub(crate) fn terminal_size_allows_task_browser() -> bool {
    browser::terminal_size_allows_browser()
}
