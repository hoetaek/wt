#[cfg(target_os = "linux")]
mod start_time_linux;
#[cfg(target_os = "macos")]
mod start_time_macos;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("wt identity locator supports shell-sid liveness only on Linux and macOS");

use crate::context::Ctx;
use crate::messages::AgentId;
use anyhow::{Context, Result, bail};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::{Pid, getpid, getsid};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CMUX_SURFACE_ID: &str = "CMUX_SURFACE_ID";
const CLAUDE_CODE_SESSION_ID: &str = "CLAUDE_CODE_SESSION_ID";
const CODEX_THREAD_ID: &str = "CODEX_THREAD_ID";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorKind {
    Surface,
    ClaudeSession,
    CodexThread,
    ShellSid,
}

impl AnchorKind {
    fn slug(&self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::ClaudeSession => "claude-session",
            Self::CodexThread => "codex-thread",
            Self::ShellSid => "shell-sid",
        }
    }

    fn from_slug(slug: &str) -> Result<Self> {
        match slug {
            "surface" => Ok(Self::Surface),
            "claude-session" => Ok(Self::ClaudeSession),
            "codex-thread" => Ok(Self::CodexThread),
            "shell-sid" => Ok(Self::ShellSid),
            _ => bail!("Unknown anchor kind `{slug}`"),
        }
    }

    fn env_var(&self) -> Option<&'static str> {
        match self {
            Self::Surface => Some(CMUX_SURFACE_ID),
            Self::ClaudeSession => Some(CLAUDE_CODE_SESSION_ID),
            Self::CodexThread => Some(CODEX_THREAD_ID),
            Self::ShellSid => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorKey {
    pub kind: AnchorKind,
    pub value: String,
}

impl AnchorKey {
    pub fn encode(&self) -> String {
        percent_encode(&self.display())
    }

    pub fn decode(encoded: &str) -> Result<Self> {
        Self::parse_display(&percent_decode(encoded)?)
    }

    pub fn display(&self) -> String {
        format!("{}:{}", self.kind.slug(), self.value)
    }

    pub fn parse_display(display: &str) -> Result<Self> {
        let (kind, value) = display
            .split_once(':')
            .with_context(|| format!("Invalid anchor key `{display}`"))?;
        if value.trim().is_empty() {
            bail!("Invalid anchor key `{display}`: value cannot be empty");
        }
        Ok(Self {
            kind: AnchorKind::from_slug(kind)?,
            value: value.to_string(),
        })
    }

    fn shell_sid_parts(&self) -> Result<Option<(i32, String)>> {
        if self.kind != AnchorKind::ShellSid {
            return Ok(None);
        }
        let (sid, start_time) = self
            .value
            .split_once(':')
            .with_context(|| format!("Invalid shell-sid anchor `{}`", self.display()))?;
        let sid = sid
            .parse::<i32>()
            .with_context(|| format!("Invalid shell-sid pid in `{}`", self.display()))?;
        Ok(Some((sid, start_time.to_string())))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityAnchor {
    pub id: String,
    pub anchor_kind: AnchorKind,
    pub anchor_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness_pid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness_start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_agent_kind: Option<String>,
    pub cwd: PathBuf,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityAnchorEntry {
    pub path: PathBuf,
    pub anchor: IdentityAnchor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityAnchorScanWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityAnchorLiveness {
    Live,
    NotLive,
}

trait EnvProvider {
    fn var(&self, key: &str) -> Option<String>;
}

trait ProcessProvider {
    fn current_pid(&self) -> i32;
    fn current_sid(&self) -> Result<i32>;
    fn process_start_time(&self, pid: i32) -> Result<String>;
    fn pid_is_live(&self, pid: i32) -> Result<bool>;
}

struct SystemEnv;

impl EnvProvider for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        env::var(key).ok().filter(|value| !value.trim().is_empty())
    }
}

struct SystemProcess;

impl ProcessProvider for SystemProcess {
    fn current_pid(&self) -> i32 {
        getpid().as_raw()
    }

    fn current_sid(&self) -> Result<i32> {
        Ok(getsid(Some(Pid::from_raw(self.current_pid())))
            .context("Failed to read current POSIX session id")?
            .as_raw())
    }

    fn process_start_time(&self, pid: i32) -> Result<String> {
        platform_process_start_time(pid)
    }

    fn pid_is_live(&self, pid: i32) -> Result<bool> {
        match kill(Pid::from_raw(pid), None) {
            Ok(()) => Ok(true),
            Err(Errno::ESRCH) => Ok(false),
            Err(Errno::EPERM) => Ok(true),
            Err(err) => Err(err).with_context(|| format!("Failed to check liveness for pid {pid}")),
        }
    }
}

pub fn current_anchor_key() -> Result<AnchorKey> {
    current_anchor_key_with(&SystemEnv, &SystemProcess)
}

pub fn current_agent_kind() -> Option<String> {
    current_agent_kind_with(&SystemEnv)
}

pub fn identity_anchor_path(ctx: &Ctx, key: &AnchorKey) -> PathBuf {
    anchor_search_path(ctx, key)
}

pub fn identity_anchor_path_for_id(ctx: &Ctx, id: &str, key: &AnchorKey) -> Result<PathBuf> {
    let agent = AgentId::parse(id)?;
    Ok(identity_anchor_path_for_agent(ctx, &agent, key))
}

pub fn write_identity_anchor(
    ctx: &Ctx,
    key: &AnchorKey,
    id: &str,
    agent_kind: Option<&str>,
) -> Result<IdentityAnchor> {
    let agent = AgentId::parse(id)?;
    let id = agent.as_str().to_string();
    let path = identity_anchor_path_for_agent(ctx, &agent, key);
    let existing = read_identity_anchor(ctx, key)?;
    remove_identity_anchor_files_for_key(ctx, key, Some(&path))?;
    let now = current_timestamp();
    let (liveness_pid, liveness_start_time) = match key.shell_sid_parts()? {
        Some((pid, start_time)) => (Some(pid), Some(start_time)),
        None => (None, None),
    };
    let anchor = IdentityAnchor {
        id,
        anchor_kind: key.kind.clone(),
        anchor_value: key.value.clone(),
        liveness_pid,
        liveness_start_time,
        anchor_agent_kind: agent_kind.map(str::to_string),
        cwd: ctx.invocation_root.clone(),
        created_at: existing
            .map(|anchor| anchor.created_at)
            .unwrap_or(now.clone()),
        updated_at: now,
    };
    write_identity_anchor_atomically(&path, &anchor)?;
    Ok(anchor)
}

pub fn read_identity_anchor(ctx: &Ctx, key: &AnchorKey) -> Result<Option<IdentityAnchor>> {
    let mut matching = Vec::new();
    let mut first_error: Option<anyhow::Error> = None;
    for path in identity_anchor_paths_for_key(ctx, key)? {
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                first_error.get_or_insert_with(|| {
                    anyhow::Error::new(err).context(format!(
                        "Failed to read identity anchor: {}",
                        path.display()
                    ))
                });
                continue;
            }
        };
        let anchor = match toml::from_str::<IdentityAnchor>(&content) {
            Ok(anchor) => anchor,
            Err(err) => {
                first_error.get_or_insert_with(|| {
                    anyhow::Error::new(err).context(format!(
                        "Failed to parse identity anchor: {}",
                        path.display()
                    ))
                });
                continue;
            }
        };
        match identity_anchor_path_matches_owner(ctx, &path, &anchor) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                first_error.get_or_insert_with(|| {
                    err.context(format!("Invalid identity anchor owner: {}", path.display()))
                });
                continue;
            }
        }
        if identity_anchor_matches_key(&anchor, key) {
            matching.push((path, anchor));
        }
    }
    matching.sort_by(|left, right| {
        right
            .1
            .updated_at
            .cmp(&left.1.updated_at)
            .then_with(|| left.0.cmp(&right.0))
    });
    if let Some((_, anchor)) = matching.into_iter().next() {
        return Ok(Some(anchor));
    }
    if let Some(err) = first_error {
        return Err(err);
    }
    Ok(None)
}

