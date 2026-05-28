# WT Work Sequence

EDGE (Explore -> Demonstrate -> Generalize -> Evolve) validates the user's
real intent with cheap concrete cases before committing to costly solutions,
then feeds execution lessons back into the next iteration.

Use this reference to decide which lifecycle skill owns the next artifact in a
full `wt-work` loop. The sequence is not a waterfall; it is a set of gates. When
a gate is missing, produce or update the artifact for that gate instead of
pretending the work is ready for the next command.

## EDGE Phase Map

```text
Explore:     1 Intent
       -> 2 Unknown surfacing
       -> 3 Context / reference exploration

Demonstrate: 4 Purpose / success criteria
       -> 5 Requirements / principles
       -> 6 Wireframe with mock data

Generalize:  7 Design
       -> 8 Task graph
       -> 9 Execution handoff

Launch:      wt-start (lifecycle transition, not an EDGE gate)

Evolve:     10 Review / sync
       -> wt-land close
       -> 11 Retrospect
```

## Skill Ownership

| Phase | Gate / lifecycle step | Primary skill | Artifact |
|---|---|---|
| Explore | Intent | `wt-idea`, or first minutes of `wt-ready` | `planning/ideas/<slug>.{md,toml}` or `planning/specs/<slug>/01-intent.md` |
| Explore | Unknown surfacing | `wt-idea`, or `wt-ready` when entered directly | idea body or `02-unknowns.md` with blocking-now markers |
| Explore | Context / reference exploration | `wt-idea`, or `wt-ready` for bounded prep research | idea body or `03-context.md` driven by the unknowns list |
| Demonstrate | Purpose / success criteria | `wt-ready` | `04+05-requirements.md`, or collapsed `04+05+06-requirements.md` |
| Demonstrate | Requirements / principles | `wt-ready` | `04+05-requirements.md` with output form, or collapsed `04+05+06-requirements.md` |
| Demonstrate | Wireframe with mock data | `wt-ready` | requirements grouped into pages/flows/states, text-first wireframe in `06-wireframe.md` / `06-wireframe/`, then artifact-specific concrete case when needed; collapsed `04+05+06-requirements.md` for tiny work |
| Generalize | Design | `wt-ready` | `07-design.md` generalizes the passed concrete case into component/state/data/interaction/visual-system rules |
| Generalize | Task graph | `wt-ready` | `08-tasks.md`, optional `09-execution.md`, TaskDocuments/workflow TOML |
| Generalize | Execution handoff | `wt-ready` | `09-execution.md`, TaskDocument `계획 (Planning)`, and exact `wt-start` target |
| Transition | Launch prepared work | `wt-start` | TaskRun/worktree/workflow and inspect target |
| Evolve | Review / sync | `wt-coordinate` | reviewed diff/checks and updated `07-design.md`/`08-tasks.md`/`09-execution.md`; `10-review.md` for review evidence and unplanned research |
| Evolve | Land / close | `wt-land` | landed branch proof, pass, cleanup |
| Evolve | Retrospect | `wt-retrospect` | `11-retrospect.md` for spec-backed work; global `planning/retrospectives/` only for cross-work/spec-less lessons |

## Audit Questions

Before moving to the next skill, answer the matching question:

- `wt-idea` -> `wt-ready`: Do we know enough to commit to prep, or are we still
  surfacing unknowns and collecting references/possible frames? Have unknowns
  been surfaced by category (domain / standards / external / internal) with
  blocking-now markers before evidence gathering?
- `wt-ready` -> `wt-start`: Are purpose, requirements/output form, wireframe
  structure with realistic mock data, design, slice graph, expected duration,
  acceptance checks, size class, and policy explicit enough for an agent to
  start? If the work needed an artifact-specific wireframe, did the text-first
  pass happen before it? Did Gate 6 validate a cheap concrete case before Gate
  7 generalized it? Does design generalize the approved concrete case instead
  of treating one mock screen or happy path as the whole system?
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
earlier phase and gate — usually Explore / Unknown surfacing, then Context
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
- Missing structure validation: group requirements into concrete pages, flows,
  states, commands, or document sections; create or grill the text-first
  `06-wireframe.md` before design; then add an artifact-specific wireframe or
  visual mockup only when needed.
- Missing dependency graph: revise `08-tasks.md` / `09-execution.md` before launch.
- Missing execution target: stop with an unresolved `wt-start` handoff.
- Missing sync: update `07-design.md`, `08-tasks.md`, `09-execution.md`, or
  `10-review.md` before landing.
