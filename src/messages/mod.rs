use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) const COORDINATOR_AGENT_ALIAS: &str = "coordinator";
pub(crate) const COORDINATOR_AGENT_ID: &str = "agents/coordinator";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentId(String);

impl AgentId {
    pub fn parse(input: &str) -> Result<Self> {
        let raw = input.trim();
        if raw.is_empty() {
            bail!(
                "Agent id cannot be empty. Use NAME or agents/NAME, for example codex or agents/codex."
            );
        }
        if raw == "agents" {
            bail!("Invalid agent id `agents`. Use a concrete id such as agents/codex.");
        }

        let name = if let Some(rest) = raw.strip_prefix("agents/") {
            if rest.is_empty() || rest.contains('/') {
                bail!(
                    "Invalid agent id `{raw}`. Agent ids must be NAME or agents/NAME with one non-empty agent name segment."
                );
            }
            rest
        } else {
            if raw.contains('/') {
                bail!(
                    "Invalid agent id `{raw}`. Agent ids must be NAME or agents/NAME; path-like ids are ambiguous."
                );
            }
            raw
        };

        validate_agent_name(raw, name)?;
        Ok(Self(format!("agents/{name}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn inbox_dir(&self, root: &Path) -> PathBuf {
        root.join(self.as_str()).join("inbox")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub meta: MessageMeta,
    pub envelope: MessageEnvelope,
    pub body: MessageBody,
}

impl Message {
    fn new(id: String, created_at: String, from: AgentId, to: AgentId, text: &str) -> Self {
        Self {
            meta: MessageMeta {
                id,
                created_at,
                from: from.as_str().into(),
                to: to.as_str().into(),
            },
            envelope: MessageEnvelope {
                kind: "request".into(),
                priority: "normal".into(),
                expects_response: true,
                correlates_with: None,
            },
            body: MessageBody {
                summary: body_summary(text),
                parts: vec![MessagePart {
                    part_type: "text".into(),
                    content: text.into(),
                }],
            },
        }
    }

    fn text_content(&self) -> String {
        self.body
            .parts
            .iter()
            .filter(|part| part.part_type == "text")
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageMeta {
    pub id: String,
    pub created_at: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageEnvelope {
    pub kind: String,
    pub priority: String,
    pub expects_response: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlates_with: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageBody {
    pub summary: String,
    pub parts: Vec<MessagePart>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagePart {
    #[serde(rename = "type")]
    pub part_type: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentMessage {
    pub id: String,
    pub path: PathBuf,
    pub message: Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveredMessage {
    pub original_path: PathBuf,
    pub read_path: PathBuf,
    pub message: Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxDelivery {
    pub agent: AgentId,
    pub messages: Vec<DeliveredMessage>,
}

impl InboxDelivery {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn additional_context(&self) -> String {
        render_additional_context(self)
    }
}

#[derive(Clone, Debug)]
pub struct MessageStore {
    root: PathBuf,
}

impl MessageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn send(&self, to: &str, text: &str) -> Result<SentMessage> {
        let from = sender_agent_id()?;
        let to = AgentId::parse(to).context("Invalid target agent id")?;
        self.send_from_agent(from, to, text)
    }

    pub fn send_from(&self, from: &str, to: &str, text: &str) -> Result<SentMessage> {
        let from = AgentId::parse(from).context("Invalid sender agent id")?;
        let to = AgentId::parse(to).context("Invalid target agent id")?;
        self.send_from_agent(from, to, text)
    }

    fn send_from_agent(&self, from: AgentId, to: AgentId, text: &str) -> Result<SentMessage> {
        if text.trim().is_empty() {
            bail!("Message cannot be empty");
        }

        let inbox = to.inbox_dir(&self.root);
        fs::create_dir_all(&inbox)
            .with_context(|| format!("Failed to create inbox: {}", inbox.display()))?;

        for _ in 0..16 {
            let id = new_message_id(from.as_str(), to.as_str(), text);
            let message = Message::new(
                id.clone(),
                current_utc_timestamp(),
                from.clone(),
                to.clone(),
                text,
            );
            let path = inbox.join(format!("{id}.toml"));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let content = toml::to_string_pretty(&message)
                        .context("Failed to serialize message TOML")?;
                    file.write_all(content.as_bytes())
                        .with_context(|| format!("Failed to write message: {}", path.display()))?;
                    return Ok(SentMessage { id, path, message });
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("Failed to create message: {}", path.display()));
                }
            }
        }

        bail!("Failed to allocate a unique message id after repeated attempts");
    }

    pub fn check_inbox(&self, agent: &str) -> Result<InboxDelivery> {
        let agent = AgentId::parse(agent)?;
        let inbox = agent.inbox_dir(&self.root);
        let paths = unread_message_paths(&inbox)?;
        if paths.is_empty() {
            return Ok(InboxDelivery {
                agent,
                messages: Vec::new(),
            });
        }

        let read_dir = inbox.join("read");
        fs::create_dir_all(&read_dir)
            .with_context(|| format!("Failed to create read inbox: {}", read_dir.display()))?;

        let mut messages = Vec::new();
        for path in paths {
            let message = read_message_for_agent(&agent, &path)?;
            let read_path = next_read_path(&read_dir, &path)?;
            fs::rename(&path, &read_path).with_context(|| {
                format!(
                    "Failed to move message to read inbox: {} -> {}",
                    path.display(),
                    read_path.display()
                )
            })?;
            messages.push(DeliveredMessage {
                original_path: path,
                read_path,
                message,
            });
        }

        Ok(InboxDelivery { agent, messages })
    }
}

fn sender_agent_id() -> Result<AgentId> {
    match env::var("WT_AGENT_ID") {
        Ok(value) => AgentId::parse(&value).context("Invalid WT_AGENT_ID"),
        Err(env::VarError::NotPresent) => AgentId::parse("agents/user"),
        Err(env::VarError::NotUnicode(_)) => bail!("Invalid WT_AGENT_ID: value is not Unicode"),
    }
}

#[derive(Debug, Serialize)]
pub struct HookOutput {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput,
}

impl HookOutput {
    pub fn new(hook_event_name: impl Into<String>, additional_context: impl Into<String>) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: hook_event_name.into(),
                additional_context: additional_context.into(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    hook_event_name: String,
    #[serde(rename = "additionalContext")]
    additional_context: String,
}

fn validate_agent_name(raw: &str, name: &str) -> Result<()> {
    if name == "." || name == ".." || name.starts_with('.') {
        bail!(
            "Invalid agent id `{raw}`. Agent name must not be a hidden or parent directory segment."
        );
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        bail!(
            "Invalid agent id `{raw}`. Agent names may contain only ASCII letters, digits, dots, dashes, and underscores."
        );
    }
    Ok(())
}

fn unread_message_paths(inbox: &Path) -> Result<Vec<PathBuf>> {
    if !inbox.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in
        fs::read_dir(inbox).with_context(|| format!("Failed to read inbox: {}", inbox.display()))?
    {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_message_for_agent(agent: &AgentId, path: &Path) -> Result<Message> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read message: {}", path.display()))?;
    let message: Message = toml::from_str(&content)
        .with_context(|| format!("Failed to parse message: {}", path.display()))?;
    validate_message_for_agent(agent, &message, path)?;
    Ok(message)
}

fn validate_message_for_agent(agent: &AgentId, message: &Message, path: &Path) -> Result<()> {
    if message.meta.id.trim().is_empty() {
        bail!("Message {} is missing meta.id", path.display());
    }
    if message.meta.created_at.trim().is_empty() {
        bail!("Message {} is missing meta.created_at", path.display());
    }
    AgentId::parse(&message.meta.from)
        .with_context(|| format!("Message {} has invalid meta.from", path.display()))?;
    let to = AgentId::parse(&message.meta.to)
        .with_context(|| format!("Message {} has invalid meta.to", path.display()))?;
    if to.as_str() != agent.as_str() {
        bail!(
            "Message {} is addressed to {}, not {}",
            path.display(),
            to.as_str(),
            agent.as_str()
        );
    }
    if message.envelope.kind.trim().is_empty() {
        bail!("Message {} is missing envelope.kind", path.display());
    }
    if message.envelope.priority.trim().is_empty() {
        bail!("Message {} is missing envelope.priority", path.display());
    }
    if message.body.parts.is_empty() {
        bail!(
            "Message {} must contain at least one body part",
            path.display()
        );
    }
    for part in &message.body.parts {
        if part.part_type != "text" {
            bail!(
                "Message {} contains unsupported body part type `{}`",
                path.display(),
                part.part_type
            );
        }
    }
    Ok(())
}

fn next_read_path(read_dir: &Path, original_path: &Path) -> Result<PathBuf> {
    let file_name = original_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!("Message file has no file name: {}", original_path.display())
        })?;
    let target = read_dir.join(file_name);
    if !target.exists() {
        return Ok(target);
    }

    let stem = original_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("message");
    for index in 1..1000 {
        let candidate = read_dir.join(format!("{stem}-{index}.toml"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    bail!(
        "Failed to choose a read inbox path for {}",
        original_path.display()
    );
}

fn render_additional_context(delivery: &InboxDelivery) -> String {
    let count = delivery.messages.len();
    let mut lines = vec![
        format!(
            "WT INBOX for {}: {count} unread message(s).",
            delivery.agent.as_str()
        ),
        "When a message asks for a response, use `wt msg send --to <agent> <message>`.".into(),
    ];

    for delivered in &delivery.messages {
        let message = &delivered.message;
        lines.push(String::new());
        lines.push(format!("- id: {}", message.meta.id));
        lines.push(format!("  from: {}", message.meta.from));
        lines.push(format!("  summary: {}", message.body.summary));
        lines.push("  content:".into());
        let content = message.text_content();
        if content.is_empty() {
            lines.push("    ".into());
        } else {
            for line in content.lines() {
                lines.push(format!("    {line}"));
            }
        }
    }

    lines.join("\n")
}

fn new_message_id(from: &str, to: &str, text: &str) -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = duration.as_nanos();
    let pid = std::process::id();
    let sequence = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let fingerprint = fingerprint64(&[
        from.as_bytes(),
        to.as_bytes(),
        text.as_bytes(),
        &nanos.to_le_bytes(),
        &pid.to_le_bytes(),
        &sequence.to_le_bytes(),
    ]);
    format!("msg_{nanos:x}_{pid:x}_{sequence:x}_{fingerprint:016x}")
}

fn fingerprint64(parts: &[&[u8]]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn body_summary(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&one_line, 120)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn normalizes_agent_ids() {
        assert_eq!(AgentId::parse("codex").unwrap().as_str(), "agents/codex");
        assert_eq!(
            AgentId::parse("agents/codex").unwrap().as_str(),
            "agents/codex"
        );
        assert_eq!(
            AgentId::parse(COORDINATOR_AGENT_ALIAS).unwrap().as_str(),
            COORDINATOR_AGENT_ID
        );
    }

    #[test]
    fn rejects_path_like_or_ambiguous_agent_ids() {
        for input in [
            "",
            "agents",
            "agents/codex/worker",
            "humans/user",
            "../codex",
            ".codex",
        ] {
            assert!(
                AgentId::parse(input).is_err(),
                "{input:?} should be rejected"
            );
        }
    }

    #[test]
    fn send_writes_a_toml_message_to_the_agent_inbox() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));

        let sent = store.send("codex", "hello from unit test").unwrap();

        assert_eq!(sent.message.meta.to, "agents/codex");
        assert!(
            sent.path
                .starts_with(temp.path().join("messages/agents/codex/inbox"))
        );
        let content = fs::read_to_string(&sent.path).unwrap();
        let parsed: Message = toml::from_str(&content).unwrap();
        assert_eq!(parsed.meta.id, sent.id);
        assert_eq!(parsed.body.summary, "hello from unit test");
        assert_eq!(parsed.body.parts[0].content, "hello from unit test");
    }

    #[test]
    fn check_inbox_moves_messages_to_read_and_renders_context() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let sent = store
            .send_from("agents/claude", "agents/codex", "please respond")
            .unwrap();

        let delivery = store.check_inbox("agents/codex").unwrap();

        assert_eq!(delivery.messages.len(), 1);
        assert!(!sent.path.exists());
        assert!(delivery.messages[0].read_path.exists());
        assert!(
            delivery.messages[0]
                .read_path
                .ends_with(format!("{}.toml", sent.id))
        );
        let context = delivery.additional_context();
        assert!(context.contains("WT INBOX for agents/codex: 1 unread message"));
        assert!(context.contains("from: agents/claude"));
        assert!(context.contains("please respond"));
    }

    #[test]
    fn empty_inbox_has_no_delivery() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));

        let delivery = store.check_inbox("codex").unwrap();

        assert!(delivery.is_empty());
    }
}
