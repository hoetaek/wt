use crate::context::Ctx;
use crate::messages::{AgentId, MessageScope, MessageStore};
use crate::services::git::GitService;
use crate::services::inbox_wake;
use crate::task_run::{self, TaskRunRecord};
use anyhow::{Context, Result, bail};
use std::env;

pub(crate) fn run(ctx: &Ctx, message: &[String]) -> Result<()> {
    let task_run_id = env_value("WT_TASK_RUN_ID")?;
    let agent_id = env_value("WT_AGENT_ID")?;
    run_with_env(ctx, message, task_run_id.as_deref(), agent_id.as_deref())
}

fn run_with_env(
    ctx: &Ctx,
    message: &[String],
    task_run_id: Option<&str>,
    runtime_agent_id: Option<&str>,
) -> Result<()> {
    let text = message.join(" ");
    if text.trim().is_empty() {
        bail!("Report message cannot be empty");
    }

    let record = resolve_report_task_run(ctx, task_run_id)?;
    let scope = report_scope(&record)?;
    let from = required_agent_id(&record, "agent_id")?;
    let to = required_coordinator_id(&record)?;
    validate_runtime_agent(runtime_agent_id, &record, &from)?;

    let store = MessageStore::new(ctx.storage_root.messages_dir());
    let sent = store.send_scoped_from(from.as_str(), to.as_str(), scope, &text)?;
    task_run::update_report_metadata(&record, &sent.id)?;
    let _wake_result = inbox_wake::wake_sent_message_recipient(ctx, &sent);

    if !ctx.quiet {
        println!("{}", ctx.storage_root.display_path(&sent.path));
    }

    Ok(())
}

