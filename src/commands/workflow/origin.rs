use crate::config::IssueProviderType;
use crate::context::{Ctx, PromptItem};
use crate::origin_snapshot::{
    FieldHashes, FieldSnapshot, OriginRef, OriginSnapshot, ProviderContext, read_workflow_snapshot,
    write_snapshot,
};
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
use crate::services::issues::{
    IssueComment, IssueCommenter, IssueDetail, IssueFieldUpdate, IssueReader, IssueUpdater,
};
use crate::workflow::{self as workflow_store, WorkflowMetadata, WorkflowOrigin, WorkflowRecord};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::io::Write;

pub(crate) fn attach(ctx: &Ctx, workflow: &str, issue: &str) -> Result<()> {
    let reader = build_issue_reader(ctx)?;
    attach_with_reader(ctx, workflow, issue, reader.as_ref())
}

pub(crate) fn fetch(ctx: &Ctx, workflows: &[String]) -> Result<()> {
    let records = resolve_origin_workflows(
        ctx,
        workflows,
        "fetch",
        "wt workflow origin fetch requires WORKFLOW when it cannot open an interactive selector. Pass a workflow id or path, for example `wt workflow origin fetch <workflow>`.",
    )?;
    if records.is_empty() {
        ctx.ui
            .print_warning("No origin-backed workflows selected to fetch");
        return Ok(());
    }
    validate_fetchable_origin_workflows(&records)?;
    validate_origin_providers_match_config(ctx, &records)?;
    let reader = build_issue_reader(ctx)?;
    fetch_resolved_with_reader(ctx, records, reader.as_ref())
}

