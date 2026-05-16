use crate::cli::BaseMode;
use crate::commands::issue;
use crate::commands::issue_selection::{self, SelectedIssue};
use crate::commands::task::{self, PreparedTask};
use crate::commands::task_run::{
    self, STATUS_DONE, STATUS_FAILED, STATUS_PREPARED, STATUS_RUNNING, STATUS_SKIPPED,
};
use crate::config::{Config, validate_profile_name};
use crate::context::Ctx;
use crate::error::WtError;
use crate::services::git::GitService;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const STATUS_PARTIAL: &str = "partial";

pub fn task(
    ctx: &Ctx,
    tasks: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;
    let prepared_tasks = task::prepare_named_tasks(ctx, tasks)?;
    write_prepared_stack(ctx, profile, base, prepared_tasks)
}

pub fn issue(
    ctx: &Ctx,
    issues: &[String],
    profile: Option<&str>,
    base: &Option<String>,
) -> Result<()> {
    validate_profile(ctx, profile)?;

    let selected_issues = if issues.is_empty() {
        select_ordered_issues(ctx)?
            .into_iter()
            .map(|issue| issue.identifier)
            .collect::<Vec<_>>()
    } else {
        issues.to_vec()
    };

    if selected_issues.is_empty() {
        ctx.ui.print_warning("No issues selected");
        return Ok(());
    }

    let prepared_tasks = task::prepare_issue_tasks(ctx, &selected_issues)?;
    write_prepared_stack(ctx, profile, base, prepared_tasks)
}

fn write_prepared_stack(
    ctx: &Ctx,
    profile: Option<&str>,
    base: &Option<String>,
    prepared_tasks: Vec<PreparedTask>,
) -> Result<()> {
    if prepared_tasks.is_empty() {
        ctx.ui.print_warning("No tasks selected");
        return Ok(());
    }

    let resolved_base = resolve_stack_base(ctx, base)?;
    let stack_path = next_available_stack_path(ctx)?;
    write_prepared_stack_at_path(ctx, profile, &resolved_base, &stack_path, prepared_tasks)?;

    ctx.ui
        .print_step(&format!("Created stack: {}", stack_path.display()));
    Ok(())
}

fn write_prepared_stack_at_path(
    ctx: &Ctx,
    profile: Option<&str>,
    resolved_base: &str,
    stack_path: &Path,
    prepared_tasks: Vec<PreparedTask>,
) -> Result<()> {
    let prepared = stack_tasks_from_prepared(
        ctx,
        stack_path,
        prepared_tasks,
        Some(resolved_base.to_string()),
    )?;
    let now = current_utc_timestamp();
    let stack = StackMetadata {
        profile: profile.map(str::to_string),
        base_mode: "explicit".into(),
        base: Some(resolved_base.to_string()),
        status: STATUS_PREPARED.into(),
        created_at: now.clone(),
        updated_at: now,
        tasks: prepared.tasks,
    };
    if let Err(err) = write_stack_metadata(stack_path, &stack) {
        rollback_task_runs(&prepared.task_runs);
        return Err(err);
    }
    Ok(())
}

pub fn run(ctx: &Ctx, stack: &str) -> Result<()> {
    let stack_path = resolve_stack_path(ctx, stack)?;
    let mut metadata = read_stack_metadata(&stack_path)?;
    validate_profile(ctx, metadata.profile.as_deref())?;

    if metadata.tasks.is_empty() {
        bail!("Stack has no tasks: {}", stack_path.display());
    }

    let task_states = read_stack_task_states(ctx, &stack_path, &metadata)?;

    if let Some(state) = task_states
        .iter()
        .find(|state| state.run.status == STATUS_RUNNING)
    {
        bail!(
            "Stack task {} is already running. Mark it complete with: wt stack complete {} {}",
            state.stack_task.label(),
            stack_path.display(),
            state.stack_task.label()
        );
    }

    let Some(idx) = next_runnable_task(&task_states) else {
        ctx.ui
            .print_step("No prepared or failed tasks to run in this stack.");
        metadata.status = summarize_stack_status(&task_states);
        metadata.updated_at = current_utc_timestamp();
        write_stack_metadata(&stack_path, &metadata)?;
        return Ok(());
    };

    let parent = parent_for_task(ctx, &metadata, &task_states, idx)?;
    metadata.status = STATUS_RUNNING.into();
    metadata.updated_at = current_utc_timestamp();
    metadata.tasks[idx].parent = Some(parent.clone());
    update_stack_task_run(ctx, &stack_path, &metadata.tasks[idx], STATUS_RUNNING, None)?;
    write_stack_metadata(&stack_path, &metadata)?;

    let result = run_stack_task(
        ctx,
        &stack_path,
        &metadata.tasks[idx],
        &parent,
        metadata.profile.as_deref(),
    );

    match result {
        Ok(result) => {
            task::write_task_branch(ctx, &metadata.tasks[idx].task, &result.branch_name)?;
            update_stack_task_run(ctx, &stack_path, &metadata.tasks[idx], STATUS_RUNNING, None)?;
            ctx.ui.print_step(&format!(
                "Started stack task {}. Mark it complete with: wt stack complete {} {}",
                metadata.tasks[idx].label(),
                stack_path.display(),
                metadata.tasks[idx].label()
            ));
        }
        Err(err) => {
            if err
                .downcast_ref::<WtError>()
                .is_some_and(|err| matches!(err, WtError::Cancelled))
            {
                update_stack_task_run(
                    ctx,
                    &stack_path,
                    &metadata.tasks[idx],
                    STATUS_SKIPPED,
                    Some("User cancelled"),
                )?;
                metadata.status = summarize_current_stack_status(ctx, &stack_path, &metadata)?;
                metadata.updated_at = current_utc_timestamp();
                write_stack_metadata(&stack_path, &metadata)?;
                return Ok(());
            }

            let error = err.to_string();
            update_stack_task_run(
                ctx,
                &stack_path,
                &metadata.tasks[idx],
                STATUS_FAILED,
                Some(&error),
            )?;
        }
    }

    metadata.status = summarize_current_stack_status(ctx, &stack_path, &metadata)?;
    metadata.updated_at = current_utc_timestamp();
    write_stack_metadata(&stack_path, &metadata)?;
    ctx.ui
        .print_step(&format!("Stack status: {}", metadata.status));

    if metadata.status == STATUS_FAILED {
        bail!("Stack failed: {}", stack_path.display());
    }

    Ok(())
}

