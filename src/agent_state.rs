use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const WAIT_OBSERVATIONS_FILE: &str = "wait-observations.jsonl";
pub(crate) const NON_IDLE_WAIT_CLASS: &str = "non_idle";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WaitObservation {
    pub(crate) recorded_at: String,
    pub(crate) wait_class: String,
    pub(crate) wait_reason: String,
    pub(crate) wait_seconds: u64,
    pub(crate) bound_seconds: u64,
    pub(crate) target: String,
    pub(crate) branch: String,
    pub(crate) worktree: Option<String>,
    pub(crate) task_run_id: Option<String>,
    pub(crate) agent_kind: String,
    pub(crate) agent_state: String,
    pub(crate) last_tool: Option<String>,
    pub(crate) last_event_at: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) cmux_workspace: Option<String>,
    pub(crate) cmux_surface: Option<String>,
}

impl WaitObservation {
    pub(crate) fn new_non_idle(
        wait_reason: impl Into<String>,
        wait_seconds: u64,
        bound_seconds: u64,
        target: impl Into<String>,
        branch: impl Into<String>,
        agent_kind: impl Into<String>,
        agent_state: impl Into<String>,
    ) -> Self {
        Self {
            recorded_at: current_utc_timestamp(),
            wait_class: NON_IDLE_WAIT_CLASS.into(),
            wait_reason: wait_reason.into(),
            wait_seconds,
            bound_seconds,
            target: target.into(),
            branch: branch.into(),
            worktree: None,
            task_run_id: None,
            agent_kind: agent_kind.into(),
            agent_state: agent_state.into(),
            last_tool: None,
            last_event_at: None,
            session_id: None,
            cmux_workspace: None,
            cmux_surface: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WaitObservationSummary {
    pub(crate) path: String,
    pub(crate) count: u64,
    pub(crate) sum_seconds: u64,
    pub(crate) min_seconds: Option<u64>,
    pub(crate) max_seconds: Option<u64>,
    pub(crate) buckets: BTreeMap<String, u64>,
}

impl WaitObservationSummary {
    fn empty(path: &Path) -> Self {
        Self {
            path: path.display().to_string(),
            count: 0,
            sum_seconds: 0,
            min_seconds: None,
            max_seconds: None,
            buckets: BTreeMap::new(),
        }
    }

    fn record(&mut self, observation: &WaitObservation) {
        if observation.wait_class != NON_IDLE_WAIT_CLASS {
            return;
        }

        self.count += 1;
        self.sum_seconds += observation.wait_seconds;
        self.min_seconds = Some(match self.min_seconds {
            Some(current) => current.min(observation.wait_seconds),
            None => observation.wait_seconds,
        });
        self.max_seconds = Some(match self.max_seconds {
            Some(current) => current.max(observation.wait_seconds),
            None => observation.wait_seconds,
        });
        *self
            .buckets
            .entry(bucket_for(observation.wait_seconds).into())
            .or_insert(0) += 1;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WaitObservationStore {
    path: PathBuf,
}

impl WaitObservationStore {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn append(&self, observation: &WaitObservation) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create agent state directory: {}",
                    parent.display()
                )
            })?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| {
                format!(
                    "Failed to open wait observations file: {}",
                    self.path.display()
                )
            })?;
        serde_json::to_writer(&mut file, observation)
            .context("Failed to serialize wait observation")?;
        writeln!(file).with_context(|| {
            format!("Failed to append wait observation: {}", self.path.display())
        })?;
        Ok(())
    }

    pub(crate) fn summary(&self) -> Result<WaitObservationSummary> {
        let content = match fs::read_to_string(&self.path) {
            Ok(content) => content,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Ok(WaitObservationSummary::empty(&self.path));
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to read wait observations file: {}",
                        self.path.display()
                    )
                });
            }
        };

        let mut summary = WaitObservationSummary::empty(&self.path);
        for (index, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let observation = serde_json::from_str::<WaitObservation>(line).with_context(|| {
                format!(
                    "Failed to parse wait observation line {} in {}",
                    index + 1,
                    self.path.display()
                )
            })?;
            summary.record(&observation);
        }
        Ok(summary)
    }
}

fn bucket_for(seconds: u64) -> &'static str {
    match seconds {
        0..=59 => "0-59s",
        60..=299 => "1-4m",
        300..=899 => "5-14m",
        900..=3_599 => "15-59m",
        _ => "1h+",
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