pub(crate) fn diff(ctx: &Ctx, workflows: &[String]) -> Result<()> {
    let records = resolve_origin_workflows(
        ctx,
        workflows,
        "diff",
        "wt workflow origin diff requires WORKFLOW when it cannot open an interactive selector. Pass a workflow id or path, for example `wt workflow origin diff <workflow>`.",
    )?;
    if records.is_empty() {
        ctx.ui
            .print_warning("No origin-backed workflows selected to diff");
        return Ok(());
    }

    let reports = records
        .iter()
        .map(|record| diff_workflow(ctx, record))
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

pub(crate) fn pull(ctx: &Ctx, workflows: &[String]) -> Result<()> {
    let records = resolve_origin_workflows(
        ctx,
        workflows,
        "pull",
        "wt workflow origin pull requires WORKFLOW when it cannot open an interactive selector. Pass a workflow id or path, for example `wt workflow origin pull <workflow>`.",
    )?;
    if records.is_empty() {
        ctx.ui
            .print_warning("No origin-backed workflows selected to pull");
        return Ok(());
    }
    if ctx.is_json() || ctx.quiet || !ctx.ui.can_prompt() {
        bail!(
            "wt workflow origin pull requires interactive field selection before editing Workflow title/body"
        );
    }

    for record in &records {
        let report = diff_workflow(ctx, record)?;
        print_diff_report(ctx, &report);
        let selection = prompt_pull_selection(ctx, &report)?;
        if !selection.any() {
            ctx.ui
                .print_warning(&format!("No workflow fields selected for {}", record.id));
            continue;
        }
        pull_workflow_record(ctx, record.clone(), selection)?;
        ctx.ui
            .print_plain(&format!("Pulled workflow origin fields for {}", record.id));
    }

    Ok(())
}

pub(crate) fn push(ctx: &Ctx, workflows: &[String]) -> Result<()> {
    let records = resolve_origin_workflows(
        ctx,
        workflows,
        "push",
        "wt workflow origin push requires WORKFLOW when it cannot open an interactive selector. Pass a workflow id or path, for example `wt workflow origin push <workflow>`.",
    )?;
    if records.is_empty() {
        ctx.ui
            .print_warning("No origin-backed workflows selected to push");
        return Ok(());
    }
    if ctx.is_json() || ctx.quiet || !ctx.ui.can_prompt() {
        bail!("wt workflow origin push requires confirmation before writing to provider issues");
    }

    validate_origin_providers_match_config(ctx, &records)?;
    let writer = build_issue_writer(ctx)?;
    for record in &records {
        let origin = require_origin(&record.id, &record.workflow, "push")?;
        let selection = PushSelection {
            append_comment: true,
            title: false,
            body: false,
        };
        ctx.ui.print_plain(&format!(
            "Workflow origin push for {}: {}:{}",
            record.id, origin.provider, origin.id
        ));
        ctx.ui.print_plain("  append comment: selected");
        ctx.ui.print_plain("  title/body overwrite: not selected");
        if !ctx.ui.confirm(
            &format!(
                "Append workflow origin comment to provider issue {}:{}?",
                origin.provider, origin.id
            ),
            false,
        )? {
            ctx.ui
                .print_warning(&format!("Skipped workflow origin push for {}", record.id));
            continue;
        }
        push_workflow_record_with_writer(ctx, record.clone(), selection, writer.as_ref())?;
        ctx.ui
            .print_plain(&format!("Pushed workflow origin comment for {}", record.id));
    }

    Ok(())
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
pub(crate) struct WorkflowOriginDiffReport {
    pub(crate) workflow_id: String,
    pub(crate) origin: OriginRef,
    pub(crate) fields: BTreeMap<String, FieldDiff>,
    pub(crate) conflicts: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PullSelection {
    title: bool,
    body: bool,
}

impl PullSelection {
    fn any(self) -> bool {
        self.title || self.body
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PushSelection {
    append_comment: bool,
    title: bool,
    body: bool,
}

impl PushSelection {
    fn any(self) -> bool {
        self.append_comment || self.title || self.body
    }
}

#[cfg(test)]
fn fetch_with_reader(ctx: &Ctx, workflows: &[String], reader: &dyn IssueReader) -> Result<()> {
    let records = resolve_origin_workflows(
        ctx,
        workflows,
        "fetch",
        "wt workflow origin fetch requires WORKFLOW when it cannot open an interactive selector. Pass a workflow id or path, for example `wt workflow origin fetch <workflow>`.",
    )?;
    fetch_resolved_with_reader(ctx, records, reader)
}

fn attach_with_reader(
    ctx: &Ctx,
    workflow: &str,
    issue: &str,
    reader: &dyn IssueReader,
) -> Result<()> {
    let mut record = read_workflow_record(ctx, workflow)?;
    let provider = configured_issue_provider_name(ctx)?;
    let detail = reader
        .get_issue_detail(issue)
        .with_context(|| format!("Failed to fetch provider issue {issue} for workflow attach"))?;
    record.workflow.origin = Some(WorkflowOrigin {
        provider: provider.to_string(),
        id: detail.identifier.clone(),
    });
    workflow_store::rewrite(ctx, &record.path, &record.workflow)?;

    let origin = record
        .workflow
        .origin
        .as_ref()
        .expect("origin just attached");
    let snapshot = fetched_workflow_snapshot(ctx, &record, origin, detail)?;
    write_snapshot(&ctx.storage_root, &snapshot)?;
    ctx.ui.print_plain(&format!(
        "Attached workflow origin for {}: {}:{}",
        record.id, origin.provider, origin.id
    ));
    Ok(())
}

fn fetch_resolved_with_reader(
    ctx: &Ctx,
    records: Vec<WorkflowRecord>,
    reader: &dyn IssueReader,
) -> Result<()> {
    if records.is_empty() {
        ctx.ui
            .print_warning("No origin-backed workflows selected to fetch");
        return Ok(());
    }

    for record in records {
        let result = fetch_one(ctx, &record, reader)?;
        ctx.ui.print_plain(&format!(
            "Fetched workflow origin for {}: {}:{}",
            result.workflow_id, result.provider, result.issue_id
        ));
    }

    Ok(())
}

fn diff_workflow(ctx: &Ctx, record: &WorkflowRecord) -> Result<WorkflowOriginDiffReport> {
    let origin = require_origin(&record.id, &record.workflow, "diff")?;
    let snapshot = read_workflow_snapshot(&ctx.storage_root, &record.id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No fetched origin snapshot for workflow {}. Run `wt workflow origin fetch {}` before diffing.",
            record.id,
            record.id
        )
    })?;
    if !snapshot.matches_origin(&origin.provider, &origin.id) {
        bail!(
            "Fetched origin snapshot for workflow {} does not match current [origin] {}:{}. Run `wt workflow origin fetch {}` to refresh origin evidence.",
            record.id,
            origin.provider,
            origin.id,
            record.id
        );
    }

    let local_fields = workflow_fields(&record.workflow);
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

    Ok(WorkflowOriginDiffReport {
        workflow_id: record.id.clone(),
        origin: snapshot.origin,
        fields,
        conflicts,
    })
}

#[cfg(test)]
fn pull_workflow_fields(ctx: &Ctx, workflow: &str, selection: PullSelection) -> Result<()> {
    let record = read_workflow_record(ctx, workflow)?;
    pull_workflow_record(ctx, record, selection)
}

fn pull_workflow_record(
    ctx: &Ctx,
    mut record: WorkflowRecord,
    selection: PullSelection,
) -> Result<()> {
    if !selection.any() {
        return Ok(());
    }

    let origin = require_origin(&record.id, &record.workflow, "pull")?;
    let mut snapshot = read_workflow_snapshot(&ctx.storage_root, &record.id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No fetched origin snapshot for workflow {}. Run `wt workflow origin fetch {}` before pulling.",
            record.id,
            record.id
        )
    })?;
    if !snapshot.matches_origin(&origin.provider, &origin.id) {
        bail!(
            "Fetched origin snapshot for workflow {} does not match current [origin] {}:{}. Run `wt workflow origin fetch {}` to refresh origin evidence.",
            record.id,
            origin.provider,
            origin.id,
            record.id
        );
    }

    if selection.title {
        record.workflow.title = non_empty_string(snapshot.remote.fields.title.clone());
    }
    if selection.body {
        record.workflow.body = non_empty_string(snapshot.remote.fields.body.clone());
    }
    workflow_store::rewrite(ctx, &record.path, &record.workflow)?;

    let local_fields = workflow_fields(&record.workflow);
    advance_baseline_for_selection(&mut snapshot, &local_fields, selection);
    write_snapshot(&ctx.storage_root, &snapshot)?;

    Ok(())
}

#[cfg(test)]
fn push_workflow(
    ctx: &Ctx,
    workflow: &str,
    selection: PushSelection,
    commenter: &dyn IssueCommenter,
) -> Result<()> {
    let writer = CommentOnlyWriter { commenter };
    push_workflow_with_writer(ctx, workflow, selection, &writer)
}

#[cfg(test)]
fn push_workflow_with_writer(
    ctx: &Ctx,
    workflow: &str,
    selection: PushSelection,
    writer: &dyn WorkflowIssueWriter,
) -> Result<()> {
    let record = read_workflow_record(ctx, workflow)?;
    push_workflow_record_with_writer(ctx, record, selection, writer)
}

fn push_workflow_record_with_writer(
    ctx: &Ctx,
    record: WorkflowRecord,
    selection: PushSelection,
    writer: &dyn WorkflowIssueWriter,
) -> Result<()> {
    if !selection.any() {
        return Ok(());
    }

    let origin = require_origin(&record.id, &record.workflow, "push")?;
    let mut updated_detail = None;
    if selection.title || selection.body {
        let local_fields = workflow_fields(&record.workflow);
        let update = IssueFieldUpdate {
            title: selection.title.then_some(local_fields.title),
            body: selection.body.then_some(local_fields.body),
        };
        updated_detail = Some(writer.update_issue_fields(&origin.id, update)?);
    }
    if selection.append_comment {
        writer.create_comment(
            &origin.id,
            &workflow_push_comment_body(&record.id, &record.workflow),
        )?;
    }
    if let Some(detail) = updated_detail {
        let snapshot = pushed_workflow_snapshot(ctx, &record, origin, detail, selection)?;
        write_snapshot(&ctx.storage_root, &snapshot)?;
    }

    Ok(())
}

trait WorkflowIssueWriter {
    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment>;
    fn update_issue_fields(&self, id: &str, update: IssueFieldUpdate) -> Result<IssueDetail>;
}

impl<T> WorkflowIssueWriter for T
where
    T: IssueCommenter + IssueUpdater,
{
    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment> {
        IssueCommenter::create_comment(self, id, body)
    }

    fn update_issue_fields(&self, id: &str, update: IssueFieldUpdate) -> Result<IssueDetail> {
        IssueUpdater::update_issue_fields(self, id, update)
    }
}

#[cfg(test)]
struct CommentOnlyWriter<'a> {
    commenter: &'a dyn IssueCommenter,
}

