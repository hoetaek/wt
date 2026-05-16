# Consistency Philosophy

이 문서는 `wt` 코드베이스가 지켜야 할 UX 일관성 원칙을 정리한다.

`wt`는 worktree, issue, pull request, profile, batch, stack, agent runtime처럼 서로
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

`profile`은 어떻게 실행할지에 대한 개념이다.

`batch`는 무엇들을 한꺼번에 실행할지에 대한 개념이다.

`stack`은 어떤 작업 task들을 어떤 순서의 branch parent 체인으로 쌓을지에 대한
개념이다.

`matrix`는 하나의 issue, branch-name 입력, 또는 명시적으로 선택한 prepared task를
named profile 목록으로 확장하는 개념이다. `batch`나 `stack`처럼 여러 task 자체를
뜻하지 않고, profile 축으로 여러 worktree를 만드는 실행 형태다.

`wt stack task`와 `wt stack issue`는 둘 다 stack 상태 파일과 task 문서를 만든다.
차이는 입력 소스다. `task`는 branch-name text에서 local task를 만들고, `issue`는
provider issue를 origin이 있는 task로 가져온다.

`wt new <words...>`는 branch-name text에서 바로 worktree를 시작한다.
`.local/tasks` 아래의 준비된 task를 시작하려면 `wt new --task` selector나
`wt new --task <task>`를 쓴다. 여러 prepared task를 한 workspace에서 처리하려면
`wt new <workspace-branch-words...> --task <task> --task <task>`처럼 `--task`를
반복한다. 이때 branch-name text는 공유 workspace branch이고, 각 TaskDocument는 같은
branch와 group을 가리키는 별도 TaskRun으로 기록된다. `--tasks` 같은 별도 복수 옵션은
만들지 않는다. Bare `wt new`는 여전히 거부한다. Bare `wt new --task`는 빈 입력을 다른
소스로 추론하는 것이 아니라, 사용자가 task selector를 명시한 것이다.

`wt open`은 issue selector가 아니라 branch/worktree 상태 selector다. 선택지는 현재
checkout을 제외하고 `existing`(이미 별도 worktree가 있음), `local`(local branch만
있음), `remote`(origin branch만 있음)으로 나뉜다. Linear나 GitHub issue 번호를 추정해
분류하지 않는다. issue provider가 제안한 branch와 `worktree.naming`으로 만든 branch가
다를 수 있기 때문이다.

이 셋이 섞이면 사용자는 batch가 실행환경인지, stack이 단순 실행 목록인지,
profile이 작업 묶음인지 다시 추론해야
한다. 이런 혼동은 기능 추가보다 먼저 제거한다.

### Omission Means Default Behavior

생략은 기본 동작을 뜻한다. 생략을 특정 이름으로 저장하거나 노출하지 않는다.

`default`는 profile 이름이 아니라, 사용자가 profile을 명시하지 않았을 때 적용되는
선택 규칙이다. 따라서 `default`를 실제 profile 이름처럼 다루면 안 된다.

기본값은 편의 기능이지만, 이름 있는 리소스처럼 보이는 순간 UX 부채가 된다.

### Ambiguity Fails Early

애매한 조합은 추론하지 말고 거부한다.

예를 들어 `--profile`은 “하나의 profile 선택”을 뜻하고 `--matrix`는 “모든
profile로 확장”을 뜻한다면, 둘을 동시에 받은 상태에서 임의로 우선순위를 정하지
않는다.

명령은 사용자가 의도를 잘못 표현했을 때 조용히 다른 일을 해서는 안 된다. 빠르게
실패하고, 어떤 선택을 해야 하는지 알려줘야 한다.

### Help Text Is a Contract

`--help`에 보이는 설명은 실제 동작과 같아야 한다.

도움말에 보이는 명령은 실제로 지원되어야 하고, 숨겨진 의미를 알아야만 사용할 수 있는
옵션은 없어야 한다. 옵션 설명이 “무엇을 하는지”가 아니라 “언제 어떤 개념을 선택하는지”
를 설명해야 한다.

도움말을 읽고 생긴 기대와 실제 동작이 다르면 구현이 아니라 UX가 깨진 것이다.

