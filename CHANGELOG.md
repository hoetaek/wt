# Changelog

All notable changes to this project will be documented in this file.

This project follows pre-1.0 SemVer. Until the CLI, config, and persisted state
model are stable enough for 1.0, breaking user-facing changes bump the `0.x.0`
minor version instead of moving to `x.0.0`.

## Unreleased

- Removed hidden parser and dispatch shells for top-level `wt batch`,
  `wt stack`, `wt review`, and `wt status` names. Those names now use clap's
  unknown-subcommand handling instead of parsing only to return replacement
  guidance, while canonical behavior remains under `wt workflow`, `wt inspect`,
  and `wt agent status` / `wt agent watch`.
- Added `wt task import [<issue>...]` as the non-executing inverse of
  `wt task publish`. It imports existing provider issues into `.local/tasks`
  TaskDocuments, records `[origin]`, supports explicit issue ids and a bare
  provider issue selector, and refuses duplicate ids or existing local
  TaskDocument collisions.
- Changed workflow policy config to direct `[workflow]` fields
  `pull_request = "none" | "draft" | "ready"` and
  `landing = "manual" | "auto"`. The old nested policy table, legacy review
  landing value, approval boolean, and pull-request aliases now fail with
  guidance to the canonical fields.
- Changed prepared workflow files to snapshot pull-request and landing policy
  once at workflow level under `[policy]`; task rows no longer store redundant
  pull-request handoff intent.
- Bumped the package version to `0.26.0` because `wt task import` adds a new
  user-facing CLI subcommand and workflow config shape, workflow state shape,
  CLI help, and workflow prompt behavior changed while `wt` is still pre-1.0.

## 0.24.0 - 2026-05-17

- Added `wt workflow repair <workflow>` as an explicit preview-first repair
  surface for workflow runtime inconsistencies. Dry-run output recommends
  repairable TaskRun failure-state updates, and `--apply` marks those TaskRuns
  failed without closing cmux workspaces or removing worktrees.
- Changed the canonical read-only work dossier from `wt review [TARGET]` to
  `wt inspect [<target>]`. The previous `wt review` surface is hidden from
  primary help and now fails with inspect migration guidance instead of acting
  as a parallel canonical command.
- Moved agent runtime observation from top-level `wt status` to
  `wt agent status <target>` and `wt agent watch <target>`. The previous
  top-level `wt status` surface is hidden from primary help and now fails with
  explicit guidance for one-shot observation, polling, and human inspection.
- Added nested workflow default config materialization for prepared workflows.
  `wt workflow task` and `wt workflow issue` snapshot landing policy into
  `[policy]`, and stack-mode PR handoff defaults into task rows unless `--pr`
  explicitly overrides them.
- Added a task-run coordinator handoff to `wt task run` prompts. Task-run
  agents now receive coordinator cmux send coordinates and report `PR=none`
  before waiting for review, landing, and cleanup.
- Added a workflow-level coordinator handoff to every `wt workflow run` task
  prompt. Single, batch, grouped single, and stack prompts now all include the
  coordinator cmux send coordinates and the shared Agent Completion Report
  format; single and batch prompts report `PR=none`, while stack prompts keep
  their pull-request and `wt workflow complete ... --run-next` instructions.
- Fixed `wt send` so an interactively selected cmux surface is used instead of
  falling back to the first matching surface.
- Fixed runtime binding so Codex cmux `list-status` signals can identify a
  live agent even when the visible screen only shows model/status text such as
  `gpt-5.5 ... Working`.
- Bumped the package version to `0.24.0` because replacing `wt review` and
  top-level `wt status`, adding `wt workflow repair`, and materializing
  workflow defaults change the user-facing CLI/config contract while `wt` is
  still pre-1.0.

## 0.23.0 - 2026-05-17

- Changed bare `wt workflow run` to select runnable workflows from
  `.local/workflows`, auto-run the only runnable workflow, and fail
  non-interactive multiple-candidate runs with explicit rerun commands before
  mutating Workflow or TaskRun state.
- Fixed sequential `wt workflow run <workflow> --jobs 1` cancellation so later
  runnable batch tasks are not started after the user cancels one task.
- Added `wt task run [<task>...]` as the immediate TaskDocument execution
  surface. Explicit task keys start one worktree per TaskDocument, while bare
  `wt task run` opens the local TaskDocument selector.
- Removed prepared TaskDocument execution from `wt new --task`; `wt new` now
  only starts one ad hoc worktree from branch-name text and guides prepared
  task execution to `wt task run`.
- Bumped the package version to `0.23.0` because `wt workflow run` target
  omission and `wt task run` change the user-facing CLI contract while `wt` is
  still pre-1.0.

## 0.21.0 - 2026-05-16

