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
| 3 | 개인 작업 | `<git-common-dir>/wt/` | commit 불가, 머신 로컬 |
| 4 | worktree context | `<git-common-dir>/wt/worktrees/<id>/` | Tier 3 안의 하위 state |

Tier 2는 팀이 이미 사용하는 시스템과 어떻게 통합하는지의 설정이다. 팀이 공유하는 작업
데이터가 아니다. TaskDocument, Workflow, TaskRun, 메시지, activity, worktree registry는
개인 작업 state이므로 Tier 3에 둔다.

Config precedence는 다음 순서다.

```text
global < team integration < personal < command-line flags
```

`wt config`는 effective behavior를 판단하는 source of truth이며, 가능한 한 어떤 scope에서
어떤 값이 왔는지 보여줘야 한다.

### Personal Storage

Personal storage의 canonical root는 `<git-common-dir>/wt/`다. `wt`는
`git rev-parse --git-common-dir`로 이 위치를 찾고, 모든 worktree가 같은 personal storage를
공유하게 한다.

Canonical personal storage layout:

```text
<git-common-dir>/wt/
├── config.toml
├── profiles/
├── tasks/
├── workflows/
├── task-runs/
├── messages/
├── agent.state/
│   └── wait-observations.jsonl
└── worktrees/<id>/
    ├── info.toml
    ├── config.toml
    ├── current.toml
    ├── status.toml
    ├── activity.jsonl
    └── notes/
```

Repo-root `.local` state is not canonical. 새 코드와 새 문서는 위 layout의
`<git-common-dir>/wt/...` 경로를 primary state로 읽고 쓴다. 이전 repo-root local state를
다뤄야 하면 명시적 import/repair 명령으로 다루고, silent fallback이나 alias처럼 동작시키지
않는다.

### Worktree Identity

Worktree identity는 안정적 id와 human label을 분리한다.

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
Participant ID는 충돌이 없을 때 `agents/<display_name>`을 쓸 수 있지만, 충돌하거나 안정성이
필요하면 opaque id를 기준으로 삼는다.

`kind = "orchestration" | "worker"`는 기본 동작 hint다. 권한 모델이나 강제 규칙이 아니다.

### Communication Channels

Activity, Inbox, Status는 다른 개념이다.

| Channel | Writer | Reader | Data | Location |
| --- | --- | --- | --- | --- |
| Activity log | hook 자동 | UI와 debug | append-only JSONL | `worktrees/<id>/activity.jsonl` |
| Inbox | 의도된 sender 또는 delivery owner | target agent와 scope owner | scoped TOML file queue | `messages/agents/<agent>/inbox/<state>/` |
| Status | hook 자동 | 누구나 | single TOML snapshot | `worktrees/<id>/status.toml` |

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
<git-common-dir>/wt/messages/agents/<agent>/inbox/new/<message-id>.toml
<git-common-dir>/wt/messages/agents/<agent>/inbox/claimed/<message-id>.toml
<git-common-dir>/wt/messages/agents/<agent>/inbox/delivered/<message-id>.toml
<git-common-dir>/wt/messages/agents/<agent>/inbox/retry/<message-id>.toml
<git-common-dir>/wt/messages/agents/<agent>/inbox/failed/<message-id>.toml
```

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
wt msg send --scope workflow:2026-05-20-001 --to coordinator "Agent Completion Report: ..."
```

`wt msg send` writes `meta.from = "agents/user"` unless `WT_AGENT_ID` is set to `agents/<agent>` or
`<agent>`. Without `--scope`, it writes direct-scope messages to `inbox/new`; direct
`wt msg send --to coordinator ...` is therefore a direct/default coordinator message and must not be
treated as workflow-owned delivery. Explicit scoped sends accept `direct`, `repo`,
`workflow:<id>`, and `task_run:<id>`.

Canonical hook compatibility delivery:

```bash
wt msg check-inbox --agent agents/codex
```

`check-inbox` exits successfully with no output when there are no deliverable direct-scope
messages. Before rendering hook output it reclaims expired leases according to the delivery
lifecycle policy, claims deliverable direct-scope messages from `inbox/new` or eligible
`inbox/retry`, prints JSON containing `hookSpecificOutput.additionalContext`, and acknowledges the
claims into `inbox/delivered/` only after stdout is written successfully. Active non-expired claims
remain owned by their current claimant. This command is a compatibility consumer for agent hooks,
not a separate unread/read lifecycle.

Workflow supervisors must scope-match before claiming. In particular, `agents/coordinator` is a
shared local coordinator address; a workflow supervisor must not claim shared `agents/coordinator`
messages unless the message has explicit `scope.kind = "workflow"` with the matching Workflow id, or
a future documented policy proves matching TaskRun ownership. Raw recipient address
`agents/coordinator`, `WT_COORDINATOR_AGENT_ID`, `coordinator` alias normalization, or
`correlates_with` is insufficient ownership evidence.