pub fn remove_identity_anchor(ctx: &Ctx, key: &AnchorKey) -> Result<bool> {
    remove_identity_anchor_files_for_key(ctx, key, None)
}

pub fn list_identity_anchors(ctx: &Ctx) -> Result<Vec<IdentityAnchor>> {
    let (entries, warnings) = list_identity_anchors_with_warnings(ctx)?;
    if let Some(warning) = warnings.into_iter().next() {
        bail!(
            "Failed to scan identity anchor {}: {}",
            warning.path.display(),
            warning.message
        );
    }
    let mut anchors = entries
        .into_iter()
        .map(|entry| entry.anchor)
        .collect::<Vec<_>>();
    anchors.sort_by(|left, right| {
        let left_key = identity_anchor_key(left).display();
        let right_key = identity_anchor_key(right).display();
        left_key.cmp(&right_key)
    });
    Ok(anchors)
}

pub fn list_identity_anchors_with_warnings(
    ctx: &Ctx,
) -> Result<(Vec<IdentityAnchorEntry>, Vec<IdentityAnchorScanWarning>)> {
    let mut anchors = Vec::new();
    let mut warnings = Vec::new();
    for path in all_identity_anchor_paths(ctx)? {
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                warnings.push(IdentityAnchorScanWarning {
                    path,
                    message: format!("Failed to read identity anchor: {err}"),
                });
                continue;
            }
        };
        let anchor = match toml::from_str::<IdentityAnchor>(&content) {
            Ok(anchor) => anchor,
            Err(err) => {
                warnings.push(IdentityAnchorScanWarning {
                    path,
                    message: format!("Failed to parse identity anchor: {err}"),
                });
                continue;
            }
        };
        match identity_anchor_path_matches_owner(ctx, &path, &anchor) {
            Ok(true) => {}
            Ok(false) => {
                warnings.push(IdentityAnchorScanWarning {
                    path,
                    message: "Identity anchor id does not match owning runtime agent directory"
                        .into(),
                });
                continue;
            }
            Err(err) => {
                warnings.push(IdentityAnchorScanWarning {
                    path,
                    message: format!("Invalid identity anchor owner: {err:#}"),
                });
                continue;
            }
        }
        anchors.push(IdentityAnchorEntry { path, anchor });
    }
    anchors.sort_by(|left, right| {
        let left_key = identity_anchor_key(&left.anchor).display();
        let right_key = identity_anchor_key(&right.anchor).display();
        left_key
            .cmp(&right_key)
            .then_with(|| left.path.cmp(&right.path))
    });
    warnings.sort_by(|left, right| left.path.cmp(&right.path));
    Ok((anchors, warnings))
}

