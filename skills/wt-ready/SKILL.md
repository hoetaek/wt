---
name: wt-ready
description: "Use before wt-work, or for vague future wt work, to capture exploratory ideas, gather evidence, promote committed work to specs, split slices, choose execution shape/policy, and prepare TaskDocuments or workflow handoff."
---

# WT Ready

Use this skill for wt preparation before execution. It has two valid stopping
points:

- Exploratory idea captured or updated under `planning/ideas/` when the user is
  not ready to commit to a spec, TaskDocument, or workflow.
- Launch-ready work for `wt-work`: scope clear, evidence gathered, slices
  ordered, and task/workflow handoff explicit.

Do not launch workspaces from this skill. Use `wt-work` after preparation and
while work is running, then `wt-land` after review passes.

## First Read

Inspect local truth before asking questions:

```bash
git status --short --branch
find . -maxdepth 2 -name AGENTS.md -o -name AGENTS.override.md
repo_root="$(git rev-parse --show-toplevel)"
# execution/tasks and execution/workflows hold flat files;
# planning/ideas and planning/specs hold one LEAF directory per slug.
find "$repo_root/.wt/execution/tasks" "$repo_root/.wt/execution/workflows" -maxdepth 1 -type f 2>/dev/null | sort
find "$repo_root/.wt/planning/ideas" "$repo_root/.wt/planning/specs" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort
# Flat idea files are legacy context, not canonical new targets.
find "$repo_root/.wt/planning/ideas" -maxdepth 1 -type f 2>/dev/null | sort
```

For `wt` itself, read `docs/consistency.md` before proposing model, CLI,
config, workflow, or state changes. Check current command help when behavior
matters; installed `wt` may differ from `./target/debug/wt`.

If the repo policy says the current branch is planning-only, limit direct work
there to reading, reference gathering, experiments, and TaskDocument/workflow
preparation. Implementation belongs in a wt task/workflow branch.

## Idea Boundary

An idea is exploration. It is allowed to die.

- Use `<repo-root>/.wt/planning/ideas/<slug>/` when the user is still exploring
  future work and should not commit to treating it as executable work yet.
- The idea uses the same LEAF structure as a spec, but it is still exploratory
  scratch surface, not a launch contract. It may be deleted, rewritten, split,
  or archived without any state transition that other components observe.
- No downstream consumer depends on an idea directory continuing to exist.
  Removing one is not breakage.
- If the user asks only to capture, compare, enrich, defer, or review ideas,
  stop at the idea directory. Do not create specs, TaskDocuments, workflows, or
  branches.

Capture enough that a later prep pass can continue without rediscovering the
basics:

- raw user wording, preserving phrasing when useful
- purpose and success criteria when visible
- local code/docs/state context and related ideas/tasks/workflows/specs
- possible frames or solution directions with tradeoffs
- assumptions, risks, non-goals, and open questions
- recommendation: enrich more, promote to spec, defer, archive, or split

Use these idea statuses in `00-status.md`:

- `captured`: raw idea saved with minimal context.
- `enriched`: meaningful context, references, or alternatives were gathered.
- `ready_for_prep`: enough information exists to promote into executable-work
  prep without rediscovering raw intent, plausible frames, tradeoffs, and the
  next question.
- `archived`: intentionally not pursuing now.

To seed an idea, run `wt scaffold <slug> --idea`. Use lowercase ASCII kebab-case
slugs. If `planning/ideas/<slug>/` already exists, update that directory instead
of creating a duplicate. If a legacy flat idea exists at
`planning/ideas/<slug>.{md,toml,markdown}`, normalize it into the LEAF directory
before continuing.

End an idea-only pass with the idea directory path, status, evidence checked, related
artifacts found, why it is or is not ready for prep, and the missing LEAF gate
when it is not ready.

## Gather Evidence

If intent and unknowns are not yet surfaced, run a brief Unknown Surfacing pass
before evidence gathering. Use four categories — domain concepts,
standards/conventions, external facts, internal facts (what you/the team already
hold but have not inventoried) — and mark each unknown `blocking now` or
`useful later`. The list becomes the agenda below. The pass surfaces unknowns,
but the same `02-unknowns.md` file also holds the positive ground — verified
facts, inventoried materials, prior decisions — so record what is already known,
not only what is missing.

Learn closes only when the user can judge the next choice, not merely when the
agent has gathered enough context. Carry Gate 2 from coming to know (domain
terms, conventions, comparable work, repo facts, internal materials) through to
being able to choose between plausible frames and state the basis for that
choice. Gate 3 Criteria consumes that user-held judgment.

Gate 2 experiments aim at the world or repo before an answer is built: "is this
true?" Record them as hypothesis -> test -> result in `01-Learn/02-unknowns.md`
and use them to verify facts, conventions, runtime behavior, or comparable
patterns. Do not use Gate 2 experiments to validate a proposed answer shape;
that belongs to Gate 4 Wireframe.

Work **inside-out**: ask the user direct clarifying questions and inventory
user/team-held materials (prior decisions, notes, related artifacts, contacts)
before reaching outward. Then check the conversation, issue references,
existing docs, current code, and current runtime behavior. If the user says
how something works, verify it against the repo when it is cheap.

Useful evidence:

