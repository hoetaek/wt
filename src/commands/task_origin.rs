use crate::context::Ctx;
use crate::context::PromptItem;
use crate::origin_snapshot::{
    FieldHashes, FieldSnapshot, OriginRef, OriginSnapshot, ProviderContext, read_task_snapshot,
    write_snapshot,
};
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
use crate::services::issues::{IssueDetail, IssueReader};
use crate::task;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;

pub(crate) fn import(ctx: &Ctx, issues: &[String]) -> Result<()> {
    crate::commands::task::import(ctx, issues)
}

pub(crate) fn publish(ctx: &Ctx, tasks: &[String]) -> Result<()> {
    crate::commands::task_publish::run(ctx, tasks)
}

pub(crate) fn attach(_ctx: &Ctx, _task: &str, _issue: &str) -> Result<()> {
    reserved("attach")
}

pub(crate) fn fetch(ctx: &Ctx, tasks: &[String]) -> Result<()> {
    let keys = resolve_origin_task_keys(
        ctx,
        tasks,
        "fetch",
        "wt task origin fetch requires TASK when it cannot open an interactive selector. Pass a task key, for example `wt task origin fetch <task>`.",
    )?;
    if keys.is_empty() {
        ctx.ui
            .print_warning("No origin-backed tasks selected to fetch");
        return Ok(());
    }
    validate_fetchable_origin_tasks(ctx, &keys)?;
    let reader = build_issue_reader(ctx)?;
    fetch_resolved_with_reader(ctx, keys, reader.as_ref())
}

pub(crate) fn diff(ctx: &Ctx, tasks: &[String]) -> Result<()> {
    let keys = resolve_origin_task_keys(
        ctx,
        tasks,
        "diff",
        "wt task origin diff requires TASK when it cannot open an interactive selector. Pass a task key, for example `wt task origin diff <task>`.",
    )?;
    if keys.is_empty() {
        ctx.ui
            .print_warning("No origin-backed tasks selected to diff");
        return Ok(());
    }

    let reports = keys
        .iter()
        .map(|key| diff_task(ctx, key))
        .collect::<Result<Vec<_>>>()?;
    if ctx.is_json() {
        write_json(&reports)?;
    } else {
        for report in &reports {
            print_diff_report(ctx, report);
        }
    }

    Ok(())
}

pub(crate) fn pull(_ctx: &Ctx, _tasks: &[String]) -> Result<()> {
    reserved("pull")
}

pub(crate) fn push(_ctx: &Ctx, _tasks: &[String]) -> Result<()> {
    reserved("push")
}

