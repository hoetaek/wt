---
name: wt-start
description: "Use when prepared wt work is ready to launch: run a direct task or workflow, verify `wt doctor`, and capture the inspect target."
---

# WT Start

Launch prepared wt work. Stop after the run is launched and the inspect target
is captured. Do not revise purpose, requirements, design, or task graph here;
return to `wt-ready` if launch reveals the handoff is incomplete.

Object model, planning-estimate requirement, and direct vs workflow-linked
distinction: see `../wt-work/references/task-lifecycle.md`.

## Current State

```bash
git status --short --branch
git worktree list
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/execution/tasks" "$repo_root/.wt/execution/task-runs" "$repo_root/.wt/execution/workflows" -maxdepth 1 -type f 2>/dev/null | sort
wt doctor
```

Confirm: worktree cleanliness, existing task/workflow state, `Agent` is not
`none` when agent work is expected, `cmux CLI` and `[workspace]` ready when
workspace automation is expected, issue provider configured for issue
workflows.

## Command Choice

Direct task — each TaskDocument gets its own worktree:

```bash
wt run task <task-key> --base .
wt run task                       # interactive selection
```

Saved workflow — multiple TaskDocuments in one saved run:

```bash
wt workflow task --mode <single|batch|stack|matrix> <tasks...> --base <branch>
wt run workflow
```

Mode selection:

- `single`: tasks share one saved workspace
- `batch`: tasks independent from the same base
- `stack`: each branch builds on the previous task branch (order matters)
- `matrix`: one local TaskDocument across explicit named profiles
  (`--profiles <a>,<b>`); local-TaskDocument only

For provider issues, use `wt workflow issue --mode <single|batch|stack> ...`.

## Start Rules

- Prefer explicit task keys in scripts; omit only for interactive selection.
- Verify every selected task body has `계획 (Planning)` with `expected duration`,
  `estimate basis`, and suggested watch cadence. If missing, return to
  `wt-ready`.
- Verify the handoff has acceptance checks, size class, output concept or workflow rationale, and PR/landing policy source. If missing, return to `wt-ready`.
- `--base .` for current branch, `--base <branch>` for explicit base, bare `--base` for interactive base selection.
- Direct execution is `wt run task`; workflow commands only for saved workflow execution.
- Do not prepare a workflow when one direct worktree run is enough.
- Do not use batch for parent-dependent tasks — use stack.
- Do not decide PR or landing preferences here; use prepared workflow policy or repo config.
- Do not report TaskRun `running` as active agent work without `wt agent status`.

## After Launch

Capture the inspect target:

```bash
git worktree list
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/execution/task-runs" -maxdepth 1 -type f 2>/dev/null | sort
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
```

Report: command used, created branch/worktree or workflow, TaskRun id when
available, next `wt inspect` target, and the recorded worker `agent_id` /
coordinator route. If inbox delivery will be used, confirm the launched process
got identity from `wt codex`, `wt claude`, `wt as`, or a live identity anchor;
`wt setup` alone only installs hooks and does not bind an already-running
session. Use `wt agent watch <target>` if waiting for a state transition.
