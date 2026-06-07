use crate::context::{Ctx, PromptItem, PromptRow};
use crate::storage::StorageRoot;
use crate::task_run;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskDocument {
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) branch: String,
    #[serde(default)]
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) origin: Option<TaskOrigin>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskOrigin {
    pub(crate) provider: String,
    pub(crate) id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedTask {
    pub(crate) key: String,
    pub(crate) branch: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectedTask {
    pub(crate) key: String,
    pub(crate) path: String,
    pub(crate) content: String,
    pub(crate) document: TaskDocument,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaskDocumentDisplay {
    label: String,
    origin_state: String,
    task_key: String,
    branch: Option<String>,
}

impl TaskDocumentDisplay {
    pub(crate) fn for_document(key: &str, document: &TaskDocument) -> Self {
        Self::for_status(key, document, &task_origin_status(document))
    }

    fn for_status(key: &str, document: &TaskDocument, status: &str) -> Self {
        let title = document.title_or_key(key);
        let label = title.trim();
        let task_key = key.trim();
        let label = if label.is_empty() { task_key } else { label };

        Self {
            label: label.to_string(),
            origin_state: task_origin_state_label(status),
            task_key: task_key.to_string(),
            branch: prepared_branch_name(&document.branch).map(str::to_string),
        }
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn selector_hint_parts(&self) -> Vec<String> {
        let mut hint_parts = Vec::new();
        if self.origin_state != "not published" && !self.origin_state.is_empty() {
            hint_parts.push(self.origin_state.clone());
        }
        if let Some(branch) = &self.branch {
            hint_parts.push(format!("branch {branch}"));
        } else if !self.task_key.is_empty() {
            hint_parts.push(format!("task {}", self.task_key));
        }
        hint_parts
    }
}

impl TaskDocument {
    pub(crate) fn empty(slug: &str) -> Self {
        Self {
            title: format!("작업: {slug}"),
            branch: slug.to_string(),
            body: "## 계획 (Planning)\n\n\
- 유형 (type): AFK\n\
- 예상 소요 (expected duration): \n\
- 예상 근거 (estimate basis): conservative planning guess\n\
- 권장 watch cadence (suggested watch cadence): launch 45s, steady heartbeat 5-10m\n\
- 막힘 / 의존성 (blocked by): none\n\
- 실행 형태 (execution shape): direct\n\
- 크기 (size class): small\n\
- 확인 방법 (acceptance checks): \n\n\
## 맥락\n\n\
- \n\n\
## 완료 기준\n\n\
- \n"
                .to_string(),
            origin: None,
        }
    }

    pub(crate) fn title_or_key(&self, key: &str) -> String {
        if self.title.trim().is_empty() {
            key.to_string()
        } else {
            self.title.clone()
        }
    }

    pub(crate) fn setup_mode(&self) -> &'static str {
        if self.origin.is_some() {
            "issue"
        } else {
            "branch"
        }
    }

    pub(crate) fn identifier_or_key(&self, key: &str) -> String {
        self.origin
            .as_ref()
            .map(|origin| origin.id.clone())
            .unwrap_or_else(|| key.to_string())
    }
}

#[cfg(test)]
pub(crate) fn select_local_task(ctx: &Ctx) -> Result<SelectedTask> {
    let tasks = list_local_tasks(ctx)?;
    if tasks.is_empty() {
        bail!("No task files found in <repo-root>/.wt/execution/tasks");
    }

    let rows = task_selection_rows(&tasks);
    let idx = ctx.ui.select_rows("Task to start", &rows)?;
    let task = tasks
        .get(idx)
        .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {idx}"))?;
    Ok(task.clone())
}

pub(crate) fn select_local_tasks(ctx: &Ctx) -> Result<Vec<SelectedTask>> {
    let tasks = list_local_tasks(ctx)?;
    if tasks.is_empty() {
        bail!("No task files found in <repo-root>/.wt/execution/tasks");
    }

    let rows = task_selection_rows(&tasks);
    let selections = ctx.ui.multi_select_rows("Tasks to start", &rows)?;
    let mut selected = Vec::new();
    for idx in selections {
        let task = tasks
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {idx}"))?;
        selected.push(task.clone());
    }
    Ok(selected)
}

pub(crate) fn select_local_task_documents(ctx: &Ctx) -> Result<Vec<SelectedTask>> {
    let tasks = list_local_task_documents(ctx)?;
    if tasks.is_empty() {
        bail!("No task files found in <repo-root>/.wt/execution/tasks");
    }

    let rows = task_selection_rows(&tasks);
    let selections = ctx.ui.multi_select_rows("Tasks", &rows)?;
    let mut selected = Vec::new();
    for idx in selections {
        let task = tasks
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("Selected task index out of range: {idx}"))?;
        selected.push(task.clone());
    }
    Ok(selected)
}

pub(crate) fn select_local_task_by_key(ctx: &Ctx, key: &str) -> Result<SelectedTask> {
    let key = safe_task_key(key);
    let (document, path, content) = read_task_file(ctx, &key)?;
    Ok(SelectedTask {
        key,
        path,
        content,
        document,
    })
}

pub(crate) fn list_local_tasks(ctx: &Ctx) -> Result<Vec<SelectedTask>> {
    let mut tasks = Vec::new();
    for task in list_local_task_documents(ctx)? {
        if task_run::task_is_selectable(ctx, &task.key)? {
            tasks.push(task);
        }
    }
    Ok(tasks)
}

pub(crate) fn list_local_task_documents(ctx: &Ctx) -> Result<Vec<SelectedTask>> {
    list_task_documents(&ctx.storage_root, &ctx.repo_root)
}

pub(crate) fn list_task_documents(
    storage_root: &StorageRoot,
    repo_root: &Path,
) -> Result<Vec<SelectedTask>> {
    let mut tasks = Vec::new();
    for path in task_document_paths_for(storage_root, repo_root)? {
        tasks.push(read_task_document_path_from_store(storage_root, &path)?);
    }
    Ok(tasks)
}

pub(crate) fn task_document_paths(ctx: &Ctx) -> Result<Vec<PathBuf>> {
    task_document_paths_for(&ctx.storage_root, &ctx.repo_root)
}

pub(crate) fn task_document_paths_for(
    storage_root: &StorageRoot,
    repo_root: &Path,
) -> Result<Vec<PathBuf>> {
    ensure_task_document_store_available(storage_root, repo_root)?;
    let tasks_dir = storage_root.tasks_dir();
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(&tasks_dir).with_context(|| {
        format!(
            "Failed to read task directory: {}",
            storage_root.display_path(&tasks_dir)
        )
    })? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn task_key_from_path(path: &Path) -> Result<String> {
    let relative_path = path.to_string_lossy();
    Ok(safe_task_key(
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("Task file is missing a key: {relative_path}"))?,
    ))
}

pub(crate) fn read_task_document_path(ctx: &Ctx, path: &Path) -> Result<SelectedTask> {
    read_task_document_path_from_store(&ctx.storage_root, path)
}

pub(crate) fn read_task_document_path_from_store(
    storage_root: &StorageRoot,
    path: &Path,
) -> Result<SelectedTask> {
    read_selected_task(storage_root, path.to_path_buf())
}

pub(crate) fn read_task_document(ctx: &Ctx, key: &str) -> Result<TaskDocument> {
    ensure_task_document_store_available(&ctx.storage_root, &ctx.repo_root)?;
    let path = task_path_for(&ctx.storage_root, key);
    let display_path = ctx.storage_root.display_path(&path);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read task: {display_path}"))?;
    let task: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {display_path}"))?;
    Ok(task)
}

pub(crate) fn read_task_file(ctx: &Ctx, key: &str) -> Result<(TaskDocument, String, String)> {
    read_task_file_from_store(&ctx.storage_root, &ctx.repo_root, key)
}

pub(crate) fn read_task_file_from_store(
    storage_root: &StorageRoot,
    repo_root: &Path,
    key: &str,
) -> Result<(TaskDocument, String, String)> {
    ensure_task_document_store_available(storage_root, repo_root)?;
    let absolute_path = task_path_for(storage_root, key);
    let path = storage_root.display_path(&absolute_path);
    let content = fs::read_to_string(&absolute_path)
        .with_context(|| format!("Failed to read task: {path}"))?;
    let task: TaskDocument =
        toml::from_str(&content).with_context(|| format!("Failed to parse task: {path}"))?;
    Ok((task, path, content))
}

pub(crate) fn write_task_document(ctx: &Ctx, key: &str, task: &TaskDocument) -> Result<()> {
    write_task_document_content(ctx, key, &render_task_document(task))
}

pub(crate) fn write_task_document_content(ctx: &Ctx, key: &str, content: &str) -> Result<()> {
    write_task_document_content_to_store(&ctx.storage_root, &ctx.repo_root, key, content)
}

pub(crate) fn write_task_document_content_to_store(
    storage_root: &StorageRoot,
    repo_root: &Path,
    key: &str,
    content: &str,
) -> Result<()> {
    ensure_task_document_store_available(storage_root, repo_root)?;
    let tasks_dir = storage_root.tasks_dir();
    fs::create_dir_all(&tasks_dir)?;
    write_task_document_atomically(&tasks_dir, &task_path_for(storage_root, key), content)
}

pub(crate) fn write_new_task_document(ctx: &Ctx, key: &str, task: &TaskDocument) -> Result<()> {
    ensure_task_document_store_available(&ctx.storage_root, &ctx.repo_root)?;
    let tasks_dir = ctx.storage_root.tasks_dir();
    fs::create_dir_all(&tasks_dir)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(task_path_for(&ctx.storage_root, key))
        .with_context(|| format!("Task already exists: {}", task_relative_path(key)))?;
    file.write_all(render_task_document(task).as_bytes())?;
    Ok(())
}

pub(crate) fn write_task_branch(ctx: &Ctx, key: &str, branch: &str) -> Result<()> {
    let mut task = read_task_document(ctx, key)?;
    task.branch = branch.to_string();
    write_task_document(ctx, key, &task)
}

pub(crate) fn task_relative_path(key: &str) -> String {
    format!(
        "<repo-root>/.wt/execution/tasks/{}.toml",
        safe_task_key(key)
    )
}

pub(crate) fn prepared_branch_name(branch: &str) -> Option<&str> {
    let branch = branch.trim();
    if branch.is_empty() || branch == "-" {
        None
    } else {
        Some(branch)
    }
}

pub(crate) fn task_exists(ctx: &Ctx, key: &str) -> Result<bool> {
    ensure_task_document_store_available(&ctx.storage_root, &ctx.repo_root)?;
    Ok(task_path_for(&ctx.storage_root, key).exists())
}

pub(crate) fn task_path_for(storage_root: &StorageRoot, key: &str) -> PathBuf {
    storage_root
        .tasks_dir()
        .join(format!("{}.toml", safe_task_key(key)))
}

pub(crate) fn ensure_task_document_store_available(
    storage_root: &StorageRoot,
    repo_root: &Path,
) -> Result<()> {
    if let Some(legacy) = storage_root.detect_legacy_tasks(repo_root) {
        bail!(
            "Found legacy TaskDocument storage at {}. Canonical TaskDocument storage is {}. wt does not silently read legacy task storage; import or repair legacy state explicitly before using this command.",
            legacy.path().display(),
            storage_root.display_path(legacy.canonical_root())
        );
    }
    Ok(())
}

fn write_task_document_atomically(
    tasks_dir: &Path,
    final_path: &Path,
    content: &str,
) -> Result<()> {
    let existing_permissions = match fs::metadata(final_path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => {
            return Err(err).with_context(|| {
                format!("Failed to stat task document: {}", final_path.display())
            });
        }
    };
    let (temp_path, mut file) = create_task_temp_file(tasks_dir, final_path)?;
    let result = (|| -> Result<()> {
        if let Some(permissions) = existing_permissions {
            fs::set_permissions(&temp_path, permissions).with_context(|| {
                format!(
                    "Failed to set temporary task document permissions: {}",
                    temp_path.display()
                )
            })?;
        }
        file.write_all(content.as_bytes()).with_context(|| {
            format!(
                "Failed to write temporary task document: {}",
                temp_path.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "Failed to sync temporary task document: {}",
                temp_path.display()
            )
        })?;
        drop(file);
        fs::rename(&temp_path, final_path).with_context(|| {
            format!("Failed to replace task document: {}", final_path.display())
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn create_task_temp_file(tasks_dir: &Path, _final_path: &Path) -> Result<(PathBuf, fs::File)> {
    let pid = std::process::id();

    for attempt in 0..100 {
        let temp_path = tasks_dir.join(format!(".wt-task-{pid}-{attempt}.tmp"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "Failed to create temporary task document: {}",
                        temp_path.display()
                    )
                });
            }
        }
    }

    bail!(
        "Failed to allocate temporary task document path in {}",
        tasks_dir.display()
    )
}

fn read_selected_task(storage_root: &StorageRoot, path: PathBuf) -> Result<SelectedTask> {
    let relative_path = storage_root.display_path(&path);
    let key = task_key_from_path(&path)?;
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read task: {relative_path}"))?;
    let document: TaskDocument = toml::from_str(&content)
        .with_context(|| format!("Failed to parse task: {relative_path}"))?;
    Ok(SelectedTask {
        key,
        path: relative_path,
        content,
        document,
    })
}

#[cfg(test)]
fn task_selection_label(task: &SelectedTask) -> String {
    task_selection_item(task).render_plain()
}

pub(crate) fn task_selection_item(task: &SelectedTask) -> PromptItem {
    task_resource_item(
        &task.key,
        &task.document,
        &task_origin_status(&task.document),
    )
}

fn task_selection_rows(tasks: &[SelectedTask]) -> Vec<PromptRow> {
    let mut rows = Vec::new();
    let mut provider_groups = Vec::<String>::new();
    let mut has_local_group = false;
    let candidates = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let group = task_selection_group(task);
            if group == "Local" {
                has_local_group = true;
            } else if !provider_groups.contains(&group) {
                provider_groups.push(group.clone());
            }
            (index, group, task_selection_item(task))
        })
        .collect::<Vec<_>>();

    let mut groups = provider_groups;
    if has_local_group {
        groups.push("Local".to_string());
    }

    for group in groups {
        rows.push(PromptRow::section(group.clone()));
        for (index, _, item) in candidates
            .iter()
            .filter(|(_, candidate_group, _)| candidate_group == &group)
        {
            rows.push(PromptRow::from_indexed_item(*index, item.clone()));
        }
    }
    rows
}

fn task_selection_group(task: &SelectedTask) -> String {
    task.document
        .origin
        .as_ref()
        .map(|origin| provider_display_label(&origin.provider))
        .unwrap_or_else(|| "Local".to_string())
}

pub(crate) fn task_resource_item(key: &str, document: &TaskDocument, status: &str) -> PromptItem {
    let display = TaskDocumentDisplay::for_status(key, document, status);
    PromptItem::from_hint_parts(display.label().to_string(), display.selector_hint_parts())
}

fn task_origin_state_label(status: &str) -> String {
    let status = status.trim();
    if status.is_empty() {
        return String::new();
    }

    if status == "origin:none" {
        return "not published".into();
    }

    if let Some(origin) = status.strip_prefix("origin:") {
        let mut parts = origin.splitn(2, ':');
        let provider = parts.next().unwrap_or_default();
        let id = parts.next().unwrap_or_default();
        let provider = provider_display_label(provider);
        if id.trim().is_empty() {
            return provider;
        }
        return format!("{provider} {id}");
    }

    status.replace(':', " ")
}

fn provider_display_label(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "github" => "GitHub".into(),
        "linear" => "Linear".into(),
        "" => "external".into(),
        other => other.to_string(),
    }
}

fn task_origin_status(document: &TaskDocument) -> String {
    document
        .origin
        .as_ref()
        .map(|origin| format!("origin:{}:{}", origin.provider, origin.id))
        .unwrap_or_else(|| "origin:none".into())
}

pub(crate) fn safe_task_key(value: &str) -> String {
    let key = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    if key.is_empty() { "task".into() } else { key }
}

pub(crate) fn workspace_run_label(idx: usize, total: usize, identifier: Option<&str>) -> String {
    let total = total.max(1);
    let mut label = format!("{}/{}", idx + 1, total);
    if let Some(identifier) = identifier
        .map(str::trim)
        .filter(|identifier| !identifier.is_empty())
    {
        label.push(' ');
        label.push_str(identifier);
    }
    label
}

pub(crate) fn render_task_document(task: &TaskDocument) -> String {
    let mut content = String::new();
    content.push_str(&format!("title = {}\n", toml_quote(&task.title)));
    if !task.branch.trim().is_empty() {
        content.push_str(&format!("branch = {}\n", toml_quote(&task.branch)));
    }
    if !task.body.trim().is_empty() {
        content.push_str(&format!("body = {}\n", toml_multiline_string(&task.body)));
    }
    if let Some(origin) = &task.origin {
        content.push_str("\n[origin]\n");
        content.push_str(&format!("provider = {}\n", toml_quote(&origin.provider)));
        content.push_str(&format!("id = {}\n", toml_quote(&origin.id)));
    }
    content
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

fn toml_multiline_string(value: &str) -> String {
    if value.starts_with(['\n', '\r']) {
        return toml_quote(value);
    }
    let escaped = value
        .replace("\\", "\\\\")
        .replace("\"\"\"", "\\\"\\\"\\\"");
    format!("\"\"\"{escaped}\"\"\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};

    #[test]
    fn safe_task_key_replaces_unsafe_chars() {
        assert_eq!(safe_task_key("#42"), "42");
        assert_eq!(safe_task_key("PROJ-123"), "PROJ-123");
        assert_eq!(safe_task_key("bad/value"), "bad-value");
    }

    #[test]
    fn workspace_run_label_keeps_order_and_identifier_short() {
        assert_eq!(workspace_run_label(1, 5, Some("PROJ-123")), "2/5 PROJ-123");
        assert_eq!(workspace_run_label(0, 3, None), "1/3");
    }

    #[test]
    fn task_selection_label_keeps_title_origin_and_branch_separate() {
        let task = SelectedTask {
            key: "PROJ-123".into(),
            path: "<repo-root>/.wt/execution/tasks/PROJ-123.toml".into(),
            content: String::new(),
            document: TaskDocument {
                title: "Fix editor".into(),
                branch: "alice/proj-123-fix-editor".into(),
                body: String::new(),
                origin: Some(TaskOrigin {
                    provider: "linear".into(),
                    id: "PROJ-123".into(),
                }),
            },
        };

        assert_eq!(
            task_selection_label(&task),
            "Fix editor  Linear PROJ-123 | branch alice/proj-123-fix-editor"
        );
    }

    #[test]
    fn task_selection_label_omits_redundant_local_origin() {
        let task = SelectedTask {
            key: "local-task".into(),
            path: "<repo-root>/.wt/execution/tasks/local-task.toml".into(),
            content: String::new(),
            document: TaskDocument {
                title: String::new(),
                branch: "local-task".into(),
                body: String::new(),
                origin: None,
            },
        };

        assert_eq!(task_selection_label(&task), "local-task  branch local-task");
    }

    #[test]
    fn empty_task_document_includes_timing_and_watch_planning_fields() {
        let task = TaskDocument::empty("foo");

        assert!(task.body.contains("## 계획 (Planning)"));
        assert!(task.body.contains("예상 소요 (expected duration)"));
        assert!(task.body.contains("예상 근거 (estimate basis)"));
        assert!(
            task.body
                .contains("권장 watch cadence (suggested watch cadence)")
        );
    }

    #[test]
    fn task_body_keeps_planning_spec_paths_opaque() {
        let body = "## 맥락\n\
근거: planning/specs/old-spec/03-Architect/05-design.md 참조\n";
        let task = TaskDocument {
            title: "demo".into(),
            branch: "demo".into(),
            body: body.into(),
            origin: None,
        };

        assert!(task.body.contains("planning/specs/old-spec"));
    }

    #[test]
    fn task_selection_rows_group_provider_tasks_before_local_tasks() {
        let tasks = vec![
            selected_task("local-a", "Local A", "local-a", None),
            selected_task(
                "PROJ-123",
                "Provider task",
                "alice/proj-123-provider-task",
                Some(TaskOrigin {
                    provider: "linear".into(),
                    id: "PROJ-123".into(),
                }),
            ),
            selected_task("local-b", "Local B", "local-b", None),
        ];

        let rows = task_selection_rows(&tasks);

        assert_eq!(
            selector_row_summary(&rows),
            vec![
                "section:Linear",
                "option:1:Provider task",
                "section:Local",
                "option:0:Local A",
                "option:2:Local B",
            ]
        );
    }

    #[test]
    fn select_local_task_errors_when_no_task_files_exist() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = select_local_task(&ctx);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No task files found in <repo-root>/.wt/execution/tasks")
        );
    }

    #[test]
    fn select_local_task_reads_selected_task_document() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("a-first.toml"),
            "title = \"First\"\nbranch = \"first\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("b-second.toml"),
            "title = \"Second\"\nbranch = \"second\"\nbody = \"details\"\n",
        )
        .unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        let selected = select_local_task(&ctx).unwrap();

        assert_eq!(selected.key, "b-second");
        assert_eq!(
            selected.path,
            "<repo-root>/.wt/execution/tasks/b-second.toml"
        );
        assert_eq!(selected.document.title, "Second");
        assert_eq!(selected.document.branch, "second");
        assert!(selected.content.contains("body = \"details\""));
    }

    #[test]
    fn list_local_tasks_normalizes_discovered_filename_key() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("ISSUE#42!.toml"),
            "title = \"Unsafe\"\nbranch = \"unsafe\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let tasks = list_local_tasks(&ctx).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].key, "ISSUE-42");
        assert_eq!(
            tasks[0].path,
            "<repo-root>/.wt/execution/tasks/ISSUE#42!.toml"
        );
    }

    #[test]
    fn write_task_document_replaces_existing_task_without_leaving_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let final_path = tasks_dir.join("replace-me.toml");
        std::fs::write(&final_path, "title = \"Old\"\nbranch = \"old\"\n").unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let task = TaskDocument {
            title: "New".into(),
            branch: "replace-me".into(),
            body: "details".into(),
            origin: None,
        };

        write_task_document(&ctx, "replace/me", &task).unwrap();

        let content = std::fs::read_to_string(final_path).unwrap();
        assert_eq!(
            content,
            "title = \"New\"\nbranch = \"replace-me\"\nbody = \"\"\"details\"\"\"\n"
        );
        let temp_files = std::fs::read_dir(tasks_dir)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .ok()
                    .and_then(|entry| entry.path().extension().map(|ext| ext == "tmp"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(temp_files, 0);
    }

    #[test]
    fn write_task_document_uses_bounded_temp_filename_for_long_task_keys() {
        let dir = tempfile::tempdir().unwrap();
        let key = "a".repeat(240);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let task = TaskDocument {
            title: "Long".into(),
            branch: key.clone(),
            body: "details".into(),
            origin: None,
        };

        write_task_document(&ctx, &key, &task).unwrap();

        let final_path = dir.path().join(format!(".wt/execution/tasks/{key}.toml"));
        assert!(final_path.exists());
    }

    #[test]
    fn task_store_rejects_legacy_local_tasks_without_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_tasks_dir = dir.path().join(".local/tasks");
        std::fs::create_dir_all(&legacy_tasks_dir).unwrap();
        std::fs::write(
            legacy_tasks_dir.join("legacy.toml"),
            "title = \"Legacy\"\nbranch = \"legacy\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let err = list_local_task_documents(&ctx).unwrap_err().to_string();

        assert!(err.contains("Found legacy TaskDocument storage"));
        assert!(err.contains(".local/tasks"));
        assert!(err.contains("<repo-root>/.wt/execution/tasks"));
    }

    #[test]
    fn task_store_rejects_legacy_git_common_tasks_without_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_tasks_dir = dir.path().join(".git/wt/tasks");
        std::fs::create_dir_all(&legacy_tasks_dir).unwrap();
        std::fs::write(
            legacy_tasks_dir.join("foo.toml"),
            "title = \"Legacy\"\nbranch = \"legacy\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let err = list_local_task_documents(&ctx).unwrap_err().to_string();

        assert!(err.contains("Found legacy TaskDocument storage"));
        assert!(err.contains(".git/wt/tasks"));
        assert!(err.contains("<repo-root>/.wt/execution/tasks"));
        assert!(!err.contains("No task files found"));
    }

    #[cfg(unix)]
    #[test]
    fn write_task_document_preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let final_path = tasks_dir.join("restricted.toml");
        std::fs::write(&final_path, "title = \"Old\"\nbranch = \"old\"\n").unwrap();
        std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        let task = TaskDocument {
            title: "New".into(),
            branch: "restricted".into(),
            body: "details".into(),
            origin: None,
        };

        write_task_document(&ctx, "restricted", &task).unwrap();

        let mode = std::fs::metadata(final_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn list_local_tasks_omits_tasks_with_passed_runs() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("a-first.toml"),
            "title = \"First\"\nbranch = \"first\"\n",
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("b-second.toml"),
            "title = \"Second\"\nbranch = \"second\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        task_run::create(&ctx, "a-first", "first", None, task_run::STATUS_PASSED).unwrap();

        let tasks = list_local_tasks(&ctx).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].key, "b-second");
    }

    #[test]
    fn list_local_tasks_keeps_skipped_runs_selectable() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("a-first.toml"),
            "title = \"First\"\nbranch = \"first\"\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        task_run::create(&ctx, "a-first", "first", None, task_run::STATUS_SKIPPED).unwrap();

        let tasks = list_local_tasks(&ctx).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].key, "a-first");
    }

    #[test]
    fn list_local_tasks_rejects_unknown_task_fields() {
        let dir = tempfile::tempdir().unwrap();
        let tasks_dir = dir.path().join(".wt/execution/tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("bad.toml"),
            "title = \"Bad\"\nbranch = \"bad\"\nextra = true\n",
        )
        .unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = list_local_tasks(&ctx);

        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("unknown field"));
    }

    fn selected_task(
        key: &str,
        title: &str,
        branch: &str,
        origin: Option<TaskOrigin>,
    ) -> SelectedTask {
        SelectedTask {
            key: key.into(),
            path: format!("<repo-root>/.wt/execution/tasks/{key}.toml"),
            content: String::new(),
            document: TaskDocument {
                title: title.into(),
                branch: branch.into(),
                body: String::new(),
                origin,
            },
        }
    }

    fn selector_row_summary(rows: &[PromptRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                PromptRow::Section(section) => format!("section:{}", section.title),
                PromptRow::Option(option) => format!(
                    "option:{}:{}",
                    option.value_index.unwrap_or(usize::MAX),
                    option.label
                ),
            })
            .collect()
    }
}