`PostToolUse` polling, push delivery, `wt://` artifact semantics, provider-private Claude/Codex
runtime integration, and detached supervisor commands are outside this contract slice. Future
delivery implementations must build on the same address/scope/delivery lifecycle instead of adding
a second hidden inbox model.

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

- `<git-common-dir>/wt/...`
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

### Unified Agent Hook Setup

One-shot hook setup의 canonical surface는 다음 두 명령이다.

```bash
wt install
wt uninstall
```

`wt install`은 PATH에서 지원되는 agent CLI를 감지한 뒤 해당 adapter의 dispatcher hook을
설치한다. 현재 지원 대상은 `claude`와 `codex`다. 이 명령은 hook capability setup만 수행하며,
특정 `WT_AGENT_ID`를 영구로 hook에 묶지 않는다. Agent identity는 `wt codex`, `wt claude`,
`wt as`, 또는 `wt run ...`이 agent process를 시작할 때 주입한다.

`wt uninstall`은 감지 여부와 무관하게 wt-managed Claude/Codex hook을 제거한다. 사용자가 작성한
hook, cmux hook, unrelated trust state는 보존한다. Provider나 agent runtime이 없어도 uninstall은
가능한 정리만 수행해야 한다.

고급/테스트용 adapter별 escape hatch는 계속 유지한다.

```bash
wt agent hook install claude
wt agent hook install codex
wt agent hook uninstall claude
wt agent hook uninstall codex
```

### Claude Hook Adapter

Claude Code inbox polling의 canonical install surface는 다음 두 명령이다.

```bash
wt agent hook install claude
wt agent hook uninstall claude
```

이 adapter는 Claude 전용이다. Install은 worktree-local
`.claude/settings.local.json`에만 `UserPromptSubmit` hook dispatcher를 추가한다. Hook
command는 runtime env `WT_AGENT_ID`를 읽어 다음 delivery command를 실행하고,
`WT_AGENT_ID`가 없으면 성공으로 조용히 종료한다.

```bash
wt msg check-inbox --agent "$WT_AGENT_ID"
```

Generated command string은 wt-managed entry를 구분하기 위한 marker를 `#` 뒤에 둔다.
Claude Code가 shell command로 실행할 때 `#` 뒤 marker는 shell comment로 처리되므로
`wt msg check-inbox`의 인자로 전달되지 않는다. 이 동작은 CLI integration test로
검증한다.

Install은 먼저 per-worktree Git exclude file에 local settings path를 추가한다.
`.claude`가 `.agents` 같은 repo-local directory로 향하는 symlink라면 symlink path와
실제 target path를 모두 exclude한다. Reinstall은 wt-managed dispatcher hook을 하나만
남기는 idempotent operation이다. `--agent <agent>`가 남아 있다면 manual/test override다.
이 override는 hook을 특정 agent에 묶으므로 normal run UX의 일부가 아니다. Uninstall은
wt-managed Claude hook entry만 제거하고, 사용자가 작성한 다른 Claude hook이나 settings key는
보존한다. `--agent <agent>` uninstall은 manual/test override entry만 대상으로 한다.

Tracked `.claude/settings.local.json` 또는 symlink target local settings file이 있으면
install/uninstall은 실패한다. Tracked `CLAUDE.md`, `AGENTS.md`, `.gitignore`,
`.claude/settings.json` 같은 shared source/config는 adapter가 수정하지 않는다.

### Codex Hook Adapter

Codex inbox polling의 canonical install surface는 다음 두 명령이다.

```bash
wt agent hook install codex
wt agent hook uninstall codex
```

이 adapter는 Codex 전용이다. 현재 Codex는 project-local `.codex/hooks.json` discovery를
신뢰할 수 없으므로 user-level `$CODEX_HOME/hooks.json` 또는 `~/.codex/hooks.json`에만
`UserPromptSubmit` hook dispatcher를 추가한다. User-level hook은 특정 agent id에 영구로
묶이면 안 된다. 기본 hook command는 runtime env `WT_AGENT_ID`를 읽어 다음 delivery command를
실행하고, `WT_AGENT_ID`가 없으면 성공으로 조용히 종료한다.

```bash
wt msg check-inbox --agent "$WT_AGENT_ID"
```

Codex는 user-level custom hook을 실행하기 전에 matching trust state를 요구한다. Install은
Codex가 쓰는 hook identity와 같은 방식으로 `trusted_hash`를 계산해
`$CODEX_HOME/config.toml` 또는 `~/.codex/config.toml`의 `[hooks.state]` 아래에 쓴다. Hash
identity는 event key `user_prompt_submit`, normalized command handler, default timeout
`600`, `async = false`를 canonical JSON으로 정렬한 뒤 SHA-256으로 계산한다. Install은
`[features].hooks = true`도 보장한다.