- Added `wt status <TARGET>` and `wt --json status <TARGET>` for read-only
  polling of a branch, worktree path/name, or TaskRun with cmux workspace,
  surface, agent, status, last tool, session, warning, and fallback metadata.
  `needs_input` exits 2, `failed` exits 3, missing work exits 1, and cmux
  unavailability now fails instead of looking like a successful no-session
  poll.
- Added advisory `wt doctor` checks for Codex cmux hook readiness, including
  `hooks.json`, the Codex hooks feature flag, and trusted hook entries used by
  reliable Codex `wt status` polling.
- Bumped the package version to `0.21.0` because `wt status` adds a new
  user-facing CLI command while `wt` is still pre-1.0.

## 0.20.0 - 2026-05-16

- Changed interactive terminal prompts to a polished selector UI with
  filterable single-select and multi-select lists, compact ten-row menus,
  checkbox-style multi-selects, and actionable non-TTY prompt errors.
- Refined task, publish, worktree, branch, PR, batch, and stack selector labels
  so interactive rows show the selected resource and high-signal state instead
  of implementation-shaped file names.
- Added label/hint prompt items and a restrained cyan-accent cliclack theme so
  primary resource names and secondary metadata render with clearer visual
  hierarchy while remaining readable without color.
- Bumped the package version to `0.20.0` because prompt rendering and selector
  presentation changed the user-facing CLI surface while `wt` is still pre-1.0.

## 0.19.0 - 2026-05-16

- Changed stack TOML to record per-task `pull_request = true|false` handoff
  intent. Prepared stack tasks write `false` by default, `wt stack task
  --pull-request` and `wt stack issue --pull-request` write `true`, and stack
  task prompts now either require a draft PR against the stack parent branch or
  explicitly report `PR=none` based on that row value.
- Added cargo-dist release automation for GitHub Release artifacts, shell
  installer generation, Homebrew formula publishing, and GitHub artifact
  attestations.
- Bumped the package version to `0.19.0` because stack persisted state and task
  handoff behavior changed while `wt` is still pre-1.0.

## 0.18.0 - 2026-05-16

- Added canonical `wt init` starter presets with `--preset
  <minimal|agent|issue|app>`, `--minimal`, and `--dry-run` so the guided wizard
  and non-interactive init paths build the same typed generation plan before
  writing only the selected config file.
- Added repo detection to `wt init` plans for setup commands, app dev tabs, and
  test commands. Dry runs print the target file, selected preset, generated
  sections, detected signals, and TOML without writing files.
- Changed bare `wt init --yes` to choose the `minimal` preset; explicit options
  such as `--agent codex` still add their matching sections.
- Documented post-init structure paths: keep simple runtime settings inline in
  the selected config file, then use `wt config extract` or
  `wt profile create` when named profile directories or prompt/scaffold files
  are needed.
- Added multi-task `wt new` workspace runs. Repeat `--task <task>` with
  branch-name text, or use bare `wt new --task` to multi-select tasks and
  prompt for a workspace branch when more than one task is selected.
- Changed `wt review [TARGET]` to show every TaskRun that matches the selected
  branch, so a multi-task `wt new` workspace can be reviewed as one worktree
  with separate task records.
- Bumped the package version to `0.15.0` because multi-task `wt new` changes
  the user-facing CLI and TaskRun state model while `wt` is still pre-1.0.
- Changed bare `wt task publish` to open a multi-select list of unprocessed
  local TaskDocuments, removed the stack/batch publish selectors from the public
  command surface, and kept explicit `wt task publish <task>...` for scripts.
- Changed bare `wt stack run` to select from runnable stacks with semantic
  labels, kept explicit stack path/id targets for scripts, and removed
  `wt stack run latest` from the run contract.
- Bumped the package version to `0.16.0` because task publish and stack run
  changed user-facing CLI behavior while `wt` is still pre-1.0.
- Changed bare `wt batch run` to select from runnable batches with semantic
  labels, kept explicit batch path/id targets for scripts, and removed the
  previous `latest` target from the run contract.
- Bumped the package version to `0.18.0` because batch run and the final
  `wt init` starter wizard changed user-facing CLI behavior while `wt` is still
  pre-1.0.
- Changed stack task prompts to include conditional draft-PR handoff guidance:
  when the repository or coordinator workflow expects PR review, task agents
  push their branch, open a draft PR against the stack parent branch, and
  `wt send` a Completion Report with the PR URL back to the coordinator
  worktree before waiting for review. The coordinator still advances the stack
  with `wt stack complete --run-next`.
- Added `wt task publish` and `wt task publish <task>...` for publishing local
  TaskDocuments to the configured issue provider and writing created issue
  origins back to selected task files without creating TaskRuns or worktrees.
- Bumped the package version to `0.14.0` because `wt task publish` adds a new
  user-facing CLI command surface while `wt` is still pre-1.0.
