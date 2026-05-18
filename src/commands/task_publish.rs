use crate::commands::{issue, task as task_command};
use crate::context::{Ctx, PromptItem};
use crate::services::issues::CreateIssueRequest;
use crate::services::issues::IssueProvider;
use crate::task;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PublishResult {
    task_key: String,
    provider: String,
    issue_id: String,
}

#[derive(Clone, Debug)]
struct PublishCandidate {
    task_key: String,
    document: task::TaskDocument,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PublishFailure {
    task_key: String,
    error: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PublishSummary {
    published: Vec<PublishResult>,
    skipped: Vec<String>,
    failed: Vec<PublishFailure>,
}

pub(crate) fn run(ctx: &Ctx, task_keys: &[String]) -> Result<()> {
    let keys = resolve_task_keys(ctx, task_keys)?;
    if keys.is_empty() {
        ctx.ui.print_warning("No tasks selected to publish");
        return Ok(());
    }

    let provider_name = task_command::issue_provider_name(ctx)?;
    let mut candidates = preflight_task_documents(ctx, &keys, &provider_name)?;
    let provider = issue::build_provider(ctx)?;
    let summary = publish_candidates(ctx, &mut candidates, &provider_name, provider.as_ref());
    print_summary(ctx, &summary);
    fail_if_needed(&summary)
}

fn resolve_task_keys(ctx: &Ctx, task_keys: &[String]) -> Result<Vec<String>> {
    if task_keys.is_empty() {
        return select_publish_task_keys(ctx);
    }

    Ok(dedupe_task_keys(task_keys.to_vec()))
}

fn dedupe_task_keys(task_keys: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for task_key in task_keys {
        let key = task::safe_task_key(&task_key);
        if seen.insert(key.clone()) {
            deduped.push(key);
        }
    }

    deduped
}

fn select_publish_task_keys(ctx: &Ctx) -> Result<Vec<String>> {
    let candidates = list_publish_candidates(ctx)?;
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let items = candidates
        .iter()
        .map(publish_candidate_item)
        .collect::<Vec<_>>();
    let selections = ctx.ui.multi_select_items("Tasks to publish", &items)?;
    let mut keys = Vec::new();
    for idx in selections {
        let candidate = candidates
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {idx}"))?;
        keys.push(candidate.task_key.clone());
    }
    Ok(keys)
}

fn list_publish_candidates(ctx: &Ctx) -> Result<Vec<PublishCandidate>> {
    let tasks_dir = ctx.repo_root.join(".local/tasks");
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in
        fs::read_dir(&tasks_dir).with_context(|| "Failed to read task directory: .local/tasks")?
    {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut candidates = Vec::new();
    for path in paths {
        let (key, document) = read_publish_candidate(ctx, &path)?;
        if document.origin.is_some() {
            continue;
        }
        candidates.push(PublishCandidate {
            task_key: key,
            document,
        });
    }
    Ok(candidates)
}

fn read_publish_candidate(ctx: &Ctx, path: &Path) -> Result<(String, task::TaskDocument)> {
    let key = task_key_from_path(ctx, path)?;
    let relative_path = publish_task_relative_path(ctx, path);
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read task: {relative_path}"))?;
    let document: task::TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {relative_path}"))?;
    Ok((key, document))
}

fn task_key_from_path(ctx: &Ctx, path: &Path) -> Result<String> {
    let relative_path = publish_task_relative_path(ctx, path);
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Task file is missing a key: {relative_path}"))
}

fn publish_task_relative_path(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
fn publish_candidate_label(candidate: &PublishCandidate) -> String {
    publish_candidate_item(candidate).render_plain()
}

fn publish_candidate_item(candidate: &PublishCandidate) -> PromptItem {
    task::task_resource_item(&candidate.task_key, &candidate.document, "origin:none")
}

fn preflight_task_documents(
    ctx: &Ctx,
    task_keys: &[String],
    provider_name: &str,
) -> Result<Vec<PublishCandidate>> {
    let mut candidates = Vec::new();

    for key in task_keys {
        let document = task::read_task_document(ctx, key)?;
        validate_publishable(key, &document, provider_name)?;
        candidates.push(PublishCandidate {
            task_key: key.clone(),
            document,
        });
    }

    Ok(candidates)
}

fn publish_candidates(
    ctx: &Ctx,
    candidates: &mut [PublishCandidate],
    provider_name: &str,
    provider: &dyn IssueProvider,
) -> PublishSummary {
    let mut summary = PublishSummary::default();

    for candidate in candidates {
        match publish_document(
            ctx,
            &candidate.task_key,
            provider_name,
            &mut candidate.document,
            provider,
        ) {
            Ok(result) => summary.published.push(result),
            Err(err) => summary.failed.push(PublishFailure {
                task_key: candidate.task_key.clone(),
                error: format!("{err:#}"),
            }),
        }
    }

    summary
}

fn print_summary(ctx: &Ctx, summary: &PublishSummary) {
    ctx.ui.print_step("Task publish summary");
    ctx.ui
        .print_dim(&format!("  Published: {}", format_published(summary)));
    ctx.ui
        .print_dim(&format!("  Skipped: {}", format_keys(&summary.skipped)));
    ctx.ui
        .print_dim(&format!("  Failed: {}", format_failed(summary)));

    for failure in &summary.failed {
        ctx.ui.print_dim(&format!(
            "  {}: {}",
            failure.task_key,
            first_error_line(&failure.error)
        ));
    }
}

fn format_published(summary: &PublishSummary) -> String {
    if summary.published.is_empty() {
        return "none".into();
    }
    summary
        .published
        .iter()
        .map(|result| {
            format!(
                "{} -> {}:{}",
                result.task_key, result.provider, result.issue_id
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_failed(summary: &PublishSummary) -> String {
    if summary.failed.is_empty() {
        return "none".into();
    }
    summary
        .failed
        .iter()
        .map(|failure| failure.task_key.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_keys(keys: &[String]) -> String {
    if keys.is_empty() {
        "none".into()
    } else {
        keys.join(", ")
    }
}

fn first_error_line(error: &str) -> &str {
    error.lines().next().unwrap_or(error)
}

fn fail_if_needed(summary: &PublishSummary) -> Result<()> {
    if summary.failed.is_empty() {
        return Ok(());
    }

    if let [failure] = summary.failed.as_slice() {
        bail!(
            "Task publish failed for {}: {}",
            failure.task_key,
            first_error_line(&failure.error)
        );
    }

    bail!("Task publish failed for {}", format_failed(summary))
}

fn publish_document(
    ctx: &Ctx,
    key: &str,
    provider_name: &str,
    document: &mut task::TaskDocument,
    provider: &dyn IssueProvider,
) -> Result<PublishResult> {
    publish_document_with_writer(
        ctx,
        key,
        provider_name,
        document,
        provider,
        task::write_task_document,
    )
}

fn publish_document_with_writer<F>(
    ctx: &Ctx,
    key: &str,
    provider_name: &str,
    document: &mut task::TaskDocument,
    provider: &dyn IssueProvider,
    mut write: F,
) -> Result<PublishResult>
where
    F: FnMut(&Ctx, &str, &task::TaskDocument) -> Result<()>,
{
    validate_publishable(key, document, provider_name)?;

    let issue = provider.create_issue(CreateIssueRequest {
        title: document.title.clone(),
        body: document.body.clone(),
    })?;
    let issue_id = issue.identifier;
    document.origin = Some(task::TaskOrigin {
        provider: provider_name.to_string(),
        id: issue_id.clone(),
    });

    write(ctx, key, document).with_context(|| {
        format!(
            "Provider issue {provider_name}:{issue_id} was created, but failed to write origin to {}. Add the [origin] table manually before retrying.",
            task::task_relative_path(key)
        )
    })?;

    Ok(PublishResult {
        task_key: key.to_string(),
        provider: provider_name.to_string(),
        issue_id,
    })
}

fn validate_publishable(
    key: &str,
    document: &task::TaskDocument,
    provider_name: &str,
) -> Result<()> {
    if let Some(origin) = &document.origin {
        if origin.provider != provider_name {
            bail!(
                "Task {key} already has origin for provider {} but configured issue provider is {provider_name}; refusing to publish duplicate issue",
                origin.provider
            );
        }
        bail!(
            "Task {key} already has origin {provider_name}:{}; refusing to publish duplicate issue",
            origin.id
        );
    }
    if document.title.trim().is_empty() {
        bail!("Task {key} has empty title; wt task publish requires a title");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::services::issues::{EnsuredBranch, IssueInfo, IssueListItem};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeIssueProvider {
        created: Mutex<Vec<CreateIssueRequest>>,
    }

    impl FakeIssueProvider {
        fn created_requests(&self) -> Vec<CreateIssueRequest> {
            self.created.lock().unwrap().clone()
        }
    }

    impl IssueProvider for FakeIssueProvider {
        fn get_issue(&self, _id: &str) -> Result<IssueInfo> {
            unimplemented!("task publish does not fetch provider issues")
        }

        fn list_issues(&self) -> Result<Vec<IssueListItem>> {
            unimplemented!("task publish does not list provider issues")
        }

        fn create_issue(&self, request: CreateIssueRequest) -> Result<IssueInfo> {
            let mut created = self.created.lock().unwrap();
            let identifier = format!("PROJ-{}", 123 + created.len());
            created.push(request.clone());
            Ok(IssueInfo {
                identifier,
                title: request.title,
                branch_name: Some("provider/suggested-branch".into()),
                body: Some(request.body),
            })
        }

        fn ensure_branch(
            &self,
            _id: &str,
            _base: Option<&str>,
            _branch_name: Option<&str>,
        ) -> Result<EnsuredBranch> {
            unimplemented!("task publish does not ensure provider branches")
        }

        fn on_start(&self, _id: &str) -> Result<()> {
            unimplemented!("task publish does not start provider issues")
        }

        fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
            unimplemented!("task publish does not clean provider issues")
        }
    }

    fn ctx_with_config(root: &std::path::Path, config: Config) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        )
    }

    fn ctx_with_config_and_ui(root: &std::path::Path, config: Config, ui: Arc<MockUi>) -> Ctx {
        Ctx::new(
            root.to_path_buf(),
            root.to_path_buf(),
            config,
            Box::new(MockRunner::new()),
            Box::new(ui),
        )
    }

    fn linear_config() -> Config {
        Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        }
    }

    fn write_task(root: &std::path::Path, key: &str, content: &str) {
        let tasks_dir = root.join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(tasks_dir.join(format!("{key}.toml")), content).unwrap();
    }

    fn publish_with_fake_provider(
        ctx: &Ctx,
        tasks: &[String],
        provider: &FakeIssueProvider,
    ) -> Result<PublishSummary> {
        let keys = resolve_task_keys(ctx, tasks)?;
        if keys.is_empty() {
            ctx.ui.print_warning("No tasks selected to publish");
            return Ok(PublishSummary::default());
        }

        let mut candidates = preflight_task_documents(ctx, &keys, "linear")?;
        let summary = publish_candidates(ctx, &mut candidates, "linear", provider);
        print_summary(ctx, &summary);
        fail_if_needed(&summary)?;
        Ok(summary)
    }

    #[test]
    fn run_rejects_missing_provider() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-publish",
            "title = \"Add publish\"\nbranch = \"add-publish\"\n",
        );
        let ctx = ctx_with_config(dir.path(), Config::default());
        let tasks = vec!["add-publish".to_string()];

        let err = run(&ctx, &tasks).unwrap_err();

        assert!(err.to_string().contains("No [issues] section in .wt.toml"));
    }

    #[test]
    fn run_rejects_missing_task() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_config(dir.path(), linear_config());
        let tasks = vec!["missing".to_string()];

        let err = run(&ctx, &tasks).unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed to read task: .local/tasks/missing.toml")
        );
    }

    #[test]
    fn publish_multiple_explicit_tasks_in_order() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        write_task(
            dir.path(),
            "task-b",
            "title = \"Task B\"\nbranch = \"task-b\"\n",
        );
        let ui = Arc::new(MockUi::new());
        let ctx = ctx_with_config_and_ui(dir.path(), linear_config(), ui.clone());
        let provider = FakeIssueProvider::default();
        let tasks = vec!["task-a".to_string(), "task-b".to_string()];

        let summary = publish_with_fake_provider(&ctx, &tasks, &provider).unwrap();

        assert_eq!(
            summary
                .published
                .iter()
                .map(|result| result.task_key.as_str())
                .collect::<Vec<_>>(),
            vec!["task-a", "task-b"]
        );
        assert_eq!(
            provider
                .created_requests()
                .iter()
                .map(|request| request.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Task A", "Task B"]
        );
        assert_eq!(
            task::read_task_document(&ctx, "task-a")
                .unwrap()
                .origin
                .unwrap()
                .id,
            "PROJ-123"
        );
        assert_eq!(
            task::read_task_document(&ctx, "task-b")
                .unwrap()
                .origin
                .unwrap()
                .id,
            "PROJ-124"
        );
        assert!(!dir.path().join(".local/task-runs").exists());

        let dims = ui.dims.lock().unwrap().clone();
        assert!(dims.contains(
            &"  Published: task-a -> linear:PROJ-123, task-b -> linear:PROJ-124".to_string()
        ));
        assert!(dims.contains(&"  Skipped: none".to_string()));
        assert!(dims.contains(&"  Failed: none".to_string()));
    }

    #[test]
    fn bare_publish_selects_unprocessed_local_tasks() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        write_task(
            dir.path(),
            "published-task",
            "title = \"Published\"\nbranch = \"published-task\"\n\n[origin]\nprovider = \"linear\"\nid = \"PROJ-1\"\n",
        );
        write_task(
            dir.path(),
            "task-b",
            "title = \"Task B\"\nbranch = \"task-b\"\n",
        );
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 1]);
        let ui = Arc::new(ui);
        let ctx = ctx_with_config_and_ui(dir.path(), linear_config(), ui);
        let provider = FakeIssueProvider::default();

        let summary = publish_with_fake_provider(&ctx, &[], &provider).unwrap();

        assert_eq!(
            summary
                .published
                .iter()
                .map(|result| result.task_key.as_str())
                .collect::<Vec<_>>(),
            vec!["task-a", "task-b"]
        );
        assert_eq!(provider.created_requests().len(), 2);
        assert_eq!(
            task::read_task_document(&ctx, "published-task")
                .unwrap()
                .origin
                .unwrap()
                .id,
            "PROJ-1"
        );
    }