#[cfg(test)]
impl WorkflowIssueWriter for CommentOnlyWriter<'_> {
    fn create_comment(&self, id: &str, body: &str) -> Result<IssueComment> {
        self.commenter.create_comment(id, body)
    }

    fn update_issue_fields(&self, _id: &str, _update: IssueFieldUpdate) -> Result<IssueDetail> {
        bail!("provider does not support updating issue title/body")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FetchResult {
    workflow_id: String,
    provider: String,
    issue_id: String,
}

fn fetch_one(ctx: &Ctx, record: &WorkflowRecord, reader: &dyn IssueReader) -> Result<FetchResult> {
    let origin = require_origin(&record.id, &record.workflow, "fetch")?;
    let detail = reader
        .get_issue_detail(origin.id.as_str())
        .with_context(|| {
            format!(
                "Failed to fetch origin for workflow {}: {}",
                record.id, origin.id
            )
        })?;
    let snapshot = fetched_workflow_snapshot(ctx, record, origin, detail)?;
    write_snapshot(&ctx.storage_root, &snapshot)?;

    Ok(FetchResult {
        workflow_id: record.id.clone(),
        provider: origin.provider.clone(),
        issue_id: origin.id.clone(),
    })
}

fn fetched_workflow_snapshot(
    ctx: &Ctx,
    record: &WorkflowRecord,
    origin: &WorkflowOrigin,
    detail: IssueDetail,
) -> Result<OriginSnapshot> {
    let local_fields = workflow_fields(&record.workflow);
    let remote_fields = issue_fields(&detail);
    let origin_ref = origin_ref_from_detail(origin, &detail);

    let existing = read_workflow_snapshot(&ctx.storage_root, &record.id)?;
    let mut snapshot =
        OriginSnapshot::workflow(record.id.clone(), origin_ref, local_fields, remote_fields);
    if let Some(existing) =
        existing.filter(|existing| existing.matches_origin(&origin.provider, &origin.id))
    {
        snapshot.baseline = existing.baseline;
    }
    snapshot.provider_context = provider_context(&detail);
    Ok(snapshot)
}

fn pushed_workflow_snapshot(
    ctx: &Ctx,
    record: &WorkflowRecord,
    origin: &WorkflowOrigin,
    detail: IssueDetail,
    selection: PushSelection,
) -> Result<OriginSnapshot> {
    let local_fields = workflow_fields(&record.workflow);
    let remote_fields = issue_fields(&detail);
    let origin_ref = origin_ref_from_detail(origin, &detail);
    let mut snapshot = if let Some(mut existing) =
        read_workflow_snapshot(&ctx.storage_root, &record.id)?
            .filter(|existing| existing.matches_origin(&origin.provider, &origin.id))
    {
        existing.origin = origin_ref;
        existing.remote.fields = remote_fields;
        existing.remote.remote_updated_at = detail.updated_at.clone();
        existing.provider_context = provider_context(&detail);
        existing
    } else {
        let mut snapshot = OriginSnapshot::workflow(
            record.id.clone(),
            origin_ref,
            local_fields.clone(),
            remote_fields,
        );
        snapshot.provider_context = provider_context(&detail);
        snapshot
    };
    advance_baseline_for_push_selection(&mut snapshot, &local_fields, selection);
    Ok(snapshot)
}

fn origin_ref_from_detail(origin: &WorkflowOrigin, detail: &IssueDetail) -> OriginRef {
    let mut origin_ref = OriginRef::new(origin.provider.clone(), origin.id.clone());
    origin_ref.url = detail.url.clone();
    origin_ref.remote_updated_at = detail.updated_at.clone();
    origin_ref
}

fn provider_context(detail: &IssueDetail) -> ProviderContext {
    ProviderContext {
        status: detail.status.clone(),
        labels: detail.labels.clone(),
        comments_count: detail.comments_count.map(|count| count as u64),
    }
}

fn issue_fields(detail: &IssueDetail) -> FieldSnapshot {
    FieldSnapshot::new(
        detail.title.clone(),
        detail.body.clone().unwrap_or_default(),
    )
}

fn workflow_fields(workflow: &WorkflowMetadata) -> FieldSnapshot {
    FieldSnapshot::new(
        workflow.title.clone().unwrap_or_default(),
        workflow.body.clone().unwrap_or_default(),
    )
}

fn advance_baseline_for_selection(
    snapshot: &mut OriginSnapshot,
    local_fields: &FieldSnapshot,
    selection: PullSelection,
) {
    if selection.title {
        snapshot.baseline.fields.title = local_fields.title.clone();
        snapshot.baseline.local_hashes.title = FieldHashes::from_fields(local_fields).title;
    }
    if selection.body {
        snapshot.baseline.fields.body = local_fields.body.clone();
        snapshot.baseline.local_hashes.body = FieldHashes::from_fields(local_fields).body;
    }
}

fn advance_baseline_for_push_selection(
    snapshot: &mut OriginSnapshot,
    local_fields: &FieldSnapshot,
    selection: PushSelection,
) {
    advance_baseline_for_selection(
        snapshot,
        local_fields,
        PullSelection {
            title: selection.title,
            body: selection.body,
        },
    );
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

fn print_diff_report(ctx: &Ctx, report: &WorkflowOriginDiffReport) {
    ctx.ui.print_plain(&format!(
        "Workflow origin diff for {}: {}:{}",
        report.workflow_id, report.origin.provider, report.origin.id
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

fn prompt_pull_selection(ctx: &Ctx, report: &WorkflowOriginDiffReport) -> Result<PullSelection> {
    let items = vec![
        PromptItem::with_hint(
            "title",
            format!(
                "{} -> {}",
                report.fields["title"].local, report.fields["title"].remote
            ),
        ),
        PromptItem::with_hint(
            "body",
            format!(
                "{} -> {}",
                report.fields["body"].local, report.fields["body"].remote
            ),
        ),
    ];
    let selections = ctx.ui.multi_select_items(
        &format!("Workflow fields to pull for {}", report.workflow_id),
        &items,
    )?;
    Ok(PullSelection {
        title: selections.contains(&0),
        body: selections.contains(&1),
    })
}

fn validate_fetchable_origin_workflows(records: &[WorkflowRecord]) -> Result<()> {
    for record in records {
        require_origin(&record.id, &record.workflow, "fetch")?;
    }
    Ok(())
}

fn validate_origin_providers_match_config(ctx: &Ctx, records: &[WorkflowRecord]) -> Result<()> {
    let configured_provider = configured_issue_provider_name(ctx)?;
    for record in records {
        let origin = record
            .workflow
            .origin
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Workflow {} is missing [origin]", record.id))?;
        let origin_provider = origin.provider.trim();
        if origin_provider != configured_provider {
            bail!(
                "Workflow {} origin provider is {}, but configured issue provider is {}; refusing to route provider calls to the wrong backend",
                record.id,
                origin_provider,
                configured_provider
            );
        }
    }
    Ok(())
}

fn require_origin<'a>(
    workflow_id: &str,
    workflow: &'a WorkflowMetadata,
    command: &str,
) -> Result<&'a WorkflowOrigin> {
    workflow.origin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "wt workflow origin {command} requires a workflow with [origin]; use `wt workflow origin attach {workflow_id} <issue>`"
        )
    })
}

