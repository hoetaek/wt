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
## 원문 의도\n\
- \n\n\
## 맥락 / 레퍼런스 탐색\n\
- 로컬: \n\
- 외부: \n\
- 참고한 방향: \n\n\
## 목적 / 성공 기준\n\
- \n\n\
## 선택지\n\
- 선택지 A: \n\
- 선택지 B: \n\n\
## 트레이드오프\n\
- \n\n\
## 리스크 / 함정\n\
- \n\n\
## 비목표\n\
- \n\n\
## 열린 질문\n\
- \n\n\
## 다음 단계\n\
- \n"
    )
}

fn render_spec_requirements() -> String {
    "사용자 스토리: [역할]은 [이유/효과]를 위해 [기능/변화]를 원한다.\n\n\
## 목적 / 성공 기준\n\n\
- \n\n\
## 원칙 / 제약\n\n\
- \n\n\
## 기능 요구사항 (EARS)\n\n\
- WHEN <조건> THE SYSTEM SHALL <관찰 가능한 동작>\n\
- GIVEN <전제> WHEN <트리거> THE SYSTEM SHALL <응답>\n\n\
## 비기능 요구사항\n\n\
- \n\n\
## 회귀 보존\n\n\
- WHEN <조건> THE SYSTEM SHALL CONTINUE TO <보존할 동작>\n"
        .to_string()
}

fn render_spec_design() -> String {
    "## 결정사항\n\n\
- \n\n\
## 영향받는 컴포넌트\n\n\
- \n\n\
## 제약\n\n\
- \n\n\
## 다이어그램\n\n\
```text\n\
[ASCII 다이어그램 자리]\n\
```\n"
        .to_string()
}

fn render_spec_tasks() -> String {
    "## 작업 목록\n\n\
작고 검토 가능한 구현 단위로 나눈다. 의존성(`[blocked by: T1]`)과 병렬 가능성(`[parallel: T2, T3]`)을 명시해서 실행 형태를 고를 수 있게 한다.\n\n\
- [ ] T1 — <짧은 제목>\n\
- [ ] T2 — <짧은 제목>  [blocked by: T1]\n\
- [ ] T3 — <짧은 제목>  [parallel: T2]\n"
        .to_string()
}

fn render_retrospect(slug: &str) -> String {
    format!(
        "# {slug}\n\n\
## 유지할 점\n\
- \n\n\
## 문제\n\
- \n\n\
## 시도할 점\n\
- \n\n\
## 액션 후보\n\
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
        assert!(task[0].1.contains("title = \"작업: foo\""));
        assert!(task[0].1.contains("branch = \"foo\""));
        assert!(task[0].1.contains("## 계획 (Planning)"));

        let workflow = DocKind::Workflow.render("foo");
        assert_eq!(workflow.len(), 1);
        assert!(workflow[0].1.contains("title = \"워크플로우: foo\""));
        assert!(workflow[0].1.contains("## 목적"));
        assert!(workflow[0].1.contains("mode = \"single\""));
    }

    #[test]
    fn spec_render_includes_tasks_skeleton() {
        let spec = DocKind::Spec.render("foo");
        assert_eq!(spec.len(), 3);
        assert_eq!(spec[2].0, PathBuf::from("specs/foo/tasks.md"));
        assert!(spec[0].1.contains("## 목적 / 성공 기준"));
        assert!(spec[0].1.contains("## 원칙 / 제약"));
        assert!(spec[2].1.contains("## 작업 목록"));
        assert!(spec[2].1.contains("[blocked by:"));
    }

    #[test]
    fn idea_render_uses_korean_work_sequence_headings() {
        let idea = DocKind::Idea.render("foo");
        assert_eq!(idea.len(), 1);
        assert!(idea[0].1.contains("## 원문 의도"));
        assert!(idea[0].1.contains("## 맥락 / 레퍼런스 탐색"));
        assert!(idea[0].1.contains("## 목적 / 성공 기준"));
        assert!(!idea[0].1.contains("Outcome / problem"));
    }
}
