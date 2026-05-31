# LEAF Work

LEAF (Learn -> Example -> Architect -> Feedback) validates the user's real
intent with cheap examples before committing to costly solutions, then feeds
review lessons back into the next iteration.

Use this reference to decide what artifact `wt-ready` should produce next. The
sequence is not a rigid waterfall. It is a set of gates: each step reduces a
different kind of uncertainty before the work becomes runnable.

## Summary

```text
Learn:       1 Intent
       -> 2 Unknowns & Context

Example:     3 Criteria
       -> 4 Wireframe with mock data

Architect:   5 Design
       -> 5.5 Critic pass (optional)
       -> 6 Task graph
       -> 7 Execution handoff

Feedback:    8 Review / sync record
       -> 9 Retrospect
```

`wt-ready` owns Learn, Example, and Architect until the handoff is runnable.
Later `wt-work` and `wt-land` continue the loop by creating, checking,
integrating, and retrospecting the result. In generic LEAF work, Gate 7 may be
the artifact itself; in `wt-ready`, Gate 7 is deliberately narrower: it prepares
the runnable handoff because the implementation result is produced after
`wt-work`.

Once Gate 1 has a current one-sentence intent, show the user a compact LEAF
route preview. Phrase each phase as a question about that intent: what to learn
in Learn, what cheap example to validate in Example, what design/tasks/handoff
to architect in Architect, and what to review or learn in Feedback. Keep it as
orientation, not a fixed plan.

Gates 1-3 use a lightweight clarity ledger to keep preparation focused:

```text
Intent      Is the desired effect and core noun stable?
Topology    What independent outcomes/components are in scope or deferred?
Success     How will we observe that the work is done?
Constraints What boundaries, non-goals, and preserved behaviors matter?
Output form What artifact or lifecycle should this produce?
```

Ask the next question against the weakest ledger row, not against the next
topic that happens to come to mind. If the user accepts moving forward while a
row is still weak, record the residual risk and the cheapest follow-up that
would reduce it.

The middle engine maps to:

```text
02-Example/03-criteria.md
-> 02-Example/04-wireframe.md or 02-Example/04-wireframe/
-> 03-Architect/05-design.md
-> 03-Architect/06-tasks.md
```

New wt specs keep Criteria, Wireframe, and Design separate. Treat pre-9-gate
files such as `04+05-requirements.md`, `04+05+06-requirements.md`,
`06-wireframe.md`, `07-design.md`, and `08-tasks.md` as legacy/starter context
and normalize them before launch-ready handoff.

Operational mapping for wt:

```text
planning/ideas/<slug>.{md,toml}
-> planning/specs/<slug>/
   00-status.md
   01-Learn/
     01-intent.md
     02-unknowns.md
     02-references/?
   02-Example/
     03-criteria.md
     04-wireframe.md or 04-wireframe/
   03-Architect/
     05-design.md
     06-tasks.md
     07-execution.md?
   04-Feedback/
     08-review.md?
     09-retrospect.md?
-> execution/tasks/<slug>.toml and/or execution/workflows/<id>.toml
-> execution/task-runs/<id>.toml
-> review, land, retrospect
```

## 0. Status Dashboard

Owner: `wt-ready`, then whichever lifecycle skill changes gate state.

Artifact: `planning/specs/<slug>/00-status.md`.

Purpose: make a durable spec resumable. It is an index, not the source of
truth; gate files remain authoritative.

Update when:

- a gate starts, becomes ready for approval, or is approved;
- work returns to an earlier gate;
- a gate is blocked/deferred;
- the next action changes materially.

Use progress values `0`, `25`, `50`, `75`, `100` and state values
`not-started`, `active`, `needs-approval`, `approved`. Treat returns as log
events, not a gate state. Put blocked/deferred reasons in `Next / Waiting on`.

## 1. Intent (Learn)

Owner: `wt-ready`.

Artifact: `planning/ideas/<slug>.{md,toml}` when the thought is exploratory;
otherwise `planning/specs/<slug>/01-Learn/01-intent.md` or a short intent note
in the TaskDocument body.

Gate to next step:

- The user's wording is preserved enough that later agents can tell what was
  requested, not only what the coordinator inferred.