pub fn show(ctx: &Ctx, stack: Option<&str>) -> Result<()> {
    let stack_path = match stack {
        Some(target) => resolve_stack_path(ctx, target)?,
        None => latest_stack_path(ctx)?,
    };
    let metadata = read_stack_metadata(&stack_path)?;
    let task_states = read_stack_task_states(ctx, &stack_path, &metadata)?;
    let status = summarize_stack_status(&task_states);

    ctx.ui
        .print_step(&format!("Stack: {}", stack_path.display()));
    ctx.ui.print_dim(&format!("  Status: {status}"));
    ctx.ui
        .print_dim(&format!("  Base: {}", describe_stack_base(&metadata)?));
    ctx.ui.print_dim(&format!(
        "  Profile: {}",
        metadata.profile.as_deref().unwrap_or("(effective config)")
    ));
    ctx.ui.print_dim(&format!(
        "  Tasks: {} ({})",
        metadata.tasks.len(),
        stack_status_counts(&task_states)
    ));

    for state in &task_states {
        let item = &state.stack_task;
        let status = state.run.status.as_str();
        let task_doc = task::read_task_document(ctx, &item.task);
        let title = task_doc
            .as_ref()
            .map(|doc| doc.title_or_key(&item.task))
            .unwrap_or_else(|_| "(missing task)".into());
        let summary = if title.is_empty() {
            format!("  {}. {} [{}]", state.idx + 1, item.label(), status)
        } else {
            format!(
                "  {}. {} [{}] {}",
                state.idx + 1,
                item.label(),
                status,
                title
            )
        };
        ctx.ui.print_dim(&summary);
        ctx.ui.print_dim(&format!(
            "     Task: {}",
            task::task_relative_path(&item.task)
        ));
        match task_doc {
            Ok(doc) => {
                if !doc.branch.trim().is_empty() {
                    ctx.ui.print_dim(&format!("     Branch: {}", doc.branch));
                }
            }
            Err(err) => ctx.ui.print_dim(&format!("     Task error: {err}")),
        }
        if let Some(parent) = item.parent.as_deref() {
            ctx.ui.print_dim(&format!("     Parent: {parent}"));
        }
        if let Some(error) = state.run.error.as_deref() {
            if !error.trim().is_empty() {
                ctx.ui.print_dim(&format!("     Error: {error}"));
            }
        }
    }

    Ok(())
}

pub fn edit(ctx: &Ctx, stack: Option<&str>) -> Result<()> {
    let stack_path = match stack {
        Some(target) => resolve_stack_path(ctx, target)?,
        None => latest_stack_path(ctx)?,
    };
    crate::commands::editor::open_file(ctx, &stack_path)
}

