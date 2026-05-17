use super::state::WorkflowTaskState;
use crate::context::Ctx;
use crate::task as task_store;
use crate::task_run::{
    STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::workflow::{WorkflowMetadata, WorkflowPullRequestMode, WorkflowTask};
use std::path::Path;

enum WorkflowCoordinatorHandoff<'a> {
    ReportOnly,
    Stack {
        workflow_path: &'a Path,
        row: &'a WorkflowTask,
    },
}

pub(super) fn base_label(metadata: &WorkflowMetadata) -> String {
    metadata
        .base
        .clone()
        .unwrap_or_else(|| format!("({})", metadata.base_mode))
}

pub(super) fn workflow_task_label(row: &WorkflowTask) -> &str {
    if row.task.trim().is_empty() {
        "workflow-task"
    } else {
        row.task.trim()
    }
}

pub(super) fn workflow_task_title_label(ctx: &Ctx, key: &str) -> String {
    match task_store::read_task_document(ctx, key) {
        Ok(document) => {
            let title = document.title_or_key(key);
            if title == key {
                key.to_string()
            } else {
                format!("{title} ({key})")
            }
        }
        Err(_) => format!("{key} (missing)"),
    }
}

pub(super) fn workflow_filtered_task_summary<F>(
    ctx: &Ctx,
    states: &[WorkflowTaskState],
    include: F,
) -> Option<String>
where
    F: Fn(&WorkflowTaskState) -> bool,
{
    let matching = states
        .iter()
        .filter(|state| include(state))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return None;
    }

    let visible = matching
        .iter()
        .take(3)
        .map(|state| workflow_task_title_label(ctx, &state.row.task))
        .collect::<Vec<_>>();
    let mut summary = visible.join(", ");
    if matching.len() > visible.len() {
        summary.push_str(&format!(", ...(+{})", matching.len() - visible.len()));
    }
    Some(summary)
}

pub(super) fn workflow_selection_status_counts(items: &[WorkflowTaskState]) -> String {
    let counts = [
        STATUS_PREPARED,
        STATUS_RUNNING,
        STATUS_DONE,
        STATUS_FAILED,
        STATUS_SKIPPED,
    ]
    .iter()
    .map(|status| {
        let count = items
            .iter()
            .filter(|item| item.run.status == *status)
            .count();
        (status, count)
    })
    .filter(|(_, count)| *count > 0)
    .map(|(status, count)| format!("{count} {status}"))
    .collect::<Vec<_>>()
    .join(" / ");

    if counts.is_empty() {
        "none".into()
    } else {
        counts
    }
}

pub(super) fn workflow_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
pub(super) fn workflow_single_task_prompt_content(content: &str) -> String {
    workflow_task_prompt_content(content, &workflow_single_task_handoff_section())
}

#[cfg(test)]
pub(super) fn workflow_batch_task_prompt_content(content: &str) -> String {
    workflow_task_prompt_content(content, &workflow_batch_task_handoff_section())
}

#[cfg(test)]
pub(super) fn workflow_stack_task_prompt_content(
    content: &str,
    workflow_path: &Path,
    row: &WorkflowTask,
) -> String {
    workflow_task_prompt_content(
        content,
        &workflow_stack_task_handoff_section(workflow_path, row),
    )
}

pub(super) fn workflow_single_task_handoff_section() -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::ReportOnly)
}

pub(super) fn workflow_batch_task_handoff_section() -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::ReportOnly)
}

pub(super) fn workflow_stack_task_handoff_section(
    workflow_path: &Path,
    row: &WorkflowTask,
) -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::Stack { workflow_path, row })
}

#[cfg(test)]
fn workflow_task_prompt_content(content: &str, handoff: &str) -> String {
    format!("{}\n\n{}", handoff, content.trim_end())
}

