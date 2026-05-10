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
`.local/profiles/codex/`, and stores your local default profile in
`.local/.wt.toml`.

Or keep the config local to your checkout:

```bash
wt init --local --agent codex --issue-provider github --yes
```

Start a workspace from an issue:

```bash
wt start 123
wt start issue PROJ-123
```

Start a workspace from a pull request:

```bash
wt start pr
wt start pr 42
```

Start a workspace from branch-name text:

```bash
wt start add profile docs
```

Open an existing worktree with its configured workspace:

```bash
wt open
```

List and clean worktrees:

```bash
wt list
wt done
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

[issues]
provider = "github"

[site]
provider = "none"

[workspace]
tabs = ["lazygit", "nvim"]
```

Example private `.local/.wt.toml`:

```toml
[profiles]
default = "codex"
```

## Profiles

`profile` describes how a workspace should run. Profiles are execution
environments: agent CLI, args, prompt files, and agent-specific scaffold files.

Set a default profile when most `wt start` runs should use one profile:

```toml
[profiles]
default = "codex"
```

`wt start 123` then behaves like `wt start 123 --profile codex`. Explicit
`--profile` still wins, and `--parallel` still starts every profile.

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

`wt init --agent <agent>` creates the matching profile automatically. Use
`wt profile <name>` when you want to add another profile later.

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

Create or list profiles:

```bash
wt init --local --agent codex --yes
wt profile codex
wt profile
```

## Batches

Batches split planning from execution. `prepare` snapshots issues and writes a
batch file without creating worktrees:

```bash
wt batch prepare 123 456 789
wt batch prepare 123 456 789 --profile codex
```

When `--profile` is omitted, the batch records `profile = "default"` and uses
the current effective config at run time.

Issue bodies are stored as markdown snapshots:

```text
.local/issues/123.md
.local/batches/2026-05-09-001.toml
```

The batch TOML records the profile, base mode, overall batch status, and one
`[[issues]]` table per issue. The double brackets are TOML's array-of-tables
syntax, equivalent to an `issues: [...]` list in JSON.

Run a prepared batch explicitly:

```bash
wt batch run .local/batches/2026-05-09-001.toml
wt batch run latest
```

`run` executes only issue items with `status = "prepared"` or
`status = "failed"`. Items marked `done` or `skipped` are left alone, so reruns
can continue from the batch file's item status instead of checking a global
issue state.

## Traefik Provider

The Traefik site provider is intended for local HTTPS routing on macOS. Inspect
expected paths and defaults with:

```bash
wt traefik paths
wt traefik example-launchd
wt traefik doctor
```

The default LaunchDaemon label is `wt.traefik`. For managed machines or
packaged installs, pass an organization-specific reverse-DNS label:

```bash
wt traefik example-launchd --label com.example.wt-traefik
```

## Development

Run the same checks as CI:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Security audit runs in GitHub Actions with `cargo audit`.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