- The current one-sentence intent states what the user appears to want after
  available interview/context, with uncertain parts marked as assumptions or
  questions.
- The core noun is named. If the wording alternates between idea, spec, task,
  workflow, decision, UI, command, or another object, the intent names which
  one is the real object of work and which are supporting artifacts.
- When the request has multiple possible outcomes, a compact topology lists the
  top-level components, surfaces, integrations, or deliverables that can
  succeed or fail independently, including explicitly deferred items.
- The idea is allowed to die, split, or be rewritten.
- It is clear whether the user already has enough context to state criteria or
  whether references/benchmarks are needed first.

Return here when:

- The request is only a symptom, preference, or implementation hunch.
- Multiple unrelated ideas are mixed together.
- One described component is crowding out quieter sibling components.
- The core noun keeps changing across answers.

## 2. Unknowns & Context (Learn)

Owner: `wt-ready`, with later lifecycle skills returning here when a missed
unknown appears.

Artifact: idea body section, or `planning/specs/<slug>/01-Learn/02-unknowns.md`.
Use `01-Learn/02-references/` only when bulky source material would drown the
working file; summarize the useful answer back in `02-unknowns.md`.

Purpose: name what is missing, then answer those entries in the same working
file. Unknown surfacing and context/reference exploration are one gate because
the natural loop is question -> source/ask -> update the same entry.

Gate to next step:

- Unknowns are grouped by domain concepts, standards/conventions, external
  facts, and internal facts.
- Each unknown is marked `blocking now` vs `useful later`.
- The most expensive unknowns are identified.
- Blocking unknowns have verified answers, explicit assumptions, owner/user
  questions, or a reason they are deferred.
- The fact/assumption boundary is visible.
- References, options, tradeoffs, and candidate frames are bounded enough for
  the current decision.
- The context confirms or revises the Gate 1 topology and core noun instead of
  silently changing them.

Return here when:

- A new unknown surfaces mid-work and is researched on the spot. Log it under
  `planning/specs/<slug>/04-Feedback/08-review.md` so `wt-land` can diagnose
  which category was missed next time.
- Repeated unplanned research detours start interrupting drafting or
  implementation.
- The user needs examples before they can say what they want.

## 3. Criteria (Example)

Owner: `wt-ready`.

Artifact: idea body, then `planning/specs/<slug>/02-Example/03-criteria.md`.

Purpose: combine purpose and requirements because both are pre-instance
judgment: the intended effect and the checks that make that effect observable.

Gate to next step:

- Purpose is one sentence and describes the desired effect, not only the
  artifact shape.
- Success criteria cover every active topology component, or the missing
  component is explicitly deferred.
- Functional behavior is written as observable statements, preferably EARS:
  `WHEN <condition> THE SYSTEM SHALL <behavior>`.
- Regression-sensitive behavior is explicit:
  `WHEN <condition> THE SYSTEM SHALL CONTINUE TO <preserved behavior>`.
- Relevant non-functional constraints are named.
- Principles and constraints are specific enough to reject unsuitable output
  forms or implementation shapes.
- The output form is explicit: docs-only change, implementation PR, prototype,
  spike, direct local edit, TaskDocument, saved Workflow, or mixed-lifecycle
  handoff.
- Remaining weak clarity ledger rows are carried forward as visible risk, not
  hidden inside the design.

Return here when:

- The agent would need to guess edge cases, compatibility, or preserved
  behavior.
- The team is jumping to "make a task" before deciding whether a spec, spike,
  prototype, docs change, or implementation PR is the right next artifact.
- Acceptance checks cannot be stated.

## 4. Wireframe with mock data (Example)

Owner: `wt-ready`, with `wt-work` updating it when execution findings show that
the validated structure was wrong.

Artifact: `planning/specs/<slug>/02-Example/04-wireframe.md` for one compact
artifact, or `02-Example/04-wireframe/` for several screens/flows/examples.

Purpose: validate concrete structure, workflow, and visual judgment before
design generalizes. The wireframe is not the generalized system; it is a check
that the unknowns/context gathered so far are sufficient to model the work with
representative data and states. Gate 4 produces two paired outputs: the
concrete instance being walked through, and the contract each placeholder or
mock value instantiates. Variation points must be positive axes of change: what
varies, along which axis, within what range, and with what limits.