fn resolve_origin_workflows(
    ctx: &Ctx,
    workflows: &[String],
    command: &str,
    explicit_target_guidance: &str,
) -> Result<Vec<WorkflowRecord>> {
    if !workflows.is_empty() {
        return dedupe_workflow_records(
            workflows
                .iter()
                .map(|workflow| read_workflow_record(ctx, workflow))
                .collect::<Result<Vec<_>>>()?,
        );
    }

    if ctx.is_json() || ctx.quiet || !ctx.ui.can_prompt() {
        bail!("{explicit_target_guidance}");
    }

    select_origin_workflows(ctx, command)
}

fn dedupe_workflow_records(records: Vec<WorkflowRecord>) -> Result<Vec<WorkflowRecord>> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for record in records {
        if seen.insert(record.id.clone()) {
            deduped.push(record);
        }
    }
    Ok(deduped)
}

fn select_origin_workflows(ctx: &Ctx, command: &str) -> Result<Vec<WorkflowRecord>> {
    let mut candidates = workflow_store::list(ctx)?
        .into_iter()
        .filter(|record| record.workflow.origin.is_some())
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let items = candidates
        .iter()
        .map(|candidate| {
            let origin = candidate
                .workflow
                .origin
                .as_ref()
                .expect("origin candidate");
            PromptItem::from_hint_parts(
                candidate
                    .workflow
                    .title
                    .clone()
                    .unwrap_or_else(|| candidate.id.clone()),
                vec![
                    format!("workflow {}", candidate.id),
                    format!("{}:{}", origin.provider, origin.id),
                ],
            )
        })
        .collect::<Vec<_>>();
    let selections = ctx
        .ui
        .multi_select_items(&format!("Workflows to {command}"), &items)?;
    let mut records = Vec::new();
    for index in selections {
        let candidate = candidates
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Selected workflow index out of range: {index}"))?;
        records.push(candidate.clone());
    }
    Ok(records)
}

