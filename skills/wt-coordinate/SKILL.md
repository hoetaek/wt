---
name: wt-coordinate
description: "Use when coordinating running wt work: inspect status, send feedback, review diffs/checks, pass workflow tasks when needed, and hand accepted work to wt-land."
---

# WT Coordinate

Coordinate running wt work. Object model, status semantics, and pass vs
cleanup boundaries: see `../wt-work/references/task-lifecycle.md`.

Stay on these responsibilities; do not absorb later phases:

| Phase | Owner |
|---|---|
| inspect / feedback / spec sync | this skill |
| `wt workflow pass` for workflow-linked runs | this skill |
| land / merge | `wt-land` |
| cleanup with `wt done` | `wt-land` |

## Start With Tab Name

Before inspection, if running inside cmux, first rename the current tab. This is
the first coordination action, so the user can identify what this coordinator is
doing before any longer inspection or watch command starts.

Use Korean for the visible tab name. Prefer a short user-facing task phrase over
raw branch, workflow, or TaskRun slugs. Naming rules:

- Format: `조율: <한글 작업명>`.
- Keep the name scannable, usually 18 characters or fewer after `조율:`.
- Use nouns or noun phrases, not status prose. Good: `조율: 리뷰 라우팅`,
  `조율: 스택 검토`, `조율: 릴리즈 준비`.
- Include an id or slug only when it is needed to disambiguate similar runs, and
  put it at the end: `조율: 리뷰 라우팅 run-42`.

```bash
cmux rename-tab --surface "$(cmux identify | jq -r .caller.surface_ref)" "조율: <한글 작업명>"
```

Use `caller`, not `focused`; `focused` is the user's current tab, not yours. If
the command shows that this shell is not inside cmux, continue with inspection.

## Inspect

```bash
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
```

Capture TaskRun id/status/context/workflow/branch/parent, worktree path and
dirty state, commits and diff against parent, coordinator id when recorded,
default worker inbox identity when it can be derived, cmux workspace/surface,
and runtime status. Scripts: `wt --json agent status <target>`.

If status is `running`, let the agent work unless clearly stuck. Separate
launch validation from steady monitoring. A short post-launch poll is fine to
confirm that the run is observable:

```bash
wt agent watch <target> --interval 5 --timeout 45 --heartbeat 45
```

Do not treat the absence of commits or reports during that short launch window
as stuck evidence. Before choosing the steady watch cadence, read the
TaskDocument `계획 (Planning)` expected duration, `09-execution.md` risks, and
recent timing retrospectives when available:

```bash
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/planning/specs" "$repo_root/.wt/planning/retrospectives" -type f \( -name '11-retrospect.md' -o -name 'timing.md' \) 2>/dev/null
wt agent wait-stats
```

`wt agent wait-stats` summarizes runtime agent wait observations under
`<repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl`
and can inform cadence, but it is not an adaptive default engine and does not
replace the task estimate or coordinator judgment.

Use a structure like this for steady monitoring:

| Expected duration | Default steady watch |
|---|---|
| <= 20m or post-feedback | `--interval 10 --heartbeat 120-300` |
| 20-60m | `--interval 20 --heartbeat 300-600` |
| 1-3h | `--interval 30 --heartbeat 600-900` |
| > 3h | `--interval 60 --heartbeat 900-1800` |

For example, a 2h conservative planning guess usually deserves a 10-15 minute
heartbeat after launch validation, not repeated sub-minute probing:

```bash
wt agent watch <target> --interval 30 --heartbeat 900
```

Use a shorter cadence only when debugging a suspected stall, waiting after
focused feedback, the expected duration is short, or an immediate transition is
expected. If status is `needs_input`, send feedback. If status is `idle`, review
the worktree instead of polling.

Record the chosen watch strategy for `wt-retrospect`: expected duration and
basis, launch-validation command, steady heartbeat/interval/timeout, first
meaningful signal time, state transitions, reports, and any nudges sent because
the cadence looked wrong.

## Message Route

Use file inbox messages as the default coordination route. `wt send` is a live
cmux prompt transport, not the canonical message record.

Inbox delivery depends on a resolvable recipient identity. Do not treat
`wt setup` as an identity fix: setup installs per-machine hooks and shell
integration, but it does not retroactively bind an already-running agent
session. Hook delivery through `wt msg check-inbox` resolves only
`WT_AGENT_ID`, then the current live identity anchor, then no-ops; it does not
create an identity anchor or fall back to cwd/TaskRun inference.

