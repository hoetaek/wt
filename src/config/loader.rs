use anyhow::{Context, bail};
use std::path::{Path, PathBuf};

use super::merge::{
    finalize_config_common_prompt_scope, finalize_config_prompt_appends, merge_config,
};
use super::profile::apply_profile_conventions;
use super::{Config, validate_profile_name};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    File(PathBuf),
    Files(Vec<PathBuf>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRecord {
    pub name: String,
    pub path: PathBuf,
    pub config: Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidProfileRecord {
    pub name: String,
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileInventory {
    pub profiles: Vec<ProfileRecord>,
    pub invalid_profiles: Vec<InvalidProfileRecord>,
}

impl Config {
    /// Load config with .wt.toml as the shared base and .local/.wt.toml as
    /// the private override.
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let (config, _) = Self::load_with_source(repo_root)?;
        Ok(config)
    }

    pub fn load_with_source(repo_root: &Path) -> anyhow::Result<(Self, ConfigSource)> {
        let (_, effective, source) = Self::load_base_and_effective_with_source(repo_root)?;
        Ok((effective, source))
    }

    pub fn load_base_and_effective_with_source(
        repo_root: &Path,
    ) -> anyhow::Result<(Self, Self, ConfigSource)> {
        let local_path = repo_root.join(".local/.wt.toml");
        let root_path = repo_root.join(".wt.toml");
        let root_exists = root_path.exists();
        let local_exists = local_path.exists();

        let (base, source) = match (root_exists, local_exists) {
            (false, false) => (Config::default(), ConfigSource::Default),
            (true, false) => {
                let mut config = Self::load_file(&root_path)?;
                finalize_config_prompt_appends(&mut config);
                (config, ConfigSource::File(root_path))
            }
            (false, true) => {
                let mut config = Self::load_file(&local_path)?;
                finalize_config_prompt_appends(&mut config);
                (config, ConfigSource::File(local_path))
            }
            (true, true) => {
                let mut root = Self::load_file(&root_path)?;
                finalize_config_prompt_appends(&mut root);
                let local = Self::load_file(&local_path)?;
                (
                    merge_config(&root, local),
                    ConfigSource::Files(vec![root_path, local_path]),
                )
            }
        };
        let effective = Self::resolve_effective_profile(repo_root, base.clone())?;
        Ok((base, effective, source))
    }

    pub fn load_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_file_for_repo(path: &Path, repo_root: &Path) -> anyhow::Result<(Self, Self)> {
        let mut base = Self::load_file(path)?;
        finalize_config_prompt_appends(&mut base);
        let effective = Self::resolve_effective_profile(repo_root, base.clone())?;
        Ok((base, effective))
    }

    pub fn resolve_effective_profile(repo_root: &Path, mut config: Self) -> anyhow::Result<Self> {
        let Some(profile) = config.profile.take() else {
            finalize_config_common_prompt_scope(&mut config);
            config.validate_effective_agent()?;
            return Ok(config);
        };

        if let Some(name) = profile.name.as_deref() {
            let mut config = Self::load_profile(repo_root, name, &config)?
                .with_context(|| format!("Profile '{name}' not found"))?;
            finalize_config_common_prompt_scope(&mut config);
            config.validate_effective_agent()?;
            return Ok(config);
        }

        if profile.has_inline_settings() {
            let mut config = merge_config(&config, profile.into_config());
            finalize_config_common_prompt_scope(&mut config);
            config.validate_effective_agent()?;
            return Ok(config);
        }

        finalize_config_common_prompt_scope(&mut config);
        config.validate_effective_agent()?;
        Ok(config)
    }

    /// Discover profile configs: .local/profiles/{name}/profile.toml
    pub fn load_profiles(repo_root: &Path, base: &Self) -> anyhow::Result<Vec<(String, Self)>> {
        let inventory = Self::load_profile_inventory(repo_root, base)?;
        if let Some(invalid) = inventory.invalid_profiles.first() {
            bail!(
                "Failed to load profile: {}: {}",
                invalid.path.display(),
                invalid.error
            );
        }
        Ok(inventory
            .profiles
            .into_iter()
            .map(|profile| (profile.name, profile.config))
            .collect())
    }

    pub fn load_profile_inventory(
        repo_root: &Path,
        base: &Self,
    ) -> anyhow::Result<ProfileInventory> {
        let profiles_dir = repo_root.join(".local/profiles");
        if !profiles_dir.exists() {
            return Ok(ProfileInventory {
                profiles: Vec::new(),
                invalid_profiles: Vec::new(),
            });
        }

        let mut profiles = Vec::new();
        let mut invalid_profiles = Vec::new();

        for entry in std::fs::read_dir(&profiles_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let profile_name = entry.file_name().to_string_lossy().into_owned();
            let profile_toml = entry.path().join("profile.toml");
            if !profile_toml.exists() {
                continue;
            }
            match validate_profile_name(&profile_name).and_then(|()| {
                Self::load_profile_from_dir(repo_root, &profile_name, &entry.path(), base)
            }) {
                Ok(config) => profiles.push(ProfileRecord {
                    name: profile_name,
                    path: profile_toml,
                    config,
                }),
                Err(err) => invalid_profiles.push(InvalidProfileRecord {
                    name: profile_name,
                    path: profile_toml,
                    error: format!("{err:#}"),
                }),
            }
        }

        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        invalid_profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(ProfileInventory {
            profiles,
            invalid_profiles,
        })
    }

    pub fn load_profile(repo_root: &Path, name: &str, base: &Self) -> anyhow::Result<Option<Self>> {
        validate_profile_name(name)?;
        let profile_dir = repo_root.join(".local/profiles").join(name);
        if !profile_dir.join("profile.toml").exists() {
            return Ok(None);
        }

        Ok(Some(Self::load_profile_from_dir(
            repo_root,
            name,
            &profile_dir,
            base,
        )?))
    }

    fn load_profile_from_dir(
        repo_root: &Path,
        name: &str,
        profile_dir: &Path,
        base: &Self,
    ) -> anyhow::Result<Self> {
        let profile_config = Self::load_file(&profile_dir.join("profile.toml"))?;
        if profile_config.profile.is_some() {
            bail!(
                "[profile] is only valid in .wt.toml files, not in {}",
                profile_dir.join("profile.toml").display()
            );
        }
        let mut config = merge_config(base, profile_config);
        config.profile = None;
        apply_profile_conventions(repo_root, name, profile_dir, &mut config)?;
        finalize_config_common_prompt_scope(&mut config);
        config.validate_effective_agent()?;
        Ok(config)
    }
}
