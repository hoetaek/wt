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

    fn inbox_state_dir(&self, root: &Path, state: MessageDeliveryState) -> PathBuf {
        self.inbox_dir(root).join(state.directory_name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub meta: MessageMeta,
    pub scope: MessageScope,
    pub envelope: MessageEnvelope,
    pub delivery: MessageDelivery,
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
            scope: MessageScope::direct(),
            envelope: MessageEnvelope {
                kind: "request".into(),
                priority: "normal".into(),
                expects_response: true,
                correlates_with: None,
            },
            delivery: MessageDelivery::new(),
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

    fn mark_delivered(&mut self) {
        self.delivery.state = MessageDeliveryState::Delivered;
        self.delivery.attempts = self.delivery.attempts.saturating_add(1);
        self.delivery.claimed_by = None;
        self.delivery.lease_expires_at = None;
        self.delivery.last_error = None;
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
pub struct MessageScope {
    pub kind: MessageScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl MessageScope {
    pub fn direct() -> Self {
        Self {
            kind: MessageScopeKind::Direct,
            id: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageScopeKind {
    Direct,
    Workflow,
    TaskRun,
    Repo,
}

impl MessageScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Workflow => "workflow",
            Self::TaskRun => "task_run",
            Self::Repo => "repo",
        }
    }
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
pub struct MessageDelivery {
    pub state: MessageDeliveryState,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl MessageDelivery {
    pub fn new() -> Self {
        Self {
            state: MessageDeliveryState::New,
            attempts: 0,
            claimed_by: None,
            lease_expires_at: None,
            last_error: None,
        }
    }
}

impl Default for MessageDelivery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeliveryState {
    New,
    Claimed,
    Delivered,
    Retry,
    Failed,
}

impl MessageDeliveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Claimed => "claimed",
            Self::Delivered => "delivered",
            Self::Retry => "retry",
            Self::Failed => "failed",
        }
    }

    fn directory_name(self) -> &'static str {
        self.as_str()
    }
}

impl std::fmt::Display for MessageDeliveryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
    pub delivered_path: PathBuf,
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

        let inbox = to.inbox_state_dir(&self.root, MessageDeliveryState::New);
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
        reject_pre_redesign_message_paths(&inbox)?;

        let new_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::New);
        let paths = message_paths(&new_dir)?;
        if paths.is_empty() {
            return Ok(InboxDelivery {
                agent,
                messages: Vec::new(),
            });
        }

        let mut pending = Vec::new();
        for path in paths {
            let message = read_message_for_agent(&agent, &path)?;
            if message.delivery.state != MessageDeliveryState::New {
                bail!(
                    "Message {} is in inbox/new but delivery.state is `{}`",
                    path.display(),
                    message.delivery.state
                );
            }
            if message.scope.kind != MessageScopeKind::Direct {
                bail!(
                    "Message {} has scope.kind `{}`; wt msg check-inbox only delivers direct-scope messages until scoped claim delivery is implemented",
                    path.display(),
                    message.scope.kind.as_str()
                );
            }
            pending.push((path, message));
        }

        let delivered_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Delivered);
        fs::create_dir_all(&delivered_dir).with_context(|| {
            format!(
                "Failed to create delivered inbox: {}",
                delivered_dir.display()
            )
        })?;

        let mut messages = Vec::new();
        for (path, mut message) in pending {
            let delivered_path = next_state_path(&delivered_dir, &path)?;
            fs::rename(&path, &delivered_path).with_context(|| {
                format!(
                    "Failed to move message to delivered inbox: {} -> {}",
                    path.display(),
                    delivered_path.display()
                )
            })?;

            message.mark_delivered();
            let content = toml::to_string_pretty(&message)
                .context("Failed to serialize delivered message TOML")?;
            fs::write(&delivered_path, content).with_context(|| {
                format!(
                    "Failed to update delivered message: {}",
                    delivered_path.display()
                )
            })?;
            messages.push(DeliveredMessage {
                original_path: path,
                delivered_path,
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

fn reject_pre_redesign_message_paths(inbox: &Path) -> Result<()> {
    if !inbox.is_dir() {
        return Ok(());
    }

    let mut legacy_paths = message_paths(inbox)?;
    legacy_paths.extend(message_paths(&inbox.join("read"))?);
    if legacy_paths.is_empty() {
        return Ok(());
    }

    legacy_paths.sort();
    let first = legacy_paths
        .first()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| inbox.display().to_string());
    bail!(
        "Found pre-redesign message file {first}. Canonical scoped delivery stores messages under inbox/new, inbox/claimed, inbox/delivered, inbox/retry, or inbox/failed; wt does not silently consume legacy inbox files."
    );
}

fn message_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("Failed to read inbox: {}", dir.display()))?
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
    validate_message_scope(&message.scope, path)?;
    if message.envelope.kind.trim().is_empty() {
        bail!("Message {} is missing envelope.kind", path.display());
    }
    if message.envelope.priority.trim().is_empty() {
        bail!("Message {} is missing envelope.priority", path.display());
    }
    validate_message_delivery(&message.delivery, path)?;
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

fn validate_message_scope(scope: &MessageScope, path: &Path) -> Result<()> {
    match scope.kind {
        MessageScopeKind::Direct | MessageScopeKind::Repo => {
            if scope.id.is_some() {
                bail!(
                    "Message {} uses scope.kind `{}` but also sets scope.id",
                    path.display(),
                    scope.kind.as_str()
                );
            }
        }
        MessageScopeKind::Workflow | MessageScopeKind::TaskRun => {
            if scope.id.as_deref().unwrap_or_default().trim().is_empty() {
                bail!(
                    "Message {} uses scope.kind `{}` but is missing scope.id",
                    path.display(),
                    scope.kind.as_str()
                );
            }
        }
    }
    Ok(())
}

fn validate_message_delivery(delivery: &MessageDelivery, path: &Path) -> Result<()> {
    if matches!(delivery.state, MessageDeliveryState::Claimed) {
        if delivery
            .claimed_by
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            bail!(
                "Message {} is claimed but missing delivery.claimed_by",
                path.display()
            );
        }
        if delivery
            .lease_expires_at
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            bail!(
                "Message {} is claimed but missing delivery.lease_expires_at",
                path.display()
            );
        }
    } else if delivery.claimed_by.is_some() || delivery.lease_expires_at.is_some() {
        bail!(
            "Message {} has claim metadata but delivery.state is `{}`",
            path.display(),
            delivery.state
        );
    }
    if delivery
        .last_error
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        bail!("Message {} has empty delivery.last_error", path.display());
    }
    Ok(())
}

