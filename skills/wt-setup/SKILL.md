---
name: wt-setup
description: "Use to initialize, audit, improve, or clean wt config: ownership, providers, prompts, workspace, workflow policy, profiles, and validation."
---

# WT Setup

Use this skill only for wt configuration: first setup, existing config audit,
safe edits, prompt/workspace recommendations, profile structure, and validation.
Do not start work, coordinate agents, land branches, or clean worktrees here.

## Check First

Check current syntax before giving exact commands:

```bash
wt init --help
wt config --help
wt config edit --help
wt config extract --help
wt config inline --help
wt profile create --help
wt doctor --help
```

For existing config:

```bash
find . .local -maxdepth 2 -name '.wt.toml' -o -name 'profile.toml' 2>/dev/null
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
- `recommendation`: ask only questions that affect config choices.
- `cleanup`: simplify comments, ordering, and formatting while preserving
  behavior unless the user asks for behavior changes.

Choose ownership:

- `.local/.wt.toml`: private checkout config, local paths, local agent commands,
  private runtime details, personal defaults.
- `.wt.toml`: shared project config for contributors.
- `.local/profiles/<name>/profile.toml`: named runtime profile only when the
  user wants reusable structured profile config.

Do not silently move settings between shared/private ownership or normalize a
mature config into one "correct" shape.

## New Config

`wt init` is a starter wizard. Canonical presets: `minimal`, `agent`, `issue`,
`app`.

Set only the choices the user has decided:

- target: `.local/.wt.toml` or `.wt.toml`
- preset: `minimal`, `agent`, `issue`, or `app`
- agent: `codex`, `claude`, `gemini`, or `none`
- issue provider: `github`, `linear`, or `none`
- site provider: `none`, `herd`, `valet`, `docker_proxy`, or `traefik`
- optional: worktree path, workspace tabs/colors, setup/test commands, editor,
  agent prompts

Preview before writing:

```bash
wt init --local --preset agent --agent codex --dry-run
wt init --local --preset agent --agent codex --yes
wt doctor
```

Bare `wt init --yes` uses the non-interactive default preset (`minimal`).
Use `--force` only after inspecting the existing target.

## Existing Config

Use the smallest safe edit:

- diagnose with `wt config` and `wt doctor`.
- use `wt config` output as the reference for what runtime behavior is active.
- edit the owning file, one scope at a time.
- use `wt config extract <source>` only when structured config is wanted.
- use `wt config inline <source>` only when inline config is wanted.
- use `wt init --dry-run` only as a reference starter shape.
- preserve user-authored prompt text unless copy editing was requested.

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

Use `[agent.prompt].common` for shared expectations, mode-specific
`issue`/`new`/`pr` for differences, and `[agent.prompt.append].common` for
final-report requirements. Avoid duplicating long common text across modes.

Workspace:

- Which tabs should open immediately?
- Which tabs should wait for setup via `[workspace].post_deps_tabs`?
- Do the built-in cmux colors need overrides? Defaults are `task`/`issue`
  blue, `new` green, and `pr` magenta.
- Should direct task, issue, new branch, or PR work use distinct cmux colors?
  Workflow color is workflow-level grouping, not a `[workspace].colors` key.

Use colors as visual hints only; do not encode lifecycle semantics in color
names. Do not add active `[workspace].colors` just to restate built-in
defaults; `wt config` shows the effective defaults when `[workspace]` is
configured. Keep init colors commented unless the user wants an override. Use an
empty string value, such as `task = ""`, to disable color for a kind. Do not add
site/dev-server tabs unless the project needs them.

Workflow policy:

- PR mode: `none`, `draft`, or `ready`
- landing mode: `manual` or `auto`

## Config Cleanup

Prefer a small active config over a tutorial file.

Remove comments that repeat key names, describe defaults already visible through
`wt config`, or mention old behavior. Keep comments that explain local intent or
non-obvious tradeoffs.

Preferred section order when it does not fight the existing structure:

1. `[issues]`
2. `[worktree]`
3. `[setup]`
4. `[workflow]`
5. `[workspace]`
6. `[site]`
7. `[agent]`
8. `[agent.prompt]`
9. `[agent.prompt.append]`

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
  `.local/profiles/<name>/profile.toml`.
- Do not combine `[profile] name` with inline `[profile.agent]`,
  `[profile.worktree]`, `[profile.setup]`, `[profile.workspace]`,
  `[profile.site]`, or `[profile.test]`.
- Omitted `--profile` means effective config; do not invent a `default`
  profile.

## Validate

After config changes:

```bash
wt config
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

For skill wiring or wt init behavior changes:

```bash
~/dotfiles/install.sh skills plan
cargo run --quiet -- init --help
cargo run --quiet -- doctor
cargo test init
```
