---
name: ready
description: "Use before work to verify prepared context is executable as wt development work, slice it, author TaskDocument bodies via writing-tasks, choose execution shape and workflow policy, and hand off an exact launch target. For vague work that still needs thinking, route to leaf-work and stop."
---

# WT Ready

wt execution readiness checker and handoff authoring helper. This skill does
not run a planning process: exploratory thinking, requirements, examples, and
design belong outside wt (the `leaf-work` skill and its repo-local `.leaf/`
workspace). ready starts when context might already be executable and ends
with either an exact `work` launch target or an explicit routing decision
back to thinking.

## Boundaries

| Situation | Owner |
|---|---|
| vague intent, unsettled requirements, design still open | `leaf-work` (`.leaf/`) — route there and stop |
| executable development work that needs wt prep | this skill |
| launch, watch, steer, accept | `work` |
| landing, cleanup, retrospective | `land` |

Separation contract: wt never parses `.leaf/`. A TaskDocument body may mention
a `.leaf/...` path as human-facing context, and the AI may read leaf artifacts
to author wt work, but no wt command, schema field, or instruction depends on
them. Do not scaffold, transform, or validate the other tool's workspace.

## First Read

Inspect local wt truth before asking questions:

```bash
git status --short --branch
find . -maxdepth 2 -name AGENTS.md -o -name AGENTS.override.md
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/execution/tasks" "$repo_root/.wt/execution/workflows" -maxdepth 1 -type f 2>/dev/null | sort
cat "$repo_root/.wt.toml" "$repo_root/.wt/config/local.toml" 2>/dev/null
```

Check current command help when behavior matters; installed `wt` may differ
from `./target/debug/wt`. For `wt` itself, read `docs/consistency.md` before
proposing model, CLI, config, workflow, or state changes.

If repo policy says the current branch is planning-only, limit direct work
there to reading, reference gathering, and TaskDocument/workflow preparation.
Implementation belongs in a wt task/workflow branch.

## Verify Executability

Prepared context (a conversation decision, a leaf artifact, an issue, a spec
the user points at) is executable as wt work when all four hold:

1. **Effect**: the desired effect fits one sentence the user agrees with.
2. **Form**: the output is development execution — code, config, docs, or
   tests in a wt-managed repo (or a direct local edit elsewhere).
3. **Observability**: acceptance can be checked by commands or review, and the
   regression-sensitive behavior to preserve is namable.
4. **Certainty**: the work can be pinned to exact file paths and symbols by
   reading the repo now — not "the agent will figure it out".

If any of the four is missing, name the missing one and route: send thinking
work to `leaf-work`, or ask the user the single cheapest question that closes
the gap. Do not force prep on unsettled intent, and do not absorb requirement
decisions silently — surface them.

Label evidence in your output as **verified fact** (with source — file:line,
URL, command output) or **flagged assumption** (still to validate).
Assumptions must not ride into a TaskDocument body as facts.

## Slice The Work

When the work is bigger than one safe task, split it into thin vertical
slices. Each slice should be independently reviewable and, where possible,
demoable. For each slice record:

- title
- type: `AFK` when an agent can implement it without more human input, `HITL`
  when a decision/review is required
- blocked by
- execution shape: direct, batch, stack, separate workflow, or direct local edit
- expected duration before first coordinator review, derived from
  `<repo-root>/.wt/execution/retrospectives/timing.md`, prior retrospectives,
  or `wt agent wait-stats`; otherwise a conservative planning guess or range
- estimate basis: which of the above produced the number
- suggested watch cadence: launch validation and steady heartbeat interval for
  `work`
- expected size class: `small`, `medium`, or `large-justified` — consult
  `references/task-pr-size-guidance.md`; for `large-justified` record why
  splitting would be worse and what checks reduce the risk
- acceptance checks

Prefer several narrow slices over one broad task. Keep HITL slices separate
from AFK implementation slices when the human decision can change the
implementation plan.

## TaskDocument Authoring

**REQUIRED SUB-SKILL:** Use writing-tasks for every TaskDocument body. It
owns the body structure (계획 / 필수 준수 / 맥락 / 작업), top-of-body hard
constraints, implementation-grade steps with complete failing tests, the
no-placeholder rule, and the pre-handoff self-review.

Verify every referenced path and symbol against the repo before handoff. The
slice metadata above goes in the body `## 계획 (Planning)` section, not as
top-level TOML fields.

## Choose Execution Shape

Classify dependency and work surface; do not default to one stack:

| Slice graph | Shape |
|---|---|
| one slice, single branch is enough | direct `wt run task <slug>` |
| all sequential, single agent | `single` |
| independent, same base | `batch` |
| later slice needs the previous slice's branch commits | `stack` |
| one task × multiple profiles | `matrix` |
| independent but different bases/repos/lifecycles | separate workflows |
| outside the wt-managed repo, simpler than a workflow | direct local edit |

A stack is a dependency claim — "T2 really cannot compile/exist without T1's
commits" must survive challenge. When unsure, prefer batch or separate
workflows over a false parent chain. If the graph is wave-shaped, split it
into explicit launch waves, one workflow per wave.

Create saved workflows with:

```bash
wt workflow task --mode <single|batch|stack|matrix> <tasks...> --base <branch>
```

## Workflow Policy

Treat `.wt.toml` / `<repo-root>/.wt/config/local.toml` `[workflow]` as
preparation policy and the workflow TOML as the prepared run's effective
policy snapshot. If policy is missing, stale, or risky for the current work,
ask; otherwise apply it and record the source in the handoff.

- PR mode: `none`, `draft`, or `ready`
- landing: `manual` (coordinator stops after review until the user directs
  landing) or `auto` (review passing is enough to proceed to landing/cleanup,
  still enforcing dirty-worktree, check, unresolved-review, and ancestry
  safety)
- review: `[policy.review].codex_base` gate when configured

## Handoff

End with one of these concrete outputs:

- existing TaskDocuments/workflow verified ready, with the exact `work`
  target
- new TaskDocument TOML files prepared
- a saved workflow prepared (mode, base, order, policy)
- a routing decision to `leaf-work` for thinking work, with what is missing
- a short list of unresolved HITL decisions that blocks launch

Report: evidence checked (facts vs assumptions), slice list with dependencies
and shapes, expected duration per slice with estimate basis and watch cadence,
acceptance checks, PR/landing policy source, and the exact next command for
`work`.
