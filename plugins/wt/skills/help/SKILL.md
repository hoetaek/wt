---
name: help
description: "Quick-reference card for wt lifecycle skills, setup, and common `wt run` starts. One-shot display, not a persistent mode."
---

# WT Help

Display this reference card when invoked. One-shot, do not run `wt setup`,
`wt init`, `wt run`, or edit config unless the user explicitly asks for
execution.

## Mental Model

Show this compact map in the user's language:

```text
wt setup   = prepare this machine
wt init    = prepare this repo's wt state/config
wt run     = start work
autopilot = after ready, run, review, land, and settle the work
config  = inspect this repo and recommend the right config
```

Explain that `wt run` starts workspaces; it does not own review, landing,
cleanup, or long-running coordination. Those are handled by `autopilot` and its
phase skills.

## Skills

| Skill | Trigger | What it does |
|-------|---------|--------------|
| **help** | `$help` | This card. |
| **config** | `$config` | Inspect the current repo and recommend wt config. |
| **ready** | `$ready` | Turn intent into an approved launch target. |
| **work** | `$work` | Launch or coordinate prepared wt work. |
| **land** | `$land` | Review, land or discard, and settle lessons. |
| **autopilot** | `$autopilot` | Run the ready-approved path through work and land. |
| **variants** | `$variants` | Compare multiple implementation variants. |
| **writing-tasks** | `$writing-tasks` | Author or revise wt TaskDocument bodies. |

## Setup

Machine/user concerns such as shell integration, agent hooks, cmux, and agent
CLIs belong to setup and doctor:

```bash
wt setup
wt doctor
wt doctor --profile codex
```

## Common Starts

```bash
wt run branch <words...>
wt run issue
wt run issue PROJ-123 --base .
wt run pr
wt run task
wt run workflow
```

Explain each command in one short phrase if helpful.

## Deeper Model

Only when the user asks for more detail, explain these concepts compactly:

- **TaskDocument:** a local executable work unit stored under
  `<repo-root>/.wt/execution/tasks/`.
- **Workflow:** a saved execution plan that groups TaskDocuments or provider
  issues.
- **TaskRun:** the record of one launched execution.
- **Modes:** `single` shares one workspace, `batch` runs independent branches,
  `stack` runs ordered dependent branches, and `matrix` runs one task through
  multiple profiles.
- **autopilot loop:** `$ready -> $work -> $land`.

Keep the deeper model conceptual unless the user asks for concrete commands.

## Guardrails

- Prefer "what owns what" over "everything wt can do".
- If the user is already inside a repo and asks what to do next, end with:
  "Run `$config` here first."
- If the user is ready to do real work, suggest `$autopilot` after config is
  healthy.