fn read_workflow_record(ctx: &Ctx, workflow: &str) -> Result<WorkflowRecord> {
    let path = workflow_store::resolve(ctx, workflow)?;
    let id = workflow_store::id_from_path(&path)?;
    let workflow = workflow_store::read(&path)?;
    Ok(WorkflowRecord { id, path, workflow })
}

fn configured_issue_provider_name(ctx: &Ctx) -> Result<&'static str> {
    let issues_config = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    Ok(match issues_config.provider {
        IssueProviderType::Linear => "linear",
        IssueProviderType::Github => "github",
    })
}

fn build_issue_reader<'a>(ctx: &'a Ctx) -> Result<Box<dyn IssueReader + 'a>> {
    let issues_config = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    match issues_config.provider {
        IssueProviderType::Linear => Ok(Box::new(LinearIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
        ))),
        IssueProviderType::Github => Ok(Box::new(GithubIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
            issues_config.gh_user.clone(),
        ))),
    }
}

fn build_issue_writer<'a>(ctx: &'a Ctx) -> Result<Box<dyn WorkflowIssueWriter + 'a>> {
    let issues_config = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    match issues_config.provider {
        IssueProviderType::Github => Ok(Box::new(GithubIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
            issues_config.gh_user.clone(),
        ))),
        IssueProviderType::Linear => bail!(
            "Configured issue provider linear does not support `wt workflow origin push` provider writes yet"
        ),
    }
}

