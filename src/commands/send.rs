use crate::context::Ctx;
use crate::services::cmux::CmuxService;
use crate::services::runtime_binding::{RuntimeBinding, RuntimeBindingResolver};
use crate::services::work::CmuxContact;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx, target: &str, message: &[String], no_enter: bool) -> Result<()> {
    if message.is_empty() {
        bail!("Message cannot be empty");
    }
    let text = message.join(" ");

    let resolver = RuntimeBindingResolver::new(ctx);
    let binding = resolve_runtime_binding(ctx, &resolver, target, &text, no_enter)?;
    let binding = resolver.revalidate(&binding)?;
    let contact = &binding.contact;

    let cmux = CmuxService::new(ctx.runner.as_ref());
    cmux.send(&contact.surface, &contact.workspace, &text)?;
    if !no_enter {
        std::thread::sleep(std::time::Duration::from_millis(500));
        cmux.send_key(&contact.surface, &contact.workspace, "enter")?;
    }

    ctx.ui.print_step(&format!(
        "Sent message to {} on {}",
        contact.surface, contact.workspace
    ));
    Ok(())
}

fn resolve_runtime_binding(
    ctx: &Ctx,
    resolver: &RuntimeBindingResolver<'_>,
    target: &str,
    message: &str,
    no_enter: bool,
) -> Result<RuntimeBinding> {
    let work = resolver.observe(Some(target))?;
    if work.target.worktree.is_none() {
        bail!(
            "Target branch is not checked out in a local worktree: {}",
            work.target.branch
        );
    }
    if let Some(binding) = resolver.unique_live_binding(&work) {
        return Ok(binding);
    }

    let live = resolver.live_candidates(&work);
    match live.as_slice() {
        [] => bail!(
            "No live agent cmux surface was validated for target {}; send was not delivered.\n{}",
            work.target.label,
            cmux_candidate_commands(&work.cmux_contacts, message, no_enter)
        ),
        _ => select_runtime_binding(ctx, resolver, &work, live, message, no_enter),
    }
}

fn select_runtime_binding(
    ctx: &Ctx,
    resolver: &RuntimeBindingResolver<'_>,
    work: &crate::services::work::Work,
    contacts: Vec<CmuxContact>,
    message: &str,
    no_enter: bool,
) -> Result<RuntimeBinding> {
    let items = contacts.iter().map(contact_label).collect::<Vec<_>>();

    match ctx.ui.select("Select live agent cmux surface", &items) {
        Ok(index) if index < contacts.len() => Ok(resolver.bind_contact(work, &contacts[index])),
        Ok(index) => bail!(
            "Selected cmux surface index {index} is out of range for {} candidates",
            contacts.len()
        ),
        Err(_) => bail!(
            "Multiple live agent cmux surfaces match the target worktree; send was not delivered.\n{}",
            cmux_candidate_commands(&contacts, message, no_enter)
        ),
    }
}

fn contact_label(contact: &CmuxContact) -> String {
    let selected = if contact.selected { " [selected]" } else { "" };
    let readable = if contact.readable {
        "readable"
    } else {
        "unreadable"
    };
    let warning = contact
        .validation_warning
        .as_deref()
        .map(|warning| format!(", warning={warning}"))
        .unwrap_or_default();
    format!(
        "{} {}{} (pane {}, workspace \"{}\", window {}, {}, agent={} status={}{})",
        contact.workspace,
        contact.surface,
        selected,
        contact.pane,
        contact.title,
        contact.window,
        readable,
        contact.state.agent_kind.as_str(),
        contact.state.status.as_str(),
        warning
    )
}

