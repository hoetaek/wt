use crate::context::{CmdOutput, CommandRunner};
use crate::services::cmux::{CmuxCaller, CmuxService};
use crate::services::cmux::{
    PASTE_SUBMIT_SETTLE, cmux_paste_buffer_args, cmux_send_args, cmux_set_buffer_args,
    unique_cmux_buffer_name,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub const DEFAULT_PAYLOAD_CAP_BYTES: usize = 1024;
pub(crate) const CODEX_IN_PROMPT_NEWLINE_KEY: &str = "shift-enter";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushKind {
    Claude,
    Codex,
    Unknown,
}

impl PushKind {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude_code" | "claude-code" => Self::Claude,
            "codex" => Self::Codex,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxPushTarget {
    pub surface_id: String,
    pub workspace: Option<String>,
    pub kind: PushKind,
}

pub struct CmuxPushService<'a> {
    runner: &'a dyn CommandRunner,
    payload_cap: usize,
}

impl<'a> CmuxPushService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self {
            runner,
            payload_cap: DEFAULT_PAYLOAD_CAP_BYTES,
        }
    }

    pub fn with_payload_cap(mut self, payload_cap: usize) -> Self {
        self.payload_cap = payload_cap;
        self
    }

    pub fn detect_target_kind(&self, surface_id: &str) -> Result<PushKind> {
        detect_target_kind(self.runner, surface_id)
    }

    pub fn detect_target(&self, surface_id: &str) -> Result<CmuxPushTarget> {
        detect_target(self.runner, surface_id)
    }

    pub fn push_to_surface(&self, surface_id: &str, kind: PushKind, text: &str) -> Result<()> {
        validate_payload(text, self.payload_cap)?;
        push_to_surface_unchecked(self.runner, surface_id, kind, text)
    }

    pub fn push_to_surface_in_workspace(
        &self,
        surface_id: &str,
        workspace: Option<&str>,
        kind: PushKind,
        text: &str,
    ) -> Result<()> {
        validate_payload(text, self.payload_cap)?;
        push_to_surface_in_workspace_unchecked(self.runner, surface_id, workspace, kind, text)
    }

    pub fn submit_to_surface_in_workspace(
        &self,
        surface_id: &str,
        workspace: Option<&str>,
        kind: PushKind,
        text: &str,
    ) -> Result<()> {
        push_to_surface_in_workspace_unchecked(self.runner, surface_id, workspace, kind, text)
    }
}

pub fn detect_target_kind(runner: &dyn CommandRunner, surface_id: &str) -> Result<PushKind> {
    Ok(detect_target(runner, surface_id)?.kind)
}

pub fn detect_target(runner: &dyn CommandRunner, surface_id: &str) -> Result<CmuxPushTarget> {
    let out = runner.run("cmux", &["top", "--all", "--processes", "--json"], None)?;
    if !out.success {
        bail!("cmux top failed: {}", command_error(&out));
    }
    let value: Value =
        serde_json::from_str(&out.stdout).context("Failed to parse cmux top JSON")?;
    Ok(
        find_surface_target(&value, surface_id, None).unwrap_or_else(|| CmuxPushTarget {
            surface_id: surface_id.into(),
            workspace: None,
            kind: PushKind::Unknown,
        }),
    )
}

pub fn push_to_surface(
    runner: &dyn CommandRunner,
    surface_id: &str,
    kind: PushKind,
    text: &str,
) -> Result<()> {
    validate_payload(text, DEFAULT_PAYLOAD_CAP_BYTES)?;
    push_to_surface_unchecked(runner, surface_id, kind, text)
}

fn push_to_surface_unchecked(
    runner: &dyn CommandRunner,
    surface_id: &str,
    kind: PushKind,
    text: &str,
) -> Result<()> {
    push_to_surface_in_workspace_unchecked(runner, surface_id, None, kind, text)
}

fn push_to_surface_in_workspace_unchecked(
    runner: &dyn CommandRunner,
    surface_id: &str,
    workspace: Option<&str>,
    kind: PushKind,
    text: &str,
) -> Result<()> {
    match kind {
        PushKind::Codex => push_codex_prompt(runner, surface_id, workspace, text),
        PushKind::Claude => push_pasted_prompt(runner, surface_id, workspace, "wt-claude", text),
        PushKind::Unknown => {
            bail!(
                "Cannot push to cmux surface {surface_id}: target agent kind is unknown. Pass --kind claude or --kind codex."
            )
        }
    }
}

