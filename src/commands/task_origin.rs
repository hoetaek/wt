use crate::context::Ctx;
use crate::context::PromptItem;
use crate::origin_snapshot::{
    FieldHashes, FieldSnapshot, OriginRef, OriginSnapshot, ProviderContext, read_task_snapshot,
    write_snapshot,
};
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
use crate::services::issues::{
    IssueComment, IssueCommenter, IssueDetail, IssueFieldUpdate, IssueReader, IssueUpdater,
};
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

pub(crate) fn attach(ctx: &Ctx, task: &str, issue: &str) -> Result<()> {
    ensure_interactive_write(ctx, "attach")?;
    let provider_name = crate::commands::task::issue_provider_name(ctx)?;
    let reader = build_issue_reader(ctx)?;
    let detail = reader
        .get_issue_detail(issue)
        .with_context(|| format!("Failed to fetch issue origin {issue}"))?;
    print_attach_preview(ctx, task, &provider_name, &detail);
    if !ctx
        .ui
        .confirm("Attach local TaskDocument to provider issue origin?", false)?
    {
        ctx.ui
            .print_warning(&format!("Skipped attach for {}", task::safe_task_key(task)));
        return Ok(());
    }

    write_attached_task_origin(ctx, task, &provider_name, detail)?;
    ctx.ui.print_plain(&format!(
        "Attached origin for {}",
        task::safe_task_key(task)
    ));
    Ok(())
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

pub(crate) fn pull(ctx: &Ctx, tasks: &[String]) -> Result<()> {
    ensure_interactive_write(ctx, "pull")?;
    let keys = resolve_origin_task_keys(
        ctx,
        tasks,
        "pull",
        "wt task origin pull requires TASK when it cannot open an interactive selector. Pass a task key, for example `wt task origin pull <task>`.",
    )?;
    if keys.is_empty() {
        ctx.ui
            .print_warning("No origin-backed tasks selected to pull");
        return Ok(());
    }

    for key in keys {
        let report = diff_task(ctx, &key)?;
        let selection = select_pull_fields(ctx, &report)?;
        if !selection.any() {
            ctx.ui.print_warning(&format!(
                "No fields selected to pull for {}",
                report.task_key
            ));
            continue;
        }
        print_pull_preview(ctx, &report, selection);
        if !ctx.ui.confirm(
            "Pull selected provider fields into local TaskDocument?",
            false,
        )? {
            ctx.ui
                .print_warning(&format!("Skipped pull for {}", report.task_key));
            continue;
        }
        pull_task_fields(ctx, &report.task_key, selection)?;
        ctx.ui
            .print_plain(&format!("Pulled origin fields for {}", report.task_key));
    }

    Ok(())
}

pub(crate) fn push(ctx: &Ctx, tasks: &[String]) -> Result<()> {
    ensure_interactive_write(ctx, "push")?;
    let keys = resolve_origin_task_keys(
        ctx,
        tasks,
        "push",
        "wt task origin push requires TASK when it cannot open an interactive selector. Pass a task key, for example `wt task origin push <task>`.",
    )?;
    if keys.is_empty() {
        ctx.ui
            .print_warning("No origin-backed tasks selected to push");
        return Ok(());
    }

    let provider = build_issue_push_provider(ctx)?;
    for key in &keys {
        let task_key = task::safe_task_key(key);
        let document = task::read_task_document(ctx, &task_key)?;
        ensure_push_provider_matches_origin(&task_key, &document, &provider)?;
    }

    for key in keys {
        let task_key = task::safe_task_key(&key);
        let document = task::read_task_document(ctx, &task_key)?;
        let report = diff_task(ctx, &task_key)?;
        let selection = select_push_operations(ctx, &document, &report, &provider)?;
        if !selection.any() {
            ctx.ui.print_warning(&format!(
                "No provider operations selected to push for {}",
                report.task_key
            ));
            continue;
        }
        print_push_preview(ctx, &document, &report, selection);
        if !ctx
            .ui
            .confirm("Push selected fields to provider issue?", false)?
        {
            ctx.ui
                .print_warning(&format!("Skipped push for {}", report.task_key));
            continue;
        }
        push_task(ctx, &report.task_key, selection, &provider)?;
        ctx.ui
            .print_plain(&format!("Pushed origin operations for {}", report.task_key));
    }

    Ok(())
}

fn ensure_interactive_write(ctx: &Ctx, command: &str) -> Result<()> {
    if ctx.is_json() || ctx.quiet || !ctx.ui.can_prompt() {
        bail!(
            "wt task origin {command} requires an interactive preview and confirmation before writing"
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PullSelection {
    pub(crate) title: bool,
    pub(crate) body: bool,
}

impl PullSelection {
    fn any(self) -> bool {
        self.title || self.body
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PushSelection {
    pub(crate) append_comment: bool,
    pub(crate) title: bool,
    pub(crate) body: bool,
}

impl PushSelection {
    fn any(self) -> bool {
        self.append_comment || self.title || self.body
    }
}

trait TaskOriginPushProvider {
    fn provider_name(&self) -> Option<&'static str> {
        None
    }

    fn supports_comment(&self) -> bool {
        false
    }

    fn supports_update_fields(&self) -> bool {
        false
    }

    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment>;

    fn update_issue_fields(&self, _id: &str, _update: IssueFieldUpdate) -> Result<IssueDetail> {
        bail!("provider does not support updating issue title/body")
    }

    fn refresh_issue_detail(&self, _id: &str) -> Result<Option<IssueDetail>> {
        Ok(None)
    }
}

impl<T> TaskOriginPushProvider for T
where
    T: IssueCommenter,
{
    fn supports_comment(&self) -> bool {
        true
    }

    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment> {
        IssueCommenter::create_comment(self, id, body)
    }
}

enum ConfiguredIssuePushProvider<'a> {
    Linear(LinearIssueProvider<'a>),
    Github(GithubIssueProvider<'a>),
}

impl TaskOriginPushProvider for ConfiguredIssuePushProvider<'_> {
    fn provider_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::Linear(_) => "linear",
            Self::Github(_) => "github",
        })
    }

    fn supports_comment(&self) -> bool {
        true
    }

    fn supports_update_fields(&self) -> bool {
        true
    }

    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment> {
        match self {
            Self::Linear(provider) => IssueCommenter::create_comment(provider, id, body),
            Self::Github(provider) => IssueCommenter::create_comment(provider, id, body),
        }
    }

    fn update_issue_fields(&self, id: &str, update: IssueFieldUpdate) -> Result<IssueDetail> {
        match self {
            Self::Linear(provider) => IssueUpdater::update_issue_fields(provider, id, update),
            Self::Github(provider) => IssueUpdater::update_issue_fields(provider, id, update),
        }
    }

    fn refresh_issue_detail(&self, id: &str) -> Result<Option<IssueDetail>> {
        match self {
            Self::Linear(provider) => provider.get_issue_detail(id).map(Some),
            Self::Github(provider) => provider.get_issue_detail(id).map(Some),
        }
    }
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
    let snapshot = read_matching_task_snapshot(ctx, &task_key, &document, "diff")?;

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

fn select_pull_fields(ctx: &Ctx, report: &TaskOriginDiffReport) -> Result<PullSelection> {
    let fields = pull_field_items(report);
    let items = fields
        .iter()
        .map(|(_, item)| item.clone())
        .collect::<Vec<_>>();
    let selections = ctx
        .ui
        .multi_select_items(&format!("Fields to pull for {}", report.task_key), &items)?;
    let mut selection = PullSelection::default();
    for index in selections {
        let (field, _) = fields
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Selected field index out of range: {index}"))?;
        match field.as_str() {
            "title" => selection.title = true,
            "body" => selection.body = true,
            _ => {}
        }
    }
    Ok(selection)
}

fn pull_field_items(report: &TaskOriginDiffReport) -> Vec<(String, PromptItem)> {
    ["title", "body"]
        .into_iter()
        .filter_map(|name| {
            let field = report.fields.get(name)?;
            Some((
                name.to_string(),
                PromptItem::from_hint_parts(
                    name.to_string(),
                    vec![
                        field.status.as_str().to_string(),
                        format!("local {}", preview_field_value(&field.local)),
                        format!("remote {}", preview_field_value(&field.remote)),
                    ],
                ),
            ))
        })
        .collect()
}

fn print_pull_preview(ctx: &Ctx, report: &TaskOriginDiffReport, selection: PullSelection) {
    ctx.ui.print_plain(&format!(
        "Pull preview for {}: {}:{}",
        report.task_key, report.origin.provider, report.origin.id
    ));
    if selection.title {
        print_selected_field_preview(ctx, "title", &report.fields["title"]);
    }
    if selection.body {
        print_selected_field_preview(ctx, "body", &report.fields["body"]);
    }
}

fn print_selected_field_preview(ctx: &Ctx, name: &str, field: &FieldDiff) {
    ctx.ui.print_plain(&format!(
        "  {name}: {} -> {}",
        preview_field_value(&field.local),
        preview_field_value(&field.remote)
    ));
}

fn select_push_operations<P>(
    ctx: &Ctx,
    document: &task::TaskDocument,
    report: &TaskOriginDiffReport,
    provider: &P,
) -> Result<PushSelection>
where
    P: TaskOriginPushProvider + ?Sized,
{
    let operations = push_operation_items(document, report, provider);
    if operations.is_empty() {
        bail!(
            "Configured issue provider does not support task origin push comment or field update operations"
        );
    }
    let items = operations
        .iter()
        .map(|(_, item)| item.clone())
        .collect::<Vec<_>>();
    let selections = ctx.ui.multi_select_items(
        &format!("Operations to push for {}", report.task_key),
        &items,
    )?;
    if selections.is_empty() && provider.supports_comment() {
        return Ok(PushSelection {
            append_comment: true,
            title: false,
            body: false,
        });
    }
    let mut selection = PushSelection::default();
    for index in selections {
        let (operation, _) = operations.get(index).ok_or_else(|| {
            anyhow::anyhow!("Selected push operation index out of range: {index}")
        })?;
        match operation.as_str() {
            "append_comment" => selection.append_comment = true,
            "title" => selection.title = true,
            "body" => selection.body = true,
            _ => {}
        }
    }
    Ok(selection)
}

fn push_operation_items<P>(
    document: &task::TaskDocument,
    report: &TaskOriginDiffReport,
    provider: &P,
) -> Vec<(String, PromptItem)>
where
    P: TaskOriginPushProvider + ?Sized,
{
    let mut items = Vec::new();
    if provider.supports_comment() {
        items.push((
            "append_comment".to_string(),
            PromptItem::from_hint_parts(
                "append comment".to_string(),
                vec![
                    "default status note".to_string(),
                    format!("branch {}", document.branch),
                ],
            ),
        ));
    }
    if provider.supports_update_fields() {
        for name in ["title", "body"] {
            if let Some(field) = report.fields.get(name) {
                items.push((
                    name.to_string(),
                    PromptItem::from_hint_parts(
                        format!("overwrite provider {name}"),
                        vec![
                            "unchecked field overwrite".to_string(),
                            field.status.as_str().to_string(),
                            format!("local {}", preview_field_value(&field.local)),
                        ],
                    ),
                ));
            }
        }
    }
    items
}

fn print_push_preview(
    ctx: &Ctx,
    document: &task::TaskDocument,
    report: &TaskOriginDiffReport,
    selection: PushSelection,
) {
    ctx.ui.print_plain(&format!(
        "Push preview for {}: {}:{}",
        report.task_key, report.origin.provider, report.origin.id
    ));
    if selection.append_comment {
        ctx.ui.print_plain("  append provider comment");
        ctx.ui.print_plain("  Comment body:");
        for line in push_comment_body(&report.task_key, document).lines() {
            ctx.ui.print_plain(&format!("    {line}"));
        }
    }
    if selection.title {
        print_selected_provider_overwrite(ctx, "title", &report.fields["title"]);
    }
    if selection.body {
        print_selected_provider_overwrite(ctx, "body", &report.fields["body"]);
    }
}

fn print_selected_provider_overwrite(ctx: &Ctx, name: &str, field: &FieldDiff) {
    ctx.ui.print_plain(&format!(
        "  overwrite provider {name}: {} -> {}",
        preview_field_value(&field.remote),
        preview_field_value(&field.local)
    ));
}

fn preview_field_value(value: &str) -> String {
    let mut preview = value.lines().next().unwrap_or_default().trim().to_string();
    if preview.chars().count() > 72 {
        preview = preview.chars().take(69).collect::<String>();
        preview.push_str("...");
    }
    if preview.is_empty() {
        "(empty)".into()
    } else {
        preview
    }
}

fn pull_task_fields(ctx: &Ctx, key: &str, selection: PullSelection) -> Result<()> {
    if !selection.any() {
        bail!("No task origin fields selected to pull");
    }

    let task_key = task::safe_task_key(key);
    let mut document = task::read_task_document(ctx, &task_key)?;
    let mut snapshot = read_matching_task_snapshot(ctx, &task_key, &document, "pull")?;

    if selection.title {
        document.title = snapshot.remote.fields.title.clone();
    }
    if selection.body {
        document.body = snapshot.remote.fields.body.clone();
    }

    task::write_task_document(ctx, &task_key, &document)?;

    advance_baseline_to_local(&mut snapshot, &document, selection.title, selection.body);
    write_snapshot(&ctx.storage_root, &snapshot)
}

fn push_task<P>(ctx: &Ctx, key: &str, selection: PushSelection, provider: &P) -> Result<()>
where
    P: TaskOriginPushProvider + ?Sized,
{
    if !selection.any() {
        bail!("No task origin provider operations selected to push");
    }

    let task_key = task::safe_task_key(key);
    let document = task::read_task_document(ctx, &task_key)?;
    let origin = document.origin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "wt task origin push requires a task with [origin]; use `wt task origin publish {task_key}` or `wt task origin attach {task_key} <issue>`"
        )
    })?;
    ensure_push_provider_matches_origin(&task_key, &document, provider)?;
    let mut snapshot = read_matching_task_snapshot(ctx, &task_key, &document, "push")?;

    if selection.append_comment {
        provider.create_comment(&origin.id, &push_comment_body(&task_key, &document))?;
    }

    if selection.title || selection.body {
        let detail = provider.update_issue_fields(
            &origin.id,
            IssueFieldUpdate {
                title: selection.title.then(|| document.title.clone()),
                body: selection.body.then(|| document.body.clone()),
            },
        )?;
        ensure_provider_update_matches(&document, selection, &detail)?;
        refresh_task_snapshot_from_issue(&mut snapshot, detail);
        advance_baseline_to_local(&mut snapshot, &document, selection.title, selection.body);
        write_snapshot(&ctx.storage_root, &snapshot)?;
    } else if let Some(detail) = provider.refresh_issue_detail(&origin.id)? {
        refresh_task_snapshot_from_issue(&mut snapshot, detail);
        write_snapshot(&ctx.storage_root, &snapshot)?;
    }

    Ok(())
}

