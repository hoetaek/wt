use crate::context::Ctx;
use crate::task as task_store;
use crate::workflow::WorkflowMetadata;
use crate::workflow::render::base_label;
use crate::workflow::run::task_run_record;
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
    if let Some(objective) = metadata.objective.as_deref() {
        ctx.ui
            .print_dim(&format!("  Objective: {}", objective.trim()));
    }
    if let Some(profile) = metadata.profile.as_deref() {
        ctx.ui.print_dim(&format!("  Profile: {profile}"));
    }
    if !metadata.profiles.is_empty() {
        ctx.ui
            .print_dim(&format!("  Profiles: {}", metadata.profiles.join(", ")));
    }
    if let Some(color) = metadata.color.as_deref() {
        ctx.ui.print_dim(&format!("  Color: {color}"));
    }
    ctx.ui.print_dim(&format!(
        "  Pull request: {}",
        metadata.policy.pull_request.as_str()
    ));
    ctx.ui
        .print_dim(&format!("  Landing: {}", metadata.policy.landing.as_str()));
    ctx.ui
        .print_dim(&format!("  Tasks: {}", metadata.tasks.len()));

    for (idx, item) in metadata.tasks.iter().enumerate() {
        let status = if metadata.mode == crate::workflow::WorkflowMode::Matrix {
            format!("{} profile runs", item.runs.len())
        } else {
            task_run_record(ctx, &item.run)
                .as_ref()
                .map(|run| run.status.as_str().to_string())
                .unwrap_or_else(|| "missing".into())
        };
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
        if metadata.mode == crate::workflow::WorkflowMode::Matrix {
            for profile_run in &item.runs {
                let run = task_run_record(ctx, &profile_run.run);
                let status = run
                    .as_ref()
                    .map(|run| run.status.as_str())
                    .unwrap_or("missing");
                ctx.ui.print_dim(&format!(
                    "     Profile {}: {} [{}]",
                    profile_run.profile, profile_run.run, status
                ));
                if let Some(error) = run.and_then(|run| run.error) {
                    if !error.trim().is_empty() {
                        ctx.ui.print_dim(&format!("       Error: {error}"));
                    }
                }
            }
        } else if let Some(error) = task_run_record(ctx, &item.run).and_then(|run| run.error) {
            if !error.trim().is_empty() {
                ctx.ui.print_dim(&format!("     Error: {error}"));
            }
        }
    }
    Ok(())
}
