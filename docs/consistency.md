# Consistency Philosophy

이 문서는 `wt` 코드베이스가 지켜야 할 UX 일관성 원칙을 정리한다.

`wt`는 worktree, issue, pull request, profile, workflow, agent runtime처럼 서로
다른 개념을 조합하는 도구다. 기능이 늘어날수록 사용자가 기억해야 할 규칙이 늘기
쉽다. 그래서 `wt`의 UX는 기능 수보다 개념의 선명함을 우선한다.

## North Star

사용자는 명령 이름, 옵션 이름, config 이름만 보고도 다음을 예측할 수 있어야 한다.

- 무엇을 대상으로 하는가
- 어떤 실행환경을 쓰는가
- 언제 한 개만 실행되고 언제 여러 개가 실행되는가
- 어떤 상태가 저장되고 다음 실행에 어떤 영향을 주는가

예측이 어렵다면 기능이 부족한 것이 아니라 모델이 흐린 것이다.

## Principles

### One Concept, One Name

같은 개념은 하나의 이름으로만 표현한다.

예를 들어 실행환경을 뜻하는 개념은 `profile`이어야 한다. 같은 의미를 `variant`,
`runtime`, `kind`, `driver` 같은 이름으로 다시 만들지 않는다.

이름이 바뀌어야 한다면 기존 이름과 새 이름을 오래 공존시키기보다, 왜 새 이름이 더
정확한지 정리하고 한쪽으로 수렴한다.

### Different Concepts Stay Separate

다른 개념은 명령, 옵션, config, 상태 저장에서도 분리한다.

`init`은 이 repo가 어떤 starter shape로 시작할지에 대한 개념이다. Starter shape는
worktree-only 최소 설정인지, agent를 붙일지, issue workflow를 붙일지, app repo용
setup/site/test/workspace 기본값까지 잡을지를 고르는 선택이다.

`profile`은 어떻게 실행할지에 대한 개념이다. `init`이 starter shape 안에서
`[profile.agent]`를 쓸 수는 있지만, agent runtime을 구조화하고 재사용하는 책임은 계속
`profile`에 둔다.

`workflow`는 `.local/workflows` 아래에 저장되는 prepared execution plan이다.
사용자는 어떤 task들을 어떤 실행 shape로 시작하고 이어갈지를 workflow로 다룬다.
Canonical command surface는 `wt workflow`다.

Workflow는 정확히 하나의 `mode = "single" | "batch" | "stack"` 값을 가진다.
`mode`는 실행 shape만 고른다. `single`은 하나의 branch workspace에서 하나 이상의
TaskDocument를 실행하고, `batch`는 같은 base에서 여러 독립 branch를 실행하며,
`stack`은 task branch들을 정해진 parent chain으로 순서대로 실행한다.

`batch`와 `stack`은 workflow mode 값으로 남지만 top-level 상태 파일 noun이나 command
namespace가 아니다. 새 상태 파일은 `.local/workflows` 아래에만 만들고, batch/stack
전용 상태 디렉터리는 새 코드가 읽거나 쓰는 상태 위치가 아니다. 별도 top-level command
surface를 `wt workflow` 옆에 남기면 두 command surface가 모두 canonical처럼 보이므로
새 CLI parser와 dispatch는 canonical `wt workflow`만 노출한다.

Workflow file은 optional `objective`, mode, base, profile, color, timestamps,
workflow-level policy, task/run link 같은 prepared-plan context와 orchestration만
저장한다. `objective`는 workflow가 완수하려는 더 큰 목표를 설명하는 human context이며
실행 상태가 아니다. Task branch name의 source of truth는 항상 TaskDocument의 `branch`다.
Workflow task row는 branch 이름을 복사해 저장하지 않는다.

Workflow color는 같은 workflow가 연 cmux workspace들을 시각적으로 묶는 표시다. 색상이
생략되면 `wt`가 내장 cmux named-color palette의 다음 색을 고르고 workflow file에
기록한다. 색상은 mode나 task의 의미가 아니라 workflow-level 표시다.

`wt config` 출력은 runtime behavior를 판단하는 effective source of truth다. `.wt.toml`,
`.local/.wt.toml`, profile file은 사용자 intent와 override를 저장하고, `wt config`는
merge된 layer, convention file, built-in default를 사용자가 복사해 수정할 수 있는 형태로
보여준다. 명령 구현은 user-facing default를 각 call site에서 새로 해석하지 말고 config
모델의 effective accessor나 effective policy snapshot을 거쳐 적용해야 한다.
활성 section의 runtime default는 `wt config` 출력에 materialize한다. 예를 들어 active
`[site]` provider는 name/root/secure/open_browser/url과 Traefik target default까지 보여주고,
active `[editor]` 설정은 생략된 placement의 `cmux_surface` default를 보여준다. 반대로
`provider = "none"`처럼 feature가 inactive인 section은 effective output에 unrelated section으로
내보내지 않는다.

`[workspace].colors`는 workspace를 시작하는 command surface의 기본 cmux 색상이다.
Canonical 색상 key는 `task`, `issue`, `new`, `pr`이다. `task`는 TaskDocument에
`[origin]`이 있는지와 무관하게 즉시 실행 표면인 `wt task run`에 대응한다. `issue`는
직접 provider issue에서 시작하는 `wt issue`, `new`는 branch-name text에서 시작하는
`wt new`, `pr`은 pull request branch를 여는 `wt pr`에만 대응한다. 이 key들은 prompt
setup mode, profile 이름, workflow `mode = "single" | "batch" | "stack" | "matrix"`
값이 아니다.
Workflow run은 필요한 경우 TaskDocument setup을 거치더라도 최종 visible grouping color는
저장된 `workflow.color`를 적용한다. `[workspace].colors` key를 생략하면 내장 기본값
`task = "blue"`, `issue = "blue"`, `new = "green"`, `pr = "magenta"`를 쓴다.
`wt config`는 이 effective 색상값을 출력하므로 사용자가 수정할 기준은 `wt config` 출력이다.
`wt init`은 이 기본값을 active config로 쓰지 않고 commented override 예시로만 보여준다.
Active `colors = { ... }`는 사용자가 기본값과 다른 색을 고정하려는 의도일 때만 둔다. 색을
아예 쓰지 않을 kind는 `task = ""`처럼 빈 문자열로 override한다.