fn next_state_path(state_dir: &Path, original_path: &Path) -> Result<PathBuf> {
    let file_name = original_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!("Message file has no file name: {}", original_path.display())
        })?;
    let target = state_dir.join(file_name);
    if !target.exists() {
        return Ok(target);
    }

    let stem = original_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("message");
    for index in 1..1000 {
        let candidate = state_dir.join(format!("{stem}-{index}.toml"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    bail!(
        "Failed to choose a message state path for {}",
        original_path.display()
    );
}

fn render_additional_context(delivery: &InboxDelivery) -> String {
    let count = delivery.messages.len();
    let mut lines = vec![
        format!(
            "WT INBOX for {}: {count} new message(s).",
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
    fn send_writes_a_toml_message_to_the_agent_inbox_new_state() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));

        let sent = store.send("codex", "hello from unit test").unwrap();

        assert_eq!(sent.message.meta.to, "agents/codex");
        assert!(
            sent.path
                .starts_with(temp.path().join("messages/agents/codex/inbox/new"))
        );
        let content = fs::read_to_string(&sent.path).unwrap();
        let parsed: Message = toml::from_str(&content).unwrap();
        assert_eq!(parsed.meta.id, sent.id);
        assert_eq!(parsed.scope.kind, MessageScopeKind::Direct);
        assert_eq!(parsed.scope.id, None);
        assert_eq!(parsed.delivery.state, MessageDeliveryState::New);
        assert_eq!(parsed.delivery.attempts, 0);
        assert_eq!(parsed.body.summary, "hello from unit test");
        assert_eq!(parsed.body.parts[0].content, "hello from unit test");
    }

    #[test]
    fn check_inbox_moves_messages_to_delivered_and_renders_context() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let sent = store
            .send_from("agents/claude", "agents/codex", "please respond")
            .unwrap();

        let delivery = store.check_inbox("agents/codex").unwrap();

        assert_eq!(delivery.messages.len(), 1);
        assert!(!sent.path.exists());
        assert!(delivery.messages[0].delivered_path.exists());
        assert!(
            delivery.messages[0]
                .delivered_path
                .ends_with(format!("{}.toml", sent.id))
        );
        assert_eq!(
            delivery.messages[0].message.delivery.state,
            MessageDeliveryState::Delivered
        );
        let content = fs::read_to_string(&delivery.messages[0].delivered_path).unwrap();
        let parsed: Message = toml::from_str(&content).unwrap();
        assert_eq!(parsed.delivery.state, MessageDeliveryState::Delivered);
        assert_eq!(parsed.delivery.attempts, 1);
        let context = delivery.additional_context();
        assert!(context.contains("WT INBOX for agents/codex: 1 new message"));
        assert!(context.contains("from: agents/claude"));
        assert!(context.contains("please respond"));
    }

    #[test]
    fn check_inbox_rejects_pre_redesign_root_messages() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let inbox = temp.path().join("messages/agents/codex/inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("old.toml"), "legacy = true\n").unwrap();

        let err = store.check_inbox("codex").unwrap_err().to_string();

        assert!(err.contains("pre-redesign message file"));
        assert!(err.contains("inbox/new"));
        assert!(err.contains("does not silently consume legacy inbox files"));
    }

    #[test]
    fn check_inbox_rejects_pre_redesign_read_messages() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let read_dir = temp.path().join("messages/agents/codex/inbox/read");
        fs::create_dir_all(&read_dir).unwrap();
        fs::write(read_dir.join("old.toml"), "legacy = true\n").unwrap();

        let err = store.check_inbox("codex").unwrap_err().to_string();

        assert!(err.contains("pre-redesign message file"));
        assert!(err.contains("inbox/read/old.toml"));
        assert!(err.contains("does not silently consume legacy inbox files"));
    }

    #[test]
    fn check_inbox_rejects_scoped_messages_until_claim_delivery_exists() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let sent = store
            .send_from("agents/claude", "agents/coordinator", "workflow scoped")
            .unwrap();
        let mut message = sent.message;
        message.scope = MessageScope {
            kind: MessageScopeKind::Workflow,
            id: Some("2026-05-20-001".into()),
        };
        fs::write(&sent.path, toml::to_string_pretty(&message).unwrap()).unwrap();

        let err = store
            .check_inbox("agents/coordinator")
            .unwrap_err()
            .to_string();

        assert!(err.contains("scope.kind `workflow`"));
        assert!(err.contains("scoped claim delivery"));
    }

    #[test]
    fn check_inbox_validates_all_messages_before_delivery_moves() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("messages");
        let store = MessageStore::new(&root);
        let new_dir = root.join("agents/codex/inbox/new");
        fs::create_dir_all(&new_dir).unwrap();

        let from = AgentId::parse("agents/claude").unwrap();
        let to = AgentId::parse("agents/codex").unwrap();
        let valid = Message::new(
            "a-valid".into(),
            "2026-05-20T12:00:00Z".into(),
            from.clone(),
            to.clone(),
            "valid",
        );
        let mut invalid = Message::new(
            "z-invalid".into(),
            "2026-05-20T12:00:00Z".into(),
            from,
            to,
            "invalid",
        );
        invalid.scope = MessageScope {
            kind: MessageScopeKind::Workflow,
            id: Some("2026-05-20-001".into()),
        };
        fs::write(
            new_dir.join("a-valid.toml"),
            toml::to_string_pretty(&valid).unwrap(),
        )
        .unwrap();
        fs::write(
            new_dir.join("z-invalid.toml"),
            toml::to_string_pretty(&invalid).unwrap(),
        )
        .unwrap();

        let err = store.check_inbox("codex").unwrap_err().to_string();

        assert!(err.contains("scope.kind `workflow`"));
        assert!(new_dir.join("a-valid.toml").exists());
        assert!(new_dir.join("z-invalid.toml").exists());
        assert!(!root.join("agents/codex/inbox/delivered").exists());
    }

    #[test]
    fn empty_inbox_has_no_delivery() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));

        let delivery = store.check_inbox("codex").unwrap();

        assert!(delivery.is_empty());
    }
}
