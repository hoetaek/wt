pub(crate) mod claude_code;
pub(crate) mod codex;
pub(crate) mod shell;

use crate::services::cmux::{CmuxEvent, CmuxStatusEntry};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentKind {
    ClaudeCode,
    Codex,
    Shell,
    Unknown,
}

impl AgentKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Shell => "shell",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentStatus {
    Running,
    Idle,
    NeedsInput,
    Completed,
    Failed,
    NoSession,
    Unknown,
}

impl AgentStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Idle => "idle",
            Self::NeedsInput => "needs_input",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::NoSession => "no_session",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WorkState {
    pub(crate) agent_kind: AgentKind,
    pub(crate) status: AgentStatus,
    pub(crate) last_tool: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) last_event_at: Option<String>,
    pub(crate) needs_input_since: Option<String>,
    pub(crate) warning: Option<String>,
    pub(crate) metadata: BTreeMap<String, String>,
}

impl WorkState {
    pub(crate) fn new(agent_kind: AgentKind, status: AgentStatus) -> Self {
        Self {
            agent_kind,
            status,
            last_tool: None,
            session_id: None,
            last_event_at: None,
            needs_input_since: None,
            warning: None,
            metadata: BTreeMap::new(),
        }
    }

    pub(crate) fn no_session(agent_kind: AgentKind) -> Self {
        Self::new(agent_kind, AgentStatus::NoSession)
    }

    pub(crate) fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warning = Some(warning.into());
        self
    }

    pub(crate) fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AgentObservation<'a> {
    pub(crate) screen: Option<&'a str>,
    pub(crate) statuses: &'a [CmuxStatusEntry],
    pub(crate) events: &'a [CmuxEvent],
    pub(crate) process_agent_kind: Option<AgentKind>,
}

impl<'a> AgentObservation<'a> {
    pub(crate) fn new(
        screen: Option<&'a str>,
        statuses: &'a [CmuxStatusEntry],
        events: &'a [CmuxEvent],
    ) -> Self {
        Self {
            screen,
            statuses,
            events,
            process_agent_kind: None,
        }
    }

    pub(crate) fn with_process_agent_kind(mut self, agent_kind: Option<AgentKind>) -> Self {
        self.process_agent_kind = agent_kind;
        self
    }
}

pub(crate) fn classify(observation: &AgentObservation<'_>) -> WorkState {
    if let Some(agent_kind) = observation.process_agent_kind {
        return state_from_process_identity(agent_kind, observation);
    }
    if let Some((kind, status_key)) = current_known_agent_status(observation) {
        return state_from_agent_signals(kind, status_key, observation);
    }
    if let Some((kind, status_key)) = latest_known_agent_sidebar_status(observation) {
        return state_from_agent_signals(kind, status_key, observation);
    }
    if let Some(state) = claude_code::classify(observation) {
        return state;
    }
    if let Some(state) = codex::classify(observation) {
        return state;
    }
    if let Some(state) = shell::classify(observation) {
        return state;
    }

    let status = match observation.screen {
        Some(screen) if screen.trim().is_empty() => AgentStatus::NoSession,
        None => AgentStatus::NoSession,
        Some(screen) => screen_status(screen).unwrap_or(AgentStatus::Unknown),
    };
    enrich_state(WorkState::new(AgentKind::Unknown, status), observation)
}

fn state_from_process_identity(
    agent_kind: AgentKind,
    observation: &AgentObservation<'_>,
) -> WorkState {
    let Some(status_key) = status_key_for_agent(agent_kind) else {
        return enrich_state(
            WorkState::new(agent_kind, AgentStatus::Unknown),
            observation,
        );
    };

    let mut state = state_from_agent_signals(agent_kind, status_key, observation)
        .with_metadata("agent_identity", "process_tree");
    if !has_status_signal(status_key, observation) && !has_hook_signal(observation) {
        state = match agent_kind {
            AgentKind::Codex => state
                .with_warning("codex_hooks_missing")
                .with_metadata("codex_hooks", "missing_or_inactive"),
            AgentKind::ClaudeCode => state.with_warning("agent_status_signals_missing"),
            AgentKind::Shell | AgentKind::Unknown => state,
        };
    }
    state
}

pub(super) fn state_from_agent_signals(
    agent_kind: AgentKind,
    status_key: &str,
    observation: &AgentObservation<'_>,
) -> WorkState {
    let status = current_status(status_key, observation.statuses)
        .or_else(|| latest_sidebar_status(status_key, observation.events))
        .or_else(|| latest_hook_status(observation.events))
        .or_else(|| observation.screen.and_then(screen_status))
        .unwrap_or(AgentStatus::Unknown);
    enrich_state(WorkState::new(agent_kind, status), observation)
}