`matrix`는 하나의 local TaskDocument를 명시한 named profile 목록으로 확장하는 saved
Workflow mode다. 첫 버전의 `mode = "matrix"`는 exactly one task x many named profiles만
허용한다. `batch`나 `stack`처럼 여러 task 자체를 뜻하지 않고, profile 축으로 여러
profile-specific TaskRun/worktree를 만드는 실행 형태다. `wt workflow task --mode matrix
--profiles <name>[,<name>...] <task>`가 canonical creation surface이고, profile list는
Workflow TOML의 `profiles = [...]`에 사용자가 넘긴 순서로 저장한다. `--profiles`는
repeatable할 수 있지만 `--mode matrix` 없이 쓰면 안 되고, `--profile`과 동시에 쓰면 안
된다. Unknown profile, duplicate profile, reserved `default` profile name은 worktree,
TaskRun, Workflow 파일을 만들기 전에 실패해야 한다. 수동 Workflow TOML도 `mode =
"matrix"`에서 task가 1개가 아니거나 `profiles`가 비어 있거나 task row가 profile별
`[[tasks.runs]]`를 정확히 저장하지 않으면 invalid state로 거부한다.

Direct `wt task run`은 immediate single-worktree path다. `wt task run <task>`와
`wt task run <task> --profile <name>`만 소유하고, profile fan-out을 소유하지 않는다.
Direct `wt issue --matrix`와 `wt new --matrix`의 legacy all-named-profiles behavior는
보존하되 selected profile subset은 Workflow matrix로 표현한다.

`wt new <words...>`는 branch-name text에서 바로 ad hoc worktree를 시작한다. 즉시
준비된 TaskDocument를 실행하는 표면은 `wt task run [<task>...]`이다. 여러
TaskDocument를 하나의 저장된 실행 계획으로 묶어 batch coordination을 해야 하면
`wt workflow task --mode batch`와 `wt workflow run`을 쓰고, 하나의 shared workspace에서
실행해야 하면 `wt workflow task --mode single`과 `wt workflow run`을 쓴다. `wt new`에
prepared-task 실행 의미를 계속 넓히면 ad hoc branch worktree, immediate task run,
saved workflow가 한 명령에서 섞인다.

`wt open`은 issue selector가 아니라 branch/worktree 상태 selector다. 선택지는 현재
checkout을 제외하고 `existing`(이미 별도 worktree가 있음), `local`(local branch만
있음), `remote`(origin branch만 있음)으로 나뉜다. Linear나 GitHub issue 번호를 추정해
분류하지 않는다. issue provider가 제안한 branch와 `worktree.naming`으로 만든 branch가
다를 수 있기 때문이다.

이 개념들이 섞이면 사용자는 workflow가 실행환경인지, mode가 상태 파일 noun인지,
profile이 작업 묶음인지 다시 추론해야 한다. 이런 혼동은 기능 추가보다 먼저 제거한다.

### Omission Means Default Behavior

생략은 기본 동작을 뜻한다. 생략을 특정 이름으로 저장하거나 노출하지 않는다.

`default`는 profile 이름이 아니라, 사용자가 profile을 명시하지 않았을 때 적용되는
선택 규칙이다. 따라서 `default`를 실제 profile 이름처럼 다루면 안 된다.

기본값은 편의 기능이지만, 이름 있는 리소스처럼 보이는 순간 UX 부채가 된다.

### Ambiguity Fails Early

애매한 조합은 추론하지 말고 거부한다.

예를 들어 direct `--profile`은 “하나의 profile 선택”을 뜻하고, `wt workflow task --mode
matrix --profiles`는 “명시한 profile subset을 저장된 workflow로 확장”을 뜻한다.
`--profile`과 `--profiles`, direct `wt task run --matrix`, matrix workflow의 여러 task처럼
이 조합들이 충돌하면 임의로 우선순위를 정하지 않는다.

명령은 사용자가 의도를 잘못 표현했을 때 조용히 다른 일을 해서는 안 된다. 빠르게
실패하고, 어떤 선택을 해야 하는지 알려줘야 한다.

### Help Text Is a Contract

`--help`에 보이는 설명은 실제 동작과 같아야 한다.

도움말에 보이는 명령은 실제로 지원되어야 하고, 숨겨진 의미를 알아야만 사용할 수 있는
옵션은 없어야 한다. 옵션 설명이 “무엇을 하는지”가 아니라 “언제 어떤 개념을 선택하는지”
를 설명해야 한다.

도움말을 읽고 생긴 기대와 실제 동작이 다르면 구현이 아니라 UX가 깨진 것이다.

Interactive prompt도 CLI contract다. 사용자가 값을 생략해서 selector가 열리는
command는 무엇을 고르는지, 한 개를 고르는지 여러 개를 고르는지, 빈 선택이 허용되는지
문서와 help text에서 같은 말로 설명해야 한다. Selector는 작은 terminal prompt로
동작하고, filterable list와 최대 10개 visible row 안에서 task, branch, PR, workflow,
config section 같은 현재 concept label만 보여준다. 색상, symbol, checkbox는 보조
표현일 뿐이고 의미는 text label에 남아야 한다. 보조 metadata가 있는 row는 같은 prompt
page 안에서 hint column을 맞춰 보여주되, filter 대상은 padding이 아니라 concept label과
metadata text여야 한다. Metadata가 없는 plain label selector에는 가짜 column을 만들지
않는다.

