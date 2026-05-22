---
name: wt-setup
description: "Use to initialize, audit, improve, or clean wt setup/config: per-machine integration, ownership, providers, prompts, workspace, workflow policy, profiles, and validation."
---

# WT Setup

Use this skill only for wt setup and configuration: per-machine integration,
first config, existing config audit, safe edits, prompt/workspace
recommendations, profile structure, and validation. Do not start work,
coordinate agents, land branches, or clean worktrees here.

## Check First

Check current syntax before giving exact commands:

```bash
wt init --help
wt config --help
wt config edit --help
wt config extract --help
wt config inline --help
wt profile create --help
wt setup --help
wt shell-init --help
wt completion --help
wt doctor --help
```

For existing config:

```bash
git rev-parse --path-format=absolute --git-common-dir
find . "$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)/wt" -maxdepth 3 \
  -name '.wt.toml' -o -name 'config.toml' -o -name 'profile.toml' 2>/dev/null
wt config
wt doctor
```

Treat `wt config` as the effective source of truth for runtime behavior. The
files store user intent and overrides; `wt config` shows merged layers plus
built-in defaults in the shape the user should copy and edit.

Inside the `wt` repo, read `README.md` and `docs/consistency.md` before
changing docs or behavior.

## Decide

Classify the request:

- `new config`: use `wt init`; preview with `--dry-run`.
- `existing config`: use `wt config`, `wt config edit`, `wt config extract`,
  or `wt config inline`; do not use `wt init` as a repair tool.
- `per-machine setup`: use `wt setup`; preview with `--dry-run`, and remove
  only wt-managed per-machine entries with `wt setup --remove`.
- `recommendation`: ask only questions that affect config choices.
- `cleanup`: simplify comments, ordering, and formatting while preserving
  behavior unless the user asks for behavior changes.

Choose ownership:

- `<git-common-dir>/wt/config.toml`: personal repo config, local paths, local
  agent commands, private runtime details, personal defaults.
- `.wt.toml`: team integration/project config for contributors.
- `<git-common-dir>/wt/profiles/<name>/profile.toml`: named runtime profile
  only when the user wants reusable structured profile config.

Do not silently move settings between shared/private ownership or normalize a
mature config into one "correct" shape.

## Per-Machine Setup

`wt setup` configures one user's machine. It is separate from repo config and
does not create `.wt.toml`, `<git-common-dir>/wt/config.toml`, worktrees, or
tracked source.

Preview before writing:

```bash
wt setup --dry-run
wt setup --yes
wt setup --remove --dry-run
wt setup --remove --yes
```

Current setup steps are:

- user-level Claude inbox hooks when the `claude` CLI is detected.
- user-level Codex inbox hooks, Codex hook trust state, and Codex hooks feature
  enablement when the `codex` CLI is detected.
- shell integration eval line for zsh or bash so worker shells get ambient
  `WT_AGENT_ID` and `WT_COORDINATOR_AGENT_ID`.
- shell completion eval line for non-Homebrew installs. Homebrew-provided `wt`
  skips this because the formula owns completion.

Each write step prompts with default No unless `--yes` is passed. `--dry-run`
prints planned writes without changing files. `--remove` removes only
wt-managed per-machine entries; preserve user-authored hooks, cmux hooks,
unrelated Codex trust state, repo config, personal wt state, worktrees, and the
wt binary.

Unsupported shells should receive manual eval-line guidance, not file edits.
Do not treat `wt doctor`, `wt msg`, or `wt agent status/watch` as installers;
they may report missing setup, but hook installation belongs to explicit
`wt setup`.

## New Config

`wt init` is a starter wizard. Canonical presets: `minimal`, `agent`, `issue`,
`app`.

Set only the choices the user has decided:

- target: `<git-common-dir>/wt/config.toml` or `.wt.toml`
- preset: `minimal`, `agent`, `issue`, or `app`
- agent: `codex`, `claude`, `gemini`, or `none`
- issue provider: `github`, `linear`, or `none`
- site provider: `none`, `herd`, `valet`, `docker_proxy`, or `traefik`
- optional: agent args/command, GitHub user filter, worktree path, workspace
  tabs/colors/browser, setup/test commands, editor, agent prompts