`--agent <agent>`가 남아 있다면 manual/test override다. 이 override는 user-level hook을
특정 agent에 묶으므로 normal run UX의 일부가 아니다. `wt run issue`, `wt run task`,
`wt run workflow`는 사용자가 매번 hook을 다시 설치하게 하지 않고, Codex launch 시 cmux
`new-workspace --command`에 `WT_AGENT_ID=agents/<branch_slug>`와
`WT_COORDINATOR_AGENT_ID=agents/coordinator`를 주입해 dispatcher에 agent binding과
coordinator target을 제공해야 한다. `<branch_slug>`는 scoped message address의 `agents/<agent>` 한 segment
제약과 맞도록 path separator가 없는 값이어야 하고, `wt msg send --to <branch_slug>`와
`wt msg check-inbox --agent "$WT_AGENT_ID"`가 같은 inbox를 보아야 한다.
Claude와 future agent CLI도 wt가 process launch를 소유하는 경로에서는 같은
launch-time `WT_AGENT_ID` shape를 받아야 한다.

### Agent Runtime Wrapper

Hook adapter install은 capability setup이고, agent identity는 run/session launch의 책임이다.
Normal daily launch의 canonical surface는 agent CLI 이름을 그대로 쓰는 짧은 wrapper다.

```bash
wt codex
wt claude
```

이 두 명령은 현재 git branch에서 `<branch_slug>`를 계산해 `WT_AGENT_ID=agents/<branch_slug>`와
`WT_COORDINATOR_AGENT_ID=agents/coordinator`를 agent process에 주입한다. 사용자가 직접
`codex` 또는 `claude`를 실행하면 wt는 환경변수를 주입하지 않으며, hook dispatcher는
`WT_AGENT_ID`가 없을 때 조용히 no-op 한다.

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

`wt as`는 explicit `WT_AGENT_ID`를 그대로 쓰고, `WT_COORDINATOR_AGENT_ID=agents/coordinator`를
함께 주입한다. 긴 `wt agent shell --agent <agent> ...` 형태를 최종 UX로 문서화하지 않는다.

### Cross-Agent Hook Smoke

Canonical non-LLM smoke는 실제 Claude/Codex 세션을 CI에서 띄우지 않고, 설치된 hook dispatcher와
file inbox만 검증한다.

1. 같은 git common dir을 공유하는 linked worktree 두 개를 만든다.
2. `wt install`로 Claude worktree-local dispatcher와 Codex user-level dispatcher를 설치한다.
3. `wt as agents/claude-smoke -- wt msg send --to agents/codex-smoke CLAUDE_SENT`로 Claude identity에서 Codex inbox로 보낸다.
4. 설치된 Codex hook command를 `WT_AGENT_ID=agents/codex-smoke`로 실행해 `CLAUDE_SENT`를 delivery한다.
5. `wt as agents/codex-smoke -- wt msg send --to agents/claude-smoke CODEX_SENT REALWT_PONG_SEEN`로 답장한다.
6. 설치된 Claude hook command를 `WT_AGENT_ID=agents/claude-smoke`로 실행해 `CODEX_SENT REALWT_PONG_SEEN`를 delivery한다.
7. `wt uninstall`로 wt-managed hook state를 정리한다.

이 smoke는 cmux workspace/surface를 만들거나 읽지 않는다. Real Claude/Codex manual smoke는 같은
message path를 실제 agent lifecycle에서 관찰하는 추가 검증이지, message transport의 canonical
요구사항은 아니다.

Reinstall은 wt-managed Codex dispatcher hook을 하나만 남기는 idempotent operation이다.
Uninstall은 wt-managed Codex `UserPromptSubmit` dispatcher entry와 matching trust state만
제거한다. `--agent <agent>` uninstall은 manual/test override entry만 대상으로 한다. cmux가
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

`init`은 이 repo가 어떤 starter shape로 시작할지에 대한 개념이다. Starter shape는
worktree-only 최소 설정인지, agent를 붙일지, issue workflow를 붙일지, app repo용
setup/site/test/workspace 기본값까지 잡을지를 고르는 선택이다.

`profile`은 어떻게 실행할지에 대한 개념이다. `init`이 starter shape 안에서
`[profile.agent]`를 쓸 수는 있지만, agent runtime을 구조화하고 재사용하는 책임은 계속
`profile`에 둔다.

`workflow`는 `<git-common-dir>/wt/workflows` 아래에 저장되는 prepared execution plan이다.
사용자는 어떤 task들을 어떤 실행 shape로 시작하고 이어갈지를 workflow로 다룬다.
Canonical command surface는 `wt workflow`다.