pub(super) fn has_status_signal(status_key: &str, observation: &AgentObservation<'_>) -> bool {
    observation
        .statuses
        .iter()
        .any(|entry| entry.key == status_key)
        || observation
            .events
            .iter()
            .any(|event| sidebar_status_command(event).is_some_and(|(key, _)| key == status_key))
}

pub(super) fn has_hook_signal(observation: &AgentObservation<'_>) -> bool {
    observation
        .events
        .iter()
        .any(|event| hook_name(event).is_some())
}

pub(super) fn enrich_state(mut state: WorkState, observation: &AgentObservation<'_>) -> WorkState {
    state.last_tool = latest_tool_name(observation.events);
    state.session_id = latest_string_payload(observation.events, "session_id");
    state.last_event_at = latest_relevant_event_at(observation.events);
    if state.status == AgentStatus::NeedsInput && state.needs_input_since.is_none() {
        state.needs_input_since = latest_needs_input_at(observation.events);
    }
    state
}

fn current_known_agent_status(
    observation: &AgentObservation<'_>,
) -> Option<(AgentKind, &'static str)> {
    observation
        .statuses
        .iter()
        .find_map(|entry| known_status_key(&entry.key))
}

fn latest_known_agent_sidebar_status(
    observation: &AgentObservation<'_>,
) -> Option<(AgentKind, &'static str)> {
    observation
        .events
        .iter()
        .filter_map(|event| {
            let (key, _) = sidebar_status_command(event)?;
            known_status_key(&key).map(|known| (event.seq, known))
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, known)| known)
}

fn known_status_key(key: &str) -> Option<(AgentKind, &'static str)> {
    match key {
        "claude_code" => Some((AgentKind::ClaudeCode, "claude_code")),
        "codex" => Some((AgentKind::Codex, "codex")),
        _ => None,
    }
}

fn status_key_for_agent(agent_kind: AgentKind) -> Option<&'static str> {
    match agent_kind {
        AgentKind::ClaudeCode => Some("claude_code"),
        AgentKind::Codex => Some("codex"),
        AgentKind::Shell | AgentKind::Unknown => None,
    }
}

fn current_status(status_key: &str, statuses: &[CmuxStatusEntry]) -> Option<AgentStatus> {
    statuses
        .iter()
        .find(|entry| entry.key == status_key)
        .and_then(|entry| normalize_status(&entry.value))
}

fn latest_sidebar_status(status_key: &str, events: &[CmuxEvent]) -> Option<AgentStatus> {
    events
        .iter()
        .filter_map(|event| {
            let (key, value) = sidebar_status_command(event)?;
            (key == status_key).then_some((event.seq, value))
        })
        .max_by_key(|(seq, _)| *seq)
        .and_then(|(_, value)| normalize_status(&value))
}

fn latest_hook_status(events: &[CmuxEvent]) -> Option<AgentStatus> {
    events
        .iter()
        .filter_map(|event| {
            let name = hook_name(event)?;
            let status = match name {
                "PermissionRequest" => AgentStatus::NeedsInput,
                "PreToolUse" | "PostToolUse" | "UserPromptSubmit" | "SessionStart" => {
                    AgentStatus::Running
                }
                "Stop" => AgentStatus::Completed,
                _ => return None,
            };
            Some((event.seq, status))
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, status)| status)
}

