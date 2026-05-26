use anyhow::{Context, bail};
use std::path::{Path, PathBuf};

use super::merge::{
    finalize_config_common_prompt_scope, finalize_config_prompt_appends, merge_config,
};
use super::profile::apply_profile_conventions;
use super::{Config, validate_profile_name};
use crate::storage::StorageRoot;

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
    /// Load config with .wt.toml as the shared base and StorageRoot local config
    /// as the private override.
    pub fn load(repo_root: &Path) -> anyhow::Result<Self> {
        let storage_root = default_storage_root(repo_root);
        let (config, _) = Self::load_with_source_and_storage_root(repo_root, &storage_root)?;
        Ok(config)
    }

    pub fn load_with_source(repo_root: &Path) -> anyhow::Result<(Self, ConfigSource)> {
        let storage_root = default_storage_root(repo_root);
        Self::load_with_source_and_storage_root(repo_root, &storage_root)
    }

    pub fn load_with_source_and_storage_root(
        repo_root: &Path,
        storage_root: &StorageRoot,
    ) -> anyhow::Result<(Self, ConfigSource)> {
        let (_, effective, source) =
            Self::load_base_and_effective_with_source_and_storage_root(repo_root, storage_root)?;
        Ok((effective, source))
    }

    pub fn load_base_and_effective_with_source(
        repo_root: &Path,
    ) -> anyhow::Result<(Self, Self, ConfigSource)> {
        let storage_root = default_storage_root(repo_root);
        Self::load_base_and_effective_with_source_and_storage_root(repo_root, &storage_root)
    }

    pub fn load_base_and_effective_with_source_and_storage_root(
        repo_root: &Path,
        storage_root: &StorageRoot,
    ) -> anyhow::Result<(Self, Self, ConfigSource)> {
        reject_legacy_config(repo_root, storage_root)?;

        let personal_path = storage_root.config_toml();
        let root_path = repo_root.join(".wt.toml");
        let root_exists = root_path.exists();
        let personal_exists = personal_path.exists();

        let (base, source) = match (root_exists, personal_exists) {
            (false, false) => (Config::default(), ConfigSource::Default),
            (true, false) => {
                let mut config = Self::load_file(&root_path)?;
                finalize_config_prompt_appends(&mut config);
                (config, ConfigSource::File(root_path))
            }
            (false, true) => {
                let mut config = Self::load_file(&personal_path)?;
                finalize_config_prompt_appends(&mut config);
                (config, ConfigSource::File(personal_path))
            }
            (true, true) => {
                let mut root = Self::load_file(&root_path)?;
                finalize_config_prompt_appends(&mut root);
                let local = Self::load_file(&personal_path)?;
                (
                    merge_config(&root, local),
                    ConfigSource::Files(vec![root_path, personal_path]),
                )
            }
        };
        let effective = Self::resolve_effective_profile_with_storage_root(
            repo_root,
            storage_root,
            base.clone(),
        )?;
        Ok((base, effective, source))
    }

    pub fn load_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_file_for_repo(path: &Path, repo_root: &Path) -> anyhow::Result<(Self, Self)> {
        let storage_root = default_storage_root(repo_root);
        Self::load_file_for_repo_with_storage_root(path, repo_root, &storage_root)
    }

    pub fn load_file_for_repo_with_storage_root(
        path: &Path,
        repo_root: &Path,
        storage_root: &StorageRoot,
    ) -> anyhow::Result<(Self, Self)> {
        let mut base = Self::load_file(path)?;
        finalize_config_prompt_appends(&mut base);
        let effective = Self::resolve_effective_profile_with_storage_root(
            repo_root,
            storage_root,
            base.clone(),
        )?;
        Ok((base, effective))
    }

    pub fn resolve_effective_profile(repo_root: &Path, config: Self) -> anyhow::Result<Self> {
        let storage_root = default_storage_root(repo_root);
        Self::resolve_effective_profile_with_storage_root(repo_root, &storage_root, config)
    }

    pub fn resolve_effective_profile_with_storage_root(
        repo_root: &Path,
        storage_root: &StorageRoot,
        mut config: Self,
    ) -> anyhow::Result<Self> {
        let Some(profile) = config.profile.take() else {
            finalize_config_common_prompt_scope(&mut config);
            config.validate_effective_agent()?;
            return Ok(config);
        };

        if let Some(name) = profile.name.as_deref() {
            let mut config =
                Self::load_profile_from_storage(repo_root, storage_root, name, &config)?
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

    /// Discover profile configs: <git-common-dir>/wt/config/profiles/{name}/profile.toml
    pub fn load_profiles(repo_root: &Path, base: &Self) -> anyhow::Result<Vec<(String, Self)>> {
        let storage_root = default_storage_root(repo_root);
        Self::load_profiles_from_storage(repo_root, &storage_root, base)
    }

    pub fn load_profiles_from_storage(
        repo_root: &Path,
        storage_root: &StorageRoot,
        base: &Self,
    ) -> anyhow::Result<Vec<(String, Self)>> {
        let inventory = Self::load_profile_inventory_from_storage(repo_root, storage_root, base)?;
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
        let storage_root = default_storage_root(repo_root);
        Self::load_profile_inventory_from_storage(repo_root, &storage_root, base)
    }

    pub fn load_profile_inventory_from_storage(
        repo_root: &Path,
        storage_root: &StorageRoot,
        base: &Self,
    ) -> anyhow::Result<ProfileInventory> {
        let profiles_dir = storage_root.profiles_dir();
        let mut invalid_profiles = Vec::new();
        if let Some(legacy) = storage_root.detect_legacy_profiles(repo_root) {
            invalid_profiles.push(InvalidProfileRecord {
                name: "<legacy>".into(),
                path: legacy.path().to_path_buf(),
                error: legacy.error_message_for("profile storage"),
            });
        }

        if !profiles_dir.exists() {
            return Ok(ProfileInventory {
                profiles: Vec::new(),
                invalid_profiles,
            });
        }

        let mut profiles = Vec::new();

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
            match validate_profile_name(&profile_name)
                .and_then(|()| Self::load_profile_from_dir(&entry.path(), base))
            {
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
        let storage_root = default_storage_root(repo_root);
        Self::load_profile_from_storage(repo_root, &storage_root, name, base)
    }

    pub fn load_profile_from_storage(
        repo_root: &Path,
        storage_root: &StorageRoot,
        name: &str,
        base: &Self,
    ) -> anyhow::Result<Option<Self>> {
        validate_profile_name(name)?;
        let profile_dir = storage_root.profiles_dir().join(name);
        if !profile_dir.join("profile.toml").exists() {
            if let Some(legacy) = storage_root.detect_legacy_profiles(repo_root) {
                bail!(
                    "Profile '{name}' not found in {}; {}",
                    storage_root.display_path(&storage_root.profiles_dir()),
                    legacy.error_message_for("profile storage")
                );
            }
            return Ok(None);
        }

        Ok(Some(Self::load_profile_from_dir(&profile_dir, base)?))
    }

    fn load_profile_from_dir(profile_dir: &Path, base: &Self) -> anyhow::Result<Self> {
        let profile_config = Self::load_file(&profile_dir.join("profile.toml"))?;
        if profile_config.profile.is_some() {
            bail!(
                "[profile] is only valid in .wt.toml or <git-common-dir>/wt/config/local.toml, not in {}",
                profile_dir.join("profile.toml").display()
            );
        }
        let mut config = merge_config(base, profile_config);
        config.profile = None;
        apply_profile_conventions(profile_dir, &mut config)?;
        finalize_config_common_prompt_scope(&mut config);
        config.validate_effective_agent()?;
        Ok(config)
    }
}

fn default_storage_root(repo_root: &Path) -> StorageRoot {
    StorageRoot::from_git_common_dir(repo_root.join(".git"))
}

fn reject_legacy_config(repo_root: &Path, storage_root: &StorageRoot) -> anyhow::Result<()> {
    if let Some(legacy) = storage_root.detect_legacy_config(repo_root) {
        bail!("{}", legacy.error_message_for("config"));
    }
    Ok(())
}
