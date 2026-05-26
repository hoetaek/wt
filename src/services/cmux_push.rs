use crate::context::{CmdOutput, CommandRunner};
use anyhow::{Context, Result, bail};
use serde_json::Value;

pub const DEFAULT_PAYLOAD_CAP_BYTES: usize = 1024;

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
        PushKind::Codex => {
            let send_args = cmux_send_args("send", surface_id, workspace, text);
            run_cmux(runner, &send_args, "send")?;
            let send_key_args = cmux_send_args("send-key", surface_id, workspace, "enter");
            run_cmux(runner, &send_key_args, "send-key")
        }
        PushKind::Claude => {
            let text = format!("{text}\\n");
            let send_args = cmux_send_args("send", surface_id, workspace, &text);
            run_cmux(runner, &send_args, "send")
        }
        PushKind::Unknown => {
            bail!(
                "Cannot push to cmux surface {surface_id}: target agent kind is unknown. Pass --kind claude or --kind codex."
            )
        }
    }
}

fn cmux_send_args<'a>(
    command: &'a str,
    surface_id: &'a str,
    workspace: Option<&'a str>,
    payload: &'a str,
) -> Vec<&'a str> {
    let mut args = vec![command, "--surface", surface_id];
    if let Some(workspace) = workspace {
        args.extend(["--workspace", workspace]);
    }
    args.push(payload);
    args
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
    fn codex_push_sends_text_then_enter() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        runner.add_response("", true);

        push_to_surface(&runner, "surface:4", PushKind::Codex, "hello").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, vec!["send", "--surface", "surface:4", "hello"]);
        assert_eq!(
            calls[1].1,
            vec!["send-key", "--surface", "surface:4", "enter"]
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
        assert_eq!(
            calls[0].1,
            vec![
                "send",
                "--surface",
                "surface:4",
                "--workspace",
                "workspace:2",
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
                "enter"
            ]
        );
    }

    #[test]
    fn claude_push_sends_inline_newline() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        push_to_surface(&runner, "surface:4", PushKind::Claude, "hello").unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            vec!["send", "--surface", "surface:4", "hello\\n"]
        );
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
