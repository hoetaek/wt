use crate::context::Ctx;
use crate::task as task_store;
use crate::workflow::WorkflowMetadata;
use crate::workflow::render::{base_label, workflow_origin_label};
use crate::workflow::run::task_run_record;
use anyhow::Result;
use std::path::Path;

pub(super) fn show_workflow(ctx: &Ctx, path: &Path, metadata: &WorkflowMetadata) -> Result<()> {
    let display_path = ctx.storage_root.display_path(path);

    ctx.ui.print_step(&format!("Workflow: {display_path}"));
    ctx.ui
        .print_dim(&format!("  Mode: {}", metadata.mode.as_str()));
    ctx.ui
        .print_dim(&format!("  Base: {}", base_label(metadata)));
    print_optional_value(ctx, "Title", metadata.title.as_deref());
    print_body(ctx, metadata.body.as_deref());
    ctx.ui.print_dim(&format!(
        "  Origin: {}",
        workflow_origin_label(metadata).unwrap_or_else(|| "(none)".into())
    ));
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
    ctx.ui.print_dim(&format!(
        "  Review codex_base: {}",
        metadata.policy.review.codex_base.as_str()
    ));
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
                if let Some(error) = run.and_then(|run| run.error)
                    && !error.trim().is_empty()
                {
                    ctx.ui.print_dim(&format!("       Error: {error}"));
                }
            }
        } else if let Some(error) = task_run_record(ctx, &item.run).and_then(|run| run.error)
            && !error.trim().is_empty()
        {
            ctx.ui.print_dim(&format!("     Error: {error}"));
        }
    }
    Ok(())
}

fn print_optional_value(ctx: &Ctx, label: &str, value: Option<&str>) {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(none)");
    ctx.ui.print_dim(&format!("  {label}: {value}"));
}

fn print_body(ctx: &Ctx, body: Option<&str>) {
    let Some(body) = body.map(str::trim).filter(|body| !body.is_empty()) else {
        ctx.ui.print_dim("  Body: (none)");
        return;
    };

    if !body.contains('\n') {
        ctx.ui.print_dim(&format!("  Body: {body}"));
        return;
    }

    ctx.ui.print_dim("  Body:");
    for line in body.lines() {
        if line.trim().is_empty() {
            ctx.ui.print_dim("    ");
        } else {
            ctx.ui.print_dim(&format!("    {}", line.trim_end()));
        }
    }
}
