# wt

`wt` is a Git worktree workspace manager for starting ready-to-code workspaces
from issues, pull requests, or branch names.

It can create worktrees, open cmux workspaces, apply setup templates, register
local development sites, and bootstrap an agent prompt.

## Install

Install with Homebrew:

```bash
brew install hoetaek/tap/wt
```

Or install from the Git repository with Cargo:

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
there by another project, so Homebrew is the recommended packaged install path.

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

Start the guided workspace starter wizard:

```bash
wt init
```

The wizard asks what starter shape this repo should use, previews the selected
config target and generated sections, then writes only the selected config file.

Create the smallest useful config:

```bash
wt init --minimal
```

Use a repeatable starter preset in automation:

```bash
wt init --preset agent --yes
```

Canonical starter presets are `minimal`, `agent`, `issue`, and `app`. The
`agent` preset uses Codex by default; pass `--agent <name>` to choose another
runtime. The `app` preset scans the repo for setup commands, dev tabs, and test
commands it can place in the generated plan. Use `--local` to target
`.local/.wt.toml` or `--shared` to target `.wt.toml`; use `--dry-run` to preview
the generated TOML without writing files. Dry run is the safe inspection path:
it prints the target file, selected preset, selected sections, detected repo
signals, and generated TOML while leaving both `.wt.toml` and `.local/.wt.toml`
untouched.

```bash
wt init --preset app --yes --dry-run
```

Bare `wt init --yes` is non-interactive and chooses the `minimal` preset.
Explicit starter options such as `--agent`, `--issue-provider`, and
`--site-provider` still add their matching sections. In a non-TTY context, pass
`--yes`, `--minimal`, or `--preset <name>` so the command never waits for
interactive answers.

For explicit config values, pass the relevant starter options:

```bash
wt init --shared --preset issue --issue-provider github --yes
```

`wt init` does not create named profile directories or prompt scaffold files;
keep small runtime settings inline in the selected config file. Use
`wt config extract` to move inline `[profile.*]` settings into a named profile,
or `wt profile create` when you want to start with an explicit profile
directory.

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

When pull request numbers are omitted, `pr` opens a filterable GitHub PR
multi-select list. Each selected PR starts sequentially.

Start a workspace from branch-name text, or explicitly choose prepared local
tasks:

```bash
wt new add profile docs
wt new --task
wt new --task add-profile-docs
wt new publish issue tasks --task add-schema --task publish-issues
```

Publish prepared local tasks to the configured issue provider without starting
workspaces:

```bash
wt task publish
wt task publish add-profile-docs
```

Prepare issue work in bulk:

```bash
wt batch issue
wt batch run
wt stack issue
wt stack show latest
wt stack run
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

List worktrees, and remove a finished worktree after its branch has landed or
is intentionally disposable:

```bash
wt list
wt review
wt review task-run-new-profile-partial-runs
wt send task-run-new-profile-partial-runs "please report current status"
wt done
wt done PROJ-123
```

`wt review [TARGET]` is a read-only pre-landing check. Omit the target to
inspect the current branch, or pass a branch, worktree path/name, or TaskRun id.
`wt send <TARGET> <MESSAGE...>` uses the same target forms to send a message to
the matching cmux surface and press enter.

When a cmux workspace was opened for the same worktree path, `wt done` attempts
to close it before removing the worktree. `wt done` also deletes the matching
local branch; it does not merge the branch into `master`.

## Interactive Prompts

When a command omits a value that can be chosen safely, `wt` shows a compact
terminal prompt instead of guessing. Selectors are filterable, cap the visible
list to ten rows, and keep the row label focused on the resource being chosen:
tasks, PRs, branches, batches, stacks, config sections, or worktrees.

Single-select prompts choose one resource and continue. Multi-select prompts
use checkbox-style rows; selecting no rows is only accepted by commands whose
documented behavior treats an empty selection as a no-op.

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

- `wt issue --profile <name>`, `wt new <words...> --profile <name>`, and
  `wt new --task [<task>] --profile <name>` create profiled worktrees. The branch
  and workspace names include the profile name so the profiled run is separate
  from the unprofiled workspace.
- `wt pr --profile <name>` applies the named profile config to every selected
  PR worktree. It uses each PR branch name as-is because the branch already
  exists.

Use `--matrix` on `wt issue`, `wt new <words...>`, or `wt new --task [<task>]` to
expand one issue, branch-name input, or prepared task across all named profiles.
`wt issue 123 --matrix` creates one issue worktree per profile,
`wt new add search --matrix` creates one branch worktree per profile from the
`add-search` branch-name seed, and `wt new --task add-search --matrix` creates
one prepared task worktree per profile.

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
wrapper script. When a cmux workspace is launched from another cmux surface,
agent prompts also receive `{{coordinator_cmux_workspace}}`,
`{{coordinator_cmux_surface}}`, `{{coordinator_send_command}}`, and
`{{coordinator_enter_command}}`; the launched agent surface receives
`{{task_agent_cmux_workspace}}` and `{{task_agent_cmux_surface}}`. For example,
a Codex profile can give Chrome DevTools MCP a
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

## New Workspaces

`wt new <words...>` starts a workspace from branch-name text:

```bash
wt new add profile docs
wt new add profile docs --base main
wt new add profile docs --profile codex
wt new add profile docs --matrix
```

Use `--task` to select prepared TaskDocuments from `.local/tasks/*.toml` and
start them immediately:

```bash
wt new --task
wt new --task add-profile-docs
wt new --task add-profile-docs --base main
wt new --task add-profile-docs --profile codex
wt new --task add-profile-docs --matrix
```

Bare `wt new --task` opens a filterable multi-select task list. Selecting one
task starts that task on its prepared branch. Selecting multiple tasks asks for
one workspace branch name and starts all selected TaskDocuments in that single
workspace.

Repeat `--task <task>` with branch-name text to run multiple prepared
TaskDocuments in one workspace without opening the selector:

```bash
wt new publish issue tasks --task add-schema --task publish-issues --task notify-users
wt new publish issue tasks --task add-schema --task publish-issues --profile claude-teams
```

This creates one worktree branch from the branch-name text and writes one
`source = "new"` TaskRun per selected TaskDocument. Multi-task workspace runs
also assign the TaskRuns a shared `group`, so review and cleanup can show that
the separate task records belong to the same workspace run.

The agent prompt includes the selected TaskDocument content, matching the task
context used by `wt batch run` and `wt stack run`. It also asks the agent to
finish with a compact completion report: summary, changed files, checks run,
and risks or follow-ups.
Bare `wt new` is rejected; pass branch-name text for a new branch workspace or
`--task`/`--task <task>` for prepared local tasks.

Prepared local tasks use two persisted concepts. A TaskDocument under
`.local/tasks/<task>.toml` describes what the work is: title, branch, body, and
optional issue origin. A TaskRun under `.local/task-runs/<id>.toml` records one
attempt to execute that task: task key, branch, status, source, optional group,
optional error, creation order, and timestamps. New TaskRun files include a
monotonic `creation_order` value so latest-run selection follows execution
creation order even when multiple runs share the same timestamp second.
Existing TaskRun files without `creation_order` remain readable and use their
timestamps as the legacy ordering fallback.

When `wt new --task` starts selected tasks, it writes TaskRuns with
`source = "new"`. Successful starts remain `running` until the matching
worktree is cleaned up with `wt done`, which marks the matching runs `done`.
The task selector hides tasks whose latest run is `running`, `done`, or
`skipped`; failed runs stay retryable. With `--profile` or `--matrix`, each
created profile worktree gets its own TaskRun record. With multiple selected
tasks, each task gets its own TaskRun record on the same created branch.

## Publishing Local Tasks

Use `wt task publish` to choose unprocessed local TaskDocuments and create
provider issues without starting workspaces. Pass explicit task keys when a
script already knows which tasks to publish:

```bash
wt task publish
wt task publish add-profile-docs
wt task publish add-profile-docs wire-publish-api
```

This is the reverse direction from `wt batch issue` and `wt stack issue`, which
import provider issues into `.local/tasks/`. `wt issue` remains focused on
starting work from existing provider issues; it is not the issue-creation
surface for local tasks.

Publishing is a provider side effect with a durable local link. It does not
start workspaces, create TaskRuns, or run batch or stack work. The command
succeeds only after the provider issue is created and the selected
`.local/tasks/<task>.toml` file is updated with `[origin]`. If either step
fails, the command must not report success. The `origin` table is the durable
link to the external issue, not a pending publish request.

After `[origin]` is written, `wt new --task`, `wt batch run`, and
`wt stack run` treat that TaskDocument as provider-origin issue work. The
`origin.id` becomes the issue identifier used for naming, setup mode, and agent
prompt context. If the TaskDocument still has a `branch`, future runs use that
branch; if `branch` is empty, the configured provider is asked to ensure the
issue branch and the resulting branch is written back to the task on a
successful start.

TaskDocument stays limited to title, branch, body, and optional origin:

```toml
title = "Add profile docs"
branch = "add-profile-docs"
body = """
Document profile setup and extraction.
"""

[origin]
provider = "github"
id = "123"
```

Publish does not store TaskRun execution state, batch or stack orchestration,
profile selection, retry status, or branch landing state in the TaskDocument.
Bare `wt task publish` opens a filterable multi-select list of local
TaskDocuments that do not have `[origin]`. Selecting no tasks prints a warning
and exits successfully.
Explicit task keys remain available for scripts and are published once in first
visible order. The command prints a summary of published, skipped, and failed
task keys.

Ambiguity fails before any provider write:

- No configured issue provider: fail with a clear error.
- Existing `origin`: fail by default and do not silently create a duplicate
  issue. A future option may add explicit skipping, but skipping is not
  implicit.
- Existing `origin.provider` differs from the configured issue provider: fail
  as a provider mismatch.
- Empty `title`: fail because the provider issue needs a title.
- Empty or omitted `body`: allowed; publish an empty issue body.

Dry-run is not part of the first publish write path. If it is added later, it
should run the same validation and print the planned provider issue fields and
local `origin` update without writing pending state to the TaskDocument.

Use `wt review [branch|worktree|task-run]` to inspect a task run before landing
it. The command reads the linked TaskRun and TaskDocument when available,
reports the recorded parent branch, shows dirty worktree state, committed
commits and diff stats, and repeats the expected agent completion report shape.
When cmux is available, it also shows every matching workspace surface for the
target worktree, plus the raw `cmux send` and `cmux send-key` commands needed to
talk to each surface. Multiple matches are reported as ambiguous. It does not
mark anything done, merge branches, send messages, or remove worktrees.

Use `wt send <branch|worktree|task-run> <message...>` when the review output
shows a task agent surface and you want to ask for a completion report, request
extra verification, or give a follow-up instruction. By default it sends the
message and presses enter. Add `--no-enter` before the message to leave the
text inserted without submitting it. If the target worktree has more than one
matching cmux surface, `wt send` asks which surface to use; when it cannot ask,
it fails without sending and prints the exact raw `cmux send` commands.

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

When issue identifiers are omitted, `issue` opens a filterable provider issue
multi-select list.

Batch preparation asks once for the base branch and stores the resolved branch
in the batch file. `--base .` stores the current branch without prompting,
`--base` with no value opens the local branch selector, and `--base <branch>`
stores the named branch explicitly.
`run` requires that stored explicit base before it starts any linked TaskRun.

When `--profile` is omitted, the batch does not store a profile field and uses
the effective config at run time. When `--profile <name>` is provided, the
batch stores that named profile.

Task definitions, execution state, and batch orchestration are stored
separately:

```text
.local/tasks/123.toml
.local/task-runs/batch-2026-05-09-001-123.toml
.local/batches/2026-05-09-001.toml
```

The batch TOML records the optional profile, base mode, overall batch status,
and one `[[tasks]]` table per task. The double brackets are TOML's
array-of-tables syntax, equivalent to a `tasks: [...]` list in JSON. Each task
row stores only the task key and the linked TaskRun id created during
preparation. The TaskRun TOML is the execution-instance record: it stores the
task key, branch, status, `source = "batch"`, `group` derived from the batch
file stem, optional error, creation order, and timestamps. The TaskDocument
stores the title, branch, body, and optional issue origin. Prepared
TaskDocuments can also be started directly with `wt new --task` or
`wt new --task <task>`, without creating or running a batch file.

Run a prepared batch:

```bash
wt batch show
wt batch show latest
wt batch edit
wt batch edit latest
wt batch run
wt batch run .local/batches/2026-05-09-001.toml
wt batch run 2026-05-09-001 --jobs 3
wt batch clean
wt batch clean latest
```

`show` prints the stored base branch and profile, then derives batch and task
statuses from the linked TaskRun records.
`edit` opens the batch TOML file without changing task state.

Bare `run` opens a filterable selector for runnable batches. A runnable batch
has at least one linked TaskRun with `status = "prepared"` or
`status = "failed"`. Running sibling TaskRuns do not block the batch selector
because batch tasks are independent; the selector label shows running counts so
in-flight work remains visible. Passing a batch TOML path or shorthand id keeps
the command scriptable. `latest` is not a `run` target.

`run` executes only linked TaskRuns with `status = "prepared"` or
`status = "failed"`. TaskRuns marked `running`, `done`, or `skipped` are left
alone, so reruns can continue from the linked execution records instead of
checking a global issue state. A successfully created workspace leaves the
TaskRun `running`; `wt done` records actual completion for batch TaskRuns by
marking matching `running` records `done`.
Batch-created cmux workspace names start with a compact order label, for example
`B2/5 PROJ-123 Fix editor`, so narrow workspace tabs still show both the batch
position and the issue or task being edited.
By default `run` uses `--jobs 1` and keeps the current sequential behavior.
`--jobs <N>` starts at most N runnable tasks concurrently. The coordinator is
the only writer for shared batch metadata: workers do not write batch metadata
directly, and the coordinator records started, failed, and skipped task events
in linked TaskRun files. Shared Git metadata writes such as `parentbranch` are
serialized by the repo mutation guard. In parallel mode, conflict cases that
would require an interactive worker prompt, such as an existing worktree path,
are recorded as task failures instead.

`clean` deletes TaskDocument files from `.local/tasks/` for a completed batch.
It keeps the batch TOML orchestration record and TaskRun execution history,
refuses batches with linked TaskRuns still in `prepared`, `running`, or
`failed`, skips TaskDocuments still referenced by another batch or stack, and
reports deleted, skipped, and already-missing task files.

## Stacks

Stacks are ordered work where each task branch is based on the previous
completed task branch.

Create a stack from branch-name text without creating worktrees:

```bash
wt stack task "add schema" "wire API" --base main
wt stack task "add schema" "wire API" --base .
wt stack task "add schema" "wire API" --base main --pull-request
```

`issue` imports provider issues as tasks and writes a stack file without
creating worktrees:

```bash
wt stack issue
wt stack issue 123 456 789 --base main
wt stack issue 123 456 789 --base .
wt stack issue 123 456 789 --base main --profile codex
wt stack issue 123 456 789 --base main --pull-request
```

When issue identifiers are omitted, `issue` opens a filterable provider issue
multi-select list, then asks for the base-to-top order. When identifiers are
provided, their argument order is the stack order.

Both `task` and `issue` create stack TOML plus linked TaskRun records under
`.local/task-runs/`. `task` writes local task documents from branch-name text.
`issue` writes task documents with provider origin metadata.

Both commands ask once for the base branch and store the resolved branch in the
stack file. `--base .` stores the current branch without prompting, `--base`
with no value opens the local branch selector, and `--base <branch>` stores the
named branch explicitly. The first task uses that stored base as its parent.
Both commands write `pull_request = false` by default. Pass `--pull-request`
to write `pull_request = true` for every prepared task row.

You can also edit stack TOML directly:

```toml
# .local/stacks/manual.toml
base_mode = "explicit"
base = "main"
status = "prepared"

[[tasks]]
task = "add-schema"
run = "stack-manual-add-schema"
pull_request = false
parent = "main"

[[tasks]]
task = "wire-api"
run = "stack-manual-wire-api"
pull_request = true
parent = "add-schema"
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

The linked TaskRun documents hold execution state:

```toml
# .local/task-runs/stack-manual-add-schema.toml
task = "add-schema"
branch = "add-schema"
status = "prepared"
source = "stack"
group = "manual"
creation_order = 1
created_at = "2026-05-16T00:00:00.000000000Z"
updated_at = "2026-05-16T00:00:00.000000000Z"
```

Run `wt new --task` or `wt new --task <task>` to select and start prepared task
documents outside the stack state machine.

`[[tasks]]` is the canonical stack list. `pull_request = true` means the task
agent should open a draft pull request against the stack parent branch after
committing; `pull_request = false` means it should report `PR=none` without
opening a pull request. Omitted `--pull-request` on the stack creation command
writes `false`. Pull requests remain a separate workflow and are not stack
tasks.

Start the next runnable stack task:

```bash
wt stack show
wt stack show latest
wt stack edit
wt stack edit latest
wt stack run
wt stack run .local/stacks/2026-05-12-001.toml
```

`show` prints the stored base branch, profile, stack status, task statuses, and
the recorded parent chain. Status and error details are derived from linked
TaskRun records.
`edit` opens the stack TOML file without changing task state.

Bare `run` opens a filterable selector for runnable stacks. A runnable stack
has a next `prepared` or `failed` TaskRun and no current `running` TaskRun. The
selector labels include task titles or keys, the next runnable task, status
counts, base, profile, and stack path so the intended stack is recognizable.
Passing a stack TOML path or shorthand id keeps the command scriptable.

`run` starts one prepared or failed task at a time and leaves it marked
`running`. The first task branch uses the stack base branch. Each following
task branch starts only after the previous task is completed, and uses that
previous completed task branch as its parent. Skipped tasks are not used as
parents; if every earlier task was skipped, the next task uses the stack base.
Stack-created cmux workspace names start with a compact order label, for example
`S2/5 PROJ-123 Wire API`, so narrow workspace tabs still show both stack position
and issue or task content.

When `run` starts a task, the agent prompt includes a coordinator handoff based
on the task row's `pull_request` value. With `pull_request = true`, after
committing the task work, the task agent pushes the branch and opens a draft
pull request against the stack parent branch:

```bash
git push -u origin HEAD
gh pr create --draft --base <parent-branch> --fill
```

With `pull_request = false`, it skips the pull request and reports `PR=none`.
In both cases it sends its completion report back to the coordinator cmux
surface that started the stack:

```bash
cmux send --workspace <coordinator-workspace> --surface <coordinator-surface> "Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr-url-or-none>; Risks or follow-ups=<risks>"
cmux send-key --workspace <coordinator-workspace> --surface <coordinator-surface> enter
```

The coordinator reviews that report, the branch state, and the pull request when
one exists, then advances the stack with the completion command:

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

Stack preparation creates one TaskRun record per task with `source = "stack"`
and `group` derived from the stack file stem, then stores each run id on the
stack task row. `run` updates the next runnable TaskRun to `running` and
`complete` updates the same TaskRun to `done`. The stack TOML records each
task's `parent`, run id, and `pull_request` handoff intent so reruns can
continue from the stored ordering state while TaskRun remains the
execution-state source of truth.
Stack TaskRuns are completed by `wt stack complete`, not by `wt done`, because a
stack task must be checked against its parent branch before the next task can
start.

## Landing Completed Task Branches

`complete`, `done`, `merge`, and local task cleanup are separate lifecycle
steps. A TaskRun with `status = "done"` says the execution instance is finished;
it does not prove that the branch has landed on `master`.

The current command surface stays explicit: there is no `wt land` command yet.
Landing uses normal Git commands so `wt done` and `wt stack complete` do not get
hidden merge side effects.

For a batch-produced branch or a branch started with `wt new --task`:

1. Review and test the branch while its worktree exists, or through a pushed
   branch or PR. `wt batch show <batch>` shows linked TaskRun status for batch
   tasks.
2. Land the branch with the repository's normal Git or PR policy, for example
   `git switch master`, `git pull --ff-only`, and
   `git merge --ff-only <task-branch>`. Do this before `wt done` when the local
   branch is the branch you intend to merge.
3. Run `wt done <target>` after the branch has landed or is intentionally
   disposable. It removes the worktree, cleans matching integrations, deletes
   the local branch, and marks matching `source = "new"` or `source = "batch"`
   running TaskRuns `done`.
4. Keep `.local/tasks/<task>.toml` when the task is reusable or referenced by
   another batch or stack. For one-shot batch tasks, run
   `wt batch clean <batch>` after linked TaskRuns are `done` or `skipped`; it
   deletes unreferenced TaskDocuments while keeping batch and TaskRun history.

For stack-produced branches:

1. Review and test the running task branch.
2. Run `wt stack complete <stack> <task> [--run-next]`. It verifies the task
   branch is clean and ahead of its parent, then marks the linked
   `source = "stack"` TaskRun `done`. It does not remove the worktree or merge
   the branch.
3. Land stack branches in the base-to-top order shown by
   `wt stack show <stack>`, because each higher branch is based on the completed
   branch below it. Merge is still a Git or PR step, not `wt stack complete`.
4. After `wt stack complete` and landing, run `wt done <target>` only for
   worktree and local branch cleanup. It will not mark stack TaskRuns done;
   that remains `wt stack complete`.
5. Keep stack TaskDocuments when they are reusable or needed as local history.
   There is no stack cleanup command today; remove one-shot
   `.local/tasks/<task>.toml` files manually only after the stack has landed and
   no other batch or stack references them.

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