fn reserved(command: &str) -> Result<()> {
    bail!(
        "`wt task origin {command}` is reserved for provider issue origin design until its implementation slice lands"
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FieldDiffStatus {
    Unchanged,
    LocalChanged,
    RemoteChanged,
    Conflict,
    NoBaseline,
}

impl FieldDiffStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::LocalChanged => "local-changed",
            Self::RemoteChanged => "remote-changed",
            Self::Conflict => "conflict",
            Self::NoBaseline => "no-baseline",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct FieldDiff {
    pub(crate) status: FieldDiffStatus,
    pub(crate) local_changed: bool,
    pub(crate) remote_changed: bool,
    pub(crate) baseline: String,
    pub(crate) local: String,
    pub(crate) remote: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct TaskOriginDiffReport {
    pub(crate) task_key: String,
    pub(crate) origin: OriginRef,
    pub(crate) fields: BTreeMap<String, FieldDiff>,
    pub(crate) conflicts: Vec<String>,
}

#[cfg(test)]
fn fetch_with_reader(ctx: &Ctx, tasks: &[String], reader: &dyn IssueReader) -> Result<()> {
    let keys = resolve_origin_task_keys(
        ctx,
        tasks,
        "fetch",
        "wt task origin fetch requires TASK when it cannot open an interactive selector. Pass a task key, for example `wt task origin fetch <task>`.",
    )?;
    fetch_resolved_with_reader(ctx, keys, reader)
}

fn fetch_resolved_with_reader(
    ctx: &Ctx,
    keys: Vec<String>,
    reader: &dyn IssueReader,
) -> Result<()> {
    if keys.is_empty() {
        ctx.ui
            .print_warning("No origin-backed tasks selected to fetch");
        return Ok(());
    }

    for key in keys {
        let result = fetch_one(ctx, &key, reader)?;
        ctx.ui.print_plain(&format!(
            "Fetched origin for {}: {}:{}",
            result.task_key, result.provider, result.issue_id
        ));
    }

    Ok(())
}

fn diff_task(ctx: &Ctx, key: &str) -> Result<TaskOriginDiffReport> {
    let task_key = task::safe_task_key(key);
    let document = task::read_task_document(ctx, &task_key)?;
    let origin = document.origin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "wt task origin diff requires a task with [origin]; use `wt task origin publish {task_key}` or `wt task origin attach {task_key} <issue>`"
        )
    })?;
    let snapshot = read_task_snapshot(&ctx.storage_root, &task_key)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No fetched origin snapshot for task {task_key}. Run `wt task origin fetch {task_key}` before diffing."
        )
    })?;
    if !snapshot.matches_origin(&origin.provider, &origin.id) {
        bail!(
            "Fetched origin snapshot for task {task_key} does not match current [origin] {}:{}. Run `wt task origin fetch {task_key}` to refresh origin evidence.",
            origin.provider,
            origin.id
        );
    }

    let local_fields = FieldSnapshot::new(document.title.clone(), document.body.clone());
    let local_hashes = FieldHashes::from_fields(&local_fields);
    let mut fields: BTreeMap<String, FieldDiff> = BTreeMap::new();
    fields.insert(
        "title".into(),
        field_diff(
            &snapshot.baseline.fields.title,
            &snapshot.baseline.local_hashes.title,
            &local_fields.title,
            &local_hashes.title,
            &snapshot.remote.fields.title,
        ),
    );
    fields.insert(
        "body".into(),
        field_diff(
            &snapshot.baseline.fields.body,
            &snapshot.baseline.local_hashes.body,
            &local_fields.body,
            &local_hashes.body,
            &snapshot.remote.fields.body,
        ),
    );
    let conflicts = fields
        .iter()
        .filter(|(_, field)| field.status == FieldDiffStatus::Conflict)
        .map(|(name, _)| name.clone())
        .collect();

    Ok(TaskOriginDiffReport {
        task_key,
        origin: snapshot.origin,
        fields,
        conflicts,
    })
}

fn field_diff(
    baseline: &str,
    baseline_local_hash: &str,
    local: &str,
    local_hash: &str,
    remote: &str,
) -> FieldDiff {
    let has_baseline = !baseline_local_hash.trim().is_empty();
    let local_changed = has_baseline && local_hash != baseline_local_hash;
    let remote_changed = has_baseline && remote != baseline;
    let status = if !has_baseline {
        FieldDiffStatus::NoBaseline
    } else if local_changed && remote_changed {
        FieldDiffStatus::Conflict
    } else if local_changed {
        FieldDiffStatus::LocalChanged
    } else if remote_changed {
        FieldDiffStatus::RemoteChanged
    } else {
        FieldDiffStatus::Unchanged
    };

    FieldDiff {
        status,
        local_changed,
        remote_changed,
        baseline: baseline.to_string(),
        local: local.to_string(),
        remote: remote.to_string(),
    }
}