fn workflow_push_comment_body(workflow_id: &str, workflow: &WorkflowMetadata) -> String {
    let fields = workflow_fields(workflow);
    format!(
        "wt workflow origin push for {workflow_id}\n\nTitle:\n{}\n\nBody:\n{}",
        fields.title, fields.body
    )
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
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

    fn ctx_with_issue_provider(root: &std::path::Path, provider: IssueProviderType) -> Ctx {
        let config = Config {
            issues: Some(IssuesConfig {
                provider,
                gh_user: None,
                origin_policy: Default::default(),
            }),
            ..Config::default()
        };
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions::default(),
        )
    }

    fn workflow_toml_with_origin(title: &str, body: &str, origin_id: &str) -> String {
        format!(
            r#"title = "{title}"
body = "{body}"
mode = "batch"
base_mode = "default"
created_at = "2026-06-06T00:00:00Z"
updated_at = "2026-06-06T00:00:00Z"

[origin]
provider = "linear"
id = "{origin_id}"

[policy]
pull_request = "none"
landing = "manual"

[policy.review]
codex_base = "none"

[[tasks]]
task = "origin-sync-tui"
run = "run-origin-sync-tui"
"#
        )
    }

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

    #[test]
    fn fetch_requires_workflow_origin_with_attach_guidance() {
        let dir = tempfile::tempdir().unwrap();
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            r#"title = "Local workflow"
body = "workflow body"
mode = "batch"
base_mode = "default"
created_at = "2026-06-06T00:00:00Z"
updated_at = "2026-06-06T00:00:00Z"

[policy]
pull_request = "none"
landing = "manual"

[policy.review]
codex_base = "none"

[[tasks]]
task = "origin-sync-tui"
run = "run-origin-sync-tui"
"#,
        )
        .unwrap();

        let err = fetch(&ctx(dir.path()), &["2026-06-06-001".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("wt workflow origin fetch"));
        assert!(err.contains("wt workflow origin attach 2026-06-06-001 <issue>"));
    }

    #[test]
    fn workflow_fetch_writes_snapshot_without_editing_child_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::write(
            tasks_dir.join("origin-sync-tui.toml"),
            r#"title = "Origin sync TUI"
branch = "origin-sync-tui"
body = "task body"
"#,
        )
        .unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            workflow_toml_with_origin("Ship provider-origin UX", "workflow body", "WT-100"),
        )
        .unwrap();
        let provider = FakeIssueReader::with_detail(IssueDetail {
            identifier: "WT-100".into(),
            title: "Remote workflow title".into(),
            body: Some("remote workflow body".into()),
            url: None,
            status: None,
            labels: vec![],
            comments_count: None,
            updated_at: Some("2026-06-06T05:18:00Z".into()),
        });

        fetch_with_reader(&ctx, &["2026-06-06-001".to_string()], &provider).unwrap();

        let task_content = std::fs::read_to_string(tasks_dir.join("origin-sync-tui.toml")).unwrap();
        assert!(!task_content.contains("[origin]"));
        let workflow_content =
            std::fs::read_to_string(workflows_dir.join("2026-06-06-001.toml")).unwrap();
        assert!(workflow_content.contains("Ship provider-origin UX"));
        let snapshot =
            crate::origin_snapshot::read_workflow_snapshot(&ctx.storage_root, "2026-06-06-001")
                .unwrap()
                .unwrap();
        assert_eq!(snapshot.remote.fields.title, "Remote workflow title");
    }

    #[test]
    fn workflow_fetch_rejects_origin_provider_mismatch_before_provider_call() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_issue_provider(dir.path(), IssueProviderType::Github);
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            workflow_toml_with_origin("Local workflow title", "local workflow body", "WT-100"),
        )
        .unwrap();

        let err = fetch(&ctx, &["2026-06-06-001".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("Workflow 2026-06-06-001 origin provider is linear"));
        assert!(err.contains("configured issue provider is github"));
    }

    #[test]
    fn workflow_push_rejects_origin_provider_mismatch_before_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_issue_provider(dir.path(), IssueProviderType::Github);
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            workflow_toml_with_origin("Local workflow title", "local workflow body", "WT-100"),
        )
        .unwrap();

        let err = push(&ctx, &["2026-06-06-001".to_string()])
            .unwrap_err()
            .to_string();

        assert!(err.contains("Workflow 2026-06-06-001 origin provider is linear"));
        assert!(err.contains("configured issue provider is github"));
    }

    #[test]
    fn workflow_pull_updates_workflow_title_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            workflow_toml_with_origin("Local workflow title", "local workflow body", "WT-100"),
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::workflow(
            "2026-06-06-001",
            crate::origin_snapshot::OriginRef::new("linear", "WT-100"),
            crate::origin_snapshot::FieldSnapshot::new(
                "Local workflow title",
                "local workflow body",
            ),
            crate::origin_snapshot::FieldSnapshot::new(
                "Remote workflow title",
                "remote workflow body",
            ),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();

        pull_workflow_fields(
            &ctx,
            "2026-06-06-001",
            PullSelection {
                title: true,
                body: false,
            },
        )
        .unwrap();

        let path = workflows_dir.join("2026-06-06-001.toml");
        let workflow = crate::workflow::read(&path).unwrap();
        assert_eq!(workflow.title.as_deref(), Some("Remote workflow title"));
        assert_eq!(workflow.body.as_deref(), Some("local workflow body"));
        assert_eq!(workflow.origin.unwrap().id, "WT-100");
    }

    #[test]
    fn workflow_pull_preserves_explicit_resolved_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0]);
        let ctx = Ctx::new_with_options(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
            CtxOptions::default(),
        );
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        let external_dir = dir.path().join("external-workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        std::fs::create_dir_all(&external_dir).unwrap();
        std::fs::write(
            workflows_dir.join("2026-06-06-001.toml"),
            workflow_toml_with_origin("Active workflow title", "active body", "WT-100"),
        )
        .unwrap();
        let external_path = external_dir.join("2026-06-06-001.toml");
        std::fs::write(
            &external_path,
            workflow_toml_with_origin("External workflow title", "external body", "WT-100"),
        )
        .unwrap();
        let snapshot = crate::origin_snapshot::OriginSnapshot::workflow(
            "2026-06-06-001",
            crate::origin_snapshot::OriginRef::new("linear", "WT-100"),
            crate::origin_snapshot::FieldSnapshot::new("External workflow title", "external body"),
            crate::origin_snapshot::FieldSnapshot::new("Remote workflow title", "remote body"),
        );
        crate::origin_snapshot::write_snapshot(&ctx.storage_root, &snapshot).unwrap();
        let external_target = external_path.to_string_lossy().to_string();

        pull(&ctx, &[external_target]).unwrap();

        let active = crate::workflow::read(&workflows_dir.join("2026-06-06-001.toml")).unwrap();
        let external = crate::workflow::read(&external_path).unwrap();
        assert_eq!(active.title.as_deref(), Some("Active workflow title"));
        assert_eq!(external.title.as_deref(), Some("Remote workflow title"));
    }

    #[test]
    fn workflow_push_comment_failure_preserves_workflow_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let workflows_dir = dir.path().join(".wt/execution/workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();
        let workflow_path = workflows_dir.join("2026-06-06-001.toml");
        std::fs::write(
            &workflow_path,
            workflow_toml_with_origin("Local workflow title", "local workflow body", "WT-100"),
        )
        .unwrap();
        let before = std::fs::read_to_string(&workflow_path).unwrap();
        let provider = FailingCommentProvider;

        let err = push_workflow(
            &ctx,
            "2026-06-06-001",
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
        let after = std::fs::read_to_string(&workflow_path).unwrap();
        assert_eq!(after, before);
    }
}