fn cmux_candidate_commands(contacts: &[CmuxContact], message: &str, no_enter: bool) -> String {
    contacts
        .iter()
        .map(|contact| {
            let mut lines = vec![
                format!("  - {}", contact_label(contact)),
                format!(
                    "    cmux send --workspace {} --surface {} {}",
                    shell_arg(&contact.workspace),
                    shell_arg(&contact.surface),
                    shell_arg(message)
                ),
            ];
            if !no_enter {
                lines.push(format!(
                    "    cmux send-key --workspace {} --surface {} enter",
                    shell_arg(&contact.workspace),
                    shell_arg(&contact.surface)
                ));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::sync::Arc;

    #[test]
    fn send_resolves_target_and_submits_message_to_cmux_surface() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        add_revalidated_live_contact(&mut runner, &worktree, "surface:4", "uuid-surface-4");
        runner.add_response("", true);
        runner.add_response("", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, "feature", &["hello".into(), "agent".into()], false).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Sent message to surface:4 on workspace:1"));
    }

    #[test]
    fn send_resolves_task_run_target_through_runtime_binding() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(repo.join(".local/task-runs")).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            repo.join(".local/task-runs/run-feature.toml"),
            "task = \"feature\"\nbranch = \"feature\"\nstatus = \"running\"\ncreated_at = \"2026-05-16T00:00:00Z\"\nupdated_at = \"2026-05-16T00:00:00Z\"\n",
        )
        .unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response("", false);
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        add_revalidated_live_contact(&mut runner, &worktree, "surface:4", "uuid-surface-4");
        runner.add_response("", true);
        runner.add_response("", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(
            &ctx,
            "run-feature",
            &["hello".into(), "agent".into()],
            false,
        )
        .unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Sent message to surface:4 on workspace:1"));
    }

    #[test]
    fn send_rejects_stale_binding_after_revalidation() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Terminal surface not found", false);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let err = run(&ctx, "feature", &["hello".into(), "agent".into()], false)
            .unwrap_err()
            .to_string();

        assert!(err.contains("stale or no longer a live agent surface"));
    }

    #[test]
    fn send_fails_with_raw_cmux_commands_when_surface_match_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":null,"selected_surface_ref":null}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        runner.add_response("Codex Ready", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let err = run(&ctx, "feature", &["hello".into(), "agent".into()], false)
            .unwrap_err()
            .to_string();

        assert!(err.contains("Multiple live agent cmux surfaces match the target worktree"));
        assert!(
            err.contains("cmux send --workspace workspace:1 --surface surface:4 'hello agent'")
        );
        assert!(err.contains("cmux send-key --workspace workspace:1 --surface surface:5 enter"));
    }

    #[test]
    fn send_uses_the_selected_live_agent_when_it_is_the_unique_live_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5\nsurface:6", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-5","selected_surface_ref":"surface:5"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("zsh %", true);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        runner.add_response("zsh %", true);
        add_revalidated_live_contact(&mut runner, &worktree, "surface:5", "uuid-surface-5");
        runner.add_response("", true);
        runner.add_response("", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, "feature", &["hello".into(), "agent".into()], false).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let prompts = ui.prompts.lock().unwrap().join("\n");
        assert!(steps.contains("Sent message to surface:5 on workspace:1"));
        assert!(!prompts.contains("Select cmux surface"));
    }

    #[test]
    fn send_uses_unique_live_agent_even_when_shell_surface_is_selected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("zsh %", true);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        add_revalidated_live_contact(&mut runner, &worktree, "surface:5", "uuid-surface-5");
        runner.add_response("", true);
        runner.add_response("", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, "feature", &["hello".into(), "agent".into()], false).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let prompts = ui.prompts.lock().unwrap().join("\n");
        assert!(steps.contains("Sent message to surface:5 on workspace:1"));
        assert!(!prompts.contains("Select cmux surface"));
    }

    #[test]
    fn send_excludes_unreadable_surface_when_unique_live_agent_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Terminal surface not found", false);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        add_revalidated_live_contact(&mut runner, &worktree, "surface:5", "uuid-surface-5");
        runner.add_response("", true);
        runner.add_response("", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, "feature", &["hello".into(), "agent".into()], false).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        assert!(steps.contains("Sent message to surface:5 on workspace:1"));
    }

    #[test]
    fn send_fails_when_multiple_matching_surfaces_are_selected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3\npane:4", true);
        runner.add_response("surface:4", true);
        runner.add_response("surface:5", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"uuid-surface-4","selected_surface_ref":"surface:4"},{"id":"uuid-pane-4","ref":"pane:4","selected_surface_id":"uuid-surface-5","selected_surface_ref":"surface:5"}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        runner.add_response("Codex Ready", true);
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );

        let err = run(&ctx, "feature", &["hello".into(), "agent".into()], false)
            .unwrap_err()
            .to_string();

        assert!(err.contains("Multiple live agent cmux surfaces match the target worktree"));
        assert!(
            err.contains("cmux send --workspace workspace:1 --surface surface:4 'hello agent'")
        );
        assert!(
            err.contains("cmux send --workspace workspace:1 --surface surface:5 'hello agent'")
        );
    }

    #[test]
    fn send_prompts_for_live_agent_surface_when_match_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("sample");
        let worktree = dir.path().join("sample-feature");
        std::fs::create_dir_all(&worktree).unwrap();

        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/master\n\nworktree {}\nHEAD def\nbranch refs/heads/feature\n\n",
                repo.display(),
                worktree.display()
            ),
            true,
        );
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response("surface:4\nsurface:5", true);
        runner.add_response(
            r#"{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":null,"selected_surface_ref":null}]}"#,
            true,
        );
        add_no_surface_processes(&mut runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
        runner.add_response("Codex Ready", true);
        add_revalidated_live_contact(&mut runner, &worktree, "surface:5", "uuid-surface-5");
        runner.add_response("", true);
        runner.add_response("", true);
        let mut ui = MockUi::new();
        ui.add_select(1);
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            repo,
            worktree,
            Config::default(),
            Box::new(runner),
            Box::new(ui.clone()),
        );

        run(&ctx, "feature", &["hello".into(), "agent".into()], false).unwrap();

        let steps = ui.steps.lock().unwrap().join("\n");
        let prompts = ui.prompts.lock().unwrap().join("\n");
        assert!(prompts.contains("Select live agent cmux surface"));
        assert!(steps.contains("Sent message to surface:5 on workspace:1"));
    }

    fn add_revalidated_live_contact(
        runner: &mut MockRunner,
        worktree: &std::path::Path,
        surface: &str,
        surface_id: &str,
    ) {
        runner.add_response(
            r#"{"windows":[{"id":"uuid-window-1","ref":"window:1"}]}"#,
            true,
        );
        runner.add_response(
            &format!(
                r#"{{"window_id":"uuid-window-1","window_ref":"window:1","workspaces":[{{"id":"uuid-workspace-1","ref":"workspace:1","title":"feature","current_directory":"{}"}}]}}"#,
                worktree.display()
            ),
            true,
        );
        runner.add_response("pane:3", true);
        runner.add_response(surface, true);
        runner.add_response(
            &format!(
                r#"{{"workspace_id":"uuid-workspace-1","workspace_ref":"workspace:1","panes":[{{"id":"uuid-pane-3","ref":"pane:3","selected_surface_id":"{surface_id}","selected_surface_ref":"{surface}"}}]}}"#
            ),
            true,
        );
        add_no_surface_processes(runner);
        runner.add_response("Codex Ready", true);
        runner.add_response("codex=Idle", true);
        runner.add_response("", true);
    }

    fn add_no_surface_processes(runner: &mut MockRunner) {
        runner.add_response(
            r#"{"windows":[{"workspaces":[{"panes":[{"surfaces":[]}]}]}]}"#,
            true,
        );
    }
}