pub(super) fn screen_status(screen: &str) -> Option<AgentStatus> {
    let trimmed = screen.trim();
    if trimmed.is_empty() {
        return Some(AgentStatus::NoSession);
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("needs input")
        || lower.contains("waiting for input")
        || (lower.contains("permission") && lower.contains("request"))
        || (lower.contains("approval") && lower.contains("required"))
    {
        return Some(AgentStatus::NeedsInput);
    }
    if lower.contains("failed") || lower.contains("fatal:") {
        return Some(AgentStatus::Failed);
    }
    if lower.contains("working")
        || lower.contains("running")
        || lower.contains("thinking")
        || lower.contains("exploring")
    {
        return Some(AgentStatus::Running);
    }
    if lower.contains("ready") || lower.contains("idle") {
        return Some(AgentStatus::Idle);
    }

    None
}

fn normalize_status(value: &str) -> Option<AgentStatus> {
    match normalize_token(value).as_str() {
        "running" | "working" | "busy" | "thinking" => Some(AgentStatus::Running),
        "idle" | "ready" => Some(AgentStatus::Idle),
        "needs_input" | "needsinput" | "waiting_for_input" | "permissionrequest" => {
            Some(AgentStatus::NeedsInput)
        }
        "completed" | "complete" | "done" | "success" => Some(AgentStatus::Completed),
        "failed" | "failure" | "error" => Some(AgentStatus::Failed),
        "no_session" | "nosession" => Some(AgentStatus::NoSession),
        "unknown" => Some(AgentStatus::Unknown),
        _ => None,
    }
}

fn normalize_token(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn latest_tool_name(events: &[CmuxEvent]) -> Option<String> {
    events
        .iter()
        .filter(|event| hook_name(event).is_some_and(|name| name == "PreToolUse"))
        .filter_map(|event| {
            string_payload(&event.payload, "tool_name").map(|tool| (event.seq, tool))
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, tool)| tool)
}

fn latest_string_payload(events: &[CmuxEvent], key: &str) -> Option<String> {
    events
        .iter()
        .filter_map(|event| string_payload(&event.payload, key).map(|value| (event.seq, value)))
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, value)| value)
}

fn latest_relevant_event_at(events: &[CmuxEvent]) -> Option<String> {
    events
        .iter()
        .filter(|event| hook_name(event).is_some() || sidebar_status_command(event).is_some())
        .filter_map(|event| {
            event
                .occurred_at
                .as_ref()
                .map(|occurred_at| (event.seq, occurred_at))
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, occurred_at)| occurred_at.clone())
}

fn latest_needs_input_at(events: &[CmuxEvent]) -> Option<String> {
    events
        .iter()
        .filter_map(|event| {
            let is_needs_input = hook_name(event).is_some_and(|name| name == "PermissionRequest")
                || sidebar_status_command(event)
                    .and_then(|(_, value)| normalize_status(&value))
                    .is_some_and(|status| status == AgentStatus::NeedsInput);
            if is_needs_input {
                event
                    .occurred_at
                    .as_ref()
                    .map(|occurred_at| (event.seq, occurred_at))
            } else {
                None
            }
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, occurred_at)| occurred_at.clone())
}

fn hook_name(event: &CmuxEvent) -> Option<&str> {
    event
        .name
        .strip_prefix("agent.hook.")
        .or_else(|| event.payload.get("hook_event_name").and_then(Value::as_str))
}

fn sidebar_status_command(event: &CmuxEvent) -> Option<(String, String)> {
    let command = event.payload.get("command").and_then(Value::as_str)?;
    let mut parts = command.split_whitespace();
    if parts.next()? != "set_status" {
        return None;
    }
    let key = parts.next()?.to_string();
    let value = parts.next()?.to_string();
    Some((key, value))
}