fn ensure_push_provider_matches_origin<P>(
    task_key: &str,
    document: &task::TaskDocument,
    provider: &P,
) -> Result<()>
where
    P: TaskOriginPushProvider + ?Sized,
{
    let Some(configured_provider) = provider.provider_name() else {
        return Ok(());
    };
    let origin = document.origin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "wt task origin push requires a task with [origin]; use `wt task origin publish {task_key}` or `wt task origin attach {task_key} <issue>`"
        )
    })?;
    if origin.provider != configured_provider {
        bail!(
            "Cannot push task {task_key}: origin provider {} does not match configured provider {}",
            origin.provider,
            configured_provider
        );
    }
    Ok(())
}

#[cfg(test)]
fn attach_task_origin(ctx: &Ctx, key: &str, issue: &str, reader: &dyn IssueReader) -> Result<()> {
    let provider_name =
        crate::commands::task::issue_provider_name(ctx).unwrap_or_else(|_| "linear".into());
    attach_task_origin_with_provider(ctx, key, issue, &provider_name, reader)
}

#[cfg(test)]
fn attach_task_origin_with_provider(
    ctx: &Ctx,
    key: &str,
    issue: &str,
    provider_name: &str,
    reader: &dyn IssueReader,
) -> Result<()> {
    let detail = reader
        .get_issue_detail(issue)
        .with_context(|| format!("Failed to fetch issue origin {issue}"))?;
    write_attached_task_origin(ctx, key, provider_name, detail)
}