pub fn complete(ctx: &Ctx, stack: &str, task: Option<&str>, run_next: bool) -> Result<()> {
    let stack_path = resolve_stack_path(ctx, stack)?;
    let mut metadata = read_stack_metadata(&stack_path)?;
    let task_states = read_stack_task_states(ctx, &stack_path, &metadata)?;

    let Some(state) = task_states
        .iter()
        .find(|state| state.run.status == STATUS_RUNNING)
    else {
        ctx.ui.print_warning("No running stack task found");
        return Ok(());
    };
    let idx = state.idx;

    if let Some(task) = task {
        let running = &metadata.tasks[idx];
        if !stack_task_matches(ctx, running, task) {
            bail!(
                "Running stack task is {}, but complete was requested for {task}",
                running.label()
            );
        }
    }

    validate_completable_stack_task(ctx, &metadata.tasks[idx])?;

    update_stack_task_run(ctx, &stack_path, &metadata.tasks[idx], STATUS_DONE, None)?;
    metadata.status = summarize_current_stack_status(ctx, &stack_path, &metadata)?;
    metadata.updated_at = current_utc_timestamp();
    write_stack_metadata(&stack_path, &metadata)?;

    ctx.ui
        .print_step(&format!("Marked {} done", metadata.tasks[idx].label()));
    if run_next {
        run(ctx, stack_path.to_string_lossy().as_ref())?;
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StackMetadata {
    #[serde(default)]
    profile: Option<String>,
    base_mode: String,
    #[serde(default)]
    base: Option<String>,
    #[serde(default = "default_stack_status")]
    status: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    tasks: Vec<StackTask>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StackTask {
    task: String,
    run: String,
    #[serde(default)]
    parent: Option<String>,
}

impl StackTask {
    fn from_prepared(task: PreparedTask, run: String, parent: Option<String>) -> Self {
        Self {
            task: task.key,
            run,
            parent,
        }
    }

    fn label(&self) -> &str {
        if self.task.trim().is_empty() {
            "stack-task"
        } else {
            self.task.trim()
        }
    }
}

fn select_ordered_issues(ctx: &Ctx) -> Result<Vec<SelectedIssue>> {
    let selected = issue_selection::select_issues(ctx, "Select issues for stack")?;
    if selected.len() <= 1 {
        return Ok(selected);
    }

    ctx.ui.print_step("Stack order (base -> top):");
    for (idx, issue) in selected.iter().enumerate() {
        ctx.ui
            .print_dim(&format!("  {}. {}", idx + 1, issue.display));
    }

    let default_order = (1..=selected.len())
        .map(|idx| idx.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let raw_order = ctx
        .ui
        .input("Stack order (base -> top)", Some(&default_order))?;
    let order = parse_order(&raw_order, selected.len())?;
    Ok(order.into_iter().map(|idx| selected[idx].clone()).collect())
}

fn parse_order(raw: &str, len: usize) -> Result<Vec<usize>> {
    let numbers = raw
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<usize>()
                .with_context(|| format!("Invalid stack order task: {part}"))
        })
        .collect::<Result<Vec<_>>>()?;

    if numbers.len() != len {
        bail!("Stack order must include each selected issue exactly once");
    }

    let mut seen = vec![false; len];
    let mut order = Vec::new();
    for number in numbers {
        if number == 0 || number > len {
            bail!("Stack order task out of range: {number}");
        }
        let idx = number - 1;
        if seen[idx] {
            bail!("Stack order includes duplicate task: {number}");
        }
        seen[idx] = true;
        order.push(idx);
    }

    Ok(order)
}

struct PreparedStackTasks {
    tasks: Vec<StackTask>,
    task_runs: Vec<task_run::TaskRunRecord>,
}

fn stack_tasks_from_prepared(
    ctx: &Ctx,
    stack_path: &Path,
    prepared_tasks: Vec<PreparedTask>,
    initial_parent: Option<String>,
) -> Result<PreparedStackTasks> {
    let group = task_run::group_from_path(stack_path)?;
    let mut parent = initial_parent;
    let mut tasks = Vec::new();
    let mut task_runs = Vec::new();
    for task in prepared_tasks {
        let task_parent = parent.clone();
        let run = match task_run::create(
            ctx,
            &task.key,
            &task.branch,
            task_run::SOURCE_STACK,
            Some(&group),
            STATUS_PREPARED,
        ) {
            Ok(run) => run,
            Err(err) => {
                rollback_task_runs(&task_runs);
                return Err(err);
            }
        };
        let run_id = run.id.clone();
        task_runs.push(run);
        parent = prepared_branch_name(&task.branch).map(str::to_string);
        tasks.push(StackTask::from_prepared(task, run_id, task_parent));
    }
    Ok(PreparedStackTasks { tasks, task_runs })
}

fn rollback_task_runs(task_runs: &[task_run::TaskRunRecord]) {
    for run in task_runs.iter().rev() {
        let _ = task_run::delete_record(run);
    }
}

fn default_stack_status() -> String {
    STATUS_PREPARED.into()
}

fn validate_profile(ctx: &Ctx, profile: Option<&str>) -> Result<()> {
    let Some(profile) = profile else {
        return Ok(());
    };

    validate_profile_name(profile)?;
    if Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?.is_none() {
        bail!("Profile '{profile}' not found");
    }

    Ok(())
}

fn run_stack_task(
    ctx: &Ctx,
    stack_path: &Path,
    stack_task: &StackTask,
    parent: &str,
    profile: Option<&str>,
) -> Result<issue::IssueRunResult> {
    let (task_doc, task_path, content) = task::read_task_file(ctx, &stack_task.task)?;
    let content = format!(
        "{}\n\n## Stack Completion\n\nWhen this task is complete and committed, run:\n\n```bash\nwt stack complete {} {} --run-next\n```",
        content.trim_end(),
        stack_path.display(),
        stack_task.label()
    );
    let branch_name = prepared_branch_name(&task_doc.branch);
    if branch_name.is_none() && task_doc.origin.is_none() {
        bail!("Stack task {} has no branch", stack_task.label());
    }
    let base = Some(parent.to_string());
    let identifier = task_doc.identifier_or_key(&stack_task.task);
    let title = task_doc.title_or_key(&stack_task.task);

    issue::run_with_issue_snapshot(
        ctx,
        &base,
        profile,
        false,
        issue::PreparedIssueContext {
            identifier: &identifier,
            title: &title,
            branch_name,
            mode: task_doc.mode(),
            prompt_intro: "Use this task before changing code.",
            snapshot: issue::IssueSnapshotContext {
                path_label: "Task path",
                path: &task_path,
                content: &content,
            },
        },
    )
}

#[derive(Clone)]
struct StackTaskState {
    idx: usize,
    stack_task: StackTask,
    run: task_run::TaskRun,
}

fn update_stack_task_run(
    ctx: &Ctx,
    stack_path: &Path,
    item: &StackTask,
    status: &str,
    error: Option<&str>,
) -> Result<()> {
    let path = task_run::resolve(ctx, &item.run).with_context(|| {
        format!(
            "Stack task {} references missing TaskRun {}",
            item.label(),
            item.run
        )
    })?;
    let run = task_run::read(&path)?;
    validate_stack_task_run(stack_path, item, &run)?;

    let branch = task::read_task_document(ctx, &item.task)
        .ok()
        .map(|task| task.branch);
    let updated = task_run::update(ctx, &item.run, status, branch.as_deref(), error)?;
    validate_stack_task_run(stack_path, item, &updated.run)?;
    Ok(())
}

fn stack_task_matches(ctx: &Ctx, item: &StackTask, target: &str) -> bool {
    if item.task == target {
        return true;
    }
    let Ok(task_doc) = task::read_task_document(ctx, &item.task) else {
        return false;
    };
    task_doc.title == target
        || prepared_branch_name(&task_doc.branch) == Some(target)
        || task_doc.branch.rsplit('/').next() == Some(target)
}

fn validate_completable_stack_task(ctx: &Ctx, item: &StackTask) -> Result<()> {
    let task_doc = task::read_task_document(ctx, &item.task)?;
    let branch = prepared_branch_name(&task_doc.branch)
        .ok_or_else(|| anyhow::anyhow!("Stack task {} has no branch", item.label()))?;
    let parent = item
        .parent
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Stack task {} has no parent", item.label()))?;
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.repo_root));

    if let Some(path) = git.checked_out_path(branch)? {
        let status = git.status_porcelain(&path)?;
        let relevant_status = relevant_worktree_status(ctx, &status);
        if !relevant_status.trim().is_empty() {
            bail!(
                "Stack task {} has uncommitted changes in {}. Commit or stash them before completing.\n{}",
                item.label(),
                path.display(),
                relevant_status.trim_end()
            );
        }
    }

    if !git.branch_has_commits_ahead(parent, branch)? {
        bail!(
            "Stack task {} has no commits ahead of parent {parent}. Commit the task work before completing.",
            item.label()
        );
    }

    Ok(())
}

