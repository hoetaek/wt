# Consistency Philosophy

이 문서는 `wt` 코드베이스가 지켜야 할 사용자-facing canonical model을 정리한다.
정체성, 페르소나, 모델 진화의 이유는 [docs/north-star.md](north-star.md)가 소유하고,
이 문서는 그 방향을 CLI, config, 상태 파일, help text, selector, agent handoff에 적용한
운영 계약을 소유한다.

`wt`는 worktree, issue, pull request, profile, workflow, agent runtime처럼 서로
다른 개념을 조합하는 도구다. 기능이 늘어날수록 사용자가 기억해야 할 규칙이 늘기
쉽다. 그래서 `wt`의 UX는 기능 수보다 개념의 선명함을 우선한다.

## Operational Contract

사용자는 명령 이름, 옵션 이름, config 이름만 보고도 다음을 예측할 수 있어야 한다.

- 무엇을 대상으로 하는가
- 어떤 실행환경을 쓰는가
- 언제 한 개만 실행되고 언제 여러 개가 실행되는가
- 어떤 상태가 저장되고 다음 실행에 어떤 영향을 주는가

예측이 어렵다면 기능이 부족한 것이 아니라 모델이 흐린 것이다.

이 문서의 규칙은 `README.md`의 짧은 설명보다 우선하고, 구현과 테스트가 따라야 할
사용자-facing source of truth다. 이 문서와 `docs/north-star.md`가 경로, 명령, 상태 모양에
대해 충돌하면 branch는 incomplete 상태다. 두 canonical surface를 함께 남기지 말고 merge 전
한쪽으로 수렴한다.

## State Model

### Scope 4-tier

`wt` state는 네 scope로 나뉜다.

| Tier | 범위 | 위치 | 공유 |
| --- | --- | --- | --- |
| 1 | 글로벌 | `~/.config/wt/` | 내 머신 전체 |
| 2 | 팀 연동 설정 | `<repo>/.wt.toml` | git commit |
| 3 | 개인 작업 | `<repo-root>/.wt/` | commit 불가, 머신 로컬 |
| 4 | runtime actor context | `<repo-root>/.wt/runtime/agents/<name>/` | Tier 3 안의 하위 state |

Tier 2는 팀이 이미 사용하는 시스템과 어떻게 통합하는지의 설정이다. 팀이 공유하는 작업
데이터가 아니다. Idea, Spec, TaskDocument, Workflow, TaskRun, 메시지, activity,
runtime observation은 개인 작업 state이므로 Tier 3에 둔다.

Config precedence는 다음 순서다.

```text
global < team integration < personal < command-line flags
```

`wt config`는 effective behavior를 판단하는 source of truth이며, 가능한 한 어떤 scope에서
어떤 값이 왔는지 보여줘야 한다.

### Personal Storage

Personal storage의 canonical root는 `<repo-root>/.wt/`다. 이 결정의 source는
`<repo-root>/.wt/planning/specs/personal-storage-repo-root/`이며, 목적은 세 가지다.

- `.git/` namespace는 Git 자신의 metadata 자리로 비워 둔다.
- `.wt/...`는 `.git/wt/...`보다 짧고 사용자가 직접 열어 보기 쉽다.
- Claude Code 같은 agent harness가 `.git/` write permission friction 없이 wt state를 다룰 수 있다.

한 repository의 모든 worktree가 같은 personal storage를 보는 contract는 유지한다. 메커니즘은
directory-like path 기준이다: main repo의 `<repo-root>/.wt/`는 real directory이거나
directory를 가리키는 symlink일 수 있고, linked worktree에는 `.wt -> <main-repo>/.wt`
symlink를 둔다. `wt init`은 main repo의 `.wt`가 directory로 사용할 수 있는 path이고
clone-local `.git/info/exclude`에 exact line `/.wt`가 있음을 보장한다. wt가 만든 linked
worktree는 worktree setup path에서 `.wt -> <main-repo>/.wt` symlink를 보장한다.
`wt setup`은 repo personal storage를 준비하지 않는다.

Canonical personal storage layout:

```text
<repo-root>/.wt/
├── config/
│   ├── local.toml
│   └── profiles/
├── planning/
│   ├── ideas/
│   │   └── <slug>/                   # exploratory LEAF prep; kill-able
│   │       ├── 00-status.md
│   │       ├── 01-Learn/
│   │       │   ├── 01-intent.md
│   │       │   ├── 02-unknowns.md
│   │       │   └── 02-references/
│   │       ├── 02-Example/
│   │       │   ├── 03-criteria.md
│   │       │   └── 04-wireframe.md
│   │       ├── 03-Architect/
│   │       │   ├── 05-design.md
│   │       │   ├── 06-critic.md
│   │       │   ├── 07-tasks.md
│   │       │   └── 08-execution.md
│   │       └── 04-Feedback/
│   │           ├── 09-review.md
│   │           └── 10-retrospect.md
│   ├── specs/
│   │   └── <slug>/                   # executable-work baseline LEAF prep
│   │       ├── 00-status.md
│   │       ├── 01-Learn/
│   │       │   ├── 01-intent.md
│   │       │   ├── 02-unknowns.md
│   │       │   └── 02-references/      # template, always scaffolded; holds bulky source material
│   │       ├── 02-Example/
│   │       │   ├── 03-criteria.md
│   │       │   └── 04-wireframe.md
│   │       ├── 03-Architect/
│   │       │   ├── 05-design.md
│   │       │   ├── 06-critic.md      # lazy, when critic triggers fire
│   │       │   ├── 07-tasks.md
│   │       │   └── 08-execution.md    # lazy, when launch handoff exists
│   │       └── 04-Feedback/
│   │           ├── 09-review.md       # lazy, when review/sync evidence exists
│   │           └── 10-retrospect.md   # lazy, for spec-backed work retrospectives
│   └── retrospectives/
│       └── <slug>.md         # cross-work/spec-less retrospectives only
├── execution/
│   ├── tasks/
│   │   └── <task>.toml
│   ├── workflows/
│   │   └── <id>.toml
│   ├── task-runs/
│   │   └── <id>.toml
│   └── archive/
│       └── workflows/<id>/
└── runtime/
    ├── agents/
    │   └── <name>/
    │       ├── inbox/
    │       │   ├── new/
    │       │   ├── claimed/
    │       │   ├── delivered/
    │       │   ├── retry/
    │       │   └── failed/
    │       ├── activity.jsonl
    │       ├── observations/
    │       │   └── wait-observations.jsonl
    │       ├── anchors/
    │       ├── status.toml
    │       └── supervisor.{toml,log}
```

Repo-root `.local` state and legacy `.git/wt/` state are not canonical. 새 코드와 새 문서는
위 layout의 `<repo-root>/.wt/...` 경로를 primary state로 읽고 쓴다. 이전 storage를 다뤄야
하면 명시적 import/repair 명령으로 다루고, silent fallback이나 alias처럼 동작시키지 않는다.

The four top-level buckets are the canonical personal-state owners:

- `config/` owns personal config and profile inputs that shape later behavior.
- `planning/` owns prep and learning artifacts: ideas, specs, and retrospectives.
- `execution/` owns launch and run artifacts: TaskDocuments, Workflows, TaskRuns,
  and archive.
- `runtime/` owns local agent identity and agent-owned runtime state: inbox,
  activity/status snapshots, identity anchors, supervisor state, and runtime
  observation.

`runtime/agents/<name>` is the filesystem projection of `AgentId` `agents/<name>`.
It is not a second identity grammar. Because canonical `AgentId` values have one
non-empty segment after `agents/`, no slash escaping is needed for the canonical
agent directory.

Top-level `messages/`, `agent.state/`, `sessions/`, and `worktrees/` are not
canonical state owners in this model. They may appear only as legacy/migration
context until implementation paths are moved. New docs, new state owners, and
normal code must not read from or write to them as canonical paths or equivalent
aliases. Migration, import, or repair code that still touches a legacy root must
transform the data into one of the four canonical buckets, record the canonical
result, and leave later code paths on bucket readers.

Legacy `agent.state/` wait observations and `sessions/` identity anchors are runtime
actor context. `wt doctor` should surface them as legacy roots and `wt init`
should reject bootstrapping over them; neither command should silently treat
them as canonical state. The canonical replacements are
`runtime/agents/<name>/observations/wait-observations.jsonl` and
`runtime/agents/<name>/anchors/<encoded-anchor-key>.toml`.

### Idea And Spec Prep

LEAF 구조는 사고 절차이고, Idea/Spec 위치는 commit level이다. Idea와 Spec은 같은 numbered
LEAF layout을 쓴다. 차이는 파일 모양이 아니라 사용자가 그 일을 실행 가능한 작업으로
취급하기로 commit했는지다.

Idea는 kill-able exploration이다. `wt-ready`는 사용자가 아직 실행 가능한 작업으로 취급하겠다고
commit하지 않은 탐색을 `<repo-root>/.wt/planning/ideas/<slug>/`에 쓴다. Idea 안에서도 계획,
criteria, wireframe, design, task graph 후보를 얼마든지 작성할 수 있다. 하지만 Idea는
committed-work status가 없고, downstream consumer가 의존하지 않으며, 언제든 삭제하거나 다시 쓸 수
있다. Idea 삭제나 재작성은 다른 component가 관찰해야 하는 state transition이 아니다.

Spec은 실행 가능한 작업으로 취급하기로 commit한 LEAF prep artifact다. `wt-ready`가 idea를 받고
사용자가 실행 가능한 작업으로 취급하겠다고 commit하면 idea는 spec으로 promotion된다. Promotion은
`planning/ideas/<slug>/` directory를 `planning/specs/<slug>/`로 옮기는 동작이다. 이 directory
location change가 visible commit gate다. `wt` state tree를 읽는 사람은
`planning/ideas/<slug>/` 아래의 exploration과 `planning/specs/<slug>/` 아래의 executable-work
baseline을 directory 위치만으로 구분할 수 있어야 한다. Flat `planning/ideas/<slug>.{md,toml}`은
legacy/migration-only artifact이며 새 scaffold나 새 docs가 만들지 않는다.

TaskDocument는 계속 `<repo-root>/.wt/execution/tasks/<slug>.toml`에 있는 launch unit이다. 그
body는 `planning/specs/<slug>/` relative path를 참조할 수 있지만 TaskDocument schema는 바뀌지
않는다. Spec은 intent, unknowns & context, criteria, wireframe, design, tasks, execution handoff,
review/sync, retrospect를 담는 긴 human/AI artifact이고, TaskDocument는 `wt run task`와
`wt workflow`가 소비하는 실행 단위다. Spec 없이 TaskDocument TOML만 있는 pre-redesign task도
valid local task로 남는다.

Idea/Spec 내부의 canonical layout은 LEAF phase folder다. `planning/ideas/<slug>/`와
`planning/specs/<slug>/` bucket은 wt의 personal-state ownership을 유지하고, 그 안에서
`01-Learn/`, `02-Example/`, `03-Architect/`, `04-Feedback/`이 사고 과정의 phase를 표현한다.
일반 leaf-work는 여러 artifact를 담기 위해 `05-design-<artifact>.md`,
`08-execution-<artifact>.md`, `09-review-<artifact>-vN.md`, `10-retrospect-<topic>.md` 같은
suffix를 권장하지만, wt idea/spec은 slug 하나가 하나의 work item을 소유하므로 canonical file은
`05-design.md`, `06-critic.md`, `08-execution.md`, `09-review.md`, `10-retrospect.md`처럼
deterministic하게 둔다.
`06-critic.md`는 critic trigger가 실제로 발동할 때만 만드는 lazy artifact이고, skip/low-risk
판단은 `05-design.md`에 기록한다.

`wt scaffold`가 만드는 idea/spec/task/workflow/retrospect template의 사람이 읽는 제목과
section heading은 한국어를 기본으로 한다. TOML field name(`title`, `branch`, `mode` 등)은
schema contract이므로 영어로 유지하지만, 값과 body template은 한국어로 읽히게 한다.

LEAF prep은 scaffold가 첫 동작이다. `wt scaffold <slug> --idea`와
`wt scaffold <slug> --spec`은 각각 `planning/ideas/<slug>/` 또는 `planning/specs/<slug>/`에
`00-status.md`와 `01-Learn/`, `02-Example/`, `03-Architect/`, `04-Feedback/` 네 phase directory를
보장한 뒤 gate file을 채운다. 너무 작아서 이 body가 필요 없는 작업은 LEAF idea/spec prep으로
승격하지 않는다.

`00-status.md`는 idea/spec을 재개 가능하게 하는 dashboard다. 현재 phase/gate,
첫 미충족 gate, 다음 액션, 최근 return, return 횟수, gate별 progress를 적는다. progress 값은
`0`, `25`, `50`, `75`, `100`, state 값은 `not-started`, `active`, `needs-approval`, `approved`를
쓰고 return은 별도 gate state가 아니라 Return Log event로 기록한다. 이 파일은 index이지 source of
truth가 아니다 — gate file이 authoritative하다.

`01-Learn/01-intent.md`는 raw user wording, interpreted intent, and commitment note를 preserve한다.
이 파일은 later agent가 "사용자가 실제로 무엇을 요청했는지"와 "coordinator가 어떻게 해석했는지"를
구분할 수 있게 해야 한다.
Intent는 core noun과 topology도 기록할 수 있다. Core noun은 사용자가 실제로 바꾸려는 대상
(`idea`, `spec`, `task`, `workflow`, command, UI 등)이고, topology는 독립적으로 성공/실패할 수
있는 top-level outcome, surface, integration, deliverable 목록이다. Deferred topology item은
삭제하지 않고 왜 이번 prep에서 제외했는지 적는다.

`01-Learn/02-unknowns.md`는 domain concepts, standards/conventions, external facts,
internal facts를 구분하고 각 항목을 `blocking now` 또는 `useful later`로 표시한다. Evidence
gathering은 이 unknown list를 agenda로 삼는다. Clarity ledger는 gate마다 역할이 다르다.
Gate 1은 desired effect와 core noun으로 Intent row를 잠그고, Gate 2는 ledger를 learning lens로만
써서 가장 약한 row를 판단할 수 있게 해 줄 domain / standards-conventions / external / internal
unknown 또는 inventory를 겨냥한다. Gate 2에서 row를 억지로 닫지 않는다. Gate 3은 intent,
topology, success, constraints, output form 전체를 score하고, purpose / requirements /
principles / acceptance checks / explicit assumptions-or-risks로 잠근다.

Gate 2는 unknown surfacing과 context/reference 탐색을 하나의 gate로 합친다. 같은
`02-unknowns.md`에 verified facts, inventoried user/team material, flagged assumptions,
references/options/tradeoffs를 함께 적어 각 unknown 항목 옆에서 답을 채운다. 이 파일은 결정문이
아니라 downstream gates가 의존할 수 있는 fact inventory다. 분량이 큰 원본은 항상 seed되는
`01-Learn/02-references/`(README 템플릿 포함)로 빼고 요약만 `02-unknowns.md`에 남긴다. 폴더가
항상 존재하므로 보유 자료를 둘 자리는 빈 게이트에서도 보이며, `02-unknowns.md` 파일명이 unknown만
받는다는 인상을 슬롯의 존재로 상쇄한다.

LEAF의 middle gates는 produce/consume engine이다. Gate 3 Criteria는 답이 존재하기 전에 의도한
효과와 관찰 가능한 기준을 적는 arbiter이자 test이고 `03-criteria.md`에 둔다. Gate 4 Wireframe은
criteria가 통과해야 하는 concrete instance와 contract이므로, 의미 있는 mock data, placeholder,
variation point, structure decision이 있으면 `04-wireframe.md` 또는 `04-wireframe/`로 분리한다.
Gate 5 Design은 그 contract를 consume해서 empty/overflow/edge/timing/failure를 포함한 전체
variation range로 일반화하는 generator다. 세 파일(`03-criteria.md`, `04-wireframe.md`,
`05-design.md`)은 합치지 않는다.

`wt scaffold <slug> --idea`와 `wt scaffold <slug> --spec`은 새 idea/spec에 phase folders를 seed한다:
`00-status.md`, `01-Learn/01-intent.md`, `01-Learn/02-unknowns.md`,
`01-Learn/02-references/README.md`, `02-Example/03-criteria.md`,
`02-Example/04-wireframe.md`, `03-Architect/05-design.md`,
`03-Architect/07-tasks.md`. `02-references/`는 더 이상 lazy가 아니라 항상 README
템플릿과 함께 seed되는 보관 슬롯이다. lazy artifact인
`03-Architect/08-execution.md`, `04-Feedback/09-review.md`, `04-Feedback/10-retrospect.md`는
seed하지 않고 handoff/review/retrospect 시점에 만든다. 이전 numbering(`03-context.md`,
`04+05-requirements.md`, `04+05+06-requirements.md`, `06-wireframe.md`, `07-design.md`,
`08-tasks.md`, `09-execution.md`, `10-review.md`, `11-retrospect.md`)과 이전 wt 9-gate
파일(`03-Architect/06-tasks.md`, `03-Architect/07-execution.md`,
`04-Feedback/08-review.md`, `04-Feedback/09-retrospect.md`)은 pre-10-gate
legacy/starter artifact로만 취급한다. 새 idea/spec이나 새 docs는 그 이름을 만들지 않고, scaffold는
그런 파일이 남아 있으면 ten-gate 파일을 만들기 전에 정리하도록 거부한다.

`planning/specs/<slug>/02-Example/03-criteria.md`는 purpose/success criteria,
requirements/principles, output form을 담는다. 첫 줄은 한국어 사용자 스토리 line으로 시작한다.

```text
사용자 스토리: [역할]은 [이유/효과]를 위해 [기능/변화]를 원한다.
```

Functional requirement section heading은 한국어로 두되, requirement 문장은 EARS statement를
유지한다.

```text
WHEN <조건> THE SYSTEM SHALL <관찰 가능한 동작>
GIVEN <전제> WHEN <트리거> THE SYSTEM SHALL <응답>
```

`03-criteria.md`는 목적 / 성공 기준, 원칙 / 제약, 출력 형태, 기능 요구사항(EARS),
비기능 요구사항, 회귀 보존 section을 둔다. 비기능 요구사항은 성능, 보안, 호환성, 또는 해당
작업에 적용되는 cross-cutting constraint를 명시적으로 이름 붙인다. Regression-sensitive work는
preserved behavior를 다음 형태로 적는다.

```text
WHEN <조건> THE SYSTEM SHALL CONTINUE TO <보존할 동작>
```