fn write_attached_task_origin(
    ctx: &Ctx,
    key: &str,
    provider_name: &str,
    detail: IssueDetail,
) -> Result<()> {
    let task_key = task::safe_task_key(key);
    let mut document = task::read_task_document(ctx, &task_key)?;
    if document.origin.is_some() {
        bail!("Task {task_key} already has [origin]");
    }

    document.origin = Some(task::TaskOrigin {
        provider: provider_name.to_string(),
        id: detail.identifier.clone(),
    });

    let local_fields = FieldSnapshot::new(document.title.clone(), document.body.clone());
    let remote_fields = FieldSnapshot::new(
        detail.title.clone(),
        detail.body.clone().unwrap_or_default(),
    );
    let mut origin_ref = OriginRef::new(provider_name, detail.identifier.clone());
    origin_ref.url = detail.url.clone();
    origin_ref.remote_updated_at = detail.updated_at.clone();
    let mut snapshot = OriginSnapshot::task(&task_key, origin_ref, local_fields, remote_fields);
    snapshot.provider_context = provider_context(&detail);

    task::write_task_document(ctx, &task_key, &document)?;
    write_snapshot(&ctx.storage_root, &snapshot)
}

fn print_attach_preview(ctx: &Ctx, task_key: &str, provider_name: &str, detail: &IssueDetail) {
    ctx.ui.print_plain(&format!(
        "Attach preview for {}: {}:{}",
        task::safe_task_key(task_key),
        provider_name,
        detail.identifier
    ));
    ctx.ui.print_plain(&format!(
        "  remote title: {}",
        preview_field_value(&detail.title)
    ));
    ctx.ui.print_plain(&format!(
        "  remote body: {}",
        preview_field_value(detail.body.as_deref().unwrap_or_default())
    ));
}

