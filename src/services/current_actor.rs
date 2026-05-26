use crate::context::Ctx;
use crate::messages::AgentId;
use crate::services::identity_locator::{self, AnchorKey, AnchorKind};
use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub(crate) fn resolve_launch_coordinator(ctx: &Ctx) -> Result<AgentId> {
    if let Some(value) = ctx.launcher_coordinator_id.as_deref() {
        return AgentId::parse(value)
            .context("Invalid launch coordinator agent id from WT_AGENT_ID or identity anchor");
    }

    match identity_locator::resolve_identity(ctx) {
        Ok(Some(anchor)) => {
            return AgentId::parse(&anchor.id)
                .context("Invalid launch coordinator agent id from live identity anchor");
        }
        Ok(None) => {}
        Err(err) => {
            return Err(err).context(
                "Failed to resolve launch coordinator from live identity anchor before auto-create",
            );
        }
    }

    let key = identity_locator::current_anchor_key().with_context(|| {
        "Could not resolve launch coordinator agent id. Tried WT_AGENT_ID, live identity anchor, and auto-created identity anchor for the current terminal or agent anchor."
    })?;
    let generated = generated_agent_id_for_anchor(&key)?;
    let anchor = identity_locator::write_identity_anchor(
        ctx,
        &key,
        generated.as_str(),
        identity_locator::current_agent_kind().as_deref(),
    )
    .with_context(|| {
        format!(
            "Failed to auto-create launch coordinator identity anchor for {}",
            key.display()
        )
    })?;
    AgentId::parse(&anchor.id).context("Invalid auto-created launch coordinator agent id")
}

fn generated_agent_id_for_anchor(key: &AnchorKey) -> Result<AgentId> {
    let name = format!(
        "{}-{}",
        generated_anchor_prefix(&key.kind),
        short_anchor_hash(&key.display())
    );
    AgentId::parse(&format!("agents/{name}"))
}

fn generated_anchor_prefix(kind: &AnchorKind) -> &'static str {
    match kind {
        AnchorKind::Surface => "surface",
        AnchorKind::ClaudeSession => "claude",
        AnchorKind::CodexThread => "codex",
        AnchorKind::ShellSid => "shell",
    }
}

fn short_anchor_hash(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())[..12].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions};
    use crate::storage::StorageRoot;

    #[test]
    fn launch_coordinator_prefers_existing_context_agent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new_with_options(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                storage_root: Some(StorageRoot::from_git_common_dir(dir.path().join(".git"))),
                launcher_coordinator_id: Some("agents/coord-a".into()),
                ..CtxOptions::default()
            },
        );

        let agent = resolve_launch_coordinator(&ctx).unwrap();

        assert_eq!(agent.as_str(), "agents/coord-a");
    }

    #[test]
    fn launch_coordinator_auto_creates_identity_anchor_for_current_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new_with_options(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                storage_root: Some(StorageRoot::from_git_common_dir(dir.path().join(".git"))),
                ..CtxOptions::default()
            },
        );

        let agent = resolve_launch_coordinator(&ctx).unwrap();
        let key = identity_locator::current_anchor_key().unwrap();
        let anchor = identity_locator::read_identity_anchor(&ctx, &key)
            .unwrap()
            .unwrap();

        assert_eq!(anchor.id, agent.as_str());
        assert!(
            agent.as_str().starts_with("agents/surface-")
                || agent.as_str().starts_with("agents/claude-")
                || agent.as_str().starts_with("agents/codex-")
                || agent.as_str().starts_with("agents/shell-")
        );
    }

    #[test]
    fn generated_anchor_agent_ids_are_flat() {
        let key = AnchorKey {
            kind: AnchorKind::Surface,
            value: "workspace:6/surface:27".into(),
        };

        let agent = generated_agent_id_for_anchor(&key).unwrap();

        assert!(agent.as_str().starts_with("agents/surface-"));
        assert!(!agent.as_str().trim_start_matches("agents/").contains('/'));
    }
}