Before relying on inbox reports, verify coordinator identity and the TaskRun
route:

```bash
wt session whoami
eval "$(wt session set <coordinator-agent-id>)"   # when starting a coordinator shell
echo "${WT_AGENT_ID:-}"
wt inspect <target>
```

`wt session set <id>` prints shell exports; use
`eval "$(wt session set <id>)"` so the current coordinator shell receives
`WT_AGENT_ID` and writes an identity anchor for later resolution. For an
already-open agent session, write the identity anchor only when you know the exact
intended agent id from `wt inspect`; otherwise relaunch through `wt codex`,
`wt claude`, or `wt as <agent-id> -- <command...>` so launch-time
`WT_AGENT_ID` is unambiguous. A launched TaskRun should record `agent_id` and
`coordinator_id`; `wt task report` and `wt task review` use those TaskRun-owned
routes instead of a dynamic `coordinator` alias or ambient coordinator env
fallback. `wt task report` remains valid for `running` and `passed` TaskRuns;
without `WT_TASK_RUN_ID`, it uses branch fallback only when exactly one running
or passed TaskRun matches.

Monitor coordinator inbox reports with:

```bash
wt msg watch --timeout 300
wt msg list --agent <coordinator-agent-id>
wt msg read --agent <coordinator-agent-id> <message-id>
```

For coordinator review feedback, prefer the canonical TaskRun review route:

```bash
wt task review <task-run-id> --accept "<검토 통과 메시지>"
wt task review <task-run-id> --reject "<수정 요청>"
wt task review <task-run-id> --block "<외부 입력 또는 충돌 대기 사유>"
```

Late review after pass is normal. `--reject` and `--block` reopen a passed
TaskRun to `running`, so the agent can continue and report again through the
same TaskRun route. `--accept` records review metadata only; it does not pass a
running TaskRun.

For task-specific instructions that should not update review metadata, use the
low-level durable inbox route with explicit TaskRun scope:

```bash
wt msg send \
  --to <task-agent-id> \
  --scope task_run:<task-run-id> \
  "<작업 지시>"
```

`<task-agent-id>` comes from `wt inspect` TaskRun route. It is not necessarily
`agents/<branch_slug>`; workflow runs may use generated ids such as
`agents/run-900025-<task>`. If the worker uses a role identity, `wt as`, or
another explicit agent id, use that exact id. Until wt has a target-addressed
inbox send, do not pretend branch/worktree/TaskRun selectors are accepted by
`wt msg send`.

After sending a TaskRun-scoped inbox message, trust the automatic idle wake
path first. If the agent was idle, observe the same target before using live
cmux:

```bash
wt agent status <target>
wt agent watch <target> --interval 5 --timeout 30 --heartbeat 30
```

Idle is not by itself a reason to use `wt send`: a correctly routed inbox
message should wake an idle live TaskRun agent. Use `wt send <target> ...` only
after the inbox route has been tried and one of these is true:

- the agent remains idle after the short wake-observation window
- the worker is `needs_input` and hooks are not delivering
- `wt session whoami` reports no id for the live target and no exact identity anchor can
  be safely set
- the TaskRun `agent_id` or delivery route is missing, invalid, or ambiguous
- immediate prompt-level attention is explicitly more important than preserving
  the canonical message path

If neither inbox nor `wt send` can validate the target surface, use raw cmux
only after confirming the surface is the agent prompt.

## Review

Ask for a report only as input, not as proof:

```bash
wt msg send \
  --to <task-agent-id> \
  --scope task_run:<task-run-id> \
  "현재 상태를 Agent Completion Report 형식으로 짧게 보고해줘. 코드 변경이나 명령 실행은 하지 말고 상태/변경 파일/검증/리스크만 알려줘."
```

Then inspect directly:

```bash
git -C <worktree> status --short --branch
git -C <worktree> log --oneline <parent>..<branch>
git -C <worktree> diff <parent>...HEAD
```

Read touched files when behavior cannot be judged from the diff. For wt Rust
changes:

```bash
cargo fmt --all --check
cargo check --locked --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
cargo test --locked --all-features
```

Send focused feedback. Accumulate review findings across a single inspection
pass and send them as **one consolidated message**, not one message per
finding — bouncing the agent between micro-corrections wastes context and
makes "what's left" hard to track:

```bash
wt task review <task-run-id> --reject "검토 결과입니다. <파일/동작>에서 <문제>가 보입니다. <기대 수정 방향>으로 고치고, 완료 후 변경 파일/검증 결과/남은 리스크를 짧게 보고해줘."
```

If the feedback is task-specific but should not overwrite review metadata, use
`wt msg send --scope task_run:<task-run-id>` instead. In both cases, observe the
automatic inbox wake before falling back to cmux.

If the worker is `needs_input`, hooks are not delivering after a scoped inbox
send, or the worker inbox identity is unclear, use the cmux fallback:

```bash
wt send <target> "검토 결과입니다. <파일/동작>에서 <문제>가 보입니다. <기대 수정 방향>으로 고치고, 완료 후 변경 파일/검증 결과/남은 리스크를 짧게 보고해줘."
```

Use raw cmux only when `wt send` / `wt agent status` cannot resolve the target
or validate a live surface; confirm the surface is the agent prompt first.

## Sync the Spec

`wt-ready` produces a numbered spec at
`<repo-root>/.wt/planning/specs/<slug>/`. The spec is not frozen at launch. Findings
often invalidate an assumption in `07-design.md`, prove an item in
`08-tasks.md` is too coarse or mis-scoped, or show that the chosen execution
shape in `09-execution.md` has drifted.

Edit `07-design.md`, `08-tasks.md`, `09-execution.md`, and `10-review.md` in
place during the run. The TaskDocument at
`<repo-root>/.wt/execution/tasks/<slug>.toml` is the canonical launch
context for the wt CLI and is not rewritten here; only the spec artifact moves.

Drift-resolution rule: when implementation and spec disagree, update the spec.
Do not let code silently diverge. If a decision changes mid-flight, the spec
is where it lands. `04+05+06-requirements.md` carries the approved purpose and
requirements — surface needed changes to the user rather than rewriting it
silently. wt CLI does not treat `planning/specs/` as executable state, so spec edits are
coordinator-driven file edits.

Make the rationale visible:

```bash
wt msg send \
  --to <task-agent-id> \
  --scope task_run:<task-run-id> \
  "07-design.md / 08-tasks.md / 09-execution.md / 10-review.md를 업데이트했습니다. 변경: <무엇이 바뀌었나>. 이유: <왜 바뀌었나>. 이 업데이트된 spec 기준으로 진행해주세요."
```

After sending, observe the automatic inbox wake. Use `wt send <target> ...` for
this notice only when the worker still does not wake, needs immediate
prompt-level attention, or the inbox route is not reliable for this run.

### Log Mid-Process Discoveries

If unplanned research happens during the run — a domain term that needed a
definition, a convention that was not surveyed, an external example that
changed the approach, or an internal fact that was not inventoried — log it
under a `## Mid-process discoveries` section in `planning/specs/<slug>/10-review.md`
instead of silently absorbing it.

Format: one entry per discovery, dated, with a category tag (`domain` /
`standards` / `external` / `internal`) and a one-line note on what was
researched and why it was not in the original Unknown surfacing list.
`wt-retrospect` reads this section to diagnose which category was missed and
strengthen the next run's surfacing checklist. If no unplanned research
happens, do not create `10-review.md` only for an empty section.

## Complete When Applicable

Complete only after the worktree is clean and useful commits exist ahead of
the parent. Applies to workflow-linked runs after review passes.

Stack mode with the next task ready:

```bash
wt workflow pass <workflow> <task> --run-next
```

Single, batch, the final stack task, or a stack task whose successor should
wait:

```bash
wt workflow pass <workflow> <task>
```

For direct TaskRuns, no separate pass command exists before landing — see
`task-lifecycle.md`.

## Handoff

When review passes, hand the branch to `wt-land` with enough context to avoid
re-discovery:

- reviewed branch or stack order
- parent from `wt inspect`, current branch at coordination start, intended integration branch when explicitly known
- prepared workflow landing policy from `wt workflow show` (workflow work)
- worktree path and dirty/clean state
- checks run and known gaps
- expected duration, chosen watch cadence, first meaningful signal, and actual
  elapsed time when known
- coordinator id and worker agent id used for inbox messages, when known
- feedback route used (`wt task review`, TaskRun-scoped `wt msg`, or `wt send`
  fallback) and why
- stack pass command already run, if any

Report coordinated branches, feedback sent, pass command, final review
result, checks run, message route, and the exact `wt-land` target.