`planning/specs/<slug>/02-Example/04-wireframe.md` 또는 `02-Example/04-wireframe/`는 Gate
4의 concrete instance와 contract를 담는다. 먼저 requirements를 pages, flows, states,
commands, document sections 같은 실제 나타날 bucket으로 묶고, text-first wireframe을 만든다:
ASCII layout, command transcript, sequence sketch, table/state matrix, 또는 placeholder
evidence가 있는 outline. 각 placeholder나 mock value는 어떤 contract를 instantiates하는지,
무엇이 어떤 axis/range 안에서 변할 수 있는지 기록한다. User/operator walkthrough가 통과하기
전에는 `03-Architect/05-design.md`가 구조를 일반화하면 안 된다.

`planning/specs/<slug>/03-Architect/05-design.md`는 결정사항, 영향받는 컴포넌트, 제약을 적는다.
Gate 5 design은 Gate 4 contract와 variation points를 input으로 consume하고, concrete instance를
모든 valid instance를 생성할 수 있는 reusable generator로 일반화한다. Empty, overflow, edge,
timing, failure case처럼 단일 mock instance가 보여주지 못한 variation range를 다룬다. Design이
artifact shape, schema, placeholder meaning을 새로 발명해야 한다면 Gate 4로 돌아간다.
Gate 5는 principles, decision drivers, viable options, steelman antithesis를 durable rationale로
남긴다. Brownfield work에서는 새 design 전에 Static Model section(Purpose, Components, Business
Rules)과 Dynamic Model section(workflow/behavior)을 둘 수 있다. Design은 raw code dump가 아니라
intent와 component responsibility 중심으로 설명한다.

`planning/specs/<slug>/03-Architect/06-critic.md`는 public CLI/config/state shape, migration,
security, cross-module coupling, large UI/workflow behavior shift, weak alternatives처럼 critic trigger가
발동할 때만 만든다. Verdict는 `APPROVE`, `ITERATE`, `REJECT` 중 하나이고, `ITERATE`/`REJECT`는
Gate 7 tasking 전에 필요한 가장 작은 design revision을 적는다.

`planning/specs/<slug>/03-Architect/07-tasks.md`는 작업 목록 section 아래에 sequenced
atomic unit을 checkbox item으로 나열한다. 각 item은 dependency를 적고, dependency가 없는
item은 parallel 가능하다고 표시할 수 있다.

`planning/specs/<slug>/03-Architect/08-execution.md`는 `03-Architect/07-tasks.md`에서
드러난 slice graph를 어떤 execution shape로 실행할지와 그 이유, `wt-work` target,
TaskDocument path, optional saved Workflow TOML path, PR/landing policy, acceptance checks를
prose로 기록하는 lazy prep/execution artifact다. 실제 handoff에는 file path, module/symbol,
issue/task id, acceptance criteria, numbered implementation step, command/config transcript,
representative example/mock data, named output artifact, or user-accepted residual risk 같은
concrete execution signal이 있어야 한다. Saved execution plan은 계속
`<repo-root>/.wt/execution/workflows/<id>.toml`에 있고, `03-Architect/08-execution.md`는
executable Workflow TOML이 아니다.

Canonical `07-tasks.md` slice graph → execution decision mapping:

| 07-tasks.md slice graph | Execution decision |
| --- | --- |
| All sequential, single agent | `single` |
| All independent, same base | `batch` |
| Parent → child chain (each builds on previous branch) | `stack` |
| One task × multiple profiles | `matrix` |
| One direct slice only, or mixed-lifecycle slices | `none` (spec prep judgment; not persisted as Workflow mode) |

`none`은 `<repo-root>/.wt/execution/workflows/<id>.toml`에 저장되는 Workflow `mode` 값이 아니라 spec
prep 판단이다. 이 값은 direct `wt run task`로 충분하거나, slice들이 서로 다른 lifecycle에서
실행되어 하나의 saved Workflow로 표현하면 오히려 모델이 흐려지는 경우를 뜻한다.

Spec은 `wt-ready` exit 시점에 frozen되지 않는다. Execution 중 `wt-work` phase에서
findings가 나오면 design, task list, execution shape rationale을 in place로 업데이트할 수
있다. 선택한 mode가 더 이상 맞지 않거나 실제 Workflow TOML과 spec이 갈라지면
`03-Architect/08-execution.md`를 rationale과 함께 업데이트한다. Review/check evidence, spec
drift, and mid-process discoveries는 `planning/specs/<slug>/04-Feedback/09-review.md`에
기록한다. Spec과 implementation이 drift하면 조용히 갈라지게 두지 말고 spec을 업데이트해
다시 맞춘다.

Spec-backed work의 retrospective는 기본적으로 `planning/specs/<slug>/04-Feedback/10-retrospect.md`에 둔다.
`<repo-root>/.wt/planning/retrospectives/`는 여러 work item을 가로지르는 cross-work learning,
spec이 없는 legacy/direct work, 또는 의도적으로 한 spec에 묶이지 않는 회고의 fallback이다.
새 per-work retrospective를 전역 `retrospectives/` 아래에 만들지 않는다.

`04-Feedback/10-retrospect.md`는 작업별 timing record를 포함한다. 최소한 TaskDocument의 expected
duration, estimate basis, 실제 시작/종료/elapsed, 최초 meaningful signal, 사용한
`wt agent watch` cadence, `needs_input`/report 전이, 개입 이유, 다음 추정 조정을 적는다.
Spec-backed workflow는 workflow 전체 요약만 쓰지 말고 task/slice별 timing entry를 둔다.
Cross-work timing 보정은 `<repo-root>/.wt/planning/retrospectives/timing.md` 같은 rolling
retrospective에 축약할 수 있지만, 이것은 여러 작업을 가로지르는 학습 기록이지 per-work
`04-Feedback/10-retrospect.md`의 대체물이 아니다. `runtime/agents/<name>/observations/wait-observations.jsonl`과
`wt agent wait-stats`는 watch heartbeat/timeout 관측 증거로 인용할 수 있으나, 실제 작업
소요시간의 canonical source로 보지 않는다.

### Worktree Facts And Agent Identity

Worktree facts are not a canonical state owner. Git owns the checked-out
worktree list; `wt` records branch/path/worktree facts only inside the planning,
execution, or runtime record that needs them. New state must not create
`<repo-root>/.wt/worktrees/<id>/` as a fourth personal-state owner.

When a record needs a stable worktree-ish identity, it separates opaque identity
from human label:

```toml
id = "wt_20260519_103045_a3f8"
display_name = "feat-add-schema"
branch = "feat-add-schema"
path = "/abs/path/to/worktree"
kind = "worker"
created_at = "2026-05-19T10:30:45Z"
updated_at = "2026-05-19T10:30:45Z"
```

`id`는 opaque, stable, immutable이다. `display_name`은 UI와 selector용 human label이고
변경될 수 있다. `branch`와 `path`는 현재 Git/worktree 사실을 기록한 값이며 변경될 수 있다.
Agent identity is the communication actor identity. `AgentId` remains
`agents/<name>`; its canonical runtime directory is
`<repo-root>/.wt/runtime/agents/<name>/`. When a worktree display name is
safe and unique enough for an agent inbox, `agents/<display_name>` can be the
agent id. Here "safe" means the display name is non-empty, contains no `/` or
control characters, already fits the one-segment `AgentId` grammar, and is
unique within the current agent namespace. For example, a record with
`display_name = "feat-add-schema"` may use `agents/feat-add-schema`. A display
name with a slash, control character, or collision must not be used directly;
use an opaque stable name such as `agents/wt_20260519_103045_a3f8` or a
deterministic slug-plus-hash derived from the opaque id. If stability or
collision handling is required, derive the agent name from the opaque id instead
of adding a separate `worktrees/` state owner.

`kind = "orchestration" | "worker"`는 기본 동작 hint다. 권한 모델이나 강제 규칙이 아니다.

### Communication Channels

Activity, Inbox, Status는 다른 개념이다.

| Channel | Writer | Reader | Data | Location |
| --- | --- | --- | --- | --- |
| Activity log | hook 자동 | UI와 debug | append-only JSONL | `runtime/agents/<name>/activity.jsonl` |
| Inbox | 의도된 sender 또는 delivery owner | target agent와 scope owner | scoped TOML file queue | `runtime/agents/<name>/inbox/<state>/` |
| Status | hook 자동 | 누구나 | single TOML snapshot | `runtime/agents/<name>/status.toml` |

Activity는 communication이 아니다. Hook은 activity와 status를 자동으로 채울 수 있지만,
Inbox는 의도된 메시지 또는 정의된 lifecycle event만 받는다. Message state는 detached
supervisor가 생기더라도 cmux transport, hook transport, Workflow/TaskRun 상태와 섞지 않는다.

### Scoped Message Delivery

Canonical Message model은 세 개념을 분리한다.

- Address/recipient: `meta.from`과 `meta.to`는 누가 보냈고 누가 받아야 하는지를 나타낸다.
  현재 recipient는 여전히 agent-oriented `agents/<agent>` 주소다. CLI 입력에서는
  `<agent>`를 `agents/<agent>`로 정규화한다. `<agent>`는 path segment 하나여야 하고,
  `agents/<agent>/<role>`처럼 여러 segment가 필요한 주소는 모호하므로 실패한다.
- Scope/ownership: `[scope]`는 어떤 context가 message delivery를 소유하는지를 나타낸다.
  `scope.kind = "direct" | "workflow" | "task_run" | "repo"`가 canonical 값이다. `direct`와
  `repo`는 repo-local singleton scope이므로 `scope.id`를 쓰지 않는다. `workflow`와
  `task_run`은 각각 Workflow id 또는 TaskRun id를 `scope.id`에 저장한다.
- Delivery lifecycle: `[delivery]`는 delivery responsibility와 recovery state다.
  `delivery.state = "new" | "claimed" | "delivered" | "retry" | "failed"`가 canonical 값이다.
  `claimed`는 `delivery.claimed_by`와 `delivery.lease_expires_at`가 있어야 한다.
  `delivery.attempts`는 delivery attempt count이고, `delivery.last_error`는 retry/fail 원인이다.

Canonical message TOML shape:

```toml
[meta]
id = "msg_..."
created_at = "2026-05-20T12:00:00Z"
from = "agents/worker-a"
to = "agents/coordinator"

[scope]
kind = "workflow"
id = "2026-05-20-001"

[envelope]
kind = "request"
priority = "normal"
expects_response = true
correlates_with = "msg_previous"

[delivery]
state = "new"
attempts = 0

[body]
summary = "Review complete"

[[body.parts]]
type = "text"
content = "Agent Completion Report: ..."
```

`envelope.correlates_with` is correlation/threading only. It is not Workflow, TaskRun, scope,
ownership, claim, or routing metadata. Workflow ownership always comes from `[scope]`, not from
`correlates_with`, message body text, cmux workspace/surface coordinates, or recipient address alone.

Canonical physical layout is state-directory based:

```text
<repo-root>/.wt/runtime/agents/<name>/inbox/new/<message-id>.toml
<repo-root>/.wt/runtime/agents/<name>/inbox/claimed/<message-id>.toml
<repo-root>/.wt/runtime/agents/<name>/inbox/delivered/<message-id>.toml
<repo-root>/.wt/runtime/agents/<name>/inbox/retry/<message-id>.toml
<repo-root>/.wt/runtime/agents/<name>/inbox/failed/<message-id>.toml
```

Here `<name>` is the single name segment from `AgentId` `agents/<name>`. The
runtime directory is a filesystem projection of the same identity, not a new
message address format.

Directory state is the visible source of truth for inspection and atomic transitions; TOML
`delivery.state` mirrors the directory. Exact transition names are:

- `send`: create `inbox/new/<message-id>.toml` with `scope.kind = "direct"` unless an explicit
  scoped send surface says otherwise.
- `claim`: move `inbox/new` or eligible `inbox/retry` to `inbox/claimed`, set `delivery.state =
  "claimed"`, `delivery.claimed_by`, and `delivery.lease_expires_at`.
- `deliver`: move `inbox/claimed` to `inbox/delivered` after successful transport delivery.
- `retry`: move failed delivery attempts to `inbox/retry`, increment `delivery.attempts`, and set
  `delivery.last_error`.
- `fail`: move poison or exhausted messages to `inbox/failed` with `delivery.last_error`.

Pre-redesign files directly under `inbox/<message-id>.toml` or old `inbox/read/<message-id>.toml`
are legacy state, not an alternate canonical lifecycle. Under the pre-1.0 policy, new code must not
silently consume or reinterpret those paths as aliases for `new` or `delivered`. Repair/import code,
if added, must be explicit and should explain that the old unread/read layout has been replaced.

Canonical scriptable send:

```bash
wt msg send --to agents/codex "hello"
wt msg send --to codex "hello"
wt msg send --scope workflow:2026-05-20-001 --to agents/coordinator "workflow note"
```

`wt msg send` writes `meta.from = "agents/user"` unless `WT_AGENT_ID` is set to `agents/<agent>` or
`<agent>`. Without `--scope`, it writes direct-scope messages to `inbox/new`. Explicit scoped sends
accept `direct`, `repo`, `workflow:<id>`, and `task_run:<id>`. `agents/coordinator` is an ordinary
explicit inbox address; it is not dynamically resolved from runtime context.

TaskRun completion reports use the TaskRun-owned report route instead of composing a raw scoped
message command:

```bash
wt task report "Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>"
```

For a direct running or passed TaskRun, `wt task report` sends direct scope. For a
workflow-linked running or passed TaskRun, it derives `workflow:<id>` from persisted TaskRun state
and sends from the TaskRun `agent_id` to the stored `coordinator_id`. Without
`WT_TASK_RUN_ID`, branch fallback resolves exactly one running or passed TaskRun and fails with
candidate ids on ambiguity. Workflow task prompts must therefore not ask agents to manually compose
raw message scope and recipient arguments themselves.

There is no dynamic `coordinator` recipient alias. Bare `coordinator` is accepted only because
`AgentId` accepts bare `NAME` and normalizes it to `agents/NAME`; it has the same meaning as the
ordinary explicit inbox address `agents/coordinator`.

Canonical hook compatibility delivery:

```bash
wt msg check-inbox
```

Without `--agent`, `check-inbox` first reads valid `WT_AGENT_ID`. If that env is absent, it resolves
the current live identity anchor and reads that anchor id's inbox. If neither source exists, it exits
successfully with no output and does not create an identity anchor. Before rendering hook output `check-inbox`
reclaims expired leases according to the delivery lifecycle policy, claims deliverable messages from
`inbox/new` or eligible `inbox/retry`, prints JSON containing
`hookSpecificOutput.additionalContext`, and acknowledges the claims into `inbox/delivered/` only
after stdout is written successfully. Active non-expired claims remain owned by their current
claimant. This command is a compatibility consumer for agent hooks, not a separate unread/read
lifecycle.
Omitted hook event name preserves the compatible `UserPromptSubmit` output default; wt-managed
hook templates pass an internal event name argument so `UserPromptSubmit` and `PostToolUse` outputs
match the event that invoked the hook.

For ordinary agent recipients, `check-inbox` claims direct-scope messages. Non-direct scope delivery
requires explicit ownership evidence. For workflow reports, the ownership evidence is recorded
TaskRun state: if a TaskRun's `coordinator_id` is the resolved inbox agent and that TaskRun records
workflow `<id>`, the agent may claim `workflow:<id>` messages without loading the live workflow
file. Hook context includes a `scope:` line for non-direct messages so the coordinator can
distinguish workflow reports from standalone direct messages.

For coordinator review feedback, the canonical path is:

```bash
wt task review <task-run-id> --accept|--reject|--block "<message>"
```

`wt task review` sends from the current actor id to the TaskRun's stored `agent_id` using
`task_run:<task-run-id>` scope. It records `last_review_status`, `last_review_message_id`,
`last_reviewed_at`, and `updated_at` on the TaskRun. `--reject` and `--block` reopen a passed
TaskRun to `running` after sending feedback; `--accept` records metadata only and does not pass a
running TaskRun. `--accept --codex-base <parent>` additionally records dedicated
`codex_base_review_*` evidence for required Codex base-diff review gates; generic `--accept`
metadata alone does not satisfy those gates. Task-agent hooks may claim `task_run:<id>` messages only when both `WT_AGENT_ID`
matches `TaskRun.agent_id` and `WT_TASK_RUN_ID` matches the scoped TaskRun id. Passing `--agent` to
the low-level hook consumer is not by itself task-run ownership evidence.

After a file-inbox sender durably writes a message, `wt` may best-effort wake the recipient when it
can prove a live idle runtime target from canonical state. Task-run-scoped messages wake only when
the scoped TaskRun's recorded `agent_id` matches `meta.to`; direct messages may wake when exactly
one running TaskRun owns `meta.to` or when `meta.to` has a live surface identity anchor. Wake attempts
are internal delivery help: they do not create another message, do not change message delivery
state, do not add a user-facing command, and must not make a successful inbox write fail merely
because cmux/runtime observation is unavailable.

General workflow-supervisor ownership beyond TaskRun-recorded workflow scopes is not implemented
yet. Future supervisor identities must define explicit scope ownership before claiming shared
messages; raw recipient address, alias normalization, or `correlates_with` is insufficient ownership
evidence for that future mechanism.

wt-managed Claude/Codex agent hooks register the same inbox check on both `UserPromptSubmit` and
`PostToolUse`; both events route through the `wt msg check-inbox` claim → hook JSON → acknowledge
lifecycle above.

Supervisor delivery is Layer 3 stale-rescue, not a second inbox model. A started supervisor watches
the target `inbox/new/` directory with `notify` and keeps `--poll-interval` as a missed-event and
stop-check fallback. It only pushes messages that are older than `--stale-threshold`; fresh messages
remain available for normal hook delivery at the next `UserPromptSubmit` or `PostToolUse` event.
When a message is stale, the supervisor claims the exact `inbox/new` path, renders a bounded ASCII
cmux payload, resolves the target surface's workspace from cmux runtime evidence, pushes it to that
workspace/surface pair, and acknowledges the claim only after cmux push succeeds. Failed pushes move
through retry/failed delivery states using the same claim lifecycle.

Push delivery outside the supervisor stale-rescue path, `wt://` artifact semantics, and
provider-private Claude/Codex runtime integration are outside this contract slice. Future delivery
implementations must build on the same address/scope/delivery lifecycle instead of adding a second
hidden inbox model.

### Supervisor Lifecycle