### Progressive Disclosure

처음 쓰는 경로는 짧아야 하고, 복잡한 경로는 필요해질 때 드러나야 한다.

간단한 실행환경은 작게 시작할 수 있어야 한다. prompt, scaffold, agent별 파일처럼
복잡한 요소가 필요해질 때 더 구조화된 profile로 옮겨갈 수 있어야 한다.

이때 중요한 것은 두 경로가 다른 개념처럼 보이지 않는 것이다. 단순한 형태와 복잡한
형태는 같은 profile 모델의 두 표현이어야 한다.

`wt init`은 단순히 작은 config 파일을 쓰는 명령이 아니라 workspace starter wizard다.
Interactive TTY에서 bare `wt init`은 starter shape, config target, agent runtime, issue
provider, site provider, workspace tabs, setup deps, tests, editor 같은 질문을 guided flow로
묻는다. 쓰기 전에는 어떤 target file에 어떤 starter preset과 config section이 생성될지
명확히 보여주고 확인을 받아야 한다.

Canonical starter preset 이름은 `minimal`, `agent`, `issue`, `app`이다.

- `minimal`은 가장 작은 유용한 config다. Worktree를 만들 수 있는 기본값만 두고 agent,
  issue provider, site provider, setup deps, tests, editor 설정은 명시적으로 선택하지
  않는 한 쓰지 않는다. `wt init --minimal`은 이 canonical preset의 짧은 경로다.
- `agent`는 `minimal`에 inline `[profile.agent]` runtime을 더한 starter다. `--yes`에서는
  명시적 `--agent`가 없으면 `codex`를 default agent로 사용하되, 자세한 prompt/scaffold
  파일은 만들지 않는다.
- `issue`는 provider issue에서 workspace를 시작하기 위한 starter다. `--yes`에서는
  명시적 `--issue-provider`가 없으면 `github`를 default provider로 사용한다. Provider를
  repo 상태에서 추론하지 않는다.
- `app`은 local app repo에서 반복 실행할 setup deps, tests, site, workspace tabs를 잡는
  starter다. Detected command는 사용자가 선택했거나 non-interactive default로 안전하게
  설명할 수 있을 때만 active config로 쓴다.

preset을 명시하지 않은 `wt init --yes`는 non-interactive default를 받아들이는 자동화 경로이며
`minimal` preset을 선택한다. TTY가 아니면 `--yes`, `--minimal`, 또는
`--preset <name>`처럼 prompt 없이 끝낼 수 있는 starter 선택이 있어야 하며, 그렇지 않으면
interactive prompt를 시도하지 말고 명확한 에러로 실패한다.
`wt init --preset <name> --yes`는 반복 가능한 automation 표면이므로 같은 repo 상태와 같은
flag 조합에서 같은 config content를 만들어야 한다. `app` starter는 repo manifest를 scan해
setup command, dev tab, test command 후보를 plan에 반영한다. `wt init --dry-run`은 같은
validation을 거친 뒤 생성될 target, preset, section, detected signal, TOML content를
preview하고 파일을 쓰지 않는다.

Generated output은 여전히 사용자가 선택한 config 파일 하나에만 쓴다. `.wt.toml`과
`.local/.wt.toml` 중 하나를 선택하고, 답한 설정은 그 파일에만 쓴다. 다른 config 파일,
named profile directory, prompt/scaffold 파일은 `wt init`의 부수 효과로 만들지 않는다.
그런 구조가 필요하면 `wt config extract`나 `wt profile create`로 드러낸다. 나중에 starter
scaffold generation을 추가하더라도 별도의 명시적 starter choice로 다뤄야 한다.

`wt init --help` contract도 이 모델을 따라야 한다. Subcommand 설명은 “start a
workspace config wizard”를 말해야 하고, `--minimal`, `--preset <minimal|agent|issue|app>`,
`--yes`, `--dry-run`, `--local`, `--shared`는 starter shape, automation, preview, target
file 선택을 설명해야 한다. Help text는 named profile directory나 prompt/scaffold file을
자동 생성한다고 암시하면 안 된다.

Prompt도 같은 원칙을 따른다. `common`은 별도 실행 mode가 아니라 기존
`[agent.prompt]` / `[agent.prompt.append]` 모델 안의 공통 scope다. Config layer와
profile convention file merge를 모두 끝낸 뒤 최종 effective config에서 한 번만
`issue`, `new`, `pr` prompt 앞에 펼친다. `common`을 각 layer마다 mode별 prompt로
복사하지 않는다.

`workflow`도 Workflow `mode = "single" | "batch" | "stack"`나 setup mode가 아니라
`[agent.prompt]` / `[agent.prompt.append]` 안의 workflow-started task 전용 scope다.
`wt workflow run`으로 시작한 task에만 적용하고, direct `wt task run`, `wt issue`,
`wt new`, `wt pr`에는 적용하지 않는다. Workflow task의 setup mode는 계속
TaskDocument origin에 따라 `issue` 또는 `new`를 사용하므로 기존 setup-mode prompt도
함께 적용된다. `common`은 `workflow`로 펼치지 않는다. Workflow task는 이미 `issue` 또는
`new` prompt를 받기 때문에 `common`을 `workflow`에도 펼치면 같은 공통 지시가 중복된다.
Profile convention file은 `.local/profiles/<name>/prompts/workflow.md`와
`.local/profiles/<name>/prompts/workflow.append.md`를 같은 scope로 읽는다.

### State Is Explicit

저장되는 상태는 사용자가 이해할 수 있는 상태여야 한다.

TaskDocument는 작업이 무엇인지를 담는 정의다. `.local/tasks/<task>.toml`
아래에 title, branch, body, origin처럼 실행과 무관하게 읽을 수 있는 정보를 둔다.