- docs and glossary terms that define the domain model
- current config shape and local overrides such as `.wt.toml` / `<repo-root>/.wt/config/local.toml`
- current persisted state such as `<repo-root>/.wt/execution/tasks` and `<repo-root>/.wt/execution/workflows`
- command help for user-facing CLI contracts
- tests or small local experiments for uncertain world/repo behavior, recorded
  as hypothesis -> test -> result
- spec-local retrospectives and the cross-work timing baseline at
  `<repo-root>/.wt/planning/retrospectives/timing.md` for similar task type, size,
  agent/profile, and coordination shape
- `wt agent wait-stats` as a read-only summary of prior non-idle watch
  observations; use it as evidence for cadence and uncertainty, not as the
  source of actual task duration
- external references only when the user asks or the decision depends on current
  best practice outside the repo

Label items in your output as **verified fact** (with source — file:line,
URL, command output), **flagged assumption** (still to validate), or
**inventoried material** (user/team already holds it). Assumptions must not
ride along as facts into the next gate.

When intent is still soft, use bounded context/reference exploration before
forcing purpose or requirements. Gather enough local or external examples to
name 2-4 plausible frames, record why each fits or fails, and then either:
continue to purpose/success criteria, stop with/update an idea directory, or ask one
HITL question that chooses the next exploration direction.

## LEAF Work

Leaf before tree: validate one cheap, inspectable instance before growing it
into the whole artifact or runnable work. The core move is not to generate the
whole artifact upfront; it is to learn first, make one instance right, then
expand. Before promoting an idea, writing specs, or preparing TaskDocuments,
work through gates by phase and stop at the earliest missing LEAF gate:

| Phase | What it makes wt-ready able to do | Gates |
| --- | --- | --- |
| Learn | Judge what the work needs, learned rather than guessed | 1 Intent, 2 Unknowns & Context |
| Example | Prove one cheap instance right before scaling | 3 Criteria, 4 Wireframe |
| Architect | Generalize that instance into a shippable generator | 5 Design, 6 Critic, 7 Tasks, 8 Artifact / execution handoff |
| Feedback | Confirm it still holds and settle the lessons | 9 Review/sync, 10 Retrospect |

Learn asks whether the user has learned what this needs well enough to judge
it. Do not treat Learn as a private research phase where the agent silently
collects facts and then hands back criteria. The user's ability to name what to
choose between, and why, is the output that Example consumes.

Scaffolding is the first act for wt LEAF prep, so the work stands on a firm
foundation. Run or normalize `wt scaffold <slug> --idea` for exploratory prep
and `wt scaffold <slug> --spec` only after the user commits to treating the work
as executable. In both locations the scaffold creates `00-status.md` and the
four phase folders before any gate work, making "which gate am I in / what is
the first missing gate" inspectable before each gate file fills in. If the work
is too small for that body, keep it as a direct note or direct edit instead of
invoking LEAF.

Start from `00-status.md` when it exists. It is the project dashboard for
current phase/gate, first missing gate, next action, and progress; gate files
remain authoritative. Keep it current when a gate starts, becomes ready for
approval, is approved, returns to an earlier gate, is blocked/deferred, or when
the next action changes materially.

With the scaffold and `00-status.md` in hand, read `references/leaf-work.md`,
identify the current wt gate, tell the user that gate, and proceed by that
gate's wt-specific entry/exit/return conditions. Read it before judging gate
readiness, creating or revising a gate artifact, proposing a transition, or
handling a return. `SKILL.md` gives the operating shape;
`references/leaf-work.md` gives the wt pass/fail test. Skip it only to start a
small run that needs no gate judgment. Do not treat the sequence as a
waterfall. Gates loop: when a downstream gate overturns an assumption or
surfaces a new unknown, return to `01-Learn/02-unknowns.md`, update only the
later files that depended on what changed, and record the return in
`00-status.md` as a Return Log event, not as a gate state. For wt specs,
generic leaf-work's
`04-Feedback/10-retrospective/mid-process-discoveries.md` maps to wt's
deterministic `04-Feedback/10-retrospect.md`; do not create the generic nested
folder inside a wt spec. If execution/review evidence caused the return,
`wt-work` also records that discovery in `04-Feedback/09-review.md` for
review/sync evidence.

For wt specs, keep the `.wt/planning/specs/<slug>/` personal-state bucket and
store LEAF artifacts under phase folders inside it: `01-Learn/`,
`02-Example/`, `03-Architect/`, and `04-Feedback/`. The slug already names the
work item, so canonical wt files use stable names like `03-Architect/05-design.md`,
`03-Architect/08-execution.md`, and `04-Feedback/10-retrospect.md` instead of
leaf-work's generic artifact-suffixed forms. Gate 6 Critic is lazy: create
`03-Architect/06-critic.md` only when critic triggers fire, otherwise record
the accepted skip/low-risk rationale in `03-Architect/05-design.md`.

If a user enters with implementation-shaped wording, reconstruct the missing
intent, purpose/success criteria, and output form first. If Learn is incomplete,
surface unknowns, resolve enough context in the same file, and only then enter
Example.
If Example is incomplete, stop at criteria or wireframe instead of design.
If the Architect gates are incomplete, stop at design, critic, task graph, or
execution handoff instead of fabricating a TaskDocument.

