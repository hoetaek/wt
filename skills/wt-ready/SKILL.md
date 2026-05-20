---
name: wt-ready
description: "Use before wt-start to make unclear wt work launch-ready: gather evidence, split slices, choose execution shape/policy, and prepare TaskDocuments or workflow handoff."
---

# WT Ready

Use this skill to prepare wt work before execution. Stop when the work is
ready for `wt-start`: the scope is clear, evidence is gathered, slices are
ordered, and the task/workflow handoff is explicit.

Do not launch workspaces from this skill. Use `wt-start` after preparation,
`wt-coordinate` after work is running, and `wt-land` after review passes.

## First Read

Inspect local truth before asking questions:

```bash
git status --short --branch
find . -maxdepth 2 -name AGENTS.md -o -name AGENTS.override.md
common_dir="$(git rev-parse --git-common-dir)"
find "$common_dir/wt/tasks" "$common_dir/wt/workflows" -maxdepth 1 -type f 2>/dev/null | sort
```

For `wt` itself, read `docs/consistency.md` before proposing model, CLI,
config, workflow, or state changes. Check current command help when behavior
matters; installed `wt` may differ from `./target/debug/wt`.

If the repo policy says the current branch is planning-only, limit direct work
there to reading, reference gathering, experiments, and TaskDocument/workflow
preparation. Implementation belongs in a wt task/workflow branch.

## Gather Evidence

Work from the conversation, issue references, existing docs, current code, and
current runtime behavior. If the user says how something works, verify it
against the repo when it is cheap.

Useful evidence:

- docs and glossary terms that define the domain model
- current config shape and local overrides such as `.wt.toml` / `<git-common-dir>/wt/config.toml`
- current persisted state such as `<git-common-dir>/wt/tasks` and `<git-common-dir>/wt/workflows`
- command help for user-facing CLI contracts
- tests or small local experiments for uncertain behavior
- external references only when the user asks or the decision depends on current
  best practice outside the repo

Separate confirmed facts, experiment results, tradeoffs, and unresolved
questions in your notes or response.

## Questions

Ask only when the answer cannot be discovered from the repo or a reasonable
assumption would be risky. Ask one focused question at a time and include your
recommended answer.

Resolve terminology as you go. If the user uses a term that conflicts with the
repo docs or code, point to the conflict and propose the canonical term.

## Slice The Work

When the work is bigger than one safe task, split it into thin vertical slices.
Each slice should be independently reviewable and, where possible, demoable.
This is a planning step, not just a list-making step: decide which slices truly
depend on each other before choosing a workflow shape.

For each slice, record:

- title
- type: `AFK` when an agent can implement it without more human input, `HITL`
  when a decision/review is required
- blocked by
- execution shape: direct, batch, stack, separate workflow, or direct local edit
- expected duration before first coordinator review, such as `20m`, `45m`, or
  `2h`; use a conservative estimate or range when uncertain
- acceptance checks
- notes for experiments or tradeoffs that shaped the slice

When you prepare new TaskDocuments, preserve this planning context inside the
TaskDocument `body` as a text section, not as top-level TOML fields. Current
TaskDocument TOML accepts only the canonical task fields; planning metadata is
for the task agent to read in the prompt. Every prepared TaskDocument or
workflow task must include an expected duration in its `Planning:` body section
before `wt-start`; if an existing TaskDocument lacks one, update the body or
stop with that as the remaining preparation work.

Example body section:

```text
Planning:
- type: AFK
- expected duration: 45m
- blocked by: workflow-policy-contract-simplified
- execution shape: stack child
- acceptance checks: update docs, run cargo fmt --all --check
```

Prefer several narrow slices over one broad task.

## Choose Execution Shape

Do not put every slice into one stack by default. First classify dependency and
work surface:

- Use one direct task run when a single branch is enough.
- Use `batch` when slices are independent, can start from the same base, and do
  not need each other's branch commits.
- Use `stack` only when a later slice must build on the previous slice's branch
  or the review order is intrinsically parent-to-child.
- Use separate workflows when slices are independent but have different bases,
  repositories, agents, or lifecycle policies.
- Use a direct local edit when the work is outside the wt-managed repo and is
  simpler than preparing a wt workflow, such as a small shared-skill update in
  dotfiles.
- Keep HITL slices separate from AFK implementation slices when the human
  decision can change the implementation plan.

If two slices can run in parallel, they should not be placed in the same stack
only because they came from the same conversation. A stack is a dependency
claim. When unsure, explain the dependency assumption and prefer batch or
separate workflows over a false parent chain.

## Workflow Policy

Treat `.wt.toml` / `<git-common-dir>/wt/config.toml` `[workflow]` as workflow preparation
policy and workflow TOML as the prepared run's effective policy snapshot.

Read existing workflow policy when present. If policy is missing, stale, or
risky for the current work, ask. Otherwise, apply the effective policy and
record it in the handoff.

Policy questions include:

- whether PR handoff is intended
- PR mode: `none`, `draft`, or `ready`
- landing mode: `manual` or `auto`

Review is always part of the coordinator flow. `landing = "manual"` means the
coordinator stops after review until the user explicitly directs landing.
`landing = "auto"` means review passing is enough approval for the coordinator
to proceed to landing/cleanup, while still enforcing dirty-worktree, check,
unresolved-review, and ancestry safety checks.

Do not treat policy as state. PR review result, merge ancestry, cleanup,
TaskRun lifecycle, and branch deletion remain explicit later steps.

## Prepare Handoff

End with one of these concrete outputs:

- existing TaskDocuments/workflow are ready, with the exact `wt-start` target
- new TaskDocument TOML files are prepared
- a saved workflow is prepared, including mode, base, order, and policy
- a short list of unresolved HITL decisions blocks launch

Use existing repo patterns for TaskDocument bodies. Avoid stale implementation
file paths unless they are necessary for the task. For new TaskDocuments,
include a concise `Planning:` section in `body` when HITL/AFK classification,
expected duration, dependencies, execution shape, or acceptance checks would
help the task agent. Do not add fields such as `type`, `blocked_by`,
`expected_duration`, or `[planning]` to the TaskDocument TOML unless the repo
schema explicitly supports them.

Report:

- evidence checked
- selected approach and rejected alternatives
- slice list with dependencies and chosen execution shape
- expected duration for each slice and whether it is a firm estimate or a
  conservative planning guess
- PR/landing policy source: `[workflow]` config, CLI/workflow override, or
  explicit user answer
- exact next command or target for `wt-start`
