use crate::commands::review;
use crate::context::Ctx;
use crate::services::cmux::CmuxService;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx, target: &str, message: &[String], no_enter: bool) -> Result<()> {
    if message.is_empty() {
        bail!("Message cannot be empty");
    }
    let text = message.join(" ");

    let target = review::resolve_review_target(ctx, Some(target))?;
    let worktree = target.worktree.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Target branch is not checked out in a local worktree: {}",
            target.branch
        )
    })?;
    let contact = resolve_cmux_contact(ctx, worktree, &text, no_enter)?;

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

fn resolve_cmux_contact(
    ctx: &Ctx,
    worktree: &std::path::Path,
    message: &str,
    no_enter: bool,
) -> Result<review::CmuxContact> {
    let contacts = review::cmux_contacts(ctx, worktree)?;
    match contacts.as_slice() {
        [] => bail!(
            "No cmux workspace/surface found for worktree: {}",
            worktree.display()
        ),
        [contact] => Ok(contact.clone()),
        _ => select_cmux_contact(ctx, contacts, message, no_enter),
    }
}

fn select_cmux_contact(
    ctx: &Ctx,
    contacts: Vec<review::CmuxContact>,
    message: &str,
    no_enter: bool,
) -> Result<review::CmuxContact> {
    let items = contacts.iter().map(contact_label).collect::<Vec<_>>();

    match ctx.ui.select("Select cmux surface", &items) {
        Ok(index) if index < contacts.len() => Ok(contacts[index].clone()),
        Ok(index) => bail!(
            "Selected cmux surface index {index} is out of range for {} candidates",
            contacts.len()
        ),
        Err(_) => bail!(
            "Multiple cmux surfaces match the target worktree; send was not delivered.\n{}",
            cmux_candidate_commands(&contacts, message, no_enter)
        ),
    }
}

fn contact_label(contact: &review::CmuxContact) -> String {
    format!(
        "{} {} (pane {}, workspace \"{}\", window {})",
        contact.workspace, contact.surface, contact.pane, contact.title, contact.window
    )
}

fn cmux_candidate_commands(
    contacts: &[review::CmuxContact],
    message: &str,
    no_enter: bool,
) -> String {
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

        assert!(err.contains("Multiple cmux surfaces match the target worktree"));
        assert!(
            err.contains("cmux send --workspace workspace:1 --surface surface:4 'hello agent'")
        );
        assert!(err.contains("cmux send-key --workspace workspace:1 --surface surface:5 enter"));
    }

    #[test]
    fn send_prompts_for_cmux_surface_when_match_is_ambiguous() {
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
        assert!(steps.contains("Sent message to surface:5 on workspace:1"));
    }
}
