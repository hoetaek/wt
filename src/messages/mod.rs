use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    fn mark_claimed(&mut self, claimed_by: &AgentId, lease_expires_at: String) {
        self.delivery.state = MessageDeliveryState::Claimed;
        self.delivery.claimed_by = Some(claimed_by.as_str().into());
        self.delivery.lease_expires_at = Some(lease_expires_at);
        self.delivery.last_error = None;
    }

    fn mark_retry(&mut self, error: &str) {
        self.delivery.state = MessageDeliveryState::Retry;
        self.delivery.attempts = self.delivery.attempts.saturating_add(1);
        self.delivery.claimed_by = None;
        self.delivery.lease_expires_at = None;
        self.delivery.last_error = Some(error.into());
    }

    fn mark_failed(&mut self, error: &str, count_attempt: bool) {
        self.delivery.state = MessageDeliveryState::Failed;
        if count_attempt {
            self.delivery.attempts = self.delivery.attempts.saturating_add(1);
        }
        self.delivery.claimed_by = None;
        self.delivery.lease_expires_at = None;
        self.delivery.last_error = Some(error.into());
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

    pub fn repo() -> Self {
        Self {
            kind: MessageScopeKind::Repo,
            id: None,
        }
    }

    pub fn workflow(id: impl Into<String>) -> Result<Self> {
        Self::with_required_id(MessageScopeKind::Workflow, id)
    }

    pub fn task_run(id: impl Into<String>) -> Result<Self> {
        Self::with_required_id(MessageScopeKind::TaskRun, id)
    }

    fn with_required_id(kind: MessageScopeKind, id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if id.trim().is_empty() {
            bail!("Message scope `{}` requires a non-empty id", kind.as_str());
        }
        Ok(Self { kind, id: Some(id) })
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
pub struct ClaimedMessage {
    pub original_path: PathBuf,
    pub claimed_path: PathBuf,
    pub message: Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetriedMessage {
    pub original_path: PathBuf,
    pub retry_path: PathBuf,
    pub message: Message,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailedMessage {
    pub original_path: PathBuf,
    pub failed_path: PathBuf,
    pub message: Option<Message>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReclaimedMessage {
    pub original_path: PathBuf,
    pub retry_path: PathBuf,
    pub message: Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageLease {
    duration: Duration,
}

impl MessageLease {
    pub fn new(duration: Duration) -> Result<Self> {
        if duration.is_zero() {
            bail!("Message claim lease duration must be greater than zero");
        }
        Ok(Self { duration })
    }

    fn expires_at(self, now: SystemTime) -> String {
        timestamp_from_system_time(now + self.duration)
    }
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
        self.send_from_agent_with_scope(from, to, MessageScope::direct(), text)
    }

    fn send_from_agent(&self, from: AgentId, to: AgentId, text: &str) -> Result<SentMessage> {
        self.send_from_agent_with_scope(from, to, MessageScope::direct(), text)
    }

    pub fn send_scoped(&self, to: &str, scope: MessageScope, text: &str) -> Result<SentMessage> {
        let from = sender_agent_id()?;
        let to = AgentId::parse(to).context("Invalid target agent id")?;
        self.send_from_agent_with_scope(from, to, scope, text)
    }

    pub fn send_scoped_from(
        &self,
        from: &str,
        to: &str,
        scope: MessageScope,
        text: &str,
    ) -> Result<SentMessage> {
        let from = AgentId::parse(from).context("Invalid sender agent id")?;
        let to = AgentId::parse(to).context("Invalid target agent id")?;
        self.send_from_agent_with_scope(from, to, scope, text)
    }

    fn send_from_agent_with_scope(
        &self,
        from: AgentId,
        to: AgentId,
        scope: MessageScope,
        text: &str,
    ) -> Result<SentMessage> {
        if text.trim().is_empty() {
            bail!("Message cannot be empty");
        }
        validate_scope_value(&scope).context("Invalid message scope")?;

        let inbox = to.inbox_state_dir(&self.root, MessageDeliveryState::New);
        fs::create_dir_all(&inbox)
            .with_context(|| format!("Failed to create inbox: {}", inbox.display()))?;

        for _ in 0..16 {
            let id = new_message_id(from.as_str(), to.as_str(), text);
            let mut message = Message::new(
                id.clone(),
                current_utc_timestamp(),
                from.clone(),
                to.clone(),
                text,
            );
            message.scope = scope.clone();
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

    pub fn claim_next(
        &self,
        agent: &str,
        scope: &MessageScope,
        claimed_by: &str,
        lease: MessageLease,
    ) -> Result<Option<ClaimedMessage>> {
        let agent = AgentId::parse(agent)?;
        let claimed_by =
            AgentId::parse(claimed_by).context("Invalid delivery claimant agent id")?;
        validate_scope_value(scope).context("Invalid claim scope")?;

        for state in [MessageDeliveryState::New, MessageDeliveryState::Retry] {
            if let Some(claimed) =
                self.claim_next_from_state(&agent, scope, &claimed_by, lease, state)?
            {
                return Ok(Some(claimed));
            }
        }

        Ok(None)
    }

    fn claim_next_from_state(
        &self,
        agent: &AgentId,
        scope: &MessageScope,
        claimed_by: &AgentId,
        lease: MessageLease,
        state: MessageDeliveryState,
    ) -> Result<Option<ClaimedMessage>> {
        let state_dir = agent.inbox_state_dir(&self.root, state);
        for path in message_paths(&state_dir)? {
            let message = match read_message_for_agent(agent, &path) {
                Ok(message) => message,
                Err(err) => {
                    self.poison_message(agent, &path, &format!("{err:#}"))?;
                    continue;
                }
            };
            if message.delivery.state != state {
                self.poison_message(
                    agent,
                    &path,
                    &format!(
                        "Message {} is in inbox/{} but delivery.state is `{}`",
                        path.display(),
                        state.directory_name(),
                        message.delivery.state
                    ),
                )?;
                continue;
            }
            if &message.scope != scope {
                continue;
            }

            match self.claim_path(agent, &path, message, claimed_by, lease)? {
                Some(claimed) => return Ok(Some(claimed)),
                None => continue,
            }
        }

        Ok(None)
    }

    fn claim_path(
        &self,
        agent: &AgentId,
        path: &Path,
        mut message: Message,
        claimed_by: &AgentId,
        lease: MessageLease,
    ) -> Result<Option<ClaimedMessage>> {
        let claimed_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Claimed);
        fs::create_dir_all(&claimed_dir).with_context(|| {
            format!("Failed to create claimed inbox: {}", claimed_dir.display())
        })?;
        let claimed_path = exact_state_path(&claimed_dir, path)?;
        if claimed_path.exists() {
            self.poison_message(
                agent,
                path,
                &format!(
                    "Message {} already exists in inbox/claimed",
                    claimed_path.display()
                ),
            )?;
            return Ok(None);
        }

        match fs::rename(path, &claimed_path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to claim message: {} -> {}",
                        path.display(),
                        claimed_path.display()
                    )
                });
            }
        }

        message.mark_claimed(claimed_by, lease.expires_at(SystemTime::now()));
        write_message(&claimed_path, &message, "claimed")?;
        Ok(Some(ClaimedMessage {
            original_path: path.to_path_buf(),
            claimed_path,
            message,
        }))
    }

    pub fn acknowledge_delivery(
        &self,
        agent: &str,
        claimed_by: &str,
        message_id: &str,
    ) -> Result<DeliveredMessage> {
        let (agent, claimed_path, mut message) =
            self.read_claimed_message(agent, claimed_by, message_id)?;
        let delivered_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Delivered);
        fs::create_dir_all(&delivered_dir).with_context(|| {
            format!(
                "Failed to create delivered inbox: {}",
                delivered_dir.display()
            )
        })?;
        let delivered_path = exact_state_path(&delivered_dir, &claimed_path)?;
        move_message_exact(&claimed_path, &delivered_path, "deliver message")?;

        message.mark_delivered();
        write_message(&delivered_path, &message, "delivered")?;
        Ok(DeliveredMessage {
            original_path: claimed_path,
            delivered_path,
            message,
        })
    }

    pub fn retry_delivery(
        &self,
        agent: &str,
        claimed_by: &str,
        message_id: &str,
        error: &str,
    ) -> Result<RetriedMessage> {
        let error = normalized_delivery_error(error)?;
        let (agent, claimed_path, mut message) =
            self.read_claimed_message(agent, claimed_by, message_id)?;
        let retry_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Retry);
        fs::create_dir_all(&retry_dir)
            .with_context(|| format!("Failed to create retry inbox: {}", retry_dir.display()))?;
        let retry_path = exact_state_path(&retry_dir, &claimed_path)?;
        move_message_exact(&claimed_path, &retry_path, "retry message")?;

        message.mark_retry(&error);
        write_message(&retry_path, &message, "retry")?;
        Ok(RetriedMessage {
            original_path: claimed_path,
            retry_path,
            message,
        })
    }

    pub fn fail_delivery(
        &self,
        agent: &str,
        claimed_by: &str,
        message_id: &str,
        error: &str,
    ) -> Result<FailedMessage> {
        let error = normalized_delivery_error(error)?;
        let (agent, claimed_path, mut message) =
            self.read_claimed_message(agent, claimed_by, message_id)?;
        let failed_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Failed);
        fs::create_dir_all(&failed_dir)
            .with_context(|| format!("Failed to create failed inbox: {}", failed_dir.display()))?;
        let failed_path = exact_state_path(&failed_dir, &claimed_path)?;
        move_message_exact(&claimed_path, &failed_path, "fail message")?;

        message.mark_failed(&error, true);
        write_message(&failed_path, &message, "failed")?;
        Ok(FailedMessage {
            original_path: claimed_path,
            failed_path,
            message: Some(message),
        })
    }

    pub fn reclaim_expired_leases(
        &self,
        agent: &str,
        now: SystemTime,
    ) -> Result<Vec<ReclaimedMessage>> {
        let agent = AgentId::parse(agent)?;
        let claimed_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Claimed);
        let retry_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Retry);
        let now_seconds = unix_seconds(now);
        let mut reclaimed = Vec::new();

        for path in message_paths(&claimed_dir)? {
            let mut message = match read_message_for_agent(&agent, &path) {
                Ok(message) => message,
                Err(err) => {
                    self.poison_message(&agent, &path, &format!("{err:#}"))?;
                    continue;
                }
            };
            if message.delivery.state != MessageDeliveryState::Claimed {
                self.poison_message(
                    &agent,
                    &path,
                    &format!(
                        "Message {} is in inbox/claimed but delivery.state is `{}`",
                        path.display(),
                        message.delivery.state
                    ),
                )?;
                continue;
            }
            let lease_expires_at = message
                .delivery
                .lease_expires_at
                .clone()
                .unwrap_or_default();
            let expires_at = match parse_utc_timestamp(&lease_expires_at) {
                Ok(expires_at) => expires_at,
                Err(err) => {
                    self.poison_message(
                        &agent,
                        &path,
                        &format!(
                            "Message {} has invalid delivery.lease_expires_at `{lease_expires_at}`: {err}",
                            path.display()
                        ),
                    )?;
                    continue;
                }
            };
            if expires_at > now_seconds {
                continue;
            }

            fs::create_dir_all(&retry_dir).with_context(|| {
                format!("Failed to create retry inbox: {}", retry_dir.display())
            })?;
            let retry_path = exact_state_path(&retry_dir, &path)?;
            move_message_exact(&path, &retry_path, "reclaim expired message lease")?;
            message.mark_retry(&format!("lease expired at {lease_expires_at}"));
            write_message(&retry_path, &message, "retry")?;
            reclaimed.push(ReclaimedMessage {
                original_path: path,
                retry_path,
                message,
            });
        }

        Ok(reclaimed)
    }

    fn read_claimed_message(
        &self,
        agent: &str,
        claimed_by: &str,
        message_id: &str,
    ) -> Result<(AgentId, PathBuf, Message)> {
        let agent = AgentId::parse(agent)?;
        let claimed_by =
            AgentId::parse(claimed_by).context("Invalid delivery claimant agent id")?;
        let file_name = message_file_name(message_id)?;
        let claimed_path = agent
            .inbox_state_dir(&self.root, MessageDeliveryState::Claimed)
            .join(file_name);
        let message = read_message_for_agent(&agent, &claimed_path)?;
        if message.delivery.state != MessageDeliveryState::Claimed {
            bail!(
                "Message {} is in inbox/claimed but delivery.state is `{}`",
                claimed_path.display(),
                message.delivery.state
            );
        }
        if message.delivery.claimed_by.as_deref() != Some(claimed_by.as_str()) {
            bail!(
                "Message {} is claimed by {}, not {}",
                claimed_path.display(),
                message
                    .delivery
                    .claimed_by
                    .as_deref()
                    .unwrap_or("<missing>"),
                claimed_by.as_str()
            );
        }
        Ok((agent, claimed_path, message))
    }

    fn poison_message(
        &self,
        agent: &AgentId,
        path: &Path,
        error: &str,
    ) -> Result<Option<FailedMessage>> {
        let error = normalized_delivery_error(error)?;
        let failed_dir = agent.inbox_state_dir(&self.root, MessageDeliveryState::Failed);
        fs::create_dir_all(&failed_dir)
            .with_context(|| format!("Failed to create failed inbox: {}", failed_dir.display()))?;
        let failed_path = next_state_path(&failed_dir, path)?;
        let parsed = fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str::<Message>(&content).ok());

        match fs::rename(path, &failed_path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to move poison message to failed inbox: {} -> {}",
                        path.display(),
                        failed_path.display()
                    )
                });
            }
        }

        let mut message = parsed;
        if let Some(message) = message.as_mut() {
            message.mark_failed(&error, false);
            write_message(&failed_path, message, "failed poison")?;
        }

        Ok(Some(FailedMessage {
            original_path: path.to_path_buf(),
            failed_path,
            message,
        }))
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
            let message = match read_message_for_agent(&agent, &path) {
                Ok(message) => message,
                Err(err) => {
                    self.poison_message(&agent, &path, &format!("{err:#}"))?;
                    continue;
                }
            };
            if message.delivery.state != MessageDeliveryState::New {
                self.poison_message(
                    &agent,
                    &path,
                    &format!(
                        "Message {} is in inbox/new but delivery.state is `{}`",
                        path.display(),
                        message.delivery.state
                    ),
                )?;
                continue;
            }
            if message.scope.kind != MessageScopeKind::Direct {
                continue;
            }
            pending.push((path, message));
        }
        if pending.is_empty() {
            return Ok(InboxDelivery {
                agent,
                messages: Vec::new(),
            });
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

fn validate_scope_value(scope: &MessageScope) -> Result<()> {
    match scope.kind {
        MessageScopeKind::Direct | MessageScopeKind::Repo => {
            if scope.id.is_some() {
                bail!("scope.kind `{}` must not set scope.id", scope.kind.as_str());
            }
        }
        MessageScopeKind::Workflow | MessageScopeKind::TaskRun => {
            if scope.id.as_deref().unwrap_or_default().trim().is_empty() {
                bail!("scope.kind `{}` requires scope.id", scope.kind.as_str());
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

fn normalized_delivery_error(error: &str) -> Result<String> {
    let error = error.trim();
    if error.is_empty() {
        bail!("Delivery error cannot be empty");
    }
    Ok(error.into())
}

fn message_file_name(message_id: &str) -> Result<String> {
    let id = message_id.trim();
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.starts_with('.')
    {
        bail!("Invalid message id `{message_id}`");
    }
    Ok(format!("{id}.toml"))
}

fn exact_state_path(state_dir: &Path, original_path: &Path) -> Result<PathBuf> {
    let file_name = original_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!("Message file has no file name: {}", original_path.display())
        })?;
    Ok(state_dir.join(file_name))
}

fn move_message_exact(from: &Path, to: &Path, action: &str) -> Result<()> {
    if to.exists() {
        bail!(
            "Cannot {action}: target message already exists at {}",
            to.display()
        );
    }
    fs::rename(from, to)
        .with_context(|| format!("Failed to {action}: {} -> {}", from.display(), to.display()))
}

fn write_message(path: &Path, message: &Message, state_name: &str) -> Result<()> {
    let content = toml::to_string_pretty(message)
        .with_context(|| format!("Failed to serialize {state_name} message TOML"))?;
    fs::write(path, content)
        .with_context(|| format!("Failed to update {state_name} message: {}", path.display()))
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
    timestamp_from_system_time(SystemTime::now())
}

fn timestamp_from_system_time(time: SystemTime) -> String {
    let seconds = unix_seconds(time);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(err) => -(err.duration().as_secs() as i64),
    }
}

fn parse_utc_timestamp(value: &str) -> Result<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        bail!("expected UTC timestamp formatted as YYYY-MM-DDTHH:MM:SSZ");
    }

    let year = parse_i32_digits(&bytes[0..4], "year")?;
    let month = parse_u32_digits(&bytes[5..7], "month")?;
    let day = parse_u32_digits(&bytes[8..10], "day")?;
    let hour = parse_u32_digits(&bytes[11..13], "hour")?;
    let minute = parse_u32_digits(&bytes[14..16], "minute")?;
    let second = parse_u32_digits(&bytes[17..19], "second")?;

    if !(1..=12).contains(&month) {
        bail!("month out of range");
    }
    if hour > 23 {
        bail!("hour out of range");
    }
    if minute > 59 {
        bail!("minute out of range");
    }
    if second > 59 {
        bail!("second out of range");
    }

    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) {
        bail!("day out of range");
    }

    Ok(days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second))
}

