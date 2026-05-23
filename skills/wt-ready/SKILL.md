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

If raw intent and unknowns are not yet surfaced (e.g., the user skipped
`wt-idea` and entered ready directly), run a brief Unknown Surfacing pass
before evidence gathering. Use the four categories from `wt-idea` — domain
concepts, standards/conventions, external facts, internal facts — and mark
each unknown `blocking now` or `useful later`. The list becomes the agenda
below.

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

When raw intent is still soft, use bounded context/reference exploration before
forcing purpose or requirements. Gather enough local or external examples to
name 2-4 plausible frames, record why each fits or fails, and then either:
continue to purpose/success criteria, return to `wt-idea`, or ask one HITL
question that chooses the next exploration direction.

## Work Sequence

Before promoting an idea, writing specs, or preparing TaskDocuments, identify
where the work currently sits in `references/work-sequence.md`. Do not treat
the sequence as a waterfall; use it as a set of gates that prevent skipping
from vague intent straight to runnable work.

If a user enters `wt-ready` directly with implementation-shaped wording,
reconstruct the missing raw intent, purpose/success criteria, and output
concept first. If the purpose, requirements/principles, output concept, design,
or task graph cannot be stated clearly, stop at the matching earlier artifact
instead of fabricating a TaskDocument.

## Questions

Use this rule for non-spec moments: clarifying execution shape, workflow policy,
terminology resolution, or evidence gaps. Ask only when the answer cannot be
discovered from the repo or a reasonable assumption would be risky. Ask one
focused question at a time and include your recommended answer.

Resolve terminology as you go. If the user uses a term that conflicts with the
repo docs or code, point to the conflict and propose the canonical term.

When authoring a spec file (`requirements.md`, `design.md`, `tasks.md`,
`workflow.md`), use the **Grill The Spec** cycle instead.

## Set Output Concept

After purpose, requirements, and principles are clear, decide what kind of
artifact this preparation should produce. Do this before design and task graph
work so an implementation PR is not assumed by default.

Record the output concept in the spec notes, TaskDocument `계획 (Planning)`
section, or `workflow.md` rationale:

- docs-only change
- implementation PR
- prototype or mockup
- spike / experiment
- direct local edit outside the wt-managed repo
- TaskDocument
- saved Workflow
- mixed-lifecycle handoff

If several output forms are plausible, split them or record the deferred form.
Do not bundle a prototype, docs cleanup, and implementation branch into one
task unless the dependency is real and review remains safe.

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

Record this planning context in the TaskDocument `body` as a text section, not
as top-level TOML fields (only canonical task fields are accepted). Every
TaskDocument or workflow task must include an expected duration in its
`계획 (Planning)` body section before `wt-start`. Prefer Korean human-facing
labels with the stable English key in parentheses.

Example body section:

```text
## 계획 (Planning)
- 유형 (type): AFK
- 예상 소요 (expected duration): 45m
- 막힘 / 의존성 (blocked by): workflow-policy-contract-simplified
- 실행 형태 (execution shape): stack child
- 크기 (size class): medium
- 확인 방법 (acceptance checks): update docs, run cargo fmt --all --check
```

Prefer several narrow slices over one broad task.

### Task and PR size budget

When deciding whether one slice is reviewable, consult
`references/task-pr-size-guidance.md`. Treat those thresholds as tripwires, not
hard blockers: a slice may be larger when it is mechanically coupled,
generated, deletion-heavy, or would leave the product in an invalid intermediate
state if split.

For each prepared slice, record the expected size class in the TaskDocument
body: `small`, `medium`, or `large-justified`. For `large-justified`, also
record why splitting would be worse and what checks or reviewer guidance reduce
the risk.

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

- `single` / `batch` / `stack` — create the workflow TOML via
  `wt workflow task --mode <mode> ...` at
  `<git-common-dir>/wt/workflows/<id>.toml`. Record its path in `workflow.md`
  under "Linked workflow TOML".
- `matrix` — create the workflow TOML via
  `wt workflow task --mode matrix <task> --profiles <profile-a>,<profile-b> ...`.
  Matrix mode requires exactly one local TaskDocument and explicit named
  profiles.
- `none` — no workflow TOML. Slices launch as direct TaskDocuments
  (`wt run task <slug>`) or as direct local edits outside the wt-managed repo.

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

## Spec Deliverables

### Prep eagerly across in-flight work

Prepare every identifiable in-flight item (ideas, partial specs, decisions
made in the current conversation) to launch-ready, rather than sequencing
prep behind execution. Specs are living artifacts — "the spec might change
later" is not a blocking dependency. Sequence prep only when a downstream
spec genuinely cannot be authored until an upstream has partially landed.
When asked "anything still not ready?", enumerate everything in-flight and
bring it all to ready unless the user explicitly defers a specific item.

Prepared wt work lives in three state directories under `<git-common-dir>/wt/`:

- `ideas/<slug>.{md,toml}` — kill-able exploration captured by `wt-idea`. Free-form
  Markdown or TOML. May be deleted at any time. No commitment.
