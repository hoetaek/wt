use crate::commands::new as new_command;
use crate::commands::{issue, issue_selection};
use crate::config::IssueProviderType;
use crate::context::Ctx;
use crate::services::issues::{IssueInfo, IssueProvider};
use crate::task::{self, PreparedTask, TaskDocument, TaskOrigin};
use crate::worktree_naming;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportedTask {
    key: String,
    provider: String,
    issue_id: String,
}

#[derive(Clone, Debug)]
struct IssueTaskCandidate {
    key: String,
    branch: String,
    provider: String,
    issue_id: String,
    document: TaskDocument,
}

pub(crate) fn prepare_named_tasks(ctx: &Ctx, names: &[String]) -> Result<Vec<PreparedTask>> {
    if names.is_empty() {
        bail!("Usage: wt <batch|stack> task <task>...");
    }

    let mut seen = HashSet::new();
    let mut tasks = Vec::new();
    for name in names {
        let title = name.trim();
        if title.is_empty() {
            bail!("Task cannot be empty");
        }
        let key = task_key_from_text(title)?;
        if !seen.insert(key.clone()) {
            bail!("Duplicate task: {key}");
        }

        let doc = if task::task_exists(ctx, &key) {
            task::read_task_document(ctx, &key)?
        } else {
            let branch = new_command::branch_name_from_words(&[title.to_string()])?;
            let doc = TaskDocument {
                title: title.to_string(),
                branch,
                body: String::new(),
                origin: None,
            };
            task::write_task_document(ctx, &key, &doc)?;
            doc
        };

        tasks.push(PreparedTask {
            key,
            branch: doc.branch,
        });
    }

    Ok(tasks)
}

pub(crate) fn prepare_issue_tasks(ctx: &Ctx, issues: &[String]) -> Result<Vec<PreparedTask>> {
    let provider = issue::build_provider(ctx)?;
    let provider_name = issue_provider_name(ctx)?;
    let candidates = resolve_issue_task_candidates(ctx, issues, &provider_name, provider.as_ref())?;
    write_issue_task_candidates(ctx, &candidates)?;

    Ok(candidates
        .into_iter()
        .map(|candidate| PreparedTask {
            key: candidate.key,
            branch: candidate.branch,
        })
        .collect())
}

pub(crate) fn import(ctx: &Ctx, issues: &[String]) -> Result<()> {
    let provider_name = issue_provider_name(ctx)?;
    let provider = issue::build_provider(ctx)?;
    let issue_ids = resolve_import_issue_ids(ctx, issues, provider.as_ref())?;
    if issue_ids.is_empty() {
        ctx.ui.print_warning("No issues selected to import");
        return Ok(());
    }

    let imported = import_issue_task_documents(ctx, &issue_ids, &provider_name, provider.as_ref())?;
    print_import_summary(ctx, &imported);
    Ok(())
}

fn resolve_import_issue_ids(
    ctx: &Ctx,
    issues: &[String],
    provider: &dyn IssueProvider,
) -> Result<Vec<String>> {
    if issues.is_empty() {
        return Ok(issue_selection::select_issues_with_provider(
            ctx,
            "Select issues to import",
            provider,
        )?
        .into_iter()
        .map(|issue| issue.identifier)
        .collect());
    }

    Ok(issues.to_vec())
}

fn import_issue_task_documents(
    ctx: &Ctx,
    issues: &[String],
    provider_name: &str,
    provider: &dyn IssueProvider,
) -> Result<Vec<ImportedTask>> {
    let candidates = resolve_issue_task_candidates(ctx, issues, provider_name, provider)?;
    write_issue_task_candidates(ctx, &candidates)?;

    Ok(candidates
        .into_iter()
        .map(|candidate| ImportedTask {
            key: candidate.key,
            provider: candidate.provider,
            issue_id: candidate.issue_id,
        })
        .collect())
}

fn resolve_issue_task_candidates(
    ctx: &Ctx,
    issues: &[String],
    provider_name: &str,
    provider: &dyn IssueProvider,
) -> Result<Vec<IssueTaskCandidate>> {
    validate_issue_ids(issues)?;

    let mut candidates = Vec::new();
    for source in issues {
        let issue = provider.get_issue(source.trim().trim_start_matches('#'))?;
        candidates.push(issue_task_candidate(ctx, provider_name, issue)?);
    }

    validate_import_candidates(ctx, &candidates)?;
    Ok(candidates)
}

