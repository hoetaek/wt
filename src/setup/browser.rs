use super::chrome_devtools;
use super::chrome_devtools::ChromeDevtoolsMcpConfig;
use crate::config::{Config, WorkspaceBrowserConfig, WorkspaceBrowserMode};
use crate::context::Ctx;
use crate::template;
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::path::Path;

pub(crate) enum BrowserLaunch {
    System { url: String, app: Option<String> },
    ChromeDevtools(chrome_devtools::ChromeDevtoolsSession),
}

impl BrowserLaunch {
    pub(crate) fn chrome_devtools_mcp_config(&self) -> Option<ChromeDevtoolsMcpConfig> {
        match self {
            BrowserLaunch::ChromeDevtools(session) => Some(session.mcp_config()),
            BrowserLaunch::System { .. } => None,
        }
    }
}

pub(crate) fn prepare_browser_launch(
    config: &Config,
    wt_path: &Path,
    vars: &mut HashMap<String, String>,
) -> Result<Option<BrowserLaunch>> {
    let Some(workspace) = config.workspace.as_ref() else {
        return Ok(None);
    };
    let Some(browser) = workspace.browser.as_ref() else {
        return Ok(None);
    };

    match browser.mode {
        WorkspaceBrowserMode::None => Ok(None),
        WorkspaceBrowserMode::System => Ok(Some(BrowserLaunch::System {
            url: render_browser_url(browser, vars)?,
            app: browser.app.clone(),
        })),
        WorkspaceBrowserMode::ChromeDevtools => {
            let chrome_devtools = workspace.chrome_devtools.clone().unwrap_or_default();
            Ok(Some(BrowserLaunch::ChromeDevtools(
                chrome_devtools::prepare_chrome_devtools_session(
                    &chrome_devtools,
                    browser,
                    wt_path,
                    vars,
                )?,
            )))
        }
    }
}

pub(crate) fn launch_browser(ctx: &Ctx, launch: Option<BrowserLaunch>) -> Result<()> {
    match launch {
        Some(BrowserLaunch::System { url, app }) => {
            open_system_browser(ctx, &url, app.as_deref());
            Ok(())
        }
        Some(BrowserLaunch::ChromeDevtools(session)) => {
            chrome_devtools::launch_chrome_devtools(ctx, session)
        }
        None => Ok(()),
    }
}

fn open_system_browser(ctx: &Ctx, url: &str, app: Option<&str>) {
    if let Some(app) = app {
        ctx.runner.run("open", &["-a", app, url], None).ok();
    } else {
        ctx.runner.run("open", &[url], None).ok();
    }
}

fn render_browser_url(
    browser: &WorkspaceBrowserConfig,
    vars: &HashMap<String, String>,
) -> Result<String> {
    let template_value = browser
        .effective_url()
        .expect("active workspace browser mode has a URL");
    let rendered = template::render(template_value.as_ref(), vars);
    if rendered.trim().is_empty() {
        bail!("[workspace.browser].url cannot render to an empty URL");
    }
    Ok(rendered)
}
