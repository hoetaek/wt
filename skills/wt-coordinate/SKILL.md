---
name: wt-coordinate
description: "Use when coordinating running wt work: inspect status, send feedback, review diffs/checks, complete workflow tasks when needed, and hand accepted work to wt-land."
---

# WT Coordinate

Coordinate running wt work. Object model, status semantics, and completion vs
cleanup boundaries: see `../wt-work/references/task-lifecycle.md`.

Stay on these responsibilities; do not absorb later phases:

| Phase | Owner |
|---|---|
| inspect / feedback / spec sync | this skill |
| `wt workflow complete` for workflow-linked runs | this skill |
| land / merge | `wt-land` |
| cleanup with `wt done` | `wt-land` |

## Inspect

```bash
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
```

Capture TaskRun id/status/context/workflow/branch/parent, worktree path and
dirty state, commits and diff against parent, cmux workspace/surface and
runtime status. Scripts: `wt --json agent status <target>`.

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
common_dir="$(git rev-parse --git-common-dir)"
find "$common_dir/wt/specs" "$common_dir/wt/retrospectives" -type f \( -name '11-retrospect.md' -o -name 'timing.md' \) 2>/dev/null
wt agent wait-stats
```

`wt agent wait-stats` summarizes `agent.state` wait observations and can inform
cadence, but it is not an adaptive default engine and does not replace the task
estimate or coordinator judgment.

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

## Review

Ask for a report only as input, not as proof:

```bash
wt send <target> "현재 상태를 Agent Completion Report 형식으로 짧게 보고해줘. 코드 변경이나 명령 실행은 하지 말고 상태/변경 파일/검증/리스크만 알려줘."
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
wt send <target> "검토 결과입니다. <파일/동작>에서 <문제>가 보입니다. <기대 수정 방향>으로 고치고, 완료 후 변경 파일/검증 결과/남은 리스크를 짧게 보고해줘."
```

Use raw cmux only when `wt send` / `wt agent status` cannot resolve the target
or validate a live surface; confirm the surface is the agent prompt first.

## Sync the Spec

`wt-ready` produces a committed numbered spec at
`<git-common-dir>/wt/specs/<slug>/`. The spec is not frozen at launch. Findings
often invalidate an assumption in `07-design.md`, prove an item in
`08-tasks.md` is too coarse or mis-scoped, or show that the chosen execution
shape in `09-execution.md` has drifted.

Edit `07-design.md`, `08-tasks.md`, `09-execution.md`, and `10-review.md` in
place during the run. The TaskDocument at `<git-common-dir>/wt/tasks/<slug>.toml`
is the canonical launch context for the wt CLI and is not rewritten here; only
the spec artifact moves.

Drift-resolution rule: when implementation and spec disagree, update the spec.
Do not let code silently diverge. If a decision changes mid-flight, the spec
is where it lands. `04+05+06-requirements.md` carries the approved purpose and
requirements — surface needed changes to the user rather than rewriting it
silently. wt CLI does not treat `specs/` as executable state, so spec edits are
coordinator-driven file edits.

Make the rationale visible:

```bash
wt send <target> "07-design.md / 08-tasks.md / 09-execution.md / 10-review.md를 업데이트했습니다. 변경: <무엇이 바뀌었나>. 이유: <왜 바뀌었나>. 이 업데이트된 spec 기준으로 진행해주세요."
```

### Log Mid-Process Discoveries

If unplanned research happens during the run — a domain term that needed a
definition, a convention that was not surveyed, an external example that
changed the approach, or an internal fact that was not inventoried — log it
under a `## Mid-process discoveries` section in `specs/<slug>/10-review.md`
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
wt workflow complete <workflow> <task> --run-next
```

Single, batch, the final stack task, or a stack task whose successor should
wait:

```bash
wt workflow complete <workflow> <task>
```

For direct TaskRuns, no separate completion exists before landing — see
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
- stack completion command already run, if any

Report coordinated branches, feedback sent, completion command, final review
result, checks run, and the exact `wt-land` target.