### Progressive Disclosure

처음 쓰는 경로는 짧아야 하고, 복잡한 경로는 필요해질 때 드러나야 한다.

간단한 실행환경은 작게 시작할 수 있어야 한다. prompt, scaffold, agent별 파일처럼
복잡한 요소가 필요해질 때 더 구조화된 profile로 옮겨갈 수 있어야 한다.

이때 중요한 것은 두 경로가 다른 개념처럼 보이지 않는 것이다. 단순한 형태와 복잡한
형태는 같은 profile 모델의 두 표현이어야 한다.

`wt init`은 사용자가 선택한 config 파일 하나를 만드는 시작점이다. 먼저 `.wt.toml` 또는
`.local/.wt.toml` 중 하나를 고르고, 이후 issue provider, site provider, agent runtime,
자주 쓰는 설정 질문은 타깃 구분 없이 동일하게 묻는다. 답한 설정은 선택한
파일에만 쓰고, 다른 config 파일이나 named profile directory, prompt/scaffold 파일은
부수적으로 만들지 않는다. Inline 설정을 구조화할 때 `wt config extract`나
`wt profile create`로 드러낸다.

Prompt도 같은 원칙을 따른다. `common`은 별도 실행 mode가 아니라 기존
`[agent.prompt]` / `[agent.prompt.append]` 모델 안의 공통 scope다. Config layer와
profile convention file merge를 모두 끝낸 뒤 최종 effective config에서 한 번만
`issue`, `new`, `pr` prompt 앞에 펼친다. `common`을 각 layer마다 mode별 prompt로
복사하지 않는다.

### State Is Explicit

저장되는 상태는 사용자가 이해할 수 있는 상태여야 한다.

TaskDocument는 작업이 무엇인지를 담는 정의다. `.local/tasks/<task>.toml`
아래에 title, branch, body, origin처럼 실행과 무관하게 읽을 수 있는 정보를 둔다.

TaskDocument publish는 local task 정의를 configured issue provider의 issue로 만드는
side effect다. Canonical command shape는 `wt task publish` 또는
`wt task publish <task>...`다. Bare `wt task publish`는 아직 `[origin]`이 없는 local
TaskDocument를 multi-select로 고르게 하고, 명시 task key는 scriptable path로 남긴다.
`publish`는 각 task의 provider issue 생성과 `.local/tasks/<task>.toml`의 `origin`
업데이트가 모두 끝났을 때만 해당 task를 성공으로 보고한다. 둘 중 하나만 끝난 상태를
성공으로 보고하지 않는다. `origin`은 external issue와의 durable link이지, 아직
publish해야 한다는 pending request가 아니다.

`wt issue`는 이미 존재하는 provider issue에서 worktree를 시작하는 명령으로 남긴다.
Local TaskDocument를 provider issue로 만드는 흐름을 `wt issue create`, `import`,
`sync` 같은 이름으로 추가하지 않는다. `import`는 provider issue를 TaskDocument로
가져오는 방향이고, `publish`는 TaskDocument를 provider issue로 내보내는 방향이다.

Publish는 TaskDocument의 schema를 넓히지 않는다. TaskDocument에는 계속 title, branch,
body, optional origin만 둔다. TaskRun, batch, stack, profile, retry status, pending
publish state는 TaskDocument에 저장하지 않는다. Publish selector는 어떤 local
TaskDocument를 고를지에만 관여하고, provider issue link는 선택된 각 TaskDocument의
`origin`에만 기록한다.

Publish ambiguity는 provider side effect 전에 실패해야 한다. Explicit task keys,
bare selector 외에 batch나 stack alias 같은 두 번째 task source를 만들면 안 된다.
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
보여줘야 한다. Worktree 시작, TaskRun 생성, batch/stack 실행, branch landing처럼 다른
lifecycle을 publish 도움말에 섞지 않는다.

TaskRun은 그 작업을 한 번 실행한 인스턴스다. `.local/task-runs/<id>.toml` 아래에
task, branch, status, source, group, error, creation_order, created_at,
updated_at을 저장한다. `creation_order`는 같은 task의 최신 실행을 고를 때 파일명이나
초 단위 timestamp 우연성에 기대지 않도록 새 TaskRun마다 증가하는 실행 생성 순서다.
status는 `prepared`, `running`, `done`, `failed`, `skipped`만 canonical이고, source는
`new`, `batch`, `stack`만 canonical이다. 알 수 없는 status/source 값은 조용히
해석하지 않고 파싱 단계에서 실패시킨다.

