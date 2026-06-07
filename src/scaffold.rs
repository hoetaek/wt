use crate::storage::StorageRoot;
use crate::task::{TaskDocument, render_task_document};
use crate::workflow::{WorkflowMetadata, render_workflow_metadata};
use std::path::PathBuf;

pub const ALL_DOC_KINDS: [DocKind; 2] = [DocKind::Task, DocKind::Workflow];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Task,
    Workflow,
}

impl DocKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Workflow => "workflow",
        }
    }

    pub fn legacy_state_name(self) -> &'static str {
        match self {
            Self::Task => "TaskDocument storage",
            Self::Workflow => "Workflow storage",
        }
    }

    pub fn paths(self, storage: &StorageRoot, slug: &str) -> Vec<PathBuf> {
        match self {
            Self::Task => vec![storage.tasks_dir().join(format!("{slug}.toml"))],
            Self::Workflow => vec![storage.workflows_dir().join(format!("{slug}.toml"))],
        }
    }

    pub fn render(self, slug: &str) -> Vec<(PathBuf, String)> {
        match self {
            Self::Task => vec![(
                PathBuf::from(format!("execution/tasks/{slug}.toml")),
                render_task_document(&TaskDocument::empty(slug)),
            )],
            Self::Workflow => vec![(
                PathBuf::from(format!("execution/workflows/{slug}.toml")),
                render_workflow_metadata(&WorkflowMetadata::empty(slug)),
            )],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_return_expected_storage_locations() {
        let dir = tempfile::tempdir().unwrap();
        let storage = StorageRoot::from_git_common_dir(dir.path().join(".git"));

        assert_eq!(
            DocKind::Task.paths(&storage, "foo"),
            vec![dir.path().join(".wt/execution/tasks/foo.toml")]
        );
        assert_eq!(
            DocKind::Workflow.paths(&storage, "foo"),
            vec![dir.path().join(".wt/execution/workflows/foo.toml")]
        );
    }

    #[test]
    fn task_and_workflow_render_reuse_toml_renderers() {
        let task = DocKind::Task.render("foo");
        assert_eq!(task.len(), 1);
        assert!(task[0].1.contains("title = \"작업: foo\""));
        assert!(task[0].1.contains("branch = \"foo\""));
        assert!(task[0].1.contains("## 계획 (Planning)"));
        assert!(task[0].1.contains("예상 근거 (estimate basis)"));
        assert!(
            task[0]
                .1
                .contains("권장 watch cadence (suggested watch cadence)")
        );

        let workflow = DocKind::Workflow.render("foo");
        assert_eq!(workflow.len(), 1);
        assert!(workflow[0].1.contains("title = \"워크플로우: foo\""));
        assert!(workflow[0].1.contains("## 목적"));
        assert!(workflow[0].1.contains("mode = \"single\""));
    }
}