Workflow는 정확히 하나의 `mode = "single" | "batch" | "stack" | "matrix"` 값을 가진다.
`mode`는 실행 shape만 고른다. `single`은 하나의 branch workspace에서 하나 이상의
TaskDocument를 실행하고, `batch`는 같은 base에서 여러 독립 branch를 실행하며,
`stack`은 task branch들을 정해진 parent chain으로 순서대로 실행하고, `matrix`는 하나의
TaskDocument를 named profile 목록으로 확장한다.

`batch`, `stack`, `matrix`는 workflow mode 값으로 남지만 top-level 상태 파일 noun이나 command
namespace가 아니다. 새 상태 파일은 `<git-common-dir>/wt/workflows` 아래에만 만들고, batch/stack
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
`<git-common-dir>/wt/config.toml`, profile file은 사용자 intent와 override를 저장하고, `wt config`는
merge된 layer, convention file, built-in default를 사용자가 복사해 수정할 수 있는 형태로
보여준다. 명령 구현은 user-facing default를 각 call site에서 새로 해석하지 말고 config
모델의 effective accessor나 effective policy snapshot을 거쳐 적용해야 한다.
활성 section의 runtime default는 `wt config` 출력에 materialize한다. 예를 들어 active
`[site]` provider는 name/root/secure/url과 Traefik target default까지 보여주고,
`[workspace.browser]`는 setup/open 때 browser를 띄울지와 어떤 URL을 열지 결정한다.
`[site]`는 `site_url`을 만들고, browser launch policy를 소유하지 않는다. active `[editor]`
설정은 생략된 placement의 `cmux_surface` default를 보여준다. 반대로
`provider = "none"`처럼 feature가 inactive인 section은 effective output에 unrelated section으로
내보내지 않는다.

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
`wt init`은 이 기본값을 active config로 쓰지 않고 commented override 예시로만 보여준다.
Active `colors = { ... }`는 사용자가 기본값과 다른 색을 고정하려는 의도일 때만 둔다. 색을
아예 쓰지 않을 kind는 `task = ""`처럼 빈 문자열로 override한다.

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
observation은 `wt agent status` / `wt agent watch`, workflow file lifecycle과 repair /
completion은 `wt workflow`가 맡는다. `wt run` 아래에 cleanup, inspect, repair, complete,
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
문서와 help text에서 같은 말로 설명해야 한다. Selector는 작은 terminal prompt로
동작하고, filterable list와 최대 10개 visible row 안에서 task, branch, PR, workflow,
config section 같은 현재 concept label만 보여준다. 색상, symbol, checkbox는 보조
표현일 뿐이고 의미는 text label에 남아야 한다. 보조 metadata가 있는 row는 같은 prompt
page 안에서 hint column을 맞춰 보여주되, filter 대상은 padding이 아니라 concept label과
metadata text여야 한다. Metadata가 없는 plain label selector에는 가짜 column을 만들지
않는다.

