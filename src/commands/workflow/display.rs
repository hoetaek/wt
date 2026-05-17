use crate::context::Ctx;
use crate::task as task_store;
use crate::workflow::render::base_label;
use crate::workflow::run::task_run_record;
use crate::workflow::{WorkflowMetadata, WorkflowMode};
use anyhow::Result;
use std::path::Path;

pub(super) fn show_workflow(ctx: &Ctx, path: &Path, metadata: &WorkflowMetadata) -> Result<()> {
    let display_path = path
        .strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .display()
        .to_string();

    ctx.ui.print_step(&format!("Workflow: {display_path}"));
    ctx.ui
        .print_dim(&format!("  Mode: {}", metadata.mode.as_str()));
    ctx.ui
        .print_dim(&format!("  Base: {}", base_label(metadata)));
    if let Some(profile) = metadata.profile.as_deref() {
        ctx.ui.print_dim(&format!("  Profile: {profile}"));
    }
    if let Some(color) = metadata.color.as_deref() {
        ctx.ui.print_dim(&format!("  Color: {color}"));
    }
    if let Some(policy) = metadata.policy.as_ref() {
        ctx.ui.print_dim(&format!(
            "  Landing: {} (requires approval: {})",
            policy.landing.as_str(),
            policy.landing_requires_approval
        ));
    }
    ctx.ui
        .print_dim(&format!("  Tasks: {}", metadata.tasks.len()));

    for (idx, item) in metadata.tasks.iter().enumerate() {
        let run = task_run_record(ctx, &item.run);
        let status = run
            .as_ref()
            .map(|run| run.status.as_str())
            .unwrap_or("missing");
        let task_doc = task_store::read_task_document(ctx, &item.task);
        let title = task_doc
            .as_ref()
            .map(|document| document.title_or_key(&item.task))
            .unwrap_or_else(|_| item.task.clone());
        ctx.ui.print_dim(&format!(
            "  {}. {} [{}] {}",
            idx + 1,
            item.task,
            status,
            title
        ));
        ctx.ui.print_dim(&format!(
            "     Task: {}",
            task_store::task_relative_path(&item.task)
        ));
        match task_doc {
            Ok(document) => {
                if !document.branch.trim().is_empty() {
                    ctx.ui
                        .print_dim(&format!("     Branch: {}", document.branch));
                }
            }
            Err(err) => ctx.ui.print_dim(&format!("     Task error: {err}")),
        }
        if let Some(parent) = item.parent.as_deref() {
            ctx.ui.print_dim(&format!("     Parent: {parent}"));
        }
        if metadata.mode == WorkflowMode::Stack {
            let pull_request = item
                .pull_request
                .map(|mode| mode.as_str())
                .unwrap_or("none");
            ctx.ui
                .print_dim(&format!("     Pull request: {pull_request}"));
        }
        if let Some(error) = run.and_then(|run| run.error) {
            if !error.trim().is_empty() {
                ctx.ui.print_dim(&format!("     Error: {error}"));
            }
        }
    }
    Ok(())
}
