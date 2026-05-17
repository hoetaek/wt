use super::SiteDescriptor;
use crate::config::{Config, SiteConfig, SiteProvider};
use crate::context::Ctx;
use crate::services::site::{SiteService, provider_label};
use crate::template;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

const DEFAULT_SITE_NAME_TEMPLATE: &str = "{{repo}}-{{branch_slug}}";

pub(crate) fn apply_site_template_vars(
    config: &Config,
    vars: &mut HashMap<String, String>,
) -> Option<SiteDescriptor> {
    let site = config.effective_site()?;
    let name_template = site.name.as_deref().unwrap_or(DEFAULT_SITE_NAME_TEMPLATE);
    let name = render_site_name(name_template, vars);
    vars.insert("site_name".into(), name.clone());

    let secure = site.secure.unwrap_or(true);
    let url = render_site_url(&site, &name, secure, vars);
    vars.insert("site_url".into(), url.clone());
    let target = render_site_target(&site, vars);

    Some(SiteDescriptor {
        provider: site.provider,
        name,
        url,
        root: site.root.unwrap_or_else(|| ".".into()),
        secure,
        target,
        open_browser: site.open_browser,
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

fn render_site_url(
    site: &SiteConfig,
    site_name: &str,
    secure: bool,
    vars: &HashMap<String, String>,
) -> String {
    if let Some(url_template) = site.url.as_deref() {
        return template::render(url_template, vars);
    }

    let scheme = if secure { "https" } else { "http" };
    format!("{scheme}://{site_name}.test")
}

fn render_site_target(site: &SiteConfig, vars: &HashMap<String, String>) -> Option<String> {
    let template_value = match site.target.as_deref() {
        Some(target) => target,
        None if site.provider == SiteProvider::Traefik => "http://127.0.0.1:{{vite_port}}",
        None => return None,
    };
    Some(template::render(template_value, vars))
}
