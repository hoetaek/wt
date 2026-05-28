# WT Work Sequence

Use this reference to decide which lifecycle skill owns the next artifact in a
full `wt-work` loop. The sequence is not a waterfall; it is a set of gates. When
a gate is missing, produce or update the artifact for that gate instead of
pretending the work is ready for the next command.

## Phase Map

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
       -> wt-start launch

Improve: 10 Review / sync
       -> wt-land close
       -> 11 Retrospect
```

## Skill Ownership

| Phase | Gate / lifecycle step | Primary skill | Artifact |
|---|---|---|
| Discover | Raw intent | `wt-idea`, or first minutes of `wt-ready` | `planning/ideas/<slug>.{md,toml}` or `planning/specs/<slug>/01-intent.md` |
| Discover | Unknown surfacing | `wt-idea`, or `wt-ready` when entered directly | idea body or `02-unknowns.md` with blocking-now markers |
| Discover | Context / reference exploration | `wt-idea`, or `wt-ready` for bounded prep research | idea body or `03-context.md` driven by the unknowns list |
| Shape | Purpose / success criteria | `wt-ready` | `04+05-requirements.md`, or collapsed `04+05+06-requirements.md` |
| Shape | Requirements / principles | `wt-ready` | `04+05-requirements.md` with output form, or collapsed `04+05+06-requirements.md` |
| Shape | Wireframe with mock data | `wt-ready` | `06-wireframe.md` / `06-wireframe/`, or collapsed `04+05+06-requirements.md` for tiny work |
| Execute | Design | `wt-ready` | `07-design.md` built from the passed wireframe |
| Execute | Task graph | `wt-ready` | `08-tasks.md`, optional `09-execution.md`, TaskDocuments/workflow TOML |
| Execute | Execution handoff | `wt-ready` | `09-execution.md`, TaskDocument `계획 (Planning)`, and exact `wt-start` target |
| Execute | Execution launch | `wt-start` | TaskRun/worktree/workflow and inspect target |
| Improve | Review / sync | `wt-coordinate` | reviewed diff/checks and updated `07-design.md`/`08-tasks.md`/`09-execution.md`; `10-review.md` for review evidence and unplanned research |
| Improve | Land / close | `wt-land` | landed branch proof, pass, cleanup |
| Improve | Retrospect | `wt-retrospect` | `11-retrospect.md` for spec-backed work; global `planning/retrospectives/` only for cross-work/spec-less lessons |

## Audit Questions

Before moving to the next skill, answer the matching question:

- `wt-idea` -> `wt-ready`: Do we know enough to commit to prep, or are we still
  surfacing unknowns and collecting references/possible frames? Have unknowns
  been surfaced by category (domain / standards / external / internal) with
  blocking-now markers before evidence gathering?
- `wt-ready` -> `wt-start`: Are purpose, requirements/output form, wireframe
  structure with realistic mock data, design, slice graph, expected duration,
  acceptance checks, size class, and policy explicit enough for an agent to
  start?
- `wt-start` -> `wt-coordinate`: Is there a concrete inspect target and is
  runtime state visible through `wt inspect` / `wt agent status`?
- `wt-coordinate` -> `wt-land`: Has the coordinator inspected the diff directly,
  run checks scaled to risk, resolved spec drift, logged any unplanned
  research to `10-review.md`, and completed workflow-linked runs when
  applicable?
- `wt-land` -> `wt-retrospect`: Is the work landed or explicitly discarded, and
  is there a reusable lesson worth preserving? When `10-review.md` has
  mid-process discoveries, diagnose which Unknown surfacing category was missed.

## Phase / Gate Loops Are Normal

The phase/gate chain is not a one-way pipeline. Mid-work it is normal to
discover a new unknown, find that a prior assumption was wrong, or have a
premise overturned by fresh evidence. When this happens, return to the matching
earlier phase and gate — usually Discover / Unknown surfacing, then Context
exploration if research is needed — update the unknowns list, gather fresh
material, separate verified facts from assumptions again, and only then resume
the downstream gates that depended on what changed.

This loop is normal, not a failure mode. Log each return to
`<repo-root>/.wt/planning/specs/<slug>/10-review.md` so `wt-retrospect` can classify
which surfacing category was missed and sharpen the next run's checklist.

## Practical Rule

If the current step feels blocked, name the missing gate instead of skipping it.
Examples:

- Missing unknown list: surface domain, standards/conventions, external, and
  internal unknowns before researching.
- Missing examples or direction: use the unknowns list to run bounded
  context/reference exploration.
- Missing purpose or success criteria: capture/enrich an idea or grill
  `04+05-requirements.md` or collapsed `04+05+06-requirements.md`.
- Missing observable behavior or output form: write EARS-style requirements
  and name whether the next artifact is a spec, spike, prototype, docs change,
  TaskDocument, workflow, or direct local edit.
- Missing realistic examples, states, workflow, or constraints for a
  wireframe: return to Unknown surfacing / Context exploration.
- Missing structure validation: create or grill `06-wireframe.md` before design.
- Missing dependency graph: revise `08-tasks.md` / `09-execution.md` before launch.
- Missing execution target: stop with an unresolved `wt-start` handoff.
- Missing sync: update `07-design.md`, `08-tasks.md`, `09-execution.md`, or
  `10-review.md` before landing.
