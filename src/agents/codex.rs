use super::{AgentKind, AgentObservation, WorkState};

const STATUS_KEY: &str = "codex";

pub(crate) fn classify(observation: &AgentObservation<'_>) -> Option<WorkState> {
    let has_status = super::has_status_signal(STATUS_KEY, observation);
    let screen_mentions_codex = super::screen_mentions(observation.screen, "codex");
    if !has_status && !screen_mentions_codex {
        return None;
    }

    let mut state = super::state_from_agent_signals(AgentKind::Codex, STATUS_KEY, observation);
    if screen_mentions_codex && !has_status && !super::has_hook_signal(observation) {
        state = state
            .with_warning("codex_hooks_missing")
            .with_metadata("codex_hooks", "missing_or_inactive");
    }
    Some(state)
}