Once the current one-sentence intent is available, show a compact opening
preview before deep prep. Phrase the four phases as the capability each builds
for this specific intent, not as generic labels or a fixed plan:

- Learn: by the end the user can judge what this specific work needs, having
  learned the facts, conventions, and alternatives instead of guessing them.
- Example: one cheap instance can be proven right before scaling it up.
- Architect: that passed case can be generalized into reusable structure, task
  order, and a shippable result.
- Feedback: the plan can be checked against review/sync evidence and the
  lessons can be carried forward.

Before Gate 2, run a lightweight topology confirmation when the request has
more than one possible outcome. Name the top-level outcomes, surfaces,
integrations, or deliverables that can succeed or fail independently, then ask
whether any should be added, removed, merged, split, or explicitly deferred.
Store the confirmed topology in the idea/spec note. Do not let the most
described component stand in for quieter sibling components.

Use the compact clarity ledger at its intended gate, not as a running
conversation-order checklist:

- intent: desired effect and core noun are stable
- topology: independent outcomes/components are named
- success: completion can be observed
- constraints: non-goals, boundaries, and preserved behavior are clear
- output form: idea, spec, TaskDocument, workflow, prototype, docs-only, or
  mixed handoff is explicit

Gate 1 locks the Intent row: desired effect and core noun. During Gate 2,
use the ledger only as a lens for learning: glance at the weakest row to aim
domain, standards/conventions, external, or internal unknowns and inventory,
but do not force the row closed there. Gate 3 scores and locks the full set:
Intent becomes purpose, and topology, success, constraints, and output form
become criteria, requirements, principles, or explicit assumptions/risks.

When choosing the next question, target the weakest ledger row and say why
that row is the current bottleneck before asking. If the core noun changes
across answers (`idea`, `spec`, `task`, `workflow`, `decision`, etc.), pause
feature questions and ask which noun is the actual object of the work and which
are supporting views or artifacts. A row is stable only when the user can judge
it in their own words; a verified fact held only by the agent is not yet a
stable row. Once a row is stable, stop re-asking it unless later evidence
changes it.

Every gate transition requires explicit user approval, including returns to an
earlier gate. The agent may propose that unknowns/context are adequate,
criteria are settled, a wireframe has passed, design is ready for tasking, or a
return is needed, but the user decides. The agent never unilaterally declares a
gate approved, moves forward, or returns to an earlier gate. For tiny wt work,
gate artifacts may be brief, but the Gate 3 -> Gate 4 and Gate 4 -> Gate 5 file
boundaries stay separate.

If the user wants to proceed past Gate 3 while any ledger row is still weak,
state the remaining risk and the cheapest next question or artifact that would
reduce it. Continue only after the user accepts that risk or chooses the next
gate.

The middle gates are a criteria -> instance -> generator chain. Gate 3 writes
the test before any answer exists, Gate 4 locks one concrete instance and its
contract, and Gate 5 generalizes that contract across valid variation. When
entering Gate 3, read the middle-engine material in `references/leaf-work.md`
for the wt mechanics: contract, variation points, the falsification loop, and
the gate return rules.

- Gate 3 Criteria is the arbiter plus test: write the intended effect and
  observable criteria before the answer exists.
- Gate 4 Wireframe is the instance plus contract: one concrete case with mock
  data, declared placeholder contracts, and explicit variation points.
- Gate 5 Design is the generator: it consumes the Gate 4 contract and defines
  behavior across the full variation range, including empty, overflow, edge,
  timing, and failure cases.

Never hide disagreement across a produce/consume edge.
`02-Example/03-criteria.md` and `02-Example/04-wireframe.md` stay separate;
`03+04`, `04+05`, and `03+04+05` are not canonical wt forms. If inherited work
still has pre-10-gate files such as `04+05-requirements.md`,
`04+05+06-requirements.md`, or `06-wireframe.md`, treat them as legacy/starter
context and split them into current ③ criteria and ④ wireframe artifacts before
launch-ready handoff. When Gate 5 has to invent an artifact shape, Gate 4
failed to lock the contract; return to Wireframe. When the Gate 4 instance and
Gate 3 criteria conflict, use Gate 3's purpose as the arbiter.

## Questions

Use this rule for non-spec moments: clarifying execution shape, workflow policy,
terminology resolution, or evidence gaps. Ask only when the answer cannot be
discovered from the repo or a reasonable assumption would be risky. Ask one
focused question at a time and include your recommended answer.

Resolve terminology as you go. If the user uses a term that conflicts with the
repo docs or code, point to the conflict and propose the canonical term.

When authoring a spec file (`01-Learn/02-unknowns.md`,
`02-Example/03-criteria.md`, `02-Example/04-wireframe.md`,
`03-Architect/05-design.md`, `03-Architect/06-critic.md`,
`03-Architect/07-tasks.md`, `03-Architect/08-execution.md`), use the
**Grill The Spec** cycle instead.

## Set Output Form

Lock the output form while authoring Gate 3 Criteria, once Gate 1 intent and
Gate 2 context give the user enough basis to judge it. Output form is one
clarity-ledger row and part of criteria, not a separate gate or a late design
choice. Do it before wireframe, design, and task graph work so an
implementation PR is not assumed by default.

Record the output form in `02-Example/03-criteria.md`, TaskDocument
`계획 (Planning)` section, or `03-Architect/08-execution.md` rationale:

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

