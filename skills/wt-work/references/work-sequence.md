# WT Work Sequence

Use this reference to decide which lifecycle skill owns the next artifact in a
full `wt-work` loop. The sequence is not a waterfall; it is a set of gates. When
a gate is missing, produce or update the artifact for that gate instead of
pretending the work is ready for the next command.

## Gate Map

```text
Raw intent <-> Context / reference exploration
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
| Raw intent | `wt-idea`, or first minutes of `wt-ready` | `ideas/<slug>.{md,toml}` or a raw-intent note |
| Context / reference exploration | `wt-idea`, or `wt-ready` for bounded prep research | idea body, spec discovery notes |
| Purpose / success criteria | `wt-ready` | `requirements.md` story/context |
| Requirements / principles | `wt-ready` | `requirements.md` EARS/principles |
| Output concept | `wt-ready` | spec notes, TaskDocument planning, `workflow.md` rationale |
| Design | `wt-ready` | `design.md` |
| Task graph | `wt-ready` | `tasks.md`, optional `workflow.md`, TaskDocuments/workflow TOML |
| Execution handoff | `wt-ready` | TaskDocument `계획 (Planning)` and exact `wt-start` target |
| Execution launch | `wt-start` | TaskRun/worktree/workflow and inspect target |
| Review / sync | `wt-coordinate` | reviewed diff/checks and updated `design.md`/`tasks.md`/`workflow.md` |
| Land / close | `wt-land` | landed branch proof, completion, cleanup |
| Retrospect | `wt-retrospect` | `<git-common-dir>/wt/retrospectives/YYYY-MM-DD-<slug>.toml` |

## Audit Questions

Before moving to the next skill, answer the matching question:

- `wt-idea` -> `wt-ready`: Do we know enough to commit to prep, or are we still
  collecting references and possible frames?
- `wt-ready` -> `wt-start`: Are purpose, requirements, output concept, design,
  slice graph, expected duration, acceptance checks, size class, and policy
  explicit enough for an agent to start?
- `wt-start` -> `wt-coordinate`: Is there a concrete inspect target and is
  runtime state visible through `wt inspect` / `wt agent status`?
- `wt-coordinate` -> `wt-land`: Has the coordinator inspected the diff directly,
  run checks scaled to risk, resolved spec drift, and completed workflow-linked
  runs when applicable?
- `wt-land` -> `wt-retrospect`: Is the work landed or explicitly discarded, and
  is there a reusable lesson worth preserving?

## Practical Rule

If the current step feels blocked, name the missing gate instead of skipping it.
Examples:

- Missing examples or direction: run bounded context/reference exploration.
- Missing purpose or success criteria: capture/enrich an idea or grill
  `requirements.md`.
- Missing observable behavior: write EARS-style requirements before design.
- Missing output form: decide whether the next artifact is a spec, spike,
  prototype, docs change, TaskDocument, workflow, or direct local edit.
- Missing dependency graph: revise `tasks.md` / `workflow.md` before launch.
- Missing execution target: stop with an unresolved `wt-start` handoff.
- Missing sync: update `design.md`, `tasks.md`, or `workflow.md` before landing.
