use crate::config::Config;
use crate::context::Ctx;
use crate::services::cmux::CmuxService;
use crate::template;
use anyhow::Result;
use std::collections::HashMap;

pub(super) fn open_post_deps_tabs(
    ctx: &Ctx,
    config: &Config,
    ws_handle: &str,
    template_vars: &HashMap<String, String>,
) -> Result<()> {
    let Some(ws_config) = &config.workspace else {
        return Ok(());
    };
    if ws_config.post_deps_tabs.is_empty() {
        return Ok(());
    }

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let panes = cmux.list_panes(ws_handle)?;
    if let Some(pane) = panes.first() {
        for tab_cmd in &ws_config.post_deps_tabs {
            let rendered = template::render(tab_cmd, template_vars);
            let surface = cmux.new_surface(pane, ws_handle)?;
            cmux.send(&surface, ws_handle, &format!("{rendered}\n"))?;
        }
    }

    Ok(())
}