fn push_comment_body(task_key: &str, document: &task::TaskDocument) -> String {
    format!(
        "wt task origin push status note for {task_key}\n\nTitle: {}\nBranch: {}\n\n{}",
        document.title, document.branch, document.body
    )
}

fn ensure_provider_update_matches(
    document: &task::TaskDocument,
    selection: PushSelection,
    detail: &IssueDetail,
) -> Result<()> {
    if selection.title && detail.title != document.title {
        bail!(
            "Provider issue title after push did not match selected local title; run `wt task origin fetch` before retrying"
        );
    }
    if selection.body && detail.body.clone().unwrap_or_default() != document.body {
        bail!(
            "Provider issue body after push did not match selected local body; run `wt task origin fetch` before retrying"
        );
    }
    Ok(())
}

fn refresh_task_snapshot_from_issue(snapshot: &mut OriginSnapshot, detail: IssueDetail) {
    let mut origin = OriginRef::new(snapshot.origin.provider.clone(), snapshot.origin.id.clone());
    origin.url = detail.url.clone();
    origin.remote_updated_at = detail.updated_at.clone();
    let baseline = snapshot.baseline.clone();
    let mut refreshed = OriginSnapshot::task(
        snapshot.owner.clone(),
        origin,
        baseline.fields.clone(),
        FieldSnapshot::new(
            detail.title.clone(),
            detail.body.clone().unwrap_or_default(),
        ),
    );
    refreshed.baseline = baseline;
    refreshed.provider_context = provider_context(&detail);
    *snapshot = refreshed;
}

