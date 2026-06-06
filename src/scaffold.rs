use crate::storage::StorageRoot;
use crate::task::{TaskDocument, render_task_document};
use crate::workflow::{WorkflowMetadata, render_workflow_metadata};
use std::path::PathBuf;

pub const LEAF_PHASE_DIRS: [&str; 4] = ["01-Learn", "02-Example", "03-Architect", "04-Feedback"];

const LEAF_FILES: [&str; 8] = [
    "00-status.md",
    "01-Learn/01-intent.md",
    "01-Learn/02-unknowns.md",
    "01-Learn/02-references/README.md",
    "02-Example/03-criteria.md",
    "02-Example/04-wireframe.md",
    "03-Architect/05-design.md",
    "03-Architect/07-tasks.md",
];

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

    pub fn legacy_state_name(self) -> &'static str {
        match self {
            Self::Idea => "idea storage",
            Self::Spec | Self::Retrospect => "spec storage",
            Self::Task => "TaskDocument storage",
            Self::Workflow => "Workflow storage",
        }
    }

    pub fn paths(self, storage: &StorageRoot, slug: &str) -> Vec<PathBuf> {
        match self {
            Self::Idea => leaf_paths(storage.ideas_dir().join(slug)),
            Self::Spec => leaf_paths(storage.specs_dir().join(slug)),
            Self::Task => vec![storage.tasks_dir().join(format!("{slug}.toml"))],
            Self::Workflow => vec![storage.workflows_dir().join(format!("{slug}.toml"))],
            Self::Retrospect => vec![
                storage
                    .specs_dir()
                    .join(slug)
                    .join("04-Feedback/10-retrospect.md"),
            ],
        }
    }

    pub fn render(self, slug: &str) -> Vec<(PathBuf, String)> {
        match self {
            Self::Idea => render_leaf_documents(slug, LeafKind::Idea),
            Self::Spec => render_leaf_documents(slug, LeafKind::Spec),
            Self::Task => vec![(
                PathBuf::from(format!("execution/tasks/{slug}.toml")),
                render_task_document(&TaskDocument::empty(slug)),
            )],
            Self::Workflow => vec![(
                PathBuf::from(format!("execution/workflows/{slug}.toml")),
                render_workflow_metadata(&WorkflowMetadata::empty(slug)),
            )],
            Self::Retrospect => vec![(
                PathBuf::from(format!(
                    "planning/specs/{slug}/04-Feedback/10-retrospect.md"
                )),
                render_retrospect(slug),
            )],
        }
    }
}

#[derive(Clone, Copy)]
enum LeafKind {
    Idea,
    Spec,
}

impl LeafKind {
    fn root(self, slug: &str) -> String {
        match self {
            Self::Idea => format!("planning/ideas/{slug}"),
            Self::Spec => format!("planning/specs/{slug}"),
        }
    }
}

fn leaf_paths(base: PathBuf) -> Vec<PathBuf> {
    LEAF_FILES.iter().map(|name| base.join(name)).collect()
}

fn render_leaf_documents(slug: &str, kind: LeafKind) -> Vec<(PathBuf, String)> {
    let root = kind.root(slug);
    vec![
        (
            PathBuf::from(format!("{root}/00-status.md")),
            render_leaf_status(slug),
        ),
        (
            PathBuf::from(format!("{root}/01-Learn/01-intent.md")),
            render_leaf_intent(slug),
        ),
        (
            PathBuf::from(format!("{root}/01-Learn/02-unknowns.md")),
            render_leaf_unknowns(),
        ),
        (
            PathBuf::from(format!("{root}/01-Learn/02-references/README.md")),
            render_leaf_references_readme(),
        ),
        (
            PathBuf::from(format!("{root}/02-Example/03-criteria.md")),
            render_leaf_criteria(),
        ),
        (
            PathBuf::from(format!("{root}/02-Example/04-wireframe.md")),
            render_leaf_wireframe(),
        ),
        (
            PathBuf::from(format!("{root}/03-Architect/05-design.md")),
            render_leaf_design(),
        ),
        (
            PathBuf::from(format!("{root}/03-Architect/07-tasks.md")),
            render_leaf_tasks(),
        ),
    ]
}

