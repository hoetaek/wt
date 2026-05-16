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
`.local/tasks` 아래의 준비된 task를 하나 골라 바로 시작하려면 `wt new --task [task]`를
쓴다. Bare `wt new`처럼 생략으로 입력 소스를 바꾸면 branch-name 입력과 task 선택이
같은 빈 입력에 묶여서 사용자가 대상을 다시 추론해야 한다.

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

TaskDocument는 작업이 무엇인지를 담는 정의/스냅샷이다. `.local/tasks/<task>.toml`
아래에 title, branch, body, origin처럼 실행과 무관하게 읽을 수 있는 정보를 둔다.

TaskRun은 그 작업을 한 번 실행한 인스턴스다. `.local/task-runs/<id>.toml` 아래에
task, branch, status, source, group, error, created_at, updated_at을 저장한다. status는
`prepared`, `running`, `done`, `failed`, `skipped`만 canonical이고, source는 `new`,
`batch`, `stack`만 canonical이다. 알 수 없는 status/source 값은 조용히 해석하지 않고
파싱 단계에서 실패시킨다.

Batch가 어떤 task를 준비했고, 어떤 task가 끝났고, 어떤 task가 실패했는지는 저장할
가치가 있다. Batch의 canonical 상태 목록은 `[[tasks]]`이고, 각 row는 `.local/tasks`
아래의 task 문서와 `.local/task-runs` 아래의 TaskRun을 가리킨다. Batch row는 어떤
task들이 함께 시작 대상인지와 어떤 실행 기록을 읽어야 하는지만 저장하고, status/error를
따로 가지지 않는다. 실행 인스턴스의 canonical 기록은 TaskRun이다. `[[issues]]`나
`[[items]]`처럼 같은 상태 목록을 가리키는 다른 이름은 받지 않는다. 내부 구현 편의를 위해
만든 가짜 이름이나 암묵적 상태를 저장하면 나중에 사용자가 파일을 읽을 때 모델을 다시
배워야 한다.

Stack이 어떤 task를 어떤 parent 위에 쌓았는지도 저장할 가치가 있다. canonical 상태
목록은 `[[tasks]]`이고, task 문서는 issue origin이 있는 작업과 직접 작성한 branch work
를 같은 형태로 담는다. `[[issues]]`나 `[[items]]`처럼 같은 상태 목록을 가리키는 다른
이름은 받지 않는다. parent가 아직 실행 전이라 확정되지 않았다면 가짜 값을 넣지 않고,
실행 시 확정된 branch를 기록한다. Stack row도 실행이 시작된 경우 TaskRun을 가리키고,
명시적 `complete` 신호가 들어오면 같은 TaskRun을 `done`으로 전이한다.
`pr`은 기존 pull request workflow를 가리키는 별도 개념이므로 stack task로 받지 않는다.
Stack에서 `running`은 agent prompt 전송이 아니라 사용자나 agent의 명시적
`complete` 신호를 기다리는 상태다. 완료를 추정해서 다음 task를 시작하지 않는다.
`complete`는 branch가 clean이고 parent보다 앞선 commit이 있을 때만 `done`으로
전이해야 한다. 다음 task 자동 시작은 명시적인 continuation 선택, 예를 들어
`--run-next`로만 일어난다.

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