fn push_codex_prompt(
    runner: &dyn CommandRunner,
    surface_id: &str,
    workspace: Option<&str>,
    text: &str,
) -> Result<()> {
    submit_codex_prompt(runner, surface_id, workspace, text)
}

pub(crate) fn submit_codex_prompt(
    runner: &dyn CommandRunner,
    surface_id: &str,
    workspace: Option<&str>,
    text: &str,
) -> Result<()> {
    let lines = codex_prompt_lines(text);
    for (i, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            let send_args = cmux_send_args("send", surface_id, workspace, line);
            run_cmux(runner, &send_args, "send")?;
        }
        if i + 1 < lines.len() {
            let newline_args = cmux_send_args(
                "send-key",
                surface_id,
                workspace,
                CODEX_IN_PROMPT_NEWLINE_KEY,
            );
            run_cmux(runner, &newline_args, "send-key")?;
        }
    }

    let enter_args = cmux_send_args("send-key", surface_id, workspace, "enter");
    run_cmux(runner, &enter_args, "send-key")
}

fn push_pasted_prompt(
    runner: &dyn CommandRunner,
    surface_id: &str,
    workspace: Option<&str>,
    buffer_prefix: &str,
    text: &str,
) -> Result<()> {
    submit_pasted_prompt_with_enter(runner, surface_id, workspace, buffer_prefix, text)
}

pub(crate) fn submit_pasted_prompt_with_enter(
    runner: &dyn CommandRunner,
    surface_id: &str,
    workspace: Option<&str>,
    buffer_prefix: &str,
    text: &str,
) -> Result<()> {
    let buffer = unique_cmux_buffer_name(buffer_prefix, surface_id);
    let set_buffer_args = cmux_set_buffer_args(&buffer, text);
    run_cmux(runner, &set_buffer_args, "set-buffer")?;
    let paste_buffer_args = cmux_paste_buffer_args(surface_id, workspace, &buffer);
    run_cmux_paste_buffer_with_retry(runner, surface_id, workspace, &paste_buffer_args)?;
    std::thread::sleep(PASTE_SUBMIT_SETTLE);
    let enter_args = cmux_send_args("send-key", surface_id, workspace, "enter");
    run_cmux(runner, &enter_args, "send-key")
}

/// Retry paste-buffer on transient `Command timed out` errors.
///
/// cmux 0.64.x has an offscreen-PTY race where a surface that is not currently
/// focused can be slow to process `paste-buffer`; the cmux CLI eventually
/// returns "Command timed out". The previous fix at the workspace-open layer
/// (d0c5594, `src/setup/workspace.rs:140-155`) handled the same race for the
/// initial agent launch by briefly focusing the target workspace, letting the
/// PTY wake, then restoring the prior focus. The send/paste-buffer path lost
/// that mirror when cmux send paths were unified, so we reapply it here.
fn run_cmux_paste_buffer_with_retry(
    runner: &dyn CommandRunner,
    surface_id: &str,
    workspace: Option<&str>,
    args: &[&str],
) -> Result<()> {
    const DWELL_MS: &[u64] = &[0, 1_000, 3_000];
    let cmux = CmuxService::new(runner);
    let mut prior_focus: Option<CmuxCaller> = None;
    let mut last_err: Option<String> = None;
    for (idx, &dwell_ms) in DWELL_MS.iter().enumerate() {
        if idx > 0 {
            if prior_focus.is_none() {
                prior_focus = cmux
                    .identity_context()
                    .and_then(|identity| identity.focused.or(identity.caller));
            }
            let _ = cmux.focus_surface(surface_id, workspace);
            std::thread::sleep(std::time::Duration::from_millis(dwell_ms));
            if let Some(prior) = prior_focus.as_ref() {
                restore_prior_focus(&cmux, prior);
            }
        }
        let out = runner.run("cmux", args, None)?;
        if out.success {
            return Ok(());
        }
        let message = command_error(&out);
        if !is_transient_paste_failure(&message) {
            bail!("cmux paste-buffer failed: {message}");
        }
        last_err = Some(message);
    }
    bail!(
        "cmux paste-buffer failed after {} attempts: {}",
        DWELL_MS.len(),
        last_err.unwrap_or_else(|| "command exited with non-zero status".into())
    )
}

fn restore_prior_focus(cmux: &CmuxService<'_>, prior: &CmuxCaller) {
    if let Some(surface) = prior.surface.as_deref().filter(|s| !s.trim().is_empty()) {
        if cmux
            .focus_surface(surface, prior.workspace.as_deref())
            .is_ok()
        {
            return;
        }
    }
    if let Some(workspace) = prior.workspace.as_deref().filter(|w| !w.trim().is_empty()) {
        let _ = cmux.select_workspace(workspace);
    }
}