Gate to next step:

- The text-first wireframe uses realistic mock data or representative examples,
  not empty placeholders that hide structure.
- Every placeholder or mock value has a named contract and variation point, or
  has been resolved into a real constraint before design starts.
- Requirements are grouped into the page, flow, state, command, or document
  buckets the concrete case needs.
- The operator/user can walk through the intended flow in text-first form.
- For user-facing, ambiguous, or high-risk flows, a cold reader can infer the
  actor, purpose, expected outcome, next action, and important states from the
  wireframe alone.
- Any needed artifact-specific wireframe also passes after the text-first pass.
- Empty, error, edge, loading, conflict, or migration states that affect
  structure are represented or explicitly deferred.
- The user confirms the structure fits before design starts.
- Any visual treatment is approved as a concrete case; reusable component,
  token, responsive, interaction, and state rules are deferred to design.

Return here when:

- Design is doing hidden structure discovery.
- A missing example, state, workflow step, or constraint could change the
  information architecture or command/config shape.
- The user cannot walk through the flow with the current mock data.
- A cold reader infers the wrong actor, purpose, next action, outcome, or state.
- The concrete case exposes missing behavior or acceptance criteria; return to
  Gate 3 Criteria.
- The approved instance conflicts with criteria; use Gate 3 purpose as the
  arbiter, then fix whichever of the instance or criteria fails the purpose.

## 5. Design (Architect)

Owner: `wt-ready`, with `wt-work` updating it when execution findings invalidate
assumptions.

Artifact: `planning/specs/<slug>/03-Architect/05-design.md`.

Purpose: consume the Gate 4 instance, contracts, and variation points, then
generalize them into implementation-facing design rules: component boundaries,
state model, command/config shape, data contracts, interaction rules,
responsive rules, visual system rules when relevant, and rejected alternatives.
Gate 5 borrows RALPLAN-DR as an artifact shape, not as an automatic multi-agent
loop.

Gate to next step:

- The wireframe was confirmed, even if the artifact is brief for tiny work.
- The Gate 4 contract and variation points are consumed as inputs; design does
  not rediscover the artifact shape that the wireframe was supposed to lock.
- The design names affected components and boundaries.
- The design explains how the approved concrete case generalizes to realistic
  data volume, responsive breakpoints, states, and edge cases.
- Principles, decision drivers, viable options, and steelman antithesis explain
  why the chosen option should survive review.
- Brownfield assumptions are checked against local code or docs where cheap.
- The design explains intent and responsibility, not raw code dumps.
- For high-risk or non-obvious designs, a critic pass is prepared or requested
  using `references/design-critic.md`.

Return here when:

- The requirement is clear, but the owning component, state shape, or command
  surface is not.
- The task would create a new user-facing model term without checking
  canonical docs.
- The design changes structure that the wireframe did not validate; return to
  wireframe first.
- The design treats one approved mock screen, example, or happy path as the
  whole system instead of extracting general rules.
- The design cannot generalize without inventing missing cases; return to Gate
  4 and add another concrete case.

## 5.5 Design Critic Pass (optional)

Owner: `wt-ready`, only when risk triggers fire.

Artifact: review note in the conversation, in `03-Architect/05-design.md`, or in
a short adjacent note if a durable record is needed.

Use `references/design-critic.md` when the design involves security, migration,
public CLI/config/state shape, cross-module coupling, new user-facing terms,
large UI/workflow behavior shifts, or one asserted option with weak
alternatives. Verdicts are `APPROVE`, `ITERATE`, or `REJECT`.

## 6. Task graph (Architect)

Owner: `wt-ready`.

Artifact: `planning/specs/<slug>/03-Architect/06-tasks.md`, optional
`03-Architect/07-execution.md`, TaskDocuments, and saved Workflow TOML when
needed.

Gate to next step:

- Each slice is independently reviewable or has a clear dependency reason.
- Dependencies are real branch/content dependencies, not conversation order.
- Each slice has type (`AFK` or `HITL`), expected duration, acceptance checks,
  and size class.