The agent supervisor is Layer 3 stale-rescue insurance. It is opt-in and default-off. It does not
replace normal message delivery and it should push zero payloads during engaged operation: it only
intervenes when a message remains in the recipient's `inbox/new/` longer than the registered
`stale_threshold_secs`. A supervisor started with `--surface` is hosted in an unfocused cmux surface
inside the target surface's pane because cmux push delivery fails from PPID 1 orphan processes; a
supervisor without `--surface` may use the detached process path because it does not push to cmux.

The three message attention layers are:

| Layer | Surface | Responsibility |
| --- | --- | --- |
| Layer 1 | `wt msg watch` | Human-visible inbox watching and explicit message handling. |
| Layer 2 | `wt agent watch` | Runtime observation of a specific agent target. |
| Layer 3 | `wt agent supervisor ...` | Stale-rescue push after `inbox/new/` age exceeds threshold. |

Supervisor registrations live at `<repo-root>/.wt/runtime/agents/<name>/supervisor.toml`; logs
live beside them as `supervisor.log`. `<name>` is the filesystem segment from
`AgentId` `agents/<name>`, so supervisor state stays inside the same runtime
agent owner as inbox/activity/status state. Surface-backed supervisors also record the cmux
surface, pane, and workspace that host the supervisor process so `stop` can close the host surface.
Registration schema is:

```toml
agent_id = "agents/codex"
pid = 12345
pid_start_time = "123.000000000"
started_at = "2026-05-22T00:00:00Z"
started_by = "agents/codex"
cleanup_on_session_end = true
target_surface_id = "surface:72"
target_agent_kind = "codex"
host_workspace_id = "workspace:19"
host_pane_id = "pane:3"
host_surface_id = "surface:73"
stale_threshold_secs = 900
poll_interval_secs = 60
log_path = "/repo/.wt/runtime/agents/codex/supervisor.log"
```

`target_surface_id`, `target_agent_kind`, `host_workspace_id`, `host_pane_id`, and
`host_surface_id` are optional.
`stale_threshold_secs` defaults to 900 seconds and `poll_interval_secs` defaults to 60 seconds when
starting a supervisor.

Canonical lifecycle commands:

```bash
wt agent supervisor start <agent>
wt agent supervisor stop <agent>
wt agent supervisor stop --owned-by "$WT_AGENT_ID"
wt agent supervisor status [<agent>]
wt agent supervisor logs <agent>
wt agent supervisor run <agent>
```

Cleanup is PID-registration based. Never use `pkill -f <bare-verb>` in code, tests, docs, or
operator guidance; broad command patterns can match the calling agent runtime. Always stop through
`wt agent supervisor stop`, which reads registered PIDs and checks ownership when `--owned-by` is
used. `wt doctor` scans supervisor registrations, keeps live PIDs, removes stale registration TOML
files, and preserves log files for post-mortem review.

Claude Code SessionEnd cleanup is installed only through wt-managed hook setup. The generated
SessionEnd command is:

```bash
wt agent supervisor stop --owned-by "$WT_AGENT_ID"
```

Codex does not expose an equivalent SessionEnd hook today. Codex operators should stop owned
supervisors manually with the same command before closing a session.

Supervisor cmux push payloads are ASCII-only and bounded by the supervisor payload cap. The default
payload cap follows the cmux push service default. Agent-targeted cmux prompt submit paths
(`agent.prompts`, inbox wake/push, and `wt send` without `--no-enter`) use the same kind-specific
submit patterns:

| Agent kind | cmux submit pattern |
| --- | --- |
| `claude` | `set-buffer`, `paste-buffer`, short settle, then `send-key enter`. |
| `codex` | `set-buffer`, `paste-buffer`, wait for Codex folded-paste marker on long prompts, then `send-key enter`. |

Canonical read-only message lifecycle inspection:

```bash
wt msg list --agent agents/codex
wt msg read --agent agents/codex <message-id>
```

`wt msg list` scans the canonical state directories without claiming, acknowledging, reclaiming, or
poisoning messages. Its counts use the visible directory state for `new`, `claimed`, `delivered`,
`retry`, and `failed`; invalid records are included in those state counts and also reported as
invalid diagnostics instead of being hidden. Rows include scope, attempts, claim owner, lease expiry,
last error, and summary when the record can be parsed.

`wt msg read` reads one exact message id from the same lifecycle directories without mutating it.
If the same id exists in multiple lifecycle directories, the command fails rather than guessing the
intended record. `--json` is supported for both inspection commands and uses the same read-only
inventory model. This is message inventory; runtime observation stays under `wt agent status` and
`wt agent watch`.

## Agent Adapter Policy

`wt`는 Git에 commit되는 source를 자동으로 바꾸지 않는다.

Agent adapter가 자동으로 쓸 수 있는 곳:

- `<repo-root>/.wt/...`
- per-worktree Git exclude file (`git rev-parse --git-path info/exclude`)
- 이미 untracked이고 excluded인 adapter file

Agent adapter가 기본으로 수정하면 안 되는 곳:

- tracked `CLAUDE.md` 또는 `AGENTS.md`
- tracked `.gitignore`
- tracked `.claude/settings.json` 같은 agent config
- 그 밖의 tracked source file

Tracked agent instruction file이 이미 있으면 agent가 지원하는 local override를 쓰거나,
명시적 opt-in 명령을 요구하거나, clear guidance와 함께 실패한다. Tracked file을 silent patch
하지 않는다.

### Per-Machine Setup

Per-machine setup의 canonical surface는 한 명령이다.

```bash
wt setup
wt setup --remove
```

`wt setup`은 한 사용자/한 머신에 필요한 wt integration만 다룬다. 현재 step은 Claude
user-level settings hook, Codex user-level hook/trust state, shell integration eval line,
그리고 Homebrew가 아닌 설치에서 shell completion eval line이다. 각 write step은 `[y/N]`
default No prompt를 거치며, `--yes`는 감지된 step을 모두 수락하고, `--dry-run`은 쓰지 않고
변경 의도만 출력한다.

`wt setup --remove`는 wt-managed per-machine entry만 제거한다. `<repo-root>/.wt/`,
`.wt.toml`, worktree, tracked source, wt binary는 제거 대상이 아니다. 사용자가 작성한 hook,
cmux hook, unrelated Codex trust state는 보존한다.

Shell rc target은 zsh에서 `$ZDOTDIR/.zshrc`를 우선하고 `$ZDOTDIR`가 없으면 `~/.zshrc`를
쓴다. bash는 `~/.bashrc`를 쓴다. Unsupported shell이면 eval line을 직접 넣으라는 안내만
출력하고 파일을 만들지 않는다. Homebrew prefix 아래의 `wt`는 formula가 completion을 제공하는
것으로 보고 completion eval line을 쓰지 않는다.

이전 per-machine setup-shaped surface는 compatibility alias나 hidden deprecation path로 남기지
않는다.

### Claude Hook Adapter

Claude Code inbox polling adapter는 `wt setup`의 내부 step이다. 이 step은 user-level
`$CLAUDE_HOME/settings.json` 또는 `~/.claude/settings.json`에 `UserPromptSubmit`,
`PostToolUse`, `SessionEnd` hook dispatcher를 추가한다. inbox event의 Hook command는
`wt msg check-inbox --hook-event-name <event> --silent`를 non-blocking shell wrapper로
실행한다. 성공한 delivery의 stdout hook JSON은 그대로 보존하고, 실패 stderr는 agent UI에
드러내지 않으며 command status는 0으로 끝난다. Receive identity는 `check-inbox` 계약에 따라
`WT_AGENT_ID`, current live identity anchor, no-op 순서로 결정된다.

```bash
wt msg check-inbox --hook-event-name UserPromptSubmit --silent 2>/dev/null || true
```

Generated command string은 wt-managed entry를 구분하기 위한 marker를 `#` 뒤에 둔다.
Claude Code가 shell command로 실행할 때 `#` 뒤 marker는 shell comment로 처리되므로
marker는 `wt msg check-inbox`의 인자로 전달되지 않는다. Hook event name은 marker 앞의
hidden/internal 인자로 전달되며, 이 동작은 CLI integration test로 검증한다.

Reinstall은 managed event마다 wt-managed dispatcher hook을 하나씩만 남기는 idempotent
operation이다. `SessionEnd`에는 owned supervisor cleanup hook을 하나만 남긴다.
`wt setup --remove`는 wt-managed Claude hook entry만 managed event별로 제거하고,
사용자가 작성한 다른 Claude hook이나 settings key는 보존한다. Repo-local tracked
`CLAUDE.md`, `AGENTS.md`, `.gitignore`, `.claude/settings.json` 같은 shared source/config는
per-machine setup이 수정하지 않는다.

### Codex Hook Adapter

Codex inbox polling adapter는 `wt setup`의 내부 step이다. 현재 Codex는 project-local `.codex/hooks.json` discovery를
신뢰할 수 없으므로 user-level `$CODEX_HOME/hooks.json` 또는 `~/.codex/hooks.json`에만
`UserPromptSubmit`과 `PostToolUse` hook dispatcher를 추가한다. User-level hook은 특정 agent
id에 영구로 묶이면 안 된다. 두 event의 기본 hook command는
`wt msg check-inbox --hook-event-name <event> --silent`를 non-blocking shell wrapper로
실행한다. 성공한 delivery의 stdout hook JSON은 그대로 보존하고, 실패 stderr는 agent UI에
드러내지 않으며 command status는 0으로 끝난다. Receive identity는 `check-inbox` 계약에 따라
`WT_AGENT_ID`, current live identity anchor, no-op 순서로 결정된다.

```bash
wt msg check-inbox --hook-event-name PostToolUse --silent 2>/dev/null || true
```

Codex는 user-level custom hook을 실행하기 전에 matching trust state를 요구한다. `wt setup`은
Codex가 쓰는 hook identity와 같은 방식으로 `trusted_hash`를 계산해
`$CODEX_HOME/config.toml` 또는 `~/.codex/config.toml`의 `[hooks.state]` 아래에 쓴다. Hash
identity는 event key `user_prompt_submit` 또는 `post_tool_use`, normalized command handler,
default timeout `600`, `async = false`를 canonical JSON으로 정렬한 뒤 SHA-256으로 계산한다. `wt setup`은
`[features].hooks = true`도 보장한다.

`wt run issue`, `wt run task`, `wt run workflow`는 사용자가 매번 hook을 다시 설치하게 하지 않고, Codex launch 시 cmux
`new-workspace --command`에 먼저 제거된 legacy coordinator routing env를 clear한 뒤
`WT_AGENT_ID=agents/<branch_slug>`와 필요한 경우 `WT_TASK_RUN_ID`를 주입해야 한다.
`<branch_slug>`는 scoped message address의 `agents/<agent>` 한 segment 제약과
맞도록 path separator가 없는 값이어야 하고, `wt msg send --to <branch_slug>`와
`wt msg check-inbox --agent "$WT_AGENT_ID"`가 같은 inbox를 보아야 한다. Claude와 future agent
CLI도 wt가 process launch를 소유하는 경로에서는 같은 launch-time `WT_AGENT_ID` shape를 받아야
한다.

### Agent Runtime Wrapper

Hook adapter install은 capability setup이고, agent identity는 run/session launch의 책임이다.
Normal daily launch의 canonical surface는 agent CLI 이름을 그대로 쓰는 짧은 wrapper다.

```bash
wt codex
wt claude
```

이 두 명령은 현재 git branch에서 `<branch_slug>`를 계산해 `WT_AGENT_ID=agents/<branch_slug>`를
agent process에 주입하고, 제거된 legacy coordinator routing env는 child process에서 제거한다.
사용자가 직접 `codex` 또는 `claude`를 실행하면 wt는 환경변수를 주입하지 않는다. 이 경우 hook
dispatcher는 current live identity anchor가 있으면 그 anchor inbox를 읽고, anchor도 없으면
조용히 no-op 한다.

같은 worktree에서 여러 agent를 띄울 때는 첫 positional argument로 role을 명시한다.

```bash
wt codex @planner
wt claude @reviewer
```

Role launch는 `agents/<branch_slug>-<role>`을 사용한다. 예를 들어 branch
`alice/feat-add-schema`에서 `wt codex`는 `agents/feat-add-schema`, `wt codex @planner`는
`agents/feat-add-schema-planner`를 사용한다. Role launch는 같은 worktree의 default inbox를
소비하면 안 된다. 이 분리가 same-worktree multi-agent 충돌 방지 contract다.

수동으로 agent id를 지정해 agent command를 실행하는 low-level escape hatch는 다음 형태다.

```bash
wt as <agent-id> -- <command...>
wt as agents/coordinator -- codex
```

`wt as`는 explicit `WT_AGENT_ID`를 그대로 쓰는 low-level wrapper다. 긴
`wt agent shell --agent <agent> ...` 형태를 최종 UX로 문서화하지 않는다.

### Cross-Agent Hook Smoke

Canonical non-LLM smoke는 실제 Claude/Codex 세션을 CI에서 띄우지 않고, 설치된 hook dispatcher와
file inbox만 검증한다.

1. `wt init`으로 repo-local personal storage와 config bootstrap을 준비한다.
2. 같은 git common dir을 공유하는 linked worktree 두 개를 만든다.
3. `wt setup`으로 Claude user-level dispatcher와 Codex user-level dispatcher를 설치한다.
4. `wt as agents/claude-smoke -- wt msg send --to agents/codex-smoke CLAUDE_SENT`로 Claude identity에서 Codex inbox로 보낸다.
5. 설치된 Codex hook command를 `WT_AGENT_ID=agents/codex-smoke`로 실행해 `CLAUDE_SENT`를 delivery한다.
6. `wt as agents/codex-smoke -- wt msg send --to agents/claude-smoke CODEX_SENT REALWT_PONG_SEEN`로 답장한다.
7. 설치된 Claude hook command를 `WT_AGENT_ID=agents/claude-smoke`로 실행해 `CODEX_SENT REALWT_PONG_SEEN`를 delivery한다.
8. `wt setup --remove`로 wt-managed per-machine hook state를 정리한다.

이 smoke는 cmux workspace/surface를 만들거나 읽지 않는다. Real Claude/Codex manual smoke는 같은
message path를 실제 agent lifecycle에서 관찰하는 추가 검증이지, message transport의 canonical
요구사항은 아니다.

Reinstall은 managed event마다 wt-managed Codex dispatcher hook을 하나씩만 남기는 idempotent
operation이다. `wt setup --remove`는 wt-managed Codex `UserPromptSubmit`/`PostToolUse` dispatcher entry와
matching trust state만 제거한다. cmux가
설치한 Codex hook, 사용자가 작성한 다른 Codex hook, 다른 `hooks.state` entry는 보존한다.

## Canonical Interfaces

이 섹션은 구현이 향해야 할 interface boundary를 적는다. 구체적 CLI와 schema는 해당 기능의
구현 PR에서 strict contract로 승격한다.

Runtime integration은 core runtime과 optional capability를 분리한다. cmux는 현재 중요한
surface이지만 canonical TaskDocument, Workflow, TaskRun, Message state가 아니다. Screen
capture는 headless runtime이 지원하지 않을 수 있으므로 core runtime capability가 아니다.

Provider integration은 external system capability별로 나눈다. Issue, PullRequest, Review는
서로 다른 capability이며, GitHub처럼 모두 지원하는 provider도 있고 Linear/Jira처럼 issue만
지원하는 provider도 있다. 하나의 overbroad provider trait에 모든 책임을 넣지 않는다.

## Principles

