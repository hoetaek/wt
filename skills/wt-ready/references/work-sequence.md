# Work Sequence

Use this reference to decide what artifact `wt-ready` should produce next. The
sequence is not a rigid waterfall. It is a set of gates: each step reduces a
different kind of uncertainty before the work becomes runnable.

## Summary

```text
Discover: 1 Raw intent
       -> 2 Unknown surfacing
       -> 3 Context / reference exploration

Shape:    4 Purpose / success criteria
       -> 5 Requirements / principles
       -> 6 Wireframe with mock data

Execute:  7 Design
       -> 8 Task graph
       -> 9 Execution handoff

Improve: 10 Review / sync
       -> 11 Retrospect
```

For `wt-ready`, Discover is either inherited from `wt-idea` or reconstructed
when the user enters prep directly. Shape and Execute-prep are the main
ownership area: settle purpose, requirements, wireframe, design, task graph, and
handoff. Improve is owned later by `wt-coordinate` and `wt-retrospect`, but
`wt-ready` must leave artifacts that are easy to sync and learn from.

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

## 1. Raw intent (Discover)

Owner: `wt-idea`, or the first minutes of `wt-ready` when the user skips idea
capture.

Artifact: `planning/ideas/<slug>.{md,toml}` when the thought is exploratory; otherwise
`planning/specs/<slug>/01-intent.md` or a short raw-intent note in the TaskDocument body.

Gate to next step:

- The user's wording is preserved enough that later agents can tell what was
  requested, not only what the coordinator inferred.
- The idea is allowed to die, split, or be rewritten.
- It is clear whether the user already has enough context to state purpose and
  success criteria, or whether references/benchmarks are needed first.

Return here when:

- The request is only a symptom, preference, or implementation hunch.
- Multiple unrelated ideas are mixed together.

## 2. Unknown surfacing (Discover)

Owner: `wt-idea`, or `wt-ready` when entered directly without prior idea capture.

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
  `wt-retrospect` can diagnose which category was missed next time.
- Repeated unplanned research detours start interrupting drafting or
  implementation.

## 3. Context / reference exploration (Discover)

Owner: `wt-idea` for exploratory research, `wt-ready` when reference gathering
is needed before committed prep.

Artifact: idea body sections for references/options/tradeoffs, or
`planning/specs/<slug>/03-context.md`.

Purpose: use the Unknown surfacing list as the research agenda, then sharpen
raw intent before forcing purpose, requirements, or output form. This is where
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

## 4. Purpose / success criteria (Shape)

Owner: `wt-idea` for exploration, `wt-ready` for committed prep.

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

## 5. Requirements / principles (Shape)

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

## 6. Wireframe with mock data (Shape)

Owner: `wt-ready`, with `wt-coordinate` updating it when execution findings
show that the validated structure was wrong.

Artifact: `planning/specs/<slug>/06-wireframe.md` for one compact artifact,
`06-wireframe/` for several screens/flows/examples, or collapsed
`04+05+06-requirements.md` when the work is tiny and the structural sketch fits
inside requirements. For non-UI work, use the same gate with the right low-fi
shape: command transcript, generated TOML example, TaskDocument/workflow flow,
outline with placeholder evidence, API request/response examples, or realistic
state table.

Purpose: validate structure and workflow before design. The wireframe is not a
visual polish step; it is a check that the unknowns/context gathered so far are
sufficient to model the work with representative data and states.

Entry condition:

- Wireframe-relevant unknowns have been surfaced and either resolved,
  explicitly deferred, or turned into HITL/spike work.
- `03-context.md` has enough verified facts, representative examples, current
  behavior, user/team material, constraints, and assumptions to build a
  realistic low-fi artifact.
- Mock data or representative examples exist. If they do not, return to Unknown
  surfacing / Context exploration before drawing the structure.

Gate to next step:

- The wireframe uses realistic mock data or representative examples, not empty
  placeholders that hide structure.
- The operator/user can walk through the intended flow.
- Empty, error, edge, loading, conflict, or migration states that affect
  structure are represented or explicitly deferred.
- The wireframe reveals whether the requirements are complete enough for
  design.
- The user confirms the structure fits before design starts.

Return here when:

- Design is doing hidden structure discovery.
- A missing example, state, workflow step, or constraint could change the
  information architecture or command/config shape.
- The user cannot walk through the flow with the current mock data.

## 7. Design (Execute prep)

Owner: `wt-ready`, with `wt-coordinate` updating it when execution findings
invalidate assumptions.

Artifact: `planning/specs/<slug>/07-design.md`.

Purpose: turn the passed wireframe into a final design: component boundaries,
state model, command/config shape, information architecture, visual language
when relevant, and rejected alternatives.

Gate to next step:

- The wireframe was confirmed or intentionally collapsed for tiny work.
- The design names affected components and boundaries.
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

## 8. Task graph (Execute prep)

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

## 9. Execution handoff (Execute prep)

Owner: `wt-ready` prepares; `wt-start` launches.

Artifact: `09-execution.md`, TaskDocument body `계획 (Planning)` section,
optional Workflow policy snapshot, and exact `wt-start` target.

Gate to next step:

- The handoff names what to run next, not just what to think about.
- PR/landing policy source is recorded.
- The work has enough context for an agent to start without rediscovering the
  basics.

Return here when:

- The next command is ambiguous.
- Required expected duration, policy, or acceptance checks are missing.

## 10. Review / sync (Improve)

Owner: `wt-coordinate`.

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

## 11. Retrospect (Improve)

Owner: `wt-retrospect`, normally called by `wt-work` after landing or explicit
discard.

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
- Missing structure validation: write or grill `06-wireframe.md` before design.
- Missing ownership/boundary decision: write or grill `07-design.md`.
- Missing dependency graph: write or grill `08-tasks.md` / `09-execution.md`.
- Missing reviewable size: split the task graph.
- Missing execution target: stop with unresolved `wt-start` handoff.