fn relevant_worktree_status(ctx: &Ctx, status: &str) -> String {
    status
        .lines()
        .filter(|line| !is_configured_link_status_line(ctx, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_configured_link_status_line(ctx: &Ctx, line: &str) -> bool {
    let Some(path) = porcelain_status_path(line) else {
        return false;
    };

    ctx.config
        .worktree
        .link
        .iter()
        .map(|linked| linked.trim_end_matches('/'))
        .any(|linked| path == linked || path.starts_with(&format!("{linked}/")))
}

fn porcelain_status_path(line: &str) -> Option<&str> {
    let path = line.get(3..)?.trim();
    let path = path.rsplit(" -> ").next().unwrap_or(path);
    Some(path.trim_matches('"'))
}

fn next_runnable_task(items: &[StackTaskState]) -> Option<usize> {
    for item in items {
        match item.run.status.as_str() {
            STATUS_DONE | STATUS_SKIPPED => continue,
            status if is_runnable_status(status) => return Some(item.idx),
            _ => return None,
        }
    }
    None
}

fn parent_for_task(
    ctx: &Ctx,
    stack: &StackMetadata,
    states: &[StackTaskState],
    idx: usize,
) -> Result<String> {
    if idx == 0 {
        return resolve_initial_base(ctx, stack);
    }

    for previous in states.iter().rev().filter(|state| state.idx < idx) {
        match previous.run.status.as_str() {
            STATUS_DONE => {
                let task_doc = task::read_task_document(ctx, &previous.stack_task.task)?;
                return prepared_branch_name(&task_doc.branch)
                    .map(str::to_string)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Previous stack task {} has no branch",
                            previous.stack_task.label()
                        )
                    });
            }
            STATUS_SKIPPED => continue,
            _ => bail!(
                "Previous stack task {} is not done",
                previous.stack_task.label()
            ),
        }
    }

    resolve_initial_base(ctx, stack)
}

fn prepared_branch_name(branch: &str) -> Option<&str> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "-" {
        None
    } else {
        Some(branch)
    }
}

fn next_available_stack_path(ctx: &Ctx) -> Result<PathBuf> {
    let stacks_dir = ctx.repo_root.join(".local/stacks");
    fs::create_dir_all(&stacks_dir)?;

    let date = current_utc_date();
    let mut seq = 1;
    loop {
        let candidate = stacks_dir.join(format!("{date}-{seq:03}.toml"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        seq += 1;
    }
}

fn read_stack_metadata(path: &Path) -> Result<StackMetadata> {
    let content = fs::read_to_string(path)?;
    let mut metadata: StackMetadata = toml::from_str(&content)?;
    for item in &mut metadata.tasks {
        validate_stack_task(item)?;
    }
    Ok(metadata)
}

fn validate_stack_task(item: &StackTask) -> Result<()> {
    if item.task.trim().is_empty() {
        bail!("Stack task is missing task");
    }
    if item.run.trim().is_empty() {
        bail!("Stack task {} is missing TaskRun id", item.label());
    }
    Ok(())
}

fn read_stack_task_states(
    ctx: &Ctx,
    stack_path: &Path,
    metadata: &StackMetadata,
) -> Result<Vec<StackTaskState>> {
    let group = task_run::group_from_path(stack_path)?;
    metadata
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let path = task_run::resolve(ctx, &item.run).with_context(|| {
                format!(
                    "Stack task {} references missing TaskRun {}",
                    item.label(),
                    item.run
                )
            })?;
            let run = task_run::read(&path)?;
            validate_stack_task_run_with_group(&group, item, &run)?;
            Ok(StackTaskState {
                idx,
                stack_task: item.clone(),
                run,
            })
        })
        .collect()
}

fn validate_stack_task_run(
    stack_path: &Path,
    item: &StackTask,
    run: &task_run::TaskRun,
) -> Result<()> {
    let group = task_run::group_from_path(stack_path)?;
    validate_stack_task_run_with_group(&group, item, run)
}

fn validate_stack_task_run_with_group(
    group: &str,
    item: &StackTask,
    run: &task_run::TaskRun,
) -> Result<()> {
    let expected_task = task::safe_task_key(&item.task);
    if run.task != expected_task {
        bail!(
            "Stack task {} references TaskRun {} for task {}",
            item.label(),
            item.run,
            run.task
        );
    }
    if run.source != task_run::SOURCE_STACK {
        bail!(
            "Stack task {} references TaskRun {} with source {}",
            item.label(),
            item.run,
            run.source
        );
    }
    if run.group.as_deref() != Some(group) {
        bail!(
            "Stack task {} references TaskRun {} outside stack group {}",
            item.label(),
            item.run,
            group
        );
    }
    Ok(())
}

fn write_stack_metadata(path: &Path, stack: &StackMetadata) -> Result<()> {
    let mut content = String::new();
    if let Some(profile) = stack.profile.as_deref() {
        content.push_str(&format!("profile = {}\n", toml_quote(profile)));
    }
    content.push_str(&format!("base_mode = {}\n", toml_quote(&stack.base_mode)));
    if let Some(base) = &stack.base {
        content.push_str(&format!("base = {}\n", toml_quote(base)));
    }
    content.push_str(&format!("status = {}\n", toml_quote(&stack.status)));
    content.push_str(&format!("created_at = {}\n", toml_quote(&stack.created_at)));
    content.push_str(&format!("updated_at = {}\n", toml_quote(&stack.updated_at)));

    for item in &stack.tasks {
        content.push_str("\n[[tasks]]\n");
        content.push_str(&format!("task = {}\n", toml_quote(&item.task)));
        content.push_str(&format!("run = {}\n", toml_quote(&item.run)));
        if let Some(parent) = item.parent.as_deref() {
            content.push_str(&format!("parent = {}\n", toml_quote(parent)));
        }
    }

    fs::write(path, content)
        .with_context(|| format!("Failed to write stack metadata: {}", path.display()))?;
    Ok(())
}

fn resolve_stack_path(ctx: &Ctx, target: &str) -> Result<PathBuf> {
    if target == "latest" {
        return latest_stack_path(ctx);
    }

    let path = PathBuf::from(target);
    if path.is_absolute() && path.exists() {
        return Ok(path);
    }

    let invocation_path = ctx.invocation_root.join(target);
    if invocation_path.exists() {
        return Ok(invocation_path);
    }

    let repo_path = ctx.repo_root.join(target);
    if repo_path.exists() {
        return Ok(repo_path);
    }

    if !target.ends_with(".toml") {
        let shorthand = ctx
            .repo_root
            .join(".local/stacks")
            .join(format!("{target}.toml"));
        if shorthand.exists() {
            return Ok(shorthand);
        }
    }

    bail!("Stack not found: {target}");
}

fn latest_stack_path(ctx: &Ctx) -> Result<PathBuf> {
    let stacks_dir = ctx.repo_root.join(".local/stacks");
    let mut paths = Vec::new();
    if stacks_dir.exists() {
        for entry in fs::read_dir(&stacks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
        .pop()
        .ok_or_else(|| anyhow::anyhow!("No stack files found in .local/stacks"))
}

fn resolve_stack_base(ctx: &Ctx, base: &Option<String>) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match BaseMode::from_raw(base) {
        BaseMode::Explicit(branch) => branch,
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            branches[idx].clone()
        }
        BaseMode::Current => git.current_branch()?,
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))?
        }
    };

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
}

