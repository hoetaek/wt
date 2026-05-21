use crate::context::Ctx;
use crate::services::identity_locator::{percent_encode, process_start_time};
use anyhow::{Context, Result};
use nix::errno::Errno;
use nix::sys::signal;
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub agent_id: String,
    pub pid: u32,
    pub pid_start_time: String,
    pub started_at: String,
    pub started_by: String,
    pub cleanup_on_session_end: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent_kind: Option<String>,
    pub stale_threshold_secs: u64,
    pub poll_interval_secs: u64,
    pub log_path: PathBuf,
}

pub fn registration_path(ctx: &Ctx, agent_id: &str) -> PathBuf {
    registrations_dir(ctx).join(format!("{}.toml", encoded_agent_id(agent_id)))
}

pub fn log_path(ctx: &Ctx, agent_id: &str) -> PathBuf {
    registrations_dir(ctx).join(format!("{}.log", encoded_agent_id(agent_id)))
}

pub fn write_registration(ctx: &Ctx, registration: &Registration) -> Result<()> {
    let path = registration_path(ctx, &registration.agent_id);
    let dir = registrations_dir(ctx);
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "Failed to create supervisor registration directory {}",
            dir.display()
        )
    })?;
    let rendered = toml::to_string_pretty(registration)
        .context("Failed to serialize supervisor registration")?;
    let temp_path = dir.join(format!(
        ".{}.{}.{}.tmp",
        encoded_agent_id(&registration.agent_id),
        std::process::id(),
        TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temp_path, rendered).with_context(|| {
        format!(
            "Failed to write temporary supervisor registration {}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "Failed to atomically write supervisor registration {}",
            path.display()
        )
    })?;
    Ok(())
}

pub fn read_registration(ctx: &Ctx, agent_id: &str) -> Result<Option<Registration>> {
    let path = registration_path(ctx, agent_id);
    read_registration_file(path)
}

pub fn remove_registration(ctx: &Ctx, agent_id: &str) -> Result<bool> {
    let path = registration_path(ctx, agent_id);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to remove supervisor registration {}",
                path.display()
            )
        }),
    }
}

pub fn list_registrations(ctx: &Ctx) -> Result<Vec<Registration>> {
    let dir = registrations_dir(ctx);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to read supervisor registration directory {}",
                    dir.display()
                )
            });
        }
    };

    let mut registrations = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "Failed to read supervisor registration directory entry in {}",
                dir.display()
            )
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        if let Some(registration) = read_registration_file(path)? {
            registrations.push(registration);
        }
    }
    registrations.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    Ok(registrations)
}

pub fn supervisor_is_alive(registration: &Registration) -> Result<bool> {
    let pid = Pid::from_raw(registration.pid as i32);
    match signal::kill(pid, None) {
        Ok(()) => match process_start_time(registration.pid as i32) {
            Ok(start_time) => Ok(start_time == registration.pid_start_time),
            Err(_) => Ok(false),
        },
        Err(Errno::ESRCH) => Ok(false),
        Err(Errno::EPERM) => Ok(true),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to check supervisor PID {} for {}",
                registration.pid, registration.agent_id
            )
        }),
    }
}

fn read_registration_file(path: PathBuf) -> Result<Option<Registration>> {
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to read supervisor registration {}", path.display())
            });
        }
    };
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse supervisor registration {}", path.display()))
        .map(Some)
}

fn registrations_dir(ctx: &Ctx) -> PathBuf {
    ctx.storage_root.personal_root().join("supervisors")
}

fn encoded_agent_id(agent_id: &str) -> String {
    percent_encode(agent_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::storage::StorageRoot;
    use std::process::Command;
    use std::time::Duration;
    use tempfile::TempDir;

    fn test_ctx(dir: &TempDir) -> Ctx {
        let repo_root = dir.path().join("repo");
        let git_common_dir = repo_root.join(".git");
        fs::create_dir_all(&git_common_dir).unwrap();
        Ctx::new_with_options(
            repo_root.clone(),
            repo_root,
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            crate::context::CtxOptions {
                storage_root: Some(StorageRoot::from_git_common_dir(git_common_dir)),
                ..Default::default()
            },
        )
    }

    fn registration(agent_id: &str) -> Registration {
        Registration {
            agent_id: agent_id.into(),
            pid: 123,
            pid_start_time: "12345.000000000".into(),
            started_at: "2026-05-22T00:00:00Z".into(),
            started_by: "user".into(),
            cleanup_on_session_end: false,
            target_surface_id: Some("surface:72".into()),
            target_agent_kind: Some("codex".into()),
            stale_threshold_secs: 900,
            poll_interval_secs: 60,
            log_path: PathBuf::from("/tmp/supervisor.log"),
        }
    }

    #[test]
    fn registration_round_trips_with_optional_fields() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let reg = registration("agents/codex");

        write_registration(&ctx, &reg).unwrap();

        assert_eq!(read_registration(&ctx, "agents/codex").unwrap(), Some(reg));
        assert!(registration_path(&ctx, "agents/codex").ends_with("agents%2Fcodex.toml"));
    }

    #[test]
    fn registration_paths_do_not_collide_for_slashes_and_underscores() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        assert_ne!(
            registration_path(&ctx, "foo/bar"),
            registration_path(&ctx, "foo__bar")
        );
    }

    #[test]
    fn registration_round_trips_without_optional_fields() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let mut reg = registration("agents/plain");
        reg.target_surface_id = None;
        reg.target_agent_kind = None;

        write_registration(&ctx, &reg).unwrap();

        let content = fs::read_to_string(registration_path(&ctx, "agents/plain")).unwrap();
        assert!(!content.contains("target_surface_id"));
        assert!(!content.contains("target_agent_kind"));
        assert_eq!(read_registration(&ctx, "agents/plain").unwrap(), Some(reg));
    }

    #[test]
    fn list_and_remove_registrations() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        write_registration(&ctx, &registration("agents/b")).unwrap();
        write_registration(&ctx, &registration("agents/a")).unwrap();

        let listed = list_registrations(&ctx).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|registration| registration.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["agents/a", "agents/b"]
        );
        assert!(remove_registration(&ctx, "agents/a").unwrap());
        assert!(!remove_registration(&ctx, "agents/a").unwrap());
    }

    #[test]
    fn supervisor_is_alive_reports_live_and_dead_pid() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .spawn()
            .unwrap();
        let mut reg = registration("agents/live");
        reg.pid = child.id();
        reg.pid_start_time = process_start_time(child.id() as i32).unwrap();
        assert!(supervisor_is_alive(&reg).unwrap());

        reg.pid_start_time = "0.000000000".into();
        assert!(!supervisor_is_alive(&reg).unwrap());
        reg.pid_start_time = process_start_time(child.id() as i32).unwrap();

        child.kill().unwrap();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!supervisor_is_alive(&reg).unwrap());
    }
}
