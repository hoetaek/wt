use super::{AgentKind, AgentObservation, AgentStatus, WorkState};

pub(crate) fn classify(observation: &AgentObservation<'_>) -> Option<WorkState> {
    let screen = observation.screen?;
    if screen.trim().is_empty() {
        return None;
    }
    if screen_has_prompt(screen) {
        return Some(super::enrich_state(
            WorkState::new(AgentKind::Shell, AgentStatus::Idle),
            observation,
        ));
    }

    if super::screen_status(screen).is_some_and(|status| status == AgentStatus::Running) {
        return Some(super::enrich_state(
            WorkState::new(AgentKind::Shell, AgentStatus::Running),
            observation,
        ));
    }

    None
}

fn screen_has_prompt(screen: &str) -> bool {
    let Some(line) = screen.lines().rev().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let line = line.trim_end();
    line.ends_with('$') || line.ends_with('%') || line.ends_with('#') || line.ends_with('>')
}
