use crate::commands::msg::runtime_message_store;
use crate::context::Ctx;
use crate::messages::{AgentId, MessageScope};
use crate::services::current_actor;
use crate::services::inbox_wake;
use crate::task_run::{self, TaskReviewStatus, TaskRunRecord};
use crate::workflow::render::shell_arg;
use anyhow::{Context, Result, bail};

pub(crate) fn run(
    ctx: &Ctx,
    task_run_id: &str,
    status: TaskReviewStatus,
    message: &[String],
) -> Result<()> {
    let from = current_actor::resolve_launch_coordinator(ctx, None)?;
    run_with_actor(ctx, task_run_id, status, message, &from)
}

pub(super) fn run_with_actor(
    ctx: &Ctx,
    task_run_id: &str,
    status: TaskReviewStatus,
    message: &[String],
    from: &AgentId,
) -> Result<()> {
    let review_text = review_message_text(message)?;
    let record = resolve_review_task_run(ctx, task_run_id)?;
    validate_review_sender(&record, from, status, &review_text)?;
    let text = review_message(status, &review_text);
    let to = required_task_agent_id(&record)?;
    let scope = MessageScope::task_run(record.id.clone())?;

    let store = runtime_message_store(ctx)?;
    let sent = store.send_scoped_from(from.as_str(), to.as_str(), scope, &text)?;
    task_run::update_review_metadata(&record, status, &sent.id)?;
    let _wake_result = inbox_wake::wake_sent_message_recipient(ctx, &sent);

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

fn required_coordinator_id(record: &TaskRunRecord) -> Result<AgentId> {
    let Some(coordinator_id) = record.run.coordinator_id.as_deref() else {
        bail!(
            "TaskRun {} is missing coordinator_id; it is a legacy or incomplete TaskRun and cannot send review feedback through `wt task review`.",
            record.id
        );
    };
    AgentId::parse(coordinator_id).with_context(|| {
        format!(
            "TaskRun {} has invalid coordinator_id: {coordinator_id}",
            record.id
        )
    })
}

fn validate_review_sender(
    record: &TaskRunRecord,
    from: &AgentId,
    status: TaskReviewStatus,
    message: &str,
) -> Result<()> {
    let expected = required_coordinator_id(record)?;
    if from.as_str() != expected.as_str() {
        let hint = review_sender_mismatch_hint(record, &expected, status, message);
        bail!(
            "Current actor id {} does not match TaskRun {} coordinator_id {}; review feedback must be sent by the TaskRun coordinator route.\nHint: {hint}",
            from.as_str(),
            record.id,
            expected.as_str()
        );
    }
    Ok(())
}

fn review_sender_mismatch_hint(
    record: &TaskRunRecord,
    expected: &AgentId,
    status: TaskReviewStatus,
    message: &str,
) -> String {
    format!(
        "wt as {} -- wt task review {} {} {}",
        shell_arg(expected.as_str()),
        shell_arg(&record.id),
        review_status_flag(status),
        shell_arg(message)
    )
}

fn review_status_flag(status: TaskReviewStatus) -> &'static str {
    match status {
        TaskReviewStatus::Accepted => "--accept",
        TaskReviewStatus::Rejected => "--reject",
        TaskReviewStatus::Blocked => "--block",
    }
}

fn review_message_text(message: &[String]) -> Result<String> {
    let message = message.join(" ");
    let message = message.trim();
    if message.is_empty() {
        bail!("Review message cannot be empty");
    }
    Ok(message.to_string())
}

