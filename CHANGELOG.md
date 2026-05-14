# Changelog

All notable changes to this project will be documented in this file.

This project follows SemVer.

## Unreleased

- Added the reserved `common` agent prompt scope, including
  `common.md`/`common.append.md` profile convention files, so shared
  instructions are prepended to `issue`, `new`, and `pr` prompts after all
  config layers are merged.
- Added `wt config extract [SOURCE]` for interactive one-step config
  refactors, including inline profile extraction and profile prompt file
  extraction.
- Removed `wt profile promote`; use `wt config extract .local/.wt.toml`
  instead.
- Added `wt config` to print the merged effective config, with
  `wt config --profile <name>` support for inspecting named profile layers.
- Changed named profile scaffold files to live under
  `.local/profiles/<name>/scaffold/`, copied onto the worktree root.
- Renamed batch issue snapshot creation to `wt batch issue`; `wt batch prepare`
  remains as a hidden compatibility alias.
- Added interactive multi-select for `wt batch issue` when no issue
  identifiers are provided.
- Added `wt stack new` to create manual branch stacks from branch-name text
  without an issue provider.
- Added `wt stack issue`, `wt stack run`, and `wt stack complete` for
  ordered issue stacks where each issue branch is based on the previous
  completed issue branch.
- Generalized stack state to canonical `[[items]]` entries so stack TOML can
  be authored directly without an issue provider, while keeping legacy
  `[[issues]]` stack files readable.

## 0.4.0

- Changed config loading to merge `.wt.toml` as the shared base with
  `.local/.wt.toml` as the private override.
- Changed `wt init --agent <agent>` to create a default profile under
  `.local/profiles/<agent>/` and set `[profiles] default = "<agent>"`.

## 0.3.0

- Added `[profiles] default = "..."` support for default `wt start` profile
  selection.

## 0.2.1

- Added open-source project metadata and licensing files.
- Documented installation, requirements, configuration, and development checks.
- Changed the default Traefik LaunchDaemon label to avoid maintainer-specific
  namespaces.

## 0.2.0

- Added Traefik site provider support.
- Reworked agent profiles and batch workflows.
- Added GitHub and Linear issue provider support.
