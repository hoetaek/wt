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
    let mut tokens = line
        .split_whitespace()
        .map(trim_token)
        .filter(|token| !token.is_empty());
    let Some(first) = tokens.next() else {
        return false;
    };
    if trim_token(first) != "codex" {
        return false;
    }
    let Some(second) = tokens.next() else {
        return false;
    };
    is_codex_status_token(second)
        || is_version_token(second)
        || (second == "cli" && tokens.next().is_some_and(is_version_token))
}

fn screen_has_codex_model_status_line(line: &str) -> bool {
    let line = line.trim();
    let tokens = line
        .split_whitespace()
        .map(trim_token)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let Some(first) = tokens.first() else {
        return false;
    };
    if !is_gpt_model_token(first) {
        return false;
    }
    let Some(status_index) = tokens.iter().position(|token| is_codex_status_token(token)) else {
        return false;
    };
    status_index == 1
        || tokens.iter().any(|token| is_reasoning_effort_token(token))
        || (tokens.contains(&"context") && tokens.contains(&"left"))
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
            | "starting"
            | "waiting"
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
    fn codex_waiting_footer_is_ui_marker() {
        let screen = "gpt-5.5 xhigh . ~/dev/tools/wt . develop . Waiting . 5h 87% . weekly 65%";

        assert!(screen_has_codex_ui_marker(Some(screen)));
    }

    #[test]
    fn codex_starting_footer_is_ui_marker() {
        let screen = "gpt-5.5 xhigh . ~/dev/tools/wt . develop . Starting . 5h 87% . weekly 65%";

        assert!(screen_has_codex_ui_marker(Some(screen)));
    }

    #[test]
    fn generic_gpt_model_note_is_not_ui_marker() {
        let screen = "notes about gpt-5.5 model behavior";

        assert!(!screen_has_codex_ui_marker(Some(screen)));
    }

    #[test]
    fn generic_gpt_or_codex_waiting_prose_is_not_ui_marker() {
        let screen = "gpt-5.5 is Waiting in this note\nCodex is Waiting for GPT output";

        assert!(!screen_has_codex_ui_marker(Some(screen)));
    }
}
