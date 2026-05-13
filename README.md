# wt

`wt` is a Git worktree workspace manager for starting ready-to-code workspaces
from issues, pull requests, or branch names.

It can create worktrees, open cmux workspaces, apply setup templates, register
local development sites, and bootstrap an agent prompt.

## Install

`wt` is currently distributed from this repository:

```bash
cargo install --git https://github.com/hoetaek/wt
```

For local development:

```bash
git clone https://github.com/hoetaek/wt.git
cd wt
cargo install --path .
```

The crate is not published to crates.io. The `wt` package name is already used
there by another project.

## Requirements

Required:

- Rust 1.85 or newer to build from source
- Git

Optional integrations:

- `gh` for GitHub issue and pull request workflows
- `linear` for Linear issue workflows
- `cmux` for workspace/window automation
- Codex, Claude, Gemini, or another configured agent command
- Herd, Valet, Docker proxy, or Traefik for local site registration

Run this in a project after creating a config:

```bash
wt doctor
```

## Quick Start

Create a shared project config:

```bash
wt init --shared --agent codex --issue-provider github --yes
```

This writes shared project settings to `.wt.toml`, creates
`.local/.wt.toml`, and stores the default agent runtime inline.

Or keep the config local to your checkout:

```bash
wt init --local --agent codex --issue-provider github --yes
```

Start a workspace from an issue:

```bash
wt issue
wt issue PROJ-123
wt issue PROJ-123 --base .
```

Start a workspace from a pull request:

```bash
wt pr
wt pr 42
```

Start a workspace from branch-name text:

```bash
wt new add profile docs
```

Prepare issue work in bulk:

```bash
wt batch issue
wt batch run latest
wt stack issue
wt stack run latest
wt stack complete latest PROJ-123 --run-next
```

Open an existing worktree with its configured workspace:

```bash
wt open
```

List and clean worktrees:

```bash
wt list
wt done
wt done PROJ-123
```

## Configuration

`wt` loads config in this order:

1. `--config <path>`
2. `.wt.toml` as the shared base, then `.local/.wt.toml` as the private
   override

`.local/` is ignored by this repository and is intended for private profiles,
issue snapshots, and machine-specific settings.

Example shared `.wt.toml`:

```toml
[worktree]
path = "../{{repo}}-{{branch_slug}}"
inject_local_context = """
## env
- site: {{site_url}}
- workspace: {{workspace}}
- parent: {{parent_branch}}
"""

[issues]
provider = "github"

[site]
provider = "none"

[workspace]
tabs = ["lazygit", "nvim"]
```

`inject_local_context` is a rendered text block appended to the local agent
context file in the worktree. Codex uses `AGENTS.override.md`; Claude uses
`CLAUDE.local.md`. If the target file is not present, setup leaves it alone.

Example private `.local/.wt.toml`:

```toml
[profile.agent]
cli = "codex"
timeout = 30
send_after = 2
```

## Profiles

`profile` describes how a workspace should run. Profiles are execution
environments: agent CLI, args, prompt files, and agent-specific scaffold files.

For small default behavior, keep settings inline in `.local/.wt.toml`:

```toml
[profile.agent]
cli = "codex"
args = ["--yolo"]
```

Omitting `--profile` uses the effective config. `default` is not a profile
name, so default behavior is not stored as `profile = "default"`.

Explicit `--profile` has command-specific scope:

- `wt issue --profile <name>` and `wt new --profile <name>` create profiled
  worktrees. The branch and workspace names include the profile name so the
  profiled run is separate from the unprofiled workspace.
- `wt pr --profile <name>` applies the named profile config to the PR
  worktree. It uses the PR branch name as-is because the branch already
  exists.

When prompt or scaffold files are needed, move the profile into a named
directory and select it explicitly from config:

```toml
[profile]
name = "codex"
```

`[profile] name = "codex"` and inline `[profile.agent]` settings are mutually
exclusive. Use one representation at a time.

Profiles live under `.local/profiles/<name>/`:

```text
.local/profiles/codex/
  profile.toml
  prompts/
    issue.md
    new.md
    pr.md
  codex/
    AGENTS.override.md
    skills/
  claude/
    CLAUDE.local.md
    agents/
    commands/
```

Use `wt profile create <name>` to create a named profile scaffold. Use
`wt profile promote <name>` to move inline `[profile.*]` settings from
`.local/.wt.toml` into `.local/profiles/<name>/profile.toml` and replace them
with `[profile] name = "<name>"`.

The minimal `profile.toml` keeps runtime settings:

```toml
[agent]
cli = "codex"
args = []
timeout = 30
send_after = 2
```

`cli` selects how `wt` treats the agent. Supported values are `codex`,
`claude`, `gemini`, and `none`.

When the agent should run through a wrapper, keep `cli` as the agent behavior
and override only the launch command:

```toml
[agent]
cli = "codex"
command = "sandvault run -- codex"
```

Convention-based files are loaded when present for the matching `agent.cli`:

- `prompts/issue.md`, `prompts/new.md`, and `prompts/pr.md` become prompts for
  those modes.
