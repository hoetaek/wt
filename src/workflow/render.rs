use crate::context::Ctx;
use crate::task as task_store;
use crate::task_run::{
    STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::workflow::run::WorkflowTaskState;
use crate::workflow::{WorkflowMetadata, WorkflowPullRequestMode, WorkflowTask};
use std::path::Path;

enum WorkflowCoordinatorHandoff<'a> {
    ReportOnly,
    Stack {
        workflow_path: &'a Path,
        row: &'a WorkflowTask,
    },
}

pub(crate) fn base_label(metadata: &WorkflowMetadata) -> String {
    metadata
        .base
        .clone()
        .unwrap_or_else(|| format!("({})", metadata.base_mode))
}

pub(crate) fn workflow_task_label(row: &WorkflowTask) -> &str {
    if row.task.trim().is_empty() {
        "workflow-task"
    } else {
        row.task.trim()
    }
}

pub(crate) fn workflow_task_title_label(ctx: &Ctx, key: &str) -> String {
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

pub(crate) fn workflow_filtered_task_summary<F>(
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

pub(crate) fn workflow_selection_status_counts(items: &[WorkflowTaskState]) -> String {
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

pub(crate) fn workflow_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

pub(crate) fn workflow_objective_prompt_context(objective: Option<&str>) -> Option<String> {
    objective
        .map(str::trim)
        .filter(|objective| !objective.is_empty())
        .map(|objective| format!("Workflow objective:\n\n{objective}"))
}

pub(crate) fn workflow_single_task_prompt_intro() -> &'static str {
    "Use this task before changing code."
}

pub(crate) fn workflow_single_group_prompt_intro() -> &'static str {
    "Use these tasks before changing code. Work in this single workspace and address every selected TaskDocument."
}

pub(crate) fn workflow_batch_task_prompt_intro() -> &'static str {
    "Use this task before changing code."
}

pub(crate) fn workflow_stack_task_prompt_intro() -> &'static str {
    "Use this task before changing code."
}

pub(crate) fn no_tasks_selected_message() -> &'static str {
    "No tasks selected"
}

pub(crate) fn prepared_workflow_message(workflow_path: &Path) -> String {
    format!("Prepared workflow: {}", workflow_path.display())
}

pub(crate) fn no_runnable_workflow_tasks_message() -> &'static str {
    "No prepared or failed tasks to run in this workflow."
}

pub(crate) fn starting_workflow_task_message(task: &str) -> String {
    format!("Starting {task}")
}

pub(crate) fn started_workflow_task_message(task: &str) -> String {
    format!("Started {task}")
}

pub(crate) fn failed_workflow_task_message(task: &str, error: &str) -> String {
    format!("Failed {task}: {error}")
}

pub(crate) fn stack_task_already_running_message(
    workflow_path: &Path,
    row: &WorkflowTask,
) -> String {
    format!(
        "Workflow stack task {} is already running. Mark it complete with: wt workflow complete {} {}",
        workflow_task_label(row),
        workflow_path.display(),
        workflow_task_label(row)
    )
}

pub(crate) fn started_stack_task_message(workflow_path: &Path, row: &WorkflowTask) -> String {
    format!(
        "Started workflow task {}. Mark it complete with: wt workflow complete {} {}",
        workflow_task_label(row),
        workflow_path.display(),
        workflow_task_label(row)
    )
}

pub(crate) fn render_single_workflow_snapshot(states: &[WorkflowTaskState]) -> String {
    let mut content = String::new();
    content.push_str("Selected TaskDocuments:\n");
    for state in states {
        content.push_str(&format!("- {}: {}\n", state.row.task, state.path));
    }
    for state in states {
        content.push_str(&format!("\n--- {} ({}) ---\n", state.row.task, state.path));
        content.push_str(state.content.trim_end());
        content.push('\n');
    }
    content
}

pub(crate) fn single_workflow_group_title(states: &[WorkflowTaskState]) -> String {
    let first = states
        .first()
        .map(|state| state.document.title_or_key(&state.row.task))
        .unwrap_or_else(|| "workflow".into());
    format!("{}개 작업: {first}", states.len())
}

#[cfg(test)]
pub(crate) fn workflow_single_task_prompt_content(content: &str) -> String {
    workflow_task_prompt_content(content, &workflow_single_task_handoff_section())
}

#[cfg(test)]
pub(crate) fn workflow_batch_task_prompt_content(content: &str) -> String {
    workflow_task_prompt_content(content, &workflow_batch_task_handoff_section())
}

#[cfg(test)]
pub(crate) fn workflow_stack_task_prompt_content(
    content: &str,
    workflow_path: &Path,
    row: &WorkflowTask,
) -> String {
    workflow_task_prompt_content(
        content,
        &workflow_stack_task_handoff_section(workflow_path, row),
    )
}

pub(crate) fn workflow_single_task_handoff_section() -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::ReportOnly)
}

pub(crate) fn workflow_batch_task_handoff_section() -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::ReportOnly)
}

pub(crate) fn workflow_stack_task_handoff_section(
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

pub(crate) fn shell_arg(value: &str) -> String {
    let safe = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='));
    if safe && !value.is_empty() {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}
