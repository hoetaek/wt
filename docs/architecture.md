# Architecture Boundary Contract

This document defines implementation boundaries for `wt` refactors. It is not
the user-facing model; keep UX concepts, command naming, and persisted state
semantics in [docs/consistency.md](consistency.md). Use this document when a
file is getting large and the question is where responsibility should move.

The goal is not smaller files by itself. A split is useful only when it makes a
module own one layer: command orchestration, domain model, state store,
execution planning, external service integration, rendering, or setup side
effects.

## Layer Ownership

| Layer | Source of truth | Owns | Must not own |
| --- | --- | --- | --- |
| CLI command orchestration | `src/commands/*.rs`, especially `src/commands/workflow.rs` | Argument-level flow, interactive selection, high-level calls into stores, planners, services, and setup | Persisted schemas, TOML parsing rules, provider command details, long rendering templates |
| Task state | `src/commands/task.rs` and `src/commands/task_run.rs` today | `TaskDocument` under `.local/tasks` and `TaskRun` under `.local/task-runs` | Workflow mode semantics, cmux coordinates, branch landing state |
| Workflow state store | `src/workflow.rs` today | `WorkflowMode`, `WorkflowMetadata`, `WorkflowTask`, `.local/workflows` paths, validation, read/write/list/resolve, TOML rendering for workflow files | Starting worktrees, prompting agents, selecting runnable workflows, mutating TaskRun status beyond store validation |
| Workflow execution planner | `src/commands/workflow.rs` today; extract to `src/workflow/planner.rs` when split | Runnable-workflow selection, single/batch/stack next-step rules, preflight plans, parent-chain calculation | UI printing, cmux calls, issue-provider calls, file serialization |
| Workflow runner orchestration | `src/commands/workflow.rs` today; extract to `src/workflow/run.rs` when split | Coordinating planner output with `TaskDocument`, `TaskRun`, `issue` start paths, and setup results | Domain schema definitions, config merge policy, provider implementation details |
| Rendering | `ctx.ui` call sites and prompt builders in command modules today; extract workflow prompt/status text to `src/workflow/render.rs` when split | Human status text, selector labels, agent prompt snapshots, coordinator handoff text | State transitions, filesystem writes, shelling out to tools |
| Config layering | `src/config/` | Config schema, `.wt.toml` and `.local/.wt.toml` load order, profile resolution, profile convention overlays, prompt merge/finalization | Worktree creation, site registration, TaskDocument/TaskRun/Workflow state |
| Setup side effects | `src/setup.rs` | Copy/link/env substitution, site registration, cmux workspace opening, agent bootstrap, dependency/test command execution, setup summary | Config precedence, workflow planning, task state ownership |
| External services | `src/services/*` | Shell/tool boundaries for Git, GitHub, Linear, cmux, Herd, Valet, Docker proxy, Traefik, issue providers, and work-session observation | CLI policy, persisted wt state schemas, UX concept naming |

## Canonical State

`TaskDocument` is the reusable work definition. Its current source-of-truth
module is `src/commands/task.rs`, and its durable location is
`.local/tasks/<task>.toml`. It owns title, branch, body, and optional origin.
It does not own execution status, workflow membership, profile choice, or cmux
transport data.

`TaskRun` is one execution instance of a `TaskDocument`. Its current
source-of-truth module is `src/commands/task_run.rs`, and its durable location
is `.local/task-runs/<id>.toml`. It owns task, branch, status, source, group,
error, creation_order, and timestamps. It records execution state, not branch
landing or review state.

`Workflow` is the saved prepared-work plan. Its current source-of-truth module
is `src/workflow.rs`, and its durable location is
`.local/workflows/<id>.toml`. It owns mode, base, profile, color, timestamps,
and `[[tasks]]` rows linking TaskDocuments to TaskRuns. It does not copy task
branch names or status fields; branch comes from `TaskDocument`, execution
status comes from `TaskRun`.

