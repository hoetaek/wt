use crate::context::Ctx;
use crate::task as task_store;
use crate::task_run::{
    STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::workflow::run::WorkflowTaskState;
use crate::workflow::{
    WorkflowLandingPolicy, WorkflowMetadata, WorkflowPolicy, WorkflowPullRequestMode, WorkflowTask,
};
use std::path::Path;

enum WorkflowCoordinatorHandoff<'a> {
    Task {
        policy: &'a WorkflowPolicy,
        pr_base: &'a str,
        pr_base_label: &'static str,
        issue_closing_references: &'a [String],
        completion: Option<WorkflowCompletion<'a>>,
    },
}

struct WorkflowCompletion<'a> {
    workflow_path: &'a Path,
    row: Option<&'a WorkflowTask>,
    target: Option<String>,
    run_next: bool,
}

impl WorkflowCompletion<'_> {
    fn complete_command(&self) -> String {
        let mut command = format!(
            "wt workflow complete {}",
            shell_arg(&self.workflow_path.to_string_lossy())
        );
        if let Some(target) = self.target.as_deref() {
            command.push(' ');
            command.push_str(&shell_arg(target));
        } else if let Some(row) = self.row {
            command.push(' ');
            command.push_str(&shell_arg(workflow_task_label(row)));
        }
        if self.run_next {
            command.push_str(" --run-next");
        }
        command
    }
}

fn review_followup(policy: &WorkflowPolicy) -> &'static str {
    match policy.pull_request {
        WorkflowPullRequestMode::None => {
            "After the report is sent, keep ownership of coordinator review follow-up for this task. If coordinator feedback asks for changes, implement the changes in this branch, rerun the relevant checks, commit, push if this branch tracks a remote, and send an updated Agent Completion Report."
        }
        WorkflowPullRequestMode::Draft | WorkflowPullRequestMode::Ready => {
            "After the pull request is opened and the report is sent, keep ownership of review follow-up for this task. If Codex/GitHub review or coordinator feedback asks for changes, implement the changes in this branch, rerun the relevant checks, commit, push, update the pull request body if it became stale, and send an updated Agent Completion Report."
        }
    }
}

fn landing_wait_text(policy: &WorkflowPolicy) -> &'static str {
    match policy.landing {
        WorkflowLandingPolicy::Manual => {
            "When review passes, wait for the coordinator to land and clean up the workflow task explicitly."
        }
        WorkflowLandingPolicy::Auto => {
            "When review passes, wait for the coordinator to proceed with landing and cleanup after its dirty-worktree, check, unresolved-review, and ancestry safety checks pass."
        }
    }
}

#[cfg(test)]
fn default_workflow_policy() -> WorkflowPolicy {
    WorkflowPolicy {
        pull_request: WorkflowPullRequestMode::None,
        landing: WorkflowLandingPolicy::Manual,
    }
}

#[cfg(test)]
pub(crate) fn workflow_task_prompt_content_with_policy(
    content: &str,
    workflow_path: &Path,
    row: &WorkflowTask,
    policy: &WorkflowPolicy,
) -> String {
    let validated_parent = row.parent.as_deref().unwrap_or("<workflow-parent>");
    workflow_task_prompt_content_with_policy_and_parent(
        content,
        workflow_path,
        row,
        policy,
        validated_parent,
    )
}

#[cfg(test)]
pub(crate) fn workflow_task_prompt_content_with_policy_and_parent(
    content: &str,
    workflow_path: &Path,
    row: &WorkflowTask,
    policy: &WorkflowPolicy,
    validated_parent: &str,
) -> String {
    workflow_task_prompt_content(
        content,
        &workflow_stack_task_handoff_section(workflow_path, row, policy, validated_parent, &[]),
    )
}

#[cfg(test)]
pub(crate) fn test_workflow_policy(pull_request: WorkflowPullRequestMode) -> WorkflowPolicy {
    WorkflowPolicy {
        pull_request,
        landing: WorkflowLandingPolicy::Manual,
    }
}

#[cfg(test)]
pub(crate) fn test_auto_landing_policy() -> WorkflowPolicy {
    WorkflowPolicy {
        pull_request: WorkflowPullRequestMode::None,
        landing: WorkflowLandingPolicy::Auto,
    }
}

#[cfg(test)]
const TEST_WORKFLOW_BASE: &str = "main";

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

pub(crate) fn workflow_metadata_prompt_context(metadata: &WorkflowMetadata) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(title) = metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        sections.push(format!("Workflow title:\n\n{title}"));
    }
    if let Some(body) = metadata
        .body
        .as_deref()
        .map(str::trim)
        .filter(|body| !body.is_empty())
    {
        sections.push(format!("Workflow body:\n\n{body}"));
    }
    if let Some(origin) = &metadata.origin {
        let provider = origin.provider.trim();
        let id = origin.id.trim();
        if !provider.is_empty() && !id.is_empty() {
            sections.push(format!("Workflow origin: {provider}:{id}"));
        }
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
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
        shell_arg(&workflow_path.to_string_lossy()),
        shell_arg(workflow_task_label(row))
    )
}