## Wireframe With Mock Data

Before design, confirm the unknowns and context are sufficient to build a
realistic representative structure. If mock data, representative examples,
operator workflow, important states, or system constraints are missing, return
to `01-Learn/02-unknowns.md` instead of letting design discover the structure
late.

Run cheap iterations before expensive generalization. Gate 4 validates a
concrete case and locks the contract that case instantiates; Gate 5 consumes
that contract and generalizes it into reusable design rules.

Gate 4 experiments aim at the proposed answer: "is this answer right?" The
experiment is the concrete instance hitting Gate 3 criteria and the Gate 4
contract. If that instance falsifies a criterion or placeholder contract, keep
the disagreement visible. Return to `01-Learn/02-unknowns.md` when the failure
exposes a wrong or missing world/repo fact; revise criteria or wireframe using
Gate 3 purpose as the arbiter when the failure is about the intended effect or
answer shape.

Wireframe does not mean UI only. Use the artifact form that fits the output:

1. Group requirements into the pages, flows, states, commands, or document
   sections they will appear in. For UI/web work, write the representative user
   journey before drawing.
2. Start with a text-first wireframe: ASCII layout, command transcript,
   sequence sketch, table/state matrix, or outline with representative mock
   data. This pass must succeed before design or medium-specific wireframing.
3. If needed, add an artifact-specific wireframe:
   - UI / app flow: rough screens or HTML with realistic records, empty states,
     error states, and visual treatment for the concrete approved case. For
     **brownfield web changes** (editing an existing page), you can capture the
     real page with Chrome and edit only the changed regions instead of
     hand-drawing, then save a self-contained single file — see
     `references/brownfield-html-capture.md`. This is still a medium-specific
     pass: the text-first wireframe (step 2) comes first, and the captured real
     markup stays the locked context.
   - CLI / config: expected command transcript, generated TOML, and failure
     cases.
   - Workflow/process: mock TaskDocument, Workflow, TaskRun, pass/land path,
     and coordinator handoff.
   - Docs/report: outline with placeholder evidence, claims, and reader path.
   - API/data: representative request/response examples and state table.

Every placeholder or mock value hides a contract. Gate 4 therefore produces
two paired outputs: the concrete instance being walked through, and the
contract that says what each placeholder commits to. For each placeholder,
name the contract it instantiates and the variation point it leaves open:
what can vary, along which axis, and within what range. Unaccounted
placeholders mean the wireframe validated only one example, not the reusable
contract that design can safely consume.

Write `02-Example/04-wireframe.md` for one compact artifact or
`02-Example/04-wireframe/` when there are several screens, flows, examples, or
transcripts. Do not merge Gate 3's criteria with Gate 4's answer. If inherited
work still keeps wireframe material in `04+05+06-requirements.md` or
`04+05-requirements.md`, split the concrete instance, contracts, variation
points, and walkthrough result into `02-Example/04-wireframe.md` before design
consumes it.

The gate passes only when the user can walk through the text-first structure and
confirm that it fits. If an artifact-specific wireframe is needed, that concrete
case must also pass before `03-Architect/05-design.md` generalizes it into component
boundaries, state model, command/config shape, data contracts, interaction
rules, or visual system rules.

Loops are expected. If the visual mockup exposes missing data or context, return
to Learn. If it exposes missing behavior, return to criteria. If the approved
instance and a requirement conflict, use Gate 3 purpose / success criteria as
the arbiter: requirements are proxies for purpose, not the final authority. Fix
whichever one fails the purpose, then resume. If design cannot generalize the
case without inventing rules, return to Gate 4 and add a better
concrete case.

For user-facing, ambiguous, or high-risk flows, add a cold reader check. Show
only the wireframe, mock data, labels, and visible sequence to a blind reader.
If they infer the wrong actor, outcome, next action, or important state, revise
the wireframe before design.

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
  `2h`; derive this from similar retrospectives when available, otherwise mark
  it as a conservative planning guess or range
- estimate basis: previous `04-Feedback/10-retrospect.md`, cross-work `timing.md`,
  `wt agent wait-stats`, user-provided target, or conservative planning guess
- suggested watch cadence: launch validation and steady heartbeat interval for
  `wt-work`, based on expected duration and prior timing evidence
- acceptance checks
- notes for experiments or tradeoffs that shaped the slice

Record this planning context in the TaskDocument `body` `## 계획 (Planning)`
section, not as top-level TOML fields (only canonical task fields are
accepted). Every TaskDocument or workflow task must include an expected
duration there before `wt-work`, plus the estimate basis when known. The
section format and field labels are defined by the wt-writing-tasks skill.

Prefer several narrow slices over one broad task.

### TaskDocument body authoring

**REQUIRED SUB-SKILL:** Use wt-writing-tasks when authoring or revising any
TaskDocument body. It owns the body structure (계획 / 필수 준수 / 맥락 / 작업),
the hard-constraint top-of-body placement rule and its retrospect evidence,
implementation-grade task steps with complete failing tests and implementation
contracts, the no-placeholder rule, and the pre-handoff self-review.

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

### Derive workflow mode from `03-Architect/07-tasks.md`