`batch` and `stack` are Workflow modes. New prepared-work state belongs in
`.local/workflows` with `mode = "single" | "batch" | "stack"`. The
`.local/batches`, `.local/stacks`, `wt batch`, and `wt stack` surfaces are
legacy migration context. Refactors may read them to preserve old state during
migration, but new behavior should not extend them as canonical stores or teach
them as equal command surfaces.

cmux is a runtime and surface integration. `src/services/cmux.rs` owns the
external command boundary; setup and command renderers may use current cmux
coordinates to open workspaces or print handoff instructions. `wt` owns
TaskDocument, TaskRun, and Workflow state. cmux workspace or surface coordinates
are transport details and must not become persisted task or workflow state.

Site providers are external services. `SiteConfig` and provider choice live in
`src/config/`; service dispatch lives in `src/services/site.rs`; provider
implementations live in `src/services/herd.rs`, `src/services/valet.rs`, and
`src/services/traefik.rs`, with no-op providers such as Docker proxy handled by
the dispatch layer. `src/setup.rs` may invoke them while preparing a worktree,
but provider modules should not decide CLI behavior or persisted wt state shape.

## Workflow Refactor Target

Keep `src/commands/workflow.rs` as the command facade: validate command options,
select user input, load the relevant records, call planners/runners, and print
the final user-visible outcome.

Keep `src/workflow.rs` as the Workflow state facade. If it grows, turn it into a
`src/workflow/` module while preserving these boundaries:

- `src/workflow/model.rs`: `WorkflowMode`, `WorkflowMetadata`,
  `WorkflowTask`, `WorkflowRecord`, and pure validation of workflow shape.
- `src/workflow/store.rs`: `.local/workflows` path resolution, read, list,
  create, write, migration-only target shortcuts while they exist, and
  workflow-file TOML rendering.
- `src/workflow/planner.rs`: runnable rules for single, batch, and stack modes,
  parent-chain planning, duplicate branch/path preflight, and next-task
  selection.
- `src/workflow/run.rs`: applying planner decisions to TaskDocument/TaskRun and
  invoking issue/worktree start paths.
- `src/workflow/render.rs`: selector labels, status summaries, task snapshots,
  and coordinator handoff text.

Use the same direction for task state if it is extracted later:
`src/task/model.rs` and `src/task/store.rs` for `TaskDocument`, and
`src/task_run/model.rs` and `src/task_run/store.rs` for TaskRun. Command modules
should call those stores; they should not become the long-term home for state
schemas.

## Setup and Config Boundary

`src/config/` owns how configuration becomes effective configuration:
schema parsing, shared/local load order, profile selection, profile directory
loading, convention overlays, and prompt merge/finalization. Commands may ask
whether a profile is valid and may pass an effective `Config` onward. They
should not duplicate merge rules or inspect profile directories directly.

`src/setup.rs` owns effects that happen after a worktree path, names, prompt
context, and effective config are already known. It can copy or link files,
substitute env values, register a site, open cmux, bootstrap an agent, run
dependency/test commands, and print a setup summary. It should not decide which
TaskDocument or Workflow is runnable, write TaskRun status, or define config
precedence.

## Checklist for New Commands

Before adding a command or expanding an existing one:

- Name the user-facing concept in `docs/consistency.md` first if the UX model
  changes.
- Identify the state owner: TaskDocument, TaskRun, Workflow, config, or no
  persisted state.
- Put TOML schema and validation in the state owner, not in the command facade.
- Keep provider and tool calls behind `src/services/*`.
- Keep setup effects in `src/setup.rs`; pass it prepared inputs instead of
  letting setup discover workflow/task state.
- Keep human output, selector labels, and agent prompt text in rendering
  helpers when the text is longer than a local status line.
- Treat `.local/batches`, `.local/stacks`, `wt batch`, and `wt stack` as
  migration context unless a separate task explicitly changes that policy.
- Add or update tests at the layer that owns the behavior, and inspect public
  help text when command behavior changes.
