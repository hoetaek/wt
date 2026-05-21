use crate::context::Ctx;
use crate::messages::{
    AgentId, COORDINATOR_AGENT_ALIAS, HookOutput, Message, MessageDeliveryState,
    MessageInspectionRecord, MessageInventory, MessageInventoryCounts, MessageScope, MessageStore,
};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::io::Write;

pub(crate) fn send(ctx: &Ctx, to: &str, scope: Option<&str>, message: &[String]) -> Result<()> {
    let text = message.join(" ");
    if text.trim().is_empty() {
        bail!("Message cannot be empty");
    }

    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let scope = scope.map(parse_scope_arg).transpose()?;
    let to = resolve_agent_arg(to).context("Invalid target agent id")?;
    let sent = match scope {
        Some(scope) => store.send_scoped(to.as_str(), scope, &text)?,
        None => store.send(to.as_str(), &text)?,
    };

    if !ctx.quiet {
        println!("{}", ctx.storage_root.display_path(&sent.path));
    }

    Ok(())
}

fn parse_scope_arg(scope: &str) -> Result<MessageScope> {
    let raw = scope.trim();
    if raw.is_empty() {
        bail!("Message scope cannot be empty. Use direct, repo, workflow:<id>, or task_run:<id>.");
    }

    match raw {
        "direct" => return Ok(MessageScope::direct()),
        "repo" => return Ok(MessageScope::repo()),
        "workflow" => bail!("Message scope `workflow` requires an id. Use workflow:<id>."),
        "task_run" => bail!("Message scope `task_run` requires an id. Use task_run:<id>."),
        _ => {}
    }

    let Some((kind, id)) = raw.split_once(':') else {
        bail!("Invalid message scope `{raw}`. Use direct, repo, workflow:<id>, or task_run:<id>.");
    };
    let id = id.trim();
    if id.is_empty() {
        bail!("Message scope `{kind}` requires a non-empty id");
    }
    match kind {
        "workflow" => MessageScope::workflow(id),
        "task_run" => MessageScope::task_run(id),
        "direct" | "repo" => {
            bail!("Message scope `{kind}` must not include an id. Use `{kind}`.")
        }
        _ => bail!(
            "Invalid message scope kind `{kind}`. Use direct, repo, workflow:<id>, or task_run:<id>."
        ),
    }
}

pub(crate) fn list(ctx: &Ctx, agent: &str) -> Result<()> {
    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let agent = resolve_agent_arg(agent).context("Invalid agent id")?;
    let inventory = store.list(agent.as_str())?;
    let report = MessageListReport::from_inventory(ctx, inventory);

    if ctx.is_json() {
        write_json(&report)?;
    } else {
        print_list(&report);
    }

    Ok(())
}

pub(crate) fn read(ctx: &Ctx, agent: &str, message_id: &str) -> Result<()> {
    let message_id = canonical_read_message_id(message_id)?;
    let agent = resolve_agent_arg(agent).context("Invalid agent id")?;
    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let record = store.read_for_inspection(agent.as_str(), message_id)?;
    let row = MessageRow::from_record(ctx, record);
    let report = MessageReadReport {
        agent: agent.as_str().into(),
        message: row.message.clone(),
        record: row,
    };

    if ctx.is_json() {
        write_json(&report)?;
    } else {
        print_read(&report);
    }

    Ok(())
}

fn canonical_read_message_id(message_id: &str) -> Result<&str> {
    let id = message_id.trim();
    if id.to_ascii_lowercase().ends_with(".toml") {
        bail!("Message id must not include the .toml extension; pass the message id only");
    }
    Ok(id)
}

pub(crate) fn check_inbox(ctx: &Ctx, agent: Option<&str>) -> Result<()> {
    let agents = match agent {
        Some(agent) => vec![
            resolve_agent_arg(agent)
                .context("Invalid agent id")?
                .as_str()
                .to_string(),
        ],
        None => inbox_agents_from_env()?,
    };
    if agents.is_empty() {
        return Ok(());
    }

    let store = MessageStore::new(ctx.storage_root.messages_dir());
    for agent in agents {
        let delivery = store.check_inbox(&agent)?;
        if delivery.is_empty() {
            continue;
        }

        let output = HookOutput::new("UserPromptSubmit", delivery.additional_context());
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer(&mut handle, &output)?;
        writeln!(handle)?;
        handle.flush()?;
        store.acknowledge_inbox_delivery(&delivery)?;
    }
    Ok(())
}

fn resolve_agent_arg(input: &str) -> Result<AgentId> {
    if input.trim() == COORDINATOR_AGENT_ALIAS {
        return coordinator_agent_from_env();
    }
    AgentId::parse(input)
}