- For `cli = "codex"`, `codex/AGENTS.override.md` is copied to the worktree
  root as `AGENTS.override.md`, and `codex/skills/` is copied to
  `.codex/skills/`.
- For `cli = "claude"`, `claude/CLAUDE.local.md` is copied to the worktree root
  as `CLAUDE.local.md`, and `claude/agents/`, `claude/commands/`, and
  `claude/skills/` are copied to `.claude/`.

Create a named profile scaffold:

```bash
wt profile create codex
wt profile
```

Or promote inline `.local/.wt.toml` profile settings into a named profile:

```bash
wt init --local --agent codex --no-prompts --yes
wt profile promote codex
wt profile
```

## Batches

Batches split planning from execution. `issue` snapshots issues and writes a
batch file without creating worktrees:

```bash
wt batch issue
wt batch issue 123 456 789
wt batch issue 123 456 789 --profile codex
```

When issue identifiers are omitted, `issue` opens the provider issue list and
lets you select multiple issues interactively.

When `--profile` is omitted, the batch does not store a profile field and uses
the effective config at run time. When `--profile <name>` is provided, the
batch stores that named profile.

For commands that create new branches, `--base .` uses the current branch
without prompting. `--base` with no value opens the local branch selector, and
`--base <branch>` uses the named branch explicitly.

Issue bodies are stored as markdown snapshots:

```text
.local/issues/123.md
.local/batches/2026-05-09-001.toml
```

The batch TOML records the optional profile, base mode, overall batch status,
and one `[[items]]` table per issue item. The double brackets are TOML's
array-of-tables syntax, equivalent to an `items: [...]` list in JSON. Issue
items use `kind = "issue"` and a `snapshot` path. Older batch files with
`[[issues]]` are still readable for compatibility.

Run a prepared batch explicitly:

```bash
wt batch run .local/batches/2026-05-09-001.toml
wt batch run latest
```

`run` executes only issue items with `status = "prepared"` or
`status = "failed"`. Items marked `done` or `skipped` are left alone, so reruns
can continue from the batch file's item status instead of checking a global
issue state.

## Stacks

Stacks are ordered work where each item branch is based on the previous
completed item branch.

Create a stack from branch-name text without creating worktrees:

```bash
wt stack new "add schema" "wire API" --base main
wt stack new "add schema" "wire API" --base .
```

`issue` snapshots provider issues and writes a stack file without creating
worktrees:

```bash
wt stack issue
wt stack issue 123 456 789 --base main
wt stack issue 123 456 789 --base .
wt stack issue 123 456 789 --base main --profile codex
```

When issue identifiers are omitted, `issue` opens the provider issue list,
lets you select multiple issues, then asks for the base-to-top order. When
identifiers are provided, their argument order is the stack order.

Both `new` and `issue` create stack TOML. `new` writes manual `kind = "new"`
items from branch-name text. `issue` writes `kind = "issue"` items from
provider issue snapshots.

You can also edit stack TOML directly:

```toml
# .local/stacks/manual.toml
base_mode = "explicit"
base = "main"
status = "prepared"

[[items]]
kind = "new"
title = "Add schema"
branch = "add-schema"
body = """
Create the schema first.
"""

[[items]]
kind = "new"
title = "Wire API"
branch = "wire-api"
body = """
Build on the schema branch.
"""
```

`[[items]]` is the canonical stack list. Issue-based `issue` also writes
`[[items]]` with `kind = "issue"` and a `snapshot` path. Older `[[issues]]`
stack files are still readable for compatibility.

Start the next runnable stack item:

```bash
wt stack run .local/stacks/2026-05-12-001.toml
wt stack run latest
```

`run` starts one prepared or failed item at a time and leaves it marked
`running`. The first item branch uses the stack base branch. Each following
item branch starts only after the previous item is completed, and uses that
previous item branch as its parent.

When `run` starts an item, the agent prompt includes the completion command:

```bash
wt stack complete .local/stacks/2026-05-12-001.toml 123 --run-next
```

`complete` verifies that the item branch has no uncommitted changes and has at
least one commit ahead of its parent before marking the running item `done`.
With `--run-next`, it then starts the next prepared or failed item
automatically:

```bash
wt stack complete latest 123 --run-next
```

Omit `--run-next` to mark one item done without starting another item.

The stack TOML records each item's `parent`, branch, and status so reruns can
continue from the stored state.

## Site Provider Helpers

`site` commands inspect the configured local site provider. For
`provider = "traefik"`, they also expose host-native Traefik setup helpers:

```bash
wt site doctor
wt site paths
wt site example-launchd
```

The default LaunchDaemon label is `wt.traefik`. For managed machines or
packaged installs, pass an organization-specific reverse-DNS label:

```bash
wt site example-launchd --label com.example.wt-traefik
```

## Development

Run the same checks as CI:

```bash
cargo fmt --all --check
cargo check --locked --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Security and dependency policy checks run in GitHub Actions with `cargo audit`
and `cargo deny` for license/source/bans policy.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
