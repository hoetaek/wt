use super::{AgentKind, AgentObservation, WorkState};

const STATUS_KEY: &str = "codex";

pub(crate) fn classify(observation: &AgentObservation<'_>) -> Option<WorkState> {
    let has_status = super::has_status_signal(STATUS_KEY, observation);
    let has_screen_marker = screen_has_codex_ui_marker(observation.screen);
    if !has_status && !has_screen_marker {
        return None;
    }

    let mut state = super::state_from_agent_signals(AgentKind::Codex, STATUS_KEY, observation);
    if has_screen_marker && !has_status && !super::has_hook_signal(observation) {
        state = state
            .with_warning("codex_hooks_missing")
            .with_metadata("codex_hooks", "missing_or_inactive");
    }
    Some(state)
}

pub(crate) fn screen_has_codex_ui_marker(screen: Option<&str>) -> bool {
    let Some(screen) = screen else {
        return false;
    };
    let lower = screen.to_ascii_lowercase();
    lower.lines().any(|line| {
        screen_has_codex_literal_marker(line)
            || screen_has_codex_model_status_line(line)
            || screen_has_codex_modern_header(line)
    })
}

fn screen_has_codex_literal_marker(line: &str) -> bool {
    let line = line.trim();
    let Some((first, rest)) = line.split_once(char::is_whitespace) else {
        return false;
    };
    if trim_token(first) != "codex" {
        return false;
    }
    rest.split_whitespace()
        .any(|token| is_codex_status_token(token) || is_version_token(token))
}

fn screen_has_codex_model_status_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("gpt-") && line.split_whitespace().any(is_codex_status_token)
}

fn screen_has_codex_modern_header(line: &str) -> bool {
    let tokens = line.split_whitespace().map(trim_token).collect::<Vec<_>>();
    tokens.iter().any(|token| is_gpt_model_token(token))
        && tokens.iter().any(|token| is_reasoning_effort_token(token))
        && tokens.contains(&"context")
        && tokens.contains(&"left")
}

fn is_gpt_model_token(token: &str) -> bool {
    let Some(model) = token.strip_prefix("gpt-") else {
        return false;
    };
    model.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn is_reasoning_effort_token(token: &str) -> bool {
    matches!(token, "low" | "medium" | "high" | "xhigh")
}

fn is_codex_status_token(token: &str) -> bool {
    matches!(
        trim_token(token),
        "working"
            | "running"
            | "thinking"
            | "exploring"
            | "ready"
            | "idle"
            | "failed"
            | "failure"
            | "fatal"
            | "error"
            | "permission"
    )
}

fn is_version_token(token: &str) -> bool {
    let token = trim_token(token);
    let Some(version) = token.strip_prefix('v') else {
        return false;
    };
    version.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn trim_token(token: &str) -> &str {
    token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_codex_header_is_ui_marker() {
        let screen = "remove-task-run-source . gpt-5.5 xhigh . Context 94% left . 5h 91%";

        assert!(screen_has_codex_ui_marker(Some(screen)));
    }

    #[test]
    fn generic_gpt_model_note_is_not_ui_marker() {
        let screen = "notes about gpt-5.5 model behavior";

        assert!(!screen_has_codex_ui_marker(Some(screen)));
    }
}
