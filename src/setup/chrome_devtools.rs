use crate::config::{WorkspaceBrowserConfig, WorkspaceChromeDevtoolsConfig};
use crate::context::{CmdOutput, Ctx};
use crate::template;
use anyhow::{Context as AnyhowContext, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::{Path, PathBuf};

const CHROME_DEBUG_ADDRESS: &str = "127.0.0.1";
#[cfg(all(unix, not(target_os = "macos")))]
const CHROME_DEVTOOLS_COMMANDS: [&str; 4] = [
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
];

pub(crate) struct ChromeDevtoolsSession {
    port: u16,
    debug_url: String,
    user_data_dir: PathBuf,
    url: String,
    port_guard: TcpListener,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChromeDevtoolsMcpConfig {
    pub(crate) browser_url: String,
    pub(crate) claude_config_path: PathBuf,
}

impl ChromeDevtoolsSession {
    pub(crate) fn mcp_config(&self) -> ChromeDevtoolsMcpConfig {
        ChromeDevtoolsMcpConfig {
            browser_url: self.debug_url.clone(),
            claude_config_path: self.user_data_dir.join("wt-mcp.json"),
        }
    }
}

pub(super) fn prepare_chrome_devtools_session(
    chrome_devtools: &WorkspaceChromeDevtoolsConfig,
    browser: &WorkspaceBrowserConfig,
    wt_path: &Path,
    vars: &mut HashMap<String, String>,
) -> Result<ChromeDevtoolsSession> {
    let (port, port_guard) = reserve_debug_port(chrome_devtools.port)?;
    let debug_url = format!("http://{CHROME_DEBUG_ADDRESS}:{port}");
    vars.insert("chrome_debug_port".into(), port.to_string());
    vars.insert("chrome_debug_url".into(), debug_url.clone());

    let user_data_dir = render_user_data_dir(chrome_devtools, wt_path, vars)?;
    vars.insert(
        "chrome_user_data_dir".into(),
        user_data_dir.to_string_lossy().into_owned(),
    );

    let url = render_url(browser, vars)?;

    Ok(ChromeDevtoolsSession {
        port,
        debug_url,
        user_data_dir,
        url,
        port_guard,
    })
}

pub(super) fn launch_chrome_devtools(ctx: &Ctx, session: ChromeDevtoolsSession) -> Result<()> {
    fs::create_dir_all(&session.user_data_dir).with_context(|| {
        format!(
            "create Chrome DevTools user data dir {}",
            session.user_data_dir.display()
        )
    })?;

    ctx.ui.print_step(&format!(
        "Launching Chrome DevTools session: {}",
        session.debug_url
    ));

    let launch = ChromeDevtoolsLaunch {
        port: session.port,
        user_data_dir: session.user_data_dir.to_string_lossy().into_owned(),
        url: session.url,
    };

    drop(session.port_guard);
    launch_chrome_process(ctx, &launch)
}

struct ChromeDevtoolsLaunch {
    port: u16,
    user_data_dir: String,
    url: String,
}

fn reserve_debug_port(configured_port: Option<u16>) -> Result<(u16, TcpListener)> {
    if configured_port == Some(0) {
        bail!("[workspace.browser.chrome_devtools].port must be greater than 0");
    }

    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, configured_port.unwrap_or(0));
    let listener = TcpListener::bind(addr).with_context(|| {
        let port = configured_port
            .map(|port| port.to_string())
            .unwrap_or_else(|| "an available port".into());
        format!("reserve Chrome DevTools localhost port {port}")
    })?;
    let port = listener.local_addr()?.port();
    Ok((port, listener))
}

fn render_user_data_dir(
    chrome_devtools: &WorkspaceChromeDevtoolsConfig,
    wt_path: &Path,
    vars: &HashMap<String, String>,
) -> Result<PathBuf> {
    let rendered = template::render(chrome_devtools.effective_user_data_dir(), vars);
    if rendered.trim().is_empty() {
        bail!("[workspace.browser.chrome_devtools].user_data_dir cannot render to an empty path");
    }

    let path = PathBuf::from(rendered);
    Ok(if path.is_absolute() {
        path
    } else {
        wt_path.join(path)
    })
}

fn render_url(browser: &WorkspaceBrowserConfig, vars: &HashMap<String, String>) -> Result<String> {
    let rendered = template::render(
        browser
            .effective_url()
            .expect("chrome_devtools browser mode has a URL")
            .as_ref(),
        vars,
    );
    if rendered.trim().is_empty() {
        bail!("[workspace.browser].url cannot render to an empty URL");
    }
    Ok(rendered)
}

fn launch_chrome_process(ctx: &Ctx, launch: &ChromeDevtoolsLaunch) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        launch_chrome_macos(ctx, launch)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        launch_chrome_unix(ctx, launch)
    }

    #[cfg(not(unix))]
    {
        let _ = (ctx, launch);
        bail!(
            "workspace.browser.chrome_devtools automatic Chrome launch is currently supported on macOS and Unix-like systems with Google Chrome or Chromium on PATH"
        )
    }
}

#[cfg(target_os = "macos")]
fn launch_chrome_macos(ctx: &Ctx, launch: &ChromeDevtoolsLaunch) -> Result<()> {
    let port_arg = format!("--remote-debugging-port={}", launch.port);
    let user_data_dir_arg = format!("--user-data-dir={}", launch.user_data_dir);
    let args = [
        "-na",
        "Google Chrome",
        "--args",
        "--remote-debugging-address=127.0.0.1",
        port_arg.as_str(),
        user_data_dir_arg.as_str(),
        "--no-first-run",
        "--no-default-browser-check",
        "--new-window",
        launch.url.as_str(),
    ];

    ensure_launch_success(ctx.runner.run("open", &args, None)?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launch_chrome_unix(ctx: &Ctx, launch: &ChromeDevtoolsLaunch) -> Result<()> {
    let Some(command) = CHROME_DEVTOOLS_COMMANDS
        .iter()
        .find(|command| ctx.runner.has_command(command))
    else {
        bail!(
            "workspace.browser.chrome_devtools requires Google Chrome or Chromium on PATH; tried {}",
            CHROME_DEVTOOLS_COMMANDS.join(", ")
        );
    };

    let port_arg = format!("--remote-debugging-port={}", launch.port);
    let user_data_dir_arg = format!("--user-data-dir={}", launch.user_data_dir);
    let args = [
        "-c",
        "nohup \"$@\" >/dev/null 2>&1 &",
        "wt-chrome-devtools",
        *command,
        "--remote-debugging-address=127.0.0.1",
        port_arg.as_str(),
        user_data_dir_arg.as_str(),
        "--no-first-run",
        "--no-default-browser-check",
        "--new-window",
        launch.url.as_str(),
    ];

    ensure_launch_success(ctx.runner.run("sh", &args, None)?)
}

fn ensure_launch_success(out: CmdOutput) -> Result<()> {
    if out.success {
        return Ok(());
    }

    bail!("Chrome DevTools launch failed: {}", command_error(&out));
}

fn command_error(out: &CmdOutput) -> String {
    match (out.stderr.trim().is_empty(), out.stdout.trim().is_empty()) {
        (false, false) => format!(
            "stderr: {}; stdout: {}",
            out.stderr.trim(),
            out.stdout.trim()
        ),
        (false, true) => out.stderr.trim().to_string(),
        (true, false) => out.stdout.trim().to_string(),
        (true, true) => "empty output".into(),
    }
}
