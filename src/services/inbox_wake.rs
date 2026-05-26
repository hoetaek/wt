use crate::agents::{self, AgentKind, AgentObservation, AgentStatus};
use crate::context::Ctx;
use crate::messages::{AgentId, Message, MessageScopeKind, SentMessage};
use crate::services::cmux::CmuxService;
use crate::services::cmux_push::{CmuxPushService, PushKind};
use crate::services::identity_locator::{self, AnchorKind, Marker};
use crate::services::runtime_binding::RuntimeBindingResolver;
use crate::task_run;
use anyhow::Result;

const INBOX_WAKE_PROMPT: &str = "Check your wt inbox.";
const SCREEN_LINES: usize = 80;

pub(crate) fn wake_sent_message_recipient(ctx: &Ctx, sent: &SentMessage) -> Result<bool> {
    wake_message_recipient(ctx, &sent.message)
}

fn wake_message_recipient(ctx: &Ctx, message: &Message) -> Result<bool> {
    let recipient = AgentId::parse(&message.meta.to)?;
    if let Some(task_run_id) = task_run_scope_recipient(ctx, message)? {
        return wake_idle_task_run_agent(ctx, &task_run_id);
    }
    if let Some(task_run_id) = single_running_task_run_for_agent(ctx, &recipient)? {
        if wake_idle_task_run_agent(ctx, &task_run_id)? {
            return Ok(true);
        }
    }
    wake_idle_session_marker_agent(ctx, &recipient)
}

fn task_run_scope_recipient(ctx: &Ctx, message: &Message) -> Result<Option<String>> {
    if message.scope.kind != MessageScopeKind::TaskRun {
        return Ok(None);
    }
    let Some(task_run_id) = message.scope.id.as_deref() else {
        return Ok(None);
    };
    let path = task_run::resolve(ctx, task_run_id)?;
    let run = task_run::read(&path)?;
    let Some(agent_id) = run.agent_id.as_deref() else {
        return Ok(None);
    };
    if AgentId::parse(agent_id)?.as_str() == message.meta.to {
        Ok(Some(task_run::id_from_path(&path)?))
    } else {
        Ok(None)
    }
}

fn single_running_task_run_for_agent(ctx: &Ctx, recipient: &AgentId) -> Result<Option<String>> {
    let mut matches = task_run::list(ctx)?
        .into_iter()
        .filter(|record| record.run.status == task_run::STATUS_RUNNING)
        .filter(|record| {
            record
                .run
                .agent_id
                .as_deref()
                .and_then(|id| AgentId::parse(id).ok())
                .is_some_and(|agent| agent.as_str() == recipient.as_str())
        })
        .map(|record| record.id)
        .collect::<Vec<_>>();
    matches.sort();
    Ok(match matches.as_slice() {
        [task_run_id] => Some(task_run_id.clone()),
        _ => None,
    })
}

fn wake_idle_task_run_agent(ctx: &Ctx, task_run_id: &str) -> Result<bool> {
    let resolver = RuntimeBindingResolver::new(ctx);
    let work = resolver.observe(Some(task_run_id))?;
    if work.state.status != AgentStatus::Idle {
        return Ok(false);
    }

    let Some(binding) = resolver.unique_live_binding(&work) else {
        return Ok(false);
    };
    let binding = resolver.revalidate(&binding)?;
    if binding.contact.state.status != AgentStatus::Idle {
        return Ok(false);
    }

    let Some(kind) = push_kind(binding.contact.state.agent_kind) else {
        return Ok(false);
    };
    CmuxPushService::new(ctx.runner.as_ref()).push_to_surface_in_workspace(
        &binding.contact.surface,
        Some(&binding.contact.workspace),
        kind,
        INBOX_WAKE_PROMPT,
    )?;
    Ok(true)
}

