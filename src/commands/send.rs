use crate::commands::review;
use crate::context::Ctx;
use crate::services::cmux::CmuxService;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx, target: &str, message: &[String], no_enter: bool) -> Result<()> {
    if message.is_empty() {
        bail!("Message cannot be empty");
    }

    let target = review::resolve_review_target(ctx, Some(target))?;
    let worktree = target.worktree.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Target branch is not checked out in a local worktree: {}",
            target.branch
        )
    })?;
    let contact = review::first_cmux_contact(ctx, worktree)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No cmux workspace/surface found for worktree: {}",
            worktree.display()
        )
    })?;

    let cmux = CmuxService::new(ctx.runner.as_ref());
    let text = message.join(" ");
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
}
