use crate::config::Config;
use crate::config_render::render_effective_config;
use crate::context::Ctx;
use anyhow::{Result, anyhow};
use std::borrow::Cow;
use std::io::Write;

pub fn effective(ctx: &Ctx, profile: Option<&str>) -> Result<()> {
    let config = effective_config(ctx, profile)?;
    let rendered = render_effective_config(&config);

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(rendered.as_bytes())?;
    Ok(())
}

fn effective_config<'a>(ctx: &'a Ctx, profile: Option<&str>) -> Result<Cow<'a, Config>> {
    let Some(profile) = profile else {
        return Ok(Cow::Borrowed(&ctx.config));
    };

    let config = Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?
        .ok_or_else(|| anyhow!("Profile '{profile}' not found"))?;
    Ok(Cow::Owned(config))
}
