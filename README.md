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

## Versioning

`wt` is still pre-1.0. Until the CLI, config format, and persisted state files
stabilize, breaking user-facing changes are represented as `0.x.0` minor
bumps, not `x.0.0` major releases.

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

This writes the selected settings to `.wt.toml` only.

Create the same shape as private config for this checkout:

```bash
wt init --local --agent codex --yes
```

This writes the selected settings to `.local/.wt.toml` only.

Or keep the config local to your checkout:

```bash
wt init --local --agent codex --issue-provider github --yes
```

`wt init` only creates the selected config file. It does not create named
profile directories or prompt scaffold files; use `wt config extract` or
`wt profile create` when you want that structure.

Start a workspace from an issue:

```bash
wt issue
wt issue PROJ-123
wt issue PROJ-123 --base .
```

Start workspaces from pull requests:

```bash
wt pr
wt pr 42
wt pr 42 43 44
```

When pull request numbers are omitted, `pr` opens the GitHub PR list and lets
you select multiple PRs interactively. Each selected PR starts sequentially.

Start a workspace from branch-name text:

```bash
wt new add profile docs
```

Prepare issue work in bulk:

```bash
wt batch issue
wt batch run latest
wt stack issue
wt stack show latest
wt stack run latest
wt stack complete latest PROJ-123 --run-next
```

Open an existing worktree, or pick a local/remote branch and create its worktree:

```bash
wt open
```

When `wt open` creates a worktree from a local or remote branch, it applies the
same setup flow as `wt new`: copy/link files, env templates, deps, site setup,
workspace tabs, and agent launch. When a cmux workspace already exists for the
selected worktree path, `wt open` focuses that workspace instead of opening a
duplicate.

List and clean worktrees:

```bash
wt list
wt done
wt done PROJ-123
```

When a cmux workspace was opened for the same worktree path, `wt done` attempts
to close it before removing the worktree.

## Configuration

`wt` loads config in this order:

1. `--config <path>`
2. `.wt.toml` as the shared base, then `.local/.wt.toml` as the private
   override

`.local/` is ignored by this repository and is intended for private profiles,
prepared tasks, and machine-specific settings.

Inspect the effective config after shared, local, profile, and convention-file
layers are merged:

```bash
wt config
wt config --profile codex
```

Refactor config representation one source file at a time:

```bash
wt config extract
wt config extract .local/.wt.toml
wt config extract .local/profiles/codex/profile.toml
wt config inline
wt config inline .local/.wt.toml
wt config inline .local/profiles/codex/profile.toml
wt config inline .local/profiles/codex/prompts/issue.md
wt config edit
wt config edit .local/.wt.toml
```

The merge order is `.wt.toml`, `.local/.wt.toml`, the selected profile, then
profile convention files such as `.local/profiles/<name>/prompts/issue.md`.
Later layers override scalar and map entries with the same key. List-style
entries such as `worktree.copy` append unique values. Agent prompts follow an
explicit rule: `[agent.prompt]` overwrites the prompt for that mode, while
`[agent.prompt.append]` appends text to the current prompt. The reserved
`common` prompt scope is not a runnable mode; after all layers are merged, it
is prepended to the effective `issue`, `new`, and `pr` prompts. `wt config`
prints the effective mode prompts after append directives and `common` have
been applied.

`wt config extract` is not an effective-config splitter. It moves selected
sections from a real source file into the next structured file while preserving
the local effective behavior.

`wt config inline` moves selected structured config back toward the current
source file while preserving the local effective behavior. It can move the
selected named profile from `.local/profiles/<name>/profile.toml` back into
`.local/.wt.toml` as `[profile.*]`, and it can move profile prompt convention
files back into the profile's `profile.toml`. The supported prompt-file scope
is `.local/profiles/<name>/prompts/{common,issue,new,pr}.md` and matching
`.append.md` files. It refuses to overwrite an existing `[agent.prompt]` or
`[agent.prompt.append]` key. A named profile is not inlined while supported
prompt convention files or scaffold files would be lost; inline prompt files
first. Scaffold files are not inlined.

`wt config edit` opens an existing config file in your configured editor. When
no source is provided, it lists `.wt.toml`, `.local/.wt.toml`, and named
profile `profile.toml` files; if no config file exists, it opens
`.local/.wt.toml`.

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

[setup]
deps = [
    { run = "npm install" },
    { run = "composer install" },
    { working_dir = "api", run = "uv sync" },
]

[site]
provider = "none"