통합 실행 상태 모델은 TaskDocument와 TaskRun의 책임을 나누는 데서 시작한다.
TaskDocument는 무엇을 할지에 대한 재사용 가능한 설명이고, TaskRun은 그 설명을 한 번
실행한 기록이다. `wt new --task`, `wt new --task <task>`, `wt batch run`,
`wt stack run`은 모두 TaskDocument를 읽어 실행 context로 쓰고, 실행 상태는
`.local/task-runs`의 TaskRun에만 쓴다. 하나의
`wt new <workspace> --task <task> --task <task>` 실행은 workspace를 하나 만들 수 있지만,
실행 기록은 task마다 별도 TaskRun으로 남긴다.

Batch가 어떤 task를 준비했고, 어떤 task가 끝났고, 어떤 task가 실패했는지는 저장할
가치가 있다. Batch 준비는 각 task마다 `.local/tasks` 아래의 TaskDocument와
`.local/task-runs` 아래의 `source = "batch"` TaskRun을 만든다. Batch의 canonical
상태 목록은 `[[tasks]]`이고, 각 row는 task key와 linked TaskRun id 같은 orchestration
link만 저장한다. Batch row는 어떤 task들이 함께 시작 대상인지와 어떤 실행 기록을
읽어야 하는지만 저장하고, status/error를 따로 가지지 않는다. 실행 인스턴스의 canonical
기록은 TaskRun이다. `[[issues]]`나 `[[items]]`처럼 같은 상태 목록을 가리키는 다른
이름은 받지 않는다. 내부 구현 편의를 위해 만든 가짜 이름이나 암묵적 상태를 저장하면
나중에 사용자가 파일을 읽을 때 모델을 다시 배워야 한다.
Batch가 만든 cmux workspace 이름은 저장 상태가 아니라 현재 실행을 찾기 위한 표시다.
좁은 탭에서 잘려도 의미가 남도록 `B2/5 PROJ-123 Title`처럼 짧은 source/order 라벨을
앞에 붙이고, branch/path/site 이름에는 batch label을 섞지 않는다.

Stack이 어떤 task를 어떤 parent 위에 쌓았는지도 저장할 가치가 있다. Stack 준비는 각
task마다 `.local/tasks` 아래의 TaskDocument와 `.local/task-runs` 아래의
`source = "stack"` TaskRun을 만든다. canonical 상태 목록은 `[[tasks]]`이고, task 문서는
issue origin이 있는 작업과 직접 작성한 branch work를 같은 형태로 담는다. Stack row는
task key, parent, linked TaskRun id 같은 orchestration link만 저장하고, status/error를
따로 가지지 않는다. 실행 인스턴스의 canonical 기록은 TaskRun이다. `[[issues]]`나
`[[items]]`처럼 같은 상태 목록을 가리키는 다른 이름은 받지 않는다. Bare `run`은 runnable
stack 목록을 selector로 보여준다. Runnable stack은 다음 `prepared` 또는 `failed` TaskRun이
있고 current `running` TaskRun은 없는 stack이다. Selector label은 task titles/keys, next
task, status counts, base, profile 같은 semantic summary를 담아서 `.local/stacks/<date>.toml`
같은 파일명만 보고 고르게 하지 않는다. Explicit `wt stack run <path-or-id>`는 scripts를 위해
남기지만 `latest`는 run target contract가 아니다. `run`은 선택된 stack의 다음 runnable
TaskRun을 `running`으로 전이하고, 명시적 `complete` 신호가 들어오면 같은 TaskRun을
`done`으로 전이한다.
`pr`은 기존 pull request workflow를 가리키는 별도 개념이므로 stack task로 받지 않는다.
Stack이 만든 cmux workspace 이름도 저장 상태가 아니라 현재 실행을 찾기 위한 표시다.
좁은 탭에서 잘려도 의미가 남도록 `S2/5 PROJ-123 Title`처럼 짧은 source/order 라벨을
앞에 붙이고, branch/path/site 이름에는 stack label을 섞지 않는다.
Stack에서 `running`은 agent prompt 전송이 아니라 사용자나 agent의 명시적
`complete` 신호를 기다리는 상태다. 완료를 추정해서 다음 task를 시작하지 않는다.
`complete`는 branch가 clean이고 parent보다 앞선 commit이 있을 때만 `done`으로
전이해야 한다. 다음 task 자동 시작은 명시적인 continuation 선택, 예를 들어
`--run-next`로만 일어난다.
Stack task prompt는 작업 agent가 자기 판단만으로 stack을 전진시키기보다, 작업과
commit을 끝낸 뒤 repo나 coordinator workflow가 pull request review를 기대하는지
확인하도록 안내한다. PR workflow가 필요한 경우에만 branch를 push하고 stack parent
branch를 base로 draft pull request를 열게 한다. 그 다음 `wt send <coordinator-worktree>
...`로 실행자 worktree에 pull request URL 또는 `PR=none`을 포함한 Agent Completion
Report를 보내도록 안내한다. 이 보고 전송은 transport일 뿐 상태 전이가 아니다. 실행자나
master agent가 `wt review`, 필요한 경우 pull request, 보고를 확인한 뒤
`wt stack complete ... --run-next`를 실행할 때 TaskRun 상태가 전이된다.