fn advance_baseline_to_local(
    snapshot: &mut OriginSnapshot,
    document: &task::TaskDocument,
    title: bool,
    body: bool,
) {
    let local_fields = FieldSnapshot::new(document.title.clone(), document.body.clone());
    let local_hashes = FieldHashes::from_fields(&local_fields);
    if title {
        snapshot.baseline.fields.title = local_fields.title;
        snapshot.baseline.local_hashes.title = local_hashes.title;
    }
    if body {
        snapshot.baseline.fields.body = local_fields.body;
        snapshot.baseline.local_hashes.body = local_hashes.body;
    }
}

fn read_matching_task_snapshot(
    ctx: &Ctx,
    task_key: &str,
    document: &task::TaskDocument,
    command: &str,
) -> Result<OriginSnapshot> {
    let origin = document.origin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "wt task origin {command} requires a task with [origin]; use `wt task origin publish {task_key}` or `wt task origin attach {task_key} <issue>`"
        )
    })?;
    let snapshot = read_task_snapshot(&ctx.storage_root, task_key)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No fetched origin snapshot for task {task_key}. Run `wt task origin fetch {task_key}` before {command}."
        )
    })?;
    if !snapshot.matches_origin(&origin.provider, &origin.id) {
        bail!(
            "Fetched origin snapshot for task {task_key} does not match current [origin] {}:{}. Run `wt task origin fetch {task_key}` to refresh origin evidence.",
            origin.provider,
            origin.id
        );
    }
    Ok(snapshot)
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