fn workflow_coordinator_handoff_section(handoff: WorkflowCoordinatorHandoff<'_>) -> String {
    let (pull_request_instructions, pr_report_value, complete_command) =
        workflow_handoff_policy(handoff);
    let send_command = format!(
        "cmux send --workspace {{{{coordinator_cmux_workspace}}}} --surface {{{{coordinator_cmux_surface}}}} \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR={pr_report_value}; Risks or follow-ups=<risks>\"\n{{{{coordinator_enter_command}}}}"
    );

    let after_send = if let Some((complete_command, review_followup)) = complete_command {
        format!(
            "{review_followup}\n\nWhen review passes, wait for the coordinator to advance the workflow. The coordinator will run:\n\n```bash\n{complete_command}\n```"
        )
    } else {
        "After sending the report, wait for review. If coordinator feedback asks for changes, implement the changes in this branch, rerun the relevant checks, commit, push if this branch tracks a remote, and send an updated Agent Completion Report. When review passes, wait for the coordinator to land and clean up the workflow task explicitly.".into()
    };

    format!(
        "## Workflow Coordinator Handoff\n\nSend the Agent Completion Report back to the coordinator cmux surface that started this workflow:\n\n```bash\n{}\n```\n\n{}\n\n{}\n\nIf the coordinator cmux target is unavailable or stale, leave the same report in this task session and wait.",
        send_command, pull_request_instructions, after_send
    )
}

fn workflow_handoff_policy(
    handoff: WorkflowCoordinatorHandoff<'_>,
) -> (String, &'static str, Option<(String, &'static str)>) {
    match handoff {
        WorkflowCoordinatorHandoff::ReportOnly => (
            "This workflow mode has no pull-request handoff intent. When this task is complete and committed, do not open a pull request for this workflow task; report `PR=none`.".into(),
            "none",
            None,
        ),
        WorkflowCoordinatorHandoff::Stack { workflow_path, row } => {
            let parent_branch = row.parent.as_deref().unwrap_or("<workflow-parent>");
            let pull_request = row.pull_request;
            let pr_report_value = if pull_request.is_some() {
                "<pr-url>"
            } else {
                "none"
            };
            let complete_command = format!(
                "wt workflow complete {} {} --run-next",
                shell_arg(&workflow_path.to_string_lossy()),
                shell_arg(workflow_task_label(row))
            );
            let pull_request_instructions = match pull_request {
                Some(mode) => {
                    let pr_command = workflow_pr_command(mode, parent_branch);
                    let mode_instruction = match mode {
                        WorkflowPullRequestMode::Draft => {
                            "open a draft pull request against the workflow parent branch and leave it draft"
                        }
                        WorkflowPullRequestMode::Ready => {
                            "open a pull request against the workflow parent branch that is ready for review immediately"
                        }
                    };
                    format!(
                        "Workflow task metadata sets `pull_request = \"{}\"`. When this task is complete and committed, push the branch and {mode_instruction}. Create `<pr-body-file>` from `.github/pull_request_template.md` and fill it with a review-focused PR description covering summary, context, changes, validation, and risks/follow-ups before creating the pull request:\n\n```bash\n{pr_command}\n```",
                        mode.as_str()
                    )
                }
                None => {
                    "Workflow task metadata omits `pull_request`. When this task is complete and committed, do not open a pull request for this workflow task.".into()
                }
            };
            let review_followup = if pull_request.is_some() {
                "After the pull request is opened and the report is sent, keep ownership of review follow-up for this task. If Codex/GitHub review or coordinator feedback asks for changes, implement the changes in this branch, rerun the relevant checks, commit, push, update the pull request body if it became stale, and send an updated Agent Completion Report."
            } else {
                "After the report is sent, keep ownership of coordinator review follow-up for this task. If coordinator feedback asks for changes, implement the changes in this branch, rerun the relevant checks, commit, push if this branch tracks a remote, and send an updated Agent Completion Report."
            };
            (
                pull_request_instructions,
                pr_report_value,
                Some((complete_command, review_followup)),
            )
        }
    }
}

fn workflow_pr_command(mode: WorkflowPullRequestMode, parent_branch: &str) -> String {
    let create_args = match mode {
        WorkflowPullRequestMode::Draft => "--draft --body-file <pr-body-file>",
        WorkflowPullRequestMode::Ready => "--body-file <pr-body-file>",
    };
    format!(
        "git push -u origin HEAD\n# Create <pr-body-file> from .github/pull_request_template.md and fill it before creating the pull request.\npr_url=$(gh pr create {create_args} --base {} --fill)",
        shell_arg(parent_branch)
    )
}

pub(super) fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}