pub fn identity_anchor_is_live(anchor: &IdentityAnchor) -> Result<IdentityAnchorLiveness> {
    identity_anchor_is_live_with(anchor, &SystemEnv, &SystemProcess)
}

pub fn resolve_identity(ctx: &Ctx) -> Result<Option<IdentityAnchor>> {
    resolve_identity_with(ctx, &SystemEnv, &SystemProcess)
}

fn current_anchor_key_with(
    env: &dyn EnvProvider,
    process: &dyn ProcessProvider,
) -> Result<AnchorKey> {
    for (kind, var) in [
        (AnchorKind::Surface, CMUX_SURFACE_ID),
        (AnchorKind::ClaudeSession, CLAUDE_CODE_SESSION_ID),
        (AnchorKind::CodexThread, CODEX_THREAD_ID),
    ] {
        if let Some(value) = env.var(var) {
            return Ok(AnchorKey { kind, value });
        }
    }

    let sid = process.current_sid()?;
    if sid == 1 {
        bail!("Refusing to use init session id 1 as a shell-sid anchor");
    }
    let start_time = process.process_start_time(sid)?;
    Ok(AnchorKey {
        kind: AnchorKind::ShellSid,
        value: format!("{sid}:{start_time}"),
    })
}

fn current_agent_kind_with(env: &dyn EnvProvider) -> Option<String> {
    if env.var(CLAUDE_CODE_SESSION_ID).is_some() {
        Some("claude".into())
    } else if env.var(CODEX_THREAD_ID).is_some() {
        Some("codex".into())
    } else {
        None
    }
}