pub(crate) fn started_stack_task_message(workflow_path: &Path, row: &WorkflowTask) -> String {
    format!(
        "Started workflow task {}. Mark it complete with: wt workflow complete {} {}",
        workflow_task_label(row),
        shell_arg(&workflow_path.to_string_lossy()),
        shell_arg(workflow_task_label(row))
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
    workflow_task_prompt_content(
        content,
        &workflow_single_task_handoff_section(
            Path::new("/repo/.local/workflows/test.toml"),
            Some(&WorkflowTask::new("task", "run-task")),
            &default_workflow_policy(),
            TEST_WORKFLOW_BASE,
            &[],
        ),
    )
}

#[cfg(test)]
pub(crate) fn workflow_single_task_prompt_content_for_policy(
    content: &str,
    policy: &WorkflowPolicy,
) -> String {
    workflow_task_prompt_content(
        content,
        &workflow_single_task_handoff_section(
            Path::new("/repo/.local/workflows/test.toml"),
            Some(&WorkflowTask::new("task", "run-task")),
            policy,
            TEST_WORKFLOW_BASE,
            &[],
        ),
    )
}

#[cfg(test)]
pub(crate) fn workflow_single_task_prompt_content_for_policy_and_closing_refs(
    content: &str,
    policy: &WorkflowPolicy,
    issue_closing_references: &[String],
) -> String {
    workflow_task_prompt_content(
        content,
        &workflow_single_task_handoff_section(
            Path::new("/repo/.local/workflows/test.toml"),
            Some(&WorkflowTask::new("task", "run-task")),
            policy,
            TEST_WORKFLOW_BASE,
            issue_closing_references,
        ),
    )
}

#[cfg(test)]
pub(crate) fn workflow_batch_task_prompt_content(content: &str) -> String {
    let row = WorkflowTask::new("task", "run-task");
    workflow_task_prompt_content(
        content,
        &workflow_batch_task_handoff_section(
            Path::new("/repo/.local/workflows/test.toml"),
            &row,
            &default_workflow_policy(),
            TEST_WORKFLOW_BASE,
            &[],
        ),
    )
}

#[cfg(test)]
pub(crate) fn workflow_batch_task_prompt_content_for_policy(
    content: &str,
    policy: &WorkflowPolicy,
) -> String {
    let row = WorkflowTask::new("task", "run-task");
    workflow_task_prompt_content(
        content,
        &workflow_batch_task_handoff_section(
            Path::new("/repo/.local/workflows/test.toml"),
            &row,
            policy,
            TEST_WORKFLOW_BASE,
            &[],
        ),
    )
}

#[cfg(test)]
pub(crate) fn workflow_stack_task_prompt_content(
    content: &str,
    workflow_path: &Path,
    row: &WorkflowTask,
) -> String {
    workflow_task_prompt_content_with_policy(
        content,
        workflow_path,
        row,
        &default_workflow_policy(),
    )
}

pub(crate) fn workflow_single_task_handoff_section(
    workflow_path: &Path,
    row: Option<&WorkflowTask>,
    policy: &WorkflowPolicy,
    pr_base: &str,
    issue_closing_references: &[String],
) -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::Task {
        policy,
        pr_base,
        pr_base_label: "workflow base branch",
        issue_closing_references,
        completion: Some(WorkflowCompletion {
            workflow_path,
            row,
            target: None,
            run_next: false,
        }),
    })
}

pub(crate) fn workflow_batch_task_handoff_section(
    workflow_path: &Path,
    row: &WorkflowTask,
    policy: &WorkflowPolicy,
    pr_base: &str,
    issue_closing_references: &[String],
) -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::Task {
        policy,
        pr_base,
        pr_base_label: "workflow base branch",
        issue_closing_references,
        completion: Some(WorkflowCompletion {
            workflow_path,
            row: Some(row),
            target: None,
            run_next: false,
        }),
    })
}

pub(crate) fn workflow_matrix_task_handoff_section(
    workflow_path: &Path,
    row: &WorkflowTask,
    profile: &str,
    policy: &WorkflowPolicy,
    pr_base: &str,
    issue_closing_references: &[String],
) -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::Task {
        policy,
        pr_base,
        pr_base_label: "workflow base branch",
        issue_closing_references,
        completion: Some(WorkflowCompletion {
            workflow_path,
            row: Some(row),
            target: Some(format!("{}:{profile}", workflow_task_label(row))),
            run_next: false,
        }),
    })
}

