use super::{AgentKind, AgentObservation, WorkState};

const STATUS_KEY: &str = "claude_code";

pub(crate) fn classify(observation: &AgentObservation<'_>) -> Option<WorkState> {
    if !super::has_status_signal(STATUS_KEY, observation)
        && !super::screen_mentions(observation.screen, "claude code")
    {
        return None;
    }

    Some(super::state_from_agent_signals(
        AgentKind::ClaudeCode,
        STATUS_KEY,
        observation,
    ))
}