fn identity_anchor_is_live_with(
    anchor: &IdentityAnchor,
    env: &dyn EnvProvider,
    process: &dyn ProcessProvider,
) -> Result<IdentityAnchorLiveness> {
    if let Some(var) = anchor.anchor_kind.env_var() {
        return Ok(match env.var(var) {
            Some(value) if value == anchor.anchor_value => IdentityAnchorLiveness::Live,
            _ => IdentityAnchorLiveness::NotLive,
        });
    }

    let Some(pid) = anchor.liveness_pid else {
        return Ok(IdentityAnchorLiveness::NotLive);
    };
    let Some(expected_start_time) = anchor.liveness_start_time.as_deref() else {
        return Ok(IdentityAnchorLiveness::NotLive);
    };
    if !process.pid_is_live(pid)? {
        return Ok(IdentityAnchorLiveness::NotLive);
    }
    let Ok(actual_start_time) = process.process_start_time(pid) else {
        return Ok(IdentityAnchorLiveness::NotLive);
    };
    if actual_start_time == expected_start_time {
        Ok(IdentityAnchorLiveness::Live)
    } else {
        Ok(IdentityAnchorLiveness::NotLive)
    }
}

fn resolve_identity_with(
    ctx: &Ctx,
    env: &dyn EnvProvider,
    process: &dyn ProcessProvider,
) -> Result<Option<IdentityAnchor>> {
    let key = current_anchor_key_with(env, process)?;
    let Some(anchor) = read_identity_anchor(ctx, &key)? else {
        return Ok(None);
    };
    match identity_anchor_is_live_with(&anchor, env, process)? {
        IdentityAnchorLiveness::Live => Ok(Some(anchor)),
        IdentityAnchorLiveness::NotLive => Ok(None),
    }
}

fn anchor_search_path(ctx: &Ctx, key: &AnchorKey) -> PathBuf {
    ctx.storage_root
        .runtime_agents_dir()
        .join("*")
        .join("anchors")
        .join(identity_anchor_file_name(key))
}

fn identity_anchor_path_for_agent(ctx: &Ctx, agent: &AgentId, key: &AnchorKey) -> PathBuf {
    ctx.storage_root
        .runtime_agent_anchors_dir(agent)
        .join(identity_anchor_file_name(key))
}

fn identity_anchor_file_name(key: &AnchorKey) -> String {
    format!("{}.toml", key.encode())
}

fn identity_anchor_paths_for_key(ctx: &Ctx, key: &AnchorKey) -> Result<Vec<PathBuf>> {
    let file_name = identity_anchor_file_name(key);
    let mut paths = all_identity_anchor_paths(ctx)?
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some(file_name.as_str()))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn all_identity_anchor_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let agents_dir = ctx.storage_root.runtime_agents_dir();
    let entries = match fs::read_dir(&agents_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("Failed to read {}", agents_dir.display()));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("Failed to read entry in {}", agents_dir.display()))?;
        let agent_dir = entry.path();
        if !entry
            .file_type()
            .with_context(|| format!("Failed to inspect {}", agent_dir.display()))?
            .is_dir()
        {
            continue;
        }
        let anchors_dir = agent_dir.join("anchors");
        let anchors = match fs::read_dir(&anchors_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("Failed to read {}", anchors_dir.display()));
            }
        };
        for anchor in anchors {
            let anchor = anchor
                .with_context(|| format!("Failed to read entry in {}", anchors_dir.display()))?;
            let path = anchor.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn identity_anchor_path_matches_owner(
    ctx: &Ctx,
    path: &Path,
    anchor: &IdentityAnchor,
) -> Result<bool> {
    let agent = AgentId::parse(&anchor.id)?;
    let expected_dir = ctx.storage_root.runtime_agent_anchors_dir(&agent);
    Ok(path.parent() == Some(expected_dir.as_path()))
}

fn remove_identity_anchor_files_for_key(
    ctx: &Ctx,
    key: &AnchorKey,
    except: Option<&Path>,
) -> Result<bool> {
    let mut removed = false;
    for path in identity_anchor_paths_for_key(ctx, key)? {
        if except.is_some_and(|except| except == path.as_path()) {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("Failed to remove identity anchor: {}", path.display())
                });
            }
        }
    }
    Ok(removed)
}