fn build_issue_push_provider<'a>(ctx: &'a Ctx) -> Result<ConfiguredIssuePushProvider<'a>> {
    let issues_config = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    match issues_config.provider {
        crate::config::IssueProviderType::Linear => Ok(ConfiguredIssuePushProvider::Linear(
            LinearIssueProvider::new(ctx.runner.as_ref(), Some(&ctx.repo_root)),
        )),
        crate::config::IssueProviderType::Github => Ok(ConfiguredIssuePushProvider::Github(
            GithubIssueProvider::new(
                ctx.runner.as_ref(),
                Some(&ctx.repo_root),
                issues_config.gh_user.clone(),
            ),
        )),
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
    use crate::services::issues::{IssueComment, IssueCommenter, IssueDetail, IssueReader};
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

    struct FailingCommentProvider;

    impl IssueCommenter for FailingCommentProvider {
        fn create_comment(&self, _id: &str, _body: &str) -> anyhow::Result<IssueComment> {
            anyhow::bail!("failed to create provider issue comment")
        }
    }

    struct SuccessfulCommentProvider;

    impl IssueCommenter for SuccessfulCommentProvider {
        fn create_comment(&self, id: &str, body: &str) -> anyhow::Result<IssueComment> {
            Ok(IssueComment {
                id: format!("{id}-comment"),
                body: body.to_string(),
                created_at: None,
            })
        }
    }

    struct SuccessfulUpdateProvider;

    impl TaskOriginPushProvider for SuccessfulUpdateProvider {
        fn supports_update_fields(&self) -> bool {
            true
        }

        fn create_comment(&self, _id: &str, _body: &str) -> anyhow::Result<IssueComment> {
            unreachable!("test does not append a comment")
        }

        fn update_issue_fields(
            &self,
            id: &str,
            update: IssueFieldUpdate,
        ) -> anyhow::Result<IssueDetail> {
            assert_eq!(id, "WT-142");
            Ok(IssueDetail {
                identifier: id.to_string(),
                title: update.title.unwrap_or_else(|| "Remote title".into()),
                body: Some("Remote body".into()),
                url: None,
                status: None,
                labels: vec![],
                comments_count: None,
                updated_at: Some("2026-06-06T05:18:00Z".into()),
            })
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
    fn pull_without_prompt_reports_confirmation_contract() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.set_prompt_available(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), MockRunner::new(), Arc::clone(&ui));

        let err = pull(&ctx, &["demo".to_string()]).unwrap_err().to_string();

        assert!(err.contains("wt task origin pull"));
        assert!(err.contains("interactive preview and confirmation"));
    }

    #[test]
    fn pull_updates_selected_title_and_advances_title_baseline_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Original title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Remote title", "remote body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();

        pull_task_fields(
            &ctx,
            "origin-sync-tui",
            PullSelection {
                title: true,
                body: false,
            },
        )
        .unwrap();

        let task = crate::task::read_task_document(&ctx, "origin-sync-tui").unwrap();
        assert_eq!(task.title, "Remote title");
        assert_eq!(task.body, "local body");

        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.baseline.fields.title, "Remote title");
        assert_eq!(snapshot.baseline.fields.body, "local body");
    }

    #[test]
    fn push_comment_failure_does_not_mutate_task_document() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();
        let provider = FailingCommentProvider;

        let err = push_task(
            &ctx,
            "origin-sync-tui",
            PushSelection {
                append_comment: true,
                title: false,
                body: false,
            },
            &provider,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("failed to create provider issue comment"));
        let task = crate::task::read_task_document(&ctx, "origin-sync-tui").unwrap();
        assert_eq!(task.title, "Local title");
        assert_eq!(task.body, "local body");
        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.baseline.fields.title, "Local title");
        assert_eq!(snapshot.baseline.fields.body, "local body");
    }

    #[test]
    fn push_comment_only_does_not_advance_title_or_body_baseline() {
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
            crate::origin_snapshot::FieldSnapshot::new("Remote title", "remote body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();

        push_task(
            &ctx,
            "origin-sync-tui",
            PushSelection {
                append_comment: true,
                title: false,
                body: false,
            },
            &SuccessfulCommentProvider,
        )
        .unwrap();

        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.baseline.fields.title, "Original title");
        assert_eq!(snapshot.baseline.fields.body, "original body");
    }

    #[test]
    fn push_title_overwrite_advances_title_baseline_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local edited title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Original title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Remote title", "remote body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();

        push_task(
            &ctx,
            "origin-sync-tui",
            PushSelection {
                append_comment: false,
                title: true,
                body: false,
            },
            &SuccessfulUpdateProvider,
        )
        .unwrap();

        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.baseline.fields.title, "Local edited title");
        assert_eq!(snapshot.baseline.fields.body, "local body");
        assert_eq!(snapshot.remote.fields.title, "Local edited title");
        assert_eq!(snapshot.remote.fields.body, "Remote body");
    }

    #[test]
    fn attach_writes_origin_and_snapshot_without_overwriting_body() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("scratch-clean.toml"),
            r#"title = "Scratch cleanup"
