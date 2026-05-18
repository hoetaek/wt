use super::SiteDescriptor;
use crate::config::{Config, SiteConfig, SiteProvider};
use crate::context::Ctx;
use crate::services::site::{SiteService, provider_label};
use crate::template;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

pub(crate) fn apply_site_template_vars(
    config: &Config,
    vars: &mut HashMap<String, String>,
) -> Option<SiteDescriptor> {
    let site = config.effective_site()?;
    let name = render_site_name(site.effective_name(), vars);
    vars.insert("site_name".into(), name.clone());

    let secure = site.effective_secure();
    let url = render_site_url(&site, vars);
    vars.insert("site_url".into(), url.clone());
    let target = render_site_target(&site, vars);
    let root = site.effective_root().into();
    let open_browser = Some(site.effective_open_browser());

    Some(SiteDescriptor {
        provider: site.provider,
        name,
        url,
        root,
        secure,
        target,
        open_browser,
        browser: site.browser,
    })
}

pub(super) fn register_site(ctx: &Ctx, wt_path: &Path, site: &SiteDescriptor) {
    let site_service = SiteService::new(ctx.runner.as_ref());
    if site_service.is_available(&site.provider) {
        let site_root = wt_path.join(&site.root);
        let action = if site.provider == SiteProvider::DockerProxy {
            "Using"
        } else {
            "Registering"
        };
        ctx.ui.print_step(&format!(
            "{action} {} site: {}",
            provider_label(&site.provider),
            site.url
        ));
        if let Err(e) = site_service.register(
            &site.provider,
            &site.name,
            &site_root,
            site.secure,
            site.target.as_deref(),
        ) {
            ctx.ui.print_warning(&format!(
                "{} link failed: {e}",
                provider_label(&site.provider)
            ));
        }
    } else {
        ctx.ui.print_warning(&format!(
            "{} command not found; skipping site registration",
            provider_label(&site.provider)
        ));
    }
}

pub(crate) fn open_site_url(
    ctx: &Ctx,
    site: &SiteDescriptor,
    already_opened: Option<&str>,
) -> Result<Option<String>> {
    if !site.open_browser.unwrap_or(false) {
        return Ok(None);
    }
    if already_opened == Some(site.url.as_str()) {
        return Ok(None);
    }

    if let Some(app) = site.browser.as_deref() {
        ctx.runner.run("open", &["-a", app, &site.url], None).ok();
    } else {
        ctx.runner.run("open", &[&site.url], None).ok();
    }

    Ok(Some(site.url.clone()))
}

fn render_site_name(template_value: &str, vars: &HashMap<String, String>) -> String {
    let rendered = template::render(template_value, vars);
    if rendered.contains("{{") {
        let repo = vars.get("repo").map(String::as_str).unwrap_or("repo");
        let branch_slug = vars
            .get("branch_slug")
            .map(String::as_str)
            .unwrap_or("worktree");
        format!("{repo}-{branch_slug}")
    } else {
        rendered
    }
}

fn render_site_url(site: &SiteConfig, vars: &HashMap<String, String>) -> String {
    template::render(site.effective_url().as_ref(), vars)
}

fn render_site_target(site: &SiteConfig, vars: &HashMap<String, String>) -> Option<String> {
    let template_value = site.effective_target()?;
    Some(template::render(template_value, vars))
}
