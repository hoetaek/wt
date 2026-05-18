use super::SiteDescriptor;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::services::site::provider_label;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn print_summary(
    ctx: &Ctx,
    wt_path: &Path,
    _names: &WorktreeNames,
    site: Option<&SiteDescriptor>,
    vars: &HashMap<String, String>,
) {
    ctx.ui.print_step("Done!");
    ctx.ui.print_step(&format!("  Path: {}", wt_path.display()));
    if let Some(url) = vars.get("site_url") {
        ctx.ui.print_step(&format!("  UI: {url}"));
    }
    if let Some(url) = vars.get("api_url") {
        ctx.ui.print_step(&format!("  API: {url}"));
    }
    if let Some(url) = vars.get("chrome_debug_url") {
        ctx.ui.print_step(&format!("  Chrome DevTools: {url}"));
    }
    if let Some(site) = site {
        ctx.ui.print_step(&format!(
            "  {}: {}",
            provider_label(&site.provider),
            site.url
        ));
    }
}