fn render_leaf_status(slug: &str) -> String {
    format!(
        "# {slug} — Status\n\n\
## 위치 / Commitment\n\
- planning/ideas/{slug}/: exploration, not execution-prep\n\
- planning/specs/{slug}/: committed execution-prep\n\n\
## 현재 상태\n\
- 현재 phase / gate: \n\
- 첫 미충족 gate: \n\
- 다음 액션: \n\
- 최근 return: \n\
- return 횟수: 0\n\n\
## Gate 진행\n\n\
progress: 0 / 25 / 50 / 75 / 100, state: not-started / active / needs-approval / approved\n\n\
| Gate | progress | state |\n\
|---|---|---|\n\
| 0 Status | 25 | active |\n\
| 1 Intent | 0 | not-started |\n\
| 2 Unknowns & Context | 0 | not-started |\n\
| 3 Criteria | 0 | not-started |\n\
| 4 Wireframe | 0 | not-started |\n\
| 5 Design | 0 | not-started |\n\
| 6 Critic | 0 | not-started |\n\
| 7 Tasks | 0 | not-started |\n\
| 8 Artifact / Execution | 0 | not-started |\n\
| 9 Review | 0 | not-started |\n\
| 10 Retrospect | 0 | not-started |\n\n\
## Return Log\n\
- \n"
    )
}

fn render_leaf_intent(slug: &str) -> String {
    format!(
        "# {slug}\n\n\
## 원문 의도\n\
- \n\n\
## 해석한 의도\n\
- \n\n\
## Origin / Promotion\n\
- source: direct | planning/ideas/{slug}/\n\
- promotion: move planning/ideas/{slug}/ to planning/specs/{slug}/ when committed\n"
    )
}

fn render_leaf_unknowns() -> String {
    "## Domain concepts\n\n\
- [blocking now] \n\n\
## Standards / conventions\n\n\
- [blocking now] \n\n\
## External facts\n\n\
- [useful later] \n\n\
## Internal facts\n\n\
- [blocking now] \n\n\
## Verified facts\n\n\
- \n\n\
## Inventoried materials\n\n\
- \n\n\
## Flagged assumptions\n\n\
- \n\n\
## References / options / tradeoffs\n\n\
- \n"
        .to_string()
}

fn render_leaf_references_readme() -> String {
    "# References (② Unknowns & Context)\n\n\
덩치 큰 원본 자료(긴 문서·로그·스크린샷·외부 캡처 등)를 여기 둔다.\n\
이 폴더는 보관소일 뿐, 쓸모 있는 답·요약·판단 근거는 `../02-unknowns.md`로 되돌린다.\n\n\
- \n"
        .to_string()
}

fn render_leaf_criteria() -> String {
    "사용자 스토리: [역할]은 [이유/효과]를 위해 [기능/변화]를 원한다.\n\n\
## 목적 / 성공 기준\n\n\
- \n\n\
## 원칙 / 제약\n\n\
- \n\n\
## 출력 형태\n\n\
- docs-only change | implementation PR | prototype | spike | direct local edit | TaskDocument | saved Workflow | mixed-lifecycle handoff\n\n\
## 기능 요구사항 (EARS)\n\n\
- WHEN <조건> THE SYSTEM SHALL <관찰 가능한 동작>\n\
- GIVEN <전제> WHEN <트리거> THE SYSTEM SHALL <응답>\n\n\
## 비기능 요구사항\n\n\
- \n\n\
## 회귀 보존\n\n\
- WHEN <조건> THE SYSTEM SHALL CONTINUE TO <보존할 동작>\n"
        .to_string()
}

fn render_leaf_wireframe() -> String {
    "## Concrete instance\n\n\
- \n\n\
## Text-first wireframe\n\n\
```text\n\
[ASCII layout / command transcript / sequence sketch / state table]\n\
```\n\n\
## Placeholder contracts\n\n\
- Placeholder: \n\
  - Contract: \n\
  - Variation point: axis / range / limits\n\n\
## Representative states\n\n\
- Empty: \n\
- Error: \n\
- Edge: \n\
- Loading / timing: \n\n\
## Walkthrough result\n\n\
- User/operator confirmed: yes | no | pending\n\
- Notes: \n"
        .to_string()
}

