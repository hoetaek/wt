---
name: wt-coordinate
description: "Use when coordinating running wt work: inspect status, send feedback, review diffs/checks, complete workflow tasks when needed, and hand accepted work to wt-land."
---

# WT Coordinate

Use this skill after wt work has started. Use `wt-start` for creating
TaskDocuments or launching new task/workflow runs. Use `wt-land` after review
passes and the remaining work is landing or cleanup.

## Lifecycle Boundaries

Keep these separate:

- `coordinate`: inspect runtime state, unblock agents, and keep
  `specs/<slug>/design.md` and `tasks.md` in sync with execution findings.
- `review`: read the diff, files, checks, parent branch, and agent report.
- `complete`: for workflow-linked TaskRuns, mark reviewed running workflow
  tasks complete with `wt workflow complete`.
- `land`: merge or otherwise integrate reviewed commits; handle this in
  `wt-land`.
- `done`: remove worktrees and local branches with `wt done`; handle this in
  `wt-land` after landing or discard proof.

TaskRun `done` is not proof that a branch landed. Runtime agent `idle` is not
TaskRun completion; it means the coordinator should inspect the worktree.
For direct TaskRuns, there is no separate completion command before landing;
`wt done` marks matching running direct TaskRuns done during cleanup after
landing or discard proof. For workflow-linked TaskRuns, completion belongs to
`wt workflow complete`; cleanup must not use `wt done` as that lifecycle
transition.

## Inspect

Start with wt's read-only cockpit commands:

```bash
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
```

Capture:

- TaskRun id, status, context, workflow/group, branch, and parent
- worktree path and dirty state
- commits and diff against the parent
- cmux workspace/surface and runtime status

For scripts, prefer:

```bash
wt --json agent status <target>
```

If `wt agent status` says `running`, let the agent work unless it is clearly
stuck. When waiting right after newly assigning work to an agent, prefer a quiet
heartbeat so coordination does not become noise:

```bash
wt agent watch <target> --heartbeat 120
```

Use a shorter heartbeat only when actively debugging a suspected stall, waiting
after focused feedback, or expecting an immediate transition. If the agent says
`needs_input`, inspect and send feedback. If it says `idle`, review the worktree
instead of polling.

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

Read touched files when behavior cannot be judged from the diff. Run checks
scaled to risk. For wt Rust changes, prefer:

```bash
cargo fmt --all --check
cargo check --locked --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
cargo test --locked --all-features
```

Send focused feedback when needed:

```bash
wt send <target> "검토 결과입니다. <파일/동작>에서 <문제>가 보입니다. <기대 수정 방향>으로 고치고, 완료 후 변경 파일/검증 결과/남은 리스크를 짧게 보고해줘."
```

Use raw cmux only when `wt send` or `wt agent status` cannot resolve the target
or validate a live surface, and first confirm the surface is the agent prompt.

## Sync the Spec During Coordination

`wt-ready` produces a committed spec at
`<git-common-dir>/wt/specs/<slug>/{requirements.md, design.md, tasks.md}`. That
spec is not frozen at launch. While work runs, findings often invalidate an
assumption in `design.md` or show that an item in `tasks.md` is too coarse,
mis-scoped, or already obsolete.

wt-coordinate has the explicit power and responsibility to edit
`specs/<slug>/design.md` and `specs/<slug>/tasks.md` in place during the run.
The TaskDocument body at `<git-common-dir>/wt/tasks/<slug>.toml` remains the
canonical launch context for the wt CLI and is not rewritten here; only the
spec artifact moves. Treat `specs/<slug>/` as the long human/AI artifact that
explains the work's intent and structure, and keep it true.

Drift-resolution rule: when implementation and spec disagree, update the spec.
Do not let the code silently diverge from `design.md` or `tasks.md`. If a
decision changes mid-flight, the spec is the place that change lands.

When you edit a spec file, make the rationale visible in coordination feedback
so the task agent and user can see why the design or task list moved:

```bash
wt send <target> "design.md / tasks.md를 업데이트했습니다. 변경: <무엇이 바뀌었나>. 이유: <왜 바뀌었나>. 이 업데이트된 spec 기준으로 진행해주세요."
```

`requirements.md` is the original intent from `wt-ready` and is not edited
here; if requirements themselves need to change, surface that to the user
rather than rewriting it silently. wt CLI does not read or write `specs/`, so
spec edits are coordinator-driven file edits, not a new wt subcommand.

## Complete When Applicable

Complete only after the worktree is clean and useful commits exist ahead of the
parent.

This step applies to workflow-linked runs after review passes. For stack mode,
use `--run-next` only when the next stack task should start:

```bash
wt workflow complete <workflow> <task> --run-next
```

For single, batch, the final stack task, or a stack task whose successor should
wait, omit `--run-next`:

```bash
wt workflow complete <workflow> <task>
```

Do not use `wt done` to complete workflow-linked work. `wt done` is cleanup for
landed or intentionally discarded worktrees and marks only direct running
TaskRuns done.

## Handoff

When review passes, hand the branch to `wt-land` with enough context to avoid
re-discovery:

- reviewed branch or stack order
- parent from `wt inspect`, current branch at coordination start, and intended
  integration branch when explicitly known
- prepared workflow landing policy from `wt workflow show`, if this is workflow
  work
- worktree path and dirty/clean state
- checks run and known gaps
- stack completion command already run, if any

Report the coordinated branches, feedback sent, completion command, final
review result, checks run, and the exact `wt-land` target.