fn identity_anchor_key(anchor: &IdentityAnchor) -> AnchorKey {
    AnchorKey {
        kind: anchor.anchor_kind.clone(),
        value: anchor.anchor_value.clone(),
    }
}

fn identity_anchor_matches_key(anchor: &IdentityAnchor, key: &AnchorKey) -> bool {
    anchor.anchor_kind == key.kind && anchor.anchor_value == key.value
}

fn write_identity_anchor_atomically(path: &Path, anchor: &IdentityAnchor) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("Identity anchor path has no parent: {}", path.display()))?;
    fs::create_dir_all(dir).with_context(|| {
        format!(
            "Failed to create identity anchor directory: {}",
            dir.display()
        )
    })?;
    let content =
        toml::to_string_pretty(anchor).context("Failed to serialize identity anchor TOML")?;
    let (temp_path, mut file) = create_temp_file_with_retry(dir)?;
    let result = (|| -> Result<()> {
        file.write_all(content.as_bytes()).with_context(|| {
            format!(
                "Failed to write temporary identity anchor: {}",
                temp_path.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "Failed to sync temporary identity anchor: {}",
                temp_path.display()
            )
        })?;
        drop(file);
        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to replace identity anchor: {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn create_temp_file_with_retry(dir: &Path) -> Result<(PathBuf, fs::File)> {
    let pid = std::process::id();
    for attempt in 0..100 {
        let path = dir.join(format!(".wt-identity-anchor-{pid}-{attempt}.tmp"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to create temporary identity anchor: {}",
                        path.display()
                    )
                });
            }
        }
    }
    bail!(
        "Failed to allocate temporary identity anchor path in {}",
        dir.display()
    )
}

pub fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(hex) = bytes.get(index + 1..index + 3) else {
            bail!("Invalid percent-encoded anchor key `{value}`");
        };
        let hex = std::str::from_utf8(hex).context("Invalid percent-encoded anchor key")?;
        let byte = u8::from_str_radix(hex, 16)
            .with_context(|| format!("Invalid percent-encoded anchor key `{value}`"))?;
        decoded.push(byte);
        index += 3;
    }
    String::from_utf8(decoded).context("Invalid UTF-8 in percent-encoded anchor key")
}

fn current_timestamp() -> String {
    let (seconds, nanos) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() as i64, duration.subsec_nanos()))
        .unwrap_or((0, 0));
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = z.div_euclid(146_097);
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

#[cfg(target_os = "linux")]
fn platform_process_start_time(pid: i32) -> Result<String> {
    start_time_linux::process_start_time(pid)
}

#[cfg(target_os = "macos")]
fn platform_process_start_time(pid: i32) -> Result<String> {
    start_time_macos::process_start_time(pid)
}

