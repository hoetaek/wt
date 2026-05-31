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
# execution/tasks, execution/workflows, planning/ideas hold flat files;
# planning/specs holds one directory per slug.
find "$repo_root/.wt/execution/tasks" "$repo_root/.wt/execution/workflows" "$repo_root/.wt/planning/ideas" -maxdepth 1 -type f 2>/dev/null | sort
find "$repo_root/.wt/planning/specs" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort
```

For `wt` itself, read `docs/consistency.md` before proposing model, CLI,
config, workflow, or state changes. Check current command help when behavior
matters; installed `wt` may differ from `./target/debug/wt`.

If the repo policy says the current branch is planning-only, limit direct work
there to reading, reference gathering, experiments, and TaskDocument/workflow
preparation. Implementation belongs in a wt task/workflow branch.

## Idea Boundary

An idea is exploration. It is allowed to die.

- Use `<repo-root>/.wt/planning/ideas/<slug>.{md,toml}` when the user is still
  exploring future work and should not commit to a spec, TaskDocument, or
  workflow yet.
- The idea body is scratch surface, not a contract. It may be deleted,
  rewritten, split, or archived without any state transition that other
  components observe.
- No downstream consumer depends on an idea file continuing to exist. Removing
  one is not breakage.
- If the user asks only to capture, compare, enrich, defer, or review ideas,
  stop at the idea file. Do not create specs, TaskDocuments, workflows, or
  branches.

Capture enough that a later prep pass can continue without rediscovering the
basics:

- raw user wording, preserving phrasing when useful
- purpose and success criteria when visible
- local code/docs/state context and related ideas/tasks/workflows/specs
- possible frames or solution directions with tradeoffs
- assumptions, risks, non-goals, and open questions
- recommendation: enrich more, promote to spec, defer, archive, or split

Use these idea statuses when writing TOML or a status line in Markdown:

- `captured`: raw idea saved with minimal context.
- `enriched`: meaningful context, references, or alternatives were gathered.
- `ready_for_prep`: enough information exists to promote into spec prep without
  rediscovering raw intent, plausible frames, tradeoffs, and the next question.
- `archived`: intentionally not pursuing now.

To seed a Markdown idea, run `wt scaffold <slug> --idea`. Use lowercase ASCII
kebab-case slugs. Prefer Markdown for loose notes; use TOML only when simple
top-level fields plus a `body` string help. If an idea already exists at either
extension, update that file instead of creating a duplicate.

End an idea-only pass with the idea path, status, evidence checked, related
artifacts found, why it is or is not ready for prep, and the missing LEAF gate
when it is not ready.

## Gather Evidence

If intent and unknowns are not yet surfaced, run a brief Unknown Surfacing pass
before evidence gathering. Use four categories — domain concepts,
standards/conventions, external facts, internal facts — and mark each unknown
`blocking now` or `useful later`. The list becomes the agenda below.

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
- tests or small local experiments for uncertain behavior
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
continue to purpose/success criteria, stop with/update an idea file, or ask one
HITL question that chooses the next exploration direction.

## LEAF Work

Leaf before tree: validate one cheap, inspectable instance before growing it
into the whole artifact or runnable work. Before promoting an idea, writing
specs, or preparing TaskDocuments, identify the earliest missing LEAF gate:

- Learn: 1 Intent, 2 Unknowns & Context
- Example: 3 Criteria, 4 Wireframe
- Architect: 5 Design, 5.5 optional Critic, 6 Tasks, 7 Execution handoff
- Feedback: 8 Review/sync, 9 Retrospect

Use `references/leaf-work.md` for wt-specific gate details and artifacts.
Do not treat the sequence as a waterfall. Gates loop: when a downstream gate
overturns an assumption or surfaces a new unknown, return to
`01-Learn/02-unknowns.md`, update the affected later files, and record the
discovery in `04-Feedback/08-review.md` when execution/review evidence caused
it.

For wt specs, keep the `.wt/planning/specs/<slug>/` personal-state bucket and
store LEAF artifacts under phase folders inside it: `01-Learn/`,
`02-Example/`, `03-Architect/`, and `04-Feedback/`. The slug already names the
work item, so canonical wt files use stable names like `03-Architect/05-design.md`
and `04-Feedback/09-retrospect.md` instead of leaf-work's generic
artifact-suffixed forms.

If a user enters with implementation-shaped wording, reconstruct the missing
intent, purpose/success criteria, and output form first. If Learn is incomplete,
surface unknowns, resolve enough context in the same file, and only then enter
Example.
If Example is incomplete, stop at criteria or wireframe instead of design.
If the Architect gates are incomplete, stop at design, task graph, or execution
handoff instead of fabricating a TaskDocument.

Once the current one-sentence intent is available, show a compact LEAF route
preview before deep prep. Phrase each phase as a question about this specific
intent: what to learn in Learn, what cheap example to validate in Example, what
design/tasks/handoff to architect in Architect, and what to review or learn in
Feedback. This is orientation, not a fixed plan.

Before Gate 2, run a lightweight topology confirmation when the request has
more than one possible outcome. Name the top-level outcomes, surfaces,
integrations, or deliverables that can succeed or fail independently, then ask
whether any should be added, removed, merged, split, or explicitly deferred.
Store the confirmed topology in the idea/spec note. Do not let the most
described component stand in for quieter sibling components.

During Gates 1-3, keep a compact clarity ledger instead of asking questions in
conversation order:

- intent: desired effect and core noun are stable
- topology: independent outcomes/components are named
- success: completion can be observed
- constraints: non-goals, boundaries, and preserved behavior are clear
- output form: idea, spec, TaskDocument, workflow, prototype, docs-only, or
  mixed handoff is explicit

Target the weakest ledger row with the next question. Say why that row is the
current bottleneck before asking. If the core noun changes across answers
(`idea`, `spec`, `task`, `workflow`, `decision`, etc.), pause feature questions
and ask which noun is the actual object of the work and which are supporting
views or artifacts.

Gate transitions require explicit user approval. The agent may propose that
unknowns/context are adequate, criteria are settled, a wireframe has passed, or
design is ready for tasking, but the user decides when
to move to the next gate. For tiny wt work, gate artifacts may be brief, but the
Gate 3 -> Gate 4 and Gate 4 -> Gate 5 file boundaries stay separate.

If the user wants to proceed while any Gate 1-3 ledger row is still weak, state
the remaining risk and the cheapest next question or artifact that would reduce
it. Continue only after the user accepts that risk or chooses the next gate.

The middle gates are a produce -> consume engine:

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
still has pre-9-gate files such as `04+05-requirements.md`,
`04+05+06-requirements.md`, or `06-wireframe.md`, treat them as legacy/starter
context and split them into current ③ criteria and ④ wireframe artifacts before
launch-ready handoff.

## Questions

Use this rule for non-spec moments: clarifying execution shape, workflow policy,
terminology resolution, or evidence gaps. Ask only when the answer cannot be
discovered from the repo or a reasonable assumption would be risky. Ask one
focused question at a time and include your recommended answer.

Resolve terminology as you go. If the user uses a term that conflicts with the
repo docs or code, point to the conflict and propose the canonical term.

When authoring a spec file (`01-Learn/02-unknowns.md`,
`02-Example/03-criteria.md`, `02-Example/04-wireframe.md`,
`03-Architect/05-design.md`, `03-Architect/06-tasks.md`,
`03-Architect/07-execution.md`), use the
**Grill The Spec** cycle instead.

## Set Output Form

After purpose, requirements, and principles are clear, decide what kind of
artifact this preparation should produce. This is part of requirements, not a
separate gate. Do it before wireframe, design, and task graph work so an
implementation PR is not assumed by default.

Record the output form in `02-Example/03-criteria.md`, TaskDocument
`계획 (Planning)` section, or `03-Architect/07-execution.md` rationale:

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

Wireframe does not mean UI only. Use the artifact form that fits the output:

1. Group requirements into the pages, flows, states, commands, or document
   sections they will appear in. For UI/web work, write the representative user
   journey before drawing.
2. Start with a text-first wireframe: ASCII layout, command transcript,
   sequence sketch, table/state matrix, or outline with representative mock
   data. This pass must succeed before design or medium-specific wireframing.
3. If needed, add an artifact-specific wireframe:
   - UI / app flow: rough screens or HTML with realistic records, empty states,
     error states, and visual treatment for the concrete approved case.
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
- estimate basis: previous `04-Feedback/09-retrospect.md`, cross-work `timing.md`,
  `wt agent wait-stats`, user-provided target, or conservative planning guess
- suggested watch cadence: launch validation and steady heartbeat interval for
  `wt-work`, based on expected duration and prior timing evidence
- acceptance checks
- notes for experiments or tradeoffs that shaped the slice

Record this planning context in the TaskDocument `body` as a text section, not
as top-level TOML fields (only canonical task fields are accepted). Every
TaskDocument or workflow task must include an expected duration in its
`계획 (Planning)` body section before `wt-work`, plus the estimate basis when
known. Prefer Korean human-facing labels with the stable English key in
parentheses.

Example body section:

```text
## 계획 (Planning)
- 유형 (type): AFK
- 예상 소요 (expected duration): 45m
- 예상 근거 (estimate basis): conservative planning guess
- 권장 watch cadence (suggested watch cadence): launch 45s, steady heartbeat 5-10m
- 막힘 / 의존성 (blocked by): workflow-policy-contract-simplified
- 실행 형태 (execution shape): stack child
- 크기 (size class): medium
- 확인 방법 (acceptance checks): update docs, run cargo fmt --all --check
```

Prefer several narrow slices over one broad task.

### TaskDocument body placement

The agent reads task body top-down and often acts before reaching the end. Place
**hard constraints the agent must not miss** in the top of the body, not at the
bottom. In particular, when a slice carries any of these:

- Design language or visual-grade requirements (specific fonts, banned fonts,
  layout archetype, container/shadow rules, motion easing, allowlisted icon
  set)
- Security envelope (path allowlist, sandbox boundary, secret handling)
- Cross-cutting prohibitions (e.g. "do not touch `wt ui`", "do not bump
  `Cargo.toml` version", "do not introduce dependency X")
- Base-branch / parent-branch restriction (especially when the agent might
  default to a different base)

place them in a dedicated top-level section that appears within the first ~30
lines of the body — **immediately after `## 계획 (Planning)`, before `## 맥락`**
— and reference the canonical spec/contract by path so the agent can pull
detail without scrolling.