fn is_transient_paste_failure(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("timed out") || lower.contains("timeout")
}

pub(crate) fn codex_prompt_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                i += 1;
                start = i;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                start = i;
            }
            _ => i += 1,
        }
    }

    lines.push(&text[start..]);
    lines
}

fn run_cmux(runner: &dyn CommandRunner, args: &[&str], verb: &str) -> Result<()> {
    let out = runner.run("cmux", args, None)?;
    if !out.success {
        bail!("cmux {verb} failed: {}", command_error(&out));
    }
    Ok(())
}

fn command_error(out: &CmdOutput) -> String {
    let stderr = out.stderr.trim();
    if !stderr.is_empty() {
        return stderr.into();
    }
    let stdout = out.stdout.trim();
    if !stdout.is_empty() {
        stdout.into()
    } else {
        "command exited with non-zero status".into()
    }
}

fn validate_payload(text: &str, cap: usize) -> Result<()> {
    if !text.is_ascii() {
        bail!("cmux push payload must be ASCII-only");
    }
    if text.len() > cap {
        bail!(
            "cmux push payload is {} bytes, above the {cap}-byte cap",
            text.len()
        );
    }
    Ok(())
}

fn find_surface_target(
    value: &Value,
    surface_id: &str,
    workspace: Option<&str>,
) -> Option<CmuxPushTarget> {
    let current_workspace = object_workspace_ref(value).or(workspace);

    if object_matches_surface(value, surface_id) {
        return Some(CmuxPushTarget {
            surface_id: surface_id.into(),
            workspace: current_workspace.map(str::to_string),
            kind: env_kind(value)
                .or_else(|| process_kind(value))
                .unwrap_or(PushKind::Unknown),
        });
    }

    match value {
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_surface_target(item, surface_id, current_workspace)),
        Value::Object(map) => map
            .values()
            .find_map(|item| find_surface_target(item, surface_id, current_workspace)),
        _ => None,
    }
}

fn object_workspace_ref(value: &Value) -> Option<&str> {
    let Value::Object(map) = value else {
        return None;
    };
    for key in ["workspace", "workspace_ref", "ref", "handle"] {
        let Some(value) = map.get(key).and_then(Value::as_str) else {
            continue;
        };
        if value.starts_with("workspace:") {
            return Some(value);
        }
    }
    None
}

fn object_matches_surface(value: &Value, surface_id: &str) -> bool {
    let Value::Object(map) = value else {
        return false;
    };
    [
        "surface",
        "surface_ref",
        "surface_id",
        "ref",
        "id",
        "handle",
    ]
    .iter()
    .any(|key| map.get(*key).and_then(Value::as_str) == Some(surface_id))
}

fn env_kind(value: &Value) -> Option<PushKind> {
    let Value::Object(map) = value else {
        return None;
    };
    for key in [
        "CMUX_AGENT_LAUNCH_KIND",
        "cmux_agent_launch_kind",
        "agent_kind",
        "launch_kind",
    ] {
        if let Some(kind) = map.get(key).and_then(Value::as_str).map(PushKind::parse) {
            if kind != PushKind::Unknown {
                return Some(kind);
            }
        }
    }
    if let Some(Value::Object(env)) = map.get("env").or_else(|| map.get("environment")) {
        for key in ["CMUX_AGENT_LAUNCH_KIND", "cmux_agent_launch_kind"] {
            if let Some(kind) = env.get(key).and_then(Value::as_str).map(PushKind::parse) {
                if kind != PushKind::Unknown {
                    return Some(kind);
                }
            }
        }
    }
    None
}