- Added `wt send <TARGET> <MESSAGE...>` for sending a message to the cmux
  surface discovered by the same branch/worktree/TaskRun target model used by
  `wt review`.
- Bumped the package version to `0.12.0` because `wt send` adds a new
  user-facing CLI command while `wt` is still pre-1.0.
- Added `wt review [TARGET]` for read-only branch/worktree/TaskRun inspection
  before review or landing, including matching cmux workspace/surface handles
  when cmux can find the target worktree. Task-start prompts now also ask
  agents to return a compact completion report with summary, changed files,
  checks, and risks.
- Bumped the package version to `0.11.0` because `wt review` adds a new
  user-facing CLI command while `wt` is still pre-1.0.
- Added monotonic TaskRun `creation_order` values so latest-run selection is
  deterministic when multiple runs share the same timestamp second.
- Fixed partially successful profiled `wt new --task` starts so successful
  TaskRuns remain recorded when a later profile start fails.
- Fixed failed batch and stack preparation to roll back TaskRun records created
  before the preparation error.
- Bumped the package version to `0.11.2` for the TaskRun lifecycle fixes.
- Documented the explicit landing workflow for completed task branches:
  review, `complete`, `done`, merge into `master`, worktree and local branch
  cleanup, and local TaskDocument cleanup now stay separate in the README and
  consistency notes.
- Documented the canonical TaskDocument/TaskRun state model across README and
  consistency notes: TaskDocuments define reusable work under `.local/tasks`,
  TaskRuns record execution state under `.local/task-runs`, batch and stack
  rows link to TaskRuns instead of owning task status, `wt done` completes
  `new` and `batch` TaskRuns, and stack completion stays under
  `wt stack complete`.
- Bumped the package version to `0.10.0` because the TaskRun persisted state
  model is a breaking state-file contract change while `wt` is still pre-1.0.
- Changed stack task progress to use linked TaskRun records as the source of
  truth. `wt stack task` and `wt stack issue` now create stack TaskRuns during
  preparation, stack task rows keep only task ordering data plus the run id, and
  `wt stack run`, `complete`, and `show` derive task status and errors from
  those TaskRuns.
- Bumped the package version to `0.9.0` because the persisted stack state model
  changed while `wt` is still pre-1.0.
- Added TaskRun execution records under `.local/task-runs/<id>.toml` for
  prepared local tasks started by `wt new --task`, `wt batch run`, and
  `wt stack run`.
  Batch and stack files now keep their orchestration rows while each started
  task points at a readable run record with source, group, status, error, and
  timestamps.
- Bumped the package version to `0.6.0` because `wt new --task` and TaskRun add
  new user-facing CLI and persisted local state-file contracts while `wt` is
  still pre-1.0.
- Added `--jobs <N>` for bounded concurrent batch execution while keeping batch
  metadata writes coordinated through one writer.
- Added `wt batch clean [BATCH]` for explicitly deleting completed batch
  TaskDocument files while keeping the batch metadata record.
- Bumped the package version to `0.5.0`; current `wt` development is still
  pre-1.0 because CLI, config, and persisted state contracts are still being
  stabilized.
- Changed `wt pr` without explicit PR numbers to use the multi-select PR list
  and start each selected PR worktree sequentially. `wt pr 42` remains the
  explicit single-PR path, and `wt pr 42 43 44` starts multiple explicit PR
  worktrees in order.
- Added `wt new --task [<task-key>]` support for selecting one prepared local
  task from `.local/tasks/*.toml` and starting it with the same TaskDocument
  context used by batch and stack runs. Bare `wt new` is rejected so branch-name
  workspaces and prepared-task execution stay explicit.
- Renamed `wt issue --parallel` and `wt new --parallel` to `--matrix`, without a
  compatibility alias, to describe profile-matrix workspace creation instead of
  generic parallel execution.
- Changed `wt init` to create only the selected config file. After choosing
  `.wt.toml` or `.local/.wt.toml`, issue provider, site provider, agent runtime,
  and additional setup prompts all write to that selected file only. Named
  profile/prompt scaffold creation is left to `wt config extract` and
  `wt profile create`.
- Removed `wt init --prompts` and `wt init --no-prompts`; `wt init` now always
  writes inline `[profile.agent]` settings when an agent is configured.
- Expanded the generated `wt init` config scaffold with commented examples for
  worktree copy/link/context injection, setup environment, editor, workspace,
  and test command settings.
- Added an interactive path to `wt init` for frequently customized settings such
  as worktree path, workspace tabs, detected dependency setup, detected dev/test
  commands, and config editor.
- Added optional `working_dir` for `setup.deps` and `test.commands`, allowing
  setup and test commands to run inside subprojects. `wt init` now detects
  dependency manifests in subdirectories and can suggest `uv sync` for Python
  projects.