Preview before writing:

```bash
wt init --local --preset agent --agent codex --dry-run --yes
wt init --local --preset agent --agent codex --yes
wt init --shared --preset issue --issue-provider github --dry-run --yes
wt doctor
```

Bare `wt init --yes` uses the non-interactive default preset (`minimal`).
With `--yes`, the default target is `<git-common-dir>/wt/config.toml`; use
`--shared` only when the settings should be committed for contributors.
Use `--minimal` as the explicit shortcut for that preset. In non-TTY
automation, combine `--dry-run` with `--yes` or pass every prompt-affecting
choice explicitly.
Use `--force` only after inspecting the existing target. Interactive `wt init`
may prompt to add Claude local allow rules for `.git/wt/**`; `--yes` skips that
prompt.

## Existing Config

Use the smallest safe edit:

- diagnose with `wt config` and `wt doctor`.
- use `wt config` output as the reference for what runtime behavior is active.
- edit the owning file, one scope at a time.
- use `wt config extract <source>` only when structured config is wanted.
- use `wt config inline <source>` only when inline config is wanted.
- use `wt init --dry-run --yes` only as a reference starter shape unless every
  prompt-affecting choice is passed explicitly.
- preserve user-authored prompt text unless copy editing was requested.
- config refactors support `.wt.toml`, `<git-common-dir>/wt/config.toml`,
  `<git-common-dir>/wt/profiles/<name>/profile.toml`, and profile prompt files.
  Legacy `prompts/new.md` and `prompts/new.append.md` are not supported; use
  `branch.md` and `branch.append.md`.

## Recommendation Questions

Prompts:

- What should every agent read before acting? Examples: `AGENTS.md`,
  conventions, architecture notes.
- For issue work, what should happen before coding? Examples: inspect issue
  context, read docs, make a short plan, run checks.
- For PR review, what should be prioritized? Examples: correctness,
  regressions, tests, security, UX consistency, migration risk.
- What should the completion report include? Examples: checks, risks,
  conventions applied, files changed, follow-ups.
- Are there existing prompts or reports worth reusing?

Use `[agent.prompt].common` for shared expectations and mode-specific
`issue`/`branch`/`pr` for direct setup-mode differences. Use
`[agent.prompt].workflow` only for tasks started by `wt run workflow`; it is a
separate workflow-started task scope, not a workflow mode. Use
`[agent.prompt.append].common` for direct-mode final-report requirements and
`[agent.prompt.append].workflow` for workflow-specific reporting. `common`
expands into `issue`/`branch`/`pr`, not into `workflow`. Avoid duplicating long
common text across modes.

Workspace:

- Which tabs should open immediately?
- Which tabs should wait for setup via `[workspace].post_deps_tabs`?
- Do the built-in cmux colors need overrides? Defaults are `task`/`issue`
  blue, `branch` green, and `pr` magenta.
- Should direct task, issue, branch-name text, or PR work use distinct cmux colors?
  Workflow color is workflow-level grouping, not a `[workspace].colors` key.
- Should setup open a browser via `[workspace.browser]`? Use `system` for a
  normal browser handoff and `chrome_devtools` only when an isolated debuggable
  Chrome instance is intended.

Use colors as visual hints only; do not encode lifecycle semantics in color
names. Do not add active `[workspace].colors` just to restate built-in
defaults; `wt config` shows the effective defaults when `[workspace]` is
configured. Keep init colors commented unless the user wants an override. Use an
empty string value, such as `task = ""`, to disable color for a kind. Do not add
site/dev-server tabs unless the project needs them.

Browser launch:

- Browser launch policy belongs to `[workspace.browser]`, not `[site]`.
- Use `mode = "system"` for the user's normal browser. `app` is valid only in
  system mode.
- Use `mode = "chrome_devtools"` to launch an isolated debuggable Chrome
  session. `[workspace.chrome_devtools]` only tunes that mode; it has no
  `enabled` or `url` field.

Minimal examples:

```toml
[workspace.browser]
mode = "system" # none | system | chrome_devtools
# url = "{{site_url}}"
# app = "Google Chrome"
```

```toml
[workspace.browser]
mode = "chrome_devtools"
# url = "{{site_url}}"

[workspace.chrome_devtools]
# port = 9222
# user_data_dir = "{{worktree_parent}}/.chrome-devtools/{{worktree_name}}"
```

