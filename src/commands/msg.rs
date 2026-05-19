use crate::context::Ctx;
use crate::messages::{HookOutput, MessageStore};
use anyhow::{Result, bail};
use std::io::Write;

pub(crate) fn send(ctx: &Ctx, to: &str, message: &[String]) -> Result<()> {
    let text = message.join(" ");
    if text.trim().is_empty() {
        bail!("Message cannot be empty");
    }

    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let sent = store.send(to, &text)?;

    if !ctx.quiet {
        println!("{}", ctx.storage_root.display_path(&sent.path));
    }

    Ok(())
}

pub(crate) fn check_inbox(ctx: &Ctx, agent: &str) -> Result<()> {
    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let delivery = store.check_inbox(agent)?;
    if delivery.is_empty() {
        return Ok(());
    }

    let output = HookOutput::new("UserPromptSubmit", delivery.additional_context());
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer(&mut handle, &output)?;
    writeln!(handle)?;
    Ok(())
}
