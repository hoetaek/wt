---
name: wt-work
description: "Use after wt-ready when prepared wt work should be launched or running wt work should be coordinated: verify readiness, run a task or workflow, capture the inspect target, monitor status, send feedback, review diffs/checks, pass workflow tasks when applicable, and hand accepted work to wt-land."
---

# WT Work

Launch and coordinate prepared wt work. This skill owns active execution from
pre-launch checks through accepted review/pass handoff.

Do not revise purpose, requirements, design, or task graph here. If the handoff
is incomplete, return to `wt-ready`.

Core loop:

```text
Launch -> Watch -> Steer -> Accept
```

- **Launch**: start or identify the prepared TaskRun.
- **Watch**: inspect status, worktree state, reports, and timing signals.
- **Steer**: send focused feedback, review directly, and sync the living spec.
- **Accept**: pass workflow tasks when applicable and hand accepted work to
  `wt-land`.

Object model, planning-estimate requirement, status semantics, direct vs
workflow-linked distinction, and pass vs cleanup boundaries: see
`../wt-lifecycle/references/task-lifecycle.md`.

## Boundaries

| Responsibility | Owner |
|---|---|
| idea capture and requirements/design/tasks/TaskDocument/workflow preparation | `wt-ready` |
| launch, inspect, feedback, spec sync, review, workflow pass | this skill |
| land / merge / cleanup with `wt done` | `wt-land` |

## 1. Launch

Launch means verifying readiness, starting or identifying the TaskRun, and
setting coordinator context before longer watching or feedback.

### Check Readiness

```bash
git status --short --branch
git worktree list
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/execution/tasks" "$repo_root/.wt/execution/task-runs" "$repo_root/.wt/execution/workflows" -maxdepth 1 -type f 2>/dev/null | sort
wt doctor
```

Proceed only when the selected TaskDocument/workflow is prepared:

- Task body has `계획 (Planning)` with expected duration, estimate basis, and
  suggested watch cadence.
- Handoff has acceptance checks, size class, output concept or workflow
  rationale, and PR/landing policy source.
- Worktree and tool state are healthy enough for the requested run.

If a TaskRun already exists, do not launch another one. Identify the target and
continue to Watch.

### Run

Direct task:

```bash
wt run task <task-key> --base .
wt run task                       # interactive selection
```

Saved workflow:

```bash
wt workflow task --mode <single|batch|stack|matrix> <tasks...> --base <branch>
wt run workflow
```

Use `single` for shared-workspace execution, `batch` for independent same-base
branches, `stack` for dependent branch order, and `matrix` for one local
TaskDocument across explicit profiles. For provider issues, use
`wt workflow issue --mode <single|batch|stack> ...`.

### Capture Target

After launch or when resuming an existing run, capture the inspect target:

```bash
git worktree list
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/execution/task-runs" -maxdepth 1 -type f 2>/dev/null | sort
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
```

Record the command used, created branch/worktree or workflow, TaskRun id,
inspect target, worker `agent_id`, and coordinator route when available.

### Coordinator Context

Before watching for a while or sending feedback, set a visible tab name and
session identity.

If running inside cmux, rename the current coordinator tab. Use `caller`, not
`focused`, because `focused` may be the user's tab:

```bash
cmux rename-tab --surface "$(cmux identify | jq -r .caller.surface_ref)" "작업: <한글 작업명>"
```

Use a short Korean noun phrase such as `작업: 리뷰 라우팅`, `작업: 스택 검토`, or
`작업: 릴리즈 준비`.

If inbox reports or review feedback will be used, set the coordinator session
after identifying the coordinator id from `wt inspect`:

```bash
wt session show
eval "$(wt session set coord-<work-slug>)"
echo "${WT_AGENT_ID:-}"
```

Use a semantic, one-segment coordinator id that names the work, such as
`coord-review-routing`, `coord-stack-check`, or `coord-release-prep`. Keep it to
ASCII letters, digits, dots, dashes, and underscores; do not use slashes,
`coordinator`, or throwaway names like `coord-a`.

`wt setup` installs hooks and shell integration; it does not bind an
already-running coordinator or worker session. If worker identity is missing or
ambiguous, prefer relaunching through `wt codex`, `wt claude`, or
`wt as <agent-id> -- <command...>` instead of guessing.

## 2. Watch

Watch means observing wt state and worktree evidence, not guessing from a raw
branch name or trusting a single report.

Inspect first:

```bash
wt inspect <target>
wt agent status <target>
```

Capture TaskRun status/context/workflow/branch/parent, worktree path and dirty
state, commits and diff against parent, worker/coordinator ids, cmux surface
when relevant, and runtime status.

Use a short launch validation poll to confirm the run is observable:

```bash
wt agent watch <target> --interval 5 --timeout 45 --heartbeat 45
```

