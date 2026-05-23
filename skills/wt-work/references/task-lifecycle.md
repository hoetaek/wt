# Task Lifecycle

Shared reference for wt skills that touch TaskDocuments, TaskRuns, and
workflow files (`wt-start`, `wt-coordinate`, `wt-land`). Defines the object
model, status boundaries, and completion vs cleanup commands.

## Object Model

- **TaskDocument**: `<git-common-dir>/wt/tasks/<task>.toml`. Stores intent —
  what the work is. Body must include a `계획 (Planning)` section with an
  `expected duration` line before launch. The estimate is a human-facing
  planning line in the body, not a top-level TOML field.
- **TaskRun**: `<git-common-dir>/wt/task-runs/<id>.toml`. Records one execution
  attempt — status, branch, group, error, timestamps. Does not store intent.
- **workflow file**: saved orchestration for `single`, `batch`, `stack`, or
  `matrix` execution. The workflow file carries the mode; TaskRuns do not.

## Direct vs Workflow-Linked

- TaskRun with no `group` → **direct** execution.
- TaskRun whose `group` matches a workflow file stem AND that workflow's
  `[[tasks]].run` links back → **workflow-linked**.

## Status Boundaries

| State | Means | Does NOT mean |
|---|---|---|
| TaskRun `done` | Execution attempt closed | Branch landed |
| Agent `idle` | Coordinator should inspect | TaskRun complete |
| Worktree clean | No uncommitted changes | Branch reviewed or landed |
| Agent `running` | Let it work unless clearly stuck | Active progress proven |

## Completion vs Cleanup

| Run Type | Completion (before landing) | Cleanup (after landing/discard) |
|---|---|---|
| workflow-linked | `wt workflow complete <workflow> <task> [--run-next]` | `wt done <branch-or-worktree>` |
| direct | (no separate step) | `wt done <branch-or-worktree>` — also marks running direct TaskRuns done |

`--run-next` applies in stack mode when the next stack task should start
immediately. For `single`, `batch`, the final stack task, or a stack task
whose successor should wait, omit it.

`wt done` is cleanup only. Do not use it as a substitute for `wt workflow
complete`. Cleanup runs after landing is proven or discard intent is explicit.
