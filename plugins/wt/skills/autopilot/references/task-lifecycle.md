# Task Lifecycle

Shared reference for wt skills that touch TaskDocuments, TaskRuns, and
workflow files (`work`, `land`). Defines the object model, status
boundaries, and pass vs cleanup commands.

## Object Model

- **TaskDocument**: `<repo-root>/.wt/execution/tasks/<task>.toml`. Stores intent —
  what the work is. Body must include a `계획 (Planning)` section with an
  `expected duration` line before launch. The estimate is a human-facing
  planning line in the body, not a top-level TOML field.
- **TaskRun**: `<repo-root>/.wt/execution/task-runs/<id>.toml`. Records one execution
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
| TaskRun `passed` | Current coordinator gate passed | Branch landed or cleanup complete |
| Agent `idle` | Coordinator should inspect | TaskRun passed |
| Worktree clean | No uncommitted changes | Branch reviewed or landed |
| Agent `running` | Let it work unless clearly stuck | Active progress proven |

## Pass vs Cleanup

| Run Type | Pass (before landing) | Cleanup (after landing/discard) |
|---|---|---|
| workflow-linked | `wt workflow pass <workflow> <task> [--run-next]` | `wt done <branch-or-worktree>` |
| direct | (no separate step) | `wt done <branch-or-worktree>` — also marks running direct TaskRuns passed |

`--run-next` applies in stack mode when the next stack task should start
immediately. For `single`, `batch`, the final stack task, or a stack task
whose successor should wait, omit it.

Late review can reopen a passed TaskRun. Use `wt task review <task-run-id>
--reject ...` or `--block ...` to send scoped feedback and transition
`passed` back to `running`; the task agent then reports again with
`wt task report`.

`wt done` is cleanup only. Do not use it as a substitute for `wt workflow
pass`. Cleanup runs after landing is proven or discard intent is explicit.