`wt task list`는 `.local/tasks/<task>.toml`에 저장된 TaskDocument file의 canonical
read-only inventory다. `wt task run`의 runnable selector가 아니므로 이미 완료된
TaskRun 때문에 selector에서 빠지는 TaskDocument도 보여주고, selector의 10-row visible
cap을 적용하지 않는다. Text output은 selector와 같은 TaskDocument display order인
title, origin/publish state, task key, branch를 먼저 보여주고, inventory-only field인
source, path, origin, 짧은 body summary를 함께 보여준다. JSON output은 TaskDocument의
key, path, title, branch, origin/publish state, local-vs-provider-origin source, 짧은
body summary를 stable shape로 보여준다.
TaskDocument TOML parse/validation failure는 조용히 숨기지 않고 text warning 또는 JSON
`invalid_tasks`로 보고한다. `wt task list`는 worktree, local branch, TaskRun, Workflow,
provider issue, pull request, agent setup을 만들거나 수정하지 않는다. Workflow inventory는
계속 `wt workflow list`, worktree/branch/site state는 계속 `wt list`가 맡는다.

TaskDocument import는 configured issue provider의 기존 issue를 local task 정의로
가져오는 side effect다. Canonical command shape는 `wt task import` 또는
`wt task import <issue>...`다. Bare `wt task import`는 provider issue를
multi-select로 고르게 하고, 명시 issue id는 scriptable path로 남긴다. `import`는
`.local/tasks/<safe-issue-id>.toml`에 title, branch, body, `[origin]`을 기록한다.
이때 branch는 `wt issue <issue>`가 사용할 provider issue branch와 같은 값이어야 하며,
필요하면 provider branch를 먼저 materialize한다. GitHub에서는 linked branch가 없을 때
`gh issue develop`을 호출할 수 있다. Import는 provider branch materialization 외에는
worktree, local branch, TaskRun, Workflow, pull request, agent setup을 만들지 않는다.
Provider가 branch를 공급하거나 materialize할 수 없으면 branch가 빈 TaskDocument를 쓰지
말고 실패해야 한다. `[origin]`은 provider issue와의 durable link이지, 자동 동기화 계약이
아니다.

Import ambiguity는 local TaskDocument write 전에 실패해야 한다. Configured issue
provider가 없으면 실패한다. 같은 invocation 안의 duplicate issue id는 실패한다. Provider
조회 뒤 canonical issue id가 같은 task key로 수렴하는 경우도 실패한다. Import 대상
`.local/tasks/<safe-issue-id>.toml`이 이미 있으면 local edits를 보존하기 위해 실패하고,
조용히 덮어쓰거나 merge하지 않는다. Replace/update가 필요하다면 별도의 명시 옵션과
help/test/documentation이 먼저 필요하다.

TaskDocument publish는 local task 정의를 configured issue provider의 issue로 만드는
side effect다. Canonical command shape는 `wt task publish` 또는
`wt task publish <task>...`다. Bare `wt task publish`는 아직 `[origin]`이 없는 local
TaskDocument를 multi-select로 고르게 하고, 명시 task key는 scriptable path로 남긴다.
`publish`는 각 task의 provider issue 생성과 `.local/tasks/<task>.toml`의 `origin`
업데이트가 모두 끝났을 때만 해당 task를 성공으로 보고한다. 둘 중 하나만 끝난 상태를
성공으로 보고하지 않는다. `origin`은 external issue와의 durable link이지, 아직
publish해야 한다는 pending request가 아니다.

`wt issue`는 이미 존재하는 provider issue에서 worktree를 시작하는 명령으로 남긴다.
Provider issue를 TaskDocument로 가져오는 흐름은 `wt task import`, Local TaskDocument를
provider issue로 만드는 흐름은 `wt task publish`다. `wt issue import`, `wt issue create`,
`sync`, `pull`, `push`, `export` 같은 이름을 같은 개념의 alias로 추가하지 않는다.

Publish는 TaskDocument의 schema를 넓히지 않는다. TaskDocument에는 계속 title, branch,
body, optional origin만 둔다. TaskRun, workflow, profile, retry status, pending
publish state는 TaskDocument에 저장하지 않는다. Publish selector는 어떤 local
TaskDocument를 고를지에만 관여하고, provider issue link는 선택된 각 TaskDocument의
`origin`에만 기록한다.

Publish ambiguity는 provider side effect 전에 실패해야 한다. Explicit task keys,
bare selector 외에 workflow alias 같은 두 번째 task source를 만들면 안 된다.
Configured issue provider가 없으면 실패한다. Bare selector에서는 이미 `origin`이 있는
TaskDocument를 보여주지 않는다. 명시 task key에 이미 `origin`이 있으면 해당 task는
실패이며, 같은 task를 조용히 다시 publish해서 duplicate issue를 만들지 않는다. 이미
publish된 task는 `--skip-existing` 같은 명시적 옵션이 있을 때만 skip할 수 있다. 기존
`origin.provider`가 configured issue provider와 다르면 provider mismatch로 실패한다.
Provider issue title로 쓸 `title`은 필요하므로 비어 있으면 실패한다. `body`는 없거나
비어 있어도 empty issue body로 publish한다.

Dry-run은 첫 write-path의 필수 표면이 아니다. 추가한다면 실제 publish와 같은 validation을
거친 뒤 생성될 provider, title, body, branch metadata, 업데이트될 `origin` 위치를 보여주는
plan이어야 하고, TaskDocument에 pending state를 저장해서 dry-run 결과를 표현하지 않는다.

`wt task publish --help`는 이 side effect를 그대로 설명해야 한다. 즉 provider issue를
생성하고 local TaskDocument origin을 기록한다는 점, 이미 origin이 있거나 provider가
불명확하면 실패한다는 점, bare publish는 아직 origin이 없는 TaskDocument를 고른다는 점을
보여줘야 한다. Worktree 시작, TaskRun 생성, workflow 실행, branch landing처럼 다른
lifecycle을 publish 도움말에 섞지 않는다.

