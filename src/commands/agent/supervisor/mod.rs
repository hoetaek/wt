use crate::context::Ctx;
use crate::messages::AgentId;
use crate::services::supervisor_registration::{
    Registration, list_registrations, log_path, read_registration, registration_path,
    remove_registration, supervisor_is_alive, write_registration,
};
use anyhow::{Context, Result, anyhow, bail};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_STALE_THRESHOLD_SECS: u64 = 15 * 60;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
const STOP_GRACE: Duration = Duration::from_secs(5);
const STOP_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOptions {
    pub replace: bool,
    pub surface: Option<String>,
    pub cleanup_on_session_end: Option<bool>,
    pub stale_threshold: String,
    pub poll_interval: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopOptions {
    pub agent_id: Option<String>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub foreground: bool,
    pub surface: Option<String>,
    pub cleanup_on_session_end: Option<bool>,
    pub stale_threshold_secs: u64,
    pub poll_interval_secs: u64,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogsOptions {
    pub follow: bool,
}

pub fn start(ctx: &Ctx, agent_id: &str, options: StartOptions) -> Result<()> {
    let agent_id = normalize_agent_id(agent_id)?;
    let stale_threshold_secs = parse_duration(&options.stale_threshold)
        .with_context(|| format!("Invalid --stale-threshold `{}`", options.stale_threshold))?;
    let poll_interval_secs = parse_duration(&options.poll_interval)
        .with_context(|| format!("Invalid --poll-interval `{}`", options.poll_interval))?;

    if let Some(existing) = read_registration(ctx, agent_id.as_str())? {
        if supervisor_is_alive(&existing)? {
            if !options.replace {
                bail!(
                    "Supervisor already running for {} with pid {}. Use --replace to restart it.",
                    existing.agent_id,
                    existing.pid
                );
            }
            stop_registration(ctx, &existing)?;
        } else {
            remove_registration(ctx, agent_id.as_str())?;
        }
    }

    let log_path = log_path(ctx, agent_id.as_str());
    let supervisor_dir = log_path
        .parent()
        .ok_or_else(|| anyhow!("Supervisor log path has no parent: {}", log_path.display()))?;
    fs::create_dir_all(supervisor_dir).with_context(|| {
        format!(
            "Failed to create supervisor directory {}",
            supervisor_dir.display()
        )
    })?;

    let started_by = started_by_agent()?;
    let cleanup_on_session_end = options.cleanup_on_session_end.unwrap_or_else(|| {
        started_by
            .as_deref()
            .is_some_and(|id| id.starts_with("agents/"))
    });
    let started_by = started_by.unwrap_or_else(|| "user".into());

    let child = spawn_detached_run(
        ctx,
        agent_id.as_str(),
        &log_path,
        &options,
        stale_threshold_secs,
        poll_interval_secs,
        cleanup_on_session_end,
    )?;

    let registration = Registration {
        agent_id: agent_id.as_str().into(),
        pid: child.id(),
        started_at: current_utc_timestamp(),
        started_by,
        cleanup_on_session_end,
        target_surface_id: options.surface.clone(),
        target_agent_kind: None,
        stale_threshold_secs,
        poll_interval_secs,
        log_path: log_path.clone(),
    };
    write_registration(ctx, &registration)?;

    if !ctx.quiet {
        println!(
            "Supervisor started: agent={} pid={} registration={} log={}",
            registration.agent_id,
            registration.pid,
            ctx.storage_root
                .display_path(&registration_path(ctx, &registration.agent_id)),
            ctx.storage_root.display_path(&registration.log_path)
        );
    }
    Ok(())
}

pub fn stop(ctx: &Ctx, options: StopOptions) -> Result<()> {
    let target = options
        .agent_id
        .as_deref()
        .map(normalize_agent_id)
        .transpose()?;
    let owner = options
        .owned_by
        .as_deref()
        .map(normalize_agent_id)
        .transpose()?;

    if target.is_none() && owner.is_none() {
        bail!("Stop requires <agent-id> or --owned-by <agent-id>.");
    }

    let registrations = if let Some(target) = target.as_ref() {
        match read_registration(ctx, target.as_str())? {
            Some(registration) => vec![registration],
            None => bail!("No supervisor registration found for {}", target.as_str()),
        }
    } else {
        list_registrations(ctx)?
    };

    let mut stopped = 0usize;
    for registration in registrations {
        if owner
            .as_ref()
            .is_some_and(|owner| registration.started_by != owner.as_str())
        {
            continue;
        }
        stop_registration(ctx, &registration)?;
        stopped += 1;
        if !ctx.quiet {
            println!(
                "Supervisor stopped: agent={} pid={}",
                registration.agent_id, registration.pid
            );
        }
    }

    if stopped == 0 && !ctx.quiet {
        println!("No matching supervisor registrations.");
    }
    Ok(())
}

pub fn status(ctx: &Ctx, agent_id: Option<&str>) -> Result<()> {
    let target = agent_id.map(normalize_agent_id).transpose()?;
    let registrations = if let Some(target) = target.as_ref() {
        read_registration(ctx, target.as_str())?
            .into_iter()
            .collect()
    } else {
        list_registrations(ctx)?
    };
    let reports = registrations
        .iter()
        .map(|registration| status_report(ctx, registration))
        .collect::<Result<Vec<_>>>()?;

    if ctx.is_json() {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else if reports.is_empty() {
        println!("No supervisor registrations.");
    } else {
        for report in reports {
            println!(
                "{}\t{}\tpid={}\tstale_threshold={}\tpoll_interval={}\tlog={}",
                report.agent_id,
                report.state,
                report.pid,
                report.stale_threshold,
                report.poll_interval,
                report.log_path
            );
        }
    }
    Ok(())
}

pub fn logs(ctx: &Ctx, agent_id: &str, options: LogsOptions) -> Result<()> {
    let agent_id = normalize_agent_id(agent_id)?;
    let registration = read_registration(ctx, agent_id.as_str())?
        .ok_or_else(|| anyhow!("No supervisor registration found for {}", agent_id.as_str()))?;
    if options.follow {
        follow_log(&registration.log_path)
    } else {
        let mut file = File::open(&registration.log_path).with_context(|| {
            format!(
                "Failed to open supervisor log {}",
                registration.log_path.display()
            )
        })?;
        let mut content = String::new();
        file.read_to_string(&mut content).with_context(|| {
            format!(
                "Failed to read supervisor log {}",
                registration.log_path.display()
            )
        })?;
        print!("{content}");
        Ok(())
    }
}

pub fn run(ctx: &Ctx, agent_id: &str, options: RunOptions) -> Result<()> {
    let agent_id = normalize_agent_id(agent_id)?;
    let log_path = options.log_path.unwrap_or_else(|| {
        crate::services::supervisor_registration::log_path(ctx, agent_id.as_str())
    });
    append_log(
        &log_path,
        &format!(
            "RUN_STUB agent_id={} event=loop-not-yet-implemented foreground={}",
            agent_id.as_str(),
            options.foreground
        ),
    )?;
    Ok(())
}

fn spawn_detached_run(
    ctx: &Ctx,
    agent_id: &str,
    log_path: &Path,
    options: &StartOptions,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    cleanup_on_session_end: bool,
) -> Result<std::process::Child> {
    let exe = std::env::current_exe().context("Failed to resolve current wt executable")?;
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("Failed to open supervisor log {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("Failed to clone supervisor log {}", log_path.display()))?;
    let stdin = File::open("/dev/null").context("Failed to open /dev/null")?;

    let mut command = Command::new(exe);
    command
        .arg("-C")
        .arg(&ctx.invocation_root)
        .arg("agent")
        .arg("supervisor")
        .arg("run")
        .arg(agent_id)
        .arg("--stale-threshold-secs")
        .arg(stale_threshold_secs.to_string())
        .arg("--poll-interval-secs")
        .arg(poll_interval_secs.to_string())
        .arg("--log-path")
        .arg(log_path);
    if let Some(surface) = options.surface.as_ref() {
        command.arg("--surface").arg(surface);
    }
    command
        .arg("--cleanup-on-session-end")
        .arg(cleanup_on_session_end.to_string())
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(std::io::Error::other)
        });
    }

    command
        .spawn()
        .with_context(|| format!("Failed to spawn detached supervisor for {agent_id}"))
}

fn stop_registration(ctx: &Ctx, registration: &Registration) -> Result<()> {
    let pid = Pid::from_raw(registration.pid as i32);
    if supervisor_is_alive(registration)? {
        signal::kill(pid, Signal::SIGTERM).with_context(|| {
            format!(
                "Failed to send SIGTERM to supervisor PID {} for {}",
                registration.pid, registration.agent_id
            )
        })?;

        let start = std::time::Instant::now();
        while start.elapsed() < STOP_GRACE {
            if read_registration(ctx, &registration.agent_id)?.is_none()
                || !supervisor_is_alive(registration)?
            {
                remove_registration(ctx, &registration.agent_id)?;
                return Ok(());
            }
            std::thread::sleep(STOP_POLL);
        }

        if supervisor_is_alive(registration)? {
            signal::kill(pid, Signal::SIGKILL).with_context(|| {
                format!(
                    "Failed to send SIGKILL to supervisor PID {} for {}",
                    registration.pid, registration.agent_id
                )
            })?;
        }
    }
    remove_registration(ctx, &registration.agent_id)?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct StatusReport {
    agent_id: String,
    pid: u32,
    state: String,
    started_at: String,
    started_by: String,
    cleanup_on_session_end: bool,
    target_surface_id: Option<String>,
    target_agent_kind: Option<String>,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    stale_threshold: String,
    poll_interval: String,
    log_path: String,
}

fn status_report(ctx: &Ctx, registration: &Registration) -> Result<StatusReport> {
    let state = match supervisor_is_alive(registration) {
        Ok(true) => "running",
        Ok(false) => "stale",
        Err(_) => "unknown",
    };
    Ok(StatusReport {
        agent_id: registration.agent_id.clone(),
        pid: registration.pid,
        state: state.into(),
        started_at: registration.started_at.clone(),
        started_by: registration.started_by.clone(),
        cleanup_on_session_end: registration.cleanup_on_session_end,
        target_surface_id: registration.target_surface_id.clone(),
        target_agent_kind: registration.target_agent_kind.clone(),
        stale_threshold_secs: registration.stale_threshold_secs,
        poll_interval_secs: registration.poll_interval_secs,
        stale_threshold: format_duration(registration.stale_threshold_secs),
        poll_interval: format_duration(registration.poll_interval_secs),
        log_path: ctx.storage_root.display_path(&registration.log_path),
    })
}

fn follow_log(path: &Path) -> Result<()> {
    let mut position = 0;
    loop {
        match File::open(path) {
            Ok(mut file) => {
                file.seek(SeekFrom::Start(position))
                    .with_context(|| format!("Failed to seek supervisor log {}", path.display()))?;
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .with_context(|| format!("Failed to read supervisor log {}", path.display()))?;
                if !content.is_empty() {
                    print!("{content}");
                    std::io::stdout().flush()?;
                }
                position = file.stream_position().with_context(|| {
                    format!("Failed to inspect supervisor log {}", path.display())
                })?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                position = 0;
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("Failed to open supervisor log {}", path.display()));
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn append_log(path: &Path, event: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create supervisor log directory {}",
                parent.display()
            )
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open supervisor log {}", path.display()))?;
    writeln!(file, "{} {}", current_utc_timestamp(), event)
        .with_context(|| format!("Failed to write supervisor log {}", path.display()))
}

fn normalize_agent_id(value: &str) -> Result<AgentId> {
    AgentId::parse(value)
}

fn started_by_agent() -> Result<Option<String>> {
    match std::env::var("WT_AGENT_ID") {
        Ok(value) if !value.trim().is_empty() => Ok(Some(AgentId::parse(&value)?.as_str().into())),
        _ => Ok(None),
    }
}

pub fn parse_duration(value: &str) -> Result<u64> {
    let value = value.trim();
    if value.is_empty() {
        bail!("duration cannot be empty");
    }
    let (number, unit) = value.split_at(
        value
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(value.len()),
    );
    if number.is_empty() {
        bail!("duration must start with a positive integer");
    }
    let amount = number
        .parse::<u64>()
        .context("duration must start with a positive integer")?;
    if amount == 0 {
        bail!("duration must be positive");
    }
    let multiplier = match unit {
        "" | "s" | "sec" | "secs" => 1,
        "m" | "min" | "mins" => 60,
        "h" | "hr" | "hrs" => 60 * 60,
        _ => bail!("duration unit must be s, m, or h"),
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow!("duration is too large"))
}

pub fn default_stale_threshold() -> String {
    format_duration(DEFAULT_STALE_THRESHOLD_SECS)
}

pub fn default_poll_interval() -> String {
    format_duration(DEFAULT_POLL_INTERVAL_SECS)
}

fn format_duration(seconds: u64) -> String {
    if seconds != 0 && seconds % (60 * 60) == 0 {
        format!("{}h", seconds / (60 * 60))
    } else if seconds != 0 && seconds % 60 == 0 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

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

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_human_durations() {
        assert_eq!(parse_duration("90s").unwrap(), 90);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("1h").unwrap(), 3600);
    }

    #[test]
    fn rejects_invalid_durations() {
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("m").is_err());
        assert!(parse_duration("10d").is_err());
    }
}