[workspace]
tabs = ["lazygit", "nvim"]
```

`inject_local_context` is a rendered text block appended to the local agent
context file in the worktree. Codex uses `AGENTS.override.md`; Claude uses
`CLAUDE.local.md`. If the target file is not present, setup leaves it alone.

In `setup.deps`, `working_dir` runs the command inside a subdirectory. Add
`if_exists` only for intentionally optional commands; when `working_dir` is set,
that guard is checked relative to the same directory.

Example private `.local/.wt.toml`:

```toml
[profile.agent]
cli = "codex"
timeout = 30
send_after = 2

[editor]
command = "vi {{path}}"
placement = "cmux_surface"
```

`[editor]` config controls commands that open wt-managed TOML files. `command`
is rendered with `{{path}}` as a shell-quoted path and `{{path_raw}}` as the raw
path. If the command has no path placeholder, wt appends `{{path}}`. Omit
`command` to use `$VISUAL`, `$EDITOR`, or `vi {{path}}`. `placement` is
`cmux_surface` by default, opening a new cmux surface in the caller workspace;
use `process` for commands such as `code {{path}}`.

## Profiles

`profile` describes how a workspace should run. Profiles are execution
environments: agent CLI, args, prompt files, and worktree scaffold files.

For small default behavior, keep settings inline in the selected config file
and use `.local/.wt.toml` for checkout-private settings:

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
- `wt pr --profile <name>` applies the named profile config to every selected
  PR worktree. It uses each PR branch name as-is because the branch already
  exists.

Use `--matrix` on `wt issue` or `wt new` to expand one issue or branch-name
input across all named profiles. `wt issue 123 --matrix` creates one issue
worktree per profile, while `wt new add search --matrix` creates one branch
worktree per profile from the `add-search` branch-name seed.

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
    common.md
    common.append.md  # optional append for every mode
    issue.md
    issue.append.md  # optional append
    new.md
    pr.md
  scaffold/
    AGENTS.override.md
    .codex/
      skills/
```

Use `wt profile create <name>` to create a named profile scaffold. Use
`wt config extract .local/.wt.toml` to move inline `[profile.*]` settings from
`.local/.wt.toml` into `.local/profiles/<name>/profile.toml` and replace them
with `[profile] name = "<name>"`. The profile name is requested when the
extract plan is built.

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

`args` and `command` are rendered with the same worktree template variables
used by setup values, including `{{repo_root}}`, `{{worktree_path}}`, and
`{{branch_slug}}`. This can isolate per-worktree agent resources without a
wrapper script. For example, a Codex profile can give Chrome DevTools MCP a
separate browser profile per worktree:

```toml
[agent]
cli = "codex"
args = [
  "--yolo",
  "-c", "mcp_servers.chrome-devtools.command=\"npx\"",
  "-c", "mcp_servers.chrome-devtools.args=[\"chrome-devtools-mcp@latest\",\"--user-data-dir={{repo_root}}/.local/chrome-devtools/{{branch_slug}}\"]",
]
```

If persistent browser state is not needed, use Chrome DevTools MCP's temporary
profile mode instead:

```toml
[agent]
cli = "codex"
args = [
  "--yolo",
  "-c", "mcp_servers.chrome-devtools.command=\"npx\"",
  "-c", "mcp_servers.chrome-devtools.args=[\"chrome-devtools-mcp@latest\",\"--isolated\"]",
]
```

Convention-based files are loaded when present:

- `prompts/common.md` overwrites the shared prompt scope for all modes.
- `prompts/common.append.md` appends text to the current shared prompt scope.
- `prompts/issue.md`, `prompts/new.md`, and `prompts/pr.md` become prompts for
  those modes and overwrite earlier prompts for the same mode.
- `prompts/issue.append.md`, `prompts/new.append.md`, and
  `prompts/pr.append.md` append text to the current prompt for those modes.
- `scaffold/` is copied onto the worktree root. Keep files in this directory in
  the same shape they should have in the created worktree.

Prompt append is useful when a layer needs to add instructions without
replacing the prompt it inherited:

```toml
[agent.prompt]
common = ["Read AGENTS.md and project documentation before changing code.\n"]
issue = ["Review the issue, make the change, verify it, and report the result.\n"]

[agent.prompt.append]
common = ["Report verification results before finishing.\n"]
issue = ["Also check the project-specific release checklist before finishing.\n"]
```

For a Codex profile, scaffold files commonly look like:

```text
scaffold/
  AGENTS.override.md
  .codex/
    skills/
```

For a Claude profile, scaffold files commonly look like:

```text
scaffold/
  CLAUDE.local.md
  .claude/
    agents/
    commands/
    skills/
```

Create a named profile scaffold:

```bash
wt profile create codex
wt profile
```

Or extract inline `.local/.wt.toml` profile settings into a named profile:

```bash
wt init --local --agent codex --yes
wt config extract .local/.wt.toml
wt profile
```

Or inline prompt convention files back into a profile TOML:

```bash
wt config inline .local/profiles/codex/profile.toml
```