fn parse_i32_digits(value: &[u8], field: &str) -> Result<i32> {
    Ok(parse_u32_digits(value, field)? as i32)
}

fn parse_u32_digits(value: &[u8], field: &str) -> Result<u32> {
    if !value.iter().all(|byte| byte.is_ascii_digit()) {
        bail!("{field} contains non-digit characters");
    }
    Ok(value
        .iter()
        .fold(0_u32, |acc, byte| acc * 10 + u32::from(byte - b'0')))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 }.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (month + if month > 2 { -3 } else { 9 }) + 2).div_euclid(5) + i64::from(day) - 1;
    let day_of_era =
        year_of_era * 365 + year_of_era.div_euclid(4) - year_of_era.div_euclid(100) + day_of_year;
    era * 146_097 + day_of_era - 719_468
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
    use std::sync::{Arc, Barrier};
    use std::thread;
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
    fn claim_next_moves_matching_scope_to_claimed_with_lease_metadata() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let scope = MessageScope::workflow("2026-05-20-001").unwrap();
        let scoped = store
            .send_scoped_from(
                "agents/worker",
                "agents/coordinator",
                scope.clone(),
                "workflow owned",
            )
            .unwrap();
        let direct = store
            .send_from("agents/worker", "agents/coordinator", "direct")
            .unwrap();

        let claimed = store
            .claim_next(
                "agents/coordinator",
                &scope,
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();

        assert_eq!(claimed.message.meta.id, scoped.id);
        assert!(!scoped.path.exists());
        assert!(claimed.claimed_path.exists());
        assert!(direct.path.exists());
        assert_eq!(
            claimed.message.delivery.state,
            MessageDeliveryState::Claimed
        );
        assert_eq!(
            claimed.message.delivery.claimed_by.as_deref(),
            Some("agents/supervisor")
        );
        assert!(claimed.message.delivery.lease_expires_at.is_some());
        assert_eq!(claimed.message.delivery.attempts, 0);

        let persisted = read_message(&claimed.claimed_path);
        assert_eq!(persisted.delivery.state, MessageDeliveryState::Claimed);
        assert_eq!(persisted.scope, scope);
    }

    #[test]
    fn acknowledge_delivery_moves_claim_to_delivered() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let sent = store
            .send_from("agents/claude", "agents/codex", "delivered content")
            .unwrap();
        let claimed = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();

        let delivered = store
            .acknowledge_delivery("agents/codex", "agents/supervisor", &sent.id)
            .unwrap();

        assert!(!claimed.claimed_path.exists());
        assert!(delivered.delivered_path.exists());
        assert_eq!(
            delivered.message.delivery.state,
            MessageDeliveryState::Delivered
        );
        assert_eq!(delivered.message.delivery.attempts, 1);
        assert_eq!(delivered.message.delivery.claimed_by, None);
        assert_eq!(delivered.message.delivery.lease_expires_at, None);
        assert_eq!(delivered.message.text_content(), "delivered content");
    }

    #[test]
    fn retry_and_fail_claims_preserve_content_and_attempts() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let sent = store
            .send_from("agents/claude", "agents/codex", "recoverable content")
            .unwrap();
        let first_claim = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();

        let retried = store
            .retry_delivery(
                "agents/codex",
                "agents/supervisor",
                &sent.id,
                "cmux send failed",
            )
            .unwrap();

        assert!(!first_claim.claimed_path.exists());
        assert!(retried.retry_path.exists());
        assert_eq!(retried.message.delivery.state, MessageDeliveryState::Retry);
        assert_eq!(retried.message.delivery.attempts, 1);
        assert_eq!(
            retried.message.delivery.last_error.as_deref(),
            Some("cmux send failed")
        );
        assert_eq!(retried.message.text_content(), "recoverable content");

        let second_claim = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(second_claim.message.delivery.attempts, 1);
        assert_eq!(second_claim.message.delivery.last_error, None);

        let failed = store
            .fail_delivery(
                "agents/codex",
                "agents/supervisor",
                &sent.id,
                "retry budget exhausted",
            )
            .unwrap();
        let failed_message = failed.message.as_ref().unwrap();
        assert!(!second_claim.claimed_path.exists());
        assert!(failed.failed_path.exists());
        assert_eq!(failed_message.delivery.state, MessageDeliveryState::Failed);
        assert_eq!(failed_message.delivery.attempts, 2);
        assert_eq!(
            failed_message.delivery.last_error.as_deref(),
            Some("retry budget exhausted")
        );
        assert_eq!(failed_message.text_content(), "recoverable content");
    }

    #[test]
    fn expired_claim_leases_are_reclaimed_to_retry() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let sent = store
            .send_from("agents/claude", "agents/codex", "lease content")
            .unwrap();
        let claimed = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();
        let mut claimed_message = read_message(&claimed.claimed_path);
        claimed_message.delivery.lease_expires_at = Some("1970-01-01T00:00:01Z".into());
        fs::write(
            &claimed.claimed_path,
            toml::to_string_pretty(&claimed_message).unwrap(),
        )
        .unwrap();

        let reclaimed = store
            .reclaim_expired_leases("agents/codex", UNIX_EPOCH + Duration::from_secs(2))
            .unwrap();

        assert_eq!(reclaimed.len(), 1);
        assert_eq!(reclaimed[0].message.meta.id, sent.id);
        assert!(!claimed.claimed_path.exists());
        assert!(reclaimed[0].retry_path.exists());
        assert_eq!(
            reclaimed[0].message.delivery.state,
            MessageDeliveryState::Retry
        );
        assert_eq!(reclaimed[0].message.delivery.attempts, 1);
        assert!(
            reclaimed[0]
                .message
                .delivery
                .last_error
                .as_deref()
                .unwrap()
                .contains("lease expired")
        );

        let claimed_again = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed_again.message.meta.id, sent.id);
        assert_eq!(claimed_again.message.delivery.attempts, 1);
    }

    #[test]
    fn concurrent_claim_allows_only_one_consumer_to_deliver() {
        let temp = TempDir::new().unwrap();
        let store = Arc::new(MessageStore::new(temp.path().join("messages")));
        let sent = store
            .send_from("agents/claude", "agents/codex", "claim once")
            .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();

        for index in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                store
                    .claim_next(
                        "agents/codex",
                        &MessageScope::direct(),
                        &format!("agents/consumer-{index}"),
                        MessageLease::new(Duration::from_secs(60)).unwrap(),
                    )
                    .unwrap()
            }));
        }

        let claims = handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(claims.len(), 1);

        let claimant = claims[0].message.delivery.claimed_by.as_deref().unwrap();
        store
            .acknowledge_delivery("agents/codex", claimant, &sent.id)
            .unwrap();

        let inbox = temp.path().join("messages/agents/codex/inbox");
        assert!(message_paths(&inbox.join("new")).unwrap().is_empty());
        assert!(message_paths(&inbox.join("claimed")).unwrap().is_empty());
        assert_eq!(message_paths(&inbox.join("delivered")).unwrap().len(), 1);
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
    fn check_inbox_skips_scoped_messages_and_delivers_direct_messages() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));
        let scoped = store
            .send_scoped_from(
                "agents/claude",
                "agents/coordinator",
                MessageScope::workflow("2026-05-20-001").unwrap(),
                "workflow scoped",
            )
            .unwrap();
        let direct = store
            .send_from("agents/claude", "agents/coordinator", "direct")
            .unwrap();

        let delivery = store.check_inbox("agents/coordinator").unwrap();

        assert_eq!(delivery.messages.len(), 1);
        assert_eq!(delivery.messages[0].message.meta.id, direct.id);
        assert!(scoped.path.exists());
        assert!(!direct.path.exists());
    }

    #[test]
    fn check_inbox_moves_poison_messages_to_failed_without_blocking_valid_messages() {
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
        invalid.body.parts[0].part_type = "unsupported".into();
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

        let delivery = store.check_inbox("codex").unwrap();

        assert_eq!(delivery.messages.len(), 1);
        assert_eq!(delivery.messages[0].message.meta.id, "a-valid");
        assert!(!new_dir.join("a-valid.toml").exists());
        assert!(!new_dir.join("z-invalid.toml").exists());
        assert_eq!(
            read_message(&root.join("agents/codex/inbox/delivered/a-valid.toml"))
                .delivery
                .state,
            MessageDeliveryState::Delivered
        );
        let failed = read_message(&root.join("agents/codex/inbox/failed/z-invalid.toml"));
        assert_eq!(failed.delivery.state, MessageDeliveryState::Failed);
        assert!(
            failed
                .delivery
                .last_error
                .as_deref()
                .unwrap()
                .contains("unsupported body part type")
        );
    }

    #[test]
    fn claim_poison_invalid_toml_without_blocking_valid_messages() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("messages");
        let store = MessageStore::new(&root);
        let new_dir = root.join("agents/codex/inbox/new");
        fs::create_dir_all(&new_dir).unwrap();
        fs::write(new_dir.join("a-invalid.toml"), "not = [valid\n").unwrap();
        let valid = store
            .send_from("agents/claude", "agents/codex", "valid after poison")
            .unwrap();

        let claimed = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();

        assert_eq!(claimed.message.meta.id, valid.id);
        assert!(!new_dir.join("a-invalid.toml").exists());
        let failed_files = message_paths(&root.join("agents/codex/inbox/failed")).unwrap();
        assert_eq!(failed_files.len(), 1);
        assert!(
            fs::read_to_string(&failed_files[0])
                .unwrap()
                .contains("not = [valid")
        );
    }

    #[test]
    fn claim_poison_wrong_target_without_blocking_valid_messages() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("messages");
        let store = MessageStore::new(&root);
        let new_dir = root.join("agents/codex/inbox/new");
        fs::create_dir_all(&new_dir).unwrap();
        let from = AgentId::parse("agents/claude").unwrap();
        let wrong_to = AgentId::parse("agents/other").unwrap();
        let codex = AgentId::parse("agents/codex").unwrap();
        let wrong = Message::new(
            "a-wrong".into(),
            "2026-05-20T12:00:00Z".into(),
            from.clone(),
            wrong_to,
            "wrong target",
        );
        let valid = Message::new(
            "b-valid".into(),
            "2026-05-20T12:00:00Z".into(),
            from,
            codex,
            "valid target",
        );
        fs::write(
            new_dir.join("a-wrong.toml"),
            toml::to_string_pretty(&wrong).unwrap(),
        )
        .unwrap();
        fs::write(
            new_dir.join("b-valid.toml"),
            toml::to_string_pretty(&valid).unwrap(),
        )
        .unwrap();

        let claimed = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap()
            .unwrap();

        assert_eq!(claimed.message.meta.id, "b-valid");
        let failed = read_toml_message(&root.join("agents/codex/inbox/failed/a-wrong.toml"));
        assert_eq!(failed["delivery"]["state"].as_str(), Some("failed"));
        assert!(
            failed["delivery"]["last_error"]
                .as_str()
                .unwrap()
                .contains("addressed to agents/other")
        );
    }

    #[test]
    fn claim_poison_unsupported_part_type_to_failed() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("messages");
        let store = MessageStore::new(&root);
        let new_dir = root.join("agents/codex/inbox/new");
        fs::create_dir_all(&new_dir).unwrap();
        let mut message = Message::new(
            "unsupported".into(),
            "2026-05-20T12:00:00Z".into(),
            AgentId::parse("agents/claude").unwrap(),
            AgentId::parse("agents/codex").unwrap(),
            "image payload",
        );
        message.body.parts[0].part_type = "image".into();
        fs::write(
            new_dir.join("unsupported.toml"),
            toml::to_string_pretty(&message).unwrap(),
        )
        .unwrap();

        let claimed = store
            .claim_next(
                "agents/codex",
                &MessageScope::direct(),
                "agents/supervisor",
                MessageLease::new(Duration::from_secs(60)).unwrap(),
            )
            .unwrap();

        assert!(claimed.is_none());
        let failed = read_toml_message(&root.join("agents/codex/inbox/failed/unsupported.toml"));
        assert_eq!(failed["delivery"]["state"].as_str(), Some("failed"));
        assert_eq!(failed["body"]["parts"][0]["type"].as_str(), Some("image"));
        assert!(
            failed["delivery"]["last_error"]
                .as_str()
                .unwrap()
                .contains("unsupported body part type `image`")
        );
    }

    #[test]
    fn empty_inbox_has_no_delivery() {
        let temp = TempDir::new().unwrap();
        let store = MessageStore::new(temp.path().join("messages"));

        let delivery = store.check_inbox("codex").unwrap();

        assert!(delivery.is_empty());
    }

    fn read_message(path: &Path) -> Message {
        let content = fs::read_to_string(path).unwrap();
        toml::from_str(&content).unwrap()
    }

    fn read_toml_message(path: &Path) -> toml::Value {
        let content = fs::read_to_string(path).unwrap();
        toml::from_str(&content).unwrap()
    }
}
