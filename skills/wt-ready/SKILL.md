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
# tasks/, workflows/, ideas/ hold flat files; specs/ holds one directory per slug.
find "$common_dir/wt/tasks" "$common_dir/wt/workflows" "$common_dir/wt/ideas" -maxdepth 1 -type f 2>/dev/null | sort
find "$common_dir/wt/specs" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort
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

Use this rule for non-spec moments: clarifying execution shape, workflow policy,
terminology resolution, or evidence gaps. Ask only when the answer cannot be
discovered from the repo or a reasonable assumption would be risky. Ask one
focused question at a time and include your recommended answer.

Resolve terminology as you go. If the user uses a term that conflicts with the
repo docs or code, point to the conflict and propose the canonical term.

When you are authoring a spec file (`requirements.md`, `design.md`, `tasks.md`,
or `workflow.md`), follow the grill cycle in **Grill The Spec** instead. The
two rules apply to different moments and are not contradictory: this section
keeps non-spec questions terse; the grill cycle drives active per-file
challenge dialogue during spec authoring.

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

### Derive workflow mode from `tasks.md`

When a spec exists at `<git-common-dir>/wt/specs/<slug>/`, derive the execution
shape from `tasks.md`. Read the slice graph (dependencies, parallel groups,
shared base, lifecycle) and consult the canonical mapping below to pick a
workflow mode. Then record the choice and the reasoning in
`specs/<slug>/workflow.md` (see Spec Deliverables for authoring shape).

Canonical `tasks.md` → workflow mode mapping:

| `tasks.md` slice graph | Workflow mode |
|---|---|
| All sequential, single agent | `single` |
| All independent, same base | `batch` |
| Parent → child chain (each builds on previous branch) | `stack` |
| One task × multiple profiles | `matrix` |
| One direct slice only, OR mixed-lifecycle slices (e.g. wt task + direct local edit) | `none` |

Then act on the chosen mode:

- `single` / `batch` / `stack` / `matrix` — create the executable workflow TOML
  via `wt workflow task --mode <mode> ...`. The TOML lives at
  `<git-common-dir>/wt/workflows/<id>.toml` and is the runnable artifact.
  Record its path in `workflow.md` under "Linked workflow TOML".
- `none` — no workflow TOML is created. The slices launch as direct
  TaskDocuments via `wt run task <slug>` or as direct local edits outside the
  wt-managed repo. `workflow.md` may be very brief or omitted.

The workflow mode in the TOML must match the rationale in `workflow.md`. If
execution later drifts from the chosen mode, `wt-coordinate` updates
`workflow.md` rather than letting the TOML and the spec diverge silently
(same two-way sync rule as `design.md` / `tasks.md`).

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

## Spec Deliverables

Spec authoring is not one-shot drafting. Each file in `specs/<slug>/` is produced
through the grill cycle described in **Grill The Spec** below: draft → challenge
the user with file-specific questions → revise → confirm, then move to the next
file. The derivation procedure in **Choose Execution Shape > Derive workflow
mode from `tasks.md`** runs only after `tasks.md` is confirmed, so the grill
loops happen first.

Prepared wt work lives in three state directories under `<git-common-dir>/wt/`:

- `ideas/<slug>.{md,toml}` — kill-able exploration captured by `wt-idea`. Free-form
  Markdown or TOML. May be deleted at any time. No commitment.
- `specs/<slug>/` — committed prep artifact. Holds `requirements.md`, `design.md`,
  `tasks.md`, and optionally `workflow.md`. This is the canonical location for
  prep work that has been promoted past exploration.
- `tasks/<slug>.toml` — TaskDocument, the launch unit. Schema unchanged. The body
  may reference `specs/<slug>/` files by relative path.

The wt CLI does not read or write `specs/` directly. The spec is a human/AI
artifact. TaskDocument and TaskRun models are unchanged; no new wt commands are
involved in spec authoring.

### Promotion (idea → spec)

When `wt-ready` is invoked and the user commits to preparing the work, an
existing idea file is promoted, not copied:

- delete `<git-common-dir>/wt/ideas/<slug>.{md,toml}`
- create `<git-common-dir>/wt/specs/<slug>/` containing `requirements.md`,
  `design.md`, `tasks.md`, and optionally `workflow.md`

The directory move is the visible commit gate that distinguishes exploration
from committed prep. Work that the user requests directly, without a prior
idea, may go straight into `specs/<slug>/` without an idea file existing first.

### Authoring conventions

`requirements.md`:

- First line is the user story: `As a [role], I want [feature] so that [benefit]`.
- Functional requirements use EARS-style sentences:
  `WHEN <condition> THE SYSTEM SHALL <behavior>`.
- Compound triggers optionally use:
  `GIVEN <precondition> AND <precondition> WHEN <trigger> THE SYSTEM SHALL <response>`.
- Add a non-functional section for performance, security, compatibility, and
  similar concerns when they apply.
- Regression-sensitive behavior is stated explicitly:
  `WHEN <condition> THE SYSTEM SHALL CONTINUE TO <preserved behavior>`.

`design.md`:

- Capture decisions, affected components, and constraints.
- For brownfield work, optionally include a Static Model (Purpose, Components,
  Business Rules) and a Dynamic Model (workflow / behavior) section before the
  new design.
- Prefer intent and component responsibility over raw code dumps.

`tasks.md`:

- Checkbox items, sequenced as atomic units of work.
- Mark dependencies or parallelism explicitly so downstream steps can pick the
  right execution shape.