When a spec exists at `<repo-root>/.wt/planning/specs/<slug>/`, derive the
execution shape from `03-Architect/07-tasks.md`. Read the slice graph
(dependencies, parallel groups, shared base, lifecycle) and consult the
canonical mapping below to pick a workflow mode. Then record the choice and the
reasoning in `planning/specs/<slug>/03-Architect/08-execution.md` (see Spec
Deliverables for authoring shape).

Canonical `03-Architect/07-tasks.md` → workflow mode mapping:

| `03-Architect/07-tasks.md` slice graph | Workflow mode |
|---|---|
| All sequential, single agent | `single` |
| All independent, same base | `batch` |
| Parent → child chain (each builds on previous branch) | `stack` |
| One task × multiple profiles | `matrix` |
| One direct slice only, OR mixed-lifecycle slices (e.g. wt task + direct local edit) | `none` |

Then act on the chosen mode:

- `single` / `batch` / `stack` — create the workflow TOML via
  `wt workflow task --mode <mode> ...` at
  `<repo-root>/.wt/execution/workflows/<id>.toml`. Record its path in `03-Architect/08-execution.md`
  under "Linked workflow TOML".
- `matrix` — create the workflow TOML via
  `wt workflow task --mode matrix <task> --profiles <profile-a>,<profile-b> ...`.
  Matrix mode requires exactly one local TaskDocument and explicit named
  profiles.
- `none` — no workflow TOML. Slices launch as direct TaskDocuments
  (`wt run task <slug>`) or as direct local edits outside the wt-managed repo.

## Workflow Policy

Treat `.wt.toml` / `<repo-root>/.wt/config/local.toml` `[workflow]` as workflow preparation
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

Generic leaf-work projects store persistent files under phase folders
(`01-Learn/`, `02-Example/`, `03-Architect/`, `04-Feedback/`). wt keeps those
phase folders inside its `.wt` personal-state buckets instead of creating
repo-root leaf-work folders.

Do not create nested leaf-work folders inside a wt spec. If a work item cannot
fit as a single reviewable line in `03-Architect/07-tasks.md`, split it into
several slices there. If it truly needs its own LEAF cycle, create a separate
sibling spec folder under `planning/specs/` and reference that spec from the
task graph. External non-text artifacts such as code, video, design, music, or
physical outputs live outside the spec folder; record the reference and
handoff in `03-Architect/08-execution.md` instead of storing the artifact as
LEAF process material.

Prepared wt work uses three canonical locations under the planning/execution
buckets:

- `planning/ideas/<slug>/` — kill-able LEAF exploration captured by `wt-ready`.
  Holds the same numbered LEAF files as a spec, but may be deleted, rewritten,
  split, or archived at any time. No downstream consumer may depend on it.
- `planning/specs/<slug>/` — executable-work baseline prep artifact. Holds
  numbered LEAF files:
  `00-status.md`, `01-Learn/01-intent.md`, `01-Learn/02-unknowns.md`,
  `01-Learn/02-references/` (always scaffolded as a holding slot),
  `02-Example/03-criteria.md`,
  `02-Example/04-wireframe.md` / `02-Example/04-wireframe/`,
  `03-Architect/05-design.md`, lazy `03-Architect/06-critic.md`,
  `03-Architect/07-tasks.md`, lazy `03-Architect/08-execution.md`, lazy
  `04-Feedback/09-review.md`, and lazy `04-Feedback/10-retrospect.md`.
  This is the canonical location for work promoted past exploration into an
  executable-work baseline and for spec-backed review/retrospect records.
- `execution/tasks/<slug>.toml` — TaskDocument, the launch unit. Schema
  unchanged. The body may reference `planning/specs/<slug>/` files by relative
  path.

The wt CLI does not parse or manage `planning/ideas/` or `planning/specs/` as
executable state. `wt scaffold <slug> --idea` and `wt scaffold <slug> --spec`
may seed starter files depending on the installed wt version; if they create
pre-10-gate files such as `03-context.md`,
`04+05-requirements.md`, `06-wireframe.md`, `07-design.md`, `08-tasks.md`,
or the old 9-gate wt files `03-Architect/06-tasks.md`,
`03-Architect/07-execution.md`, `04-Feedback/08-review.md`, and
`04-Feedback/09-retrospect.md`, normalize them into the current layout before
launch-ready handoff. Spec authoring stays a human/AI artifact. TaskDocument
and TaskRun models are unchanged.

### Promotion (idea → spec)

When `wt-ready` is invoked and the user commits to treating the work as
executable, an existing idea directory is promoted, not copied:

- Move `<repo-root>/.wt/planning/ideas/<slug>/` into
  `<repo-root>/.wt/planning/specs/<slug>/` — the visible commit gate that
  distinguishes exploration from executable-work baseline.
- Preserve and update the LEAF files: `00-status.md`, `01-Learn/01-intent.md`,
  `01-Learn/02-unknowns.md`, `02-Example/03-criteria.md`,
  `02-Example/04-wireframe.md`, `03-Architect/05-design.md`, and
  `03-Architect/07-tasks.md`.
- Record in `01-Learn/01-intent.md` that the spec was promoted from
  `planning/ideas/<slug>/`; if a legacy flat idea was normalized, record that
  legacy source too.
- If a mode decision is recorded at prep time, create `03-Architect/08-execution.md`
  by hand. Treat it as a decision and handoff artifact, not a blank prep
  skeleton.

