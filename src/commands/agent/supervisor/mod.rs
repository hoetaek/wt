use crate::commands::msg::{ensure_no_legacy_message_storage, runtime_message_store};
use crate::context::Ctx;
use crate::messages::{AgentId, Message, MessageLease, MessageStore};
use crate::messages::{MessageDeliveryState, MessageInspectionRecord};
use crate::services::cmux::CmuxService;
use crate::services::cmux_push::{CmuxPushService, DEFAULT_PAYLOAD_CAP_BYTES, PushKind};
use crate::services::identity_locator::process_start_time;
use crate::services::inbox_watcher::InboxWatcher;
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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_STALE_THRESHOLD_SECS: u64 = 15 * 60;
const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
const DEFAULT_CYCLE_CAP: usize = 64;
const MAX_PUSH_ATTEMPTS: u32 = 3;
const CLAIM_LEASE_SECS: u64 = 60;
const STOP_GRACE: Duration = Duration::from_secs(5);
const STOP_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOptions {
    pub replace: bool,
    pub surface: Option<String>,
    pub kind: Option<String>,
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
    pub kind: Option<String>,
    pub cleanup_on_session_end: Option<bool>,
    pub stale_threshold_secs: u64,
    pub poll_interval_secs: u64,
    pub cycle_cap: usize,
    pub payload_cap: usize,
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
    let target_agent_kind = options
        .kind
        .as_deref()
        .map(parse_kind_option)
        .transpose()?
        .map(|kind| kind.as_str().to_string());

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

    let log_path = log_path(ctx, agent_id.as_str())?;
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

    let launch = spawn_supervisor_run(
        ctx,
        agent_id.as_str(),
        &log_path,
        &options,
        stale_threshold_secs,
        poll_interval_secs,
        cleanup_on_session_end,
    )?;

    let pid_start_time = match process_start_time(launch.pid as i32) {
        Ok(start_time) => start_time,
        Err(err) => {
            cleanup_failed_launch(ctx, launch);
            return Err(err).context("Failed to record supervisor process start time after spawn");
        }
    };

    let registration = Registration {
        agent_id: agent_id.as_str().into(),
        pid: launch.pid,
        pid_start_time,
        started_at: current_utc_timestamp(),
        started_by,
        cleanup_on_session_end,
        target_surface_id: options.surface.clone(),
        target_agent_kind,
        host_workspace_id: launch.host_workspace_id.clone(),
        host_pane_id: launch.host_pane_id.clone(),
        host_surface_id: launch.host_surface_id.clone(),
        stale_threshold_secs,
        poll_interval_secs,
        log_path: log_path.clone(),
    };
    if let Err(err) = write_registration(ctx, &registration) {
        cleanup_failed_launch(ctx, launch);
        return Err(err).context("Failed to persist supervisor registration after spawn");
    }

    if !ctx.quiet {
        println!(
            "Supervisor started: agent={} pid={} registration={} log={}",
            registration.agent_id,
            registration.pid,
            ctx.storage_root
                .display_path(&registration_path(ctx, &registration.agent_id)?),
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
        if owner.is_some() && !registration.cleanup_on_session_end {
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
                "{}\t{}\tpid={}\tstale_threshold={}\tpoll_interval={}\thost={}\tlog={}",
                report.agent_id,
                report.state,
                report.pid,
                report.stale_threshold,
                report.poll_interval,
                report.host_summary(),
                report.log_path
            );
        }
    }
    Ok(())
}

pub fn logs(ctx: &Ctx, agent_id: &str, options: LogsOptions) -> Result<()> {
    let agent_id = normalize_agent_id(agent_id)?;
    let log_path = match read_registration(ctx, agent_id.as_str())? {
        Some(registration) => registration.log_path,
        None => crate::services::supervisor_registration::log_path(ctx, agent_id.as_str())?,
    };
    if options.follow {
        follow_log(&log_path)
    } else {
        let mut file = File::open(&log_path)
            .with_context(|| format!("Failed to open supervisor log {}", log_path.display()))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .with_context(|| format!("Failed to read supervisor log {}", log_path.display()))?;
        print!("{content}");
        Ok(())
    }
}

