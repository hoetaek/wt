use super::{AgentKind, AgentObservation, WorkState};

const STATUS_KEY: &str = "claude_code";

pub(crate) fn classify(observation: &AgentObservation<'_>) -> Option<WorkState> {
    if !super::has_status_signal(STATUS_KEY, observation)
        && !screen_has_claude_ui_marker(observation.screen)
    {
        return None;
    }

    Some(super::state_from_agent_signals(
        AgentKind::ClaudeCode,
        STATUS_KEY,
        observation,
    ))
}

pub(crate) fn screen_has_claude_ui_marker(screen: Option<&str>) -> bool {
    let Some(screen) = screen else {
        return false;
    };
    screen.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line == "claude code"
            || line.starts_with("claude code ")
            || line.starts_with("claude-code ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_header_is_ui_marker() {
        assert!(screen_has_claude_ui_marker(Some("Claude Code\nWorking")));
    }

    #[test]
    fn claude_code_prose_is_not_ui_marker() {
        assert!(!screen_has_claude_ui_marker(Some(
            "Task says Claude Code should inspect files"
        )));
    }
}
