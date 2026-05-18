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
        let lowered = line.trim().to_ascii_lowercase();
        let normalized = lowered.trim_start_matches(|c: char| !c.is_alphanumeric());
        normalized == "claude code"
            || normalized.starts_with("claude code ")
            || normalized.starts_with("claude-code ")
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

    #[test]
    fn surface_title_glyph_decorated_header_is_ui_marker() {
        assert!(screen_has_claude_ui_marker(Some("✳ Claude Code")));
    }

    #[test]
    fn banner_decorated_header_is_ui_marker() {
        assert!(screen_has_claude_ui_marker(Some(
            "▐▛███▜▌   Claude Code v9.99.0"
        )));
    }

    #[test]
    fn non_english_prose_prefix_is_not_ui_marker() {
        assert!(!screen_has_claude_ui_marker(Some(
            "작업: Claude Code should inspect files"
        )));
    }
}