### One Concept, One Name
*North star: [Harness Principles](north-star.md#harness-principles).*

같은 개념은 하나의 이름으로만 표현한다.

예를 들어 실행환경을 뜻하는 개념은 `profile`이어야 한다. 같은 의미를 `variant`,
`runtime`, `kind`, `driver` 같은 이름으로 다시 만들지 않는다.

이름이 바뀌어야 한다면 기존 이름과 새 이름을 오래 공존시키기보다, 왜 새 이름이 더
정확한지 정리하고 한쪽으로 수렴한다.

### Different Concepts Stay Separate
*North star: [Harness Principles](north-star.md#harness-principles).*

다른 개념은 명령, 옵션, config, 상태 저장에서도 분리한다.

`init`은 이 repo에 맞는 project-specific config recommendation을 만드는 개념이다. 먼저
repo manifests, CI, docs, 기존 config, local tool availability를 보고, 개발에 필요한 active
section과 명시적으로 선택한 integration만 하나의 config plan으로 만든다.

`profile`은 어떻게 실행할지에 대한 개념이다. `init`이 명시 선택된 단순 agent runtime을
`[profile.agent]`로 쓸 수는 있지만, agent runtime을 구조화하고 재사용하는 책임은 계속
`profile`에 둔다.

`workflow`는 `<repo-root>/.wt/execution/workflows` 아래에 저장되는 prepared execution plan이다.
사용자는 어떤 task들을 어떤 실행 shape로 시작하고 이어갈지를 workflow로 다룬다.
Canonical command surface는 `wt workflow`다.

Workflow는 정확히 하나의 `mode = "single" | "batch" | "stack" | "matrix"` 값을 가진다.
`mode`는 실행 shape만 고른다. `single`은 하나의 branch workspace에서 하나 이상의
TaskDocument를 실행하고, `batch`는 같은 base에서 여러 독립 branch를 실행하며,
`stack`은 task branch들을 정해진 parent chain으로 순서대로 실행하고, `matrix`는 하나의
TaskDocument를 named profile 목록으로 확장한다.

`batch`, `stack`, `matrix`는 workflow mode 값으로 남지만 top-level 상태 파일 noun이나 command
namespace가 아니다. 새 상태 파일은 `<repo-root>/.wt/execution/workflows` 아래에만 만들고, batch/stack
전용 상태 디렉터리는 새 코드가 읽거나 쓰는 상태 위치가 아니다. 별도 top-level command
surface를 `wt workflow` 옆에 남기면 두 command surface가 모두 canonical처럼 보이므로
새 CLI parser와 dispatch는 canonical `wt workflow`만 노출한다.

Workflow file은 `title`, `body`, optional workflow-level `[origin]`, mode, base,
profile, color, timestamps, workflow-level policy, task/run link 같은
prepared-plan context와 orchestration만 저장한다. `title`은 list/select/show 표면의
짧은 display text이고, `body`는 큰 issue context, requirements, acceptance criteria,
planning notes, decomposition rationale을 담는 긴 human context다. Workflow-level
`[origin]`은 Workflow 자체가 provider issue에서 온 경우 durable provider link를
저장한다. 이 origin은 larger issue-like unit의 출처이고, child TaskDocument의 provider
origin처럼 실행 slice를 provider issue로 취급하게 만드는 값이 아니다. Workflow는 여전히
prepared execution plan이며 parent TaskDocument나 nested-task container가 아니다. Task
branch name의 source of truth는 항상 TaskDocument의 `branch`다.
Workflow task row는 TaskDocument branch, status, body, origin을 복사해 저장하지 않는다.

Workflow color는 같은 workflow가 연 cmux workspace들을 시각적으로 묶는 표시다. 색상이
생략되면 `wt`가 내장 cmux named-color palette의 다음 색을 고르고 workflow file에
기록한다. 색상은 mode나 task의 의미가 아니라 workflow-level 표시다.

`wt config` 출력은 runtime behavior를 판단하는 effective source of truth다. `.wt.toml`,
`<repo-root>/.wt/config/local.toml`, profile file은 사용자 intent와 override를 저장하고, `wt config`는
merge된 layer, convention file, built-in default를 사용자가 복사해 수정할 수 있는 형태로
보여준다. 명령 구현은 user-facing default를 각 call site에서 새로 해석하지 말고 config
모델의 effective accessor나 effective policy snapshot을 거쳐 적용해야 한다.
활성 section의 runtime default는 `wt config` 출력에 materialize한다. 예를 들어 active
`[site]` provider는 name/root/secure/url과 Traefik target default까지 보여주고,
`[workspace.browser]`는 setup/open 때 browser를 띄울지와 어떤 URL을 열지 결정한다.
`[workspace.browser.chrome_devtools]`는 `mode = "chrome_devtools"`일 때만 쓰는 Chrome
DevTools launch detail(`port`, `user_data_dir`)을 소유한다. 이 detail은 browser mode의
하위 설정이지 `[workspace]`의 형제 section이 아니다. 예전 `[workspace.chrome_devtools]`는
canonical path가 아니며, 자동 migrate나 alias 없이 새 위치로 옮기라는 parse error로 거부한다.
`[site]`는 `site_url`을 만들고, browser launch policy를 소유하지 않는다. active `[editor]`
설정은 생략된 placement의 `cmux_surface` default를 보여준다. cmux에서 이 default는
caller surface 오른쪽 split pane에 editor를 연다. 반대로
`provider = "none"`처럼 feature가 inactive인 section은 effective output에 unrelated section으로
내보내지 않는다.

### Config Source Namespace

`wt config edit`, `wt config extract`, `wt config inline`의 typed `SOURCE`는 wt-managed
config 파일의 닫힌 네임스페이스만 받는다. 유효한 shorthand는 다음 세 형태뿐이다.

| shorthand | canonical target |
|---|---|
| `shared` | `.wt.toml` |
| `local` | `<repo-root>/.wt/config/local.toml` |
| `profiles/<name>` | `<repo-root>/.wt/config/profiles/<name>/profile.toml` |

`wt config edit SOURCE`는 위 managed target이 아직 없어도 받는다. Parent directory를 준비한 뒤
editor가 해당 파일을 만들 수 있게 한다. `wt config extract SOURCE`와 `wt config inline SOURCE`는
refactor 대상이므로 이미 존재하는 managed file만 받는다. 이 파일들을 가리키는 절대 경로나
repo-relative canonical path도 같은 대상으로 받는다.

그 외 경로, 오타, bare profile name(`codex` 같은 이름)은 editor나 refactor 작업 전에
비0 종료로 거부하고, 에러에는 유효한 대상 목록과 `SOURCE` 없이 실행하면 selector를 쓸 수
있다는 안내를 포함한다. `SOURCE` 없이 실행하는 selector는 기존 managed config file을 보여주며,
아직 아무 파일도 없을 때는 personal `local` target을 기본 생성 대상으로 삼는다.

Profile prompt convention file은 `SOURCE` 네임스페이스가 아니다. Prompt file을 inline할 때도
`profiles/<name>` 또는 해당 `profile.toml`을 `SOURCE`로 넘긴 뒤 그 profile 안의 prompt file
candidate를 선택한다.

`[workspace].colors`는 workspace를 시작하는 command surface의 기본 cmux 색상이다.
Canonical 색상 key는 `task`, `issue`, `branch`, `pr`이다. `task`는 TaskDocument에
`[origin]`이 있는지와 무관하게 즉시 실행 표면인 `wt run task`에 대응한다. `issue`는
직접 provider issue에서 시작하는 `wt run issue`, `branch`는 branch-name text에서 시작하는
`wt run branch`, `pr`은 pull request branch를 여는 `wt run pr`에만 대응한다. 이 key들은 prompt
setup mode, profile 이름, workflow `mode = "single" | "batch" | "stack" | "matrix"`
값이 아니다.
Workflow run은 필요한 경우 TaskDocument setup을 거치더라도 최종 visible grouping color는
저장된 `workflow.color`를 적용한다. `[workspace].colors` key를 생략하면 내장 기본값
`task = "blue"`, `issue = "blue"`, `branch = "green"`, `pr = "magenta"`를 쓴다.
`wt config`는 이 effective 색상값을 출력하므로 사용자가 수정할 기준은 `wt config` 출력이다.
`wt init`은 generated config의 active `[workspace]`에 이 기본 `colors = { ... }` map을 명시한다.
기존 설정을 다시 init할 때는 기존 color override를 effective 값으로 보존한다. 색을 아예 쓰지 않을
kind는 `task = ""`처럼 빈 문자열로 override한다.

`matrix`는 하나의 local TaskDocument를 명시한 named profile 목록으로 확장하는 saved
Workflow mode다. Current `mode = "matrix"` contract는 exactly one task x many named profiles만
허용한다. `batch`나 `stack`처럼 여러 task 자체를 뜻하지 않고, profile 축으로 여러
profile-specific TaskRun/worktree를 만드는 실행 형태다. `wt workflow task --mode matrix
--profiles <name>[,<name>...] <task>`가 canonical creation surface이고, profile list는
Workflow TOML의 `profiles = [...]`에 사용자가 넘긴 순서로 저장한다. `--profiles`는
repeatable할 수 있지만 `--mode matrix` 없이 쓰면 안 되고, `--profile`과 동시에 쓰면 안
된다. Unknown profile, duplicate profile, reserved `default` profile name은 worktree,
TaskRun, Workflow 파일을 만들기 전에 실패해야 한다. 수동 Workflow TOML도 `mode =
"matrix"`에서 task가 1개가 아니거나 `profiles`가 비어 있거나 task row가 profile별
`[[tasks.runs]]`를 정확히 저장하지 않으면 invalid state로 거부한다.

Direct `wt run task`는 immediate TaskDocument execution path다. `wt run task [<task>...]`와
`wt run task [<task>...] --profile <name>`만 소유하고, selected TaskDocument마다
하나의 worktree를 시작한다. Profile fan-out은 소유하지 않는다.
Direct issue/branch 경로의 all-named-profiles legacy behavior는 보존한다. Selected
profile subset은 direct command가 아니라 Workflow matrix가 소유하며,
`wt workflow task --mode matrix --profiles ...`로 표현한다.

`wt run` namespace는 workspace execution start만 소유한다. Canonical start source는
`issue`, `pr`, `branch`, `task`, `workflow` 다섯 가지뿐이다. Cleanup은 `wt done`,
inspection은 `wt inspect`, existing branch/worktree opening은 `wt open`, agent
observation은 `wt agent status` / `wt agent watch`, workflow file lifecycle, repair,
pass는 `wt workflow`가 맡는다. `wt run` 아래에 cleanup, inspect, repair, pass,
status/watch 같은 in-flight transition을 추가하지 않는다.

`wt run branch <words...>`는 branch-name text에서 바로 ad hoc worktree를 시작한다. 즉시
준비된 TaskDocument를 실행하는 표면은 `wt run task [<task>...]`이다. 여러
TaskDocument를 하나의 저장된 실행 계획으로 묶어 batch coordination을 해야 하면
`wt workflow task --mode batch`와 `wt run workflow`를 쓰고, 하나의 shared workspace에서
실행해야 하면 `wt workflow task --mode single`과 `wt run workflow`를 쓴다. `wt run branch`에
prepared-task 실행 의미를 계속 넓히면 ad hoc branch worktree, immediate task run,
saved workflow가 한 명령에서 섞인다.

`wt run branch`는 existing branch를 여는 명령이 아니다. 사용자가 넘긴 branch-name text로
새 ad hoc branch/worktree를 만드는 start surface이며, 이미 존재하는 branch나 worktree를
열 때는 `wt open <branch|worktree>`를 쓴다.

`wt open`은 issue selector가 아니라 branch/worktree 상태 selector다. 선택지는 현재
checkout을 제외하고 `existing`(이미 별도 worktree가 있음), `local`(local branch만
있음), `remote`(origin branch만 있음)으로 나뉜다. Linear나 GitHub issue 번호를 추정해
분류하지 않는다. issue provider가 제안한 branch와 `worktree.naming`으로 만든 branch가
다를 수 있기 때문이다.

이 개념들이 섞이면 사용자는 workflow가 실행환경인지, mode가 상태 파일 noun인지,
profile이 작업 묶음인지 다시 추론해야 한다. 이런 혼동은 기능 추가보다 먼저 제거한다.

### Config Merge Semantics

`wt config`는 `.wt.toml` (shared) → `<repo-root>/.wt/config/local.toml` (personal) →
named profile (`<repo-root>/.wt/config/profiles/<name>/`) 순서로 layer를 합쳐
effective config을 만든다. 같은 코드 경로(`merge_config`)가 모든 layer에 적용되므로
layer 차이로 동작이 달라지지 않는다. 다만 섹션마다 합치는 방식이 다르고, 그 차이를
사용자가 예측할 수 있어야 한다. 다음 표가 canonical 규칙이다.

| 섹션 / 필드 | 동작 | 의미 |
|---|---|---|
| `worktree.path`, `worktree.inject_local_context`, `worktree.naming` | REPLACE (later wins if set) | 단일 값. 윗 layer가 명시하면 아랫 layer를 덮어쓴다. |
| `worktree.copy`, `worktree.link`, `workspace.tabs`, `workspace.post_deps_tabs` | extend, value-level dedupe | 윗 layer가 항목을 추가한다. 같은 문자열은 한 번만 나온다. |
| `worktree.copy_as` | extend, `(from, to)` 쌍 dedupe | 같은 from/to 쌍은 한 번만. 다른 from이면 둘 다 살아남는다. |
| `setup.deps` | extend (현재 dedupe 없음) | 같은 dep을 두 layer가 적으면 두 번 실행된다. dep script는 idempotent하게 짠다. |
| `setup.env`, `setup.env_files[path]`, `workspace.colors` | HashMap extend (key-level overwrite) | 같은 key를 윗 layer가 덮어쓴다. |
| `workflow.pull_request`, `workflow.landing`, `review.codex_base`, `editor.command`, `editor.placement`, `workspace.browser` | REPLACE if Some | Option 필드. 윗 layer가 set하면 덮어쓴다. `workspace.browser.chrome_devtools`는 `workspace.browser`의 하위 설정이므로 browser section과 함께 교체된다. |
| `workspace` (Option 섹션) | deep-merge (both Some) | 두 layer가 모두 `[workspace]`를 가지면 필드별로 위 규칙대로 합친다. |
| `site`, `issues` (Option 섹션) | wholesale REPLACE if Some | 윗 layer가 `[site]`/`[issues]`를 가지면 아랫 layer의 같은 섹션이 통째로 사라진다. 한 필드만 바꾸려면 base의 모든 필드를 다시 적는다. |
| `agent.{cli, args, command, ready, submit, timeout, send_after}` | per-field presence-based REPLACE | 윗 layer가 명시한 필드만 덮어쓴다. |
| `agent.prompt[mode]` | REPLACE per mode unless `[agent.prompt.append].<mode>` | 같은 mode를 적으면 덮어쓴다. append-key form은 기존 prompt에 `\n\n`으로 이어붙인다. |

Named profile에는 profile.toml 외에 두 가지 convention이 더 있다.

| 위치 | 동작 |
|---|---|
| `<profile>/scaffold/` | 디렉토리 존재만으로 `copy_as = [{from: <abs>, to: "."}]` 자동 push. 워크트리 생성 시 워크트리 루트에 복사된다. |
| `<profile>/prompts/<mode>.md` | `agent.prompt[mode]`를 **REPLACE**. profile.toml의 inline `[agent.prompt].<mode>`가 같이 있으면 파일이 이기고 stderr에 warning이 찍힌다. |
| `<profile>/prompts/<mode>.append.md` | `agent.prompt[mode]`에 append. inline replace와 충돌이 아니다. |

`[profile] name = "<name>"`이 `.wt.toml`이나 `local.toml`에 없고 CLI `--profile`도 없으면
profile convention(`scaffold/`, `prompts/`)은 effective config에 합쳐지지 않는다. 함정을
피하려면 둘 중 하나는 명시한다.

`[profile] name`과 inline `[profile.agent]`, `[profile.worktree]`, `[profile.setup]`,
`[profile.workspace]`, `[profile.site]`를 같이 쓸 수 없다. parse 단계에서 hard error로
막는다. 한 곳만 골라서 적는다.

### Omission Means Default Behavior
*North star: [Direction-Driven Design](north-star.md#direction-driven-design).*

생략은 기본 동작을 뜻한다. 생략을 특정 이름으로 저장하거나 노출하지 않는다.

`default`는 profile 이름이 아니라, 사용자가 profile을 명시하지 않았을 때 적용되는
선택 규칙이다. 따라서 `default`를 실제 profile 이름처럼 다루면 안 된다.

기본값은 편의 기능이지만, 이름 있는 리소스처럼 보이는 순간 UX 부채가 된다.

### Ambiguity Fails Early
*North star: [Direction-Driven Design](north-star.md#direction-driven-design).*

애매한 조합은 추론하지 말고 거부한다.

예를 들어 direct `--profile`은 “하나의 profile 선택”을 뜻하고, `wt workflow task --mode
matrix --profiles`는 “명시한 profile subset을 저장된 workflow로 확장”을 뜻한다.
`--profile`과 `--profiles`, direct `wt run task --matrix`, matrix workflow의 여러 task처럼
이 조합들이 충돌하면 임의로 우선순위를 정하지 않는다.

명령은 사용자가 의도를 잘못 표현했을 때 조용히 다른 일을 해서는 안 된다. 빠르게
실패하고, 어떤 선택을 해야 하는지 알려줘야 한다.

### Help Text Is a Contract
*North star: [Harness Principles](north-star.md#harness-principles).*

`--help`에 보이는 설명은 실제 동작과 같아야 한다.

도움말에 보이는 명령은 실제로 지원되어야 하고, 숨겨진 의미를 알아야만 사용할 수 있는
옵션은 없어야 한다. 옵션 설명이 “무엇을 하는지”가 아니라 “언제 어떤 개념을 선택하는지”
를 설명해야 한다.

도움말을 읽고 생긴 기대와 실제 동작이 다르면 구현이 아니라 UX가 깨진 것이다.

Interactive prompt도 CLI contract다. 사용자가 값을 생략해서 selector가 열리는
command는 무엇을 고르는지, 한 개를 고르는지 여러 개를 고르는지, 빈 선택이 허용되는지
문서와 help text에서 같은 말로 설명해야 한다. Selector는 작은 inline terminal prompt로
동작한다. Full-screen TUI가 아니며, prompt header/footer/filter input/summary를 제외한
selector body는 기본 10개 visible row 안에서 bounded list로 보여준다. Section header와
group spacing도 visible row cap을 소비하므로 grouped selector는 스크롤 중 frame height를
유지한다. 명령이 scriptable
target argument를 지원하면 non-TTY, `--json`, `--quiet` automation에서는 selector를 열지
말고 그 explicit argument path를 요구한다.

Canonical selector row model은 두 row type이다.

- Section row: group title과 optional hint를 보여주는 non-selectable header다. Value/index가
  없고, focus를 받을 수 없으며, space/enter submission 대상이 아니다. Filtering 뒤 selectable
  option이 하나도 남지 않은 section은 보여주지 않는다.
- Option row: command가 반환할 value/index를 가진 selectable row다. Option은 concept
  `label`, optional `hint`/metadata, optional search-only text, selected/disabled state를
  별도 field로 가진다.

Selector rendering은 label과 metadata를 섞어서 하나의 layout string으로 만들지 않는다.
Label은 task, branch, PR, workflow, profile, config section 같은 현재 concept의 이름이다.
Hint/metadata는 provider id, status, branch, path, profile, policy preview처럼 같은 선택지를
구분하는 보조 정보다. Hint가 있는 row는 같은 prompt page 안에서 hint column을 맞춰 보여줄
수 있지만, filter 대상은 padding이 아니라 label, metadata, search-only text다. Metadata가
없는 plain label selector에는 가짜 column을 만들지 않는다.

Selector filtering은 현재 visible text의 모양이 아니라 row data를 대상으로 한다. Typing은
filter text를 갱신하고, backspace는 filter text를 한 character 지우며, filter가 바뀌면 active
focus는 첫 matching selectable option으로 이동한다. No-match 상태에서는 option focus가
없어야 하며 enter/space가 hidden stale option에 적용되면 안 된다. Section header는 matching
대상이 아니라 matching option의 context다.

Keyboard behavior는 prompt마다 다시 해석하지 않는다.

- Up/down arrows move active focus to the previous/next selectable option in filtered order and
  skip section rows. They stop at the first/last selectable option unless a future selector contract
  explicitly adds wrapping.
- Space toggles the active option only for multiselect prompts. It never toggles a section header.
- Enter submits the active option for single-select. For multiselect, Enter submits the current
  selected set; when that set is empty and an active selectable option exists, Enter first selects
  that active option and submits it as a one-item selection. If no active option exists, whether an
  empty multiselect is valid is command-specific and must be documented at that command.
- Escape and ctrl-c cancel cleanly, restore terminal state, and surface the same cancelled prompt
  error shape as other wt prompt cancellation.
- Backspace edits the filter text and must not alter selection state.

When filtered rows exceed the visible cap, the selector shows hidden-before/hidden-after context
using text or stable symbols such as `↑ N more` and `↓ N more`; color can strengthen this but cannot
be the only cue. Hidden counts count selectable option rows outside the current visible window, not
decorative section lines. Multiselect row state must remain readable without color: active,
selected, unselected, disabled, and cancelled/submitted states need stable text or symbols. Grouped
or long-list multiselects show a `Selected:` summary when that materially improves comprehension;
compact ungrouped multiselects do not show that summary by default. Summary text uses selected
labels and may collapse long selections with a `+N more` suffix.

Reference UI evidence for this contract:

- `vercel-labs/skills` `src/prompts/search-multiselect.ts` owns a custom row model, raw key handling,
  search input, hidden before/after counts, and selected summaries. Its locked section is useful
  evidence for non-option context, but wt should model ordinary section headers separately from
  always-selected locked values.
- `vercel-labs/skills` `src/list.ts` groups non-interactive inventory output by plugin. This supports
  sectioned scanning as a presentation pattern, but it is not an interactive selector API.
- Cliclack 0.5.4 `Select`/`MultiSelect` store only option items with `label`/`hint`; theme hooks can
  format rows, but filtering and submission are still option-list shaped. Section headers would be
  fake selectable items or require adapter hacks.
- Inquire 0.9.4 `Select`/`MultiSelect` provide filtering, paging, scorers, defaults, validators, and
  `ListOption { index, value }`, but the public model is still a non-empty `Vec<T: Display>` of
  selectable options.
- Dialoguer 0.12.0 `Select`/`MultiSelect` provide option items, defaults, paging, and optional
  cancellation surfaces; they do not provide first-class non-selectable section rows. Its
  `FuzzySelect` is single-select only and still filters a string option list.

### Progressive Disclosure
*North star: [Persona](north-star.md#persona).*

처음 쓰는 경로는 짧아야 하고, 복잡한 경로는 필요해질 때 드러나야 한다.

간단한 실행환경은 작게 시작할 수 있어야 한다. prompt, scaffold, agent별 파일처럼
복잡한 요소가 필요해질 때 더 구조화된 profile로 옮겨갈 수 있어야 한다.

이때 중요한 것은 두 경로가 다른 개념처럼 보이지 않는 것이다. 단순한 형태와 복잡한
형태는 같은 profile 모델의 두 표현이어야 한다.

`wt init`은 단순히 작은 config 파일을 쓰는 명령이 아니라 project-specific config
recommendation wizard다. Interactive TTY에서 bare `wt init`은 config target을 고른 뒤 repo를
스캔해 추천 config plan을 만들고, workflow PR/landing policy를 starter config에 반영하며,
setup deps, tests, workspace tabs, editor, agent runtime, agent prompt, issue provider, site provider
같은 항목을 필요한 만큼만 guided flow로 묻는다. 쓰기 전에는
어떤 target file에 어떤 작업으로 어떤 config section이 생성될지 명확히
보여주고 확인을 받아야 한다.

Wizard step label은 구현 단계명이 아니라 사용자가 지금 결정하는 의미를 말해야 한다.
`wt init`의 사람이 읽는 설명과 prompt는 한국어를 기본으로 쓰되, command, option, config key,
TOML value 같은 protocol literal은 영어 원문을 유지한다. Canonical flow는 `설정 파일 위치`,
`외부 도구 연결`, `개발 환경 설정`, `미리보기`, `쓰기 확인` 순서다.
각 step 시작 전에는 빈 줄을 두고, step 설명은 prompt header와 구분되도록 들여써서 보여주며,
작은 대비쌍은 bullet로 나눈다. 설명과 step 안의 질문 사이에도 빈 줄을 둔다. Step 안의 질문은
새 `◆` header를 쓰지 않고 두 칸 들여쓴 field label과 selector frame으로 보여줘서 parent step에 속한 결정임을 드러낸다.
Detected integration이 없어서 prompt가 생략되는 step도 “감지된 signal이 없어 section을 쓰지
않는다”는 의미를 보여줘야 한다. 선택지가 작고 검색할 대상이 아닌 결정은 filter 없는 selector를 쓴다. Target 결정은
`개인 설정 파일`과 `팀 공유 설정`이라는 선택지로 보여주며, 예/아니오 prompt로 두 개념을 숨기지 않는다.
소유 범위 설명만으로 모호하면 target hint에 실제 위치를 보여준다. 개인 설정 파일은
`<repo-root>/.wt/config/local.toml`이고 보통 `./.wt/config/local.toml`에 해당한다는 예시를 같이 보여준다.
팀 공유 설정은 `./.wt.toml`로 보여준다. 실제 절대 경로는 preview에서
보여준다. 개발 환경 설정 결정은 `감지한 개발 설정 저장`,
`기존 설정 파일 값 유지하기`, `개발 설정 직접 고르기`, `자동화 없이 최소 설정` 같은
선택지 중 하나로 고르게 한다. 선택 label만으로 결과가 분명하지 않으면 label 아래에 흐린 설명을
한 줄 들여써서 보여주고, 설명이 붙은 선택지 블록 사이에는 빈 `│` 줄을 둔다. 선택되지 않은
label도 설명보다 낮은 계층으로 보이지 않게 본문 색상으로 유지하고, marker와 설명만 보조색으로 둔다.
Selector가 submit되면 단순 `Submitted`가 아니라 사용자가 고른 label과 필요하면 hint를 남겨서
이전 결정이 wizard transcript에 보이게 한다.
감지한 command를 저장할지 묻는 confirm은 먼저 실제 감지값을 한 줄 또는 bullet 목록으로 보여준다.
`workspace tabs` 같은 config 개념은 prompt label로 그대로 노출하지 말고, `worktree 열 때 같이 띄울 명령`
처럼 사용자가 보는 동작으로 말한다.
`worktree.path`도 `기본 형제 폴더`처럼 구현 위치 관계를 줄여 말하지 말고,
`현재 저장소 옆에 만들기`와 `../{{default_name}}`처럼 사용자가 예상할 수 있는 위치를 함께 보여준다.
감지된 issue/site provider도 감지값을 첫 선택지로 올린 작은 selector에서 고르게 한다. Preview는
저장할 파일, 저장 범위, 작업, 저장될 설정, 안내/경고, TOML만 보여준다. 감지된 signal 전체를
debug log처럼 반복하거나 `[ok] 감지됨` row로 나열하지 않는다. 팀 공유 설정 `.wt.toml`을 선택해서
`.env` copy, local link, browser profile 같은 private helper가 빠질 때만 omission을 안내로 설명한다.
Preview는 `저장 대상`, `안내`, `경고`, `생성될 TOML`처럼 사람이 구분해서 스캔할 수 있는 블록으로
나눠 보여준다. TOML block과 설명 block은 빈 줄과 경계선으로 분리해서 안내 문장이 config 내용처럼
보이지 않게 한다.
`cmux`가 감지되지 않으면 workspace tabs, `post_deps_tabs`, workspace browser 같은 cmux workspace
자동화를 추천 config에 넣지 않는다. `lazygit`과 `nvim`은 개인 local target에서만, `cmux`가 있고
해당 command도 있을 때 기본 workspace tab으로 추천한다. shared `.wt.toml`에는 이런 개인 보조 탭을
자동 추천하지 않는다. 자동화 없이 최소 설정은 `[workspace] tabs = []`만 저장한다.

Public starter preset은 canonical surface가 아니다. `minimal`, `agent`, `issue`, `app` 같은
bundle 이름을 고르게 하지 않는다. `--preset`과 `--minimal`은 primary help surface에 남기지
않고, 새 parser surface에서는 legacy 입력으로 실패한다.

`wt init --yes`는 non-interactive project recommendation을 받아들이는 자동화 경로다. Repo
manifest를 scan해 setup command와 dev tab 후보를 active config에 반영한다.
Issue/site integration은 explicit flag 또는 `.linear.toml`, Laravel app처럼 concrete repo
signal이 있을 때만 active config에 쓴다. Agent runtime은 explicit flag나 기존 config default가
있을 때만 쓴다. Interactive wizard에서는 agent runtime도 작은 selector로 물으며, agent를 선택하면
같은 target file 안의 inline `[profile.agent]`와 `[profile.agent.prompt]`에 starter prompt를 같이 쓴다.
이 starter prompt block은 active key의 의미를 짧은 TOML comment로 설명할 수 있지만, 비활성 section이나
대체 scaffold를 commented-out 예시로 생성하지 않는다.
이 inline prompt는 나중에 `wt config extract`나 `wt profile create`로 더 구조화된 profile/prompt file로
옮길 수 있는 같은 profile model의 단순 형태다. 개인 local target에서는 `.env` copy, 기본
`inject_local_context`, known local links, `worktree.naming`, Chrome DevTools browser 같은 local helper를
추천할 수 있지만 shared `.wt.toml`에는 private helper를 쓰지 않는다.
TTY가 아니면 `--yes` 또는 충분한 explicit flag 조합처럼 prompt 없이 끝낼 수 있는 입력이
있어야 하며, 그렇지 않으면 interactive prompt를 시도하지 말고 명확한 에러로 실패한다.
`wt init --dry-run`은 같은 validation을 거친 뒤 생성될 target, 작업, 저장될 설정, 안내/경고,
TOML content를 preview하고 파일을 쓰지 않는다. `.wt` directory와 git `info/exclude`도
수정하지 않는다.

Generated config output은 여전히 사용자가 선택한 config 파일 하나에만 쓴다. `.wt.toml`과
`<repo-root>/.wt/config/local.toml` 중 하나를 선택하고, 답한 설정은 그 파일에만 쓴다. 다른 config 파일,
named profile directory, prompt/scaffold 파일은 `wt init`의 부수 효과로 만들지 않는다.
그런 구조가 필요하면 `wt config extract`나 `wt profile create`로 드러낸다. 나중에 scaffold
generation을 추가하더라도 별도의 명시적 choice로 다뤄야 한다.

Repo bootstrap side effect는 config output과 별개의 `wt init` 책임이다. Apply 경로는
main `<repo-root>/.wt/` directory-like path(real directory 또는 directory symlink),
clone-local git `info/exclude`의 exact line `/.wt`, canonical core personal-state directory를
보장한다. 이미 준비되어 있으면
idempotent no-op이고, `/.wt` line을 중복 추가하지 않는다.

`wt init --help` contract도 이 모델을 따라야 한다. Subcommand 설명은 “start a
project-specific config recommendation wizard”를 말해야 하고, `--yes`, `--dry-run`, `--local`,
`--shared`, explicit integration flags는 recommendation automation, preview, target file,
selected integration을 설명해야 한다. Help text는 named profile directory, prompt/scaffold
file, commented tutorial scaffold를 자동 생성한다고 암시하면 안 된다.

Prompt도 같은 원칙을 따른다. `common`은 별도 실행 mode가 아니라 기존
`[agent.prompt]` / `[agent.prompt.append]` 모델 안의 공통 scope다. Config layer와
profile convention file merge를 모두 끝낸 뒤 최종 effective config에서 한 번만
`issue`, `branch`, `pr` prompt 앞에 펼친다. `common`을 각 layer마다 mode별 prompt로
복사하지 않는다.

`workflow`도 Workflow `mode = "single" | "batch" | "stack" | "matrix"`나 setup mode가 아니라
`[agent.prompt]` / `[agent.prompt.append]` 안의 workflow-started task 전용 scope다.
`wt run workflow`로 시작한 task에만 적용하고, direct `wt run task`, `wt run issue`,
`wt run branch`, `wt run pr`에는 적용하지 않는다. Workflow task의 setup mode는 계속
TaskDocument origin에 따라 `issue` 또는 `branch`를 사용하므로 기존 setup-mode prompt도
함께 적용된다. `common`은 `workflow`로 펼치지 않는다. Workflow task는 이미 `issue` 또는
`branch` prompt를 받기 때문에 `common`을 `workflow`에도 펼치면 같은 공통 지시가 중복된다.
Profile convention file은 `<repo-root>/.wt/config/profiles/<name>/prompts/workflow.md`와
`<repo-root>/.wt/config/profiles/<name>/prompts/workflow.append.md`를 같은 scope로 읽는다.

### State Is Explicit
*North star: [Scope Model Direction](north-star.md#scope-model-direction).*

저장되는 상태는 사용자가 이해할 수 있는 상태여야 한다.

TaskDocument는 작업이 무엇인지를 담는 실행 정의다. `<repo-root>/.wt/execution/tasks/<task>.toml`
아래에 title, branch, body, origin처럼 실행과 무관하게 읽을 수 있는 정보를 둔다. Spec이
있는 작업에서는 자세한 requirements/design/tasks prep artifact가
`<repo-root>/.wt/planning/specs/<slug>/`에 병렬로 존재할 수 있고, TaskDocument body는 그 경로를
가리키는 launch summary로 남는다.

`wt scaffold --task`가 만드는 TaskDocument body의 `계획 (Planning)` section은
agent에게 맡길 작업의 deterministic launch contract다. 새로 준비되는 TaskDocument는 최소한
expected duration, estimate basis, suggested watch cadence, blocked by/dependency,
execution shape, size class, acceptance checks를 적는다. Provider issue import처럼 외부
본문을 그대로 보존하는 TaskDocument는 이 section이 없을 수 있으므로 CLI는 단순 TOML read
중에 planning body를 자동 생성하거나 provider body를 덮어쓰지 않는다. `wt-ready` /
`wt-work` flow가 launch 전에 부족한 planning 정보를 채우는 소유자다.

`wt task list`는 `<repo-root>/.wt/execution/tasks/<task>.toml`에 저장된 TaskDocument file 중
actionable working set을 보여주는 canonical read-only list다. Bare `wt task list`는
`wt run task`의 selectable task semantics를 따른다. TaskRun이 없거나 latest TaskRun status가
`prepared`, `failed`, `skipped`인 TaskDocument를 보여주고, latest status가 `passed` 또는
`running`인 TaskDocument는 숨긴다. 숨겨진 TaskDocument가 있으면 text output은 count와
`wt task list --all` 안내를 보여주되 TaskDocument row를 dump하지 않는다. `wt task list --all`은
full TaskDocument inventory mode이며 passed/running TaskDocument까지 포함한다. 두 mode 모두
selector의 10-row visible cap을 적용하지 않는다. Text output은 selector와 같은 TaskDocument
display order인 title, origin/publish state, task key, branch를 bounded column으로 나눠
보여주고, `provider-origin`과 `local` source group 아래에 둔다. Inventory-only field인 source는
group으로 표현하고, path, raw origin, 짧은 body summary는 text에서 반복하지 않고 JSON output에
둔다. JSON output은 두 mode 모두 `{ "tasks": [...], "invalid_tasks": [...] }` top-level shape를
유지하며, TaskDocument의 key, path, title, branch, origin/publish state, local-vs-provider-origin
source, 짧은 body summary를 stable shape로 보여준다. Bare JSON은 actionable working set만
담고, `--all --json`은 full inventory를 담는다.
TaskDocument TOML parse/validation failure는 조용히 숨기지 않고 text warning 또는 JSON
`invalid_tasks`로 보고한다. `wt task list`는 worktree, local branch, TaskRun, Workflow,
provider issue, pull request, agent setup을 만들거나 수정하지 않는다. Workflow inventory는
계속 `wt workflow list`, worktree/branch/site state는 계속 `wt list`가 맡는다.

TaskDocument import는 configured issue provider의 기존 issue를 local task 정의로
가져오는 side effect다. Canonical command shape는 `wt task import` 또는
`wt task import <issue>...`다. Bare `wt task import`는 provider issue를
multi-select로 고르게 하고, 명시 issue id는 scriptable path로 남긴다. `import`는
`<repo-root>/.wt/execution/tasks/<safe-issue-id>.toml`에 title, branch, body, `[origin]`을 기록한다.
이때 branch는 `wt run issue <issue>`가 사용할 provider issue branch와 같은 값이어야 하며,
필요하면 provider branch를 먼저 materialize한다. GitHub에서는 linked branch가 없을 때
`gh issue develop`을 호출할 수 있다. Import는 provider branch materialization 외에는
worktree, local branch, TaskRun, Workflow, pull request, agent setup을 만들지 않는다.
Provider가 branch를 공급하거나 materialize할 수 없으면 branch가 빈 TaskDocument를 쓰지
말고 실패해야 한다. `[origin]`은 provider issue와의 durable link이지, 자동 동기화 계약이
아니다.

Import ambiguity는 local TaskDocument write 전에 실패해야 한다. Configured issue
provider가 없으면 실패한다. 같은 invocation 안의 duplicate issue id는 실패한다. Provider
조회 뒤 canonical issue id가 같은 task key로 수렴하는 경우도 실패한다. Import 대상
`<repo-root>/.wt/execution/tasks/<safe-issue-id>.toml`이 이미 있으면 local edits를 보존하기 위해 실패하고,
조용히 덮어쓰거나 merge하지 않는다. Replace/update가 필요하다면 별도의 명시 옵션과
help/test/documentation이 먼저 필요하다.

TaskDocument publish는 local task 정의를 configured issue provider의 issue로 만드는
side effect다. Canonical command shape는 `wt task publish` 또는
`wt task publish <task>...`다. Bare `wt task publish`는 아직 `[origin]`이 없는 local
TaskDocument를 multi-select로 고르게 하고, 명시 task key는 scriptable path로 남긴다.
`publish`는 각 task의 provider issue 생성, provider-keyed `branch` rewrite, 그리고
`<repo-root>/.wt/execution/tasks/<task>.toml`의 `origin` 업데이트가 모두 끝났을 때만 해당 task를
성공으로 보고한다. 일부만 끝난 상태를 성공으로 보고하지 않는다. `origin`은 external issue와의
durable link이지, 아직 publish해야 한다는 pending request가 아니다. 성공 output은 생성된
provider issue와 함께 old branch와 new branch를 보여줘야 한다.

`wt run issue`는 이미 존재하는 provider issue에서 worktree를 시작하는 명령으로 남긴다. Bare
`wt run issue`는 provider issue를 multi-select로 고르게 하고, 명시 issue key 목록은
scriptable path로 남긴다.
Provider issue를 TaskDocument로 가져오는 흐름은 `wt task import`, Local TaskDocument를
provider issue로 만드는 흐름은 `wt task publish`다. `wt run issue import`, `wt run issue create`,
`sync`, `pull`, `push`, `export` 같은 이름을 같은 개념의 alias로 추가하지 않는다.

여러 대상을 시작하는 `wt run issue`, `wt run pr`, `wt run task`는 기본 `--jobs 3`
bounded parallel 실행을 사용하고, 순차 실행과 interactive conflict prompt가 필요하면
`--jobs 1`을 명시한다. Parallel worker 안에서는 기존 worktree 삭제/열기, branch 재사용 선택,
base 선택 같은 prompt를 열지 않는다. 이런 선택이 필요한 항목은 실패로 기록하거나 실패로
보고하고, 이미 시작된 다른 항목은 계속 완료한다.

Publish는 TaskDocument의 schema를 넓히지 않는다. TaskDocument에는 계속 title, branch,
body, optional origin만 둔다. TaskRun, workflow, profile, retry status, pending
publish state는 TaskDocument에 저장하지 않는다. Publish selector는 어떤 local
TaskDocument를 고를지에만 관여하고, provider issue link는 선택된 각 TaskDocument의
`origin`에 기록한다. 동시에 `branch`는 생성된 provider issue key와 기존 branch의 final path
segment slug를 조합한 provider-keyed branch로 rewrite한다. 기본 형태는
`{{branch_prefix}}{{issue_key_lower}}-{{existing_branch_slug}}`다. `existing_branch_slug`는
기존 TaskDocument `branch`의 마지막 path segment를 branch-name sanitizer로 정리한 값이며,
이미 같은 issue key prefix가 있으면 중복하지 않는다. `branch_prefix`는 provider create/fetch
결과가 suggested branch prefix를 제공할 때만 사용하고, 없으면 빈 값이다.

Publish ambiguity는 provider side effect 전에 실패해야 한다. Explicit task keys,
bare selector 외에 workflow alias 같은 두 번째 task source를 만들면 안 된다.
Configured issue provider가 없으면 실패한다. Bare selector에서는 이미 `origin`이 있는
TaskDocument를 보여주지 않는다. 명시 task key에 이미 `origin`이 있으면 해당 task는
실패이며, 같은 task를 조용히 다시 publish해서 duplicate issue를 만들지 않는다. 이미
publish된 task는 `--skip-existing` 같은 명시적 옵션이 있을 때만 skip할 수 있다. 기존
`origin.provider`가 configured issue provider와 다르면 provider mismatch로 실패한다.
Provider issue title로 쓸 `title`은 필요하므로 비어 있으면 실패한다. `body`는 없거나
비어 있어도 empty issue body로 publish한다.

Publish branch rewrite가 기존 local state와 충돌할 수 있으면 provider side effect 전에
실패해야 한다. 선택된 TaskDocument가 이미 TaskRun을 가지고 있거나, 기존 `branch`에 대한
checked-out worktree, local branch, remote branch가 있으면 provider issue를 만들기 전에
명확한 에러로 중단한다. Publish는 worktree, local branch, TaskRun, Workflow, PR, agent setup을
만들지 않으며, 기존 branch state를 rename하거나 cleanup하지 않는다.

Dry-run은 첫 write-path의 필수 표면이 아니다. 추가한다면 실제 publish와 같은 validation을
거친 뒤 생성될 provider, title, body, branch metadata, 업데이트될 `origin` 위치를 보여주는
plan이어야 하고, TaskDocument에 pending state를 저장해서 dry-run 결과를 표현하지 않는다.

`wt task publish --help`는 이 side effect를 그대로 설명해야 한다. 즉 provider issue를
생성하고 local TaskDocument branch rewrite와 origin 기록을 수행한다는 점, 이미 origin이
있거나 provider가 불명확하거나 old branch state가 있으면 실패한다는 점, bare publish는 아직
origin이 없는 TaskDocument를 고른다는 점을 보여줘야 한다. Worktree 시작, TaskRun 생성,
workflow 실행, branch landing처럼 다른 lifecycle을 publish 도움말에 섞지 않는다.

`wt task import --help`는 import가 provider issue에서 TaskDocument로 향하는
non-executing 흐름임을 그대로 설명해야 한다. 즉 explicit issue id와 bare provider issue
selector를 모두 지원한다는 점, title/branch/body/`[origin]`을 기록한다는 점, provider
branch materialization은 할 수 있지만 worktree/local branch/TaskRun/Workflow/PR/agent
setup은 만들지 않는다는 점, duplicate ids나 existing TaskDocument collision에서
실패한다는 점, branch를 materialize할 수 없으면 incomplete TaskDocument를 쓰지 않고
실패한다는 점을 보여줘야 한다.

TaskRun은 그 작업을 한 번 실행한 인스턴스다. `<repo-root>/.wt/execution/task-runs/<id>.toml` 아래에
task, branch, status, group, error, creation_order, route fields, report/review metadata,
created_at, updated_at을 저장한다.
`group`은 Workflow id와 정확히 같은 문자열이며, Workflow file stem과 맞는
workflow-linked run을 식별하는 link다. 직접 `wt run task`로 만든 TaskRun은 group을
저장하지 않는다. Legacy TaskRun TOML의
source 값 `new`, `batch`, `stack`은 읽기 전용 migration compatibility로만 받으며 새
TaskRun 출력에는 쓰지 않는다. `creation_order`는 같은 task의 최신 실행을 고를 때 파일명이나
초 단위 timestamp 우연성에 기대지 않도록 새 TaskRun마다 증가하는 실행 생성 순서다.
`creation_order`가 없는 previous TaskRun은 계속 읽되 ordered TaskRun보다 앞에 정렬하고,
previous끼리는 `created_at`과 id를 fallback으로 쓴다.
status는 `prepared`, `running`, `passed`, `failed`, `skipped`만 canonical이다. Legacy
TaskRun TOML의 `status = "done"`은 migration compatibility로만 읽고, 새 TaskRun 출력에는
쓰지 않는다. 알 수 없는 status나 workflow mode 값은 조용히 해석하지 않고 파싱 단계에서
실패시킨다.
다만 branch/worktree 중심의 read-only surface는 unrelated malformed TaskRun 하나 때문에
valid branch/worktree dossier를 잃지 않도록 partial TaskRun inventory를 사용할 수 있다. 이때
invalid TaskRun file은 warning이나 JSON invalid-record field로 명시해야 하며, malformed
TaskRun id를 직접 target으로 지정한 경우에는 여전히 그 파일의 parse/validation error로
실패해야 한다.

통합 실행 상태 모델은 TaskDocument, Workflow, TaskRun의 책임을 나누는 데서 시작한다.
TaskDocument는 무엇을 할지에 대한 재사용 가능한 slice-level 설명이고, Workflow는
workflow-level `title`, `body`, optional `[origin]`과 그 task set을 어떤 실행 shape로
이어갈지에 대한 저장된 계획이며, TaskRun은 TaskDocument 하나를 한 번 실행한 기록이다.

Workflow 준비는 `<repo-root>/.wt/execution/workflows/<id>.toml` 하나와 각 task의 TaskDocument/TaskRun link를
만든다. Workflow의 canonical task 목록은 `[[tasks]]`이고, 각 row는 task key, linked
TaskRun id, stack-mode parent처럼 orchestration에 필요한 link와 실행 지시만 저장한다.
Workflow row는 status/error를 따로 가지지 않고, branch 이름도 복사하지 않는다. 실행
인스턴스의 canonical 기록은 TaskRun이고, branch name의 canonical 기록은 TaskDocument다.
새로 준비하는 Workflow의 canonical id와 file stem은 `YYYYMMDD-<slug>`다. `--id`가
있으면 그 값이 id/file stem이 되고, 없으면 creation-time `title`, selected TaskDocument
title, task key 순서로 slug seed를 고른다. `title`은 생성 뒤에는 display metadata일 뿐이며
Workflow id와 동기화되지 않는다. 기존 `YYYY-MM-DD-NNN` Workflow ids는 active local state를
읽고 실행하기 위한 legacy 형식으로 계속 valid하지만, 새 preparation path의 canonical 형식은
아니다. 모든 workflow-linked TaskRun의 `group`은 legacy/new 형식과 관계없이 Workflow id,
즉 Workflow file stem과 정확히 같아야 한다.
TaskDocument `title`/`body`/`[origin]`은 slice-level source of truth이며 Workflow row로
복사하지 않는다. Workflow-level `title`/`body`/`[origin]`은 task row가 아니라 Workflow
top-level metadata다. `wt workflow issue`에서 선택한 provider issue들은 각각 executable
slice TaskDocument가 되므로 provider origin도 각 TaskDocument에 저장된다. 선택한 issue id를
Workflow `[origin]`으로 자동 승격하지 않는다. 하나의 broad provider issue를 여러 local
slice TaskDocument로 쪼갠다면 `wt workflow task --origin-provider <provider> --origin-id
<id>`처럼 explicit Workflow-level origin을 기록하고, child TaskDocument에는 그 slice가
별도 provider issue일 때만 `[origin]`을 둔다. `objective`는 장기 authoring alias가 아니다.
Existing local
Workflow files에 대한 support가 필요하면 migration/repair support로만 설명하고, 새
authoring surface나 docs에서 `objective`를 equal canonical field처럼 받지 않는다.
`objective`, `description`, `goal_task`, `parent_task`, `subtasks`, `[[issues]]`,
`[[items]]`처럼 같은 상태나 목표를 가리키는 다른 이름은 새 canonical authoring shape가
아니다.

Workflow preparation accepts `--pr <none|draft|ready>` as a one-run override for
pull-request handoff intent. Omitted `--pr` means use the effective `[workflow]`
config. `--pr none` means agents report `PR=none`, `--pr draft` means agents open draft
pull requests and leave them draft, and `--pr ready` means agents open pull requests that
are ready for review immediately. Boolean `--pull-request` and boolean
`pull_request = true/false` are not canonical workflow surfaces.

Workflow policy is a preparation preference in `.wt.toml`, while a Workflow file is the
prepared execution plan for one run. Preparing a workflow reads the effective config
from `.wt.toml` plus `<repo-root>/.wt/config/local.toml`, applies any explicit command-line override, and
writes the resulting policy snapshot into `<repo-root>/.wt/execution/workflows/<id>.toml`. Later edits to
`.wt.toml` do not reinterpret already prepared workflows.

Canonical config shape:

```toml
[workflow]
pull_request = "none"  # none | draft | ready
landing = "manual"     # manual | auto

[review]
codex_base = "none"    # none | advisory | required
```

`pull_request` is the default pull-request handoff intent for workflow tasks. `none`
means agents report `PR=none` and do not create pull requests. `draft` means agents open
draft pull requests and leave them draft. `ready` means agents open pull requests that
are ready for review immediately. `ready` is the canonical name; `open`, `review`,
boolean `true`, and boolean `false` are not aliases.

`landing` is the coordinator preference after review passes. Coordinator review is
always part of the flow, and config cannot disable that review. `manual` means review
completes and the coordinator stops before merge or cleanup until the user explicitly
directs landing. `auto` means review passing is enough approval for the coordinator to
proceed to landing and cleanup. `auto` does not bypass dirty-worktree checks,
configured check commands, required pull-request checks, unresolved review threads,
branch ancestry checks, workflow mode ordering, or any other landing safety gate.

`review.codex_base` is an additional Codex-native base-diff evidence policy for the
coordinator. `none` means no Codex base-diff review evidence is required. `advisory`
asks the coordinator to open a Codex surface and run
`/review --base <resolved-parent>` when practical, with
`codex review --base <resolved-parent>` as the non-interactive fallback, and record
concise evidence; a missing/unavailable run is not by itself a blocker if reported.
`required` means the coordinator must run that review against the resolved workflow
base or stack parent and record concise evidence before `wt workflow pass`, landing,
or cleanup. Required Codex base review is enforced by `wt workflow pass`: the TaskRun
must also have dedicated accepted Codex base review metadata from
`wt task review <task-run-id> --accept --codex-base <resolved-parent>` after the
latest Agent Completion Report and after the coordinator records the evidence note.
This key does not replace normal coordinator review or pull-request review gates.

If a pull request exists, "review passes" is an evidence-backed pull-request review
gate, not an inferred state from green checks or an agent completion report. The
coordinator must refresh the pull-request review surfaces immediately before landing:
submitted reviews, review threads, PR comments, relevant check runs, PR body reactions,
and reactions on any review-request comments. Flat `gh pr view` output is not enough
when thread state, reviewer or review-agent replies, or reactions matter. `auto` may
proceed only after that gate is satisfied.

Review-thread resolution is part of the same gate. A thread can be resolved when the
issue is fixed on the PR branch and the reviewer or review agent has acknowledged it,
when the reviewer or review agent clearly agrees it is not actionable, or when a human
explicitly overrides with evidence in the PR conversation. Conversational review agents
are not one-shot signals: after the coordinator replies to an inline review comment,
the coordinator must refresh the thread and wait for the follow-up response before
resolving it. A thread-specific addressed marker or equivalent explicit acknowledgment
can satisfy that follow-up check; a follow-up saying the PR branch still contains the
issue keeps the thread unresolved. Tool-specific reactions or markers on the PR body or
review-request comment are provider status hints and must be recorded as such, not
treated as a substitute for checking threads, comments, and checks. When reviewing old
or closed PR threads against current `develop`, a later mainline fix may be useful
evidence for a reply, but it does not by itself prove the old PR branch's thread should
be resolved.

Provider examples are illustrative, not the canonical provider list:

- CodeRabbit inline comments require a refresh after replying, and resolve only after
  an addressed marker, explicit no-action agreement, or equivalent follow-up.
- Codex PR body or review-request reactions are status signals to record while still
  checking reviews, threads, comments, and checks.

The Workflow file stores the effective policy snapshot once at workflow level:

```toml
[policy]
pull_request = "none"
landing = "manual"

[policy.review]
codex_base = "none"
```

Workflow policy is intent, not state: actual pull-request review result, merge status,
ancestry proof, worktree cleanup, branch deletion, TaskRun lifecycle status, and
TaskDocument cleanup remain outside Workflow policy. `wt inspect`, pull-request state,
Git commands, `wt workflow pass`, and `wt done` continue to own those checks and
transitions explicitly.

The built-in config defaults are `pull_request = "none"`, `landing = "manual"`, and
`review.codex_base = "none"`.
Explicit workflow preparation flags override the config for one run while keeping the
same value names and failing early for conflicting forms instead of introducing aliases.
`wt config` shows the effective `[workflow]` and `[review]` policy, including built-in
defaults, so scripts and humans can inspect the actual policy that new workflow
preparation will use.
`wt init` does not write a commented optional `[workflow]` tutorial block; generated config
writes an explicit starter `[workflow]` policy with `pull_request = "none"` and `landing = "manual"`
unless it is preserving an existing explicit workflow policy from the target config.
It writes `[review]` only when preserving an existing explicit review policy, so local
init does not materialize `codex_base = "none"` as an accidental override of a shared
or root requirement.
`wt workflow show` displays the prepared policy snapshot from the workflow file, not the
current `.wt.toml` value.

This model changes both `.wt.toml` config shape and `<repo-root>/.wt/execution/workflows` state shape, so
implementing parser/runtime behavior is a pre-1.0 minor user-facing change. Replacing
workflow `objective` with workflow-level `title`, `body`, and optional `[origin]` also
changes the `<repo-root>/.wt/execution/workflows` state shape and `wt workflow task` / `wt workflow issue`
preparation surface, so it belongs in the same pre-1.0 minor model-change category.
Ordinary development commits still do not bump `Cargo.toml`; the release branch owns the
eventual version bump.

Workflow-level `title`/`body`/`[origin]` are saved to `<repo-root>/.wt/execution/workflows/<id>.toml` as
top-level Workflow metadata. They appear in `wt workflow show` and workflow-started agent
prompts as context, but do not change runnable selection, TaskRun lifecycle, landing
policy, cleanup behavior, provider issue status transitions, provider sync, or PR
issue-closing keywords. Prompt에서는 coordinator handoff가 먼저 전달되고, Workflow
metadata는 그 뒤 TaskDocument snapshot 근처에 배치된다. Existing `objective` values may
be read only to diagnose or repair old local files, and any explicit repair should
rewrite them into the canonical title/body/origin shape instead of preserving
`objective` as an authoring alias.
Bare `wt workflow task --mode <mode>`는 기존 local TaskDocument를 multi-select로 고른다.
명시 task argument는 scriptable path이며, 선택과 명시 argument를 한 command에서 섞는
두 번째 task source를 만들지 않는다.
Workflow coordinator는 `wt workflow task`가 Workflow와 linked TaskRun을 준비하는 생성
시점에 한 번 바인딩된다. 우선순위는 명시 `--coordinator <id>`가 있으면 그것을 쓰고,
없으면 `WT_AGENT_ID`, current live identity anchor, auto-created identity anchor 순서다.
`<id>`는 `wt as`와 같은 `NAME` 또는 `agents/NAME` 단일 agent-name segment만 허용한다.
생성 출력은 항상 저장된 `coordinator: agents/<name>`을 보여주고, auto-created identity
anchor로 떨어진 경우에만 다른 coordinator로 묶으려면 `--coordinator <id>` 또는
`WT_AGENT_ID=<id>`로 다시 생성하라는 hint를 보여준다. `wt run workflow`는 이 바인딩을
다시 결정하지 않고 linked TaskRun의 저장된 `coordinator_id`를 보존한다. legacy 또는
incomplete TaskRun처럼 `coordinator_id`가 비어 있는 경우에만 현재 actor 우선순위로 누락
route를 repair하고 그 fallback 사용 사실을 출력한다.

`<repo-root>/.wt/execution/workflows`는 `<repo-root>/.wt/batches`와 `<repo-root>/.wt/stacks`를 대체한다. 이유는 batch와 stack이
저장소 noun이 아니라 하나의 Workflow 안에서 고르는 execution mode이기 때문이다. 새
기능이 `<repo-root>/.wt/batches`나 `<repo-root>/.wt/stacks`에 상태를 계속 추가하면 사용자는 같은 준비 작업을
workflow, batch file, stack file 중 무엇으로 읽어야 하는지 다시 배워야 한다. 새 canonical
state는 Workflow file 하나로 수렴시킨다.

`single` mode workflow는 하나의 branch workspace에서 하나 이상의 TaskDocument를 실행한다.
`batch` mode workflow는 같은 base에서 여러 TaskDocument를 독립 branch로 실행한다. Batch
task들은 독립적이므로 이미 `running`인 TaskRun이 있어도 prepared/failed sibling이 있으면
workflow는 runnable로 남을 수 있다. `stack` mode workflow는 TaskDocument를 base-to-top
parent chain으로 실행하고, current `running` TaskRun이 있으면 다음 task를 시작하지 않는다.
Stack-mode에서 `running`은 agent prompt 전송이 아니라 coordinator의 명시적 pass
신호를 기다리는 상태다. Gate 통과를 추정해서 다음 task를 시작하지 않는다.

`wt run workflow`에서 workflow target 생략은 runnable workflow를 고르는 기본 동작이다. 후보가
하나뿐이어도 selector를 생략하고 바로 실행하지 않는다. 비대화형 shell에서는 명시 workflow
id/path를 넘겨야 한다.
`single`은 linked TaskRun 전체가 `prepared` 또는 `failed`일 때만 runnable이고, `batch`는
하나 이상의 linked TaskRun이 `prepared` 또는 `failed`이면 runnable이며, `stack`은 다음
`prepared` 또는 `failed` task가 있고 현재 `running` task가 없을 때 runnable이다. 명시
workflow id/path는 automation surface로 남긴다.
`wt run workflow`는 saved Workflow execution start만 뜻한다. Workflow list/show/edit/repair,
pass, task/issue preparation은 계속 `wt workflow` namespace에 남고 `run`이 소유하지
않는다.

`wt workflow list`는 `<repo-root>/.wt/execution/workflows/<id>.toml`에 저장된 Workflow file의 canonical
read-only inventory다. `wt run workflow`의 runnable selector가 아니므로 runnable workflow만
필터링하거나 selector의 10-row visible cap을 적용하지 않는다. `wt workflow show`의 latest
default도 all-workflow inventory로 확장하지 않는다. Output은 Workflow 자체의 단일
`status`를 만들지 않고, linked TaskRun에서 파생한 task-run status count/summary와 mode별
runnable metadata를 보여준다. Human text output은 `runnable`, `waiting`, `passed` 같은
파생 presentation group 아래에 workflow title, workflow id/mode, TaskRun summary,
profile/action/policy preview를 compact list row로 둔다. Human reason은 waiting row의
preview로 보여주되 body summary, raw origin, base, path 같은 상세 필드는 text에서 반복하지
않고 JSON output이나 `wt workflow show`에 둔다. JSON
output은 top-level `title`, `body`, optional `origin` metadata와 raw runnable reason
identifiers를 machine-readable metadata로 보존한다.
Workflow TOML parse/validation failure는 조용히 숨기지 않고
text warning 또는 JSON `invalid_workflows`로 보고한다. Batch/stack은 계속 Workflow `mode`
값일 뿐이므로 `wt list workflow`, top-level `batch`/`stack` 같은 symmetry
command를 추가하지 않는다. `wt task list`는 symmetry command가 아니라 별도 TaskDocument
inventory surface이며 Workflow, TaskRun, branch, worktree 목록 의미를 갖지 않는다.

`wt workflow show <id>`는 한 Workflow file을 읽는 canonical one-shot observation surface다.
기본 human 출력은 Workflow meta(path, mode, base, title/body/origin, policy, task count)와
번호 매긴 task row, task file path, branch, parent를 보여준다. `--json`은 같은 대상에 대해
`path`, `mode`, `base`, `title`, `pull_request`, `landing`, `review.codex_base`, `tasks[]`를 출력하며,
`tasks[]` 각 record는 `order`, `task`, `status`, `branch`, `parent`, `title`을 가진다.
`status`는 linked TaskRun의 durable lifecycle 값(`prepared`, `running`, `passed`, `failed`,
`skipped`)만 사용하고, JSON mode에서 linked TaskRun이나 TaskDocument를 판독할 수 없으면
가짜 status를 만들지 않고 비0으로 실패한다. 이 명령은 polling이나 gate verdict를 소유하지
않으므로 exit code는 command 성공/실패만 뜻한다. Workflow 전체 top-level `status`는 만들지
않고, 전체 terminal 여부와 실패 verdict는 별도 반복 관찰 command가 소유한다.

`wt profile list`는 named profile inventory를 위한 canonical surface이고,
`<repo-root>/.wt/config/profiles/<name>/profile.toml`을 config/profile loader로 읽어 정렬된 valid
profile 목록과 함께 invalid profile 레코드를 text warning 또는 JSON
`invalid_profiles`로 보고한다. Bare `wt profile`은 omission default로 `wt profile list`를
호출하며, 두 surface는 모두 `wt profile --help`와 `wt profile list --help`에 명시한다.
`default`는 reserved이므로 valid profile로 표시하지 않는다.

`wt ui [--port <PORT>]`는 `<repo-root>/.wt`와 wt config state를 읽기 쉽게 보는 read-only local web
UI다. 이 명령은 `127.0.0.1`에만 bind하고, port `0`은 available port 선택을 뜻하며, 시작
후 URL을 출력한 뒤 default browser로 그 URL을 연다. `--quiet`에서는 script-friendly stdout
contract를 위해 URL만 출력하고 browser를 열지 않는다.
UI 서버는 binary에 embedded된 no-build HTML/CSS/JS asset과 allowlisted route만 제공한다.
첫 API surface는 `GET /api/snapshot`이며 ideas, TaskDocuments, Workflows, TaskRuns,
retrospectives, profile summaries, effective config summary/source paths를 한 snapshot으로
반환한다.

`wt ui`는 inventory lens이지 새로운 state owner가 아니다. Ideas는
`<repo-root>/.wt/planning/ideas`, Specs는 `<repo-root>/.wt/planning/specs`, TaskDocument는 계속
`<repo-root>/.wt/execution/tasks`, Workflow는 `<repo-root>/.wt/execution/workflows`, TaskRun은
`<repo-root>/.wt/execution/task-runs`, config/profile layering은 `.wt.toml`,
`<repo-root>/.wt/config/local.toml`, `<repo-root>/.wt/config/profiles`가 source of truth다. UI
board group은 linked TaskRun 상태와 runnable metadata에서 파생한 presentation일 뿐이고,
Workflow나 TaskDocument에 새 status/column 값을 쓰지 않는다. Parse/validation failure는
snapshot과 UI에 invalid record로 드러내며, invalid TOML을 조용히 숨기지 않는다.

Workflow detail의 relationship summary와 secondary canvas view도 같은 파생 presentation이다.
행과 canvas node는 Workflow file의 `[[tasks]]`/`[[tasks.runs]]` 링크, TaskDocument, TaskRun
snapshot을 읽어 `Workflow → TaskDocument → TaskRun → Agent` 관계를 보여주지만 새 TaskDocument,
graph node, canvas position, agent contact, live agent state를 Workflow/TaskDocument/TaskRun에
저장하지 않는다. Agent 칸은 durable/현재 관찰 가능한 정보가 없으면 중립적인 not-observed
상태로 남기고, `TaskRun.status`와 runtime agent observation을 합치지 않는다.

`wt ui` read-only contract는 write API, drag/drop mutation, 별도 DB, frontend build pipeline,
Tauri/Electron, arbitrary repo file serving, `.env` 읽기를 추가하지 않는다. `/api/snapshot`은
state-owner reader와 config/profile loader를 거쳐 요약 DTO만 만들고, CLI text output을
scrape하지 않는다. Editing controls must not be hidden inside `wt ui`; a view that changes
TaskDocuments, Workflows, profiles, or config is a different surface.

Write-capable web editing belongs to a separate `wt studio` surface. `wt studio` is the canonical
place for canvas/form editing such as creating a Workflow, adding or removing TaskDocument nodes,
editing TaskDocument fields, changing Workflow task order/edges, and preparing execution from that
edited plan. Studio writes still use the same source-of-truth files: TaskDocuments in
`<repo-root>/.wt/execution/tasks`, Workflows in `<repo-root>/.wt/execution/workflows`, personal config in
`<repo-root>/.wt/config/local.toml`, and profiles in `<repo-root>/.wt/config/profiles`. TaskRun remains an
execution record and is read-only in Studio except through explicit lifecycle commands that already
own TaskRun mutation.

`wt studio [--port <PORT>]` starts a separate loopback web server from `wt ui`. It binds only to
`127.0.0.1`, treats port `0` as available-port allocation, prints a one-time
`http://127.0.0.1:<port>/auth?token=...` URL, and opens that URL in the default browser unless
`--quiet` is set. The `/auth` endpoint accepts the minted 256-bit session token once, sets an
`HttpOnly; SameSite=Strict; Path=/` cookie, and redirects to `/`. Every `/api/*` route must require
both that session cookie and an `Origin` header matching the bound `http://127.0.0.1:<port>` origin;
failed auth returns 401 and must not touch files.

`wt studio --dev` is the canonical frontend development mode. It keeps the write-capable API on the
same loopback Studio server, but treats the Vite dev-server origin as the browser origin so HMR can
serve `src/studio/web` directly. The default dev origin is `http://127.0.0.1:5173`; `--dev-origin`
may override it only with an explicit loopback `http://host:port` origin. In dev mode, `/auth` and
`/api` traffic must be proxied from Vite back to the printed Studio API origin with cookies and the
browser `Origin` preserved.

Unlike `wt ui`, Studio may use a frontend build pipeline. The canonical Studio frontend location is
`src/studio/web/` with Vite, Preact, and TypeScript. `cargo build` runs `npm ci && npm run build`
when the frontend inputs are stale, fails clearly if `node` or `npm` is unavailable, and embeds the
resulting `src/studio/web/dist/` assets in the `wt` binary with `include_dir!`. Runtime use of
normal `wt studio` must not depend on Node; only explicit `wt studio --dev` development mode does.

The Studio bootstrap layer exposes non-mutating routes such as the embedded page, `/auth`, and
authenticated `GET /api/ping`; mutation routes are added only with an operation-specific contract.
No Studio mutation route may write outside an
explicit allowlist for the state type it owns; TaskDocument writes are limited to
`<repo-root>/.wt/execution/tasks/*.toml` unless a later consistency update defines another
canonical allowlist.

Studio TaskDocument editing is a plan/apply contract over the canonical TaskDocument store, not a
new schema. `POST /api/task-documents/plan` accepts a TaskDocument path under
`<repo-root>/.wt/execution/tasks/<slug>.toml` and either a structured TaskDocument candidate or
raw candidate TOML. Structured candidates are first rendered to normalized TaskDocument TOML; raw
candidate TOML is preserved. A create plan treats `before` as the empty string and returns `after` as
that normalized or raw candidate TOML. An update plan reads the current disk file through the
TaskDocument owner path and returns `before` exactly as disk content and `after` exactly as the
candidate TOML that apply would write. Every plan returns a unified diff, validation status,
validation errors, and a file precondition containing the observed mtime and SHA-256 hash.
The mtime is serialized as a decimal string so browser clients preserve the exact nanosecond value.

`POST /api/task-documents/apply` accepts only the exact path, `before`, `after`, and precondition
from a prior plan. Apply re-validates `after` as TaskDocument TOML before writing; invalid
TaskDocument schema is rejected and must not touch disk. Apply succeeds only when the current disk
content, mtime, and hash still match the plan precondition. On success it writes atomically via a
temporary file in the TaskDocument directory followed by rename, then returns the new mtime and hash.
Within one Studio server process, TaskDocument apply operations are serialized before this
precondition recheck so concurrent applies cannot both pass the same observed file state.
If the precondition is stale because an external editor changed the file, apply returns 409 with the
current disk content, current fingerprint, and a unified diff from planned `before` to current disk
so the user can re-plan.

The TaskDocument mutation allowlist is exact: Studio may write only
`<repo-root>/.wt/execution/tasks/<slug>.toml`, where `<slug>` is the same safe task key form used
by the TaskDocument store. Absolute paths, parent traversal such as `../escape`, nested paths, and
non-TaskDocument file names fail with 4xx before validation or write. Studio must continue to use
the TaskDocument state-owner reader/writer and must not scrape `wt ui` snapshot DTOs or CLI text
output to derive mutation state.

`wt studio` mutations must have a visible draft state, validation errors, and a preview of the exact
state-file changes before apply. Applying a studio edit requires an explicit mutation contract with
path allowlist, same-origin/token policy, and operation-specific validation; it must not scrape CLI
text output or write arbitrary repo files. Canvas position, temporary selection, inspector dirty
state, and UI layout preferences are editor presentation state unless a later canonical state model
defines them as durable data.

Workspace label은 저장 상태가 아니라 현재 실행을 찾기 위한 표시다. 좁은 탭에서 잘려도
의미가 남도록 `2/5 PROJ-123 Title`처럼 짧은 order 라벨을 앞에 붙이고, branch/path/site
이름에는 `batch`나 `stack` 같은 mode label을 섞지 않는다. `B`/`S` prefix는 workflow
contract에 포함하지 않는다.

`wt run task` coordinator handoff는 즉시 TaskDocument 실행 handoff다. `wt run task`가
시작하는 prompt에는 `Task Run Coordinator Handoff` section이 포함되고, 기본 보고 route인
`wt task report "Agent Completion Report: ..."` 명령이 먼저 들어간다. `wt task report`는
TaskRun에 저장된 `agent_id`와 `coordinator_id`를 사용해 direct scope 보고를 보낸다. 같은
명령은 TaskRun이 `running` 또는 `passed`일 때 유효하다. `WT_TASK_RUN_ID`가 없으면 현재 branch에서
running 또는 passed TaskRun이 정확히 하나일 때만 fallback으로 보고 대상을 고르고, 둘 이상이면
후보 id를 보여주고 실패한다. 같은 section은 fallback으로 현재 coordinator cmux workspace/surface
좌표로 렌더링되는 `cmux send`와 `cmux send-key ... enter` 명령도 포함한다.
이것은 Workflow orchestration이나 pass command가 아니다. Task-run agent는
`PR=none`인 `Agent Completion Report`를 coordinator에게 보내고, coordinator가 review,
landing, cleanup을 명시적으로 처리할 때까지 기다린다. cmux 좌표는 현재 transport 정보일
뿐이므로 TaskDocument나 TaskRun에 저장하지 않는다. file inbox route가 unavailable이면
agent는 cmux fallback으로 같은 보고를 보내고, 둘 다 unavailable이면 task session에 남기고
기다린다. Handoff section과 그 안의 task-report/cmux report 명령은 TaskDocument 본문과
custom/setup prompt까지 포함한 같은 첫 prompt의 앞부분에 둔다. 그래서 agent가 같은 turn 안에서
coordinator route, 작업 본문, 실행 지침을 함께 받으면서도, terminal prompt가 축약될 때
coordinator route가 앞쪽에 남게 한다. TaskRun launcher는 이 첫 prompt를 보내기 전에
TaskRun을 `running`으로 전이해, agent가 첫 turn에서 곧바로 `wt task report`를 실행해도
저장된 report route가 reportable해야 한다.

Workflow coordinator handoff는 `stack` 전용 개념이 아니라 `wt run workflow`가 시작하는
모든 task prompt의 계약이다. Prompt에는 `Workflow Coordinator Handoff` section이 포함되고,
기본 보고 route인 `wt task report "Agent Completion Report: ..."` 명령이 먼저 들어간다.
Workflow-prepared TaskRun은 stable `agent_id`, `coordinator_id`, `coordinator_label`을
저장하고, `wt task report`는 그 TaskRun context에서 `workflow:<workflow-id>` scope를
자동으로 적용한다. 이 report route는 TaskRun이 `running`이거나 `passed`일 때 열려 있다.
Prompt는 raw message recipient/scope를 agent가 직접 구성하도록 지시하지 않는다. 같은 section은
fallback으로 현재 coordinator cmux
workspace/surface 좌표로 렌더링되는 `cmux send`와 `cmux send-key ... enter` 명령도
포함한다. cmux 좌표는 현재 transport 정보일 뿐이므로 Workflow file, TaskRun, TaskDocument에
저장하지 않는다. file inbox route가 unavailable이면 agent는 cmux fallback으로 같은
`Agent Completion Report`를 보내고, 둘 다 unavailable이면 task session에 남기고 기다린다.
Handoff section과 그 안의 task-report/cmux report 명령은 TaskDocument 본문과 custom/setup
prompt까지 포함한 같은 첫 prompt의 앞부분에 둔다. 그래서 agent가 같은 turn 안에서 coordinator
route, 작업 본문, 실행 지침을 함께 받으면서도, terminal prompt가 축약될 때 coordinator route가
앞쪽에 남게 한다. 사용자 정의 `[agent.prompt.workflow]` prompt가 있으면 같은 첫 prompt 안에서
이 built-in handoff와 TaskDocument snapshot 뒤, 기존 `issue`/`branch` setup-mode prompt 앞에
붙인다. Workflow TaskRun도 첫 prompt를 보내기 전에 `running`이어야 하며, 이 보장은
single/batch/matrix/stack mode 전체에 동일하게 적용된다.

보고 형식은 workflow mode와 무관하게
`Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>`
이다. `PR` 값은 workflow file의 prepared policy를 따른다. `pull_request = "none"`이면
pull request를 열지 않고 `PR=none`으로 보고한다. `"draft"`는 작업 agent가 branch를
push하고 준비된 workflow base 또는 parent branch를 base로 draft pull request를 열어 draft로
남긴다는 뜻이다. `"ready"`는 draft를 만들었다가 전환하지 않고 바로 review-ready pull request를
연다는 뜻이다. PR을 여는 workflow task는 `.github/pull_request_template.md`에서
`<pr-body-file>`을 만들고 summary, context, changes, validation, risks/follow-ups 중심의
review-focused 본문을 채운다. TaskDocument에 `[origin]`이 있으면 PR merge가 provider
issue를 닫도록 `Closes <origin.id>` issue-closing keyword도 PR 본문에 포함한다.
Workflow-level `[origin]`은 child PR의 closing keyword source가 아니다. 여러 child PR이
같은 Workflow-level origin을 자동으로 닫으면 broad issue가 첫 slice merge에서 닫힐 수 있으므로,
그 정책은 별도 명시/테스트 없이는 추가하지 않는다. 그런 뒤
`gh pr create --body-file <pr-body-file>` 경로로 PR을 생성한다.
Agent Completion Report는 coordinator transport/report 형식이며 PR 본문으로 복사하지 않는다.
이것은 PR 자체나 review 상태가 아니라 다음 실행자에게 전달할 작업 계약이다. 보고 전송은
transport일 뿐 상태 전이가 아니다. Review는 항상 coordinator flow에 포함된다. Coordinator가
task agent에게 전달하는 canonical feedback은
`wt task review <task-run-id> --accept|--reject|--block "<message>"`이며, 이 명령은
TaskRun의 `agent_id`로 `task_run:<id>` scope 메시지를 보내고 TaskRun review metadata를
갱신한다. Review를 보내는 actor가 TaskRun의 저장된 `coordinator_id`와 다르면 feedback은
거부되어야 하며, 교정 경로는 기록된 coordinator로 재실행하는
`wt as agents/<recorded> -- wt task review <task-run-id> --accept|--reject|--block "<message>"`
형태다. Late review after pass는 정상 flow다. `--reject`와 `--block`은 passed TaskRun을
`running`으로 되열고, task agent는 같은 TaskRun route로 다시 `wt task report`를 보낼 수
있다. `--accept`는 metadata-only이며 running TaskRun을 `passed`로 만들지 않는다.
`--accept --codex-base <parent>`는 required Codex base-diff review gate를 위한
`codex_base_review_*` evidence도 별도로 기록하며, 일반 `--accept` metadata만으로는 그 gate를
충족하지 않는다. Pull request
review나 coordinator가 전달한 리뷰는 해당 task agent가 반영하고, 필요한 check를 다시 돌린 뒤
commit/push하고 PR 본문이 stale해졌을 때만 PR 본문과 Agent Completion Report를 갱신한다.
실행자나 coordinator가 `wt inspect`, 필요한 경우 pull request, 보고를 확인한 뒤
`wt workflow pass`를 실행할 때 TaskRun 상태가 `passed`로 전이된다. Pull request가 있으면
coordinator는 workflow pass나 landing 전에 pull-request review gate를 통과했는지 별도로 확인한다.
이 gate는 unresolved thread가 0인지뿐 아니라 최근 reviewer 또는 review-agent 답글, PR comment,
review-request reaction, check 상태를 포함한다. Review-agent thread는 coordinator 답글 직후
바로 resolve하지 않고, follow-up을 refresh해서 해결 또는 비조치 동의가 확인된 뒤 resolve한다.

`wt done`은 worktree와 local branch cleanup 명령이다. `done`은 cleanup 신호이고,
workflow pass는 coordinator gate 통과 신호이며, `merge`/`land`는 branch commit을 `master` 같은
통합 branch에 넣는 Git workflow다. `wt done`이나 `wt workflow pass`가 branch를
`master`에 merge했다고 해석하지 않는다. 현재는 별도 `wt land` 명령을 만들지 않고,
`git switch master`, `git pull --ff-only`, `git merge --ff-only <branch>` 같은 명시적 Git
단계로 landing을 문서화한다. Stack-mode workflow branch는 workflow가 보여주는 base-to-top
순서대로 landing한다.

`wt done <target>`의 explicit cleanup target은 branch, worktree path/name,
issue-like branch-name shorthand, direct TaskRun id다. Direct TaskRun id는 해당 TaskRun의
branch를 checked-out worktree로 해석한 뒤 같은 cleanup path를 탄다. Workflow-linked
TaskRun id는 workflow pass를 우회하지 않도록 거부하고 `wt inspect`와
`wt workflow pass` 경로를 안내한다. Issue shorthand는 provider issue lookup이 아니라
현재 branch text에 대한 compatibility shorthand다.

Local task cleanup도 별도 단계다. TaskDocument는 재사용 가능한 work definition이므로
기본적으로 보존한다. 한 번 실행하고 끝난 task라도 linked TaskRun과 Workflow reference가
정리되기 전까지 TaskDocument 삭제를 gate pass에 섞지 않는다. 나중에 `wt land`,
`wt task clean`, `wt run clean`, `wt workflow clean` 같은 명령을 만들더라도 `done`이나
`pass`에 merge나 task definition 삭제 의미를 섞지 않는다.

`wt inspect [<target>]`는 branch, worktree, TaskRun을 읽어서 parent, dirty 상태,
commit/diff 정보, Agent Completion Report 기대치, 현재 cmux contact를 보여주는 canonical
read-only dossier다. `--pr`을 명시하면 같은 inspect report 안에 Pull Request Review evidence
section을 추가로 가져와서 PR metadata, submitted review/head synchronization, review
threads의 file/line evidence, PR comments/reactions, check rollup, warning을 보여준다. 이 PR
evidence는 read-only inspection surface이며 thread resolve, reply, review request, PR body edit,
merge, TaskRun/Workflow state mutation을 하지 않는다. `--pr`을 생략한 `wt inspect <target>`은
GitHub auth나 network fetch 없이 local dossier만 출력해야 한다. Agent observation snapshot을 같이
보여줄 수 있지만, `inspect`의 exit code는 command 자체의 성공/실패만 뜻한다. 관찰된 agent가
`needs_input`이거나 `failed`여도 그 사실만 출력하고 polling용 exit code로 바꾸지 않는다. PR
review verdict도 human output과 nested JSON evidence에만 두고 exit-code 의미를 바꾸지 않는다.
실제 gate 통과 기록은 direct 또는 workflow-linked context별 명령이 맡는다. 직접 `wt run task`가 만든
TaskRun은 review/landing 확인 뒤 `wt done` cleanup이 정리할 수 있고, Workflow file의
`[[tasks]].run`과 matching `group`으로 연결된 TaskRun은 `wt workflow pass`가 전이한다.

`wt inspect`에서 `<target>` 생략은 interactive TTY human mode에서 inspectable work target
selector를 여는 기본 동작이다. `--json`, `--quiet`, 또는 non-TTY automation에서는 selector를
열지 않고 explicit `<target>`을 요구해야 한다. 실패 메시지는 branch, worktree path/name,
TaskRun id 중 하나를 넘기거나 interactive TTY에서 selector를 열라는 guidance를 정확히
보여줘야 한다.

`wt inspect <target> --pr --json`은 PR evidence를 `pull_request_review` nested field 아래에
둔다. Top-level `status`는 만들지 않는다. Durable execution lifecycle은 `task_runs[].status`,
agent observation은 runtime observation field, PR review result는
`pull_request_review.verdict`처럼 각 concept owner 아래에 남아야 한다.

`wt send`도 상태 전이 명령이 아니다. `wt inspect`와 같은 target 해석으로 현재 cmux
surface를 찾아 메시지를 보내는 transport 명령이다. 메시지를 보냈다는 사실을 TaskRun
상태로 저장하지 않고, gate 통과 여부는 여전히 TaskRun status와 `wt workflow pass`로만
표현한다.

`wt agent status [<target>]`는 현재 agent/cmux observation surface다. `target`은
`wt inspect`와 `wt send`가 받는 branch, worktree path/name, TaskRun id와 같은 work selector다.
이 명령은 현재 cmux workspace/surface와 agent 화면/status/event를 관찰해서 agent-friendly
text/JSON 상태를 돌려주며, TaskRun status나 provider issue status를 쓰지 않는다. Text 출력은
target, branch, TaskRun lifecycle status, agent kind/state, cmux contact, 마지막
tool/session/event, warning을 compact하게 보여준다. JSON에는 top-level `status`를 만들지
않고 `task_run.status`와 agent runtime observation을 서로 다른 nested field로 둔다.
`TaskRun.status`는 durable execution lifecycle이고, agent runtime observation은 현재
runtime state이므로 한 top-level field 이름으로 합치지 않는다.

`wt agent watch [<target>]`는 polling/waiting surface다. 같은 target과 observation model을
쓰되, interval마다 상태 변화를 compact하게 출력하고 blocked/failed terminal observation에
도달하면 polling contract에 맞춰 종료한다. GitHub CLI의 `gh run watch`
(`https://cli.github.com/manual/gh_run_watch`)처럼 `watch`는 반복 관찰과 의미 있는 종료
상태를 가진 surface 이름이다.
기본 출력은 transition-only로 유지한다. 오래 running 상태가 바뀌지 않는 coordinator wait는
`--heartbeat <SECONDS>`를 명시해서 elapsed time, target, branch, TaskRun lifecycle status,
agent kind/state, 마지막 tool/session/event, cmux contact/warning을 compact하게 반복 출력한다.
`--timeout <SECONDS>`는 지정한 bound를 넘어서도 agent가 running이면 timeout 메시지를 출력하고
현재 observation exit-code contract로 종료한다. 따라서 여전히 running이고 blocked/failed가
아니라면 observable/not blocked인 0으로 끝난다.

`wt agent watch`는 `--heartbeat`나 `--timeout`으로 non-idle heartbeat/timeout sample을
emit하고 runtime AgentId가 TaskRun route나 live surface anchor로 확인될 때마다
`<repo-root>/.wt/runtime/agents/<name>/observations/wait-observations.jsonl`에 append-only JSONL
sample로 기록한다. 별도 기록 flag, opt-out flag, config key는 없다. idle, needs_input,
failed, no_session observation은 non-idle wait sample을 만들지 않는다. 이 state owner는
`runtime/agents/<name>/observations`이며 runtime observation 자료일 뿐이므로 TaskRun status, Workflow file,
TaskDocument, cmux transport 좌표에 쓰지 않는다. sample은 watch 시작 이후의 안정된
`elapsed_seconds`, 사용자가 지정한 heartbeat/timeout bound인 `bound_seconds`, 마지막 출력 이후
변하지 않은 시간을 나타내는 `unchanged_seconds`를 분리한다. sample 기록 때문에 TaskRun을
failed, blocked, passed, skipped로 전이하지 않고, `wt agent watch`를 delivery loop나 detached
supervisor로 넓히지 않는다. `--heartbeat`와 `--timeout`이 모두 없으면 기록할 heartbeat/timeout
sample도 없다.

`wt agent wait-stats`는 `<repo-root>/.wt/runtime/agents/<name>/observations/wait-observations.jsonl`을 읽는
read-only summary surface다. count, sum, average, min, max, bucket과 `wait_reason`,
`bound_seconds`, `agent_kind`, `agent_state` 같은 low-cardinality group aggregate를 보여줄 수
있지만 agent를 새로 관찰하거나 cmux에 접속하거나 TaskRun/Workflow state를 수정하지 않는다. 이
summary는 나중에 추천을 설계하기 위한 근거가 될 수 있지만 현재 slice에서는 adaptive default를
자동 추론하거나 사용자가 준 `--heartbeat`/`--timeout` 값을 덮어쓰지 않는다.

`wt agent status`와 `wt agent watch`의 `<target>` 생략도 interactive TTY human mode에서만
selector를 연다. `--json`, `--quiet`, 또는 non-TTY automation에서는 explicit `<target>`이
필수이며, omission은 `wt agent status <target>`, `wt agent watch <target>`, `wt inspect
[<target>]` 중 어떤 표면을 써야 하는지 알려주며 실패해야 한다.

Agent observation exit code는 agent command에만 속한다. `wt agent status`와
`wt agent watch`는 0을 observable/not blocked, 1을 target/session/cmux unavailable, 2를
`needs_input`, 3을 `failed`로 유지한다. cmux 자체를 사용할 수 없으면 성공한 `no_session`처럼
보이지 않도록 실패한다.

`wt workflow watch [<workflow>]`는 Workflow file과 linked TaskRun durable status를 반복 관찰하는
workflow-level terminal block이다. `wt agent watch`가 한 task agent의 Layer 2 runtime state를
관찰하는 것과 달리, `wt workflow watch`는 cmux runtime 관찰이나 delivery loop가 아니며 모든 workflow
task가 terminal(`passed`, `failed`, `skipped`)이 될 때까지 기다린다. 출력은 기본 transition-only이고,
긴 unchanged 대기는 `--heartbeat <SECONDS>`를 명시해야 반복 출력한다. `--timeout <SECONDS>` 뒤에도
아직 non-terminal task가 남아 있으면 timeout 메시지와 현재 상태를 출력하고 0으로 종료한다.
종료 코드는 agent watch 관찰 계약을 workflow durable status에 맞춰 재사용한다: workflow state가
unavailable이면 1, terminal task 중 `failed`가 하나라도 있으면 3, 그 외 all passed/skipped 또는
timeout-still-running은 0이다. `--json`은 종료 시 `wt workflow show --json`과 같은 final snapshot
스키마를 출력한다. 이 command는 `<repo-root>/.wt/runtime/agents/<name>/observations/wait-observations.jsonl`
에 쓰지 않는다. `<workflow>` 생략은 interactive TTY human mode에서만 selector를 열며, `--json`,
`--quiet`, non-TTY automation에서는 explicit workflow path나 id를 요구해야 한다.

Agent별 상태 신호 준비도는 `agent status`나 `agent watch`가 고치는 대상이 아니라 관찰의
신뢰도 조건이다. Claude Code는 cmux의 Claude 통합에서 status/sidebar 신호가 나오고, Codex는
사용자가 `cmux hooks codex install --yes`를 명시적으로 실행한 뒤에야 `agent.hook.*`와
`set_status codex Running/Idle` 신호가 나온다. Codex hook이 없으면 agent commands는 화면
텍스트 fallback을 쓸 수 있지만 약한 관찰이라는 warning을 남겨야 한다. `wt doctor`는 cmux
상태 신호 준비도를 보고만 한다.

Codex inbox delivery는 상태 신호와 다른 per-machine setup step이다. `wt setup`은 사용자가
요청하고 수락한 경우에만 user-level Codex `hooks.json`과 `config.toml` trust state를
수정한다. 일반 `wt agent status/watch`, `wt doctor`, `wt msg` 같은 명령이 전역 Codex hook을
자동 설치하거나 사용자 agent config를 몰래 수정하면 안 된다.

Agent runtime observation은 `wt agent status`와 `wt agent watch` 아래에 둔다. Git의
`status` 문서(`https://git-scm.com/docs/git-status.html`)는 worktree/index state를 뜻하므로
top-level status command는 좁은 agent screen poll보다 managed work state 전반을 암시한다.
GitHub CLI도 `gh auth status`(`https://cli.github.com/manual/gh_auth_status`),
`gh pr status`(`https://cli.github.com/manual/gh_pr_status`)처럼 좁은 status를 noun namespace
아래에 둔다. 따라서 agent observation parser와 dispatch는 canonical agent namespace에만
존재해야 하며, top-level silent alias를 남기지 않는다.

Read-only dossier command는 `wt inspect [<target>]`다. GitHub CLI의
`gh pr review`(`https://cli.github.com/manual/gh_pr_review`)처럼 external CLI convention에서
review는 검토를 추가/제출하는 action으로 읽히기 쉽다. Read-only detail dossier는 Kubernetes
`describe`(`https://kubernetes.io/docs/reference/kubectl/generated/kubectl_describe/`)처럼
inspection surface에 가깝다. 따라서 parser와 dispatch는 canonical inspect surface에만
존재해야 하며, review-named silent alias를 남기지 않는다.

`wt workflow repair <workflow>`는 관찰 side effect가 아니라 coordinator/operator가
명시적으로 실행하는 복구 표면이다. 기본 동작은 dry-run preview이며, linked TaskRun,
local worktree, 현재 cmux agent surface를 관찰해 어떤 TaskRun을 기존 `failed` 상태로
기록할 수 있는지 보여준다. `--apply`를 줬을 때만 TaskRun failure model을 통해 status와
error를 쓴다. repair는 Workflow나 TaskRun에 cmux workspace/surface 좌표를 저장하지
않고, cmux workspace close나 worktree removal 같은 파괴적 정리는 수행하지 않는다. 그런
정리가 필요하면 별도의 명확한 flag/confirmation이 있는 cleanup 표면에서 다뤄야 한다.
`wt inspect`, `wt send`, `wt agent status/watch`는 repair를 권할 수는 있지만 repair
action을 대신 실행하지 않는다.

상태 파일은 내부 캐시가 아니라 사용자가 읽어도 이해되는 기록이어야 한다.

### Agent-Neutral Names Stay Agent-Neutral
*North star: [Provider Direction](north-star.md#provider-direction).*

여러 agent에 공통으로 적용되는 기능에는 특정 agent 이름을 붙이지 않는다.

특정 agent에만 적용되는 기능에는 그 agent 이름을 붙여도 된다. 하지만 Codex와 Claude
모두에 영향을 주는 기능이 `claude_*` 이름을 가지면 이름이 거짓말을 하게 된다.

이름은 구현의 과거가 아니라 현재의 의미를 설명해야 한다.

### Compatibility Does Not Create Aliases
*North star: [Direction-Driven Design](north-star.md#direction-driven-design).*

호환성은 canonical 모델을 흐리게 만드는 두 번째 이름이나 상태 형태를 정당화하지
않는다.

사용자-facing 설정, 명령, 옵션, 상태 파일에는 같은 개념을 가리키는 호환 alias를
남기지 않는다. 이전 이름을 입력하면 새 이름으로 조용히 해석하지 말고 실패시켜야 한다.

예를 들어 local site 설정은 `[site] provider = "herd"`가 canonical이고, 같은 의미를
`[herd]` 섹션으로 다시 받지 않는다.

### Versioning Reflects Stability
*North star: [Direction-Driven Design](north-star.md#direction-driven-design).*

버전 번호도 사용자-facing 계약이다.

`wt`는 1.0 완성점을 전제로 하지 않는 pre-1.0 personal harness다. 새 기능과 breaking
user-facing 정리는 `0.x.0` minor로 올리고, 버그 수정이나 내부 로직 변경은 `0.x.y`
patch로 올린다.
예를 들어 prepared-task 실행 표면을 `wt run task`로 수렴시키거나, saved orchestration을
`wt workflow`와 `<repo-root>/.wt/execution/workflows`로 수렴시키는 변경은 CLI와 상태
파일 계약을 바꾸므로 patch가 아니라 pre-1.0 minor 변경이다.

다만 version bump는 일반 개발 커밋마다 하지 않는다. 버전 변경은 릴리즈 작업에서만
허용하며, 기능/버그/문서/리팩터링 PR은 변경 범위와 무관하게 `Cargo.toml`/`Cargo.lock`
version을 올리지 않는다. `develop`은 기본 개발 브랜치이고, `master`는 릴리즈 브랜치다.
릴리즈 PR에서 `Cargo.toml`/`Cargo.lock` version을 한 번만 올리고, 그 릴리즈에 포함된
변경 중 가장 큰 SemVer 범위를 적용한다. 릴리즈 PR이
`master`에 merge되고 tag가 생성되면, version bump를 다시 `develop`에 merge해서 두
브랜치의 기준점을 맞춘다.

언젠가 1.0 안정화 약속을 명시적으로 만들기 전까지는, major bump는 현재 모델의 판단 기준이
아니다.

## UX Checklist

새 명령, 옵션, config, 상태 파일을 추가하기 전에 다음 질문에 답한다.

- 이 개념을 한 문장으로 설명할 수 있는가?
- 이미 같은 개념을 가리키는 이름이 있는가?
- 생략했을 때의 동작과 명시했을 때의 동작이 분명한가?
- 잘못 조합된 옵션을 조용히 추론하고 있지는 않은가?
- `--help`만 읽어도 실제 동작을 예측할 수 있는가?
- 저장되는 값이 사용자의 의도를 나타내는가, 내부 구현 편의를 나타내는가?
- 이름이 특정 도구나 agent에 과하게 묶여 있지는 않은가?

이 질문 중 하나라도 애매하면 기능을 추가하기 전에 개념을 먼저 정리한다.