fn render_leaf_design() -> String {
    "## 결정사항\n\n\
- \n\n\
## 원칙 (Principles)\n\n\
- \n\n\
## 결정 동인 (Decision drivers)\n\n\
- \n\n\
## 선택지 (Viable options)\n\n\
- 선택지 A: \n\
  - 장점: \n\
  - 단점: \n\
- 선택지 B: \n\
  - 장점: \n\
  - 단점: \n\n\
## 반대 논거 (Steelman antithesis)\n\n\
- 가장 강한 반대: \n\
- 답변: \n\n\
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

fn render_leaf_tasks() -> String {
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
## 결과\n\
- target: \n\
- result: \n\
- proof: \n\n\
## 시간 / Watch 회고\n\
- 작업 (task): \n\
- TaskRun: \n\
- branch / worktree: \n\
- agent / profile: \n\
- 예상 소요 (expected duration): \n\
- 예상 근거 (estimate basis): \n\
- 시작 / 종료 / 실제 소요: \n\
- 최초 meaningful signal: \n\
- watch 전략: launch validation / steady heartbeat / timeout\n\
- 실제 watch 관측: \n\
- 개입 / feedback: \n\
- cadence 판단: \n\
- 다음 추정 조정: \n\n\
## 유지할 점\n\
- \n\n\
## 문제\n\
- \n\n\
## 시도할 점\n\
- \n\n\
## 액션 후보\n\
- \n\n\
## Harness tuning\n\
- \n\n\
## Unknown surfacing misses\n\
- \n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_leaf_paths(root: &str) -> Vec<String> {
        LEAF_FILES
            .iter()
            .map(|file| format!("{root}/{file}"))
            .collect()
    }

    #[test]
    fn paths_return_expected_storage_locations() {
        let dir = tempfile::tempdir().unwrap();
        let storage = StorageRoot::from_git_common_dir(dir.path().join(".git"));

        assert_eq!(
            DocKind::Idea.paths(&storage, "foo"),
            LEAF_FILES
                .iter()
                .map(|file| dir.path().join(".wt/planning/ideas/foo").join(file))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            DocKind::Spec.paths(&storage, "foo"),
            LEAF_FILES
                .iter()
                .map(|file| dir.path().join(".wt/planning/specs/foo").join(file))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            DocKind::Task.paths(&storage, "foo"),
            vec![dir.path().join(".wt/execution/tasks/foo.toml")]
        );
        assert_eq!(
            DocKind::Workflow.paths(&storage, "foo"),
            vec![dir.path().join(".wt/execution/workflows/foo.toml")]
        );
        assert_eq!(
            DocKind::Retrospect.paths(&storage, "foo"),
            vec![
                dir.path()
                    .join(".wt/planning/specs/foo/04-Feedback/10-retrospect.md")
            ]
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

    #[test]
    fn spec_render_includes_tasks_skeleton() {
        let spec = DocKind::Spec.render("foo");
        assert_eq!(spec.len(), 8);
        assert_eq!(
            spec.iter()
                .map(|(path, _)| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            expected_leaf_paths("planning/specs/foo")
        );
        assert!(spec[0].1.contains("## Gate 진행"));
        assert!(spec[0].1.contains("planning/specs/foo/"));
        assert!(spec[1].1.contains("move planning/ideas/foo/"));
        assert!(spec[2].1.contains("## Domain concepts"));
        assert!(spec[2].1.contains("## Verified facts"));
        assert!(spec[3].1.contains("02-unknowns.md"));
        assert!(spec[4].1.contains("## 목적 / 성공 기준"));
        assert!(spec[4].1.contains("## 원칙 / 제약"));
        assert!(spec[5].1.contains("## Placeholder contracts"));
        assert!(spec[7].1.contains("## 작업 목록"));
        assert!(spec[7].1.contains("[blocked by:"));
    }

    #[test]
    fn idea_render_uses_korean_work_sequence_headings() {
        let idea = DocKind::Idea.render("foo");
        assert_eq!(idea.len(), 8);
        assert_eq!(
            idea.iter()
                .map(|(path, _)| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            expected_leaf_paths("planning/ideas/foo")
        );
        assert!(idea[0].1.contains("planning/ideas/foo/"));
        assert!(idea[1].1.contains("## 원문 의도"));
        assert!(idea[2].1.contains("## Domain concepts"));
        assert!(idea[4].1.contains("## 목적 / 성공 기준"));
        assert!(!idea[0].1.contains("Outcome / problem"));
    }

    #[test]
    fn retrospect_render_includes_timing_and_watch_fields() {
        let retrospect = DocKind::Retrospect.render("foo");
        assert_eq!(retrospect.len(), 1);
        assert!(retrospect[0].1.contains("## 시간 / Watch 회고"));
        assert!(retrospect[0].1.contains("예상 소요 (expected duration)"));
        assert!(retrospect[0].1.contains("watch 전략"));
        assert!(retrospect[0].1.contains("cadence 판단"));
    }
}