fn validate_issue_ids(issues: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for source in issues {
        let issue_id = source.trim();
        if issue_id.is_empty() {
            bail!("Issue id cannot be empty");
        }
        let dedupe_key = issue_id.trim_start_matches('#').to_string();
        if !seen.insert(dedupe_key) {
            bail!("Duplicate issue id: {issue_id}");
        }
    }
    Ok(())
}

fn issue_task_candidate(
    ctx: &Ctx,
    provider_name: &str,
    issue: IssueInfo,
) -> Result<IssueTaskCandidate> {
    let suggested_branch = issue.branch_name.clone();
    let naming = worktree_naming::generate(
        ctx,
        &issue.identifier,
        &issue.title,
        suggested_branch.as_deref(),
    )?;
    let branch = naming
        .and_then(|naming| naming.branch)
        .or(suggested_branch)
        .unwrap_or_default();
    let issue_id = issue.identifier;
    let key = task::safe_task_key(&issue_id);
    let document = TaskDocument {
        title: issue.title,
        branch: branch.clone(),
        body: issue.body.unwrap_or_default(),
        origin: Some(TaskOrigin {
            provider: provider_name.to_string(),
            id: issue_id.clone(),
        }),
    };

    Ok(IssueTaskCandidate {
        key,
        branch,
        provider: provider_name.to_string(),
        issue_id,
        document,
    })
}

fn validate_import_candidates(ctx: &Ctx, candidates: &[IssueTaskCandidate]) -> Result<()> {
    let mut seen = HashSet::new();
    for candidate in candidates {
        if !seen.insert(candidate.key.clone()) {
            bail!(
                "Duplicate issue id resolves to task {}; refusing to import duplicate",
                candidate.key
            );
        }
        if task::task_exists(ctx, &candidate.key) {
            bail!(
                "TaskDocument already exists at {}; refusing to overwrite local task edits",
                task::task_relative_path(&candidate.key)
            );
        }
    }
    Ok(())
}

fn write_issue_task_candidates(ctx: &Ctx, candidates: &[IssueTaskCandidate]) -> Result<()> {
    for candidate in candidates {
        task::write_new_task_document(ctx, &candidate.key, &candidate.document).with_context(
            || {
                format!(
                    "Failed to import provider issue {}:{} to {}",
                    candidate.provider,
                    candidate.issue_id,
                    task::task_relative_path(&candidate.key)
                )
            },
        )?;
    }
    Ok(())
}

fn print_import_summary(ctx: &Ctx, imported: &[ImportedTask]) {
    ctx.ui.print_step("Task import summary");
    ctx.ui
        .print_dim(&format!("  Imported: {}", format_imported(imported)));
}