Example body order:

```text
## 계획 (Planning)
- ...

## 필수 준수 (Hard constraints)
- Design language: Soft Structuralism + Geist + Phosphor Light. 금지: Inter,
  generic border. 정본: `<spec-path>/03-Architect/05-design.md` "Design language" 절.
- Security: write 는 `<repo-root>/.wt/execution/tasks/*.toml` 만.
- 회귀: `wt ui` 손대지 않음. `Cargo.toml` version 변경 금지.
- Base: develop (master 아님).

## 맥락
- ...
```

Background reason: empirically
(`<repo-root>/.wt/planning/specs/wt-studio-authoring-surface/04-Feedback/09-retrospect.md`),
visual-grade constraints buried in the lower half of a long task body are
silently dropped by the first agent turn even when the spec file fully states
them. Top-of-body placement is the cheap structural fix.

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

### Derive workflow mode from `03-Architect/06-tasks.md`

When a spec exists at `<repo-root>/.wt/planning/specs/<slug>/`, derive the
execution shape from `03-Architect/06-tasks.md`. Read the slice graph
(dependencies, parallel groups, shared base, lifecycle) and consult the
canonical mapping below to pick a workflow mode. Then record the choice and the
reasoning in `planning/specs/<slug>/03-Architect/07-execution.md` (see Spec
Deliverables for authoring shape).

Canonical `03-Architect/06-tasks.md` → workflow mode mapping:

| `03-Architect/06-tasks.md` slice graph | Workflow mode |
|---|---|
| All sequential, single agent | `single` |
| All independent, same base | `batch` |
| Parent → child chain (each builds on previous branch) | `stack` |
| One task × multiple profiles | `matrix` |
| One direct slice only, OR mixed-lifecycle slices (e.g. wt task + direct local edit) | `none` |

Then act on the chosen mode:

- `single` / `batch` / `stack` — create the workflow TOML via
  `wt workflow task --mode <mode> ...` at
  `<repo-root>/.wt/execution/workflows/<id>.toml`. Record its path in `03-Architect/07-execution.md`
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
Prepared wt work uses three canonical locations under the planning/execution
buckets:

- `planning/ideas/<slug>.{md,toml}` — kill-able exploration captured by
  `wt-ready`. Free-form Markdown or TOML. May be deleted at any time. No
  commitment.
- `planning/specs/<slug>/` — committed prep artifact. Holds numbered LEAF files:
  `00-status.md`, `01-Learn/01-intent.md`, `01-Learn/02-unknowns.md`,
  `01-Learn/02-references/` when needed, `02-Example/03-criteria.md`,
  `02-Example/04-wireframe.md` / `02-Example/04-wireframe/`,
  `03-Architect/05-design.md`, `03-Architect/06-tasks.md`, lazy
  `03-Architect/07-execution.md`, lazy `04-Feedback/08-review.md`, and lazy
  `04-Feedback/09-retrospect.md`.
  This is the canonical location for prep work that has been promoted past
  exploration and for spec-backed review/retrospect records.
- `execution/tasks/<slug>.toml` — TaskDocument, the launch unit. Schema
  unchanged. The body may reference `planning/specs/<slug>/` files by relative
  path.

The wt CLI does not parse or manage `planning/specs/` as executable state.
`wt scaffold <slug> --spec` may seed starter files depending on the installed
wt version; if it creates pre-9-gate files such as `03-context.md`,
`04+05-requirements.md`, `06-wireframe.md`, `07-design.md`, or
`08-tasks.md`, normalize them into the current layout before launch-ready
handoff. Spec authoring stays a human/AI artifact. TaskDocument and TaskRun
models are unchanged.

### Promotion (idea → spec)

When `wt-ready` is invoked and the user commits to preparing the work, an
existing idea file is promoted, not copied:

- `rm <repo-root>/.wt/planning/ideas/<slug>.{md,toml}` — the visible commit gate
  that distinguishes exploration from committed prep.
- Create or normalize the spec files: `00-status.md`, `01-Learn/01-intent.md`,
  `01-Learn/02-unknowns.md`, `02-Example/03-criteria.md`,
  `02-Example/04-wireframe.md`, `03-Architect/05-design.md`, and
  `03-Architect/06-tasks.md`.
- If a mode decision is recorded at prep time, create `03-Architect/07-execution.md`
  by hand. Treat it as a decision and handoff artifact, not a blank prep
  skeleton.

The deletion plus spec directory creation is the visible commit gate that
distinguishes exploration from committed prep. Work that the user requests
directly, without a prior idea, may go straight into `planning/specs/<slug>/`
without an idea file existing first.

### Authoring conventions

`01-Learn/01-intent.md`:

- Preserve the user's raw wording and the coordinator's interpreted intent as
  separate text.
- Record whether this spec was promoted from an idea path or entered prep
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
- Update each entry with verified answers, assumptions, material/conditions,
  unresolved questions, references, options, and tradeoffs.
- Put bulky source material in `01-Learn/02-references/`, but summarize the
  useful answer back in `02-unknowns.md`.
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
  `04+05+06-requirements.md`, treat it as pre-9-gate legacy context. Move
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

`03-Architect/06-tasks.md`:

- Checkbox items, sequenced as atomic units of work.
- Mark dependencies or parallelism explicitly so downstream steps can pick the
  right execution shape.

`03-Architect/07-execution.md` (LAZY, only when launch handoff exists):

- Prose record of the chosen execution shape and the reasoning derived from
  `03-Architect/06-tasks.md`. wt CLI does not read or write this file; it is for
  the human and the agent.
- Recommended sections:
  - **선택한 모드**: one of `single` / `batch` / `stack` / `matrix` / `none`.
  - **이유**: dependency analysis from `03-Architect/06-tasks.md` (sequential vs
    independent, shared base, lifecycle, parallel groups).
  - **슬라이스 → TaskDocument 매핑**: how `03-Architect/06-tasks.md` slices became
    one or more TaskDocuments (or direct local edits), with paths.
  - **연결된 workflow TOML**: `<repo-root>/.wt/execution/workflows/<id>.toml` when
    applicable; `none` otherwise.
  - **wt-work target**: exact command or target for execution launch.
  - **시간 가정 / watch cadence**: expected duration, estimate basis, launch
    validation cadence, and steady watch cadence to hand to `wt-work`.
  - **리스크**: anything to watch when execution starts.
- When mode = `none`, `03-Architect/07-execution.md` may be very brief (one
  paragraph plus the slice → TaskDocument mapping) or omitted entirely.
- The executable workflow is still the TOML at
  `<repo-root>/.wt/execution/workflows/<id>.toml`, created via
  `wt workflow task --mode ...`. `03-Architect/07-execution.md` is prose only
  and never replaces the TOML.

Spec files are not frozen at handoff. `wt-work` may update
`03-Architect/05-design.md`, `03-Architect/06-tasks.md`,
`03-Architect/07-execution.md`, and `04-Feedback/08-review.md` in place during
execution to reflect findings; treat the spec as a living artifact that the
running work writes back to. The two-way sync rule applies to
`03-Architect/07-execution.md` the same way it applies to
`03-Architect/05-design.md` / `03-Architect/06-tasks.md`: when execution drifts
from the chosen mode, update `03-Architect/07-execution.md` rather than silently
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

- Whether the clarity ledger names the weakest row instead of hiding vague
  intent inside later requirements.
- Whether unknowns are grouped by domain, standards/conventions, external, and
  internal categories.
- Whether each blocking unknown has a verified answer, explicit assumption,
  owner/user question, or a reason it is deferred.
- Whether context, references, options, and tradeoffs are summarized next to
  the unknown they answer rather than split into a stale second file.
- Whether bulky reference material belongs in `01-Learn/02-references/`.

`02-Example/03-criteria.md`:

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

`03-Architect/06-tasks.md`:

- Slice granularity. Is a slice too coarse to review safely, or too fine to
  justify its own branch?
- Dependency vs parallel claims. "Does T2 really need T1's branch commits, or
  can they share the same base?" A stack claim must survive that question.
- Whether sequential ordering is intrinsic or just the order the conversation
  produced. If it is the latter, batch or separate workflows may fit better.
- Whether each slice is independently demoable, and what the acceptance check
  actually proves.

`03-Architect/07-execution.md`:

- Mode-choice rationale. Walk the canonical mapping table from **Derive
  workflow mode from `03-Architect/06-tasks.md`** and ask which row this spec
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

- idea captured/updated at `<repo-root>/.wt/planning/ideas/<slug>.{md,toml}`
  when the user is still exploring
- spec deliverables prepared (or promoted from `planning/ideas/`) at
  `<repo-root>/.wt/planning/specs/<slug>/`, recording the chosen execution shape
- existing TaskDocuments/workflow ready, with the exact `wt-work` target
- new TaskDocument TOML files prepared
- a saved workflow prepared (mode, base, order, policy)
- a short list of unresolved HITL decisions that blocks launch

Use existing repo patterns for TaskDocument bodies. Avoid stale implementation
file paths unless they are necessary for the task.

Report:

- the four LEAF route questions after a current one-sentence intent is
  available
- current phase/gate and the first missing gate, if any
- why the next move belongs to Learn, Example, Architect, or Feedback
- evidence checked
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