fn coordinator_agent_from_env() -> Result<AgentId> {
    match env::var("WT_COORDINATOR_AGENT_ID") {
        Ok(value) if value.trim().is_empty() => bail!(coordinator_alias_error()),
        Ok(value) => AgentId::parse(&value)
            .map_err(|err| anyhow::anyhow!("Invalid WT_COORDINATOR_AGENT_ID: {err:#}")),
        Err(env::VarError::NotPresent) => bail!(coordinator_alias_error()),
        Err(env::VarError::NotUnicode(_)) => {
            bail!("Invalid WT_COORDINATOR_AGENT_ID: value is not Unicode")
        }
    }
}

fn coordinator_alias_error() -> &'static str {
    "The `coordinator` alias requires WT_COORDINATOR_AGENT_ID. Run `wt coord use <id>` in the coordinator shell or enable ambient binding with `eval \"$(wt shell-init zsh)\"`."
}

fn inbox_agents_from_env() -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut agents = Vec::new();
    for key in ["WT_AGENT_ID", "WT_COORDINATOR_AGENT_ID"] {
        match env::var(key) {
            Ok(value) => {
                if value.is_empty() || !seen.insert(value.clone()) {
                    continue;
                }
                agents.push(value);
            }
            Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_)) => bail!("Invalid {key}: value is not Unicode"),
        }
    }
    Ok(agents)
}

#[derive(Debug, Serialize)]
struct MessageListReport {
    agent: String,
    counts: MessageCounts,
    messages: Vec<MessageRow>,
}

