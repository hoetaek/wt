use crate::storage::StorageRoot;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::ErrorKind;
#[cfg(not(test))]
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginSnapshotKind {
    Task,
    Workflow,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginRef {
    pub provider: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_updated_at: Option<String>,
}

impl OriginRef {
    pub fn new(provider: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            id: id.into(),
            url: None,
            remote_updated_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSnapshot {
    pub title: String,
    pub body: String,
}

impl FieldSnapshot {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineSnapshot {
    pub recorded_at: String,
    pub fields: FieldSnapshot,
    pub local_hashes: FieldHashes,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldHashes {
    pub title: String,
    pub body: String,
}

impl FieldHashes {
    pub fn from_fields(fields: &FieldSnapshot) -> Self {
        Self {
            title: field_hash(&fields.title),
            body: field_hash(&fields.body),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSnapshot {
    pub fetched_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_updated_at: Option<String>,
    pub fields: FieldSnapshot,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments_count: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginSnapshot {
    pub kind: OriginSnapshotKind,
    pub owner: String,
    pub origin: OriginRef,
    pub baseline: BaselineSnapshot,
    pub remote: RemoteSnapshot,
    pub provider_context: ProviderContext,
}

impl OriginSnapshot {
    pub fn task(
        owner: impl Into<String>,
        origin: OriginRef,
        baseline_fields: FieldSnapshot,
        remote_fields: FieldSnapshot,
    ) -> Self {
        Self::new(
            OriginSnapshotKind::Task,
            owner,
            origin,
            baseline_fields,
            remote_fields,
        )
    }

    pub fn workflow(
        owner: impl Into<String>,
        origin: OriginRef,
        baseline_fields: FieldSnapshot,
        remote_fields: FieldSnapshot,
    ) -> Self {
        Self::new(
            OriginSnapshotKind::Workflow,
            owner,
            origin,
            baseline_fields,
            remote_fields,
        )
    }

    pub fn matches_origin(&self, provider: &str, id: &str) -> bool {
        self.origin.provider == provider && self.origin.id == id
    }

    pub fn matches_owner(&self, owner: &str) -> bool {
        self.owner == owner
    }

    fn new(
        kind: OriginSnapshotKind,
        owner: impl Into<String>,
        origin: OriginRef,
        baseline_fields: FieldSnapshot,
        remote_fields: FieldSnapshot,
    ) -> Self {
        let now = current_utc_timestamp();
        let remote_updated_at = origin.remote_updated_at.clone();
        Self {
            kind,
            owner: owner.into(),
            origin,
            baseline: BaselineSnapshot {
                recorded_at: now.clone(),
                local_hashes: FieldHashes::from_fields(&baseline_fields),
                fields: baseline_fields,
            },
            remote: RemoteSnapshot {
                fetched_at: now,
                remote_updated_at,
                fields: remote_fields,
            },
            provider_context: ProviderContext::default(),
        }
    }
}

pub fn write_snapshot(storage: &StorageRoot, snapshot: &OriginSnapshot) -> Result<()> {
    let path = snapshot_path(storage, snapshot);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create origin snapshot directory: {}",
                parent.display()
            )
        })?;
    }
    let content =
        toml::to_string_pretty(snapshot).context("Failed to serialize origin snapshot TOML")?;
    fs::write(&path, content)
        .with_context(|| format!("Failed to write origin snapshot: {}", path.display()))?;
    Ok(())
}

pub fn read_task_snapshot(storage: &StorageRoot, owner: &str) -> Result<Option<OriginSnapshot>> {
    let Some(snapshot) = read_snapshot_path(&storage.origin_task_snapshot_path(owner))? else {
        return Ok(None);
    };
    if snapshot.kind != OriginSnapshotKind::Task {
        return Ok(None);
    }
    if crate::task::safe_task_key(&snapshot.owner) != crate::task::safe_task_key(owner) {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

pub fn read_workflow_snapshot(
    storage: &StorageRoot,
    owner: &str,
) -> Result<Option<OriginSnapshot>> {
    let Some(snapshot) = read_snapshot_path(&storage.origin_workflow_snapshot_path(owner))? else {
        return Ok(None);
    };
    if snapshot.kind != OriginSnapshotKind::Workflow {
        return Ok(None);
    }
    if !snapshot.matches_owner(owner) {
        return Ok(None);
    }
    Ok(Some(snapshot))
}

fn snapshot_path(storage: &StorageRoot, snapshot: &OriginSnapshot) -> std::path::PathBuf {
    match snapshot.kind {
        OriginSnapshotKind::Task => storage.origin_task_snapshot_path(&snapshot.owner),
        OriginSnapshotKind::Workflow => storage.origin_workflow_snapshot_path(&snapshot.owner),
    }
}

fn read_snapshot_path(path: &std::path::Path) -> Result<Option<OriginSnapshot>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to read origin snapshot: {}", path.display()));
        }
    };
    let snapshot = toml::from_str(&content)
        .with_context(|| format!("Failed to parse origin snapshot: {}", path.display()))?;
    Ok(Some(snapshot))
}

fn field_hash(content: &str) -> String {
    format!("sha256:{}", sha256_hex(content.as_bytes()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
fn current_utc_timestamp() -> String {
    "2026-06-06T05:00:00Z".into()
}

#[cfg(not(test))]
fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(not(test))]
fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageRoot;

    #[test]
    fn task_snapshot_roundtrips_baseline_and_remote_fields() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());
        let snapshot = OriginSnapshot::task(
            "origin-sync-tui",
            OriginRef::new("linear", "WT-142"),
            FieldSnapshot::new("Origin sync TUI", "## Plan"),
            FieldSnapshot::new("Origin sync in TUI", "## Goal"),
        );

        write_snapshot(&storage, &snapshot).unwrap();
        let loaded = read_task_snapshot(&storage, "origin-sync-tui")
            .unwrap()
            .unwrap();

        assert_eq!(loaded.kind, OriginSnapshotKind::Task);
        assert_eq!(loaded.owner, "origin-sync-tui");
        assert_eq!(loaded.origin.provider, "linear");
        assert_eq!(loaded.baseline.fields.title, "Origin sync TUI");
        assert_eq!(loaded.remote.fields.title, "Origin sync in TUI");
    }

    #[test]
    fn mismatched_origin_is_rejected_for_current_local_origin() {
        let snapshot = OriginSnapshot::task(
            "origin-sync-tui",
            OriginRef::new("linear", "WT-142"),
            FieldSnapshot::new("Origin sync TUI", "## Plan"),
            FieldSnapshot::new("Origin sync in TUI", "## Goal"),
        );

        assert!(snapshot.matches_origin("linear", "WT-142"));
        assert!(!snapshot.matches_origin("linear", "WT-999"));
        assert!(!snapshot.matches_origin("github", "WT-142"));
    }

    #[test]
    fn reader_ignores_snapshot_with_wrong_owner() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());
        let snapshot = OriginSnapshot::task(
            "old-owner",
            OriginRef::new("linear", "WT-142"),
            FieldSnapshot::new("Old", "old"),
            FieldSnapshot::new("Remote", "remote"),
        );
        let path = storage.origin_task_snapshot_path("new-owner");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, toml::to_string_pretty(&snapshot).unwrap()).unwrap();

        let loaded = read_task_snapshot(&storage, "new-owner").unwrap();
        assert!(loaded.is_none());
    }
}
