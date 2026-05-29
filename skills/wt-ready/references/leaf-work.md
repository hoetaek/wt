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
       -> 2 Unknown surfacing
       -> 3 Context / reference exploration

Example:     4 Purpose / success criteria
       -> 5 Requirements / principles
       -> 6 Wireframe with mock data

Architect:   7 Design
       -> 8 Task graph
       -> 9 Execution handoff

Feedback:   10 Review / sync record
       -> 11 Retrospect
```

`wt-ready` owns Learn when needed, and may stop there with a kill-able idea
under `planning/ideas/`. When the user commits to prep, `wt-ready` continues
through Example and Architect: purpose, requirements, wireframe, design, task
graph, and handoff. Later `wt-work` and `wt-land` continue Architect by
creating, checking, and integrating the result. Feedback is owned by `wt-land`,
using review evidence and spec drift captured during coordination.

Once Gate 1 has a current one-sentence intent, show the user a compact LEAF
route preview. Phrase each phase as a question about that intent: what to learn
in Learn, what cheap example to validate in Example, what design/tasks/handoff
to architect in Architect, and what to review or learn in Feedback.
Keep it as orientation, not a fixed plan.

Kiro's spec-driven workflow maps to
`04+05-requirements.md -> 06-wireframe.md -> 07-design.md -> 08-tasks.md`.
Existing scaffolded specs may collapse requirements and wireframe into
`04+05+06-requirements.md` for tiny work.
AI-DLC maps more broadly to `Inception -> Construction -> Operations`. For wt,
the operational mapping is:

```text
planning/ideas/<slug>.{md,toml}
-> planning/specs/<slug>/{01-intent.md,02-unknowns.md,03-context.md,
   04+05-requirements.md or 04+05+06-requirements.md,06-wireframe.md?,
   07-design.md,08-tasks.md,09-execution.md?,10-review.md?,11-retrospect.md?}
