---
name: wt-start
description: "Use when prepared wt work is ready to launch: run a direct task or workflow, verify `wt doctor`, and capture the inspect target."
---

# WT Start

Use this skill to start prepared wt work. Stop after the run is launched and
the inspect target is clear. Use `wt-ready` first when the task/workflow does
not already exist or when scope, slice order, evidence, PR policy, or landing
policy is still unsettled.

Use `wt-coordinate` for monitoring, feedback, and review after work is running.
Use `wt-land` for landing and cleanup after review passes.

In the work-sequence model, this skill consumes the execution handoff from
`wt-ready` and turns it into a concrete TaskRun/worktree/workflow plus an
inspect target. Do not revise purpose, requirements, design, or task graph here
unless launch reveals that the handoff is incomplete; in that case return to
`wt-ready`.

## Current State

Check the repo and runtime before choosing a command:

```bash
git status --short --branch
git worktree list
common_dir="$(git rev-parse --git-common-dir)"
find "$common_dir/wt/tasks" "$common_dir/wt/task-runs" "$common_dir/wt/workflows" -maxdepth 1 -type f 2>/dev/null | sort
wt doctor
```

Confirm:

- worktree cleanliness and existing task/workflow state
- `Agent` is not `none` when agent work is expected
- `cmux CLI` and `[workspace]` config are ready when workspace automation is expected
- issue provider is configured before issue-based workflows

## Task Model

- `TaskDocument`: `<git-common-dir>/wt/tasks/<task>.toml`; defines the work.
- `TaskRun`: `<git-common-dir>/wt/task-runs/<id>.toml`; records one execution attempt.
- workflow file: saved orchestration for `single`, `batch`, or `stack` execution.

TaskDocuments store intent, not runtime status. TaskRuns store status, branch,
group, error, and timestamps. A missing `group` means direct execution; a
`group` matching a workflow file stem plus that workflow's `[[tasks]].run`
link makes the run workflow-linked. Workflow mode lives on the workflow, not on
TaskRun.

TaskDocuments and workflow tasks must carry a planning estimate before launch.
Until the repo schema explicitly supports a machine field, the estimate belongs
in the TaskDocument `body` under `계획 (Planning)` as a line containing
`expected duration`, not as a top-level TOML field.

## Command Choice

Use `wt run task` when each selected TaskDocument should get its own worktree now:

```bash
wt run task <task-key> --base .
wt run task
```

Use `wt workflow task --mode single` when multiple TaskDocuments should share
one saved workspace run:

```bash
wt workflow task --mode single <task-a> <task-b> --base .
wt run workflow
```

Use `wt workflow task --mode batch` when tasks are independent and may run from
the same base:

```bash
wt workflow task --mode batch <task-a> <task-b> --base <base-branch>
wt run workflow
```

Use `wt workflow task --mode stack` when task order matters and each branch
should build on the previous task branch:

```bash
wt workflow task --mode stack <task-a> <task-b> <task-c> --base <base-branch>
wt run workflow
```

Use `wt workflow task --mode matrix` when one local TaskDocument should run
across explicit named profiles:

```bash
wt workflow task --mode matrix <task> --profiles <profile-a>,<profile-b> --base <base-branch>
wt run workflow
```

For provider issues, use `wt workflow issue --mode <single|batch|stack> ...`
when a saved workflow is useful. Matrix mode is local-TaskDocument only; do not
try to start provider issues as matrix workflows.

## Start Rules

- Prefer explicit task keys in scripts; omit keys only for interactive selection.
- Before launching, inspect the selected TaskDocument bodies or workflow task
  prompts and confirm every task has `계획 (Planning)` and `expected duration`.
  If any task is missing an expected duration, do not start it; return to
  `wt-ready` or update the TaskDocument body first.
- Also confirm the handoff has acceptance checks, size class, output concept or
  workflow rationale, and PR/landing policy source when relevant. If these are
  missing, return to `wt-ready`; launch should not invent planning context.
- Use `--base .` for current branch, `--base <branch>` for an explicit base,
  or bare `--base` for interactive base selection.
- Direct TaskDocument execution is `wt run task`; use workflow commands only for saved workflow execution.
- Do not prepare a workflow when one direct worktree run is enough.
- Do not use batch for parent-dependent tasks; use stack mode.
- Do not decide PR or landing preferences here; use the prepared workflow policy
  from `wt-ready` or the repository config.
- Do not report TaskRun `running` as active agent work without `wt agent status`.

## After Launch

Verify the run and capture the inspect target:

```bash
git worktree list
common_dir="$(git rev-parse --git-common-dir)"
find "$common_dir/wt/task-runs" -maxdepth 1 -type f 2>/dev/null | sort
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
```

Report the command used, created branch/worktree or workflow, TaskRun id when
available, and the next `wt inspect` target. If the agent is still running and
you need to wait for a state transition, use `wt agent watch <target>`.