The directory move plus spec-location update is the visible commit gate that
distinguishes exploration from executable-work prep. Work that the user requests
directly, without a prior idea, may go straight into `planning/specs/<slug>/`
without an idea directory existing first.

### Authoring conventions

`01-Learn/01-intent.md`:

- Preserve the user's raw wording and the coordinator's interpreted intent as
  separate text.
- Record whether this spec was promoted from an idea directory or entered prep
  directly.

`00-status.md`:

- Keep the dashboard current: current phase/gate, first missing gate, next
  action, latest return, return count, and per-gate progress.
- Use progress values `0`, `25`, `50`, `75`, `100` and states `not-started`,
  `active`, `needs-approval`, `approved`.
- Record returns in a Return Log. Do not create a `returned` gate state.

`01-Learn/02-unknowns.md`:

- Group unknowns by domain concepts, standards/conventions, external facts,
  and internal facts.
- Mark each unknown `blocking now` or `useful later`; blocking unknowns drive
  evidence gathering.
- Record both sides: unknowns (domain / standards / external / internal) and the
  positive ground the template seeds as headings — verified facts, inventoried
  materials, flagged assumptions, references/options/tradeoffs. Resolve each
  unknown in place.
- `01-Learn/02-references/` is always scaffolded as a holding slot; put bulky
  source material there and summarize the useful answer back in `02-unknowns.md`.
- Do not record final design decisions here unless the decision has already
  been approved downstream.

`02-Example/03-criteria.md`:

- First line is the user story in Korean:
  `사용자 스토리: [역할]은 [이유/효과]를 위해 [기능/변화]를 원한다.`
- Include `목적 / 성공 기준` so the work states why it matters before
  describing behavior.
- Include the output form: docs-only change, implementation PR, prototype,
  spike, direct local edit, TaskDocument, saved Workflow, or mixed-lifecycle
  handoff.
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
- If inherited work still has `04+05-requirements.md` or
  `04+05+06-requirements.md`, treat it as pre-10-gate legacy context. Move
  purpose/requirements into `02-Example/03-criteria.md` and Gate 4 material into
  `02-Example/04-wireframe.md` before launch-ready handoff.

`02-Example/04-wireframe.md` or `02-Example/04-wireframe/`:

- Validate a concrete case before design. Start by grouping requirements into
  pages, flows, states, commands, or document sections, then create a
  text-first wireframe with realistic mock data or representative examples.
- Record the paired Gate 4 outputs: the concrete instance and the contract it
  instantiates. For every placeholder or mock value, name the contract it must
  obey and the variation point it leaves open: axis, range, and limits.
- Record the context adequacy check: which unknowns were resolved, which facts
  or examples from `01-Learn/02-unknowns.md` support the structure, and which
  states are intentionally deferred.
- After the text-first pass, add an artifact-specific form when needed: HTML for
  web, command transcript, generated TOML, TaskDocument/workflow flow, outline
  with placeholder evidence, API examples, or state table. For UI/web work, this
  can include the concrete visual treatment to approve as a case.
- Include important empty/error/edge/loading/conflict states when they affect
  structure.
- Record the user/operator walkthrough result for the text-first pass, and for
  the artifact-specific pass when one exists, before design starts.
- For user-facing, ambiguous, or high-risk flows, record what a blind reader
  inferred from the wireframe alone, plus any mismatch against requirements.

`03-Architect/05-design.md`:

- Start from the passed wireframe case or explicitly note the brief Gate 4
  artifact that was accepted for tiny work.
- Consume the Gate 4 contract, placeholder decisions, and variation points as
  inputs. Design must not rediscover the artifact shape that the wireframe was
  supposed to lock.
- Turn the passed concrete case into reusable decisions, affected
  components, state and data contracts, interaction rules, responsive rules,
  visual system rules when relevant, and constraints.
- Include the Gate 5 RALPLAN-DR sections as durable design rationale:
  - **원칙 (Principles)**: 3-5 design rules this choice must respect.
  - **결정 동인 (Decision drivers)**: top 3 forces that selected the option.
  - **선택지 (Viable options)**: at least two real options with bounded pros/cons
    when they exist; otherwise record why rejected alternatives are invalid.
  - **반대 논거 (Steelman antithesis)**: the strongest argument against the
    chosen option, with the answer.
  These are artifact-quality rules, not an automatic Planner/Architect/Critic
  loop.
- For brownfield work, optionally include a Static Model (Purpose, Components,
  Business Rules) and a Dynamic Model (workflow / behavior) section before the
  new design.
- Prefer intent and component responsibility over raw code dumps.
- **Embed ASCII diagrams inside `03-Architect/05-design.md`** where the static model, dynamic
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

`03-Architect/06-critic.md` (LAZY, only when critic triggers fire):

- Record the reviewer surface: user, human reviewer, another agent, or subagent.
- Use verdicts from `references/design-critic.md`: `APPROVE`, `ITERATE`, or
  `REJECT`.
- For `ITERATE` or `REJECT`, name the smallest design revision needed before
  Gate 7 Tasks can start.
- If critic triggers are considered but skipped, record that accepted
  low-risk/skip rationale in `03-Architect/05-design.md` instead of creating a
  blank critic file.

`03-Architect/07-tasks.md`:

- Checkbox items, sequenced as atomic units of work.
- Mark dependencies or parallelism explicitly so downstream steps can pick the
  right execution shape.
- Keep this file at slice-graph level: titles, dependencies, parallel groups,
  and TaskDocument paths. Implementation-grade steps live in each TaskDocument
  body (wt-writing-tasks); do not duplicate them here.

`03-Architect/08-execution.md` (LAZY, only when launch handoff exists):

- Prose record of the chosen execution shape and the reasoning derived from
  `03-Architect/07-tasks.md`. wt CLI does not read or write this file; it is for
  the human and the agent.
- Recommended sections:
  - **선택한 모드**: one of `single` / `batch` / `stack` / `matrix` / `none`.
  - **이유**: dependency analysis from `03-Architect/07-tasks.md` (sequential vs
    independent, shared base, lifecycle, parallel groups).
  - **슬라이스 → TaskDocument 매핑**: how `03-Architect/07-tasks.md` slices became
    one or more TaskDocuments (or direct local edits), with paths.
  - **연결된 workflow TOML**: `<repo-root>/.wt/execution/workflows/<id>.toml` when
    applicable; `none` otherwise.
  - **wt-work target**: exact command or target for execution launch.
  - **시간 가정 / watch cadence**: expected duration, estimate basis, launch
    validation cadence, and steady watch cadence to hand to `wt-work`.
  - **리스크**: anything to watch when execution starts.
- When mode = `none`, `03-Architect/08-execution.md` may be very brief (one
  paragraph plus the slice → TaskDocument mapping) or omitted entirely.
- The executable workflow is still the TOML at
  `<repo-root>/.wt/execution/workflows/<id>.toml`, created via
  `wt workflow task --mode ...`. `03-Architect/08-execution.md` is prose only
  and never replaces the TOML.

Spec files are not frozen at handoff. `wt-work` may update
`03-Architect/05-design.md`, `03-Architect/07-tasks.md`,
`03-Architect/08-execution.md`, and `04-Feedback/09-review.md` in place during
execution to reflect findings; treat the spec as a living artifact that the
running work writes back to. The two-way sync rule applies to
`03-Architect/08-execution.md` the same way it applies to
`03-Architect/05-design.md` / `03-Architect/07-tasks.md`: when execution drifts
from the chosen mode, update `03-Architect/08-execution.md` rather than silently
changing the workflow TOML.

## Grill The Spec

Spec authoring is an active dialogue, not one-shot generation. For each file in
`planning/specs/<slug>/`, run a draft → grill → revise → confirm loop with the user
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

`01-Learn/02-unknowns.md`:

- Whether the clarity ledger was used only as a lens to aim learning, instead
  of forcing decisions that belong in Gate 3 Criteria.
- Whether the weakest row is visible enough that vague intent is not hidden
  inside later requirements.
- Whether unknowns are grouped by domain, standards/conventions, external, and
  internal categories.
- Whether each blocking unknown has a verified answer, explicit assumption,
  owner/user question, or a reason it is deferred.
- Whether context, references, options, and tradeoffs are summarized next to
  the unknown they answer rather than split into a stale second file.
- Whether bulky reference material belongs in `01-Learn/02-references/`.

`02-Example/03-criteria.md`:

- Whether Gate 3 scores and locks the clarity-ledger rows: intent as purpose,
  and topology, success, constraints, and output form as requirements,
  principles, acceptance checks, or explicit assumptions/risks.
- Whether the purpose/success criteria explain the desired effect rather than
  only naming an artifact to produce.
- Whether principles/constraints are specific enough to reject unsuitable
  designs or output forms.
- Whether the output form belongs here, or whether the work should split into
  separate docs/prototype/implementation/workflow artifacts.
- Edge cases the user story or EARS sentences silently skip (empty input,
  cancellation mid-flight, concurrent invocation, missing config).
- Non-functional concerns that apply but are unwritten: performance budgets,
  security posture, compatibility with prior `wt` releases, observability.
- Regression-sensitive behavior — what must explicitly `CONTINUE TO` work, and
  what evidence shows it currently works that way.
- Ambiguity inside EARS phrasing: does `WHEN` describe a trigger or a state?
  Is `THE SYSTEM` the CLI, the harness, or the agent?

`02-Example/04-wireframe.md` / `02-Example/04-wireframe/`:

- Whether unknowns and context are sufficient to create realistic mock data.
  If not, return to `01-Learn/02-unknowns.md` before design.
- Whether requirements are grouped into concrete pages, flows, states,
  commands, or document sections before drawing.
- Whether the text-first wireframe uses representative data, examples, states,
  or command transcripts instead of abstract placeholders.
- Whether every placeholder or mock value has a named contract and variation
  point, or is explicitly resolved into a real constraint before design.
- Whether the intended user/operator can walk through the flow and find the
  expected outcome.
- Whether a blind reader, seeing only the wireframe and mock data, can infer the
  actor, purpose, expected outcome, next action, and important states.
- Whether an artifact-specific wireframe is needed after text-first approval
  (for example HTML for web, generated TOML for config, or API
  request/response examples).
- For UI/web work, whether the visual treatment is sufficient to judge the
  concrete case before generalizing a visual system in `03-Architect/05-design.md`.
