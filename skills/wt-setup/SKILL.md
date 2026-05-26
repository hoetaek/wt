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

Read the relevant manifests, project docs, and CI workflows. Check local
availability for tools that existing config or recommended commands depend on:
agent CLIs, cmux, provider CLIs, test/lint/audit tools, browsers, secret
bootstrap tools, and editors.

For env/secret bootstrap, inspect only file names and command availability.
Never read or print secret file contents such as `.env`.

## Decide Ownership

Choose the file by ownership, not convenience:

- `<git-common-dir>/wt/config/local.toml`: personal repo config, local paths, local
  agent commands, private runtime details, personal defaults.
- `.wt.toml`: project integration config contributors should share.
- `<git-common-dir>/wt/config/profiles/<name>/profile.toml`: named runtime profile only
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
- Do not add framework-habit setup commands just because they are common. Only
  recommend commands proven by project docs, manifests, CI, existing config, or
  the user's stated workflow. For example, do not add
  `php artisan storage:link --force` to a Laravel project unless this repo uses
  it.
- Prefer per-worktree `.env` copy only in personal config when `.env` exists and
  the repo needs local env state. If docs mention a secret bootstrap tool but
  the tool is missing, recommend `.env` copy over a failing bootstrap command.
- For `[editor]`, recommend exactly one active command. Useful concrete choices
  include `vim {{path}}`, `code {{path}}`, `phpstorm {{path}}`, or
  `pstorm {{path}}`, depending on what the user chose and what is installed.
- Compact examples are allowed when they clarify a real project choice, but they
  must be project-shaped alternatives, not a general config manual.

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

## Diagnose Existing Setup Against Intent

Before recommending changes, reconcile three views: what files declare, what
`wt config` actually resolves, and what the user intends. Many setups fail
silently because the resolved effective config diverges from the written intent.

Run this triangulation pass:

1. **Read every config file that exists.** `.wt.toml`, `<git-common-dir>/wt/config/local.toml`,
   and every `<git-common-dir>/wt/config/profiles/*/profile.toml`. Also list
   `<git-common-dir>/wt/config/profiles/*/scaffold/` and `prompts/` to see what
   the profile expects to inject.
2. **Read the user's intent.** What did the user just paste, ask for, or
   describe? If unclear, ask one targeted question — do not guess.
3. **Run `wt config` and compare.** The effective output is the source of truth
   for what wt will actually apply. Diff it against the user's intent.

For every divergence, propose a concrete fix. Common divergences to check
explicitly:

- **Profile dormancy.** `<git-common-dir>/wt/config/profiles/<name>/` exists
  (with `profile.toml`, `scaffold/`, or `prompts/`) but `wt config` shows no
  `copy_as` pointing into that scaffold and no prompts from
  `profile.toml`/`prompts/*.md`. Cause: `.wt.toml`/`local.toml` is missing
  `[profile] name = "<name>"`, and no command passes `--profile <name>`. Fix:
  add `[profile] name = "<name>"` to the owner file, or remove the profile
  directory if it is unused.
- **Named profile + inline `[profile.agent.*]` collision.** When `[profile]` has
  both `name = "<name>"` and inline settings like `[profile.agent.prompt]`,
  `wt config` fails with a hard parse error (schema validation rejects the
  combination). Fix: pick one — drop `name` to use inline, or move the inline
  prompts into the named profile's `prompts/*.md` (or its own `profile.toml`'s
  `[agent.prompt]`) and delete the inline block.
- **Scaffold drift.** `[profile] name = "<name>"` is set and `wt config` shows
  the `copy_as` scaffold entry, but `scaffold/` is empty or missing the files
  the user expects. Fix: populate `<git-common-dir>/wt/config/profiles/<name>/scaffold/`
  with the actual files the worktree should receive, then re-run `wt config`.
- **Effective ≠ files in another way.** Any setting the user clearly intended
  (a prompt, a tab, a command) is absent from `wt config`. Trace which file
  owns it and why the merge dropped it (wrong section, wrong owner file,
  shadowed by profile, etc.).

If divergence is detected, lead the response with the divergence and the fix,
not with a generic recommendation.

## Response Shape

Use this order:

1. Observed facts: project type, CI checks, detected commands, existing
   effective config, available/missing tools.
2. Divergences between files, intent, and `wt config` — with concrete fixes
   (skip this item only when there is no existing setup).
3. Recommended owner file.
4. Recommended active TOML.
5. Keep/add/omit rationale.
6. Unresolved choices or compact project-shaped alternatives, only if needed.
7. Validation commands.

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
wt config | grep -E 'copy_as|\[agent' # named profile actually merged into effective?
```

If this skill file or wt init behavior changes inside the `wt` repo, validate
with:

```bash
~/dotfiles/install.sh skills plan
cargo run --quiet -- init --help
cargo run --quiet -- doctor
cargo test init
```
