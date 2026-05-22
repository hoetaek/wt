---
name: wt-setup
description: "Use to inspect the current project and recommend an ideal wt config: ownership, active sections, omitted sections, commands, providers, workspace, workflow policy, profiles, and validation."
---

# WT Setup

Use this skill to produce a project-specific wt config recommendation. Do not
answer as a generic config manual. Do not list every possible field unless it is
relevant to the current repo. Do not start work, coordinate agents, land
branches, or clean worktrees.

## Core Job

Given a repo, inspect the actual project and recommend the smallest useful
active config for that project.

The recommendation must answer:

- what should go into the active config;
- which file should own it;
- what should stay out and why;
- what is already configured effectively;
- which local tools are missing for recommended commands;
- what commands validate the result.

Ask questions only for choices that cannot be inferred from repo facts and
would change the config.

## Inspect First

Run these before giving a concrete recommendation:

```bash
git rev-parse --path-format=absolute --git-common-dir
find . "$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)/wt" -maxdepth 3 \
  -name '.wt.toml' -o -name 'config.toml' -o -name 'profile.toml' 2>/dev/null
wt config
wt doctor
rg --files | rg '(^|/)(Cargo.toml|Cargo.lock|package.json|pnpm-lock.yaml|yarn.lock|bun.lockb?|pyproject.toml|uv.lock|composer.json|Gemfile|Makefile|justfile|Justfile|deny.toml|rust-toolchain.toml|\.github/workflows/.*\.ya?ml)$'
```

If this is the `wt` repo itself, also read `README.md`, `docs/consistency.md`,
and `docs/north-star.md` before recommending user-facing model changes.

Read the relevant manifests and CI workflows. Check local availability for
tools that existing config or recommended commands depend on: agent CLIs, cmux,
provider CLIs, test/lint/audit tools, browsers, and editors.

## Decide Ownership

Choose the file by ownership, not convenience:

- `<git-common-dir>/wt/config.toml`: personal repo config, local paths, local
  agent commands, private runtime details, personal defaults.
- `.wt.toml`: project integration config contributors should share.
- `<git-common-dir>/wt/profiles/<name>/profile.toml`: named runtime profile only
  when reusable structured profile config is worth the extra file.

Do not silently move settings between shared/private ownership. If existing
config is mature, recommend a minimal patch rather than normalizing it into one
ideal shape.

## Recommend Active Config

Recommend active TOML, not a commented tutorial file. Include only settings that
the project needs or the user explicitly chose.

Default recommendation rules:

- Keep `[workspace]` when the user benefits from repeatable cmux tabs.
- Add `[setup]` only when the repo has a real per-worktree install/sync step.
- Add `[test]` commands that mirror CI and are available locally.
- Add `[issues]` only when provider issue workflows are used.
- Add `[site]`, `[workspace.browser]`, and `workspace.post_deps_tabs` only for
  app/web repos with a local server or URL.
- Add `[editor]` only when a concrete editor command is useful for wt-managed
  TOML editing.
- Add `[worktree]` only for real path/copy/link/context needs.
- Add `[workflow]` only when future workflow PR/landing policy should differ
  from built-in defaults.
- Keep simple agent defaults inline under `[profile.agent]`; use named profiles
  only when prompt/scaffold/profile reuse is worth the structure.
- Do not add active `[workspace].colors` when it only restates built-in
  defaults.
- Do not add cargo-audit, cargo-deny, browsers, or provider helpers to active
  config if the tool is not installed, unless the user explicitly wants a config
  that assumes it will be installed.

## Explain Omissions

For every relevant section that could plausibly be expected, say whether to
keep, add, change, or omit it.

Cover these when relevant:

- `[workspace]`
- `[setup]`
- `[test]`
- `[issues]`
- `[site]`
- `[workspace.browser]`
- `[editor]`
- `[worktree]`
- `[workflow]`
- `[profile.agent]`
- named profiles

The omission rationale should be practical, for example: "CLI repo, no dev
server", "tool missing locally", "built-in default already covers this", or
"shared config would leak a personal path".

## Response Shape

Use this order:

1. Observed facts: project type, CI checks, detected commands, existing
   effective config, available/missing tools.
2. Recommended owner file.
3. Recommended active TOML.
4. Keep/add/omit rationale.
5. Unresolved choices, only if needed.
6. Validation commands.

Keep the answer concrete. Prefer a short active config block over a long field
catalog.

## Validation

After recommending or editing config, validate through the public interface:

```bash
wt config
wt doctor
```

If the recommendation includes named profiles, also run:

```bash
wt config --profile <name>
```

If this skill file or wt init behavior changes inside the `wt` repo, validate
with:

```bash
~/dotfiles/install.sh skills plan
cargo run --quiet -- init --help
cargo run --quiet -- doctor
cargo test init
```