    #[test]
    fn publish_candidate_label_shows_title_key_origin_and_branch() {
        let candidate = PublishCandidate {
            task_key: "add-publish".into(),
            document: task::TaskDocument {
                title: "Add publish".into(),
                branch: "team/add-publish".into(),
                body: String::new(),
                origin: None,
            },
        };

        assert_eq!(
            publish_candidate_label(&candidate),
            "Add publish  not published | task add-publish | branch team/add-publish"
        );
    }

    #[test]
    fn bare_publish_empty_selection_exits_successfully() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![]);
        let ui = Arc::new(ui);
        let ctx = ctx_with_config_and_ui(dir.path(), linear_config(), Arc::clone(&ui));
        let provider = FakeIssueProvider::default();

        let summary = publish_with_fake_provider(&ctx, &[], &provider).unwrap();

        assert!(summary.published.is_empty());
        assert!(provider.created_requests().is_empty());
        assert!(
            task::read_task_document(&ctx, "task-a")
                .unwrap()
                .origin
                .is_none()
        );
        assert_eq!(
            ui.warnings.lock().unwrap().as_slice(),
            ["No tasks selected to publish"]
        );
    }

    #[test]
    fn publish_dedupes_task_keys_preserving_first_visible_order() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        write_task(
            dir.path(),
            "task-b",
            "title = \"Task B\"\nbranch = \"task-b\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::default();
        let tasks = vec![
            "task-a".to_string(),
            "task-a".to_string(),
            "task-b".to_string(),
            "task-a".to_string(),
        ];

        let summary = publish_with_fake_provider(&ctx, &tasks, &provider).unwrap();

        assert_eq!(
            summary
                .published
                .iter()
                .map(|result| result.task_key.as_str())
                .collect::<Vec<_>>(),
            vec!["task-a", "task-b"]
        );
        assert_eq!(provider.created_requests().len(), 2);
    }

    #[test]
    fn publish_preflight_rejects_later_missing_task_before_provider_create() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::default();
        let tasks = vec!["task-a".to_string(), "missing-task".to_string()];

        let err = publish_with_fake_provider(&ctx, &tasks, &provider).unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed to read task: .local/tasks/missing-task.toml")
        );
        assert!(provider.created_requests().is_empty());
        assert!(
            task::read_task_document(&ctx, "task-a")
                .unwrap()
                .origin
                .is_none()
        );
    }

    #[test]
    fn publish_preflight_rejects_later_invalid_task_before_provider_create() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        write_task(dir.path(), "invalid-task", "branch = \"invalid-task\"\n");
        write_task(
            dir.path(),
            "task-b",
            "title = \"Task B\"\nbranch = \"task-b\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::default();
        let tasks = vec![
            "task-a".to_string(),
            "invalid-task".to_string(),
            "task-b".to_string(),
        ];

        let err = publish_with_fake_provider(&ctx, &tasks, &provider).unwrap_err();

        assert!(err.to_string().contains("invalid-task has empty title"));
        assert!(provider.created_requests().is_empty());
        assert!(
            task::read_task_document(&ctx, "task-a")
                .unwrap()
                .origin
                .is_none()
        );
        assert!(
            task::read_task_document(&ctx, "task-b")
                .unwrap()
                .origin
                .is_none()
        );
    }

    #[test]
    fn publish_preflight_rejects_later_existing_origin_before_provider_create() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "task-a",
            "title = \"Task A\"\nbranch = \"task-a\"\n",
        );
        write_task(
            dir.path(),
            "published-task",
            "title = \"Published\"\nbranch = \"published-task\"\n\n[origin]\nprovider = \"linear\"\nid = \"PROJ-1\"\n",
        );
        write_task(
            dir.path(),
            "task-b",
            "title = \"Task B\"\nbranch = \"task-b\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::default();
        let tasks = vec![
            "task-a".to_string(),
            "published-task".to_string(),
            "task-b".to_string(),
        ];

        let err = publish_with_fake_provider(&ctx, &tasks, &provider).unwrap_err();

        assert!(err.to_string().contains("already has origin linear:PROJ-1"));
        assert!(provider.created_requests().is_empty());
        assert!(
            task::read_task_document(&ctx, "task-a")
                .unwrap()
                .origin
                .is_none()
        );
        assert_eq!(
            task::read_task_document(&ctx, "published-task")
                .unwrap()
                .origin
                .unwrap()
                .id,
            "PROJ-1"
        );
        assert!(
            task::read_task_document(&ctx, "task-b")
                .unwrap()
                .origin
                .is_none()
        );
    }

    #[test]
    fn publish_rejects_existing_origin_before_provider_create() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-publish",
            "title = \"Add publish\"\nbranch = \"add-publish\"\n\n[origin]\nprovider = \"linear\"\nid = \"PROJ-1\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let mut document = task::read_task_document(&ctx, "add-publish").unwrap();
        let provider = FakeIssueProvider::default();

        let err = publish_document_with_writer(
            &ctx,
            "add-publish",
            "linear",
            &mut document,
            &provider,
            task::write_task_document,
        )
        .unwrap_err();

        assert!(err.to_string().contains("already has origin linear:PROJ-1"));
        assert!(provider.created_requests().is_empty());
    }

    #[test]
    fn publish_rejects_provider_mismatch_before_provider_create() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-publish",
            "title = \"Add publish\"\nbranch = \"add-publish\"\n\n[origin]\nprovider = \"github\"\nid = \"#7\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let mut document = task::read_task_document(&ctx, "add-publish").unwrap();
        let provider = FakeIssueProvider::default();

        let err = publish_document_with_writer(
            &ctx,
            "add-publish",
            "linear",
            &mut document,
            &provider,
            task::write_task_document,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("configured issue provider is linear")
        );
        assert!(provider.created_requests().is_empty());
    }

    #[test]
    fn publish_rejects_empty_title_before_provider_create() {
        let dir = tempfile::tempdir().unwrap();
        write_task(dir.path(), "add-publish", "branch = \"add-publish\"\n");
        let ctx = ctx_with_config(dir.path(), linear_config());
        let mut document = task::read_task_document(&ctx, "add-publish").unwrap();
        let provider = FakeIssueProvider::default();

        let err = publish_document_with_writer(
            &ctx,
            "add-publish",
            "linear",
            &mut document,
            &provider,
            task::write_task_document,
        )
        .unwrap_err();

        assert!(err.to_string().contains("empty title"));
        assert!(provider.created_requests().is_empty());
    }

    #[test]
    fn publish_writes_origin_and_preserves_task_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-publish",
            "title = \"Add publish\"\nbranch = \"manual/add-publish\"\nbody = \"Create provider issue.\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let mut document = task::read_task_document(&ctx, "add-publish").unwrap();
        let provider = FakeIssueProvider::default();

        let result = publish_document_with_writer(
            &ctx,
            "add-publish",
            "linear",
            &mut document,
            &provider,
            task::write_task_document,
        )
        .unwrap();

        assert_eq!(result.issue_id, "PROJ-123");
        let requests = provider.created_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].title, "Add publish");
        assert_eq!(requests[0].body, "Create provider issue.");

        let updated = task::read_task_document(&ctx, "add-publish").unwrap();
        assert_eq!(updated.title, "Add publish");
        assert_eq!(updated.branch, "manual/add-publish");
        assert_eq!(updated.body, "Create provider issue.");
        assert_eq!(updated.origin.as_ref().unwrap().provider, "linear");
        assert_eq!(updated.origin.as_ref().unwrap().id, "PROJ-123");
    }

    #[test]
    fn publish_write_failure_reports_created_issue_id() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-publish",
            "title = \"Add publish\"\nbranch = \"manual/add-publish\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let mut document = task::read_task_document(&ctx, "add-publish").unwrap();
        let provider = FakeIssueProvider::default();

        let err = publish_document_with_writer(
            &ctx,
            "add-publish",
            "linear",
            &mut document,
            &provider,
            |_ctx, _key, _document| anyhow::bail!("disk is read-only"),
        )
        .unwrap_err();

        let err = format!("{err:#}");
        assert!(err.contains("Provider issue linear:PROJ-123 was created"));
        assert!(err.contains(".local/tasks/add-publish.toml"));
        assert!(err.contains("disk is read-only"));
    }
}