fn resolve_initial_base(ctx: &Ctx, stack: &StackMetadata) -> Result<String> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = match stack.base_mode.as_str() {
        "default" => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))?
        }
        "interactive" => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            branches[idx].clone()
        }
        "current" => git.current_branch()?,
        "explicit" => stack
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Stack base_mode is explicit but base is missing"))?,
        other => bail!("Unknown stack base_mode: {other}"),
    };

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
}

fn describe_stack_base(stack: &StackMetadata) -> Result<String> {
    match stack.base_mode.as_str() {
        "default" => Ok("prompt at run time".into()),
        "interactive" => Ok("branch selector at run time".into()),
        "current" => Ok("current branch at run time".into()),
        "explicit" => stack
            .base
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Stack base_mode is explicit but base is missing")),
        other => bail!("Unknown stack base_mode: {other}"),
    }
}

fn stack_status_counts(items: &[StackTaskState]) -> String {
    let statuses = [
        STATUS_PREPARED,
        STATUS_RUNNING,
        STATUS_DONE,
        STATUS_FAILED,
        STATUS_SKIPPED,
    ];
    let counts = statuses
        .iter()
        .filter_map(|status| {
            let count = items
                .iter()
                .filter(|item| item.run.status == *status)
                .count();
            (count > 0).then(|| format!("{status}={count}"))
        })
        .collect::<Vec<_>>();

    if counts.is_empty() {
        "none".into()
    } else {
        counts.join(", ")
    }
}

fn is_runnable_status(status: &str) -> bool {
    matches!(status, STATUS_PREPARED | STATUS_FAILED)
}

fn summarize_current_stack_status(
    ctx: &Ctx,
    stack_path: &Path,
    metadata: &StackMetadata,
) -> Result<String> {
    let states = read_stack_task_states(ctx, stack_path, metadata)?;
    Ok(summarize_stack_status(&states))
}