`wt task import --help`는 import가 provider issue에서 TaskDocument로 향하는
non-executing 흐름임을 그대로 설명해야 한다. 즉 explicit issue id와 bare provider issue
selector를 모두 지원한다는 점, title/branch/body/`[origin]`을 기록한다는 점, provider
branch materialization은 할 수 있지만 worktree/local branch/TaskRun/Workflow/PR/agent
setup은 만들지 않는다는 점, duplicate ids나 existing TaskDocument collision에서
실패한다는 점, branch를 materialize할 수 없으면 incomplete TaskDocument를 쓰지 않고
실패한다는 점을 보여줘야 한다.

TaskRun은 그 작업을 한 번 실행한 인스턴스다. `.local/task-runs/<id>.toml` 아래에
task, branch, status, group, error, creation_order, created_at, updated_at을 저장한다.
`group`은 Workflow file stem과 맞는 workflow-linked run을 식별하는 link이고, 직접
`wt task run`으로 만든 TaskRun은 group을 저장하지 않는다. Legacy TaskRun TOML의
source 값 `new`, `batch`, `stack`은 읽기 전용 migration compatibility로만 받으며 새
TaskRun 출력에는 쓰지 않는다. `creation_order`는 같은 task의 최신 실행을 고를 때 파일명이나
초 단위 timestamp 우연성에 기대지 않도록 새 TaskRun마다 증가하는 실행 생성 순서다.
`creation_order`가 없는 previous TaskRun은 계속 읽되 ordered TaskRun보다 앞에 정렬하고,
previous끼리는 `created_at`과 id를 fallback으로 쓴다.
status는 `prepared`, `running`, `done`, `failed`, `skipped`만 canonical이다. 알 수 없는
status나 workflow mode 값은 조용히 해석하지 않고 파싱 단계에서 실패시킨다.

통합 실행 상태 모델은 TaskDocument, Workflow, TaskRun의 책임을 나누는 데서 시작한다.
TaskDocument는 무엇을 할지에 대한 재사용 가능한 설명이고, Workflow는 optional
`objective`와 그 task set을 어떤 실행 shape로 이어갈지에 대한 저장된 계획이며,
TaskRun은 TaskDocument 하나를 한 번 실행한 기록이다.

Workflow 준비는 `.local/workflows/<id>.toml` 하나와 각 task의 TaskDocument/TaskRun link를
만든다. Workflow의 canonical task 목록은 `[[tasks]]`이고, 각 row는 task key, linked
TaskRun id, stack-mode parent처럼 orchestration에 필요한 link와 실행 지시만 저장한다.
Workflow row는 status/error를 따로 가지지 않고, branch 이름도 복사하지 않는다. 실행
인스턴스의 canonical 기록은 TaskRun이고, branch name의 canonical 기록은 TaskDocument다.
`objective`는 workflow-level field로만 저장하고 TaskDocument `body`나 row-level field로
복사하지 않는다. `body`, `description`, `goal_task`, `parent_task`, `subtasks`,
`[[issues]]`, `[[items]]`처럼 같은 상태나 목표를 가리키는 다른 이름은 받지 않는다.

Workflow preparation accepts `--pr <none|draft|ready>` as a one-run override for
pull-request handoff intent. Omitted `--pr` means use the effective `[workflow]`
config. `--pr none` means agents report `PR=none`, `--pr draft` means agents open draft
pull requests and leave them draft, and `--pr ready` means agents open pull requests that
are ready for review immediately. Boolean `--pull-request` and boolean
`pull_request = true/false` are not canonical workflow surfaces.

Workflow policy is a preparation preference in `.wt.toml`, while a Workflow file is the
prepared execution plan for one run. Preparing a workflow reads the effective config
from `.wt.toml` plus `.local/.wt.toml`, applies any explicit command-line override, and
writes the resulting policy snapshot into `.local/workflows/<id>.toml`. Later edits to
`.wt.toml` do not reinterpret already prepared workflows.

Canonical config shape:

```toml
[workflow]
pull_request = "none"  # none | draft | ready
landing = "manual"     # manual | auto
```

`pull_request` is the default pull-request handoff intent for workflow tasks. `none`
means agents report `PR=none` and do not create pull requests. `draft` means agents open
draft pull requests and leave them draft. `ready` means agents open pull requests that
are ready for review immediately. `ready` is the canonical name; `open`, `review`,
boolean `true`, and boolean `false` are not aliases.