fn string_payload(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_claude_running_and_idle_from_sidebar_status() {
        for (raw, expected) in [
            ("Running", AgentStatus::Running),
            ("Idle", AgentStatus::Idle),
        ] {
            let statuses = vec![status("claude_code", raw)];
            let observation = AgentObservation::new(None, &statuses, &[]);

            let state = classify(&observation);

            assert_eq!(state.agent_kind, AgentKind::ClaudeCode);
            assert_eq!(state.status, expected);
        }
    }

    #[test]
    fn classifies_codex_running_and_idle_from_installed_hook_signals() {
        for (raw, expected) in [
            ("Running", AgentStatus::Running),
            ("Idle", AgentStatus::Idle),
        ] {
            let events = vec![event(
                10,
                "sidebar.metadata.updated",
                json!({ "command": format!("set_status codex {raw}") }),
            )];
            let observation = AgentObservation::new(None, &[], &events);

            let state = classify(&observation);

            assert_eq!(state.agent_kind, AgentKind::Codex);
            assert_eq!(state.status, expected);
        }
    }

    #[test]
    fn current_status_wins_over_stale_sidebar_event() {
        let statuses = vec![status("codex", "Idle")];
        let events = vec![event(
            10,
            "sidebar.metadata.updated",
            json!({ "command": "set_status codex Running" }),
        )];
        let observation = AgentObservation::new(Some("Codex v0.130.0\nReady"), &statuses, &events);

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Codex);
        assert_eq!(state.status, AgentStatus::Idle);
    }

    #[test]
    fn current_codex_status_wins_over_claude_screen_text() {
        let statuses = vec![status("codex", "Idle")];
        let observation = AgentObservation::new(
            Some("Task says Claude Code should inspect files.\nCodex Ready"),
            &statuses,
            &[],
        );

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Codex);
        assert_eq!(state.status, AgentStatus::Idle);
    }

    #[test]
    fn current_claude_status_wins_over_codex_screen_text() {
        let statuses = vec![status("claude_code", "Idle")];
        let observation = AgentObservation::new(Some("Codex v0.130.0\nReady"), &statuses, &[]);

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::ClaudeCode);
        assert_eq!(state.status, AgentStatus::Idle);
    }

    #[test]
    fn codex_screen_without_hook_signal_is_not_a_failure() {
        let observation = AgentObservation::new(Some("Codex v0.130.0\nReady"), &[], &[]);

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Codex);
        assert_eq!(state.status, AgentStatus::Idle);
        assert_eq!(state.warning.as_deref(), Some("codex_hooks_missing"));
        assert_eq!(
            state.metadata.get("codex_hooks").map(String::as_str),
            Some("missing_or_inactive")
        );
    }

    #[test]
    fn codex_process_identity_does_not_need_screen_marker() {
        let observation = AgentObservation::new(Some("custom footer text"), &[], &[])
            .with_process_agent_kind(Some(AgentKind::Codex));

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Codex);
        assert_eq!(state.status, AgentStatus::Unknown);
        assert_eq!(state.warning.as_deref(), Some("codex_hooks_missing"));
        assert_eq!(
            state.metadata.get("agent_identity").map(String::as_str),
            Some("process_tree")
        );
    }

    #[test]
    fn claude_process_identity_does_not_need_screen_marker() {
        let observation = AgentObservation::new(Some("custom footer text"), &[], &[])
            .with_process_agent_kind(Some(AgentKind::ClaudeCode));

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::ClaudeCode);
        assert_eq!(state.status, AgentStatus::Unknown);
        assert_eq!(
            state.warning.as_deref(),
            Some("agent_status_signals_missing")
        );
        assert_eq!(
            state.metadata.get("agent_identity").map(String::as_str),
            Some("process_tree")
        );
    }

    #[test]
    fn codex_text_in_non_agent_screen_does_not_classify_as_codex() {
        let observation = AgentObservation::new(
            Some("lazygit\n3fc13dc fix: Codex model screen binding"),
            &[],
            &[],
        );

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Unknown);
        assert_eq!(state.status, AgentStatus::Unknown);
    }

    #[test]
    fn pre_tool_use_preserves_latest_tool_name() {
        let statuses = vec![status("claude_code", "Running")];
        let events = vec![
            event(
                1,
                "agent.hook.PreToolUse",
                json!({ "tool_name": "Read", "session_id": "session-1" }),
            ),
            event(
                2,
                "agent.hook.PreToolUse",
                json!({ "tool_name": "Bash", "session_id": "session-1" }),
            ),
        ];
        let observation = AgentObservation::new(None, &statuses, &events);

        let state = classify(&observation);

        assert_eq!(state.last_tool.as_deref(), Some("Bash"));
        assert_eq!(state.session_id.as_deref(), Some("session-1"));
    }

    #[test]
    fn cold_terminal_is_no_session() {
        let observation = AgentObservation::new(None, &[], &[]);

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Unknown);
        assert_eq!(state.status, AgentStatus::NoSession);
    }

    #[test]
    fn shell_prompt_fallback_is_idle() {
        let observation = AgentObservation::new(Some("~/dev/wt\n$"), &[], &[]);

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Shell);
        assert_eq!(state.status, AgentStatus::Idle);
    }

    #[test]
    fn shell_running_fallback_uses_terminal_text_without_agent_signals() {
        let observation = AgentObservation::new(Some("Running cargo test\n"), &[], &[]);

        let state = classify(&observation);

        assert_eq!(state.agent_kind, AgentKind::Shell);
        assert_eq!(state.status, AgentStatus::Running);
    }

    fn status(key: &str, value: &str) -> CmuxStatusEntry {
        CmuxStatusEntry {
            key: key.into(),
            value: value.into(),
            icon: None,
            color: None,
        }
    }

    fn event(seq: u64, name: &str, payload: Value) -> CmuxEvent {
        CmuxEvent {
            seq,
            name: name.into(),
            category: None,
            occurred_at: Some(format!("2026-05-16T00:00:{seq:02}Z")),
            window_id: None,
            workspace_id: None,
            pane_id: None,
            surface_id: None,
            payload,
        }
    }
}
