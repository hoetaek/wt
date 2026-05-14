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

`stack`은 어떤 작업 item들을 어떤 순서의 branch parent 체인으로 쌓을지에 대한
개념이다.

`wt stack new`와 `wt stack issue`는 둘 다 stack 상태 파일을 만든다. 차이는 입력
소스다. `new`는 branch-name text에서 직접 작성 item을 만들고, `issue`는 provider
issue snapshot에서 issue item을 만든다.

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

예를 들어 한 옵션은 “하나의 profile 선택”을 뜻하고 다른 옵션은 “모든 profile 실행”을
뜻한다면, 둘을 동시에 받은 상태에서 임의로 우선순위를 정하지 않는다.

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

Prompt도 같은 원칙을 따른다. `common`은 별도 실행 mode가 아니라 기존
`[agent.prompt]` / `[agent.prompt.append]` 모델 안의 공통 scope다. Config layer와
profile convention file merge를 모두 끝낸 뒤 최종 effective config에서 한 번만
`issue`, `new`, `pr` prompt 앞에 펼친다. `common`을 각 layer마다 mode별 prompt로
복사하지 않는다.

### State Is Explicit

저장되는 상태는 사용자가 이해할 수 있는 상태여야 한다.

Batch가 어떤 item을 준비했고, 어떤 item이 끝났고, 어떤 item이 실패했는지는 저장할
가치가 있다. Batch의 canonical 상태 목록도 `[[items]]`이고, issue snapshot에서 만든
item은 `kind = "issue"`로 저장한다. 과거 `[[issues]]` batch 상태는 compatibility를
위해 읽을 수 있지만 새로 쓰는 상태는 `[[items]]`로 수렴한다. 반대로 내부 구현 편의를
위해 만든 가짜 이름이나 암묵적 상태를 저장하면 나중에 사용자가 파일을 읽을 때 모델을
다시 배워야 한다.

Stack이 어떤 item을 어떤 parent 위에 쌓았는지도 저장할 가치가 있다. canonical 상태
목록은 `[[items]]`이고, item source는 issue, 직접 작성한 branch work 등으로 나뉠 수
있다. 과거 `[[issues]]` 상태는 compatibility를 위해 읽을 수 있지만 새로 쓰는 상태는
`[[items]]`로 수렴한다. parent가 아직 실행 전이라 확정되지 않았다면 가짜 값을 넣지
않고, 실행 시 확정된 branch를 기록한다.
Stack에서 `running`은 agent prompt 전송이 아니라 사용자나 agent의 명시적
`complete` 신호를 기다리는 상태다. 완료를 추정해서 다음 item을 시작하지 않는다.
`complete`는 branch가 clean이고 parent보다 앞선 commit이 있을 때만 `done`으로
전이해야 한다. 다음 item 자동 시작은 명시적인 continuation 선택, 예를 들어
`--run-next`로만 일어난다.

상태 파일은 내부 캐시가 아니라 사용자가 읽어도 이해되는 기록이어야 한다.

### Agent-Neutral Names Stay Agent-Neutral

여러 agent에 공통으로 적용되는 기능에는 특정 agent 이름을 붙이지 않는다.

특정 agent에만 적용되는 기능에는 그 agent 이름을 붙여도 된다. 하지만 Codex와 Claude
모두에 영향을 주는 기능이 `claude_*` 이름을 가지면 이름이 거짓말을 하게 된다.

이름은 구현의 과거가 아니라 현재의 의미를 설명해야 한다.

### Compatibility Is Secondary to Clarity

호환성은 중요하지만, 일관성을 깨뜨리는 alias를 기본값으로 만들지 않는다.

호환 alias가 필요하다면 다음 조건을 만족해야 한다.

- 새 사용자가 봐도 개념이 흐려지지 않는다.
- 도움말과 문서에서 어느 이름이 canonical인지 분명하다.
- 제거 계획이나 유지 이유가 설명되어 있다.

그 조건을 만족하지 못하면 실패시키는 편이 낫다.

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
