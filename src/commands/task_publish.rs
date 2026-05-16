use crate::commands::{issue, task};
use crate::context::Ctx;
use crate::services::issues::CreateIssueRequest;
use crate::services::issues::IssueProvider;
use anyhow::{Context, Result, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PublishResult {
    task_key: String,
    provider: String,
    issue_id: String,
}

pub(crate) fn run(ctx: &Ctx, task_key: &str) -> Result<()> {
    let key = task::safe_task_key(task_key);
    let mut document = task::read_task_document(ctx, &key)?;
    let provider_name = task::issue_provider_name(ctx)?;
    validate_publishable(&key, &document, &provider_name)?;

    let provider = issue::build_provider(ctx)?;
    let result = publish_document(ctx, &key, &provider_name, &mut document, provider.as_ref())?;

    ctx.ui.print_step(&format!(
        "{} -> {}:{}",
        result.task_key, result.provider, result.issue_id
    ));
    Ok(())
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
    use std::sync::Mutex;

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
            self.created.lock().unwrap().push(request.clone());
            Ok(IssueInfo {
                identifier: "PROJ-123".into(),
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

    #[test]
    fn run_rejects_missing_provider() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "add-publish",
            "title = \"Add publish\"\nbranch = \"add-publish\"\n",
        );
        let ctx = ctx_with_config(dir.path(), Config::default());

        let err = run(&ctx, "add-publish").unwrap_err();

        assert!(err.to_string().contains("No [issues] section in .wt.toml"));
    }

    #[test]
    fn run_rejects_missing_task() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_config(dir.path(), linear_config());

        let err = run(&ctx, "missing").unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed to read task: .local/tasks/missing.toml")
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