Prefer leaving `port` unset for normal multi-worktree use; setup reserves an
available localhost port at runtime. Do not set `port = 0`. Set a stable port
only when the user explicitly needs it and accepts concurrent worktree
conflicts. Setup exposes `{{chrome_debug_port}}`, `{{chrome_debug_url}}`, and
`{{chrome_user_data_dir}}` after Chrome DevTools is prepared. Use those values
in setup templates, post-deps tabs, local context, or agent bootstrap instead
of hardcoding a port.

Do not point Chrome DevTools `user_data_dir` at the user's normal browser
profile. The built-in default is `{{worktree_parent}}/.chrome-devtools/{{worktree_name}}`,
outside the repository checkout when the default sibling worktree layout is
used.

Workflow policy:

- PR mode: `none`, `draft`, or `ready`
- landing mode: `manual` or `auto`

## Config Cleanup

Prefer a small active config over a tutorial file.

Remove comments that repeat key names, describe defaults already visible through
`wt config`, or mention old behavior. Keep comments that explain local intent or
non-obvious tradeoffs.

Preferred section order when it does not fight the existing structure:

Order sections from least likely to change to most likely to change: stable
repo/provider ownership first, project execution contract next, local tool and
workspace preferences after that, active profile selection near the bottom, and
long prompt text last.

1. `[issues]`
2. `[worktree]`
3. `[site]`
4. `[setup]`
5. `[test]`
6. `[workflow]`
7. `[editor]`
8. `[workspace]`
9. `[workspace.browser]`
10. `[workspace.chrome_devtools]`
11. `[profile]` or `[profile.agent]` in `.wt.toml` / `<git-common-dir>/wt/config.toml`
12. `[agent]`, `[agent.prompt]`, and `[agent.prompt.append]` in profile files

Keep identity/provider fields before optional tuning fields. Preserve arrays
and prompt blocks exactly unless the user requested copy editing.

## Workflow Policy

Workflow config is preparation policy for future workflows:

```toml
[workflow]
pull_request = "none"  # none | draft | ready
landing = "manual"     # manual | auto
```

Built-in defaults are `pull_request = "none"` and `landing = "manual"`.
Do not silently enable pull request creation or automatic landing.

Changing `[workflow]` affects workflows prepared after the edit. It does not
reinterpret existing workflow TOML, running agents, review state, merge
ancestry, cleanup, or TaskRun status.

## Profile Rules

- Keep simple defaults inline under `[profile.agent]`.
- Use `[profile] name = "<name>"` only to select
  `<git-common-dir>/wt/profiles/<name>/profile.toml`.
- Do not combine `[profile] name` with inline `[profile.agent]`,
  `[profile.worktree]`, `[profile.setup]`, `[profile.workspace]`,
  `[profile.site]`, or `[profile.test]`.
- `profile.toml` itself must not contain `[profile]`; it is already the named
  profile layer.
- Omitted `--profile` means effective config; do not invent a `default`
  profile.
- `wt profile create <name>` creates the profile directory, `profile.toml`,
  prompt files for `issue`/`branch`/`pr`, and agent-specific scaffold files
  when applicable. Use it when that structure is wanted; keep simple personal
  defaults inline under `[profile.agent]`.

## Validate

After config changes:

```bash
wt config
wt config --profile <name>
wt doctor
```

Common checks:

- agent does not launch: confirm agent, cmux readiness, and active workspace
  config in `wt doctor`.
- issue commands fail: check `[issues] provider` and provider auth outside wt.
- site URLs are wrong: run `wt site doctor` and verify provider/service state.
- config differs from expectation: inspect merged layers with `wt config`.
- workflow behavior differs from expectation: inspect `[workflow]`; it only
  affects future workflow preparation.
- per-machine hooks or shell integration differ from expectation: run
  `wt setup --dry-run`, then `wt setup --yes` or `wt setup --remove --yes`
  only after reviewing the planned entries.

For skill wiring or wt init behavior changes:

```bash
~/dotfiles/install.sh skills plan
cargo run --quiet -- init --help
cargo run --quiet -- doctor
cargo test init
```
