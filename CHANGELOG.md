# Changelog

All notable changes to this project will be documented in this file.

This project follows pre-1.0 SemVer. Until the CLI, config, and persisted state
model are stable enough for 1.0, breaking user-facing changes bump the `0.x.0`
minor version instead of moving to `x.0.0`.

## Unreleased

- Added `wt batch clean [BATCH]` for explicitly deleting completed batch task
  snapshot files while keeping the batch metadata record.
- Changed the package version line back to `0.3.x`; current `wt` development is
  still pre-1.0 because CLI, config, and persisted state contracts are still
  being stabilized.
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
  state instead of accepting alternate `[[issues]]` or legacy `[[items]]`
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

The entries below predate the current `0.3.x` pre-1.0 version reset and are
kept as internal development history, not package release ordering.

### Former 0.4.0

- Changed config loading to merge `.wt.toml` as the shared base with
  `.local/.wt.toml` as the private override.
- Changed `wt init --agent <agent>` to create a default profile under
  `.local/profiles/<agent>/` and set `[profiles] default = "<agent>"`.

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