`landing` is the coordinator preference after review passes. Review is always part of
the coordinator flow, and config cannot disable review. `manual` means review completes
and the coordinator stops before merge or cleanup until the user explicitly directs
landing. `auto` means review passing is enough approval for the coordinator to proceed
to landing and cleanup. `auto` does not bypass dirty-worktree checks, configured check
commands, required pull-request checks, unresolved review threads, branch ancestry
checks, workflow mode ordering, or any other landing safety gate.

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
```

Workflow policy is intent, not state: actual pull-request review result, merge status,
ancestry proof, worktree cleanup, branch deletion, TaskRun lifecycle status, and
TaskDocument cleanup remain outside Workflow policy. `wt inspect`, pull-request state,
Git commands, workflow completion, and `wt done` continue to own those checks and
transitions explicitly.

The built-in config defaults are `pull_request = "none"` and `landing = "manual"`.
Explicit workflow preparation flags override the config for one run while keeping the
same value names and failing early for conflicting forms instead of introducing aliases.
`wt config` shows the effective `[workflow]` policy, including built-in defaults, so
scripts and humans can inspect the actual policy that new workflow preparation will use.
`wt init` may include a commented optional `[workflow]` block for discoverability, but
generated config must not actively enable pull-request creation or automatic landing by
default. `wt workflow show` displays the prepared policy snapshot from the workflow file,
not the current `.wt.toml` value.

This model changes both `.wt.toml` config shape and `.local/workflows` state shape, so
implementing parser/runtime behavior is a pre-1.0 minor user-facing change. Adding
workflow `objective` also changes the `.local/workflows` state shape and
`wt workflow task` / `wt workflow issue` preparation surface, so it belongs in the same
pre-1.0 minor model-change category. Ordinary development commits still do not bump
`Cargo.toml`; the release branch owns the eventual version bump.

`wt workflow task --objective <text>`와 `wt workflow issue --objective <text>`는 저장된
Workflow의 더 큰 목표를 `.local/workflows/<id>.toml`의 top-level `objective`로 기록한다.
이 값은 `wt workflow show`와 workflow-started agent prompt에 context로 나타나지만,
runnable selection, TaskRun lifecycle, landing policy, cleanup behavior를 바꾸지 않는다.
Prompt에서는 coordinator handoff가 먼저 전달되고, objective는 그 뒤 TaskDocument snapshot
근처에 배치된다.
Bare `wt workflow task --mode <mode>`는 기존 local TaskDocument를 multi-select로 고른다.
명시 task argument는 scriptable path이며, 선택과 명시 argument를 한 command에서 섞는
두 번째 task source를 만들지 않는다.

`.local/workflows`는 `.local/batches`와 `.local/stacks`를 대체한다. 이유는 batch와 stack이
저장소 noun이 아니라 하나의 Workflow 안에서 고르는 execution mode이기 때문이다. 새
기능이 `.local/batches`나 `.local/stacks`에 상태를 계속 추가하면 사용자는 같은 준비 작업을
workflow, batch file, stack file 중 무엇으로 읽어야 하는지 다시 배워야 한다. 새 canonical
state는 Workflow file 하나로 수렴시킨다.

`single` mode workflow는 하나의 branch workspace에서 하나 이상의 TaskDocument를 실행한다.
`batch` mode workflow는 같은 base에서 여러 TaskDocument를 독립 branch로 실행한다. Batch
task들은 독립적이므로 이미 `running`인 TaskRun이 있어도 prepared/failed sibling이 있으면
workflow는 runnable로 남을 수 있다. `stack` mode workflow는 TaskDocument를 base-to-top
parent chain으로 실행하고, current `running` TaskRun이 있으면 다음 task를 시작하지 않는다.
Stack-mode에서 `running`은 agent prompt 전송이 아니라 사용자나 agent의 명시적 completion
신호를 기다리는 상태다. 완료를 추정해서 다음 task를 시작하지 않는다.

`wt workflow run`에서 workflow target 생략은 runnable workflow를 고르는 기본 동작이다.
`single`은 linked TaskRun 전체가 `prepared` 또는 `failed`일 때만 runnable이고, `batch`는
하나 이상의 linked TaskRun이 `prepared` 또는 `failed`이면 runnable이며, `stack`은 다음
`prepared` 또는 `failed` task가 있고 현재 `running` task가 없을 때 runnable이다. 명시
workflow id/path는 automation surface로 남긴다.

`wt workflow list`는 `.local/workflows/<id>.toml`에 저장된 Workflow file의 canonical
read-only inventory다. `wt workflow run`의 runnable selector가 아니므로 runnable workflow만
필터링하거나 selector의 10-row visible cap을 적용하지 않는다. `wt workflow show`의 latest
default도 all-workflow inventory로 확장하지 않는다. Output은 Workflow 자체의 단일
`status`를 만들지 않고, linked TaskRun에서 파생한 task-run status count/summary와 mode별
runnable metadata를 보여준다. Human text output은 workflow id/mode, TaskRun summary,
runnable summary, updated timestamp를 primary line에 두고 objective, base/profile/policy,
path는 secondary detail line에 둔다. JSON output은 raw runnable reason identifiers를
계속 machine-readable metadata로 보존한다. Workflow TOML parse/validation failure는 조용히 숨기지 않고
text warning 또는 JSON `invalid_workflows`로 보고한다. Batch/stack은 계속 Workflow `mode`
값일 뿐이므로 `wt list workflow`, top-level `batch`/`stack`, `wt profile list` 같은 symmetry
command를 추가하지 않는다. `wt task list`는 symmetry command가 아니라 별도 TaskDocument
inventory surface이며 Workflow, TaskRun, branch, worktree 목록 의미를 갖지 않는다.

Workspace label은 저장 상태가 아니라 현재 실행을 찾기 위한 표시다. 좁은 탭에서 잘려도
의미가 남도록 `2/5 PROJ-123 Title`처럼 짧은 order 라벨을 앞에 붙이고, branch/path/site
이름에는 `batch`나 `stack` 같은 mode label을 섞지 않는다. `B`/`S` prefix는 workflow
contract에 포함하지 않는다.

`wt task run` coordinator handoff는 즉시 TaskDocument 실행 handoff다. `wt task run`이
시작하는 prompt에는 `Task Run Coordinator Handoff` section이 포함되고, 현재 coordinator
cmux workspace/surface 좌표로 렌더링되는 `cmux send`와 `cmux send-key ... enter` 명령이
들어간다. 이것은 Workflow orchestration이나 completion command가 아니다. Task-run agent는
`PR=none`인 `Agent Completion Report`를 coordinator에게 보내고, coordinator가 review,
landing, cleanup을 명시적으로 처리할 때까지 기다린다. 좌표는 현재 transport 정보일 뿐이므로
TaskDocument나 TaskRun에 저장하지 않는다. 좌표가 unavailable 또는 stale이면 agent는 같은
보고를 task session에 남기고 기다린다. Handoff section과 그 안의 `cmux send`/enter 명령은
긴 TaskDocument 본문과 분리된 첫 prompt로 먼저 보내서 terminal prompt가 축약되어도
coordinator 좌표가 앞쪽에 남게 한다.

Workflow coordinator handoff는 `stack` 전용 개념이 아니라 `wt workflow run`이 시작하는
모든 task prompt의 계약이다. Prompt에는 `Workflow Coordinator Handoff` section이 포함되고,
현재 coordinator cmux workspace/surface 좌표로 렌더링되는 `cmux send`와
`cmux send-key ... enter` 명령이 들어간다. 이 좌표는 현재 transport 정보일 뿐이므로
Workflow file, TaskRun, TaskDocument에 저장하지 않는다. 좌표가 unavailable 또는 stale이면
agent는 같은 `Agent Completion Report`를 task session에 남기고 기다린다. Handoff section과
그 안의 `cmux send`/enter 명령은 긴 TaskDocument 본문과 분리된 첫 prompt로 먼저 보내서
terminal prompt가 축약되어도 coordinator 좌표가 앞쪽에 남게 한다.
사용자 정의 `[agent.prompt.workflow]` prompt가 있으면 이 built-in handoff와 TaskDocument
snapshot 뒤, 기존 `issue`/`new` setup-mode prompt 앞에 보낸다.

보고 형식은 workflow mode와 무관하게
`Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>`
이다. `PR` 값은 workflow file의 prepared policy를 따른다. `pull_request = "none"`이면
pull request를 열지 않고 `PR=none`으로 보고한다. `"draft"`는 작업 agent가 branch를
push하고 준비된 workflow base 또는 parent branch를 base로 draft pull request를 열어 draft로
남긴다는 뜻이다. `"ready"`는 draft를 만들었다가 전환하지 않고 바로 review-ready pull request를
연다는 뜻이다. PR을 여는 workflow task는 `.github/pull_request_template.md`에서
`<pr-body-file>`을 만들고 summary, context, changes, validation, risks/follow-ups 중심의
review-focused 본문을 채운다. TaskDocument에 `[origin]`이 있으면 PR merge가 provider
issue를 닫도록 `Closes <origin.id>` issue-closing keyword도 PR 본문에 포함한다. 그런 뒤
`gh pr create --body-file <pr-body-file>` 경로로 PR을 생성한다.
Agent Completion Report는 coordinator transport/report 형식이며 PR 본문으로 복사하지 않는다.
이것은 PR 자체나 review 상태가 아니라 다음 실행자에게 전달할 작업 계약이다. 보고 전송은
transport일 뿐 상태 전이가 아니다. Review는 항상 coordinator flow에 포함된다. Pull request
review나 coordinator가 전달한 리뷰는 해당 task agent가 반영하고, 필요한 check를 다시 돌린 뒤
commit/push하고 PR 본문이 stale해졌을 때만 PR 본문과 Agent Completion Report를 갱신한다.
실행자나 coordinator가 `wt inspect`, 필요한 경우 pull request, 보고를 확인한 뒤 workflow
completion command를 실행할 때 TaskRun 상태가 전이된다. Pull request가 있으면 coordinator는
workflow completion이나 landing 전에 pull-request review gate를 통과했는지 별도로 확인한다.
이 gate는 unresolved thread가 0인지뿐 아니라 최근 reviewer 또는 review-agent 답글, PR comment,
review-request reaction, check 상태를 포함한다. Review-agent thread는 coordinator 답글 직후
바로 resolve하지 않고, follow-up을 refresh해서 해결 또는 비조치 동의가 확인된 뒤 resolve한다.

`wt done`은 worktree와 local branch cleanup 명령이다. `done`은 cleanup 신호이고,
workflow completion은 실행 완료 신호이며, `merge`/`land`는 branch commit을 `master` 같은
통합 branch에 넣는 Git workflow다. `wt done`이나 workflow completion command가 branch를
`master`에 merge했다고 해석하지 않는다. 현재는 별도 `wt land` 명령을 만들지 않고,
`git switch master`, `git pull --ff-only`, `git merge --ff-only <branch>` 같은 명시적 Git
단계로 landing을 문서화한다. Stack-mode workflow branch는 workflow가 보여주는 base-to-top
순서대로 landing한다.

`wt done <target>`의 explicit cleanup target은 branch, worktree path/name,
issue-like branch-name shorthand, direct TaskRun id다. Direct TaskRun id는 해당 TaskRun의
branch를 checked-out worktree로 해석한 뒤 같은 cleanup path를 탄다. Workflow-linked
TaskRun id는 workflow completion을 우회하지 않도록 거부하고 `wt inspect`와
`wt workflow complete` 경로를 안내한다. Issue shorthand는 provider issue lookup이 아니라
현재 branch text에 대한 compatibility shorthand다.

Local task cleanup도 별도 단계다. TaskDocument는 재사용 가능한 work definition이므로
기본적으로 보존한다. 한 번 실행하고 끝난 task라도 linked TaskRun과 Workflow reference가
정리되기 전까지 TaskDocument 삭제를 execution completion에 섞지 않는다. 나중에 `wt land`,
`wt task clean`, `wt run clean`, `wt workflow clean` 같은 명령을 만들더라도 `done`이나
`complete`에 merge나 task definition 삭제 의미를 섞지 않는다.

`wt inspect [<target>]`는 branch, worktree, TaskRun을 읽어서 parent, dirty 상태,
commit/diff 정보, Agent Completion Report 기대치, 현재 cmux contact를 보여주는 canonical
read-only dossier다. Agent observation snapshot을 같이 보여줄 수 있지만, `inspect`의 exit
code는 command 자체의 성공/실패만 뜻한다. 관찰된 agent가 `needs_input`이거나 `failed`여도
그 사실만 출력하고 polling용 exit code로 바꾸지 않는다. 실제 완료 기록은 direct 또는
workflow-linked context별 명령이 맡는다. 직접 `wt task run`이 만든 TaskRun은 review/landing
확인 뒤 `wt done` cleanup이 정리할 수 있고, Workflow file의 `[[tasks]].run`과 matching
`group`으로 연결된 TaskRun은 workflow completion command가 전이한다.

`wt inspect`에서 `<target>` 생략은 interactive TTY human mode에서 inspectable work target
selector를 여는 기본 동작이다. `--json`, `--quiet`, 또는 non-TTY automation에서는 selector를
열지 않고 explicit `<target>`을 요구해야 한다. 실패 메시지는 branch, worktree path/name,
TaskRun id 중 하나를 넘기거나 interactive TTY에서 selector를 열라는 guidance를 정확히
보여줘야 한다.

`wt send`도 상태 전이 명령이 아니다. `wt inspect`와 같은 target 해석으로 현재 cmux
surface를 찾아 메시지를 보내는 transport 명령이다. 메시지를 보냈다는 사실을 TaskRun
상태로 저장하지 않고, 완료 여부는 여전히 TaskRun status와 workflow completion command로만
표현한다.

`wt agent status [<target>]`는 현재 agent/cmux observation surface다. `target`은
`wt inspect`와 `wt send`가 받는 branch, worktree path/name, TaskRun id와 같은 work selector다.
이 명령은 현재 cmux workspace/surface와 agent 화면/status/event를 관찰해서 agent-friendly
text/JSON 상태를 돌려주며, TaskRun status나 provider issue status를 쓰지 않는다. Text 출력은
target, branch, TaskRun lifecycle status, agent kind/state, cmux contact, 마지막
tool/session/event, warning을 compact하게 보여준다. JSON에는 top-level `status`를 만들지
않고 `task_run.status`와 `agent.state` 또는 `agent.status`를 서로 다른 nested field로 둔다.
`TaskRun.status`는 durable execution lifecycle이고, `agent.state`/`agent.status`는 현재
runtime observation이므로 한 top-level field 이름으로 합치지 않는다.

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

`wt agent status`와 `wt agent watch`의 `<target>` 생략도 interactive TTY human mode에서만
selector를 연다. `--json`, `--quiet`, 또는 non-TTY automation에서는 explicit `<target>`이
필수이며, omission은 `wt agent status <target>`, `wt agent watch <target>`, `wt inspect
[<target>]` 중 어떤 표면을 써야 하는지 알려주며 실패해야 한다.

Agent observation exit code는 agent command에만 속한다. `wt agent status`와
`wt agent watch`는 0을 observable/not blocked, 1을 target/session/cmux unavailable, 2를
`needs_input`, 3을 `failed`로 유지한다. cmux 자체를 사용할 수 없으면 성공한 `no_session`처럼
보이지 않도록 실패한다.

Agent별 상태 신호 준비도는 `agent status`나 `agent watch`가 고치는 대상이 아니라 관찰의
신뢰도 조건이다. Claude Code는 cmux의 Claude 통합에서 status/sidebar 신호가 나오고, Codex는
사용자가 `cmux hooks codex install --yes`를 명시적으로 실행한 뒤에야 `agent.hook.*`와
`set_status codex Running/Idle` 신호가 나온다. Codex hook이 없으면 agent commands는 화면
텍스트 fallback을 쓸 수 있지만 약한 관찰이라는 warning을 남겨야 한다. `wt doctor`는 이
준비도를 보고만 하고, 일반 wt 명령이 전역 Codex hook을 자동 설치하거나 사용자 agent config를
몰래 수정하면 안 된다.

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

여러 agent에 공통으로 적용되는 기능에는 특정 agent 이름을 붙이지 않는다.

특정 agent에만 적용되는 기능에는 그 agent 이름을 붙여도 된다. 하지만 Codex와 Claude
모두에 영향을 주는 기능이 `claude_*` 이름을 가지면 이름이 거짓말을 하게 된다.

이름은 구현의 과거가 아니라 현재의 의미를 설명해야 한다.

### Compatibility Does Not Create Aliases

호환성은 canonical 모델을 흐리게 만드는 두 번째 이름이나 상태 형태를 정당화하지
않는다.

사용자-facing 설정, 명령, 옵션, 상태 파일에는 같은 개념을 가리키는 호환 alias를
남기지 않는다. 이전 이름을 입력하면 새 이름으로 조용히 해석하지 말고 실패시켜야 한다.

예를 들어 local site 설정은 `[site] provider = "herd"`가 canonical이고, 같은 의미를
`[herd]` 섹션으로 다시 받지 않는다.

### Versioning Reflects Stability

버전 번호도 사용자-facing 계약이다.

`wt`는 아직 1.0에 도달하지 않았으므로 CLI, config, 상태 파일 모델이 안정화될 때까지
breaking change를 `x.0.0` major로 표현하지 않는다. 새 기능과 breaking user-facing
정리는 `0.x.0` minor로 올리고, 버그 수정이나 내부 로직 변경은 `0.x.y` patch로 올린다.
예를 들어 prepared-task 실행 표면을 `wt new --task`에서 `wt task run`으로 옮기거나,
saved orchestration을 `wt workflow`와 `.local/workflows`로 수렴시키는 변경은 CLI와 상태
파일 계약을 바꾸므로 patch가 아니라 pre-1.0 minor 변경이다.

다만 version bump는 일반 개발 커밋마다 하지 않는다. `develop`은 기본 개발 브랜치이고,
`master`는 릴리즈 브랜치다. 릴리즈 PR에서 `Cargo.toml`/`Cargo.lock` version을 한 번만
올리고, 그 릴리즈에 포함된 변경 중 가장 큰 SemVer 범위를 적용한다. 릴리즈 PR이
`master`에 merge되고 tag가 생성되면, version bump를 다시 `develop`에 merge해서 두
브랜치의 기준점을 맞춘다.

1.0 이후에만 기존 사용자-facing 계약을 깨는 변경을 major bump로 표현한다.

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