fn print_diff_report(ctx: &Ctx, report: &TaskOriginDiffReport) {
    ctx.ui.print_plain(&format!(
        "Origin diff for {}: {}:{}",
        report.task_key, report.origin.provider, report.origin.id
    ));
    for (name, field) in &report.fields {
        ctx.ui
            .print_plain(&format!("  {name}: {}", field.status.as_str()));
    }
    if !report.conflicts.is_empty() {
        ctx.ui
            .print_warning(&format!("Conflicts: {}", report.conflicts.join(", ")));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FetchResult {
    task_key: String,
    provider: String,
    issue_id: String,
}

fn fetch_one(ctx: &Ctx, key: &str, reader: &dyn IssueReader) -> Result<FetchResult> {
    let task_key = task::safe_task_key(key);
    let document = task::read_task_document(ctx, &task_key)?;
    let origin = require_fetch_origin(&document)?;
    let detail = reader
        .get_issue_detail(origin.id.as_str())
        .with_context(|| format!("Failed to fetch origin for task {task_key}: {}", origin.id))?;
    let snapshot = fetched_task_snapshot(ctx, &task_key, &document, origin, detail)?;
    write_snapshot(&ctx.storage_root, &snapshot)?;

    Ok(FetchResult {
        task_key,
        provider: origin.provider.clone(),
        issue_id: origin.id.clone(),
    })
}

fn validate_fetchable_origin_tasks(ctx: &Ctx, keys: &[String]) -> Result<()> {
    for key in keys {
        let task_key = task::safe_task_key(key);
        let document = task::read_task_document(ctx, &task_key)?;
        require_fetch_origin(&document)?;
    }
    Ok(())
}

fn require_fetch_origin(document: &task::TaskDocument) -> Result<&task::TaskOrigin> {
    document.origin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "wt task origin fetch requires a task with [origin]; use wt task origin publish or wt task origin attach"
        )
    })
}

fn fetched_task_snapshot(
    ctx: &Ctx,
    task_key: &str,
    document: &task::TaskDocument,
    origin: &task::TaskOrigin,
    detail: IssueDetail,
) -> Result<OriginSnapshot> {
    let local_fields = FieldSnapshot::new(document.title.clone(), document.body.clone());
    let remote_fields = FieldSnapshot::new(
        detail.title.clone(),
        detail.body.clone().unwrap_or_default(),
    );
    let mut origin_ref = OriginRef::new(origin.provider.clone(), origin.id.clone());
    origin_ref.url = detail.url.clone();
    origin_ref.remote_updated_at = detail.updated_at.clone();

    let existing = read_task_snapshot(&ctx.storage_root, task_key)?;
    let mut snapshot = OriginSnapshot::task(task_key, origin_ref, local_fields, remote_fields);
    if let Some(existing) =
        existing.filter(|existing| existing.matches_origin(&origin.provider, &origin.id))
    {
        snapshot.baseline = existing.baseline;
    }
    snapshot.provider_context = provider_context(&detail);
    Ok(snapshot)
}

fn provider_context(detail: &IssueDetail) -> ProviderContext {
    ProviderContext {
        status: detail.status.clone(),
        labels: detail.labels.clone(),
        comments_count: detail.comments_count.map(|count| count as u64),
    }
}

fn build_issue_reader<'a>(ctx: &'a Ctx) -> Result<Box<dyn IssueReader + 'a>> {
    let issues_config = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    match issues_config.provider {
        crate::config::IssueProviderType::Linear => Ok(Box::new(LinearIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
        ))),
        crate::config::IssueProviderType::Github => Ok(Box::new(GithubIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
            issues_config.gh_user.clone(),
        ))),
    }
}

fn resolve_origin_task_keys(
    ctx: &Ctx,
    tasks: &[String],
    command: &str,
    explicit_target_guidance: &str,
) -> Result<Vec<String>> {
    if !tasks.is_empty() {
        return Ok(dedupe_task_keys(tasks));
    }

    if ctx.is_json() || ctx.quiet || !ctx.ui.can_prompt() {
        bail!("{explicit_target_guidance}");
    }

    select_origin_task_keys(ctx, command)
}