- Kept `if_exists` as an advanced optional guard, but stopped generating it for
  active `wt init` dependency and test commands so stale paths fail visibly.
- Added template rendering for agent `args` and `command`, including
  `{{repo_root}}` and `{{worktree_path}}`, so profiles can isolate per-worktree
  agent resources such as Chrome DevTools MCP browser data.
- Fixed `wt done` cmux cleanup so closing a matching workspace in another cmux
  window does not leave the caller on the wrong window.
- Added `wt done` cleanup that attempts to close open cmux workspaces for the
  same worktree path before removing their worktrees.
- Added the reserved `common` agent prompt scope, including
  `common.md`/`common.append.md` profile convention files, so shared
  instructions are prepended to `issue`, `new`, and `pr` prompts after all
  config layers are merged.
- Added `wt config extract [SOURCE]` for interactive one-step config
  refactors, including inline profile extraction and profile prompt file
  extraction.
- Added `wt config inline [SOURCE]` support for moving selected named profile
  settings back into `.local/.wt.toml`, alongside prompt convention file
  inlining.
- Added `[editor]` config for opening wt-managed files with a configurable
  command and placement.
- Added `wt config edit [SOURCE]`, `wt batch edit [BATCH]`, and
  `wt stack edit [STACK]` for opening config, batch, and stack TOML files.
- Removed `wt profile promote`; use `wt config extract .local/.wt.toml`
  instead.
- Added `wt config` to print the merged effective config, with
  `wt config --profile <name>` support for inspecting named profile layers.
- Changed named profile scaffold files to live under
  `.local/profiles/<name>/scaffold/`, copied onto the worktree root.
- Renamed batch issue preparation to `wt batch issue`.
- Added interactive multi-select for `wt batch issue` when no issue
  identifiers are provided.
- Changed `wt batch issue` to resolve and store the base branch during
  preparation, matching `wt issue` base prompt behavior.
- Changed `wt batch run` to require an explicit stored batch base before
  marking any task `running`, so base selection cannot strand a task in a
  stale running state.
- Changed issues prepared by batch and stack workflows to persist as
  `.local/tasks/*.toml` task documents, including the `worktree.naming` branch
  when branch naming is configured.
- Changed cmux workspace creation to target the caller's cmux window explicitly
  when caller context is available.
- Changed `wt open` to focus an existing cmux workspace for the selected
  worktree path, and to run the normal setup flow when it creates a worktree
  from a local or remote branch.
- Changed `wt list` and `wt done` to infer matching profile config from the
  branch suffix when rendering or unlinking local site URLs.
- Changed `wt batch show` to require batches with an explicit stored base,
  matching `wt batch run`.
- Changed batch and stack metadata parsing to require canonical `[[tasks]]`
  state instead of accepting alternate `[[issues]]` or previous `[[items]]`
  tables.
- Changed site config parsing to require canonical `[site] provider = "herd"`
  instead of accepting a separate `[herd]` section.
- Changed global `--json` to appear in help output for commands that support
  machine-readable output.
- Removed Traefik cleanup of old compatibility TLS config filenames.
- Changed stack runs so skipped tasks are not used as parent branches.
- Changed `wt issue` and `wt new` to reject empty base branch input.
- Added `wt batch task <TASK>...` to prepare local tasks without an issue
  provider.
- Added `wt batch show [BATCH]` for inspecting stored batch base, profile,
  status, and tasks without opening the TOML file.
- Added `wt stack task <TASK>...` to create manual branch stacks from
  branch-name text without an issue provider.
- Added `wt stack issue`, `wt stack run`, and `wt stack complete` for
  ordered issue stacks where each issue branch is based on the previous
  completed issue branch.
- Changed `wt stack task` and `wt stack issue` to resolve and store the base
  branch during preparation.
- Added `wt stack show [STACK]` for inspecting stored stack base, profile,
  status, tasks, and parent chain without opening the TOML file.
- Generalized batch and stack state to canonical `[[tasks]]` entries that
  reference `.local/tasks/*.toml` task documents.

## Historical Notes

The entries below predate the current pre-1.0 version reset and are
kept as internal development history, not package release ordering.

### Former Pre-Reset 0.4.0

- Changed config loading to merge `.wt.toml` as the shared base with
  `.local/.wt.toml` as the private override.
- Added early agent-oriented init scaffolding that was later replaced by the
  selected-config-only `wt init` model documented above.

### Former 0.3.0

- Added `[profiles] default = "..."` support for default `wt start` profile
  selection.

### Former 0.2.1

- Added open-source project metadata and licensing files.
- Documented installation, requirements, configuration, and development checks.
- Changed the default Traefik LaunchDaemon label to avoid maintainer-specific
  namespaces.

### Former 0.2.0

- Added Traefik site provider support.
- Reworked agent profiles and batch workflows.
- Added GitHub and Linear issue provider support.