- `specs/<slug>/` — committed prep artifact. Holds `requirements.md`, `design.md`,
  `tasks.md`, and optionally `workflow.md`. May also gain
  `mid-process-discoveries.md` during execution when `wt-coordinate` logs
  unplanned research (see that skill's Sync the Spec section). This is the
  canonical location for prep work that has been promoted past exploration.
- `tasks/<slug>.toml` — TaskDocument, the launch unit. Schema unchanged. The body
  may reference `specs/<slug>/` files by relative path.

The wt CLI does not read, parse, or manage `specs/` content. It can *seed* the
three core files via `wt scaffold <slug> --spec`; after that, spec authoring
stays a human/AI artifact. TaskDocument and TaskRun models are unchanged.

### Promotion (idea → spec)

When `wt-ready` is invoked and the user commits to preparing the work, an
existing idea file is promoted, not copied:

- `rm <git-common-dir>/wt/ideas/<slug>.{md,toml}` — the visible commit gate
  that distinguishes exploration from committed prep.
- `wt scaffold <slug> --spec` — seeds `requirements.md`, `design.md`, and
  `tasks.md`.
- If a mode decision is recorded at prep time, create `workflow.md` by hand.
  Scaffold intentionally does not make `workflow.md` — it is a decision
  artifact, not a skeleton.

The deletion plus spec directory creation is the visible commit gate that
distinguishes exploration from committed prep. Work that the user requests
directly, without a prior idea, may go straight into `specs/<slug>/` without an
idea file existing first.

### Authoring conventions

`requirements.md`:

- First line is the user story in Korean:
  `사용자 스토리: [역할]은 [이유/효과]를 위해 [기능/변화]를 원한다.`
- Include `목적 / 성공 기준` so the work states why it matters before
  describing behavior.
- Include `원칙 / 제약` when review, compatibility, UX, security,
  migration, or process rules should shape the output.
- Functional requirements use EARS-style sentences:
  `WHEN <조건> THE SYSTEM SHALL <관찰 가능한 동작>`.
- Compound triggers optionally use:
  `GIVEN <전제> AND <전제> WHEN <트리거> THE SYSTEM SHALL <응답>`.
- Add a non-functional section for performance, security, compatibility, and
  similar concerns when they apply.
- Regression-sensitive behavior is stated explicitly:
  `WHEN <조건> THE SYSTEM SHALL CONTINUE TO <보존할 동작>`.

`design.md`:

- Capture decisions, affected components, and constraints.
- For brownfield work, optionally include a Static Model (Purpose, Components,
  Business Rules) and a Dynamic Model (workflow / behavior) section before the
  new design.
- Prefer intent and component responsibility over raw code dumps.
- **Embed ASCII diagrams inside design.md** where the static model, dynamic
  model, or layered relationship with sibling specs would benefit from a
  structural view. Diagrams that live only in chat evaporate; the durable
  artifact is the spec file. At minimum, when the design has non-trivial
  structure, include diagrams for:
  - **Component layout** — modules / files / boundaries, marking what is new
    versus reused.
  - **Key flow paths** — happy path(s) and any critical race or ordering
    constraint.
  - **Layered or cross-spec relationships** — how this design depends on or is
    depended on by sibling specs (dependency direction, layer assignment).

`tasks.md`:

- Checkbox items, sequenced as atomic units of work.
- Mark dependencies or parallelism explicitly so downstream steps can pick the
  right execution shape.

`workflow.md` (OPTIONAL, 4th file under `specs/<slug>/`):

- Prose record of the chosen execution shape and the reasoning derived from
  `tasks.md`. wt CLI does not read or write this file; it is for the human
  and the agent.
- Recommended sections:
  - **선택한 모드**: one of `single` / `batch` / `stack` / `matrix` / `none`.
  - **이유**: dependency analysis from `tasks.md` (sequential vs independent,
    shared base, lifecycle, parallel groups).
  - **슬라이스 → TaskDocument 매핑**: how `tasks.md` slices became one or
    more TaskDocuments (or direct local edits), with paths.
  - **연결된 workflow TOML**: `<git-common-dir>/wt/workflows/<id>.toml` when
    applicable; `none` otherwise.
  - **리스크**: anything to watch when execution starts.
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

### Per-file cycle

1. **Draft** the file from the evidence gathered in **First Read** and **Gather
   Evidence**. Keep the draft small enough to challenge in one pass.
2. **Grill** the user with file-specific questions from the foci below. Ask one
   question at a time, include your recommended answer, and prefer to verify
   against the repo before asking when the answer is discoverable there.
3. **Revise** the draft inline based on the answer. Surface terminology
   conflicts against `docs/consistency.md` (or the project's CONTEXT.md when
   present) as they appear — ubiquitous-language drift is cheapest to fix here.
4. **Display for review** — surface what changed so the user can confirm.
   See `references/spec-review-surfaces.md` for the default summarize-don't-
   dump rule and the editor / cmux pane / `Ctrl+E` zero-token surfaces.
5. **Confirm** with the user that the file is settled before moving to the
   next file. Until that confirmation, the draft is not authoritative and
   downstream derivation (workflow mode, TaskDocument prep) must not run.

### File-specific grill foci

`requirements.md`:

- Whether the purpose/success criteria explain the desired effect rather than
  only naming an artifact to produce.
- Whether principles/constraints are specific enough to reject unsuitable
  designs or output forms.
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

When the user uses a term that conflicts with `docs/consistency.md` (or the
project glossary), call it out immediately and propose the canonical term.
This is part of step 3 (Revise), called out separately because terminology
drift is the cheapest defect to fix during grilling.



End with one of these concrete outputs:

- spec deliverables prepared (or promoted from `ideas/`) at
  `<git-common-dir>/wt/specs/<slug>/`, recording the chosen execution shape
- existing TaskDocuments/workflow ready, with the exact `wt-start` target
- new TaskDocument TOML files prepared
- a saved workflow prepared (mode, base, order, policy)
- a short list of unresolved HITL decisions that blocks launch

Use existing repo patterns for TaskDocument bodies. Avoid stale implementation
file paths unless they are necessary for the task.

Report:

- evidence checked
- selected approach and rejected alternatives
- output concept
- slice list with dependencies and chosen execution shape
- expected duration per slice (firm or conservative planning guess)
- PR/landing policy source: `[workflow]` config, CLI/workflow override, or
  explicit user answer
- exact next command or target for `wt-start`