fn format_imported(imported: &[ImportedTask]) -> String {
    if imported.is_empty() {
        return "none".into();
    }
    imported
        .iter()
        .map(|task| format!("{} <- {}:{}", task.key, task.provider, task.issue_id))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn issue_provider_name(ctx: &Ctx) -> Result<String> {
    let issues = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    Ok(match issues.provider {
        IssueProviderType::Github => "github",
        IssueProviderType::Linear => "linear",
    }
    .into())
}

fn task_key_from_text(value: &str) -> Result<String> {
    new_command::branch_name_from_words(&[value.to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use crate::services::issues::{CreateIssueRequest, EnsuredBranch, IssueListItem};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct FakeIssueProvider {
        issues: Vec<IssueInfo>,
        list: Vec<IssueListItem>,
        fetched: Mutex<Vec<String>>,
    }

    impl FakeIssueProvider {
        fn with_issues(issues: Vec<IssueInfo>) -> Self {
            Self {
                issues,
                list: Vec::new(),
                fetched: Mutex::new(Vec::new()),
            }
        }

        fn with_list(mut self, list: Vec<IssueListItem>) -> Self {
            self.list = list;
            self
        }

        fn fetched_ids(&self) -> Vec<String> {
            self.fetched.lock().unwrap().clone()
        }
    }

    impl IssueProvider for FakeIssueProvider {
        fn get_issue(&self, id: &str) -> Result<IssueInfo> {
            self.fetched.lock().unwrap().push(id.to_string());
            let lookup = id.trim_start_matches('#');
            self.issues
                .iter()
                .find(|issue| {
                    issue.identifier == id || issue.identifier.trim_start_matches('#') == lookup
                })
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing fake issue {id}"))
        }

        fn list_issues(&self) -> Result<Vec<IssueListItem>> {
            Ok(self.list.clone())
        }

        fn create_issue(&self, _request: CreateIssueRequest) -> Result<IssueInfo> {
            unimplemented!("task import does not create provider issues")
        }

        fn ensure_branch(
            &self,
            _id: &str,
            _base: Option<&str>,
            _branch_name: Option<&str>,
        ) -> Result<EnsuredBranch> {
            unimplemented!("task import does not ensure provider branches")
        }

        fn on_start(&self, _id: &str) -> Result<()> {
            unimplemented!("task import does not start provider issues")
        }

        fn on_clean(&self, _id: &str, _branch: &str) -> Result<()> {
            unimplemented!("task import does not clean provider issues")
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

    fn issue(
        identifier: &str,
        title: &str,
        branch_name: Option<&str>,
        body: Option<&str>,
    ) -> IssueInfo {
        IssueInfo {
            identifier: identifier.into(),
            title: title.into(),
            branch_name: branch_name.map(str::to_string),
            body: body.map(str::to_string),
        }
    }

    fn list_item(identifier: &str, title: &str, hint: &str) -> IssueListItem {
        IssueListItem {
            identifier: identifier.into(),
            title: title.into(),
            display: format!("{identifier} {title}"),
            hint: Some(hint.into()),
        }
    }

    fn write_task(root: &std::path::Path, key: &str, content: &str) {
        let tasks_dir = root.join(".local/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(tasks_dir.join(format!("{key}.toml")), content).unwrap();
    }

    #[test]
    fn prepare_issue_tasks_writes_task_toml() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor","description":"Long issue body"}"#,
            true,
        );
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let tasks = prepare_issue_tasks(&ctx, &["PROJ-123".into()]).unwrap();

        assert_eq!(tasks[0].key, "PROJ-123");
        assert_eq!(tasks[0].branch, "alice/proj-123-fix-editor");
        let content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(content.contains("title = \"Fix editor\""));
        assert!(content.contains("branch = \"alice/proj-123-fix-editor\""));
        assert!(content.contains("body = \"\"\""));
        assert!(content.contains("Long issue body"));
        assert!(content.contains("[origin]"));
        assert!(content.contains("provider = \"linear\""));
        assert!(content.contains("id = \"PROJ-123\""));
    }

    #[test]
    fn import_issue_task_documents_writes_body_branch_origin_without_running_work() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::with_issues(vec![issue(
            "PROJ-123",
            "Fix editor",
            Some("alice/proj-123-fix-editor"),
            Some("Long issue body"),
        )]);

        let imported =
            import_issue_task_documents(&ctx, &["PROJ-123".into()], "linear", &provider).unwrap();

        assert_eq!(
            imported,
            vec![ImportedTask {
                key: "PROJ-123".into(),
                provider: "linear".into(),
                issue_id: "PROJ-123".into(),
            }]
        );
        let document = task::read_task_document(&ctx, "PROJ-123").unwrap();
        assert_eq!(document.title, "Fix editor");
        assert_eq!(document.branch, "alice/proj-123-fix-editor");
        assert_eq!(document.body, "Long issue body");
        let origin = document.origin.unwrap();
        assert_eq!(origin.provider, "linear");
        assert_eq!(origin.id, "PROJ-123");
        assert!(!dir.path().join(".local/task-runs").exists());
    }

    #[test]
    fn import_rejects_missing_provider() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_config(dir.path(), Config::default());

        let err = import(&ctx, &["PROJ-123".into()]).unwrap_err();

        assert!(err.to_string().contains("No [issues] section in .wt.toml"));
    }

    #[test]
    fn import_rejects_duplicate_issue_ids_before_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::with_issues(vec![issue(
            "PROJ-123",
            "Fix editor",
            Some("alice/proj-123-fix-editor"),
            Some("Long issue body"),
        )]);

        let err = import_issue_task_documents(
            &ctx,
            &["PROJ-123".into(), "PROJ-123".into()],
            "linear",
            &provider,
        )
        .unwrap_err();

        assert!(err.to_string().contains("Duplicate issue id: PROJ-123"));
        assert!(provider.fetched_ids().is_empty());
        assert!(!dir.path().join(".local/tasks").exists());
    }

    #[test]
    fn import_rejects_existing_task_document_collision() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "PROJ-123",
            "title = \"Local edits\"\nbranch = \"local-edits\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::with_issues(vec![issue(
            "PROJ-123",
            "Provider title",
            Some("alice/proj-123-provider-title"),
            Some("Provider body"),
        )]);

        let err = import_issue_task_documents(&ctx, &["PROJ-123".into()], "linear", &provider)
            .unwrap_err();

        assert!(err.to_string().contains("TaskDocument already exists"));
        let content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(content.contains("title = \"Local edits\""));
        assert!(!content.contains("Provider title"));
    }

    #[test]
    fn import_preflights_later_collision_before_writing_any_tasks() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "PROJ-2",
            "title = \"Existing\"\nbranch = \"existing\"\n",
        );
        let ctx = ctx_with_config(dir.path(), linear_config());
        let provider = FakeIssueProvider::with_issues(vec![
            issue(
                "PROJ-1",
                "First",
                Some("alice/proj-1-first"),
                Some("First body"),
            ),
            issue(
                "PROJ-2",
                "Second",
                Some("alice/proj-2-second"),
                Some("Second body"),
            ),
        ]);

        let err = import_issue_task_documents(
            &ctx,
            &["PROJ-1".into(), "PROJ-2".into()],
            "linear",
            &provider,
        )
        .unwrap_err();

        assert!(err.to_string().contains(".local/tasks/PROJ-2.toml"));
        assert!(!dir.path().join(".local/tasks/PROJ-1.toml").exists());
        let content = std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-2.toml")).unwrap();
        assert!(content.contains("title = \"Existing\""));
    }

    #[test]
    fn bare_import_selects_provider_issues() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_multi_select(vec![1]);
        let ui = Arc::new(ui);
        let ctx = ctx_with_config_and_ui(dir.path(), linear_config(), Arc::clone(&ui));
        let provider = FakeIssueProvider::with_issues(vec![
            issue("PROJ-1", "Fix A", Some("alice/proj-1-fix-a"), None),
            issue("PROJ-2", "Fix B", Some("alice/proj-2-fix-b"), None),
        ])
        .with_list(vec![
            list_item("PROJ-1", "Fix A", "Todo"),
            list_item("PROJ-2", "Fix B", "Ready"),
        ]);

        let selected = resolve_import_issue_ids(&ctx, &[], &provider).unwrap();
        let imported = import_issue_task_documents(&ctx, &selected, "linear", &provider).unwrap();

        assert_eq!(selected, vec!["PROJ-2".to_string()]);
        assert_eq!(imported[0].key, "PROJ-2");
        assert!(dir.path().join(".local/tasks/PROJ-2.toml").exists());
        assert!(!dir.path().join(".local/tasks/PROJ-1.toml").exists());
        assert_eq!(
            ui.multi_select_items.lock().unwrap().as_slice(),
            [vec![
                "Fix A  PROJ-1 | Todo".to_string(),
                "Fix B  PROJ-2 | Ready".to_string(),
            ]]
        );
    }

    #[test]
    fn prepare_issue_tasks_rejects_existing_task_document_collision() {
        let dir = tempfile::tempdir().unwrap();
        write_task(
            dir.path(),
            "PROJ-123",
            "title = \"Local edits\"\nbranch = \"local-edits\"\n",
        );
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Provider title","branchName":"alice/proj-123-provider-title","description":"Provider body"}"#,
            true,
        );
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            linear_config(),
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let err = prepare_issue_tasks(&ctx, &["PROJ-123".into()]).unwrap_err();

        assert!(err.to_string().contains("TaskDocument already exists"));
        let content =
            std::fs::read_to_string(dir.path().join(".local/tasks/PROJ-123.toml")).unwrap();
        assert!(content.contains("title = \"Local edits\""));
        assert!(!content.contains("Provider title"));
    }
}
