use crate::storage::StorageRoot;
use crate::task::{TaskDocument, render_task_document};
use crate::workflow::{WorkflowMetadata, render_workflow_metadata};
use std::path::PathBuf;

pub const ALL_DOC_KINDS: [DocKind; 5] = [
    DocKind::Idea,
    DocKind::Spec,
    DocKind::Task,
    DocKind::Workflow,
    DocKind::Retrospect,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind {
    Idea,
    Spec,
    Task,
    Workflow,
    Retrospect,
}

impl DocKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idea => "idea",
            Self::Spec => "spec",
            Self::Task => "task",
            Self::Workflow => "workflow",
            Self::Retrospect => "retrospect",
        }
    }

    pub fn paths(self, storage: &StorageRoot, slug: &str) -> Vec<PathBuf> {
        match self {
            Self::Idea => vec![storage.ideas_dir().join(format!("{slug}.md"))],
            Self::Spec => {
                let dir = storage.specs_dir().join(slug);
                vec![
                    dir.join("requirements.md"),
                    dir.join("design.md"),
                    dir.join("tasks.md"),
                ]
            }
            Self::Task => vec![storage.tasks_dir().join(format!("{slug}.toml"))],
            Self::Workflow => vec![storage.workflows_dir().join(format!("{slug}.toml"))],
            Self::Retrospect => vec![storage.retrospectives_dir().join(format!("{slug}.md"))],
        }
    }

    pub fn render(self, slug: &str) -> Vec<(PathBuf, String)> {
        match self {
            Self::Idea => vec![(PathBuf::from(format!("ideas/{slug}.md")), render_idea(slug))],
            Self::Spec => vec![
                (
                    PathBuf::from(format!("specs/{slug}/requirements.md")),
                    render_spec_requirements(),
                ),
                (
                    PathBuf::from(format!("specs/{slug}/design.md")),
                    render_spec_design(),
                ),
                (
                    PathBuf::from(format!("specs/{slug}/tasks.md")),
                    render_spec_tasks(),
                ),
            ],
            Self::Task => vec![(
                PathBuf::from(format!("tasks/{slug}.toml")),
                render_task_document(&TaskDocument::empty(slug)),
            )],
            Self::Workflow => vec![(
                PathBuf::from(format!("workflows/{slug}.toml")),
                render_workflow_metadata(&WorkflowMetadata::empty(slug)),
            )],
            Self::Retrospect => vec![(
                PathBuf::from(format!("retrospectives/{slug}.md")),
                render_retrospect(slug),
            )],
        }
    }
}

fn render_idea(slug: &str) -> String {
    format!(
        "# {slug}\n\n\
## Raw intent\n\
- \n\n\
## Outcome / problem\n\
- \n\n\
## Evidence\n\
- Local: \n\
- External: \n\n\
## Options\n\
- Option A: \n\
- Option B: \n\n\
## Tradeoffs\n\
- \n\n\
## Risks / rabbit holes\n\
- \n\n\
## Non-goals\n\
- \n\n\
## Open questions\n\
- \n\n\
## Next step\n\
- \n"
    )
}

fn render_spec_requirements() -> String {
    "As a [role], I want [feature] so that [benefit]\n\n\
## Functional requirements (EARS)\n\n\
- WHEN <condition> THE SYSTEM SHALL <behavior>\n\
- GIVEN <precondition> WHEN <trigger> THE SYSTEM SHALL <response>\n\n\
## Non-functional\n\n\
- \n\n\
## Regression-sensitive\n\n\
- WHEN <condition> THE SYSTEM SHALL CONTINUE TO <preserved behavior>\n"
        .to_string()
}

fn render_spec_design() -> String {
    "## Decisions\n\n\
- \n\n\
## Affected components\n\n\
- \n\n\
## Constraints\n\n\
- \n\n\
## Diagrams\n\n\
```text\n\
[ASCII diagram placeholder]\n\
```\n"
        .to_string()
}

fn render_spec_tasks() -> String {
    "## Tasks\n\n\
Sequence atomic units of work. Mark dependencies (`[blocked by: T1]`) or parallel groups (`[parallel: T2, T3]`) explicitly so the execution shape can be derived.\n\n\
- [ ] T1 — <short title>\n\
- [ ] T2 — <short title>  [blocked by: T1]\n\
- [ ] T3 — <short title>  [parallel: T2]\n"
        .to_string()
}

fn render_retrospect(slug: &str) -> String {
    format!(
        "# {slug}\n\n\
## What worked\n\
- \n\n\
## What didn't\n\
- \n\n\
## Surprises\n\
- \n\n\
## Decisions for next time\n\
- \n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_return_expected_storage_locations() {
        let dir = tempfile::tempdir().unwrap();
        let storage = StorageRoot::from_git_common_dir(dir.path().join(".git"));

        assert_eq!(
            DocKind::Idea.paths(&storage, "foo"),
            vec![dir.path().join(".git/wt/ideas/foo.md")]
        );
        assert_eq!(
            DocKind::Spec.paths(&storage, "foo"),
            vec![
                dir.path().join(".git/wt/specs/foo/requirements.md"),
                dir.path().join(".git/wt/specs/foo/design.md"),
                dir.path().join(".git/wt/specs/foo/tasks.md")
            ]
        );
        assert_eq!(
            DocKind::Task.paths(&storage, "foo"),
            vec![dir.path().join(".git/wt/tasks/foo.toml")]
        );
        assert_eq!(
            DocKind::Workflow.paths(&storage, "foo"),
            vec![dir.path().join(".git/wt/workflows/foo.toml")]
        );
        assert_eq!(
            DocKind::Retrospect.paths(&storage, "foo"),
            vec![dir.path().join(".git/wt/retrospectives/foo.md")]
        );
    }

    #[test]
    fn task_and_workflow_render_reuse_toml_renderers() {
        let task = DocKind::Task.render("foo");
        assert_eq!(task.len(), 1);
        assert!(task[0].1.contains("title = \"foo\""));
        assert!(task[0].1.contains("branch = \"foo\""));

        let workflow = DocKind::Workflow.render("foo");
        assert_eq!(workflow.len(), 1);
        assert!(workflow[0].1.contains("mode = \"single\""));
    }

    #[test]
    fn spec_render_includes_tasks_skeleton() {
        let spec = DocKind::Spec.render("foo");
        assert_eq!(spec.len(), 3);
        assert_eq!(spec[2].0, PathBuf::from("specs/foo/tasks.md"));
        assert!(spec[2].1.contains("## Tasks"));
        assert!(spec[2].1.contains("[blocked by:"));
    }
}