pub fn run(ctx: &Ctx, agent_id: &str, options: RunOptions) -> Result<()> {
    let agent_id = normalize_agent_id(agent_id)?;
    let log_path = match options.log_path {
        Some(path) => path,
        None => crate::services::supervisor_registration::log_path(ctx, agent_id.as_str())?,
    };
    let registration = read_registration(ctx, agent_id.as_str())?;
    let target_surface_id = options.surface.or_else(|| {
        registration
            .as_ref()
            .and_then(|registration| registration.target_surface_id.clone())
    });
    let fallback_kind = options
        .kind
        .as_deref()
        .map(parse_kind_option)
        .transpose()?
        .or_else(|| {
            registration
                .as_ref()
                .and_then(|registration| registration.target_agent_kind.as_deref())
                .map(PushKind::parse)
        })
        .unwrap_or(PushKind::Unknown);
    let config = SupervisorLoopConfig {
        target_surface_id,
        fallback_kind,
        stale_threshold_secs: options.stale_threshold_secs,
        poll_interval_secs: options.poll_interval_secs,
        cycle_cap: options.cycle_cap,
        payload_cap: options.payload_cap,
        log_path,
    };
    if config.cycle_cap == 0 {
        bail!("Supervisor --cycle-cap must be positive");
    }
    if config.payload_cap == 0 {
        bail!("Supervisor --payload-cap must be positive");
    }
    if config.stale_threshold_secs == 0 {
        bail!("Supervisor --stale-threshold-secs must be positive");
    }
    if config.poll_interval_secs == 0 {
        bail!("Supervisor --poll-interval-secs must be positive");
    }
    append_log(
        &config.log_path,
        &format!(
            "START agent_id={} foreground={} stale_threshold_secs={} poll_interval_secs={} cycle_cap={} payload_cap={} wake=notify",
            agent_id.as_str(),
            options.foreground,
            config.stale_threshold_secs,
            config.poll_interval_secs,
            config.cycle_cap,
            config.payload_cap
        ),
    )?;
    ensure_no_legacy_message_storage(ctx)?;
    let inbox_new =
        agent_id.inbox_state_dir(&ctx.storage_root.runtime_dir(), MessageDeliveryState::New);
    fs::create_dir_all(&inbox_new)
        .with_context(|| format!("Failed to create inbox: {}", inbox_new.display()))?;
    let mut watcher = InboxWatcher::new(&inbox_new)?;
    let mut state = SupervisorLoopState::default();
    loop {
        let stop_path = stop_requested_path(ctx, &agent_id);
        if stop_path.is_file() {
            append_log(
                &config.log_path,
                &format!("STOP agent_id={}", agent_id.as_str()),
            )?;
            match fs::remove_file(&stop_path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("Failed to remove stop sentinel {}", stop_path.display())
                    });
                }
            }
            remove_registration(ctx, agent_id.as_str())?;
            return Ok(());
        }
        run_one_cycle(ctx, agent_id.as_str(), &config, &mut state)?;
        wait_for_next_cycle(ctx, agent_id.as_str(), &config, &mut watcher)?;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SupervisorLoopConfig {
    target_surface_id: Option<String>,
    fallback_kind: PushKind,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    cycle_cap: usize,
    payload_cap: usize,
    log_path: PathBuf,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct SupervisorLoopState {
    pushes_total: u64,
    cycles_since_start: u64,
    last_push_at: Option<String>,
}

#[derive(Debug)]
struct SupervisorLaunch {
    pid: u32,
    host_workspace_id: Option<String>,
    host_pane_id: Option<String>,
    host_surface_id: Option<String>,
    child: Option<std::process::Child>,
}

fn run_one_cycle(
    ctx: &Ctx,
    agent_id: &str,
    config: &SupervisorLoopConfig,
    state: &mut SupervisorLoopState,
) -> Result<()> {
    state.cycles_since_start = state.cycles_since_start.saturating_add(1);
    let store = runtime_message_store(ctx)?;
    store.reclaim_expired_leases(agent_id, SystemTime::now())?;
    let mut candidates = store.list_new(agent_id)?;
    candidates.extend(store.list_retry(agent_id)?);
    candidates.sort_by(|left, right| {
        let left_created = left
            .message
            .as_ref()
            .map(|message| message.meta.created_at.as_str())
            .unwrap_or_default();
        let right_created = right
            .message
            .as_ref()
            .map(|message| message.meta.created_at.as_str())
            .unwrap_or_default();
        left_created
            .cmp(right_created)
            .then_with(|| left.path.cmp(&right.path))
    });
    if candidates.is_empty() {
        return Ok(());
    }

    let now = SystemTime::now();
    let mut processed_this_cycle = 0usize;
    for record in candidates {
        if !record.is_valid() {
            let error = record.error.as_deref().unwrap_or("invalid message");
            if fail_candidate_path(&store, agent_id, &record, error)
                .with_context(|| format!("Failed to quarantine invalid message {}", record.id))?
                .is_some()
            {
                append_log(
                    &config.log_path,
                    &format!(
                        "FAILED_INVALID msg_id={} error={}",
                        record.id,
                        log_value(error)
                    ),
                )?;
            }
            continue;
        }
        let Some(message) = record.message.as_ref() else {
            if fail_candidate_path(&store, agent_id, &record, "missing message payload")
                .with_context(|| format!("Failed to quarantine invalid message {}", record.id))?
                .is_some()
            {
                append_log(
                    &config.log_path,
                    &format!("FAILED_INVALID msg_id={} error=missing_message", record.id),
                )?;
            }
            continue;
        };
        let created_at = match parse_utc_timestamp(&message.meta.created_at) {
            Ok(created_at) => created_at,
            Err(err) => {
                let error = format!(
                    "invalid meta.created_at `{}`: {err:#}",
                    message.meta.created_at
                );
                if fail_candidate_path(&store, agent_id, &record, &error)
                    .with_context(|| {
                        format!("Failed to quarantine invalid message {}", message.meta.id)
                    })?
                    .is_some()
                {
                    append_log(
                        &config.log_path,
                        &format!(
                            "FAILED_INVALID msg_id={} error=invalid_created_at detail={}",
                            message.meta.id,
                            log_value(&format!("{err:#}"))
                        ),
                    )?;
                }
                continue;
            }
        };
        let age = now
            .duration_since(created_at)
            .unwrap_or_else(|_| Duration::from_secs(0));
        if age < Duration::from_secs(config.stale_threshold_secs) {
            append_log(
                &config.log_path,
                &format!(
                    "SKIP_FRESH msg_id={} age_secs={} stale_threshold_secs={}",
                    message.meta.id,
                    age.as_secs(),
                    config.stale_threshold_secs
                ),
            )?;
            continue;
        }
        if processed_this_cycle >= config.cycle_cap {
            append_log(
                &config.log_path,
                &format!(
                    "CYCLE_CAP_REACHED cycle={} cap={} skipped_msg_id={}",
                    state.cycles_since_start, config.cycle_cap, message.meta.id
                ),
            )?;
            break;
        }
        processed_this_cycle += 1;

        if has_terminal_marker(message) {
            if deliver_candidate_without_claim(&store, agent_id, &record)
                .with_context(|| format!("Failed to deliver terminal message {}", message.meta.id))?
                .is_some()
            {
                append_log(
                    &config.log_path,
                    &format!("TERMINAL_DELIVERED msg_id={}", message.meta.id),
                )?;
            }
            continue;
        }

        let Some(surface_id) = config.target_surface_id.as_deref() else {
            append_log(
                &config.log_path,
                &format!("SKIP_NO_SURFACE msg_id={}", message.meta.id),
            )?;
            continue;
        };

        let lease = MessageLease::new(Duration::from_secs(CLAIM_LEASE_SECS))?;
        let Some(claimed) =
            claim_candidate_path(&store, agent_id, &record, "agents/supervisor", lease)?
        else {
            continue;
        };
        let payload = render_payload(&claimed.message, config.payload_cap);
        let push = CmuxPushService::new(ctx.runner.as_ref()).with_payload_cap(config.payload_cap);
        let detected_target = push.detect_target(surface_id).ok();
        let detected_kind = detected_target
            .as_ref()
            .map(|target| target.kind)
            .unwrap_or(PushKind::Unknown);
        let workspace = detected_target
            .as_ref()
            .and_then(|target| target.workspace.as_deref());
        let kind = if detected_kind == PushKind::Unknown {
            config.fallback_kind
        } else {
            detected_kind
        };
        append_log(
            &config.log_path,
            &format!(
                "PUSH_ATTEMPT msg_id={} target_surface={} kind={} payload_bytes={}",
                claimed.message.meta.id,
                surface_id,
                kind.as_str(),
                payload.len()
            ),
        )?;
        match push.push_to_surface_in_workspace(surface_id, workspace, kind, &payload) {
            Ok(()) => {
                store.acknowledge_claimed_path(
                    agent_id,
                    "agents/supervisor",
                    &claimed.claimed_path,
                )?;
                state.pushes_total = state.pushes_total.saturating_add(1);
                state.last_push_at = Some(current_utc_timestamp());
                append_log(
                    &config.log_path,
                    &format!(
                        "PUSH_SUCCESS msg_id={} pushes_total={}",
                        claimed.message.meta.id, state.pushes_total
                    ),
                )?;
            }
            Err(err) => {
                let error = format!("{err:#}");
                if claimed.message.delivery.attempts.saturating_add(1) >= MAX_PUSH_ATTEMPTS {
                    store.fail_delivery(
                        agent_id,
                        "agents/supervisor",
                        &claimed.message.meta.id,
                        &error,
                    )?;
                    append_log(
                        &config.log_path,
                        &format!(
                            "PUSH_FAILED msg_id={} attempts={} error={}",
                            claimed.message.meta.id,
                            claimed.message.delivery.attempts.saturating_add(1),
                            log_value(&error)
                        ),
                    )?;
                } else {
                    store.retry_delivery(
                        agent_id,
                        "agents/supervisor",
                        &claimed.message.meta.id,
                        &error,
                    )?;
                    append_log(
                        &config.log_path,
                        &format!(
                            "PUSH_RETRY msg_id={} attempts={} error={}",
                            claimed.message.meta.id,
                            claimed.message.delivery.attempts.saturating_add(1),
                            log_value(&error)
                        ),
                    )?;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    Ok(())
}

fn claim_candidate_path(
    store: &MessageStore,
    agent_id: &str,
    record: &MessageInspectionRecord,
    claimed_by: &str,
    lease: MessageLease,
) -> Result<Option<crate::messages::ClaimedMessage>> {
    match record.state {
        MessageDeliveryState::New => {
            store.claim_new_path(agent_id, &record.path, claimed_by, lease)
        }
        MessageDeliveryState::Retry => {
            store.claim_retry_path(agent_id, &record.path, claimed_by, lease)
        }
        other => bail!(
            "Supervisor cannot claim message {} from inbox/{}",
            record.path.display(),
            other.as_str()
        ),
    }
}

fn deliver_candidate_without_claim(
    store: &MessageStore,
    agent_id: &str,
    record: &MessageInspectionRecord,
) -> Result<Option<crate::messages::DeliveredMessage>> {
    match record.state {
        MessageDeliveryState::New => store.deliver_new_without_claim(agent_id, &record.path),
        MessageDeliveryState::Retry => store.deliver_retry_without_claim(agent_id, &record.path),
        other => bail!(
            "Supervisor cannot deliver message {} from inbox/{}",
            record.path.display(),
            other.as_str()
        ),
    }
}

fn fail_candidate_path(
    store: &MessageStore,
    agent_id: &str,
    record: &MessageInspectionRecord,
    error: &str,
) -> Result<Option<crate::messages::FailedMessage>> {
    match record.state {
        MessageDeliveryState::New => store.fail_new_path(agent_id, &record.path, error),
        MessageDeliveryState::Retry => store.fail_retry_path(agent_id, &record.path, error),
        other => bail!(
            "Supervisor cannot fail message {} from inbox/{}",
            record.path.display(),
            other.as_str()
        ),
    }
}

fn wait_for_next_cycle(
    ctx: &Ctx,
    agent_id: &str,
    config: &SupervisorLoopConfig,
    watcher: &mut InboxWatcher,
) -> Result<()> {
    let wait_for = next_wait_duration(ctx, agent_id, config)?;
    match watcher.wait_next(wait_for)? {
        Some(path) => append_log(
            &config.log_path,
            &format!(
                "WATCH_EVENT agent_id={} path={}",
                agent_id,
                ctx.storage_root.display_path(&path)
            ),
        ),
        None => Ok(()),
    }
}

fn next_wait_duration(
    ctx: &Ctx,
    agent_id: &str,
    config: &SupervisorLoopConfig,
) -> Result<Duration> {
    let poll = Duration::from_secs(config.poll_interval_secs);
    let Some(until_stale) = next_fresh_message_stale_duration(ctx, agent_id, config)? else {
        return Ok(poll);
    };
    Ok(until_stale.min(poll).max(Duration::from_millis(100)))
}

fn next_fresh_message_stale_duration(
    ctx: &Ctx,
    agent_id: &str,
    config: &SupervisorLoopConfig,
) -> Result<Option<Duration>> {
    let store = runtime_message_store(ctx)?;
    let threshold = Duration::from_secs(config.stale_threshold_secs);
    let now = SystemTime::now();
    let mut next = None;
    for record in store.list_new(agent_id)? {
        let Some(message) = record.message.as_ref() else {
            continue;
        };
        let Ok(created_at) = parse_utc_timestamp(&message.meta.created_at) else {
            continue;
        };
        let age = now
            .duration_since(created_at)
            .unwrap_or_else(|_| Duration::from_secs(0));
        if age >= threshold {
            continue;
        }
        let remaining = threshold - age;
        next = Some(match next {
            Some(current) => remaining.min(current),
            None => remaining,
        });
    }
    Ok(next)
}

fn spawn_supervisor_run(
    ctx: &Ctx,
    agent_id: &str,
    log_path: &Path,
    options: &StartOptions,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    cleanup_on_session_end: bool,
) -> Result<SupervisorLaunch> {
    if options.surface.is_some() {
        spawn_cmux_hosted_run(
            ctx,
            agent_id,
            log_path,
            options,
            stale_threshold_secs,
            poll_interval_secs,
            cleanup_on_session_end,
        )
    } else {
        spawn_detached_run(
            ctx,
            agent_id,
            log_path,
            options,
            stale_threshold_secs,
            poll_interval_secs,
            cleanup_on_session_end,
        )
    }
}

fn spawn_detached_run(
    ctx: &Ctx,
    agent_id: &str,
    log_path: &Path,
    options: &StartOptions,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    cleanup_on_session_end: bool,
) -> Result<SupervisorLaunch> {
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
        .arg("--cycle-cap")
        .arg(DEFAULT_CYCLE_CAP.to_string())
        .arg("--payload-cap")
        .arg(DEFAULT_PAYLOAD_CAP_BYTES.to_string())
        .arg("--log-path")
        .arg(log_path);
    if let Some(surface) = options.surface.as_ref() {
        command.arg("--surface").arg(surface);
    }
    if let Some(kind) = options.kind.as_ref() {
        command.arg("--kind").arg(kind);
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

    let child = command
        .spawn()
        .with_context(|| format!("Failed to spawn detached supervisor for {agent_id}"))?;
    Ok(SupervisorLaunch {
        pid: child.id(),
        host_workspace_id: None,
        host_pane_id: None,
        host_surface_id: None,
        child: Some(child),
    })
}

fn spawn_cmux_hosted_run(
    ctx: &Ctx,
    agent_id: &str,
    log_path: &Path,
    options: &StartOptions,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    cleanup_on_session_end: bool,
) -> Result<SupervisorLaunch> {
    let exe = std::env::current_exe().context("Failed to resolve current wt executable")?;
    let pid_path = log_path.with_extension("pid");
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        bail!("Surface-backed supervisors require cmux on PATH");
    }
    let target_surface = options
        .surface
        .as_deref()
        .ok_or_else(|| anyhow!("Surface-backed supervisor missing target surface"))?;
    let host_location = cmux
        .find_surface_location(target_surface)?
        .ok_or_else(|| anyhow!("Target cmux surface not found: {target_surface}"))?;

    match fs::remove_file(&pid_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to remove stale pid file {}", pid_path.display())
            });
        }
    }

    let mut args = vec![
        shell_arg(&exe.to_string_lossy()),
        "-C".into(),
        shell_arg(&ctx.invocation_root.to_string_lossy()),
        "agent".into(),
        "supervisor".into(),
        "run".into(),
        shell_arg(agent_id),
        "--foreground".into(),
        "--stale-threshold-secs".into(),
        stale_threshold_secs.to_string(),
        "--poll-interval-secs".into(),
        poll_interval_secs.to_string(),
        "--cycle-cap".into(),
        DEFAULT_CYCLE_CAP.to_string(),
        "--payload-cap".into(),
        DEFAULT_PAYLOAD_CAP_BYTES.to_string(),
        "--log-path".into(),
        shell_arg(&log_path.to_string_lossy()),
    ];
    if let Some(surface) = options.surface.as_ref() {
        args.push("--surface".into());
        args.push(shell_arg(surface));
    }
    if let Some(kind) = options.kind.as_ref() {
        args.push("--kind".into());
        args.push(shell_arg(kind));
    }
    args.push("--cleanup-on-session-end".into());
    args.push(cleanup_on_session_end.to_string());

    let command = format!(
        "sh -lc {}",
        shell_arg(&format!(
            "echo $$ > {}; exec {}",
            shell_arg(&pid_path.to_string_lossy()),
            args.join(" ")
        ))
    );
    let host_surface_id = cmux
        .new_surface_with_focus(
            &host_location.pane_handle,
            &host_location.workspace_handle,
            false,
        )
        .with_context(|| format!("Failed to create cmux supervisor surface for {agent_id}"))?;
    if let Err(err) = cmux.send(
        &host_surface_id,
        &host_location.workspace_handle,
        &format!("{command}\n"),
    ) {
        let _ = cmux.close_surface(&host_surface_id, Some(&host_location.workspace_handle));
        return Err(err).with_context(|| {
            format!("Failed to start supervisor command in cmux surface for {agent_id}")
        });
    }
    let pid = match wait_for_pid_file(&pid_path, Duration::from_secs(5)) {
        Ok(pid) => pid,
        Err(err) => {
            let _ = cmux.close_surface(&host_surface_id, Some(&host_location.workspace_handle));
            return Err(err);
        }
    };
    let _ = fs::remove_file(&pid_path);
    Ok(SupervisorLaunch {
        pid,
        host_workspace_id: Some(host_location.workspace_handle),
        host_pane_id: Some(host_location.pane_handle),
        host_surface_id: Some(host_surface_id),
        child: None,
    })
}

fn wait_for_pid_file(path: &Path, timeout: Duration) -> Result<u32> {
    let started = Instant::now();
    loop {
        match fs::read_to_string(path) {
            Ok(content) => {
                let pid = content
                    .trim()
                    .parse::<u32>()
                    .with_context(|| format!("Invalid supervisor pid file {}", path.display()))?;
                if pid > 0 {
                    return Ok(pid);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("Failed to read supervisor pid file {}", path.display())
                });
            }
        }
        if started.elapsed() >= timeout {
            bail!(
                "Timed out waiting for cmux surface-hosted supervisor pid file {}",
                path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn cleanup_failed_launch(ctx: &Ctx, mut launch: SupervisorLaunch) {
    let _ = signal::kill(Pid::from_raw(launch.pid as i32), Signal::SIGKILL);
    close_cmux_host(
        ctx,
        launch.host_surface_id.as_deref(),
        launch.host_workspace_id.as_deref(),
    );
    if let Some(child) = launch.child.as_mut() {
        let _ = signal::kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
        let _ = child.try_wait();
    }
}

fn parse_kind_option(value: &str) -> Result<PushKind> {
    let kind = PushKind::parse(value);
    if kind == PushKind::Unknown && value.trim() != "unknown" {
        bail!("Supervisor kind must be claude, codex, or unknown");
    }
    Ok(kind)
}

fn stop_requested_path(ctx: &Ctx, agent_id: &AgentId) -> PathBuf {
    ctx.storage_root
        .runtime_agent_dir(agent_id)
        .join("supervisor.stop")
}

fn has_terminal_marker(message: &Message) -> bool {
    let body = message.text_content();
    let body = body.trim_start();
    body.starts_with("[done]") || body.starts_with("[stop]")
}

fn render_payload(message: &Message, cap_bytes: usize) -> String {
    let summary = ascii_words(&message.body.summary);
    let content = ascii_words(&message.text_content());
    let reply_target = ascii_words(&message.meta.from);
    let mut payload = format!(
        "from {reply_target}: {summary}. {content} respond via: wt msg send --to {reply_target}"
    );
    payload = compact_spaces(&payload);
    if payload.len() <= cap_bytes {
        return payload;
    }

    let mut metadata_only = compact_spaces(&format!(
        "from {}: message {}. respond via: wt msg send --to {}",
        reply_target,
        ascii_words(&message.meta.id),
        reply_target
    ));
    if metadata_only.len() > cap_bytes {
        metadata_only.truncate(cap_bytes);
        while !metadata_only.is_char_boundary(metadata_only.len()) {
            metadata_only.pop();
        }
    }
    metadata_only
}

fn ascii_words(value: &str) -> String {
    compact_spaces(&value.chars().filter(|ch| ch.is_ascii()).collect::<String>())
}

fn compact_spaces(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn log_value(value: &str) -> String {
    ascii_words(value).replace(' ', "_")
}

fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
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
            match read_registration(ctx, &registration.agent_id)? {
                None => {
                    close_registered_cmux_host(ctx, registration);
                    return Ok(());
                }
                Some(current) if !same_registered_supervisor(&current, registration) => {
                    close_registered_cmux_host(ctx, registration);
                    return Ok(());
                }
                Some(_) if !supervisor_is_alive(registration)? => {
                    remove_registration(ctx, &registration.agent_id)?;
                    close_registered_cmux_host(ctx, registration);
                    return Ok(());
                }
                Some(_) => {}
            }
            std::thread::sleep(STOP_POLL);
        }

        if let Some(current) = read_registration(ctx, &registration.agent_id)? {
            if !same_registered_supervisor(&current, registration) {
                close_registered_cmux_host(ctx, registration);
                return Ok(());
            }
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
    remove_registration_if_current(ctx, registration)?;
    close_registered_cmux_host(ctx, registration);
    Ok(())
}

fn close_registered_cmux_host(ctx: &Ctx, registration: &Registration) {
    close_cmux_host(
        ctx,
        registration.host_surface_id.as_deref(),
        registration.host_workspace_id.as_deref(),
    );
}

fn close_cmux_host(ctx: &Ctx, surface: Option<&str>, workspace: Option<&str>) {
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if let Some(surface) = surface {
        let _ = cmux.close_surface(surface, workspace);
    } else if let Some(workspace) = workspace {
        let _ = cmux.close_workspace(workspace);
    }
}

fn same_registered_supervisor(current: &Registration, expected: &Registration) -> bool {
    current.pid == expected.pid && current.pid_start_time == expected.pid_start_time
}

fn remove_registration_if_current(ctx: &Ctx, registration: &Registration) -> Result<()> {
    if let Some(current) = read_registration(ctx, &registration.agent_id)? {
        if same_registered_supervisor(&current, registration) {
            remove_registration(ctx, &registration.agent_id)?;
        }
    }
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
    host_workspace_id: Option<String>,
    host_pane_id: Option<String>,
    host_surface_id: Option<String>,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
    stale_threshold: String,
    poll_interval: String,
    log_path: String,
}

impl StatusReport {
    fn host_summary(&self) -> String {
        match (
            self.host_surface_id.as_deref(),
            self.host_workspace_id.as_deref(),
        ) {
            (Some(surface), Some(workspace)) => format!("{surface}@{workspace}"),
            (Some(surface), None) => surface.to_string(),
            (None, Some(workspace)) => workspace.to_string(),
            (None, None) => "-".into(),
        }
    }
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
        host_workspace_id: registration.host_workspace_id.clone(),
        host_pane_id: registration.host_pane_id.clone(),
        host_surface_id: registration.host_surface_id.clone(),
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

fn parse_utc_timestamp(value: &str) -> Result<SystemTime> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        bail!("expected UTC timestamp formatted as YYYY-MM-DDTHH:MM:SSZ");
    }
    let year = parse_i32_digits(&bytes[0..4], "year")?;
    let month = parse_u32_digits(&bytes[5..7], "month")?;
    let day = parse_u32_digits(&bytes[8..10], "day")?;
    let hour = parse_u32_digits(&bytes[11..13], "hour")?;
    let minute = parse_u32_digits(&bytes[14..16], "minute")?;
    let second = parse_u32_digits(&bytes[17..19], "second")?;
    if !(1..=12).contains(&month) {
        bail!("month out of range");
    }
    if hour > 23 || minute > 59 || second > 59 {
        bail!("time out of range");
    }
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year as i64, month, day) {
        bail!("day out of range");
    }
    let seconds = days
        .checked_mul(86_400)
        .and_then(|base| base.checked_add(i64::from(hour * 3_600 + minute * 60 + second)))
        .ok_or_else(|| anyhow!("timestamp is out of range"))?;
    if seconds >= 0 {
        Ok(UNIX_EPOCH + Duration::from_secs(seconds as u64))
    } else {
        Ok(UNIX_EPOCH - Duration::from_secs((-seconds) as u64))
    }
}

fn parse_i32_digits(value: &[u8], field: &str) -> Result<i32> {
    Ok(parse_u32_digits(value, field)? as i32)
}

fn parse_u32_digits(value: &[u8], field: &str) -> Result<u32> {
    if !value.iter().all(|byte| byte.is_ascii_digit()) {
        bail!("{field} contains non-digit characters");
    }
    Ok(value
        .iter()
        .fold(0_u32, |acc, byte| acc * 10 + u32::from(byte - b'0')))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - if month <= 2 { 1 } else { 0 };
    let month = i64::from(month);
    let day_of_year =
        (153 * (month + if month > 2 { -3 } else { 9 }) + 2).div_euclid(5) + i64::from(day) - 1;
    let era = if year >= 0 { year } else { year - 399 }.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_era =
        year_of_era * 365 + year_of_era.div_euclid(4) - year_of_era.div_euclid(100) + day_of_year;
    era * 146_097 + day_of_era - 719_468
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
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::messages::MessageStore;
    use crate::services::supervisor_registration::{read_registration, write_registration};
    use crate::storage::StorageRoot;
    use std::process::Command;
    use tempfile::TempDir;

    fn test_ctx(dir: &TempDir) -> Ctx {
        test_ctx_with_runner(dir, MockRunner::new())
    }

    fn test_ctx_with_runner(dir: &TempDir, runner: MockRunner) -> Ctx {
        let repo_root = dir.path().join("repo");
        let git_common_dir = repo_root.join(".git");
        fs::create_dir_all(&git_common_dir).unwrap();
        Ctx::new_with_options(
            repo_root.clone(),
            repo_root,
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
            crate::context::CtxOptions {
                storage_root: Some(StorageRoot::from_git_common_dir(git_common_dir)),
                ..Default::default()
            },
        )
    }

    fn registration(agent_id: &str, pid: u32) -> Registration {
        Registration {
            agent_id: agent_id.into(),
            pid,
            pid_start_time: process_start_time(pid as i32).unwrap(),
            started_at: "2026-05-22T00:00:00Z".into(),
            started_by: "user".into(),
            cleanup_on_session_end: false,
            target_surface_id: None,
            target_agent_kind: None,
            host_workspace_id: None,
            host_pane_id: None,
            host_surface_id: None,
            stale_threshold_secs: 900,
            poll_interval_secs: 60,
            log_path: PathBuf::from("/tmp/supervisor.log"),
        }
    }

    fn spawn_term_ignoring_child() -> std::process::Child {
        Command::new("sh")
            .arg("-c")
            .arg("trap '' TERM; while :; do sleep 1; done")
            .spawn()
            .unwrap()
    }

    #[test]
    fn parses_human_durations() {
        assert_eq!(parse_duration("90s").unwrap(), 90);
        assert_eq!(parse_duration("5m").unwrap(), 300);
        assert_eq!(parse_duration("1h").unwrap(), 3600);
    }

    #[test]
    fn rejects_invalid_durations() {
        assert!(parse_duration("m").is_err());
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("10d").is_err());
    }

    #[test]
    fn stale_gate_leaves_fresh_message_in_new() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "fresh")
            .unwrap();
        rewrite_message_created_at(&sent.path, &current_utc_timestamp());
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert!(sent.path.exists());
        assert!(!inbox_state_dir(&ctx, "codex", "delivered").exists());
    }

    #[test]
    fn next_wait_duration_wakes_when_fresh_message_reaches_stale_threshold() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "fresh")
            .unwrap();
        rewrite_message_created_at(&sent.path, &current_utc_timestamp());
        let config = SupervisorLoopConfig {
            poll_interval_secs: 60,
            stale_threshold_secs: 2,
            ..loop_config(&ctx, 2, 10)
        };

        let wait = next_wait_duration(&ctx, "agents/codex", &config).unwrap();

        assert!(wait <= Duration::from_secs(2));
        assert!(wait >= Duration::from_millis(100));
    }

    #[test]
    fn stale_message_is_claimed_pushed_and_delivered() {
        let dir = TempDir::new().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"surfaces":[{"surface":"surface:4","env":{"CMUX_AGENT_LAUNCH_KIND":"codex"}}]}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let ctx = test_ctx_with_runner(&dir, runner);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "stale")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert!(!sent.path.exists());
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "delivered")).len(),
            1
        );
        assert_eq!(state.pushes_total, 1);
    }

    #[test]
    fn terminal_marker_moves_to_delivered_without_push() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "[done] finished")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert!(!sent.path.exists());
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "delivered")).len(),
            1
        );
        assert_eq!(state.pushes_total, 0);
    }

    #[test]
    fn missing_new_file_during_claim_is_treated_as_race() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "claimed by hook")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        fs::remove_file(&sent.path).unwrap();

        let lease = MessageLease::new(Duration::from_secs(CLAIM_LEASE_SECS)).unwrap();
        let claimed = store
            .claim_new_path("agents/codex", &sent.path, "agents/supervisor", lease)
            .unwrap();

        assert!(claimed.is_none());
    }

    #[test]
    fn terminal_marker_inside_user_text_is_not_control_signal() {
        let dir = TempDir::new().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"surfaces":[{"surface":"surface:4","env":{"CMUX_AGENT_LAUNCH_KIND":"codex"}}]}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let ctx = test_ctx_with_runner(&dir, runner);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from(
                "agents/claude",
                "agents/codex",
                "please include [done] later",
            )
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "delivered")).len(),
            1
        );
        assert_eq!(state.pushes_total, 1);
    }

    #[test]
    fn malformed_created_at_moves_to_failed() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "bad timestamp")
            .unwrap();
        rewrite_message_created_at(&sent.path, "not-a-timestamp");
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert!(!sent.path.exists());
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "failed")).len(),
            1
        );
    }

    #[test]
    fn invalid_inspection_record_moves_to_failed_before_claim() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "wrong state")
            .unwrap();
        rewrite_message_delivery_state(&sent.path, "retry");
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert!(!sent.path.exists());
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "failed")).len(),
            1
        );
    }

    #[test]
    fn cycle_cap_limits_stale_messages_per_poll() {
        let dir = TempDir::new().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"surfaces":[]}"#, true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let ctx = test_ctx_with_runner(&dir, runner);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let first = store
            .send_from("agents/claude", "agents/codex", "first")
            .unwrap();
        let second = store
            .send_from("agents/claude", "agents/codex", "second")
            .unwrap();
        rewrite_message_created_at(&first.path, "1970-01-01T00:00:01Z");
        rewrite_message_created_at(&second.path, "1970-01-01T00:00:02Z");
        let mut config = loop_config(&ctx, 900, 1);
        config.fallback_kind = PushKind::Claude;
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "delivered")).len(),
            1
        );
        assert_eq!(toml_files(&inbox_state_dir(&ctx, "codex", "new")).len(), 1);
    }

    #[test]
    fn push_failure_moves_to_retry() {
        let dir = TempDir::new().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"surfaces":[]}"#, true);
        runner.add_response_with_stderr(
            "",
            "Failed to write to socket (Broken pipe, errno 32)",
            false,
        );
        let ctx = test_ctx_with_runner(&dir, runner);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "retry me")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        let mut config = loop_config(&ctx, 900, 10);
        config.fallback_kind = PushKind::Claude;
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "retry")).len(),
            1
        );
    }

    #[test]
    fn retry_message_is_pushed_and_delivered_on_later_cycle() {
        let dir = TempDir::new().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"surfaces":[]}"#, true);
        runner.add_response_with_stderr(
            "",
            "Failed to write to socket (Broken pipe, errno 32)",
            false,
        );
        runner.add_response(r#"{"surfaces":[]}"#, true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        let ctx = test_ctx_with_runner(&dir, runner);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "retry later")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        let mut config = loop_config(&ctx, 900, 10);
        config.fallback_kind = PushKind::Claude;
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "retry")).len(),
            1
        );

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "retry")).len(),
            0
        );
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "delivered")).len(),
            1
        );
        assert_eq!(state.pushes_total, 1);
    }

    #[test]
    fn expired_claim_is_reclaimed_and_processed() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "[done] after lease")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        let lease = MessageLease::new(Duration::from_secs(CLAIM_LEASE_SECS)).unwrap();
        let claimed = store
            .claim_new_path("agents/codex", &sent.path, "agents/supervisor", lease)
            .unwrap()
            .unwrap();
        rewrite_message_lease_expires_at(&claimed.claimed_path, "1970-01-01T00:00:02Z");
        let config = loop_config(&ctx, 900, 10);
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "claimed")).len(),
            0
        );
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "retry")).len(),
            0
        );
        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "delivered")).len(),
            1
        );
    }

    #[test]
    fn push_failure_after_max_attempts_moves_to_failed() {
        let dir = TempDir::new().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"surfaces":[]}"#, true);
        runner.add_response_with_stderr("", "surface invalid", false);
        let ctx = test_ctx_with_runner(&dir, runner);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from("agents/claude", "agents/codex", "fail me")
            .unwrap();
        rewrite_message_created_at(&sent.path, "1970-01-01T00:00:01Z");
        rewrite_message_attempts(&sent.path, MAX_PUSH_ATTEMPTS - 1);
        let mut config = loop_config(&ctx, 900, 10);
        config.fallback_kind = PushKind::Claude;
        let mut state = SupervisorLoopState::default();

        run_one_cycle(&ctx, "agents/codex", &config, &mut state).unwrap();

        assert_eq!(
            toml_files(&inbox_state_dir(&ctx, "codex", "failed")).len(),
            1
        );
    }

    #[test]
    fn render_payload_is_ascii_and_metadata_only_when_over_cap() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let sent = store
            .send_from(
                "agents/claude",
                "agents/codex",
                &format!("안녕 {}", "x".repeat(300)),
            )
            .unwrap();

        let payload = render_payload(&sent.message, 96);

        assert!(payload.is_ascii());
        assert!(payload.len() <= 96);
        assert!(payload.contains(&sent.id));
    }

    #[test]
    fn surface_supervisor_start_requires_cmux() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let options = StartOptions {
            replace: false,
            surface: Some("surface:72".into()),
            kind: Some("codex".into()),
            cleanup_on_session_end: Some(false),
            stale_threshold: "15m".into(),
            poll_interval: "60s".into(),
        };

        let err = spawn_supervisor_run(
            &ctx,
            "agents/codex",
            &ctx.storage_root
                .runtime_agent_dir(&AgentId::parse("agents/codex").unwrap())
                .join("supervisor.log"),
            &options,
            900,
            60,
            false,
        )
        .unwrap_err();

        assert!(err.to_string().contains("require cmux"));
    }

    #[test]
    fn stop_registration_preserves_concurrent_replacement() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(&dir);
        let mut old_child = spawn_term_ignoring_child();
        let mut new_child = spawn_term_ignoring_child();
        let old_registration = registration("agents/codex", old_child.id());
        let new_registration = registration("agents/codex", new_child.id());
        write_registration(&ctx, &old_registration).unwrap();

        let replacement_ctx = test_ctx(&dir);
        let replacement = new_registration.clone();
        let replacement_thread = std::thread::spawn(move || {
            std::thread::sleep(STOP_POLL * 2);
            write_registration(&replacement_ctx, &replacement).unwrap();
        });

        stop_registration(&ctx, &old_registration).unwrap();
        replacement_thread.join().unwrap();

        assert_eq!(
            read_registration(&ctx, "agents/codex").unwrap(),
            Some(new_registration)
        );

        let _ = signal::kill(Pid::from_raw(old_child.id() as i32), Signal::SIGKILL);
        let _ = old_child.wait();
        let _ = signal::kill(Pid::from_raw(new_child.id() as i32), Signal::SIGKILL);
        let _ = new_child.wait();
    }

    fn loop_config(ctx: &Ctx, stale_threshold_secs: u64, cycle_cap: usize) -> SupervisorLoopConfig {
        SupervisorLoopConfig {
            target_surface_id: Some("surface:4".into()),
            fallback_kind: PushKind::Unknown,
            stale_threshold_secs,
            poll_interval_secs: 1,
            cycle_cap,
            payload_cap: DEFAULT_PAYLOAD_CAP_BYTES,
            log_path: ctx
                .storage_root
                .runtime_agent_dir(&AgentId::parse("agents/codex").unwrap())
                .join("supervisor.log"),
        }
    }

    fn inbox_state_dir(ctx: &Ctx, agent_name: &str, state: &str) -> PathBuf {
        ctx.storage_root
            .runtime_dir()
            .join("agents")
            .join(agent_name)
            .join("inbox")
            .join(state)
    }

    fn toml_files(dir: &Path) -> Vec<PathBuf> {
        let mut paths = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|entry| {
                    let path = entry.ok()?.path();
                    (path.extension().and_then(|ext| ext.to_str()) == Some("toml")).then_some(path)
                })
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        paths.sort();
        paths
    }

    fn rewrite_message_created_at(path: &Path, created_at: &str) {
        let content = fs::read_to_string(path).unwrap();
        let mut value: toml::Value = toml::from_str(&content).unwrap();
        value["meta"]["created_at"] = toml::Value::String(created_at.into());
        fs::write(path, toml::to_string_pretty(&value).unwrap()).unwrap();
    }

    fn rewrite_message_attempts(path: &Path, attempts: u32) {
        let content = fs::read_to_string(path).unwrap();
        let mut value: toml::Value = toml::from_str(&content).unwrap();
        value["delivery"]["attempts"] = toml::Value::Integer(i64::from(attempts));
        fs::write(path, toml::to_string_pretty(&value).unwrap()).unwrap();
    }

    fn rewrite_message_lease_expires_at(path: &Path, lease_expires_at: &str) {
        let content = fs::read_to_string(path).unwrap();
        let mut value: toml::Value = toml::from_str(&content).unwrap();
        value["delivery"]["lease_expires_at"] = toml::Value::String(lease_expires_at.into());
        fs::write(path, toml::to_string_pretty(&value).unwrap()).unwrap();
    }

    fn rewrite_message_delivery_state(path: &Path, state: &str) {
        let content = fs::read_to_string(path).unwrap();
        let mut value: toml::Value = toml::from_str(&content).unwrap();
        value["delivery"]["state"] = toml::Value::String(state.into());
        fs::write(path, toml::to_string_pretty(&value).unwrap()).unwrap();
    }
}