branch = "scratch-clean"
body = "keep local body"
"#,
        )
        .unwrap();
        let provider = FakeIssueReader::with_detail(IssueDetail {
            identifier: "WT-200".into(),
            title: "Remote scratch cleanup".into(),
            body: Some("remote body".into()),
            url: None,
            status: None,
            labels: vec![],
            comments_count: None,
            updated_at: Some("2026-06-06T05:18:00Z".into()),
        });

        attach_task_origin(&ctx, "scratch-clean", "WT-200", &provider).unwrap();

        let task = crate::task::read_task_document(&ctx, "scratch-clean").unwrap();
        assert_eq!(task.title, "Scratch cleanup");
        assert_eq!(task.body, "keep local body");
        assert_eq!(task.origin.unwrap().id, "WT-200");
        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "scratch-clean")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.remote.fields.title, "Remote scratch cleanup");
        assert_eq!(snapshot.baseline.fields.title, "Scratch cleanup");
    }

    #[test]
    fn pull_declined_confirmation_does_not_mutate_task_document() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Original title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Remote title", "remote body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx(dir.path()).storage_root, &snapshot).unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), MockRunner::new(), Arc::clone(&ui));

        pull(&ctx, &["origin-sync-tui".to_string()]).unwrap();

        let task = crate::task::read_task_document(&ctx, "origin-sync-tui").unwrap();
        assert_eq!(task.title, "Local title");
        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.baseline.fields.title, "Original title");
    }

    #[test]
    fn attach_declined_confirmation_does_not_write_origin() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("scratch-clean.toml"),
            r#"title = "Scratch cleanup"