Or inline a selected named profile back into `.local/.wt.toml`:

```bash
wt config inline .local/.wt.toml
```

## Batches

Batches split planning from execution. `task` prepares local tasks and `issue`
imports provider issues as tasks without creating worktrees:

```bash
wt batch task "add schema" "wire API" --base main
wt batch issue
wt batch issue 123 456 789
wt batch issue 123 456 789 --base main
wt batch issue 123 456 789 --profile codex
```

When issue identifiers are omitted, `issue` opens the provider issue list and
lets you select multiple issues interactively.

Batch preparation asks once for the base branch and stores the resolved branch
in the batch file. `--base .` stores the current branch without prompting,
`--base` with no value opens the local branch selector, and `--base <branch>`
stores the named branch explicitly.
`run` requires that stored explicit base before it marks any task `running`.

When `--profile` is omitted, the batch does not store a profile field and uses
the effective config at run time. When `--profile <name>` is provided, the
batch stores that named profile.

Task details are stored separately from batch status:

```text
.local/tasks/123.toml
.local/batches/2026-05-09-001.toml
```

The batch TOML records the optional profile, base mode, overall batch status,
and one `[[tasks]]` table per task. The double brackets are TOML's
array-of-tables syntax, equivalent to a `tasks: [...]` list in JSON. Each task
row stores the task key and status; the task TOML stores the title, branch,
body, and optional issue origin.

Run a prepared batch explicitly:

```bash
wt batch show
wt batch show latest
wt batch edit
wt batch edit latest
wt batch run .local/batches/2026-05-09-001.toml
wt batch run latest
```

`show` prints the stored base branch, profile, batch status, and task statuses.
`edit` opens the batch TOML file without changing task state.

`run` executes only tasks with `status = "prepared"` or `status = "failed"`.
Tasks marked `done` or `skipped` are left alone, so reruns can continue from
the batch file's task status instead of checking a global issue state.

## Stacks

Stacks are ordered work where each task branch is based on the previous
completed task branch.

Create a stack from branch-name text without creating worktrees:

```bash
wt stack task "add schema" "wire API" --base main
wt stack task "add schema" "wire API" --base .
```

`issue` imports provider issues as tasks and writes a stack file without
creating worktrees:

```bash
wt stack issue
wt stack issue 123 456 789 --base main
wt stack issue 123 456 789 --base .
wt stack issue 123 456 789 --base main --profile codex
```

When issue identifiers are omitted, `issue` opens the provider issue list,
lets you select multiple issues, then asks for the base-to-top order. When
identifiers are provided, their argument order is the stack order.

Both `task` and `issue` create stack TOML. `task` writes local task documents
from branch-name text. `issue` writes task documents with provider origin
metadata.

Both commands ask once for the base branch and store the resolved branch in the
stack file. `--base .` stores the current branch without prompting, `--base`
with no value opens the local branch selector, and `--base <branch>` stores the
named branch explicitly. The first task uses that stored base as its parent.

You can also edit stack TOML directly:

```toml
# .local/stacks/manual.toml
base_mode = "explicit"
base = "main"
status = "prepared"

[[tasks]]
task = "add-schema"
parent = "main"
status = "prepared"
error = ""

[[tasks]]
task = "wire-api"
parent = "add-schema"
status = "prepared"
error = ""
```

The referenced task documents live under `.local/tasks/`:

```toml
# .local/tasks/add-schema.toml
title = "Add schema"
branch = "add-schema"
body = """
Create the schema first.
"""
```

`[[tasks]]` is the canonical stack list. Pull requests remain a separate
workflow and are not stack tasks.

Start the next runnable stack task:

```bash
wt stack show
wt stack show latest
wt stack edit
wt stack edit latest
wt stack run .local/stacks/2026-05-12-001.toml
wt stack run latest
```

`show` prints the stored base branch, profile, stack status, task statuses, and
the recorded parent chain.
`edit` opens the stack TOML file without changing task state.

`run` starts one prepared or failed task at a time and leaves it marked
`running`. The first task branch uses the stack base branch. Each following
task branch starts only after the previous task is completed, and uses that
previous completed task branch as its parent. Skipped tasks are not used as
parents; if every earlier task was skipped, the next task uses the stack base.

When `run` starts a task, the agent prompt includes the completion command:

```bash
wt stack complete .local/stacks/2026-05-12-001.toml 123 --run-next
```

`complete` verifies that the task branch has no uncommitted changes and has at
least one commit ahead of its parent before marking the running task `done`.
With `--run-next`, it then starts the next prepared or failed task
automatically:

```bash
wt stack complete latest 123 --run-next
```

Omit `--run-next` to mark one task done without starting another task.

The stack TOML records each task's `parent`, branch, and status so reruns can
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