- For brownfield web captures (see `references/brownfield-html-capture.md`),
  whether the saved artifact is self-contained (assets inlined, offline-reload
  checked) and which stack limits were left as deferred variation points.
- Which empty/error/edge/loading/conflict states change structure and therefore
  must appear before design.
- Whether the wireframe reveals missing requirements or wrong assumptions.
- When the wireframe conflicts with a requirement, whether Gate 3 purpose /
  success criteria shows the requirement should change, the instance should
  change, or the work should return to Learn.

`03-Architect/05-design.md`:

- Whether it builds on a passed wireframe instead of doing hidden wireframe
  discovery inside design.
- Whether it consumes the Gate 4 contract and variation points instead of
  inventing new placeholder meaning or artifact shape.
- Whether it generalizes the approved concrete case instead of treating one
  mock screen, example, or happy path as the whole system.
- Trade-offs and rejected alternatives. "Why not the simpler shape?" If no
  alternative was considered, that itself is the grill question.
- Conflicts with existing conventions — point at `docs/consistency.md` and
  `AGENTS.override.md` and ask whether this design honors or breaks them.
- Brownfield assumptions: which existing component is being extended vs
  replaced, and is the Static Model / Dynamic Model framing actually true
  against current code?
- Coupling and boundary questions: which module owns the new behavior, and
  does that match the file's stated component responsibility?
- Scale and variation questions: how the design handles larger data, responsive
  breakpoints, empty/error/conflict states, and cases not present in the
  approved wireframe.
- Whether principles, drivers, options, and steelman antithesis line up. A
  design that names drivers but chooses an option for unrelated reasons should
  return to revision.
- For high-risk or non-obvious designs, whether to request a critic pass using
  `references/design-critic.md`. The reviewer can be a human, another agent, or
  a subagent; do not create an automatic consensus loop. Treat security,
  migration, public CLI/config breakage, data loss, irreversible operations, and
  wide cross-module changes as critic-pass triggers unless the user explicitly
  accepts skipping review.

`03-Architect/06-critic.md`:

- Whether the critic trigger is real enough to require a durable Gate 6 file,
  or whether the accepted skip/low-risk rationale belongs in design.
- Whether the verdict is one of `APPROVE`, `ITERATE`, or `REJECT`.
- Whether required revisions are small and specific enough to route back to
  Gate 5 Design instead of drifting into implementation planning.
- Whether residual risks are explicit enough for Gate 7 Tasks to carry into
  acceptance checks.

`03-Architect/07-tasks.md`:

- Slice granularity. Is a slice too coarse to review safely, or too fine to
  justify its own branch?
- Dependency vs parallel claims. "Does T2 really need T1's branch commits, or
  can they share the same base?" A stack claim must survive that question.
- Whether sequential ordering is intrinsic or just the order the conversation
  produced. If it is the latter, batch or separate workflows may fit better.
- Whether each slice is independently demoable, and what the acceptance check
  actually proves.

`03-Architect/08-execution.md`:

- Mode-choice rationale. Walk the canonical mapping table from **Derive
  workflow mode from `03-Architect/07-tasks.md`** and ask which row this spec
  sits on.
- Alternatives considered. Could this be `batch` instead of `stack`? `matrix`
  for variant exploration? If the answer is "I didn't consider them", grill
  there before recording the choice.
- Whether `mode = none` is genuinely the right call (one direct slice, or a
  mixed-lifecycle mix) and not just an escape from picking.
- Concrete execution signal: at least one file path, module/symbol, issue/task
  id, acceptance criteria, numbered implementation step, command/config
  transcript, representative example/mock data, named output artifact, or
  user-accepted residual risk must be present before handoff.
- Risks to surface when execution starts, including dirty-worktree pitfalls
  and shared-base assumptions.

### Ubiquitous language

When the user uses a term that conflicts with `docs/consistency.md` (or the
project glossary), call it out immediately and propose the canonical term.
This is part of step 3 (Revise), called out separately because terminology
drift is the cheapest defect to fix during grilling.

End with one of these concrete outputs:

- idea captured/updated at `<repo-root>/.wt/planning/ideas/<slug>/`
  when the user is still exploring
- spec deliverables prepared (or promoted from `planning/ideas/`) at
  `<repo-root>/.wt/planning/specs/<slug>/`, recording the chosen execution shape
- existing TaskDocuments/workflow ready, with the exact `wt-work` target
- new TaskDocument TOML files prepared
- a saved workflow prepared (mode, base, order, policy)
- a short list of unresolved HITL decisions that blocks launch

Author TaskDocument bodies with the wt-writing-tasks skill. Avoid stale
implementation file paths; verify every referenced path and symbol against the
repo before handoff.

Report:

- the opening LEAF capability preview after a current one-sentence intent is
  available
- current phase/gate and the first missing gate, if any
- why the next move belongs to Learn, Example, Architect, or Feedback
- evidence checked
- open questions or accepted risks that block or shape the next gate
- selected approach and rejected alternatives
- proposed next artifact to create or revise
- output form
- wireframe/context adequacy status, including mock data or representative
  examples used
- slice list with dependencies and chosen execution shape
- expected duration per slice, with estimate basis and suggested watch cadence
- review checks that prove the next pass is useful
- PR/landing policy source: `[workflow]` config, CLI/workflow override, or
  explicit user answer
- exact next command or target for `wt-work`