`wt done`은 worktree와 local branch cleanup 명령이므로 `source = "new"`와
`source = "batch"`인 running TaskRun만 실제 worktree 제거와 함께 `done`으로
전이한다. Stack TaskRun은 parent-chain 검증이 필요한 실행 순서 모델 안에 있으므로
`wt stack complete`만 `done`으로 전이한다.

Branch landing은 TaskRun 상태와 별도 lifecycle이다. `complete`는 stack TaskRun을
검증 후 `done`으로 바꾸는 실행 완료 신호이고, `done`은 worktree와 local branch를
치우는 cleanup 신호이며, `merge`/`land`는 branch commit을 `master` 같은 통합 branch에
넣는 Git workflow다. `wt done`이나 `wt stack complete`가 branch를 `master`에
merge했다고 해석하지 않는다. 현재는 별도 `wt land` 명령을 만들지 않고,
`git switch master`, `git pull --ff-only`, `git merge --ff-only <branch>` 같은 명시적
Git 단계로 landing을 문서화한다. Stack branch는 `wt stack show`가 보여주는
base-to-top 순서대로 landing한다.

Local task cleanup도 별도 단계다. TaskDocument는 재사용 가능한 work definition이므로
기본적으로 보존한다. 한 번 실행하고 끝난 batch task는 모든 linked TaskRun이
`done`이나 `skipped`가 된 뒤 `wt batch clean`으로 `.local/tasks`의 TaskDocument만
지운다. Stack task에는 아직 cleanup 명령이 없으므로 landing이 끝났고 다른 batch나
stack이 참조하지 않는 것을 확인한 뒤 필요할 때만 `.local/tasks/<task>.toml`을
수동 삭제한다. 나중에 `wt land`, `wt task clean`, `wt run clean` 같은 명령을 만들더라도
`done`이나 `complete`에 merge나 task definition 삭제 의미를 섞지 않는다.

`wt review`는 상태 전이 명령이 아니다. branch, worktree, TaskRun을 읽어서 parent,
dirty 상태, commit/diff 정보, agent 완료 보고 기대치를 보여주는 점검 명령이다. cmux
workspace/surface 정보도 저장된 실행 상태가 아니라 현재 세션에서 발견한 transport
좌표로만 보여준다. 실제 완료 기록은 `wt done` 또는 `wt stack complete`처럼 source별
completion 명령이 맡는다.

`wt send`도 상태 전이 명령이 아니다. `wt review`와 같은 target 해석으로 현재 cmux
surface를 찾아 메시지를 보내는 transport 명령이다. 메시지를 보냈다는 사실을 TaskRun
상태로 저장하지 않고, 완료 여부는 여전히 TaskRun status와 stack completion 명령으로만
표현한다.

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