fn resolve_report_task_run(ctx: &Ctx, task_run_id: Option<&str>) -> Result<TaskRunRecord> {
    if let Some(id) = task_run_id.map(str::trim).filter(|id| !id.is_empty()) {
        let path = task_run::resolve(ctx, id)?;
        let run = task_run::read(&path)?;
        let record = TaskRunRecord {
            id: task_run::id_from_path(&path)?,
            path,
            run,
        };
        ensure_reportable_status(&record)?;
        return Ok(record);
    }

    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let branch = git
        .current_branch()
        .context("WT_TASK_RUN_ID is not set and current branch could not be resolved")?;
    let mut candidates = task_run::list(ctx)?
        .into_iter()
        .filter(|record| {
            record.run.branch == branch && record.run.status == task_run::STATUS_RUNNING
        })
        .collect::<Vec<_>>();
    candidates.sort_by(task_run::compare_task_run_records);

    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => bail!(
            "wt task report could not resolve a running TaskRun. WT_TASK_RUN_ID is not set and no running TaskRun matches current branch `{branch}`."
        ),
        _ => bail!(
            "wt task report could not resolve exactly one running TaskRun for current branch `{branch}`. Candidate TaskRun ids: {}. Set WT_TASK_RUN_ID to the intended TaskRun id.",
            candidates
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn ensure_reportable_status(record: &TaskRunRecord) -> Result<()> {
    if record.run.status != task_run::STATUS_RUNNING {
        bail!(
            "wt task report requires a running TaskRun. TaskRun {} is {}.",
            record.id,
            record.run.status
        );
    }
    Ok(())
}

fn report_scope(record: &TaskRunRecord) -> Result<MessageScope> {
    match task_run::workflow_scope_id(record) {
        Some(workflow_id) => MessageScope::workflow(workflow_id),
        None => Ok(MessageScope::direct()),
    }
}

fn required_agent_id(record: &TaskRunRecord, field: &str) -> Result<AgentId> {
    let Some(agent_id) = record.run.agent_id.as_deref() else {
        bail!(
            "TaskRun {} is missing {field}; it is a legacy or incomplete TaskRun and cannot report through `wt task report`.",
            record.id
        );
    };
    AgentId::parse(agent_id)
        .with_context(|| format!("TaskRun {} has invalid {field}: {agent_id}", record.id))
}

fn required_coordinator_id(record: &TaskRunRecord) -> Result<AgentId> {
    let Some(coordinator_id) = record.run.coordinator_id.as_deref() else {
        bail!(
            "TaskRun {} is missing coordinator_id; it is a legacy or incomplete TaskRun and cannot report through `wt task report`.",
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

fn validate_runtime_agent(
    runtime_agent_id: Option<&str>,
    record: &TaskRunRecord,
    expected: &AgentId,
) -> Result<()> {
    let Some(runtime_agent_id) = runtime_agent_id
        .map(str::trim)
        .filter(|agent_id| !agent_id.is_empty())
    else {
        return Ok(());
    };
    let actual = AgentId::parse(runtime_agent_id).context("Invalid WT_AGENT_ID")?;
    if actual.as_str() != expected.as_str() {
        bail!(
            "WT_AGENT_ID does not match TaskRun {}. Current runtime agent id is {}; expected {}.",
            record.id,
            actual.as_str(),
            expected.as_str()
        );
    }
    Ok(())
}

fn env_value(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok((!value.trim().is_empty()).then(|| value.trim().to_string())),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => bail!("Invalid {name}: value is not Unicode"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use crate::task_run::{STATUS_DONE, STATUS_RUNNING};
    use anyhow::Result;
    use std::path::Path;
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&Path>,
        ) -> Result<crate::context::CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    fn ctx(root: &Path, runner: Arc<MockRunner>) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(SharedRunner { inner: runner }),
            Box::new(MockUi::new()),
        )
    }

    #[test]
    fn report_sends_direct_message_and_records_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(MockRunner::new());
        let ctx = ctx(dir.path(), runner);
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            STATUS_RUNNING,
        )
        .unwrap();

        run_with_env(
            &ctx,
            &["Agent Completion Report: Summary=done".into()],
            Some(&record.id),
            Some("agents/run-1-add-schema"),
        )
        .unwrap();

        let updated = task_run::read(&record.path).unwrap();
        let message_id = updated.last_report_message_id.unwrap();
        assert!(updated.last_reported_at.is_some());

        let store = MessageStore::new(ctx.storage_root.messages_dir());
        let message = store
            .read_for_inspection("agents/coord-a", &message_id)
            .unwrap()
            .message
            .unwrap();
        assert_eq!(message.meta.from, "agents/run-1-add-schema");
        assert_eq!(message.meta.to, "agents/coord-a");
        assert_eq!(
            message.text_content(),
            "Agent Completion Report: Summary=done"
        );
        assert_eq!(message.scope.kind.as_str(), "direct");
    }

    #[test]
    fn report_sends_workflow_scoped_message_from_task_run_context() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(MockRunner::new());
        let ctx = ctx(dir.path(), runner);
        let record = task_run::create_workflow_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "workflow-1",
            "agents/coord-a",
            Some("Coordinator for workflow \"API\""),
            STATUS_RUNNING,
        )
        .unwrap();
        run_with_env(
            &ctx,
            &["Agent Completion Report: Summary=workflow done".into()],
            Some(&record.id),
            Some("agents/run-1-add-schema"),
        )
        .unwrap();

        let updated = task_run::read(&record.path).unwrap();
        let message_id = updated.last_report_message_id.unwrap();
        assert!(updated.last_reported_at.is_some());

        let store = MessageStore::new(ctx.storage_root.messages_dir());
        let message = store
            .read_for_inspection("agents/coord-a", &message_id)
            .unwrap()
            .message
            .unwrap();
        assert_eq!(message.meta.from, "agents/run-1-add-schema");
        assert_eq!(message.meta.to, "agents/coord-a");
        assert_eq!(message.scope.kind.as_str(), "workflow");
        assert_eq!(message.scope.id.as_deref(), Some("workflow-1"));
        assert_eq!(
            message.text_content(),
            "Agent Completion Report: Summary=workflow done"
        );
    }

    #[test]
    fn report_rejects_explicit_non_running_task_run() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(MockRunner::new());
        let ctx = ctx(dir.path(), runner);
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            None,
            STATUS_DONE,
        )
        .unwrap();

        let err = run_with_env(
            &ctx,
            &["Agent Completion Report: Summary=done".into()],
            Some(&record.id),
            Some("agents/run-1-add-schema"),
        )
        .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("requires a running TaskRun"));
        assert!(message.contains("is done"));
    }

    #[test]
    fn report_rejects_runtime_agent_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(MockRunner::new());
        let ctx = ctx(dir.path(), runner);
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();

        let err = run_with_env(
            &ctx,
            &["Agent Completion Report: Summary=done".into()],
            Some(&record.id),
            Some("agents/other"),
        )
        .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("WT_AGENT_ID does not match TaskRun"));
        assert!(message.contains("Current runtime agent id is agents/other"));
        assert!(message.contains("expected agents/run-1-add-schema"));
    }

    #[test]
    fn report_branch_fallback_fails_on_ambiguous_running_task_runs() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("feature", true);
        let runner = Arc::new(runner);
        let ctx = ctx(dir.path(), Arc::clone(&runner));
        task_run::create_direct_routed(
            &ctx,
            "first",
            "feature",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();
        task_run::create_direct_routed(
            &ctx,
            "second",
            "feature",
            "agents/coord-a",
            None,
            STATUS_RUNNING,
        )
        .unwrap();

        let err = run_with_env(
            &ctx,
            &["Agent Completion Report: Summary=done".into()],
            None,
            None,
        )
        .unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("could not resolve exactly one running TaskRun"));
        assert!(message.contains("run-first"));
        assert!(message.contains("run-second"));
    }
}