branch = "scratch-clean"
body = "keep local body"
"#,
        )
        .unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"WT-200","title":"Remote scratch cleanup","description":"remote body"}"#,
            true,
        );
        let mut ui = MockUi::new();
        ui.add_confirm(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), runner, Arc::clone(&ui));

        attach(&ctx, "scratch-clean", "WT-200").unwrap();

        let task = crate::task::read_task_document(&ctx, "scratch-clean").unwrap();
        assert!(task.origin.is_none());
        assert!(
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "scratch-clean")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn push_declined_confirmation_does_not_create_provider_comment() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let ctx_for_snapshot = ctx(dir.path());
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx_for_snapshot.storage_root, &snapshot).unwrap();
        let runner = MockRunner::new();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), runner, Arc::clone(&ui));

        push(&ctx, &["origin-sync-tui".to_string()]).unwrap();

        let snapshot =
            crate::origin_snapshot::read_task_snapshot(&ctx.storage_root, "origin-sync-tui")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.baseline.fields.title, "Local title");
    }

    #[test]
    fn push_rejects_task_origin_provider_mismatch_before_provider_write() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r##"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "github"
id = "#42"
"##,
        )
        .unwrap();
        let ctx_for_snapshot = ctx(dir.path());
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("github", "#42"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx_for_snapshot.storage_root, &snapshot).unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(true);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), MockRunner::new(), Arc::clone(&ui));

        let err = push(&ctx, &["origin-sync-tui".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("origin provider github"));
        assert!(err.contains("configured provider linear"));
    }

    #[test]
    fn push_comment_preview_includes_exact_comment_body_before_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body with private note"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let ctx_for_snapshot = ctx(dir.path());
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new(
                "Local title",
                "local body with private note",
            ),
            crate::origin_snapshot::FieldSnapshot::new(
                "Local title",
                "local body with private note",
            ),
        );
        crate::origin_snapshot::write_snapshot(&ctx_for_snapshot.storage_root, &snapshot).unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        ui.add_confirm(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), MockRunner::new(), Arc::clone(&ui));

        push(&ctx, &["origin-sync-tui".to_string()]).unwrap();

        let rendered = ui.steps.lock().unwrap().join("\n");
        assert!(rendered.contains("Comment body:"));
        assert!(rendered.contains("Title: Local title"));
        assert!(rendered.contains("Branch: origin-sync-tui"));
        assert!(rendered.contains("local body with private note"));
    }

    #[test]
    fn push_empty_operation_selection_defaults_to_comment_before_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Local title"
branch = "origin-sync-tui"
body = "local body"

[origin]
provider = "linear"
id = "WT-142"
"#,
        )
        .unwrap();
        let ctx_for_snapshot = ctx(dir.path());
        let snapshot = crate::origin_snapshot::OriginSnapshot::task(
            "origin-sync-tui",
            crate::origin_snapshot::OriginRef::new("linear", "WT-142"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
            crate::origin_snapshot::FieldSnapshot::new("Local title", "local body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx_for_snapshot.storage_root, &snapshot).unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![]);
        ui.add_confirm(false);
        let ui = Arc::new(ui);
        let ctx = ctx_with_runner_and_ui(dir.path(), MockRunner::new(), Arc::clone(&ui));

        push(&ctx, &["origin-sync-tui".to_string()]).unwrap();

        assert!(
            ui.prompts
                .lock()
                .unwrap()
                .contains(&"confirm: Push selected fields to provider issue?".to_string())
        );
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
