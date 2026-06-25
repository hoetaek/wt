---
name: ready
description: "Use before work to verify prepared context is executable as wt development work, slice it, author TaskDocument bodies, choose execution shape and workflow policy, and hand off an exact launch target. For vague work that still needs thinking, route to leaf-work and stop."
---

# WT Ready

Use ready when the context may already be executable wt work. This skill checks
whether the work is launchable, slices it when needed, writes TaskDocument
bodies, chooses the execution shape, and hands off an exact `work` target.

ready does not run planning. Exploratory thinking, unsettled requirements,
examples, and design belong outside wt in `leaf-work` and its repo-local
`.leaf/` workspace. If the work is not executable yet, route it back to
thinking instead of forcing a TaskDocument.

## Boundaries

| Situation | Owner |
|---|---|
| vague intent, unsettled requirements, design still open | `leaf-work` (`.leaf/`) — route there and stop |
| executable development work that needs wt prep | this skill |
| launch, watch, steer, accept | `work` |
| landing, cleanup, retrospective | `land` |

Separation contract: wt never parses `.leaf/`. A TaskDocument body may cite a
`.leaf/...` path as human-facing context, and the AI may read leaf artifacts
while preparing wt work, but no wt command, schema field, or instruction depends
on them. Do not scaffold, transform, or validate the other tool's workspace.

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

Prepared context is executable as wt work when all four hold:

1. **Effect**: the desired effect fits one sentence the user agrees with.
2. **Form**: the output is development execution — code, config, docs, or
   tests in a wt-managed repo (or a direct local edit elsewhere).
3. **Observability**: acceptance can be checked by commands or review, and the
   regression-sensitive behavior to preserve is namable.
4. **Certainty**: the work can be pinned to exact file paths and symbols by
   reading the repo now — not "the agent will figure it out".

If any of the four is missing, name the gap and route: send thinking work to
`leaf-work`, or ask the cheapest question that closes it. Do not absorb
requirement decisions silently.

Label evidence in your output as **verified fact** (with source — file:line,
URL, command output) or **flagged assumption** (still to validate).
Assumptions must not ride into a TaskDocument body as facts.

## Slice The Work

When the work is bigger than one safe task, split it into thin vertical slices.
Each slice should be independently reviewable and, where possible, demoable.
For each slice record:

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

Write each TaskDocument body as the launched agent's complete work contract.
The agent starts in a fresh worktree with no conversation context; external
notes can support the body but cannot replace it.

Verify every referenced path and symbol before handoff. Put slice metadata in
the body `## 계획 (Planning)` section, not in top-level TOML fields.

### Canonical location

- The body at `<repo-root>/.wt/execution/tasks/<slug>.toml` is the canonical
  home of implementation steps.
- The slice graph lives in the ready handoff report. Do not duplicate step
  detail there.
- The body may mention external context paths (for example `.leaf/...` files)
  as human-facing rationale only. wt does not parse or interpret them.

### Body contract

Use this order:

1. `## 계획 (Planning)` — type, duration, estimate basis, watch cadence,
   dependencies, execution shape, size class, and acceptance checks.
2. `## 필수 준수 (Hard constraints)` — only when needed, within the first ~30
   lines. Include the canonical path for any cross-cutting rule.
3. `## 맥락 (Context)` — one-line goal, verified evidence with `file:line` or
   command output, and external context references by path.
4. `## 작업 (Tasks)` — each task lists exact files first, then 2-5 minute
   checkbox steps: failing check, expected failure, implementation contract,
   passing check, commit.

For code work, include complete failing test code or an exact command-based
check with expected output. For docs, config, or prototype work, use the
smallest observable check that proves the change.

### Red flags

Fix these before handoff:

- "TBD", "TODO", "나중에 결정", "적절히 처리", "엣지 케이스 처리"
- "테스트 추가" without the actual test code
- "Task N과 동일/유사" instead of repeating the contract
- a step that says what to do without how (code, contract, or command required)
- a `Run:` command without its expected outcome
- a referenced type/function/path that no earlier task defines and the repo
  does not contain
- hedged file lists ("변경 예상 파일", "likely involved") or unresolved
  alternatives — read the repo while authoring and commit to one

Before handoff, check criteria coverage, placeholder scan, symbol consistency,
command executability, workflow handoff completeness, and shared-path
invariants.

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

Report the checked evidence, facts vs assumptions, slice list, dependencies,
execution shapes, duration estimates, watch cadence, acceptance checks,
PR/landing policy source, and exact next `work` command.