-> execution/tasks/<slug>.toml and/or execution/workflows/<id>.toml
-> execution/task-runs/<id>.toml
-> review, land, retrospect
```

## 1. Intent (Learn)

Owner: `wt-ready`.

Artifact: `planning/ideas/<slug>.{md,toml}` when the thought is exploratory; otherwise
`planning/specs/<slug>/01-intent.md` or a short intent note in the TaskDocument body.

Gate to next step:

- The user's wording is preserved enough that later agents can tell what was
  requested, not only what the coordinator inferred.
- The current one-sentence intent states what the user appears to want after
  available interview/context, with uncertain parts marked as assumptions or
  questions.
- The idea is allowed to die, split, or be rewritten.
- It is clear whether the user already has enough context to state purpose and
  success criteria, or whether references/benchmarks are needed first.

Return here when:

- The request is only a symptom, preference, or implementation hunch.
- Multiple unrelated ideas are mixed together.

## 2. Unknown surfacing (Learn)

Owner: `wt-ready`.

Artifact: idea body section, or `planning/specs/<slug>/02-unknowns.md`, listing unknowns
by category — domain concepts, standards/conventions, external facts, internal
facts — with each item marked `blocking now` or `useful later`.

Purpose: name what is missing before researching. Without this gate, context
exploration becomes reactive and the same kinds of research keep surfacing
mid-work as unplanned detours.

Gate to next step:

- Unknowns are grouped by category, not dumped as one flat list.
- Each unknown is marked `blocking now` vs `useful later`.
- The most expensive unknowns (the ones that would unravel later work if
  unresolved) are identified.
- The list becomes the agenda for the next exploration or evidence-gathering
  pass.

Return here when:

- A new unknown surfaces mid-work and is researched on the spot — surfacing
  was incomplete. Log it under `planning/specs/<slug>/10-review.md` so
  `wt-land` can diagnose which category was missed next time.
- Repeated unplanned research detours start interrupting drafting or
  implementation.

## 3. Context / reference exploration (Learn)

Owner: `wt-ready`.

Artifact: idea body sections for references/options/tradeoffs, or
`planning/specs/<slug>/03-context.md`.

Purpose: use the Unknown surfacing list as the research agenda, then sharpen
intent before forcing purpose, requirements, or output form. This is where
contextual research, reference benchmarking, prior art, related tasks, product
examples, and possible solution frames belong when the user cannot yet picture
the desired result.

Gate to next step:

- Blocking unknowns from gate 2 are resolved, explicitly deferred, or turned
  into HITL/spike work.
- The discovery set is bounded enough for the current decision.
- 2-4 plausible directions or frames are named.
- Each direction has a tradeoff or reason to accept/reject.
- The user/coordinator can now state clearer purpose/success criteria or choose
  the next exploration question.

Return here when:

- The purpose feels invented from the first idea instead of discovered.
- The user needs examples before they can say what they want.
- There are too many possible product/document/workflow shapes.

## 4. Purpose / success criteria (Example)

Owner: `wt-ready`.

Artifact: idea body, then `planning/specs/<slug>/04+05-requirements.md`
or collapsed `04+05+06-requirements.md` user story and problem context.

Gate to next step:

- The desired user, developer, or maintainer effect is stated in one sentence.
- Success criteria describe why the work matters, not just what artifact to
  create.
- Success can plausibly be observed.

Return here when:

- The implementation is named but the benefit is unclear.
- A task would need the agent to invent product intent.

## 5. Requirements / principles (Example)

Owner: `wt-ready`.

Artifact: `planning/specs/<slug>/04+05-requirements.md` or collapsed
`04+05+06-requirements.md`.

Gate to next step:

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
- Open questions are either resolved, recorded as assumptions, or turned into a
  HITL/spike slice.

Return here when:

- The agent would need to guess edge cases, compatibility, or preserved
  behavior.
- The team is jumping to "make a task" before deciding whether a spec, spike,
  prototype, docs change, or implementation PR is the right next artifact.
- Acceptance checks cannot be stated.

## 6. Wireframe with mock data (Example)

Owner: `wt-ready`, with `wt-work` updating it when execution findings
show that the validated structure was wrong.

Artifact: `planning/specs/<slug>/06-wireframe.md` for one compact artifact,
`06-wireframe/` for several screens/flows/examples, or collapsed
`04+05+06-requirements.md` when the work is tiny and the structural sketch fits
inside requirements. Gate 6 is the cheap-iteration gate before expensive
generalization. It validates a concrete case by grouping requirements into
pages, flows, states, commands, or document sections; walking a representative
journey; then drawing a text-first wireframe by default: ASCII layout, command
transcript, sequence sketch, table/state matrix, or outline with placeholder
evidence. After that passes, add the artifact-specific shape when needed: HTML
for web, generated TOML example, TaskDocument/workflow flow,
API request/response examples, or realistic state table. For visual outputs,
this may include visual treatment for the concrete case; it is still a case,
not the generalized system.

Purpose: validate concrete structure, workflow, and visual judgment before
design generalizes. The wireframe is not the generalized system; it is a check
that the unknowns/context gathered so far are sufficient to model the work with
representative data and states.

Entry condition:

- Wireframe-relevant unknowns have been surfaced and either resolved,
  explicitly deferred, or turned into HITL/spike work.
- `03-context.md` has enough verified facts, representative examples, current
  behavior, user/team material, constraints, and assumptions to build a
  realistic text-first artifact.
- Mock data or representative examples exist. If they do not, return to Unknown
  surfacing / Context exploration before drawing the structure.

Gate to next step:

- The text-first wireframe uses realistic mock data or representative examples,
  not empty placeholders that hide structure.
- Requirements are grouped into the page, flow, state, command, or document
  buckets the concrete case needs.
- The operator/user can walk through the intended flow in text-first form.
- For user-facing, ambiguous, or high-risk flows, a cold reader can infer the
  actor, purpose, expected outcome, next action, and important states from the
  wireframe alone.
- Any needed artifact-specific wireframe also passes after the text-first pass.
- Empty, error, edge, loading, conflict, or migration states that affect
  structure are represented or explicitly deferred.
- The wireframe reveals whether the requirements are complete enough for
  design.
- The user confirms the structure fits before design starts.
- Any visual treatment is approved as a concrete case; reusable component,
  token, responsive, interaction, and state rules are deferred to design.

Return here when:

- Design is doing hidden structure discovery.
- A missing example, state, workflow step, or constraint could change the
  information architecture or command/config shape.
- The user cannot walk through the flow with the current mock data.
- A cold reader infers the wrong actor, purpose, next action, outcome, or state
  from the wireframe alone.
- An artifact-specific wireframe was started before the text-first structure
  passed.
- The visual mockup exposes missing context or data; return to Learn.
- The concrete case exposes missing behavior or acceptance criteria; return to
  Gate 5 Requirements.

## 7. Design (Architect)

Owner: `wt-ready`, with `wt-work` updating it when execution findings
invalidate assumptions.

Artifact: `planning/specs/<slug>/07-design.md`.

Purpose: generalize the passed concrete case into implementation-facing design
rules: component boundaries, state model, command/config shape, data contracts,
interaction rules, responsive rules, visual system rules when relevant, and
rejected alternatives.

Gate to next step:

- The wireframe was confirmed or intentionally collapsed for tiny work.
- The design names affected components and boundaries.
- The design explains how the approved concrete case generalizes to realistic
  data volume, responsive breakpoints, states, and edge cases.
- At least one rejected alternative or simpler option is recorded when the
  choice is non-obvious.
- Brownfield assumptions are checked against local code or docs where cheap.
- The design explains intent and responsibility, not raw code dumps.

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
  6 and add another concrete case.

## 8. Task graph (Architect)

Owner: `wt-ready`.

Artifact: `planning/specs/<slug>/08-tasks.md`, optional `09-execution.md`,
TaskDocuments, and saved Workflow TOML when needed.

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

## 9. Execution handoff (Architect)

Owner: `wt-ready` prepares; `wt-work` launches.

Artifact: `09-execution.md`, TaskDocument body `계획 (Planning)` section,
optional Workflow policy snapshot, and exact `wt-work` target.

In generic LEAF work, Gate 9 is the result or execution artifact itself. In
`wt-ready`, Gate 9 is deliberately narrower: it prepares the runnable handoff
because the actual implementation result is produced after `wt-work`.

Gate to next step:

- The handoff names what to run next, not just what to think about.
- PR/landing policy source is recorded.
- The work has enough context for an agent to start without rediscovering the
  basics.

Return here when:

- The next command is ambiguous.
- Required expected duration, policy, or acceptance checks are missing.

## 10. Review / sync record (Feedback material)

Owner: `wt-work` records the evidence while doing Architect execution;
`wt-land` consumes it as Feedback material.

Artifact: reviewed diff/check evidence, updated `07-design.md`,
`08-tasks.md`, `09-execution.md`, and `10-review.md` when execution reality
changes the plan.

Gate to next step:

- The coordinator has inspected the work directly, not only trusted an agent
  report.
- Checks are scaled to risk and recorded.
- Spec drift is fixed in the spec instead of being left as a stale artifact.
- Unplanned research is logged to `10-review.md` so the
  retrospective can diagnose which Unknown surfacing category was missed.
- Workflow-linked runs are completed only after review passes.

Return here when:

- Implementation reveals a requirement or design assumption was wrong.
- The diff is too broad for the prepared task and needs re-slicing.

## 11. Retrospect (Feedback)

Owner: `wt-land` after landing or explicit discard.

Artifact: `planning/specs/<slug>/11-retrospect.md` for spec-backed work, or
`<repo-root>/.wt/planning/retrospectives/YYYY-MM-DD-<slug>.toml` for cross-work or
spec-less retrospectives.

Gate to future work:

- The retrospective names observable keep/problem/try lessons.
- Action candidates say whether they should become ideas, TaskDocuments, or
  skill/docs changes.
- Harness tuning names the exact file and section to update when the lesson
  should permanently change agent behavior.
- When `10-review.md` records mid-process discoveries for this run, each discovery is
  classified against the Unknown surfacing categories (domain / standards /
  external / internal). The category that was missed becomes either a
  `try` item or a `harness_tuning` entry for the next run's surfacing pass.

Return here when:

- A repeated planning, slicing, coordination, or review mistake would happen
  again unless the harness changes.

## Practical Rule

When a step is missing, produce the artifact for that step instead of pretending
the work is ready for the next one. Examples:

- Missing purpose/success criteria: capture or enrich an idea.
- Missing unknown list: surface domain, standards/conventions, external, and
  internal unknowns before researching.
- Missing examples or direction: use the unknowns list to run bounded
  discovery/reference benchmarking.
- Missing observable behavior or output form: write or grill
  `04+05-requirements.md` or collapsed `04+05+06-requirements.md`.
- Missing mock data, workflow, states, or constraints for structure: return to
  `02-unknowns.md` / `03-context.md`.
- Missing structure validation: write or grill the text-first
  `06-wireframe.md` before design; add an artifact-specific wireframe only when
  needed.
- Missing ownership/boundary decision: write or grill `07-design.md`.
- Missing dependency graph: write or grill `08-tasks.md` / `09-execution.md`.
- Missing reviewable size: split the task graph.
- Missing execution target: stop with unresolved `wt-work` handoff.
