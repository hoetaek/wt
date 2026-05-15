use crate::config::Config;
use crate::context::Ctx;
use anyhow::Result;

pub(crate) fn load_profile_config_for_branch(ctx: &Ctx, branch: &str) -> Result<Option<Config>> {
    let short = branch.rsplit('/').next().unwrap_or(branch);
    let mut profiles = Config::load_profiles(&ctx.repo_root, &ctx.base_config)?;
    profiles.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

    Ok(profiles
        .into_iter()
        .find(|(name, _)| {
            short
                .strip_suffix(name)
                .is_some_and(|prefix| prefix.ends_with('-'))
        })
        .map(|(_, config)| config))
}
