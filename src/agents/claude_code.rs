use super::{AgentKind, AgentObservation, WorkState};

const STATUS_KEY: &str = "claude_code";

const PERMISSION_FOOTER_GLYPH: &str = "\u{23F5}\u{23F5}";

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
    screen.lines().any(line_has_claude_permission_footer)
}

fn line_has_claude_permission_footer(line: &str) -> bool {
    line.contains(PERMISSION_FOOTER_GLYPH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_permissions_footer_is_ui_marker() {
        assert!(screen_has_claude_ui_marker(Some(
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)"
        )));
    }

    #[test]
    fn accept_edits_footer_is_ui_marker() {
        assert!(screen_has_claude_ui_marker(Some("⏵⏵ accept edits on")));
    }

    #[test]
    fn claude_code_prose_without_footer_is_not_ui_marker() {
        assert!(!screen_has_claude_ui_marker(Some(
            "Task says Claude Code should inspect files"
        )));
    }

    #[test]
    fn non_english_prose_without_footer_is_not_ui_marker() {
        assert!(!screen_has_claude_ui_marker(Some(
            "작업: Claude Code should inspect files"
        )));
    }

    #[test]
    fn welcome_banner_alone_is_not_ui_marker() {
        assert!(!screen_has_claude_ui_marker(Some(
            "▐▛███▜▌   Claude Code v9.99.0"
        )));
    }
}
