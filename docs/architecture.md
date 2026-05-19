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
| Task state | `src/task.rs` and `src/task_run.rs` | `TaskDocument` under `<git-common-dir>/wt/tasks` and `TaskRun` under `<git-common-dir>/wt/task-runs` | Workflow mode semantics, cmux coordinates, branch landing state |
| Workflow state store | `src/workflow.rs` today | `WorkflowMode`, `WorkflowMetadata`, `WorkflowTask`, `<git-common-dir>/wt/workflows` paths, validation, read/write/list/resolve, TOML rendering for workflow files | Starting worktrees, prompting agents, selecting runnable workflows, mutating TaskRun status beyond store validation |
| Workflow execution planner | `src/commands/workflow.rs` today; extract to `src/workflow/planner.rs` when split | Runnable-workflow selection, single/batch/stack next-step rules, preflight plans, parent-chain calculation | UI printing, cmux calls, issue-provider calls, file serialization |
| Workflow runner orchestration | `src/commands/workflow.rs` today; extract to `src/workflow/run.rs` when split | Coordinating planner output with `TaskDocument`, `TaskRun`, `issue` start paths, and setup results | Domain schema definitions, config merge policy, provider implementation details |
| Rendering | `ctx.ui` call sites and prompt builders in command modules today; extract workflow prompt/status text to `src/workflow/render.rs` when split | Human status text, selector labels, agent prompt snapshots, coordinator handoff text | State transitions, filesystem writes, shelling out to tools |
| Config layering | `src/config/` | Config schema, `.wt.toml` and `<git-common-dir>/wt/config.toml` load order, profile resolution, profile convention overlays, prompt merge/finalization | Worktree creation, site registration, TaskDocument/TaskRun/Workflow state |
| Setup side effects | `src/setup.rs` and `src/setup/*` | `run_setup` orchestration plus side-effect modules for files, env/template variables, site registration, cmux workspace runtime, agent bootstrap, dependency commands, post-deps tabs, background tests, local context injection, and setup summary | Config precedence, workflow planning, task state ownership |
| External services | `src/services/*` | Shell/tool boundaries for Git, GitHub, Linear, cmux, Herd, Valet, Docker proxy, Traefik, issue providers, and work-session observation | CLI policy, persisted wt state schemas, UX concept naming |

## Canonical State

`TaskDocument` is the reusable work definition. Its source-of-truth module is
`src/task.rs`, and its durable location is `<git-common-dir>/wt/tasks/<task>.toml`. It owns
title, branch, body, and optional origin. It does not own execution status,
workflow membership, profile choice, or cmux transport data.

`TaskRun` is one execution instance of a `TaskDocument`. Its source-of-truth
module is `src/task_run.rs`, and its durable location is
`<git-common-dir>/wt/task-runs/<id>.toml`. It owns task, branch, status, group, error,
creation_order, and timestamps. Workflow mode stays on `Workflow`; legacy
TaskRun `source` values are read only for migration compatibility and are not
written as canonical state. It records execution state, not branch landing or
review state.

`Workflow` is the saved prepared-work plan. Its current source-of-truth module
is `src/workflow.rs`, and its durable location is
`<git-common-dir>/wt/workflows/<id>.toml`. It owns title, body, optional workflow-level
origin for the larger issue-like unit, mode, base, profile, color, timestamps,
workflow-level effective policy, and `[[tasks]]` rows linking TaskDocuments to
TaskRuns. It does not copy task branch names, task status fields, TaskDocument
titles, TaskDocument bodies, TaskDocument origins, pull-request review state,
merge state, provider lifecycle state, or cleanup state; branch and slice-level
title/body/origin come from `TaskDocument`, execution status comes from
`TaskRun`, and actual landing remains an explicit Git/review workflow.
Workflow-level origin is context for the saved plan; PR issue-closing references
and provider start hooks are derived from TaskDocument origin only unless a
separate policy explicitly changes that contract.

`batch` and `stack` are Workflow modes. New prepared-work state belongs in
`<git-common-dir>/wt/workflows` with `mode = "single" | "batch" | "stack"`. Batch/stack
state directories are not ownership points, and top-level batch/stack command
shells should not exist beside the canonical `wt workflow` surface.