### Progressive Disclosure
*North star: [Persona](north-star.md#persona).*

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
`<git-common-dir>/wt/config.toml` 중 하나를 선택하고, 답한 설정은 그 파일에만 쓴다. 다른 config 파일,
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
`issue`, `branch`, `pr` prompt 앞에 펼친다. `common`을 각 layer마다 mode별 prompt로
복사하지 않는다.

`workflow`도 Workflow `mode = "single" | "batch" | "stack" | "matrix"`나 setup mode가 아니라
`[agent.prompt]` / `[agent.prompt.append]` 안의 workflow-started task 전용 scope다.
`wt run workflow`로 시작한 task에만 적용하고, direct `wt run task`, `wt run issue`,
`wt run branch`, `wt run pr`에는 적용하지 않는다. Workflow task의 setup mode는 계속
TaskDocument origin에 따라 `issue` 또는 `branch`를 사용하므로 기존 setup-mode prompt도
함께 적용된다. `common`은 `workflow`로 펼치지 않는다. Workflow task는 이미 `issue` 또는
`branch` prompt를 받기 때문에 `common`을 `workflow`에도 펼치면 같은 공통 지시가 중복된다.
Profile convention file은 `<git-common-dir>/wt/profiles/<name>/prompts/workflow.md`와
`<git-common-dir>/wt/profiles/<name>/prompts/workflow.append.md`를 같은 scope로 읽는다.

### State Is Explicit
*North star: [Scope Model Direction](north-star.md#scope-model-direction).*

저장되는 상태는 사용자가 이해할 수 있는 상태여야 한다.

TaskDocument는 작업이 무엇인지를 담는 정의다. `<git-common-dir>/wt/tasks/<task>.toml`
아래에 title, branch, body, origin처럼 실행과 무관하게 읽을 수 있는 정보를 둔다.

`wt task list`는 `<git-common-dir>/wt/tasks/<task>.toml`에 저장된 TaskDocument file의 canonical
read-only inventory다. `wt run task`의 runnable selector가 아니므로 이미 완료된
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
`<git-common-dir>/wt/tasks/<safe-issue-id>.toml`에 title, branch, body, `[origin]`을 기록한다.
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
`<git-common-dir>/wt/tasks/<safe-issue-id>.toml`이 이미 있으면 local edits를 보존하기 위해 실패하고,
조용히 덮어쓰거나 merge하지 않는다. Replace/update가 필요하다면 별도의 명시 옵션과
help/test/documentation이 먼저 필요하다.

TaskDocument publish는 local task 정의를 configured issue provider의 issue로 만드는
side effect다. Canonical command shape는 `wt task publish` 또는
`wt task publish <task>...`다. Bare `wt task publish`는 아직 `[origin]`이 없는 local
TaskDocument를 multi-select로 고르게 하고, 명시 task key는 scriptable path로 남긴다.
`publish`는 각 task의 provider issue 생성과 `<git-common-dir>/wt/tasks/<task>.toml`의 `origin`
업데이트가 모두 끝났을 때만 해당 task를 성공으로 보고한다. 둘 중 하나만 끝난 상태를
성공으로 보고하지 않는다. `origin`은 external issue와의 durable link이지, 아직
publish해야 한다는 pending request가 아니다.

`wt run issue`는 이미 존재하는 provider issue에서 worktree를 시작하는 명령으로 남긴다.
Provider issue를 TaskDocument로 가져오는 흐름은 `wt task import`, Local TaskDocument를
provider issue로 만드는 흐름은 `wt task publish`다. `wt run issue import`, `wt run issue create`,
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

TaskRun은 그 작업을 한 번 실행한 인스턴스다. `<git-common-dir>/wt/task-runs/<id>.toml` 아래에
task, branch, status, group, error, creation_order, created_at, updated_at을 저장한다.
`group`은 Workflow file stem과 맞는 workflow-linked run을 식별하는 link이고, 직접
`wt run task`로 만든 TaskRun은 group을 저장하지 않는다. Legacy TaskRun TOML의
source 값 `new`, `batch`, `stack`은 읽기 전용 migration compatibility로만 받으며 새
TaskRun 출력에는 쓰지 않는다. `creation_order`는 같은 task의 최신 실행을 고를 때 파일명이나
초 단위 timestamp 우연성에 기대지 않도록 새 TaskRun마다 증가하는 실행 생성 순서다.
`creation_order`가 없는 previous TaskRun은 계속 읽되 ordered TaskRun보다 앞에 정렬하고,
previous끼리는 `created_at`과 id를 fallback으로 쓴다.
status는 `prepared`, `running`, `done`, `failed`, `skipped`만 canonical이다. 알 수 없는
status나 workflow mode 값은 조용히 해석하지 않고 파싱 단계에서 실패시킨다.

통합 실행 상태 모델은 TaskDocument, Workflow, TaskRun의 책임을 나누는 데서 시작한다.
TaskDocument는 무엇을 할지에 대한 재사용 가능한 slice-level 설명이고, Workflow는
workflow-level `title`, `body`, optional `[origin]`과 그 task set을 어떤 실행 shape로
이어갈지에 대한 저장된 계획이며, TaskRun은 TaskDocument 하나를 한 번 실행한 기록이다.

Workflow 준비는 `<git-common-dir>/wt/workflows/<id>.toml` 하나와 각 task의 TaskDocument/TaskRun link를
만든다. Workflow의 canonical task 목록은 `[[tasks]]`이고, 각 row는 task key, linked
TaskRun id, stack-mode parent처럼 orchestration에 필요한 link와 실행 지시만 저장한다.
Workflow row는 status/error를 따로 가지지 않고, branch 이름도 복사하지 않는다. 실행
인스턴스의 canonical 기록은 TaskRun이고, branch name의 canonical 기록은 TaskDocument다.
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
from `.wt.toml` plus `<git-common-dir>/wt/config.toml`, applies any explicit command-line override, and
writes the resulting policy snapshot into `<git-common-dir>/wt/workflows/<id>.toml`. Later edits to
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

This model changes both `.wt.toml` config shape and `<git-common-dir>/wt/workflows` state shape, so
implementing parser/runtime behavior is a pre-1.0 minor user-facing change. Replacing
workflow `objective` with workflow-level `title`, `body`, and optional `[origin]` also
changes the `<git-common-dir>/wt/workflows` state shape and `wt workflow task` / `wt workflow issue`
preparation surface, so it belongs in the same pre-1.0 minor model-change category.
Ordinary development commits still do not bump `Cargo.toml`; the release branch owns the
eventual version bump.

Workflow-level `title`/`body`/`[origin]` are saved to `<git-common-dir>/wt/workflows/<id>.toml` as
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

`<git-common-dir>/wt/workflows`는 `<git-common-dir>/wt/batches`와 `<git-common-dir>/wt/stacks`를 대체한다. 이유는 batch와 stack이
저장소 noun이 아니라 하나의 Workflow 안에서 고르는 execution mode이기 때문이다. 새
기능이 `<git-common-dir>/wt/batches`나 `<git-common-dir>/wt/stacks`에 상태를 계속 추가하면 사용자는 같은 준비 작업을
workflow, batch file, stack file 중 무엇으로 읽어야 하는지 다시 배워야 한다. 새 canonical
state는 Workflow file 하나로 수렴시킨다.

`single` mode workflow는 하나의 branch workspace에서 하나 이상의 TaskDocument를 실행한다.
`batch` mode workflow는 같은 base에서 여러 TaskDocument를 독립 branch로 실행한다. Batch
task들은 독립적이므로 이미 `running`인 TaskRun이 있어도 prepared/failed sibling이 있으면
workflow는 runnable로 남을 수 있다. `stack` mode workflow는 TaskDocument를 base-to-top
parent chain으로 실행하고, current `running` TaskRun이 있으면 다음 task를 시작하지 않는다.
Stack-mode에서 `running`은 agent prompt 전송이 아니라 사용자나 agent의 명시적 completion
신호를 기다리는 상태다. 완료를 추정해서 다음 task를 시작하지 않는다.

`wt run workflow`에서 workflow target 생략은 runnable workflow를 고르는 기본 동작이다.
`single`은 linked TaskRun 전체가 `prepared` 또는 `failed`일 때만 runnable이고, `batch`는
하나 이상의 linked TaskRun이 `prepared` 또는 `failed`이면 runnable이며, `stack`은 다음
`prepared` 또는 `failed` task가 있고 현재 `running` task가 없을 때 runnable이다. 명시
workflow id/path는 automation surface로 남긴다.
`wt run workflow`는 saved Workflow execution start만 뜻한다. Workflow list/show/edit/repair,
complete, task/issue preparation은 계속 `wt workflow` namespace에 남고 `run`이 소유하지
않는다.

`wt workflow list`는 `<git-common-dir>/wt/workflows/<id>.toml`에 저장된 Workflow file의 canonical
read-only inventory다. `wt run workflow`의 runnable selector가 아니므로 runnable workflow만
필터링하거나 selector의 10-row visible cap을 적용하지 않는다. `wt workflow show`의 latest
default도 all-workflow inventory로 확장하지 않는다. Output은 Workflow 자체의 단일
`status`를 만들지 않고, linked TaskRun에서 파생한 task-run status count/summary와 mode별
runnable metadata를 보여준다. Human text output은 `runnable`, `waiting`, `done` 같은
파생 presentation group 아래에 workflow title, workflow id/mode, TaskRun summary,
profile/policy preview를 list row로 두고 body summary, origin, human reason, base, path는
secondary detail line에 둔다. JSON
output은 top-level `title`, `body`, optional `origin` metadata와 raw runnable reason
identifiers를 machine-readable metadata로 보존한다.
Workflow TOML parse/validation failure는 조용히 숨기지 않고
text warning 또는 JSON `invalid_workflows`로 보고한다. Batch/stack은 계속 Workflow `mode`
값일 뿐이므로 `wt list workflow`, top-level `batch`/`stack` 같은 symmetry
command를 추가하지 않는다. `wt task list`는 symmetry command가 아니라 별도 TaskDocument
inventory surface이며 Workflow, TaskRun, branch, worktree 목록 의미를 갖지 않는다.
`wt profile list`는 named profile inventory를 위한 canonical surface이고,
`<git-common-dir>/wt/profiles/<name>/profile.toml`을 config/profile loader로 읽어 정렬된 valid
profile 목록과 함께 invalid profile 레코드를 text warning 또는 JSON
`invalid_profiles`로 보고한다. Bare `wt profile`은 omission default로 `wt profile list`를
호출하며, 두 surface는 모두 `wt profile --help`와 `wt profile list --help`에 명시한다.
`default`는 reserved이므로 valid profile로 표시하지 않는다.

`wt ui [--port <PORT>]`는 `<git-common-dir>/wt`와 wt config state를 읽기 쉽게 보는 read-only local web
UI다. 이 명령은 `127.0.0.1`에만 bind하고, port `0`은 available port 선택을 뜻하며, 시작
후 URL을 출력한다. Browser opening은 별도 명시 옵션이 생기기 전까지 기본 동작이 아니다.
UI 서버는 binary에 embedded된 no-build HTML/CSS/JS asset과 allowlisted route만 제공한다.
첫 API surface는 `GET /api/snapshot`이며 ideas, TaskDocuments, Workflows, TaskRuns,
retrospectives, profile summaries, effective config summary/source paths를 한 snapshot으로
반환한다.

`wt ui`는 inventory lens이지 새로운 state owner가 아니다. TaskDocument는 계속
`<git-common-dir>/wt/tasks`, Workflow는 `<git-common-dir>/wt/workflows`, TaskRun은 `<git-common-dir>/wt/task-runs`, config/profile
layering은 `.wt.toml`, `<git-common-dir>/wt/config.toml`, `<git-common-dir>/wt/profiles`가 source of truth다. UI board
group은 linked TaskRun 상태와 runnable metadata에서 파생한 presentation일 뿐이고,
Workflow나 TaskDocument에 새 status/column 값을 쓰지 않는다. Parse/validation failure는
snapshot과 UI에 invalid record로 드러내며, invalid TOML을 조용히 숨기지 않는다.

Workflow detail의 relationship summary와 secondary canvas view도 같은 파생 presentation이다.
행과 canvas node는 Workflow file의 `[[tasks]]`/`[[tasks.runs]]` 링크, TaskDocument, TaskRun
snapshot을 읽어 `Workflow → TaskDocument → TaskRun → Agent` 관계를 보여주지만 새 TaskDocument,
graph node, canvas position, agent contact, live agent state를 Workflow/TaskDocument/TaskRun에
저장하지 않는다. Agent 칸은 durable/현재 관찰 가능한 정보가 없으면 중립적인 not-observed
상태로 남기고, `TaskRun.status`와 `agent.state`/`agent.status`를 합치지 않는다.

`wt ui` read-only contract는 write API, drag/drop mutation, 별도 DB, frontend build pipeline,
Tauri/Electron, arbitrary repo file serving, `.env` 읽기를 추가하지 않는다. `/api/snapshot`은
state-owner reader와 config/profile loader를 거쳐 요약 DTO만 만들고, CLI text output을
scrape하지 않는다. Editing controls must not be hidden inside `wt ui`; a view that changes
TaskDocuments, Workflows, profiles, or config is a different surface.

Write-capable web editing belongs to a separate `wt studio` surface. `wt studio` is the canonical
place for canvas/form editing such as creating a Workflow, adding or removing TaskDocument nodes,
editing TaskDocument fields, changing Workflow task order/edges, and preparing execution from that
edited plan. Studio writes still use the same source-of-truth files: TaskDocuments in
`<git-common-dir>/wt/tasks`, Workflows in `<git-common-dir>/wt/workflows`, personal config in
`<git-common-dir>/wt/config.toml`, and profiles in `<git-common-dir>/wt/profiles`. TaskRun remains an
execution record and is read-only in Studio except through explicit lifecycle commands that already
own TaskRun mutation.

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
`wt msg send --to coordinator ...` 명령이 먼저 들어간다. `coordinator`는
`agents/coordinator`로 normalize되는 예약된 local target이다. 같은 section은 fallback으로
현재 coordinator cmux workspace/surface 좌표로 렌더링되는 `cmux send`와
`cmux send-key ... enter` 명령도 포함한다.
이것은 Workflow orchestration이나 completion command가 아니다. Task-run agent는
`PR=none`인 `Agent Completion Report`를 coordinator에게 보내고, coordinator가 review,
landing, cleanup을 명시적으로 처리할 때까지 기다린다. 좌표는 현재 transport 정보일 뿐이므로
TaskDocument나 TaskRun에 저장하지 않는다. file inbox route가 unavailable이면 agent는
cmux fallback으로 같은 보고를 보내고, 둘 다 unavailable이면 task session에 남기고
기다린다. Handoff section과 그 안의 inbox/cmux report 명령은 긴 TaskDocument 본문과
분리된 첫 prompt로 먼저 보내서 terminal prompt가 축약되어도 coordinator route가 앞쪽에
남게 한다.

Workflow coordinator handoff는 `stack` 전용 개념이 아니라 `wt run workflow`가 시작하는
모든 task prompt의 계약이다. Prompt에는 `Workflow Coordinator Handoff` section이 포함되고,
기본 보고 route인 `wt msg send --scope workflow:<workflow-id> --to coordinator ...` 명령이
먼저 들어간다. 이 explicit workflow scope가 shared `agents/coordinator` inbox message의
workflow ownership이다. 같은 section은 fallback으로 현재 coordinator cmux workspace/surface
좌표로 렌더링되는 `cmux send`와 `cmux send-key ... enter` 명령도 포함한다. 이 inbox
target과 좌표는 현재 transport 정보일 뿐이므로 Workflow file, TaskRun, TaskDocument에
저장하지 않는다. file inbox route가 unavailable이면 agent는 cmux fallback으로 같은
`Agent Completion Report`를 보내고, 둘 다 unavailable이면 task session에 남기고 기다린다.
Handoff section과 그 안의 inbox/cmux report 명령은 긴 TaskDocument 본문과 분리된 첫
prompt로 먼저 보내서 terminal
prompt가 축약되어도 coordinator route가 앞쪽에 남게 한다.
사용자 정의 `[agent.prompt.workflow]` prompt가 있으면 이 built-in handoff와 TaskDocument
snapshot 뒤, 기존 `issue`/`branch` setup-mode prompt 앞에 보낸다.

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
read-only dossier다. `--pr`을 명시하면 같은 inspect report 안에 Pull Request Review evidence
section을 추가로 가져와서 PR metadata, submitted review/head synchronization, review
threads의 file/line evidence, PR comments/reactions, check rollup, warning을 보여준다. 이 PR
evidence는 read-only inspection surface이며 thread resolve, reply, review request, PR body edit,
merge, TaskRun/Workflow state mutation을 하지 않는다. `--pr`을 생략한 `wt inspect <target>`은
GitHub auth나 network fetch 없이 local dossier만 출력해야 한다. Agent observation snapshot을 같이
보여줄 수 있지만, `inspect`의 exit code는 command 자체의 성공/실패만 뜻한다. 관찰된 agent가
`needs_input`이거나 `failed`여도 그 사실만 출력하고 polling용 exit code로 바꾸지 않는다. PR
review verdict도 human output과 nested JSON evidence에만 두고 exit-code 의미를 바꾸지 않는다.
실제 완료 기록은 direct 또는 workflow-linked context별 명령이 맡는다. 직접 `wt run task`가 만든
TaskRun은 review/landing 확인 뒤 `wt done` cleanup이 정리할 수 있고, Workflow file의
`[[tasks]].run`과 matching `group`으로 연결된 TaskRun은 workflow completion command가 전이한다.

`wt inspect`에서 `<target>` 생략은 interactive TTY human mode에서 inspectable work target
selector를 여는 기본 동작이다. `--json`, `--quiet`, 또는 non-TTY automation에서는 selector를
열지 않고 explicit `<target>`을 요구해야 한다. 실패 메시지는 branch, worktree path/name,
TaskRun id 중 하나를 넘기거나 interactive TTY에서 selector를 열라는 guidance를 정확히
보여줘야 한다.

`wt inspect <target> --pr --json`은 PR evidence를 `pull_request_review` nested field 아래에
둔다. Top-level `status`는 만들지 않는다. Durable execution lifecycle은 `task_runs[].status`,
agent observation은 `agent.state`, PR review result는 `pull_request_review.verdict`처럼 각
concept owner 아래에 남아야 한다.

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

`wt agent watch --record-wait-observations`는 opt-in local telemetry write다. 이 flag는
`--heartbeat`나 `--timeout`처럼 사용자가 명시한 wait bound가 있을 때만 허용하고, bound에
도달한 순간의 still-running observation만 `<git-common-dir>/wt/agent.state/wait-observations.jsonl`
에 append-only JSONL sample로 기록한다. idle, needs_input, failed, no_session observation은
non-idle wait sample을 만들지 않는다. 이 state owner는 `agent.state`이며 runtime observation
자료일 뿐이므로 TaskRun status, Workflow file, TaskDocument, cmux transport 좌표에 쓰지 않는다.
sample은 watch 시작 이후의 안정된 `elapsed_seconds`, 사용자가 지정한 heartbeat/timeout bound인
`bound_seconds`, 마지막 출력 이후 변하지 않은 시간을 나타내는 `unchanged_seconds`를 분리한다.
sample 기록 때문에 TaskRun을 failed, blocked, done, skipped로 전이하지 않고, `wt agent watch`를
delivery loop나 detached supervisor로 넓히지 않는다. 명시 flag가 없으면 wait telemetry는
저장하지 않는다.

`wt agent wait-stats`는 `<git-common-dir>/wt/agent.state/wait-observations.jsonl`을 읽는
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

Agent별 상태 신호 준비도는 `agent status`나 `agent watch`가 고치는 대상이 아니라 관찰의
신뢰도 조건이다. Claude Code는 cmux의 Claude 통합에서 status/sidebar 신호가 나오고, Codex는
사용자가 `cmux hooks codex install --yes`를 명시적으로 실행한 뒤에야 `agent.hook.*`와
`set_status codex Running/Idle` 신호가 나온다. Codex hook이 없으면 agent commands는 화면
텍스트 fallback을 쓸 수 있지만 약한 관찰이라는 warning을 남겨야 한다. `wt doctor`는 cmux
상태 신호 준비도를 보고만 한다.

Codex inbox delivery는 상태 신호와 다른 명시적 adapter surface다.
`wt agent hook install codex`는 사용자가 요청한 경우에만 user-level Codex `hooks.json`과
`config.toml` trust state를 수정한다. 일반 `wt agent status/watch`, `wt doctor`, `wt msg`
같은 명령이 전역 Codex hook을 자동 설치하거나 사용자 agent config를 몰래 수정하면 안 된다.

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
`wt workflow`와 `<git-common-dir>/wt/workflows`로 수렴시키는 변경은 CLI와 상태
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