fn process_kind(value: &Value) -> Option<PushKind> {
    match value {
        Value::String(value) => {
            let lower = value.to_ascii_lowercase();
            if lower.contains("codex") {
                Some(PushKind::Codex)
            } else if lower.contains("claude") {
                Some(PushKind::Claude)
            } else {
                None
            }
        }
        Value::Array(items) => items.iter().find_map(process_kind),
        Value::Object(map) => map.values().find_map(process_kind),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn codex_push_sends_single_line_then_enter() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("", true);

        push_to_surface(&runner, "surface:4", PushKind::Codex, "hello").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].1,
            vec!["send", "--surface", "surface:4", "--", "hello"]
        );
        assert_eq!(
            calls[1].1,
            vec!["send-key", "--surface", "surface:4", "--", "enter"]
        );
    }

    #[test]
    fn codex_push_sends_multiline_text_with_shift_enter_between_lines() {
        let mut runner = MockRunner::new();
        for _ in 0..5 {
            runner.add_response("", true);
        }

        push_to_surface(&runner, "surface:4", PushKind::Codex, "hello\n\nworld").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(
            calls[0].1,
            vec!["send", "--surface", "surface:4", "--", "hello"]
        );
        assert_eq!(
            calls[1].1,
            vec![
                "send-key",
                "--surface",
                "surface:4",
                "--",
                CODEX_IN_PROMPT_NEWLINE_KEY
            ]
        );
        // empty middle line: no cmux send "", just shift-enter
        assert_eq!(
            calls[2].1,
            vec![
                "send-key",
                "--surface",
                "surface:4",
                "--",
                CODEX_IN_PROMPT_NEWLINE_KEY
            ]
        );
        assert_eq!(
            calls[3].1,
            vec!["send", "--surface", "surface:4", "--", "world"]
        );
        assert_eq!(
            calls[4].1,
            vec!["send-key", "--surface", "surface:4", "--", "enter"]
        );
    }

    #[test]
    fn codex_push_in_workspace_passes_workspace_to_send_commands() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("", true);

        CmuxPushService::new(&runner)
            .push_to_surface_in_workspace(
                "surface:4",
                Some("workspace:2"),
                PushKind::Codex,
                "hello",
            )
            .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[0].1,
            vec![
                "send",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2",
                "--",
                "hello"
            ]
        );
        assert_eq!(
            calls[1].1,
            vec![
                "send-key",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2",
                "--",
                "enter"
            ]
        );
    }

    #[test]
    fn codex_prompt_lines_splits_lf_crlf_and_empty_payloads() {
        assert_eq!(codex_prompt_lines(""), vec![""]);
        assert_eq!(codex_prompt_lines("a\nb\n"), vec!["a", "b", ""]);
        assert_eq!(codex_prompt_lines("a\r\nb\rc"), vec!["a", "b", "c"]);
    }

    #[test]
    fn claude_push_uses_paste_buffer_then_enter() {
        let mut runner = MockRunner::new();
        runner.add_response("", true); // set-buffer
        runner.add_response("", true); // paste-buffer
        runner.add_response("", true); // send-key enter

        push_to_surface(&runner, "surface:4", PushKind::Claude, "hello").unwrap();

        let calls = runner.calls.lock().unwrap();
        let verbs: Vec<&str> = calls.iter().map(|c| c.1[0].as_str()).collect();
        assert_eq!(verbs, vec!["set-buffer", "paste-buffer", "send-key"]);
        assert!(calls[0].1[2].starts_with("wt-claude-surface-4-"));
        assert_eq!(calls[0].1.last().unwrap(), "hello");
        assert_eq!(calls[2].1.last().unwrap(), "enter");
    }

    #[test]
    fn unknown_push_kind_fails_closed() {
        let runner = MockRunner::new();

        let err = push_to_surface(&runner, "surface:4", PushKind::Unknown, "hello").unwrap_err();

        assert!(err.to_string().contains("target agent kind is unknown"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_non_ascii_payloads() {
        let runner = MockRunner::new();
        let err = CmuxPushService::new(&runner)
            .push_to_surface("surface:4", PushKind::Claude, "안녕")
            .unwrap_err();
        assert!(err.to_string().contains("ASCII"));
    }

    #[test]
    fn free_function_rejects_invalid_payloads() {
        let runner = MockRunner::new();

        let err = push_to_surface(&runner, "surface:4", PushKind::Claude, "안녕").unwrap_err();

        assert!(err.to_string().contains("ASCII"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn detects_kind_from_surface_env() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"surfaces":[{"surface":"surface:4","env":{"CMUX_AGENT_LAUNCH_KIND":"codex"}}]}"#,
            true,
        );

        assert_eq!(
            detect_target_kind(&runner, "surface:4").unwrap(),
            PushKind::Codex
        );
    }

    #[test]
    fn detects_workspace_from_containing_workspace_object() {
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"workspaces":[{"ref":"workspace:2","surfaces":[{"ref":"surface:4","processes":[{"name":"codex"}]}]}]}"#,
            true,
        );

        let target = detect_target(&runner, "surface:4").unwrap();

        assert_eq!(target.workspace.as_deref(), Some("workspace:2"));
        assert_eq!(target.kind, PushKind::Codex);
    }
}