Do not treat no commits or no report during that short window as stuck
evidence. For steady monitoring, choose cadence from the TaskDocument expected
duration, risk, and prior timing evidence. A useful default:

- Short or post-feedback work: watch every 10s, heartbeat every 2-5m.
- 20-60m work: watch every 20s, heartbeat every 5-10m.
- 1h+ work: watch every 30-60s, heartbeat every 10-30m.

`wt agent watch` observes the worker's runtime state; it does not tell you a
report arrived. The Agent Completion Report lands in the coordinator inbox via
`wt task report`. Once the coordinator session identity is set (Launch ->
Coordinator Context), observe that inbox for the incoming report without
claiming it:

```bash
wt msg watch --timeout <seconds>
```

Omitting `--agent` resolves the inbox from `WT_AGENT_ID`, so the watch follows
the coordinator id bound earlier. This is observation only (default timeout
300s): it does not claim, move, or acknowledge the report, the wt-managed inbox
hook still delivers it on the coordinator's next turn, and
`wt inspect <task-run-id>` renders the recorded report. Use `wt agent watch` for
worker runtime state and `wt msg watch` for report arrival; they answer
different questions. Use `wt msg list --agent <coordinator-id>` for a snapshot
instead of a wait.

If status is `running`, let the agent work unless clearly stuck. If status is
`needs_input`, Steer. If status is `idle`, inspect the worktree instead of
polling forever.

Record watch evidence for `wt-land`: expected duration, basis, launch
validation, steady cadence, first meaningful signal, state transitions, reports,
and feedback sent.

## 3. Steer

Steer means using the TaskRun route to request reports, send focused feedback,
ask for fixes, and keep the spec aligned with execution reality.

### Message Route

Prefer routes in this order:

1. `wt task review` for review verdicts and fix requests.
2. TaskRun-scoped `wt msg send` for non-verdict prompts.
3. `wt send <target>` only when inbox delivery does not wake the agent, hooks
   are not delivering, or immediate prompt-level attention is required.

Review feedback:

```bash
wt task review <task-run-id> --accept "<검토 통과 메시지>"
wt task review <task-run-id> --reject "<수정 요청>"
wt task review <task-run-id> --block "<외부 입력 또는 충돌 대기 사유>"
```

Non-verdict prompt:

```bash
wt msg send \
  --to <task-agent-id> \
  --scope task_run:<task-run-id> \
  "<작업 지시>"
```

After a scoped inbox message, observe the same target briefly before falling
back to live prompt transport:

```bash
wt agent status <target>
wt agent watch <target> --interval 5 --timeout 30 --heartbeat 30
```

### Review Directly

Treat an Agent Completion Report as input, not proof. Inspect directly:

```bash
wt inspect <task-run-id>
git -C <worktree> status --short --branch
git -C <worktree> log --oneline <parent>..<branch>
git -C <worktree> diff <parent>...HEAD
```

Read touched files when the behavior cannot be judged from the diff. Run checks
scaled to risk. For wt Rust changes, the usual baseline is:

```bash
cargo fmt --all --check
cargo check --locked --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
cargo test --locked --all-features
```

Accumulate findings across one inspection pass and send one consolidated
message. Do not drip one message per finding.

### Sync The Spec

If implementation and spec disagree, update the living spec instead of letting
them drift. Common files:

- `07-design.md` when a design assumption changed.
- `08-tasks.md` when a slice was too broad, too narrow, or misordered.
- `09-execution.md` when execution shape or rationale changed.
- `10-review.md` for review evidence and mid-process discoveries.

Do not silently rewrite approved purpose/requirements in
`04+05+06-requirements.md`; surface that change to the user.

If unplanned research happened during execution, record it in
`10-review.md` under `## Mid-process discoveries` with a category:
`domain`, `standards`, `external`, or `internal`. `wt-land` uses this to improve
future unknown surfacing.

## 4. Accept

Accept means the coordinator has directly reviewed the work and can hand it to
landing. It is not merge or cleanup; those belong to `wt-land`.

For workflow-linked runs, pass only after review succeeds, useful commits exist
ahead of the parent, and the worktree is clean:

```bash
wt workflow pass <workflow> <task>
wt workflow pass <workflow> <task> --run-next
```

Direct TaskRuns have no separate pass command before landing.

Hand accepted work to `wt-land` with:

- reviewed branch or stack order
- parent and intended integration branch when known
- worktree path and clean/dirty state
- checks run and known gaps
- workflow landing policy source when applicable
- watch evidence: expected duration, cadence, first meaningful signal, elapsed
  time when known
- feedback route used and any pass command already run
- exact `wt-land` target

Report the launch command, TaskRun/inspect target, feedback sent, review/check
result, pass command when applicable, and next `wt-land` target.