impl MessageListReport {
    fn from_inventory(ctx: &Ctx, inventory: MessageInventory) -> Self {
        Self {
            agent: inventory.agent.as_str().into(),
            counts: MessageCounts::from_counts(inventory.counts),
            messages: inventory
                .messages
                .into_iter()
                .map(|record| MessageRow::from_record(ctx, record))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct MessageReadReport {
    agent: String,
    message: Option<Message>,
    record: MessageRow,
}

#[derive(Debug, Serialize)]
struct MessageCounts {
    total: usize,
    new: usize,
    claimed: usize,
    delivered: usize,
    retry: usize,
    failed: usize,
    invalid: usize,
}

impl MessageCounts {
    fn from_counts(counts: MessageInventoryCounts) -> Self {
        Self {
            total: counts.total,
            new: counts.new,
            claimed: counts.claimed,
            delivered: counts.delivered,
            retry: counts.retry,
            failed: counts.failed,
            invalid: counts.invalid,
        }
    }
}

#[derive(Debug, Serialize)]
struct MessageRow {
    id: String,
    state: MessageDeliveryState,
    path: String,
    valid: bool,
    error: Option<String>,
    from: Option<String>,
    to: Option<String>,
    scope: Option<MessageScopeSummary>,
    delivery_state: Option<MessageDeliveryState>,
    attempts: Option<u32>,
    claimed_by: Option<String>,
    lease_expires_at: Option<String>,
    last_error: Option<String>,
    summary: Option<String>,
    #[serde(skip_serializing)]
    message: Option<Message>,
}

impl MessageRow {
    fn from_record(ctx: &Ctx, record: MessageInspectionRecord) -> Self {
        let message = record.message;
        let scope = message.as_ref().map(|message| MessageScopeSummary {
            kind: message.scope.kind.as_str().into(),
            id: message.scope.id.clone(),
        });
        Self {
            id: record.id,
            state: record.state,
            path: ctx.storage_root.display_path(&record.path),
            valid: record.error.is_none(),
            error: record.error,
            from: message.as_ref().map(|message| message.meta.from.clone()),
            to: message.as_ref().map(|message| message.meta.to.clone()),
            scope,
            delivery_state: message.as_ref().map(|message| message.delivery.state),
            attempts: message.as_ref().map(|message| message.delivery.attempts),
            claimed_by: message
                .as_ref()
                .and_then(|message| message.delivery.claimed_by.clone()),
            lease_expires_at: message
                .as_ref()
                .and_then(|message| message.delivery.lease_expires_at.clone()),
            last_error: message
                .as_ref()
                .and_then(|message| message.delivery.last_error.clone()),
            summary: message.as_ref().map(|message| message.body.summary.clone()),
            message,
        }
    }
}

#[derive(Debug, Serialize)]
struct MessageScopeSummary {
    kind: String,
    id: Option<String>,
}

fn print_list(report: &MessageListReport) {
    println!(
        "{} messages: total {} (new {}, claimed {}, delivered {}, retry {}, failed {}, invalid {})",
        report.agent,
        report.counts.total,
        report.counts.new,
        report.counts.claimed,
        report.counts.delivered,
        report.counts.retry,
        report.counts.failed,
        report.counts.invalid
    );

    for row in &report.messages {
        println!("{}", list_row_text(row));
    }
}

fn list_row_text(row: &MessageRow) -> String {
    let mut parts = vec![row.state.as_str().to_string(), row.id.clone()];
    if let Some(from) = row.from.as_deref() {
        parts.push(format!("from={from}"));
    }
    if let Some(scope) = row.scope.as_ref() {
        parts.push(format!("scope={}", scope_label(scope)));
    }
    if let Some(attempts) = row.attempts {
        parts.push(format!("attempts={attempts}"));
    }
    if let Some(claimed_by) = row.claimed_by.as_deref() {
        parts.push(format!("claimed_by={claimed_by}"));
    }
    if let Some(lease_expires_at) = row.lease_expires_at.as_deref() {
        parts.push(format!("lease_expires_at={lease_expires_at}"));
    }
    if let Some(last_error) = row.last_error.as_deref() {
        parts.push(format!(
            "last_error={}",
            quoted(&truncate_chars(&one_line(last_error), 120))
        ));
    }
    if let Some(error) = row.error.as_deref() {
        parts.push(format!(
            "error={}",
            quoted(&truncate_chars(&one_line(error), 160))
        ));
    }
    if let Some(summary) = row.summary.as_deref() {
        parts.push(format!(
            "summary={}",
            quoted(&truncate_chars(&one_line(summary), 120))
        ));
    }
    parts.join(" ")
}

fn print_read(report: &MessageReadReport) {
    let row = &report.record;
    println!("id: {}", row.id);
    println!("state: {}", row.state);
    println!("path: {}", row.path);
    println!("valid: {}", row.valid);
    if let Some(error) = row.error.as_deref() {
        println!("error: {}", one_line(error));
    }
    if let Some(message) = row.message.as_ref() {
        println!("from: {}", message.meta.from);
        println!("to: {}", message.meta.to);
        println!("scope: {}", scope_label_for_message(&message.scope));
        println!(
            "envelope: kind={} priority={} expects_response={}",
            message.envelope.kind, message.envelope.priority, message.envelope.expects_response
        );
        if let Some(correlates_with) = message.envelope.correlates_with.as_deref() {
            println!("correlates_with: {correlates_with}");
        }
        println!("delivery_state: {}", message.delivery.state);
        println!("attempts: {}", message.delivery.attempts);
        if let Some(claimed_by) = message.delivery.claimed_by.as_deref() {
            println!("claimed_by: {claimed_by}");
        }
        if let Some(lease_expires_at) = message.delivery.lease_expires_at.as_deref() {
            println!("lease_expires_at: {lease_expires_at}");
        }
        if let Some(last_error) = message.delivery.last_error.as_deref() {
            println!("last_error: {}", one_line(last_error));
        }
        println!("summary: {}", message.body.summary);
        println!("content:");
        for line in message.text_content().lines() {
            println!("{line}");
        }
    }
}

fn write_json<T: Serialize>(report: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, report)?;
    writeln!(handle)?;
    Ok(())
}

fn scope_label(scope: &MessageScopeSummary) -> String {
    match scope.id.as_deref() {
        Some(id) => format!("{}:{id}", scope.kind),
        None => scope.kind.clone(),
    }
}

fn scope_label_for_message(scope: &MessageScope) -> String {
    match scope.id.as_deref() {
        Some(id) => format!("{}:{id}", scope.kind.as_str()),
        None => scope.kind.as_str().into(),
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap()
    }

    #[test]
    fn coordinator_alias_resolves_from_runtime_env() {
        let _guard = env_lock();
        unsafe {
            env::set_var("WT_COORDINATOR_AGENT_ID", "agents/foo");
        }

        let agent = resolve_agent_arg("coordinator").unwrap();

        assert_eq!(agent.as_str(), "agents/foo");
        unsafe {
            env::remove_var("WT_COORDINATOR_AGENT_ID");
        }
    }

    #[test]
    fn coordinator_alias_without_runtime_env_errors_with_setup_hint() {
        let _guard = env_lock();
        unsafe {
            env::remove_var("WT_COORDINATOR_AGENT_ID");
        }

        let err = resolve_agent_arg("coordinator").unwrap_err().to_string();

        assert!(err.contains("WT_COORDINATOR_AGENT_ID"));
        assert!(err.contains("wt coord use <id>"));
        assert!(err.contains("wt shell-init zsh"));
    }
}