fn dedupe_task_keys(tasks: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for task in tasks {
        let key = task::safe_task_key(task);
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    keys
}

fn select_origin_task_keys(ctx: &Ctx, command: &str) -> Result<Vec<String>> {
    let mut candidates = Vec::new();
    for path in task::task_document_paths(ctx)? {
        let selected = task::read_task_document_path(ctx, &path)?;
        if selected.document.origin.is_some() {
            candidates.push(selected);
        }
    }

    candidates.sort_by(|left, right| left.key.cmp(&right.key));
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let items = candidates
        .iter()
        .map(|candidate| {
            let origin = candidate
                .document
                .origin
                .as_ref()
                .expect("origin candidate");
            PromptItem::from_hint_parts(
                candidate.document.title_or_key(&candidate.key),
                vec![
                    format!("task {}", candidate.key),
                    format!("{}:{}", origin.provider, origin.id),
                ],
            )
        })
        .collect::<Vec<_>>();
    let selections = ctx
        .ui
        .multi_select_items(&format!("Tasks to {command}"), &items)?;
    let mut keys = Vec::new();
    for index in selections {
        let candidate = candidates
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {index}"))?;
        keys.push(candidate.key.clone());
    }
    Ok(keys)
}

fn write_json<T: Serialize>(value: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    writeln!(handle)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions};
    use crate::services::issues::{IssueDetail, IssueReader};
    use std::sync::Arc;

    struct FakeIssueReader {
        detail: IssueDetail,
    }

    impl FakeIssueReader {
        fn with_detail(detail: IssueDetail) -> Self {
            Self { detail }
        }
    }

    impl IssueReader for FakeIssueReader {
        fn get_issue_detail(&self, id: &str) -> anyhow::Result<IssueDetail> {
            assert_eq!(id, self.detail.identifier);
            Ok(self.detail.clone())
        }
    }

    fn ctx(root: &std::path::Path) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions::default(),
        )
    }

    fn ctx_with_runner_and_ui(root: &std::path::Path, runner: MockRunner, ui: Arc<MockUi>) -> Ctx {
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
                origin_policy: Default::default(),
            }),
            ..Config::default()
        };
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            config,
            Box::new(runner),
            Box::new(ui),
            CtxOptions::default(),
        )
    }

    #[test]
    fn reserved_pull_reports_canonical_command_name() {
        let dir = tempfile::tempdir().unwrap();
        let err = pull(&ctx(dir.path()), &["demo".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("wt task origin pull"));
        assert!(err.contains("reserved for provider issue origin"));
    }

    #[test]
    fn fetch_writes_snapshot_without_changing_task_document() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let provider = FakeIssueReader::with_detail(IssueDetail {
            identifier: "WT-142".into(),
            title: "Origin sync in TUI".into(),
            body: Some("remote body".into()),
            url: Some("https://linear.app/team/issue/WT-142".into()),
            status: Some("In Progress".into()),
            labels: vec!["wt".into(), "origin".into()],
            comments_count: Some(3),
            updated_at: Some("2026-06-06T05:18:00Z".into()),
        });

        fetch_with_reader(&ctx, &["origin-sync-tui".to_string()], &provider).unwrap();

        let task_content = std::fs::read_to_string(tasks_dir.join("origin-sync-tui.toml")).unwrap();
        assert!(task_content.contains("title = \"Origin sync TUI\""));
        assert!(task_content.contains("body = \"local body\""));

        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.remote.fields.title, "Origin sync in TUI");
        assert_eq!(snapshot.remote.fields.body, "remote body");
        assert_eq!(
            snapshot.provider_context.status.as_deref(),
            Some("In Progress")
        );
        assert_eq!(snapshot.provider_context.comments_count, Some(3));
    }

    #[test]
    fn fetch_interactive_selection_is_resolved_once() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"WT-142","title":"Origin sync in TUI","branchName":"team/wt-142-origin-sync","description":"remote body"}"#,
            true,
        );
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), runner, Arc::clone(&ui));

        fetch(&ctx, &[]).unwrap();

        assert_eq!(
            ui.prompts.lock().unwrap().as_slice(),
            ["multi_select: Tasks to fetch"]
        );
        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.remote.fields.title, "Origin sync in TUI");
    }

    #[test]
    fn diff_reports_both_changed_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local edited title"
branch = "origin-sync-tui"
body = "local edited body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Original title", "original body"),
            crate::origin_snapshot::FieldSnapshot::new("Remote edited title", "remote edited body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();

        let report = diff_task(&ctx, "origin-sync-tui").unwrap();

        assert_eq!(report.fields["title"].status, FieldDiffStatus::Conflict);
        assert_eq!(report.fields["body"].status, FieldDiffStatus::Conflict);
        assert!(report.conflicts.contains(&"title".to_string()));
        assert!(report.conflicts.contains(&"body".to_string()));
    }

    #[test]
    fn diff_fails_with_guidance_when_snapshot_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();

        let err = diff_task(&ctx, "origin-sync-tui").unwrap_err().to_string();

        assert!(err.contains("No fetched origin snapshot"));
        assert!(err.contains("wt task origin fetch origin-sync-tui"));
    }
}