pub fn process_start_time(pid: i32) -> Result<String> {
    platform_process_start_time(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ConfigSource};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions, OutputMode};
    use crate::storage::StorageRoot;
    use std::collections::{HashMap, HashSet};
    use tempfile::TempDir;

    #[derive(Default)]
    struct TestEnv {
        values: HashMap<String, String>,
    }

    impl TestEnv {
        fn with(mut self, key: &str, value: &str) -> Self {
            self.values.insert(key.into(), value.into());
            self
        }
    }

    impl EnvProvider for TestEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.values
                .get(key)
                .cloned()
                .filter(|value| !value.trim().is_empty())
        }
    }

    struct TestProcess {
        current_pid: i32,
        current_sid: i32,
        start_times: HashMap<i32, String>,
        live_pids: HashSet<i32>,
    }

    impl TestProcess {
        fn new() -> Self {
            Self {
                current_pid: 10,
                current_sid: 20,
                start_times: HashMap::from([(20, "100.000000000".into())]),
                live_pids: HashSet::from([20]),
            }
        }

        fn with_live_pid(mut self, pid: i32, start_time: &str) -> Self {
            self.live_pids.insert(pid);
            self.start_times.insert(pid, start_time.into());
            self
        }
    }

    impl ProcessProvider for TestProcess {
        fn current_pid(&self) -> i32 {
            self.current_pid
        }

        fn current_sid(&self) -> Result<i32> {
            Ok(self.current_sid)
        }

        fn process_start_time(&self, pid: i32) -> Result<String> {
            self.start_times
                .get(&pid)
                .cloned()
                .with_context(|| format!("missing test start time for pid {pid}"))
        }

        fn pid_is_live(&self, pid: i32) -> Result<bool> {
            Ok(self.live_pids.contains(&pid))
        }
    }

    #[test]
    fn anchor_key_encode_decode_round_trips_each_kind() {
        for key in [
            AnchorKey {
                kind: AnchorKind::Surface,
                value: "surface-1".into(),
            },
            AnchorKey {
                kind: AnchorKind::ClaudeSession,
                value: "claude-1".into(),
            },
            AnchorKey {
                kind: AnchorKind::CodexThread,
                value: "codex-1".into(),
            },
            AnchorKey {
                kind: AnchorKind::ShellSid,
                value: "123:456.000000000".into(),
            },
            AnchorKey {
                kind: AnchorKind::Surface,
                value: "value__with/slashes\\percent%and..dots".into(),
            },
        ] {
            assert_eq!(AnchorKey::decode(&key.encode()).unwrap(), key);
            assert!(!key.encode().contains('/'));
            assert!(!key.encode().contains('\\'));
            assert!(!key.encode().contains('.'));
        }
    }

    #[test]
    fn identity_anchor_toml_round_trips_with_optional_fields() {
        let anchor = identity_anchor_fixture(
            AnchorKind::ShellSid,
            "20:100.000000000",
            Some(20),
            Some("100.000000000"),
        );
        let encoded = toml::to_string_pretty(&anchor).unwrap();
        let decoded: IdentityAnchor = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, anchor);
    }

    #[test]
    fn identity_anchor_toml_round_trips_without_optional_fields() {
        let anchor = identity_anchor_fixture(AnchorKind::Surface, "surface-1", None, None);
        let encoded = toml::to_string_pretty(&anchor).unwrap();
        assert!(!encoded.contains("liveness_pid"));
        let decoded: IdentityAnchor = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, anchor);
    }

    #[test]
    fn shell_sid_liveness_detects_live_dead_and_reused_pid() {
        let process = TestProcess::new().with_live_pid(42, "200.000000000");
        let live = identity_anchor_fixture(
            AnchorKind::ShellSid,
            "42:200.000000000",
            Some(42),
            Some("200.000000000"),
        );
        let dead = identity_anchor_fixture(
            AnchorKind::ShellSid,
            "43:300.000000000",
            Some(43),
            Some("300.000000000"),
        );
        let reused = identity_anchor_fixture(
            AnchorKind::ShellSid,
            "42:199.000000000",
            Some(42),
            Some("199.000000000"),
        );

        assert_eq!(
            identity_anchor_is_live_with(&live, &TestEnv::default(), &process).unwrap(),
            IdentityAnchorLiveness::Live
        );
        assert_eq!(
            identity_anchor_is_live_with(&dead, &TestEnv::default(), &process).unwrap(),
            IdentityAnchorLiveness::NotLive
        );
        assert_eq!(
            identity_anchor_is_live_with(&reused, &TestEnv::default(), &process).unwrap(),
            IdentityAnchorLiveness::NotLive
        );
    }

    #[test]
    fn shell_sid_liveness_treats_start_time_read_failure_as_not_live() {
        let process = TestProcess {
            live_pids: HashSet::from([44]),
            ..TestProcess::new()
        };
        let anchor = identity_anchor_fixture(
            AnchorKind::ShellSid,
            "44:400.000000000",
            Some(44),
            Some("400.000000000"),
        );

        assert_eq!(
            identity_anchor_is_live_with(&anchor, &TestEnv::default(), &process).unwrap(),
            IdentityAnchorLiveness::NotLive
        );
    }

    #[test]
    fn env_keyed_liveness_uses_injected_env() {
        let anchor = identity_anchor_fixture(AnchorKind::Surface, "surface-1", None, None);
        let live_env = TestEnv::default().with(CMUX_SURFACE_ID, "surface-1");
        let wrong_env = TestEnv::default().with(CMUX_SURFACE_ID, "surface-2");

        assert_eq!(
            identity_anchor_is_live_with(&anchor, &live_env, &TestProcess::new()).unwrap(),
            IdentityAnchorLiveness::Live
        );
        assert_eq!(
            identity_anchor_is_live_with(&anchor, &wrong_env, &TestProcess::new()).unwrap(),
            IdentityAnchorLiveness::NotLive
        );
        assert_eq!(
            identity_anchor_is_live_with(&anchor, &TestEnv::default(), &TestProcess::new())
                .unwrap(),
            IdentityAnchorLiveness::NotLive
        );
    }

    #[test]
    fn resolve_identity_reads_matching_live_anchor() {
        let fixture = CtxFixture::new();
        let env = TestEnv::default().with(CMUX_SURFACE_ID, "surface-1");
        let key = AnchorKey {
            kind: AnchorKind::Surface,
            value: "surface-1".into(),
        };
        write_identity_anchor(&fixture.ctx, &key, "coord-a", Some("codex")).unwrap();

        let resolved = resolve_identity_with(&fixture.ctx, &env, &TestProcess::new())
            .unwrap()
            .unwrap();
        assert_eq!(resolved.id, "agents/coord-a");
        assert_eq!(resolved.anchor_agent_kind.as_deref(), Some("codex"));
    }

    #[test]
    fn resolve_identity_ignores_anchor_with_mismatched_anchor_payload() {
        let fixture = CtxFixture::new();
        let env = TestEnv::default().with(CMUX_SURFACE_ID, "surface-1");
        let key = AnchorKey {
            kind: AnchorKind::Surface,
            value: "surface-1".into(),
        };
        let mut anchor = identity_anchor_fixture(AnchorKind::Surface, "surface-2", None, None);
        anchor.id = "agents/coord-b".into();
        let path = identity_anchor_path_for_id(&fixture.ctx, "agents/coord-b", &key).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, toml::to_string_pretty(&anchor).unwrap()).unwrap();

        assert!(
            resolve_identity_with(&fixture.ctx, &env, &TestProcess::new())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn current_anchor_key_prefers_surface_then_claude_then_codex_then_shell_sid() {
        let process = TestProcess::new();
        let env = TestEnv::default()
            .with(CMUX_SURFACE_ID, "surface-1")
            .with(CLAUDE_CODE_SESSION_ID, "claude-1")
            .with(CODEX_THREAD_ID, "codex-1");
        assert_eq!(
            current_anchor_key_with(&env, &process).unwrap(),
            AnchorKey {
                kind: AnchorKind::Surface,
                value: "surface-1".into()
            }
        );

        let env = TestEnv::default()
            .with(CLAUDE_CODE_SESSION_ID, "claude-1")
            .with(CODEX_THREAD_ID, "codex-1");
        assert_eq!(
            current_anchor_key_with(&env, &process).unwrap().kind,
            AnchorKind::ClaudeSession
        );

        let env = TestEnv::default().with(CODEX_THREAD_ID, "codex-1");
        assert_eq!(
            current_anchor_key_with(&env, &process).unwrap().kind,
            AnchorKind::CodexThread
        );

        assert_eq!(
            current_anchor_key_with(&TestEnv::default(), &process).unwrap(),
            AnchorKey {
                kind: AnchorKind::ShellSid,
                value: "20:100.000000000".into()
            }
        );
    }

    #[test]
    fn current_anchor_key_refuses_init_shell_sid() {
        let process = TestProcess {
            current_sid: 1,
            ..TestProcess::new()
        };
        let err = current_anchor_key_with(&TestEnv::default(), &process).unwrap_err();
        assert!(format!("{err:#}").contains("init session id 1"));
    }

    #[test]
    fn identity_anchor_file_lifecycle_round_trips_lists_and_removes() {
        let fixture = CtxFixture::new();
        let key = AnchorKey {
            kind: AnchorKind::Surface,
            value: "surface-1".into(),
        };
        let anchor =
            write_identity_anchor(&fixture.ctx, &key, "agents/coord-a", Some("claude")).unwrap();
        assert_eq!(anchor.id, "agents/coord-a");
        assert!(
            identity_anchor_path_for_id(&fixture.ctx, "agents/coord-a", &key)
                .unwrap()
                .ends_with("runtime/agents/coord-a/anchors/surface%3Asurface-1.toml")
        );
        assert_eq!(
            read_identity_anchor(&fixture.ctx, &key)
                .unwrap()
                .unwrap()
                .id,
            anchor.id
        );
        assert_eq!(list_identity_anchors(&fixture.ctx).unwrap().len(), 1);
        assert!(remove_identity_anchor(&fixture.ctx, &key).unwrap());
        assert!(!remove_identity_anchor(&fixture.ctx, &key).unwrap());
        assert!(read_identity_anchor(&fixture.ctx, &key).unwrap().is_none());
    }

    #[test]
    fn read_identity_anchor_ignores_mismatched_anchor() {
        let fixture = CtxFixture::new();
        let key = AnchorKey {
            kind: AnchorKind::Surface,
            value: "surface-1".into(),
        };
        let anchor = identity_anchor_fixture(AnchorKind::Surface, "surface-2", None, None);
        let path = identity_anchor_path_for_id(&fixture.ctx, "agents/coord-a", &key).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, toml::to_string_pretty(&anchor).unwrap()).unwrap();

        assert!(read_identity_anchor(&fixture.ctx, &key).unwrap().is_none());
    }

    #[test]
    fn write_identity_anchor_retries_when_first_temp_path_already_exists() {
        let fixture = CtxFixture::new();
        let key = AnchorKey {
            kind: AnchorKind::Surface,
            value: "surface-1".into(),
        };
        let anchors_dir = identity_anchor_path_for_id(&fixture.ctx, "agents/coord-a", &key)
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        fs::create_dir_all(&anchors_dir).unwrap();
        let pid = std::process::id();
        fs::write(
            anchors_dir.join(format!(".wt-identity-anchor-{pid}-0.tmp")),
            "occupied",
        )
        .unwrap();

        let anchor =
            write_identity_anchor(&fixture.ctx, &key, "agents/coord-a", Some("claude")).unwrap();
        assert_eq!(anchor.id, "agents/coord-a");
        assert_eq!(
            read_identity_anchor(&fixture.ctx, &key)
                .unwrap()
                .unwrap()
                .id,
            anchor.id
        );
    }

    fn identity_anchor_fixture(
        kind: AnchorKind,
        value: &str,
        liveness_pid: Option<i32>,
        liveness_start_time: Option<&str>,
    ) -> IdentityAnchor {
        IdentityAnchor {
            id: "agents/coord-a".into(),
            anchor_kind: kind,
            anchor_value: value.into(),
            liveness_pid,
            liveness_start_time: liveness_start_time.map(str::to_string),
            anchor_agent_kind: Some("codex".into()),
            cwd: PathBuf::from("/repo"),
            created_at: "2026-05-21T00:00:00Z".into(),
            updated_at: "2026-05-21T00:00:00Z".into(),
        }
    }

    struct CtxFixture {
        _temp: TempDir,
        ctx: Ctx,
    }

    impl CtxFixture {
        fn new() -> Self {
            let temp = TempDir::new().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(&repo).unwrap();
            let storage_root = StorageRoot::from_git_common_dir(repo.join(".git"));
            let ctx = Ctx::new_with_options(
                repo.clone(),
                repo,
                Config::default(),
                Box::new(MockRunner::new()),
                Box::new(MockUi::new()),
                CtxOptions {
                    base_config: Config::default(),
                    config_source: ConfigSource::Default,
                    storage_root: Some(storage_root),
                    output_mode: OutputMode::Text,
                    verbosity: 0,
                    quiet: false,
                    launcher_coordinator_id: None,
                },
            );
            Self { _temp: temp, ctx }
        }
    }
}
