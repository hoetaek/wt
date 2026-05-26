# WT Work Sequence

Use this reference to decide which lifecycle skill owns the next artifact in a
full `wt-work` loop. The sequence is not a waterfall; it is a set of gates. When
a gate is missing, produce or update the artifact for that gate instead of
pretending the work is ready for the next command.

## Gate Map

```text
Raw intent
-> Unknown surfacing
-> Context / reference exploration
-> Purpose / success criteria
-> Requirements / principles
-> Output concept
-> Design
-> Task graph
-> Execution handoff
-> Execution launch
-> Review / sync
-> Land / close
-> Retrospect
```

## Skill Ownership

| Gate | Primary skill | Artifact |
|---|---|---|
| Raw intent | `wt-idea`, or first minutes of `wt-ready` | `ideas/<slug>.{md,toml}` or `specs/<slug>/01-intent.md` |
| Unknown surfacing | `wt-idea`, or `wt-ready` when entered directly | idea body or `02-unknowns.md` with blocking-now markers |
| Context / reference exploration | `wt-idea`, or `wt-ready` for bounded prep research | idea body or `03-context.md` driven by the unknowns list |
| Purpose / success criteria | `wt-ready` | `04+05+06-requirements.md` story/context |
| Requirements / principles | `wt-ready` | `04+05+06-requirements.md` EARS/principles |
| Output concept | `wt-ready` | `04+05+06-requirements.md`, TaskDocument planning, `09-execution.md` rationale |
| Design | `wt-ready` | `07-design.md` |
| Task graph | `wt-ready` | `08-tasks.md`, optional `09-execution.md`, TaskDocuments/workflow TOML |
| Execution handoff | `wt-ready` | `09-execution.md`, TaskDocument `계획 (Planning)`, and exact `wt-start` target |
| Execution launch | `wt-start` | TaskRun/worktree/workflow and inspect target |
| Review / sync | `wt-coordinate` | reviewed diff/checks and updated `07-design.md`/`08-tasks.md`/`09-execution.md`; `10-review.md` for review evidence and unplanned research |
| Land / close | `wt-land` | landed branch proof, completion, cleanup |
| Retrospect | `wt-retrospect` | `11-retrospect.md` for spec-backed work; global `retrospectives/` only for cross-work/spec-less lessons |

## Audit Questions

Before moving to the next skill, answer the matching question:

- `wt-idea` -> `wt-ready`: Do we know enough to commit to prep, or are we still
  surfacing unknowns and collecting references/possible frames? Have unknowns
  been surfaced by category (domain / standards / external / internal) with
  blocking-now markers before evidence gathering?
- `wt-ready` -> `wt-start`: Are purpose, requirements, output concept, design,
  slice graph, expected duration, acceptance checks, size class, and policy
  explicit enough for an agent to start?
- `wt-start` -> `wt-coordinate`: Is there a concrete inspect target and is
  runtime state visible through `wt inspect` / `wt agent status`?
- `wt-coordinate` -> `wt-land`: Has the coordinator inspected the diff directly,
  run checks scaled to risk, resolved spec drift, logged any unplanned
  research to `10-review.md`, and completed workflow-linked runs when
  applicable?
- `wt-land` -> `wt-retrospect`: Is the work landed or explicitly discarded, and
  is there a reusable lesson worth preserving? When `10-review.md` has
  mid-process discoveries, diagnose which Unknown surfacing category was missed.

## Gate Loops Are Normal

The gate chain is not a one-way pipeline. Mid-work it is normal to discover
a new unknown, find that a prior assumption was wrong, or have a premise
overturned by fresh evidence. When this happens, return to the matching
earlier gate — usually Unknown surfacing, then Context exploration if research
is needed — update the unknowns list, gather fresh material, separate verified
facts from assumptions again, and only then resume the downstream gates that
depended on what changed.

This loop is normal, not a failure mode. Log each return to
`<git-common-dir>/wt/specs/<slug>/10-review.md` so `wt-retrospect` can classify
which surfacing category was missed and sharpen the next run's checklist.

## Practical Rule

If the current step feels blocked, name the missing gate instead of skipping it.
Examples:

- Missing unknown list: surface domain, standards/conventions, external, and
  internal unknowns before researching.
- Missing examples or direction: use the unknowns list to run bounded
  context/reference exploration.
- Missing purpose or success criteria: capture/enrich an idea or grill
  `04+05+06-requirements.md`.
- Missing observable behavior: write EARS-style requirements before design.
- Missing output form: decide whether the next artifact is a spec, spike,
  prototype, docs change, TaskDocument, workflow, or direct local edit.
- Missing dependency graph: revise `08-tasks.md` / `09-execution.md` before launch.
- Missing execution target: stop with an unresolved `wt-start` handoff.
- Missing sync: update `07-design.md`, `08-tasks.md`, `09-execution.md`, or
  `10-review.md` before landing.
