# Changelog

All notable changes to this project will be documented in this file.

This project follows SemVer.

## Unreleased

- Added interactive multi-select for `wt batch prepare` when no issue
  identifiers are provided.
- Added `wt stack prepare`, `wt stack run`, and `wt stack complete` for
  ordered issue stacks where each issue branch is based on the previous
  completed issue branch.

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