fn summarize_stack_status(items: &[StackTaskState]) -> String {
    if items.is_empty() {
        return STATUS_DONE.into();
    }
    if items.iter().any(|item| item.run.status == STATUS_FAILED) {
        return STATUS_FAILED.into();
    }
    if items.iter().any(|item| item.run.status == STATUS_RUNNING) {
        return STATUS_RUNNING.into();
    }
    if items
        .iter()
        .all(|item| matches!(item.run.status.as_str(), STATUS_DONE | STATUS_SKIPPED))
    {
        return STATUS_DONE.into();
    }
    if items.iter().all(|item| item.run.status == STATUS_PREPARED) {
        return STATUS_PREPARED.into();
    }
    STATUS_PARTIAL.into()
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn current_utc_date() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2).div_euclid(153);
    let d = doy - (153 * mp + 2).div_euclid(5) + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, EditorPlacement, WorktreeNamingConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{CmdOutput, CommandRunner, Ctx};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    fn stack_task(key: &str, parent: Option<&str>, run: &str) -> StackTask {
        StackTask {
            task: key.into(),
            run: run.into(),
            parent: parent.map(str::to_string),
        }
    }

    fn stack_task_with_status(
        ctx: &Ctx,
        stack_path: &Path,
        key: &str,
        branch: &str,
        parent: Option<&str>,
        status: &str,
        error: &str,
    ) -> StackTask {
        let group = task_run::group_from_path(stack_path).unwrap();
        let record = task_run::create(
            ctx,
            key,
            branch,
            task_run::SOURCE_STACK,
            Some(&group),
            status,
        )
        .unwrap();
        if !error.is_empty() {
            task_run::update(ctx, &record.id, status, Some(branch), Some(error)).unwrap();
        }
        stack_task(key, parent, &record.id)
    }

    fn read_run(root: &Path, run_id: &str) -> task_run::TaskRun {
        task_run::read(&root.join(".local/task-runs").join(format!("{run_id}.toml"))).unwrap()
    }

    fn write_task_file(root: &Path, key: &str, title: &str, branch: &str, body: &str) {
        let tasks_dir = root.join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let mut content = format!("title = \"{title}\"\n");
        if !branch.is_empty() {
            content.push_str(&format!("branch = \"{branch}\"\n"));
        }
        if !body.is_empty() {
            content.push_str(&format!("body = \"\"\"\n{body}\n\"\"\"\n"));
        }
        std::fs::write(tasks_dir.join(format!("{key}.toml")), content).unwrap();
    }

    fn read_task_content(root: &Path, key: &str) -> String {
        std::fs::read_to_string(root.join(format!(".local/tasks/{key}.toml"))).unwrap()
    }

    #[test]
    fn parse_order_accepts_comma_or_space_separated_numbers() {
        assert_eq!(parse_order("2,1,3", 3).unwrap(), vec![1, 0, 2]);
        assert_eq!(parse_order("3 1 2", 3).unwrap(), vec![2, 0, 1]);
    }

    #[test]
    fn parse_order_rejects_missing_duplicate_or_out_of_range_tasks() {
        assert!(parse_order("1,2", 3).is_err());
        assert!(parse_order("1,1,2", 3).is_err());
        assert!(parse_order("1,2,4", 3).is_err());
    }

    #[test]
    fn relevant_worktree_status_ignores_configured_links() {
        let config = Config {
            worktree: crate::config::WorktreeConfig {
                link: vec![".local".into()],
                ..crate::config::WorktreeConfig::default()
            },
            ..Config::default()
        };
        let ctx = Ctx::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo"),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert_eq!(
            relevant_worktree_status(&ctx, "?? .local\n M src/lib.rs"),
            " M src/lib.rs"
        );
        assert_eq!(relevant_worktree_status(&ctx, "?? .local"), "");
    }

    #[test]
    fn task_creates_manual_stack_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let tasks = vec!["Add schema".into(), "Wire API".into()];

        task(&ctx, &tasks, None, &Some("main".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base_mode, "explicit");
        assert_eq!(stack.base.as_deref(), Some("main"));
        assert_eq!(stack.tasks.len(), 2);
        assert_eq!(stack.tasks[0].task, "add-schema");
        assert_eq!(stack.tasks[0].parent.as_deref(), Some("main"));
        assert!(!stack.tasks[0].run.is_empty());
        assert_eq!(stack.tasks[1].task, "wire-api");
        assert_eq!(stack.tasks[1].parent.as_deref(), Some("add-schema"));
        assert!(!stack.tasks[1].run.is_empty());

        let add_schema_run = read_run(dir.path(), &stack.tasks[0].run);
        assert_eq!(add_schema_run.task, "add-schema");
        assert_eq!(add_schema_run.branch, "add-schema");
        assert_eq!(add_schema_run.status, STATUS_PREPARED);
        assert_eq!(add_schema_run.source, task_run::SOURCE_STACK);
        assert_eq!(
            add_schema_run.group.as_deref(),
            Some(task_run::group_from_path(&stack_path).unwrap().as_str())
        );

        let add_schema = read_task_content(dir.path(), "add-schema");
        assert!(add_schema.contains("title = \"Add schema\""));
        assert!(add_schema.contains("branch = \"add-schema\""));

        let content = std::fs::read_to_string(stack_path).unwrap();
        assert!(content.contains("[[tasks]]"));
        assert!(content.contains("task = \"add-schema\""));
        assert!(content.contains("run = \""));
        let task_section = content.split("[[tasks]]").nth(1).unwrap();
        assert!(!task_section.contains("status ="));
        assert!(!task_section.contains("error ="));
        assert!(!content.contains("[[items]]"));
        assert!(!content.contains("[[issues]]"));
    }

    #[test]
    fn prepare_rolls_back_task_runs_when_stack_metadata_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let existing = task_run::create(
            &ctx,
            "unrelated",
            "unrelated",
            task_run::SOURCE_NEW,
            None,
            STATUS_DONE,
        )
        .unwrap();
        let stack_path = dir.path().join(".local/stacks/unwritable.toml");
        std::fs::create_dir_all(&stack_path).unwrap();

        let result = write_prepared_stack_at_path(
            &ctx,
            None,
            "main",
            &stack_path,
            vec![
                PreparedTask {
                    key: "add-schema".into(),
                    branch: "add-schema".into(),
                },
                PreparedTask {
                    key: "wire-api".into(),
                    branch: "wire-api".into(),
                },
            ],
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("Failed to write stack metadata"));
        assert!(error.contains("unwritable.toml"));
        let task_runs_dir = dir.path().join(".local/task-runs");
        assert!(task_runs_dir.join(format!("{}.toml", existing.id)).exists());
        assert!(
            !task_runs_dir
                .join("stack-unwritable-add-schema.toml")
                .exists()
        );
        assert!(
            !task_runs_dir
                .join("stack-unwritable-wire-api.toml")
                .exists()
        );
        let records = task_run::list(&ctx).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, existing.id);
    }

    #[test]
    fn task_resolves_current_base_for_dot_base() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("feature/current", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let tasks = vec!["Add schema".into()];

        task(&ctx, &tasks, None, &Some(".".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let content = std::fs::read_to_string(stack_path).unwrap();
        assert!(content.contains("base_mode = \"explicit\""));
        assert!(content.contains("base = \"feature/current\""));
        assert!(content.contains("parent = \"feature/current\""));
    }

    #[test]
    fn task_stores_default_base_prompt_result_at_prepare_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_input("develop");
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );
        let tasks = vec!["Add schema".into()];

        task(&ctx, &tasks, None, &None).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base_mode, "explicit");
        assert_eq!(stack.base.as_deref(), Some("develop"));
        assert_eq!(stack.tasks[0].parent.as_deref(), Some("develop"));
    }

    #[test]
    fn resolve_initial_base_current_uses_current_branch_without_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("feature/current", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let stack = StackMetadata {
            profile: None,
            base_mode: "current".into(),
            base: None,
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            tasks: Vec::new(),
        };

        assert_eq!(
            resolve_initial_base(&ctx, &stack).unwrap(),
            "feature/current"
        );
    }

    #[test]
    fn parent_for_task_skips_skipped_tasks_when_finding_parent() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "schema", "Schema", "schema", "");
        write_task_file(dir.path(), "api", "API", "api", "");
        write_task_file(dir.path(), "ui", "UI", "ui", "");
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            tasks: vec![
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "schema",
                    "schema",
                    Some("main"),
                    STATUS_DONE,
                    "",
                ),
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "api",
                    "api",
                    Some("schema"),
                    STATUS_SKIPPED,
                    "User cancelled",
                ),
                stack_task_with_status(&ctx, &stack_path, "ui", "ui", None, STATUS_PREPARED, ""),
            ],
        };
        let states = read_stack_task_states(&ctx, &stack_path, &stack).unwrap();

        assert_eq!(parent_for_task(&ctx, &stack, &states, 2).unwrap(), "schema");
    }

    #[test]
    fn parent_for_task_uses_initial_base_when_previous_tasks_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "schema", "Schema", "schema", "");
        write_task_file(dir.path(), "api", "API", "api", "");
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PARTIAL.into(),
            created_at: "2026-05-13T00:00:00Z".into(),
            updated_at: "2026-05-13T00:00:00Z".into(),
            tasks: vec![
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "schema",
                    "schema",
                    Some("main"),
                    STATUS_SKIPPED,
                    "User cancelled",
                ),
                stack_task_with_status(&ctx, &stack_path, "api", "api", None, STATUS_PREPARED, ""),
            ],
        };
        let states = read_stack_task_states(&ctx, &stack_path, &stack).unwrap();

        assert_eq!(parent_for_task(&ctx, &stack, &states, 1).unwrap(), "main");
    }

    #[test]
    fn task_rejects_duplicate_task_keys() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let tasks = vec!["Add schema".into(), "add schema".into()];

        let err = task(&ctx, &tasks, None, &None).unwrap_err();
        assert!(err.to_string().contains("Duplicate task: add-schema"));
    }

    #[test]
    fn issue_with_no_args_selects_and_reorders_issues() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"PROJ-1","title":"Schema","state":{"name":"Todo"}},{"identifier":"PROJ-2","title":"API","state":{"name":"Todo"}}]"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-2","title":"API","branchName":"alice/proj-2-api","description":"API body"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Schema","branchName":"alice/proj-1-schema","description":"Schema body"}"#,
            true,
        );
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 1]);
        ui.add_input("2,1");
        let config = crate::config::Config {
            issues: Some(crate::config::IssuesConfig {
                provider: crate::config::IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(ui),
        );

        issue(&ctx, &[], None, &Some("main".into())).unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.base.as_deref(), Some("main"));
        assert_eq!(stack.tasks[0].task, "PROJ-2");
        assert_eq!(stack.tasks[0].parent.as_deref(), Some("main"));
        assert_eq!(
            read_run(dir.path(), &stack.tasks[0].run).source,
            task_run::SOURCE_STACK
        );
        assert_eq!(stack.tasks[1].task, "PROJ-1");
        assert_eq!(stack.tasks[1].parent.as_deref(), Some("alice/proj-2-api"));
        assert_eq!(
            read_run(dir.path(), &stack.tasks[1].run).status,
            STATUS_PREPARED
        );
        let content = std::fs::read_to_string(stack_path).unwrap();
        assert!(content.contains("[[tasks]]"));
        assert!(content.contains("task = \"PROJ-2\""));
        assert!(content.contains("run = \""));
        assert!(!content.contains("[[issues]]"));
    }

    #[test]
    fn issue_applies_worktree_naming_to_prepared_parent_chain() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Schema","branchName":"alice/proj-1-schema","description":"Schema body"}"#,
            true,
        );
        runner.add_response(r#"{"english_slug":"schema-layer"}"#, true);
        runner.add_response(
            r#"{"identifier":"PROJ-2","title":"API","branchName":"alice/proj-2-api","description":"API body"}"#,
            true,
        );
        runner.add_response(r#"{"english_slug":"api-layer"}"#, true);
        let config = crate::config::Config {
            issues: Some(crate::config::IssuesConfig {
                provider: crate::config::IssueProviderType::Linear,
                gh_user: None,
            }),
            worktree: crate::config::WorktreeConfig {
                naming: Some(WorktreeNamingConfig {
                    command: "namer".into(),
                    prompt: "{{issue_title}}".into(),
                    branch: Some("{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}".into()),
                    workspace: None,
                }),
                ..Default::default()
            },
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        issue(
            &ctx,
            &["PROJ-1".into(), "PROJ-2".into()],
            None,
            &Some("main".into()),
        )
        .unwrap();

        let stack_path = latest_stack_path(&ctx).unwrap();
        let stack = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(stack.tasks[0].parent.as_deref(), Some("main"));
        assert_eq!(stack.tasks[1].task, "PROJ-2");
        assert_eq!(
            stack.tasks[1].parent.as_deref(),
            Some("alice/proj-1-schema-layer")
        );
        assert!(
            read_task_content(dir.path(), "PROJ-1")
                .contains("branch = \"alice/proj-1-schema-layer\"")
        );
        assert!(
            read_task_content(dir.path(), "PROJ-2").contains("branch = \"alice/proj-2-api-layer\"")
        );
    }

    #[test]
    fn show_prints_stack_metadata_and_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui.clone()),
        );
        write_task_file(dir.path(), "PROJ-1", "Schema", "alice/proj-1-schema", "");
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: Some("codex".into()),
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            tasks: vec![stack_task_with_status(
                &ctx,
                &stack_path,
                "PROJ-1",
                "alice/proj-1-schema",
                Some("main"),
                STATUS_FAILED,
                "missing task",
            )],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        show(&ctx, Some(stack_path.to_str().unwrap())).unwrap();

        let steps = ui.steps.lock().unwrap();
        assert!(steps[0].contains("Stack:"));
        let details = ui.dims.lock().unwrap().join("\n");
        assert!(details.contains("Status: failed"));
        assert!(details.contains("Base: main"));
        assert!(details.contains("Profile: codex"));
        assert!(details.contains("Tasks: 1 (failed=1)"));
        assert!(details.contains("PROJ-1 [failed] Schema"));
        assert!(details.contains("Task: .local/tasks/PROJ-1.toml"));
        assert!(details.contains("Branch: alice/proj-1-schema"));
        assert!(details.contains("Parent: main"));
        assert!(details.contains("Error: missing task"));
    }

    #[test]
    fn read_stack_metadata_rejects_issues_tables() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("stack.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[issues]]
id = "PROJ-1"
source = "PROJ-1"
title = "Schema"
branch = "alice/proj-1-schema"
snapshot = ".local/issues/PROJ-1.md"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_stack_metadata(&stack_path).is_err());
    }

    #[test]
    fn read_stack_metadata_rejects_legacy_items_tables() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("stack.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[items]]
kind = "pr"
id = "42"
title = "Existing PR"
branch = "alice/pr-42"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_stack_metadata(&stack_path).is_err());
    }

    #[test]
    fn read_stack_metadata_rejects_legacy_items_without_kind() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("stack.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[items]]
id = "add-schema"
title = "Add schema"
branch = "add-schema"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_stack_metadata(&stack_path).is_err());
    }

    #[test]
    fn read_stack_metadata_rejects_task_row_status_and_error() {
        let dir = tempfile::tempdir().unwrap();
        let stack_path = dir.path().join("stack.toml");
        std::fs::write(
            &stack_path,
            r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[tasks]]
task = "add-schema"
run = "stack-manual-add-schema"
parent = "main"
status = "prepared"
error = ""
"#,
        )
        .unwrap();

        assert!(read_stack_metadata(&stack_path).is_err());
    }

    #[test]
    fn edit_opens_latest_stack_with_configured_editor() {
        let dir = tempfile::tempdir().unwrap();
        let stacks_dir = dir.path().join(".local/stacks");
        std::fs::create_dir_all(&stacks_dir).unwrap();
        std::fs::write(
            stacks_dir.join("2026-05-12-001.toml"),
            "base_mode = \"explicit\"\n",
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("", true);
        let mut config = Config::default();
        config.editor.command = Some("code {{path}}".into());
        config.editor.placement = Some(EditorPlacement::Process);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        edit(&ctx, None).unwrap();
    }

    #[test]
    fn run_starts_one_task_and_complete_allows_next_parent() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "PROJ-1", "Schema", "alice/proj-1-schema", "");
        write_task_file(dir.path(), "PROJ-2", "API", "alice/proj-2-api", "");

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/alice/proj-1-schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            tasks: vec![
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "PROJ-1",
                    "alice/proj-1-schema",
                    None,
                    STATUS_PREPARED,
                    "",
                ),
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "PROJ-2",
                    "alice/proj-2-api",
                    None,
                    STATUS_PREPARED,
                    "",
                ),
            ],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();

        run(&ctx, stack_path.to_str().unwrap()).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.tasks[0].parent.as_deref(), Some("main"));
        let first_run_id = updated.tasks[0].run.clone();
        let second_run_id = updated.tasks[1].run.clone();
        let first_run = read_run(dir.path(), &first_run_id);
        let second_run = read_run(dir.path(), &second_run_id);
        assert_eq!(first_run.status, STATUS_RUNNING);
        assert_eq!(first_run.source, task_run::SOURCE_STACK);
        assert_eq!(first_run.group.as_deref(), Some("stack"));
        assert_eq!(second_run.status, STATUS_PREPARED);

        complete(&ctx, stack_path.to_str().unwrap(), Some("PROJ-1"), false).unwrap();
        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_PARTIAL);
        let first_run = read_run(dir.path(), &first_run_id);
        assert_eq!(first_run.status, STATUS_DONE);

        run(&ctx, stack_path.to_str().unwrap()).unwrap();
        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(
            updated.tasks[1].parent.as_deref(),
            Some("alice/proj-1-schema")
        );
        let second_run = read_run(dir.path(), &second_run_id);
        assert_eq!(second_run.status, STATUS_RUNNING);
        assert!(!updated.tasks[1].run.is_empty());

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(_, args, _)| {
            args.len() == 6
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b"
                && args[3] == "alice/proj-2-api"
                && args[5] == "alice/proj-1-schema"
        }));
    }

    #[test]
    fn complete_with_run_next_starts_next_task() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "PROJ-1", "Schema", "alice/proj-1-schema", "");
        write_task_file(dir.path(), "PROJ-2", "API", "alice/proj-2-api", "");

        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/alice/proj-1-schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("1", true);
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/alice/proj-1-schema\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_PREPARED.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            tasks: vec![
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "PROJ-1",
                    "alice/proj-1-schema",
                    None,
                    STATUS_PREPARED,
                    "",
                ),
                stack_task_with_status(
                    &ctx,
                    &stack_path,
                    "PROJ-2",
                    "alice/proj-2-api",
                    None,
                    STATUS_PREPARED,
                    "",
                ),
            ],
        };
        write_stack_metadata(&stack_path, &stack).unwrap();
        let first_run_id = stack.tasks[0].run.clone();
        let second_run_id = stack.tasks[1].run.clone();

        run(&ctx, stack_path.to_str().unwrap()).unwrap();
        complete(&ctx, stack_path.to_str().unwrap(), Some("PROJ-1"), true).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(read_run(dir.path(), &first_run_id).status, STATUS_DONE);
        assert_eq!(read_run(dir.path(), &second_run_id).status, STATUS_RUNNING);
        assert_eq!(
            updated.tasks[1].parent.as_deref(),
            Some("alice/proj-1-schema")
        );
    }

    #[test]
    fn complete_rejects_dirty_stack_task_worktree() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "feature", "Feature", "feature", "");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response(" M src/lib.rs", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_RUNNING.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            tasks: vec![stack_task_with_status(
                &ctx,
                &stack_path,
                "feature",
                "feature",
                Some("main"),
                STATUS_RUNNING,
                "",
            )],
        };
        let run_id = stack.tasks[0].run.clone();
        write_stack_metadata(&stack_path, &stack).unwrap();

        let err = complete(&ctx, stack_path.to_str().unwrap(), Some("feature"), false).unwrap_err();
        assert!(err.to_string().contains("uncommitted changes"));

        assert_eq!(read_run(dir.path(), &run_id).status, STATUS_RUNNING);
    }

    #[test]
    fn complete_rejects_stack_task_without_commits() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(dir.path(), "feature", "Feature", "feature", "");
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\nworktree {}/wt-proj-1\nHEAD def\nbranch refs/heads/feature\n\n",
                dir.path().display(),
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("0", true);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("stack.toml");
        let stack = StackMetadata {
            profile: None,
            base_mode: "explicit".into(),
            base: Some("main".into()),
            status: STATUS_RUNNING.into(),
            created_at: "2026-05-12T00:00:00Z".into(),
            updated_at: "2026-05-12T00:00:00Z".into(),
            tasks: vec![stack_task_with_status(
                &ctx,
                &stack_path,
                "feature",
                "feature",
                Some("main"),
                STATUS_RUNNING,
                "",
            )],
        };
        let run_id = stack.tasks[0].run.clone();
        write_stack_metadata(&stack_path, &stack).unwrap();

        let err = complete(&ctx, stack_path.to_str().unwrap(), Some("feature"), false).unwrap_err();
        assert!(err.to_string().contains("no commits ahead"));

        assert_eq!(read_run(dir.path(), &run_id).status, STATUS_RUNNING);
    }

    #[test]
    fn run_supports_manual_task_without_issue_provider() {
        let dir = tempfile::tempdir().unwrap();
        write_task_file(
            dir.path(),
            "add-schema",
            "Add schema",
            "add-schema",
            "Create the schema without an issue provider.",
        );
        let mut runner = MockRunner::new();
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                dir.path().display()
            ),
            true,
        );
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let stack_path = dir.path().join("manual.toml");
        let stack_task = stack_task_with_status(
            &ctx,
            &stack_path,
            "add-schema",
            "add-schema",
            None,
            STATUS_PREPARED,
            "",
        );
        std::fs::write(
            &stack_path,
            format!(
                r#"base_mode = "explicit"
base = "main"
status = "prepared"

[[tasks]]
task = "add-schema"
run = "{}"
"#,
                stack_task.run
            ),
        )
        .unwrap();

        run(&ctx, stack_path.to_str().unwrap()).unwrap();

        let updated = read_stack_metadata(&stack_path).unwrap();
        assert_eq!(updated.status, STATUS_RUNNING);
        assert_eq!(updated.tasks[0].task, "add-schema");
        assert_eq!(updated.tasks[0].parent.as_deref(), Some("main"));
        assert_eq!(
            read_run(dir.path(), &updated.tasks[0].run).status,
            STATUS_RUNNING
        );

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(_, args, _)| {
            args.len() == 6
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b"
                && args[3] == "add-schema"
                && args[5] == "main"
        }));
    }
}
