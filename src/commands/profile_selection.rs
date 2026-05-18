use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use anyhow::{Result, bail};
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(crate) struct ProfileSelection<'a> {
    pub(crate) profile: Option<&'a str>,
    pub(crate) selected_profiles: &'a [String],
}

impl<'a> ProfileSelection<'a> {
    pub(crate) fn new(profile: Option<&'a str>, selected_profiles: &'a [String]) -> Self {
        Self {
            profile,
            selected_profiles,
        }
    }

    pub(crate) fn uses_profiles(self) -> bool {
        self.profile.is_some() || !self.selected_profiles.is_empty()
    }
}

pub(crate) fn load_profile_selection(
    ctx: &Ctx,
    selection: ProfileSelection<'_>,
) -> Result<Vec<(String, Config)>> {
    if selection.profile.is_some() && !selection.selected_profiles.is_empty() {
        bail!("--profiles cannot be used with --profile");
    }

    if let Some(profile) = selection.profile {
        return load_one_profile(ctx, profile);
    }

    if !selection.selected_profiles.is_empty() {
        return load_selected_profiles(ctx, selection.selected_profiles);
    }

    load_all_profiles(ctx)
}

fn load_one_profile(ctx: &Ctx, profile: &str) -> Result<Vec<(String, Config)>> {
    validate_profile_name(profile)?;
    let config = Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?
        .ok_or_else(|| anyhow::anyhow!("Profile '{profile}' not found"))?;
    Ok(vec![(profile.to_string(), config)])
}

pub(crate) fn load_selected_profiles(
    ctx: &Ctx,
    selected_profiles: &[String],
) -> Result<Vec<(String, Config)>> {
    let mut seen = HashSet::new();
    for profile in selected_profiles {
        validate_profile_name(profile)?;
        if !seen.insert(profile.as_str()) {
            bail!("Duplicate profile: {profile}");
        }
    }

    selected_profiles
        .iter()
        .map(|profile| {
            let config = Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?
                .ok_or_else(|| anyhow::anyhow!("Profile '{profile}' not found"))?;
            Ok((profile.clone(), config))
        })
        .collect()
}

fn load_all_profiles(ctx: &Ctx) -> Result<Vec<(String, Config)>> {
    let profiles = Config::load_profiles(&ctx.repo_root, &ctx.base_config)?;
    if profiles.is_empty() {
        bail!("No profile configs found in .local/profiles/*/profile.toml");
    }
    Ok(profiles)
}