fn wake_idle_session_marker_agent(ctx: &Ctx, recipient: &AgentId) -> Result<bool> {
    let mut markers = identity_locator::list_markers(ctx)?
        .into_iter()
        .filter(|marker| marker.id == recipient.as_str())
        .collect::<Vec<_>>();
    markers.sort_by(|left, right| left.updated_at.cmp(&right.updated_at));
    markers.reverse();

    for marker in markers {
        if wake_idle_surface_marker(ctx, &marker)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn wake_idle_surface_marker(ctx: &Ctx, marker: &Marker) -> Result<bool> {
    if marker.anchor_kind != AnchorKind::Surface {
        return Ok(false);
    }
    let cmux = CmuxService::new(ctx.runner.as_ref());
    if !cmux.is_available() {
        return Ok(false);
    }
    let Some(location) = cmux.find_surface_location(&marker.anchor_value)? else {
        return Ok(false);
    };
    let screen = cmux.read_screen_lines(
        &location.surface_handle,
        &location.workspace_handle,
        SCREEN_LINES,
    )?;
    let statuses = cmux
        .list_status(&location.workspace_handle)
        .unwrap_or_default();
    let observation = AgentObservation::new(Some(&screen), &statuses, &[]);
    let state = agents::classify(&observation);
    if state.status != AgentStatus::Idle {
        return Ok(false);
    }
    let Some(kind) = push_kind(state.agent_kind) else {
        return Ok(false);
    };
    CmuxPushService::new(ctx.runner.as_ref()).push_to_surface_in_workspace(
        &location.surface_handle,
        Some(&location.workspace_handle),
        kind,
        INBOX_WAKE_PROMPT,
    )?;
    Ok(true)
}

fn push_kind(agent_kind: AgentKind) -> Option<PushKind> {
    match agent_kind {
        AgentKind::ClaudeCode => Some(PushKind::Claude),
        AgentKind::Codex => Some(PushKind::Codex),
        AgentKind::Shell | AgentKind::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{CommandCall, MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx, CtxOptions};
    use crate::messages::{MessageScope, MessageStore};
    use crate::storage::StorageRoot;
    use std::path::Path;
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    fn ctx(root: &std::path::Path, runner: Arc<MockRunner>) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(SharedRunner { inner: runner }),
            Box::new(MockUi::new()),
            CtxOptions {
                storage_root: Some(StorageRoot::from_git_common_dir(root.join(".git"))),
                ..CtxOptions::default()
            },
        )
    }

    #[test]
    fn wakes_task_run_scoped_idle_recipient() {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = MockRunner::new();
        mock.add_command("cmux");
        add_worktree_list(&mut mock, dir.path(), "add-schema");
        add_codex_worktree_observation(&mut mock, dir.path(), "add-schema", "Idle");
        add_codex_worktree_observation(&mut mock, dir.path(), "add-schema", "Idle");
        mock.add_response("", true);
        mock.add_response("", true);
        let runner = Arc::new(mock);
        let ctx = ctx(dir.path(), runner.clone());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            task_run::STATUS_RUNNING,
        )
        .unwrap();
        let sent = MessageStore::new(ctx.storage_root.runtime_dir())
            .send_scoped_from(
                "agents/coord-a",
                "agents/run-1-add-schema",
                MessageScope::task_run(record.id).unwrap(),
                "review feedback",
            )
            .unwrap();

        assert!(wake_sent_message_recipient(&ctx, &sent).unwrap());

        assert_wake_sent(&runner.calls.lock().unwrap());
    }

    #[test]
    fn wakes_direct_recipient_with_surface_marker() {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = MockRunner::new();
        mock.add_command("cmux");
        add_surface_marker_observation(&mut mock, dir.path(), "Idle");
        mock.add_response("", true);
        mock.add_response("", true);
        let runner = Arc::new(mock);
        let ctx = ctx(dir.path(), runner.clone());
        identity_locator::write_marker(
            &ctx,
            &identity_locator::AnchorKey {
                kind: AnchorKind::Surface,
                value: "uuid-surface-4".into(),
            },
            "agents/coord-a",
            Some("codex"),
        )
        .unwrap();
        let sent = MessageStore::new(ctx.storage_root.runtime_dir())
            .send_from("agents/worker", "agents/coord-a", "direct message")
            .unwrap();

        assert!(wake_sent_message_recipient(&ctx, &sent).unwrap());

        assert_wake_sent(&runner.calls.lock().unwrap());
    }

    #[test]
    fn does_not_wake_running_recipient() {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = MockRunner::new();
        mock.add_command("cmux");
        add_worktree_list(&mut mock, dir.path(), "add-schema");
        add_codex_worktree_observation(&mut mock, dir.path(), "add-schema", "Running");
        let runner = Arc::new(mock);
        let ctx = ctx(dir.path(), runner.clone());
        let record = task_run::create_direct_routed(
            &ctx,
            "add-schema",
            "add-schema",
            "agents/coord-a",
            Some("Coordinator"),
            task_run::STATUS_RUNNING,
        )
        .unwrap();
        let sent = MessageStore::new(ctx.storage_root.runtime_dir())
            .send_scoped_from(
                "agents/coord-a",
                "agents/run-1-add-schema",
                MessageScope::task_run(record.id).unwrap(),
                "review feedback",
            )
            .unwrap();

        assert!(!wake_sent_message_recipient(&ctx, &sent).unwrap());

        assert!(!runner.calls.lock().unwrap().iter().any(|(cmd, args, _)| {
            cmd == "cmux" && args.first().is_some_and(|arg| arg == "send")
        }));
    }

    fn add_worktree_list(runner: &mut MockRunner, worktree: &std::path::Path, branch: &str) {
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/{branch}\n\n",
                worktree.display()
            ),
            true,
        );
        runner.add_response("", false);
    }

    fn add_codex_worktree_observation(
        runner: &mut MockRunner,
        worktree: &std::path::Path,
        branch: &str,
        status: &str,
    ) {
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"{branch}","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        add_surface_contacts(runner);
        runner.add_response(&format!("Codex {status}"), true);
        runner.add_response(&format!("codex={status}"), true);
        runner.add_response("", true);
    }

    fn add_surface_marker_observation(
        runner: &mut MockRunner,
        worktree: &std::path::Path,
        status: &str,
    ) {
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"coord","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","surface_ids":["uuid-surface-4"],"surface_refs":["surface:4"]}]}"#,
            true,
        );
        runner.add_response(&format!("Codex {status}"), true);
        runner.add_response(&format!("codex={status}"), true);
    }

    fn add_surface_contacts(runner: &mut MockRunner) {
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        runner.add_response(
            r#"{"windows":[{"workspaces":[{"panes":[{"surfaces":[]}]}]}]}"#,
            true,
        );
    }

    fn assert_wake_sent(calls: &[CommandCall]) {
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "cmux"
                && args.iter().map(String::as_str).collect::<Vec<_>>()
                    == vec![
                        "send",
                        "--surface",
                        "surface:4",
                        "--workspace",
                        "workspace:1",
                        "Check your wt inbox.",
                    ]
        }));
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "cmux"
                && args.iter().map(String::as_str).collect::<Vec<_>>()
                    == vec![
                        "send-key",
                        "--surface",
                        "surface:4",
                        "--workspace",
                        "workspace:1",
                        "enter",
                    ]
        }));
    }
}
