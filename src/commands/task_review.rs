use crate::context::Ctx;
use crate::messages::{AgentId, MessageScope, MessageStore};
use crate::services::current_actor;
use crate::task_run::{self, TaskReviewStatus, TaskRunRecord};
use anyhow::{Context, Result, bail};

pub(crate) fn run(
    ctx: &Ctx,
    task_run_id: &str,
    status: TaskReviewStatus,
    message: &[String],
) -> Result<()> {
    let from = current_actor::resolve_launch_coordinator(ctx)?;
    run_with_actor(ctx, task_run_id, status, message, &from)
}

fn run_with_actor(
    ctx: &Ctx,
    task_run_id: &str,
    status: TaskReviewStatus,
    message: &[String],
    from: &AgentId,
) -> Result<()> {
    let text = review_message(status, message)?;
    let record = resolve_review_task_run(ctx, task_run_id)?;
    let to = required_task_agent_id(&record)?;
    let scope = MessageScope::task_run(record.id.clone())?;

    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let sent = store.send_scoped_from(from.as_str(), to.as_str(), scope, &text)?;
    task_run::update_review_metadata(&record, status, &sent.id)?;

    if !ctx.quiet {
        println!("{}", ctx.storage_root.display_path(&sent.path));
    }

    Ok(())
}

fn resolve_review_task_run(ctx: &Ctx, task_run_id: &str) -> Result<TaskRunRecord> {
    let task_run_id = task_run_id.trim();
    if task_run_id.is_empty() {
        bail!("TaskRun id cannot be empty");
    }
    let path = task_run::resolve(ctx, task_run_id)?;
    let run = task_run::read(&path)?;
    Ok(TaskRunRecord {
        id: task_run::id_from_path(&path)?,
        path,
        run,
    })
}

fn required_task_agent_id(record: &TaskRunRecord) -> Result<AgentId> {
    let Some(agent_id) = record.run.agent_id.as_deref() else {
        bail!(
            "TaskRun {} is missing agent_id; it is a legacy or incomplete TaskRun and cannot receive review feedback through `wt task review`.",
            record.id
        );
    };
    AgentId::parse(agent_id)
        .with_context(|| format!("TaskRun {} has invalid agent_id: {agent_id}", record.id))
}

fn review_message(status: TaskReviewStatus, message: &[String]) -> Result<String> {
    let message = message.join(" ");
    let message = message.trim();
    if message.is_empty() {
        bail!("Review message cannot be empty");
    }
    Ok(format!(
        "Coordinator Review: Status={}; Message={message}",
        status.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions};
    use crate::storage::StorageRoot;
    use crate::task_run::STATUS_RUNNING;

    fn ctx(root: &std::path::Path) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions {
                storage_root: Some(StorageRoot::from_git_common_dir(root.join(".git"))),
                ..CtxOptions::default()
            },
        )
    }

    #[test]
    fn review_sends_task_run_scoped_feedback_and_records_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            STATUS_RUNNING,
        )
        .unwrap();
        let from = AgentId::parse("agents/coord-a").unwrap();

        run_with_actor(
            &ctx,
            &record.id,
            task_run::REVIEW_ACCEPTED,
            &["looks".into(), "good".into()],
            &from,
        )
        .unwrap();

        let updated = task_run::read(&record.path).unwrap();
        let message_id = updated.last_review_message_id.unwrap();
        assert_eq!(updated.last_review_status, Some(task_run::REVIEW_ACCEPTED));
        assert!(updated.last_reviewed_at.is_some());

        let store = MessageStore::new(ctx.storage_root.messages_dir());
        let message = store
            .read_for_inspection("agents/run-1-add-schema", &message_id)
            .unwrap()
            .message
            .unwrap();
        assert_eq!(message.meta.from, "agents/coord-a");
        assert_eq!(message.meta.to, "agents/run-1-add-schema");
        assert_eq!(message.scope.kind.as_str(), "task_run");
        assert_eq!(message.scope.id.as_deref(), Some(record.id.as_str()));
        assert_eq!(
            message.text_content(),
            "Coordinator Review: Status=accepted; Message=looks good"
        );
    }

    #[test]
    fn review_rejects_legacy_task_run_without_agent_id() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create(&ctx, "legacy", "legacy", None, STATUS_RUNNING).unwrap();
        let from = AgentId::parse("agents/coord-a").unwrap();

        let err = run_with_actor(
            &ctx,
            &record.id,
            task_run::REVIEW_REJECTED,
            &["needs changes".into()],
            &from,
        )
        .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("missing agent_id"));
        assert!(message.contains("wt task review"));
    }
}