fn review_message(status: TaskReviewStatus, message: &str) -> String {
    format!(
        "Coordinator Review: Status={}; Message={message}",
        status.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions};
    use crate::messages::MessageStore;
    use crate::storage::StorageRoot;
    use crate::task_run::{STATUS_PASSED, STATUS_RUNNING};

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
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.last_review_status, Some(task_run::REVIEW_ACCEPTED));
        assert!(updated.last_reviewed_at.is_some());

        let store = MessageStore::new(ctx.storage_root.runtime_dir());
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
    fn review_reject_reopens_passed_task_run() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            STATUS_PASSED,
        )
        .unwrap();
        let from = AgentId::parse("agents/coord-a").unwrap();

        run_with_actor(
            &ctx,
            &record.id,
            task_run::REVIEW_REJECTED,
            &["needs changes".into()],
            &from,
        )
        .unwrap();

        let updated = task_run::read(&record.path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.last_review_status, Some(task_run::REVIEW_REJECTED));
        assert!(updated.last_review_message_id.is_some());
        assert!(updated.last_reviewed_at.is_some());
    }

    #[test]
    fn review_block_reopens_passed_task_run() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            STATUS_PASSED,
        )
        .unwrap();
        let from = AgentId::parse("agents/coord-a").unwrap();

        run_with_actor(
            &ctx,
            &record.id,
            task_run::REVIEW_BLOCKED,
            &["waiting on input".into()],
            &from,
        )
        .unwrap();

        let updated = task_run::read(&record.path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.last_review_status, Some(task_run::REVIEW_BLOCKED));
    }

    #[test]
    fn review_accept_keeps_status_metadata_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let running = task_run::create_direct_routed(
            &ctx,
            "running-task",
            "running-task",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();
        let passed = task_run::create_direct_routed(
            &ctx,
            "passed-task",
            "passed-task",
            "agents/coord-a",
            None,
            STATUS_PASSED,
        )
        .unwrap();
        let from = AgentId::parse("agents/coord-a").unwrap();

        run_with_actor(
            &ctx,
            &running.id,
            task_run::REVIEW_ACCEPTED,
            &["accepted".into()],
            &from,
        )
        .unwrap();
        run_with_actor(
            &ctx,
            &passed.id,
            task_run::REVIEW_ACCEPTED,
            &["accepted".into()],
            &from,
        )
        .unwrap();

        let running = task_run::read(&running.path).unwrap();
        let passed = task_run::read(&passed.path).unwrap();
        assert_eq!(running.status, STATUS_RUNNING);
        assert_eq!(running.last_review_status, Some(task_run::REVIEW_ACCEPTED));
        assert_eq!(passed.status, STATUS_PASSED);
        assert_eq!(passed.last_review_status, Some(task_run::REVIEW_ACCEPTED));
    }

    #[test]
    fn report_reject_report_loop_uses_same_task_run_route() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            STATUS_PASSED,
        )
        .unwrap();
        let from = AgentId::parse("agents/coord-a").unwrap();

        run_with_actor(
            &ctx,
            &record.id,
            task_run::REVIEW_REJECTED,
            &["needs changes".into()],
            &from,
        )
        .unwrap();
        crate::commands::task_report::run_with_env(
            &ctx,
            &["Agent Completion Report: Summary=fixed".into()],
            Some(&record.id),
            Some("agents/run-1-add-schema"),
        )
        .unwrap();

        let updated = task_run::read(&record.path).unwrap();
        let message_id = updated.last_report_message_id.unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.last_review_status, Some(task_run::REVIEW_REJECTED));
        assert!(updated.last_reported_at.is_some());

        let store = MessageStore::new(ctx.storage_root.runtime_dir());
        let message = store
            .read_for_inspection("agents/coord-a", &message_id)
            .unwrap()
            .message
            .unwrap();
        assert_eq!(message.meta.from, "agents/run-1-add-schema");
        assert_eq!(message.meta.to, "agents/coord-a");
        assert_eq!(
            message.text_content(),
            "Agent Completion Report: Summary=fixed"
        );
    }

    #[test]
    fn review_rejects_legacy_task_run_without_agent_id() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "legacy",
            "legacy",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();
        remove_task_run_line(&record.path, "agent_id");
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

    #[test]
    fn review_rejects_task_run_without_coordinator_id() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();
        remove_task_run_line(&record.path, "coordinator_id");
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
        assert!(message.contains("missing coordinator_id"));
        assert!(message.contains("wt task review"));
    }

    #[test]
    fn review_rejects_task_run_with_invalid_coordinator_id() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();
        replace_task_run_line(
            &record.path,
            "coordinator_id",
            "coordinator_id = \"agents/team/coord\"",
        );
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
        assert!(message.contains("invalid coordinator_id"));
        assert!(message.contains("agents/team/coord"));
    }

    #[test]
    fn review_rejects_actor_mismatch_with_task_run_coordinator_id() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();
        let from = AgentId::parse("agents/coord-b").unwrap();

        let err = run_with_actor(
            &ctx,
            &record.id,
            task_run::REVIEW_REJECTED,
            &["needs changes".into()],
            &from,
        )
        .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("does not match TaskRun"));
        assert!(message.contains("agents/coord-b"));
        assert!(message.contains("coordinator_id agents/coord-a"));
        assert!(message.contains(&format!(
            "Hint: wt as agents/coord-a -- wt task review {} --reject 'needs changes'",
            record.id
        )));
        assert!(
            task_run::read(&record.path)
                .unwrap()
                .last_review_message_id
                .is_none()
        );
        assert!(
            !ctx.storage_root
                .runtime_dir()
                .join("agents/run-1-add-schema/inbox/new")
                .exists()
        );
    }

    fn remove_task_run_line(path: &std::path::Path, key: &str) {
        let content = std::fs::read_to_string(path).unwrap();
        let prefix = format!("{key} = ");
        let content = content
            .lines()
            .filter(|line| !line.starts_with(&prefix))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, format!("{content}\n")).unwrap();
    }

    fn replace_task_run_line(path: &std::path::Path, key: &str, replacement: &str) {
        let content = std::fs::read_to_string(path).unwrap();
        let prefix = format!("{key} = ");
        let content = content
            .lines()
            .map(|line| {
                if line.starts_with(&prefix) {
                    replacement
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, format!("{content}\n")).unwrap();
    }
}