`workflow.md` (OPTIONAL, 4th file under `specs/<slug>/`):

- Prose record of the chosen execution shape and the reasoning derived from
  `tasks.md`. wt CLI does not read or write this file; it is for the human
  and the agent.
- Recommended sections:
  - **Chosen mode**: one of `single` / `batch` / `stack` / `matrix` / `none`.
  - **Why**: dependency analysis from `tasks.md` (sequential vs independent,
    shared base, lifecycle, parallel groups).
  - **Slices → TaskDocument mapping**: how `tasks.md` slices became one or
    more TaskDocuments (or direct local edits), with paths.
  - **Linked workflow TOML**: `<git-common-dir>/wt/workflows/<id>.toml` when
    applicable; `none` otherwise.
  - **Risks**: anything to watch when execution starts.
- When mode = `none`, `workflow.md` may be very brief (one paragraph plus the
  slice → TaskDocument mapping) or omitted entirely.
- The executable workflow is still the TOML at
  `<git-common-dir>/wt/workflows/<id>.toml`, created via
  `wt workflow task --mode ...`. `workflow.md` is prose only and never
  replaces the TOML.

Spec files are not frozen at handoff. `wt-coordinate` may update `design.md`,
`tasks.md`, and `workflow.md` in place during execution to reflect findings;
treat the spec as a living artifact that the running work writes back to. The
two-way sync rule applies to `workflow.md` the same way it applies to
`design.md` / `tasks.md` — when execution drifts from the chosen mode, update
`workflow.md` rather than silently changing the workflow TOML.

## Grill The Spec

Spec authoring is an active dialogue, not one-shot generation. For each file in
`specs/<slug>/`, run a draft → grill → revise → confirm loop with the user
before moving on. Drafts are working material; only the user's confirmation
makes a file authoritative.

This section absorbs the pattern from the `grill-with-docs` skill so a separate
`/grill-with-docs` invocation is not needed while `wt-ready` is the active
skill. The lineage matters: grill-style challenge dialogue is what produces a
spec that the rest of `wt-ready` can derive an execution shape from.

### Per-file cycle

1. **Draft** the file from the evidence gathered in **First Read** and **Gather
   Evidence**. Keep the draft small enough to challenge in one pass.
2. **Grill** the user with file-specific questions from the foci below. Ask one
   question at a time, include your recommended answer, and prefer to verify
   against the repo before asking when the answer is discoverable there.
3. **Revise** the draft inline based on the answer. Surface terminology
   conflicts against `docs/consistency.md` (or the project's CONTEXT.md when
   present) as they appear — ubiquitous-language drift is cheapest to fix here.
4. **Confirm** with the user that the file is settled before moving to the next
   file. Until that confirmation, the draft is not authoritative and downstream
   derivation (workflow mode, TaskDocument prep) must not run.

### File-specific grill foci

`requirements.md`:

- Edge cases the user story or EARS sentences silently skip (empty input,
  cancellation mid-flight, concurrent invocation, missing config).
- Non-functional concerns that apply but are unwritten: performance budgets,
  security posture, compatibility with prior `wt` releases, observability.
- Regression-sensitive behavior — what must explicitly `CONTINUE TO` work, and
  what evidence shows it currently works that way.
- Ambiguity inside EARS phrasing: does `WHEN` describe a trigger or a state?
  Is `THE SYSTEM` the CLI, the harness, or the agent?

`design.md`:

- Trade-offs and rejected alternatives. "Why not the simpler shape?" If no
  alternative was considered, that itself is the grill question.
- Conflicts with existing conventions — point at `docs/consistency.md` and
  `AGENTS.override.md` and ask whether this design honors or breaks them.
- Brownfield assumptions: which existing component is being extended vs
  replaced, and is the Static Model / Dynamic Model framing actually true
  against current code?
- Coupling and boundary questions: which module owns the new behavior, and
  does that match the file's stated component responsibility?

`tasks.md`:

- Slice granularity. Is a slice too coarse to review safely, or too fine to
  justify its own branch?
- Dependency vs parallel claims. "Does T2 really need T1's branch commits, or
  can they share the same base?" A stack claim must survive that question.
- Whether sequential ordering is intrinsic or just the order the conversation
  produced. If it is the latter, batch or separate workflows may fit better.
- Whether each slice is independently demoable, and what the acceptance check
  actually proves.

`workflow.md`:

- Mode-choice rationale. Walk the canonical mapping table from **Derive
  workflow mode from `tasks.md`** and ask which row this spec sits on.
- Alternatives considered. Could this be `batch` instead of `stack`? `matrix`
  for variant exploration? If the answer is "I didn't consider them", grill
  there before recording the choice.
- Whether `mode = none` is genuinely the right call (one direct slice, or a
  mixed-lifecycle mix) and not just an escape from picking.
- Risks to surface when execution starts, including dirty-worktree pitfalls
  and shared-base assumptions.

### Ubiquitous language

The grill is also the place to catch terminology drift. When the user uses a
term that already has a canonical definition in `docs/consistency.md` (or the
project glossary), call it out immediately and propose the canonical term —
mirror the grill-with-docs pattern. Resolving these now is cheaper than after
the TaskDocument is launched.



End with one of these concrete outputs:

- spec deliverables prepared (or promoted from `ideas/`) at
  `<git-common-dir>/wt/specs/<slug>/` with `requirements.md`, `design.md`,
  `tasks.md`, and optionally `workflow.md` recording the chosen execution shape
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
