use crate::task_run::{self, REVIEW_ACCEPTED};
use crate::workflow::render::{codex_base_review_accept_command, shell_arg, workflow_task_label};
use crate::workflow::run::WorkflowTaskState;
use crate::workflow::{WorkflowCodexBaseReview, WorkflowMetadata, WorkflowTask};
use anyhow::{Result, bail};

pub(super) fn validate_required_codex_base_review(
    metadata: &WorkflowMetadata,
    state: &WorkflowTaskState,
) -> Result<()> {
    if metadata.policy.review.codex_base != WorkflowCodexBaseReview::Required {
        return Ok(());
    }

    let parent = codex_review_parent(metadata, &state.row)?;
    if has_fresh_accepted_codex_base_review(state, &parent) {
        return Ok(());
    }

    let status = codex_base_review_status(state, &parent);
    let accept_command = codex_base_review_accept_command(&state.run_id, &parent);
    bail!(
        "Workflow task {} requires Codex base review evidence before pass; {status}. Open a Codex surface and run `{}` against this task. For non-interactive runs, use `{}`. Then record acceptance with `{accept_command}` before running `wt workflow pass`.",
        workflow_task_label(&state.row),
        codex_surface_review_command(&parent),
        codex_cli_review_command(&parent)
    )
}

fn has_fresh_accepted_codex_base_review(state: &WorkflowTaskState, parent: &str) -> bool {
    fresh_codex_base_review_issue(state, parent).is_none()
}

fn fresh_codex_base_review_issue(state: &WorkflowTaskState, parent: &str) -> Option<String> {
    let Some(reported_at) = state.run.last_reported_at.as_deref() else {
        return Some("latest Agent Completion Report timestamp is missing".into());
    };
    let Some(reported_at) = task_run::normalized_utc_timestamp(reported_at) else {
        return Some(format!(
            "latest Agent Completion Report timestamp is invalid: {reported_at}"
        ));
    };
    if state.run.codex_base_review_status != Some(REVIEW_ACCEPTED) {
        let status = state
            .run
            .codex_base_review_status
            .map(|status| status.as_str())
            .unwrap_or("missing");
        if status == "missing" {
            return Some("accepted Codex base review evidence is missing".into());
        }
        return Some(format!("Codex base review status is {status}"));
    }
    if state.run.codex_base_review_base.as_deref() != Some(parent) {
        let review_base = state
            .run
            .codex_base_review_base
            .as_deref()
            .unwrap_or("missing");
        return Some(format!(
            "accepted Codex base review is for {review_base}, but current parent is {parent}"
        ));
    }
    let Some(reviewed_at) = state.run.codex_base_reviewed_at.as_deref() else {
        return Some("accepted Codex base review timestamp is missing".into());
    };
    let Some(reviewed_at_normalized) = task_run::normalized_utc_timestamp(reviewed_at) else {
        return Some(format!(
            "accepted Codex base review timestamp is invalid: {reviewed_at}"
        ));
    };
    if reviewed_at_normalized < reported_at {
        return Some(format!(
            "accepted Codex base review at {reviewed_at} is older than latest Agent Completion Report at {}",
            state.run.last_reported_at.as_deref().unwrap_or("<missing>")
        ));
    }
    None
}

fn codex_base_review_status(state: &WorkflowTaskState, parent: &str) -> String {
    fresh_codex_base_review_issue(state, parent)
        .unwrap_or_else(|| "accepted Codex base review evidence is incomplete".into())
}

fn codex_review_parent(metadata: &WorkflowMetadata, row: &WorkflowTask) -> Result<String> {
    if let Some(parent) = row.parent.clone() {
        return Ok(parent);
    }
    match metadata.base_mode.as_str() {
        "explicit" => metadata
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Workflow base_mode is explicit but base is missing")),
        other => bail!("workflow pass only supports explicit base, found {other}"),
    }
}

fn codex_surface_review_command(parent: &str) -> String {
    format!("/review --base {}", shell_arg(parent))
}

fn codex_cli_review_command(parent: &str) -> String {
    format!("codex review --base {}", shell_arg(parent))
}