- Execution shape follows the graph:
  - one direct slice: direct task;
  - independent same-base slices: `batch`;
  - parent-to-child branch dependency: `stack`;
  - one task across profiles: `matrix`;
  - mixed lifecycles or one direct local edit: `none`.
- Size is checked against `task-pr-size-guidance.md`.

Return here when:

- A slice is too large to review or too small to justify its own branch.
- The proposed stack is only a packaging choice, not a dependency claim.

## 7. Execution handoff (Architect)

Owner: `wt-ready` prepares; `wt-work` launches.

Artifact: `03-Architect/07-execution.md`, TaskDocument body `계획 (Planning)`
section, optional Workflow policy snapshot, and exact `wt-work` target.

Gate to next step:

- The handoff names what to run next, not just what to think about.
- PR/landing policy source is recorded.
- The work has enough context for an agent to start without rediscovering the
  basics.
- At least one concrete execution signal is present: file path, module/symbol,
  issue or task id, acceptance criteria, numbered implementation steps,
  expected command/config transcript, representative example/mock data, named
  output artifact, or a user-accepted residual risk recorded in the handoff.

Return here when:

- The next command is ambiguous.
- Required expected duration, policy, or acceptance checks are missing.
- The handoff asks an agent to "improve", "build", "fix this", or otherwise
  execute without a concrete signal or explicit accepted risk.

## 8. Review / sync record (Feedback material)

Owner: `wt-work` records the evidence while doing Architect execution;
`wt-land` consumes it as Feedback material.

Artifact: reviewed diff/check evidence, updated `03-Architect/05-design.md`,
`03-Architect/06-tasks.md`, `03-Architect/07-execution.md`, and
`04-Feedback/08-review.md` when execution reality changes the plan.

Gate to next step:

- The coordinator has inspected the work directly, not only trusted an agent
  report.
- Checks are scaled to risk and recorded.
- Spec drift is fixed in the spec instead of being left as a stale artifact.
- Unplanned research is logged to `04-Feedback/08-review.md` so the
  retrospective can diagnose which Unknowns & Context category was missed.
- Workflow-linked runs are completed only after review passes.

Return here when:

- Implementation reveals a criterion or design assumption was wrong.
- The diff is too broad for the prepared task and needs re-slicing.

## 9. Retrospect (Feedback)

Owner: `wt-land` after landing or explicit discard.

Artifact: `planning/specs/<slug>/04-Feedback/09-retrospect.md` for spec-backed
work, or `<repo-root>/.wt/planning/retrospectives/YYYY-MM-DD-<slug>.toml` for
cross-work or spec-less retrospectives.

Gate to future work:

- The retrospective names observable keep/problem/try lessons.
- Action candidates say whether they should become ideas, TaskDocuments, or
  skill/docs changes.
- Harness tuning names the exact file and section to update when the lesson
  should permanently change agent behavior.
- When `04-Feedback/08-review.md` records mid-process discoveries for this run,
  each discovery is classified against the Unknowns & Context categories
  (domain / standards / external / internal). The missed category becomes a
  `try` item or a `harness_tuning` entry for the next run's surfacing pass.

Return here when:

- A repeated planning, slicing, coordination, or review mistake would happen
  again unless the harness changes.

## Practical Rule

When a step is missing, produce the artifact for that step instead of pretending
the work is ready for the next one. Examples:

- Missing purpose/success criteria: capture or enrich an idea.
- Missing unknown/context list: surface domain, standards/conventions, external,
  and internal unknowns before researching.
- Missing examples or direction: use the unknowns list to run bounded
  discovery/reference benchmarking.
- Missing observable behavior or output form: write or grill
  `02-Example/03-criteria.md`.
- Missing mock data, workflow, states, or constraints for structure: return to
  `01-Learn/02-unknowns.md`.
- Missing structure validation: write or grill the text-first
  `02-Example/04-wireframe.md` before design; add an artifact-specific
  wireframe only when needed.
- Missing ownership/boundary decision: write or grill `03-Architect/05-design.md`.
- Missing dependency graph: write or grill `03-Architect/06-tasks.md` /
  `03-Architect/07-execution.md`.
- Missing reviewable size: split the task graph.
- Missing execution target: stop with unresolved `wt-work` handoff.
