# wt

`wt` is a Git worktree workspace manager for starting ready-to-code workspaces
from issues, pull requests, or branch names.

It can create worktrees, open cmux workspaces, apply setup templates, register
local development sites, and bootstrap an agent prompt.

## Install

The recommended install path is Homebrew. The public tap is updated by the
release workflow and installs prebuilt binaries for Apple Silicon macOS, Intel
macOS, and x64 Linux.

```bash
brew install hoetaek/tap/wt
wt --version
```

Update an existing Homebrew install with:

```bash
brew update
brew upgrade hoetaek/tap/wt
```

You can also install the latest GitHub Release with the shell installer:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hoetaek/wt/releases/latest/download/wt-installer.sh | sh
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

## Project Status

`wt` is still pre-1.0. Until the CLI, config format, and persisted state files
stabilize, breaking user-facing changes are represented as `0.x.0` minor
bumps, not `x.0.0` major releases.

Development happens on `develop`. `master` is the release branch and should only
receive release PRs. Regular development commits do not bump `Cargo.toml`;
release PRs bump `Cargo.toml` and `Cargo.lock` once, using the largest SemVer
scope included in that release, then the release commit is merged back into
`develop`.

Public releases are available through GitHub Releases and Homebrew. If you are
trying `wt` in another project, start with `wt init --dry-run` or `wt doctor` to
inspect what `wt` would use before changing repository config.

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

`doctor` is a readiness inspection command. It reports missing optional tools
and, for Codex agents, whether the local Codex cmux hook files appear ready for
reliable `wt status` polling. It does not install hooks or mutate agent config.

Claude Code status is driven through cmux's Claude integration. Codex can run in
cmux without extra setup, but reliable Codex status events require explicit
Codex hooks:

```bash
cmux hooks codex install --yes
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

Prepare local tasks or issues as workflows:

```bash
wt workflow task "add schema" "wire API" --mode batch --base main
wt workflow issue 123 456 789 --mode batch --base main
wt workflow issue 123 456 789 --mode stack --base main
wt workflow show 2026-05-16-001
wt workflow run
wt workflow run 2026-05-16-001
wt workflow complete 2026-05-16-001 PROJ-123 --run-next
```

`wt workflow` is the canonical surface for prepared work. `single`, `batch`,
and `stack` are workflow modes under `.local/workflows`; the old top-level
batch and stack commands are migration context only.

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
tasks, PRs, branches, workflows, config sections, or worktrees.
Rows with supporting metadata keep that metadata in an aligned hint column
within the prompt page; filtering still uses both the resource label and the
metadata text. Plain label-only selectors stay unpadded.

Single-select prompts choose one resource and continue. Multi-select prompts
use checkbox-style rows; selecting no rows is only accepted by commands whose
documented behavior treats an empty selection as a no-op.

## Workflow Model

The target prepared-work contract is a Workflow: a saved execution plan under
`.local/workflows/<id>.toml`. A Workflow is the user-facing container for
running one or more prepared TaskDocuments, and `wt workflow` is the canonical
surface for preparing, showing, editing, running, and completing that work.

Each Workflow has exactly one execution-shape switch:

- `mode = "single"` runs one or more TaskDocuments in one branch workspace.
- `mode = "batch"` runs multiple independent TaskDocuments from the same base.
- `mode = "stack"` runs ordered TaskDocuments as a branch parent chain.

`batch` and `stack` remain mode values, not top-level state-file nouns.
`.local/workflows` replaces `.local/batches` and `.local/stacks` because the
same stored concept now covers single, independent, and ordered task execution.
Keeping separate batch and stack directories as new canonical state would make
users choose between three names for the same prepared-work container. Old
batch and stack files can be treated as migration context while the command
surface changes, but new persisted workflow state should converge on
`.local/workflows`.

A Workflow file stores workflow-level orchestration: `mode`, base, optional
profile, optional color, timestamps, and `[[tasks]]` rows that link to
TaskDocuments and TaskRuns. The branch name source of truth stays in each
TaskDocument's `branch` field. Workflow task rows must not copy branch names;
they only link to the task and its execution record, plus mode-specific
instructions such as stack parents or pull-request handoff intent.

When a Workflow does not specify a color, `wt` chooses the next color from its
built-in cmux named-color palette and writes that color back to the workflow
file. The color marks workspaces opened for the same Workflow; it is not a task
or mode meaning.

Replacing prepared-task `wt new`, `wt batch`, and `wt stack` with
`wt workflow` is a user-facing CLI and persisted state-model change. While
`wt` is pre-1.0, that migration should be released as a `0.x.0` minor version,
not as a patch.

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
context used by workflow runs. It also asks the agent to finish with a compact
completion report: summary, changed files, checks run, and risks or follow-ups.
Bare `wt new` is rejected; pass branch-name text for a new branch workspace or
`--task`/`--task <task>` for prepared local tasks.

Prepared local tasks use two persisted concepts. A TaskDocument under
`.local/tasks/<task>.toml` describes what the work is: title, branch, body, and
optional issue origin. A TaskRun under `.local/task-runs/<id>.toml` records one
attempt to execute that task: task key, branch, status, source, optional group,
optional error, creation order, and timestamps. New TaskRun files include a
monotonic `creation_order` value so latest-run selection follows execution
creation order even when multiple runs share the same timestamp second.
Existing TaskRun files without `creation_order` remain readable. They sort
before ordered TaskRuns and use timestamps as the legacy ordering fallback among
other legacy records.

When `wt new --task` starts selected tasks, it writes TaskRuns with
`source = "new"`. Successful starts remain `running` until the matching
worktree is cleaned up with `wt done`, which marks the matching runs `done`.
The task selector hides tasks whose latest run is `running` or `done`.
Latest `prepared`, `failed`, and `skipped` runs stay selectable/retryable.
With `--profile` or `--matrix`, each created profile worktree gets its own
TaskRun record. With multiple selected tasks, each task gets its own TaskRun
record on the same created branch.

## Publishing Local Tasks

Use `wt task publish` to choose unprocessed local TaskDocuments and create
provider issues without starting workspaces. Pass explicit task keys when a
script already knows which tasks to publish:

```bash
wt task publish
wt task publish add-profile-docs
wt task publish add-profile-docs wire-publish-api
```

This is the reverse direction from `wt workflow issue`, which imports provider
issues into `.local/tasks/`. `wt issue` remains focused on starting work from
existing provider issues; it is not the issue-creation surface for local tasks.

Publishing is a provider side effect with a durable local link. It does not
start workspaces, create TaskRuns, or run workflow work. The command succeeds
only after the provider issue is created and the selected
`.local/tasks/<task>.toml` file is updated with `[origin]`. If either step
fails, the command must not report success. The `origin` table is the durable
link to the external issue, not a pending publish request.

After `[origin]` is written, `wt workflow run` treats that TaskDocument as
provider-origin issue work. The `origin.id` becomes the issue identifier used
for naming, setup mode, and agent prompt context. If the TaskDocument still has
a `branch`, future runs use that branch; if `branch` is empty, the configured
provider is asked to ensure the issue branch and the resulting branch is
written back to the task on a successful start.

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

Publish does not store TaskRun execution state, workflow orchestration, profile
selection, retry status, or branch landing state in the TaskDocument.
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

Use `wt status <branch|worktree|task-run>` to poll the current agent session for
a worktree. The command uses the same target model as `review` and `send`, reads
the matching cmux workspace/surface when available, and reports the observed
agent, status, last tool, session, event time, and warnings without updating
TaskRuns or provider issues.

`status` observes the current cmux surface, screen text, cmux status values, and
agent hook/sidebar events. It does not mark TaskRuns done, update provider
issues, send messages, or install hooks. Claude Code status comes from cmux's
Claude integration. Codex emits the reliable `agent.hook.*` and
`set_status codex Running/Idle` signals only after
`cmux hooks codex install --yes`; without those hooks, Codex can still run in
cmux but `wt status` falls back to weaker terminal-screen inference and may
report a `codex_hooks_missing` warning. Run `wt doctor` to inspect cmux
availability and Codex hook readiness.

```bash
wt status feature
wt --json status feature
```

The JSON form is the scriptable surface. `needs_input` exits with status code
2, `failed` exits 3, missing work exits 1, and normal observed states exit 0.
If cmux itself is unavailable, `status` fails clearly instead of returning a
misleading successful `no_session` result.

Use `wt send <branch|worktree|task-run> <message...>` when the review output
shows a task agent surface and you want to ask for a completion report, request
extra verification, or give a follow-up instruction. By default it sends the
message and presses enter. Add `--no-enter` before the message to leave the
text inserted without submitting it. If the target worktree has more than one
matching cmux surface, `wt send` asks which surface to use; when it cannot ask,
it fails without sending and prints the exact raw `cmux send` commands.

## Running Workflows

`workflow task` prepares local TaskDocuments; `workflow issue` imports provider
issues as TaskDocuments. Both write one Workflow file and linked TaskRun records
without starting worktrees:

```bash
wt workflow task --mode batch --base main
wt workflow task "add schema" "wire API" --mode single --base main
wt workflow task "add schema" "wire API" --mode batch --base main
wt workflow task "add schema" "wire API" --mode stack --base main --pull-request
wt workflow issue 123 456 789 --mode batch --base main --profile codex
wt workflow issue 123 456 789 --mode stack --base main
```

Bare `wt workflow task --mode <mode>` opens a multi-select list of existing
local TaskDocuments. Pass task titles or task keys explicitly when a script or
prepared command already knows which tasks to include.

`--mode single` runs one workspace. With multiple tasks, all selected
TaskDocuments must share one branch. `--mode batch` runs independent task
branches from the same base. `--mode stack` runs one task at a time in parent
chain order. `--pull-request` is valid only for stack-mode workflows and records
task-agent handoff intent on each workflow row.

Workflow state is split across three files:

```text
.local/tasks/add-schema.toml
.local/task-runs/workflow-2026-05-16-001-add-schema.toml
.local/workflows/2026-05-16-001.toml
```

The Workflow file owns orchestration: `mode`, base, profile, color, timestamps,
and `[[tasks]]` rows with task/run links plus stack-mode `parent` and
`pull_request` fields. TaskRun owns execution state. TaskDocument owns title,
body, origin, and the branch name.

Inspect or edit a workflow:

```bash
wt workflow show
wt workflow show 2026-05-16-001
wt workflow edit 2026-05-16-001
```

`show` derives task status from linked TaskRun records and prints task paths,
branches, parent chain, pull-request intent, and errors where present. `edit`
opens the workflow TOML file without changing task state.

Run a workflow:

```bash
wt workflow run
wt workflow run 2026-05-16-001
wt workflow run 2026-05-16-001 --jobs 3
```

Bare `wt workflow run` selects from runnable workflows. If exactly one
workflow can run, it starts that workflow without prompting. If several can run,
interactive terminals choose one from a selector; non-interactive shells fail
before changing state and print explicit `wt workflow run <workflow>` rerun
commands. Pass a workflow id or TOML path for scripts.

`single` mode starts all selected tasks in one branch workspace and marks the
linked TaskRuns `running`. `batch` mode starts prepared or failed tasks; running
siblings remain visible and do not block independent work. `--jobs <N>` starts
at most N batch-mode tasks concurrently. Conflict cases that would require an
interactive worker prompt, such as an existing worktree path, are recorded as
task failures in linked TaskRun files.

`stack` mode starts only the next prepared or failed task and leaves it marked
`running`. The first task branch uses the workflow base. Each following task
branch starts after a previous task is completed and uses the previous completed
task branch as parent. Skipped tasks are not used as parents; if every earlier
task was skipped, the next task uses the workflow base.

When stack mode starts a task, the task-agent prompt includes a workflow
coordinator handoff. With `pull_request = true`, the task agent pushes the
branch and opens a draft pull request against the workflow parent branch. With
`pull_request = false`, it reports `PR=none`. In both cases the coordinator
reviews the report and branch state before completing the task:

```bash
wt workflow complete 2026-05-16-001 add-schema --run-next
```

`complete` verifies that the running task branch has no uncommitted changes and
has at least one commit ahead of its parent before marking the linked
`source = "stack"` TaskRun `done`. With `--run-next`, it then starts the next
prepared or failed stack-mode task. Omit `--run-next` to mark one task done
without starting another.

Old `.local/batches` and `.local/stacks` files are migration context only. New
prepared-work state belongs in `.local/workflows`, and the old `batch` and
`stack` top-level commands fail with guidance instead of mutating legacy state.

## Landing Completed Task Branches

`complete`, `done`, `merge`, and local task cleanup are separate lifecycle
steps. A TaskRun with `status = "done"` says the execution instance is finished;
it does not prove that the branch has landed on `master`.

The current command surface stays explicit: there is no `wt land` command yet.
Landing uses normal Git commands so `wt done` and `wt workflow complete` do not
get hidden merge side effects.

For a single-mode or batch-mode workflow branch:

1. Review and test the branch while its worktree exists, or through a pushed
   branch or PR. `wt workflow show <workflow>` shows linked TaskRun status.
2. Land the branch with the repository's normal Git or PR policy, for example
   `git switch master`, `git pull --ff-only`, and
   `git merge --ff-only <task-branch>`. Do this before `wt done` when the local
   branch is the branch you intend to merge.
3. Run `wt done <target>` after the branch has landed or is intentionally
   disposable. It removes the worktree, cleans matching integrations, deletes
   the local branch, and marks matching `source = "new"` or `source = "batch"`
   running TaskRuns `done`.
4. Keep `.local/tasks/<task>.toml` when the task is reusable or referenced by
   another workflow. One-shot TaskDocuments can be removed manually after the
   branch has landed and no workflow references them.

For stack-mode workflow branches:

1. Review and test the running task branch.
2. Run `wt workflow complete <workflow> <task> [--run-next]`. It verifies the
   task branch is clean and ahead of its parent, then marks the linked
   `source = "stack"` TaskRun `done`. It does not remove the worktree or merge
   the branch.
3. Land stack branches in the base-to-top order shown by
   `wt workflow show <workflow>`, because each higher branch is based on the
   completed branch below it. Merge is still a Git or PR step, not
   `wt workflow complete`.
4. After workflow completion and landing, run `wt done <target>` only for
   worktree and local branch cleanup. It will not mark stack TaskRuns done;
   that remains `wt workflow complete`.
5. Keep TaskDocuments when they are reusable or needed as local history. Remove
   one-shot `.local/tasks/<task>.toml` files manually only after the workflow
   has landed and no other workflow references them.

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