pub(crate) fn workflow_stack_task_handoff_section(
    workflow_path: &Path,
    row: &WorkflowTask,
    policy: &WorkflowPolicy,
    validated_parent: &str,
    issue_closing_references: &[String],
) -> String {
    workflow_coordinator_handoff_section(WorkflowCoordinatorHandoff::Task {
        policy,
        pr_base: validated_parent,
        pr_base_label: "workflow parent branch",
        issue_closing_references,
        completion: Some(WorkflowCompletion {
            workflow_path,
            row: Some(row),
            target: None,
            run_next: true,
        }),
    })
}

#[cfg(test)]
fn workflow_task_prompt_content(content: &str, handoff: &str) -> String {
    format!("{}\n\n{}", handoff, content.trim_end())
}

fn workflow_coordinator_handoff_section(handoff: WorkflowCoordinatorHandoff<'_>) -> String {
    let (pull_request_instructions, pr_report_value, after_send) = workflow_handoff_policy(handoff);
    let send_command = format!(
        "cmux send --workspace {{{{coordinator_cmux_workspace}}}} --surface {{{{coordinator_cmux_surface}}}} \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR={pr_report_value}; Risks or follow-ups=<risks>\"\n{{{{coordinator_enter_command}}}}"
    );

    format!(
        "## Workflow Coordinator Handoff\n\nSend the Agent Completion Report back to the coordinator cmux surface that started this workflow:\n\n```bash\n{send_command}\n```\n\n{pull_request_instructions}\n\n{after_send}\n\nIf the coordinator cmux target is unavailable or stale, leave the same report in this task session and wait."
    )
}

fn workflow_handoff_policy(
    handoff: WorkflowCoordinatorHandoff<'_>,
) -> (String, &'static str, String) {
    match handoff {
        WorkflowCoordinatorHandoff::Task {
            policy,
            pr_base,
            pr_base_label,
            issue_closing_references,
            completion,
        } => {
            let pr_report_value = match policy.pull_request {
                WorkflowPullRequestMode::None => "none",
                WorkflowPullRequestMode::Draft | WorkflowPullRequestMode::Ready => "<pr-url>",
            };
            let pull_request_instructions = match policy.pull_request {
                WorkflowPullRequestMode::Draft | WorkflowPullRequestMode::Ready => {
                    let pr_command = workflow_pr_command(policy.pull_request, pr_base);
                    let mode_instruction = match policy.pull_request {
                        WorkflowPullRequestMode::Draft => {
                            "open a draft pull request and leave it draft"
                        }
                        WorkflowPullRequestMode::Ready => {
                            "open a pull request that is ready for review immediately"
                        }
                        WorkflowPullRequestMode::None => unreachable!(),
                    };
                    let closing_instruction =
                        issue_closing_instruction(issue_closing_references);
                    format!(
                        "Workflow policy sets `pull_request = \"{}\"`. When this task is complete and committed, push the branch and {mode_instruction} against the {pr_base_label}. Create `<pr-body-file>` from `.github/pull_request_template.md` and fill it with a review-focused PR description covering summary, context, changes, validation, and risks/follow-ups{closing_instruction} before creating the pull request:\n\n```bash\n{pr_command}\n```",
                        policy.pull_request.as_str(),
                    )
                }
                WorkflowPullRequestMode::None => {
                    "Workflow policy sets `pull_request = \"none\"`. When this task is complete and committed, do not open a pull request for this workflow task; report `PR=none`.".into()
                }
            };

            let after_send = if let Some(completion) = completion {
                format!(
                    "{}\n\nWhen review passes, wait for the coordinator to advance the workflow. The coordinator will run:\n\n```bash\n{}\n```\n\n{}",
                    review_followup(policy),
                    completion.complete_command(),
                    landing_wait_text(policy)
                )
            } else {
                format!(
                    "{}\n\n{}",
                    review_followup(policy),
                    landing_wait_text(policy)
                )
            };
            (pull_request_instructions, pr_report_value, after_send)
        }
    }
}

fn issue_closing_instruction(issue_closing_references: &[String]) -> String {
    if issue_closing_references.is_empty() {
        return String::new();
    }

    let keywords = issue_closing_references
        .iter()
        .map(|reference| format!("`Closes {reference}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        ", and issue-closing keywords in `<pr-body-file>` so linked provider issues close when the pull request merges: {keywords}"
    )
}

fn workflow_pr_command(mode: WorkflowPullRequestMode, parent_branch: &str) -> String {
    let create_args = match mode {
        WorkflowPullRequestMode::Draft => "--draft --body-file <pr-body-file>",
        WorkflowPullRequestMode::Ready => "--body-file <pr-body-file>",
        WorkflowPullRequestMode::None => unreachable!("pull_request = none does not create PRs"),
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