cmux is a runtime and surface integration. `src/services/cmux.rs` owns the
external command boundary; setup and command renderers may use current cmux
coordinates to open workspaces or print handoff instructions. `wt` owns
TaskDocument, TaskRun, and Workflow state. cmux workspace or surface coordinates
are transport details and must not become persisted task or workflow state.

Site providers are external services. `SiteConfig` and provider choice live in
`src/config/`; service dispatch lives in `src/services/site.rs`; provider
implementations live in `src/services/herd.rs`, `src/services/valet.rs`, and
`src/services/traefik.rs`, with no-op providers such as Docker proxy handled by
the dispatch layer. `src/setup/site.rs` may invoke them while preparing a
worktree, but provider modules should not decide CLI behavior or persisted wt
state shape.

## Workflow Refactor Target

Keep `src/commands/workflow.rs` as the command facade: validate command options,
select user input, load the relevant records, call planners/runners, and print
the final user-visible outcome.

Keep `src/workflow.rs` as the Workflow state facade. If it grows, turn it into a
`src/workflow/` module while preserving these boundaries:

- `src/workflow/model.rs`: `WorkflowMode`, `WorkflowMetadata`,
  `WorkflowTask`, `WorkflowRecord`, and pure validation of workflow shape.
- `src/workflow/store.rs`: `<git-common-dir>/wt/workflows` path resolution, read, list,
  create, write, migration-only target shortcuts while they exist, and
  workflow-file TOML rendering.
- `src/workflow/planner.rs`: runnable rules for single, batch, and stack modes,
  parent-chain planning, duplicate branch/path preflight, and next-task
  selection.
- `src/workflow/run.rs`: applying planner decisions to TaskDocument/TaskRun and
  invoking issue/worktree start paths.
- `src/workflow/render.rs`: selector labels, status summaries, task snapshots,
  and coordinator handoff text.

Task state follows the same direction at smaller scale: `src/task.rs` owns
`TaskDocument` and `<git-common-dir>/wt/tasks` TOML read/write rules, and `src/task_run.rs`
owns TaskRun and `<git-common-dir>/wt/task-runs` TOML read/write rules. If either grows,
split it into `model.rs` and `store.rs` under a directory module without moving
schema ownership back into command modules.

## Setup and Config Boundary

`src/config/` owns how configuration becomes effective configuration:
schema parsing, shared/personal load order, profile selection, profile directory
loading, convention overlays, and prompt merge/finalization. Commands may ask
whether a profile is valid and may pass an effective `Config` onward. They
should not duplicate merge rules or inspect profile directories directly.

Workflow preparation policy is config, not Workflow state, until a workflow is
prepared. `src/config/` should own parsing and merging `[workflow]` from
`.wt.toml` and `<git-common-dir>/wt/config.toml`, including built-in defaults for
`pull_request` and `landing`; workflow preparation should materialize the
effective values into `<git-common-dir>/wt/workflows/<id>.toml` for that run. Workflow state
owns the prepared policy snapshot shown by `wt workflow show`, not the config
precedence rules that produced it.

`src/setup.rs` is the setup facade and keeps `run_setup` as the orchestration
entrypoint after a worktree path, names, prompt context, and effective config are
already known. Setup child modules own the side-effect boundaries: file
copy/link, env substitution and template variables, site rendering/registration,
cmux workspace runtime and coordinator variables, agent bootstrap, dependency
commands, post-deps tabs, local context injection, background tests, and summary
rendering. Setup should not decide which TaskDocument or Workflow is runnable,
write TaskRun status, or define config precedence.

## Checklist for New Commands

Before adding a command or expanding an existing one:

- Name the user-facing concept in `docs/consistency.md` first if the UX model
  changes.
- Identify the state owner: TaskDocument, TaskRun, Workflow, config, or no
  persisted state.
- Put TOML schema and validation in the state owner, not in the command facade.
- Keep provider and tool calls behind `src/services/*`.
- Keep setup effects behind `src/setup.rs` and its child modules; pass prepared
  inputs instead of letting setup discover workflow/task state.
- Keep human output, selector labels, and agent prompt text in rendering
  helpers when the text is longer than a local status line.
- Treat batch/stack state directories and top-level command shells as outside
  the current model unless a separate task explicitly changes that policy.
- Add or update tests at the layer that owns the behavior, and inspect public
  help text when command behavior changes.
