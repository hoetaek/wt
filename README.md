# wt

`wt` is a worktree-based agent orchestration harness for software engineers
working with AI coding agents.

It starts ready-to-code worktrees from issues, pull requests, branch-name text,
or local task files; applies repo setup; opens a cmux workspace when configured;
registers local development sites; and can hand a prepared prompt to an agent
such as Codex, Claude, or Gemini.

`wt` is a personal tool. It connects to team systems such as GitHub and Linear,
but the orchestration stays on your machine and does not require team-wide
adoption.

## Who This Is For

`wt` is for software engineers who are comfortable with Git worktrees and CLI
workflows, use AI coding agents as part of daily development, and want to shape
their own parallel-agent workflow without adopting a hosted service, daemon, or
team-wide tool.

## What wt Is Not

- `wt` is not a team standard tool.
- `wt` is not an agent runtime; Codex, Claude, Gemini, and similar tools do the
  agent work.
- `wt` is not a chatbot or general AI agent framework.
- `wt` is not a hosted service.
- `wt` is not a daemon.

## Install

Homebrew is the recommended install path:

```bash
brew install hoetaek/tap/wt
wt --version
```

Update with:

```bash
brew update
brew upgrade hoetaek/tap/wt
```

Install from the latest GitHub Release:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hoetaek/wt/releases/latest/download/wt-installer.sh | sh
```

Install from source:

```bash
cargo install --git https://github.com/hoetaek/wt
```

The crate is not published to crates.io because the `wt` package name is already
used by another project.

## wt Lifecycle Skills

This repository also ships an installable Agent Skills pack for the `wt`
lifecycle. The pack contains:

- `wt-idea`
- `wt-ready`
- `wt-start`
- `wt-coordinate`
- `wt-land`
- `wt-setup`
- `wt-work`

For an interactive install, run one command and choose the skills, agent, and
scope from the prompts:

```bash
npx skills add https://github.com/hoetaek/wt/tree/develop/skills
```

Select the `wt-*` skills you want.

For a non-interactive Codex global install, pass explicit skill names:

```bash
npx --yes skills@latest add https://github.com/hoetaek/wt/tree/develop/skills \
  --skill wt-idea \
  --skill wt-ready \
  --skill wt-start \
  --skill wt-coordinate \
  --skill wt-land \
  --skill wt-setup \
  --skill wt-work \
  -g -a codex --copy -y
```

From a local clone of this repository, use `.` as the source:

```bash
npx --yes skills@latest add . \
  --skill wt-idea \
  --skill wt-ready \
  --skill wt-start \
  --skill wt-coordinate \
  --skill wt-land \
  --skill wt-setup \
  --skill wt-work \
  -g -a codex --copy -y
```

For a project-local Codex install, run the same command from the project that
should receive the skills, point `add` at this repository clone, and omit `-g`:

```bash
npx --yes skills@latest add /path/to/wt \
  --skill wt-idea \
  --skill wt-ready \
  --skill wt-start \
  --skill wt-coordinate \
  --skill wt-land \
  --skill wt-setup \
  --skill wt-work \
  -a codex --copy -y
```

Installing these skills only installs Agent Skills playbooks. It does not
install the `wt` binary, run `wt init`, write `.wt.toml`, configure providers,
or create personal `wt` task/workflow state.

## Requirements

Required:

- Git
- Rust 1.85 or newer when building from source

Optional integrations:

- `gh` for GitHub issues and pull requests
- `linear` for Linear issue workflows
- `cmux` for workspace/window automation and agent status surfaces
- Codex, Claude, Gemini, or another configured agent command
- Herd, Valet, Docker proxy, or Traefik for local site helpers

## Quick Start

Create or preview config in a target repository:

```bash
wt init
wt init --preset app --dry-run
wt init --shared --preset issue --issue-provider github --yes
wt doctor
wt doctor --profile codex
```

Start work:

```bash
wt run issue
wt run issue PROJ-123 --base .
wt run pr
wt run pr 42 43
wt run branch add profile docs
wt run task
wt run task add-profile-docs
```

Prepare local TaskDocuments without starting work:

```bash
wt task list
wt task import PROJ-123
wt task import
wt task publish add-profile-docs
```

Prepare saved workflows when local tasks or issues need coordination:

```bash
wt workflow task --mode single add-schema wire-api --base .
wt workflow task --mode batch add-schema wire-api --base main --title "Ship search"
wt workflow task --mode matrix --profiles devtools-port,mcp-owned add-profile-docs --base main
wt workflow issue --mode stack 123 456 789 --base main --pr draft
wt workflow list
wt run workflow
wt workflow repair 2026-05-16-001
wt workflow complete 2026-05-16-001 add-schema --run-next
```

Open a read-only personal state UI:

```bash
wt ui --port 8424
```

Inspect, observe, message, and clean worktrees:

```bash
wt list
wt inspect
wt inspect <branch|worktree|task-run-id> --pr
wt agent status <branch|worktree|task-run-id>
wt agent watch <branch|worktree|task-run-id>
wt agent watch <branch|worktree|task-run-id> --timeout 300 --heartbeat 30
wt send <target> "please report current status"
wt done <target>
```

Omitting a work target opens a selector only in interactive human use. In
`--json`, `--quiet`, or non-TTY automation, pass an explicit branch, worktree
path/name, or direct TaskRun id.

`wt done` removes worktrees and local branches. It does not merge the branch.
Land reviewed work with Git or pull requests first. Workflow-linked TaskRuns are
completed with `wt workflow complete`, not `wt done`.

Pass `--pr` to `wt inspect <target>` when the dossier should include read-only
pull request review evidence. The PR section is opt-in and does not resolve
threads, post review comments, request review, edit PRs, mutate wt state, or
merge.

## Core Model

- `wt run branch <words...>` starts an ad hoc branch worktree from branch-name text.
- `wt run issue` starts a worktree from an existing provider issue.
- `wt run pr` opens existing pull request branches as worktrees.
- `wt run` only starts workspace execution. Cleanup stays under `wt done`,
  inspection under `wt inspect`, agent observation under `wt agent`, existing
  branch/worktree opening under `wt open`, and saved workflow lifecycle actions
  under `wt workflow`.
- `wt run workflow` starts runnable tasks from saved Workflow files. It does
  not list, edit, repair, or complete Workflow files.
- `TaskDocument` files in `<git-common-dir>/wt/tasks/<task>.toml` define prepared local work.
- `wt task list` is the canonical TaskDocument inventory. It lists all valid
  local TaskDocument files, reports invalid task TOML files, and does not start
  worktrees, branches, TaskRuns, Workflows, provider issues, or pull requests.
- `wt task import [<issue>...]` imports provider issues as TaskDocuments,
  records title, branch, body, and `[origin]`, and may materialize the provider
  issue branch first; it does not start worktrees, local branches, TaskRuns,
  Workflows, or pull requests.
- `wt run task [<task>...]` starts one worktree per selected TaskDocument.
- `wt task publish [<task>...]` creates provider issues from TaskDocuments and
  records `[origin]`; it does not start worktrees.
- `Workflow` files in `<git-common-dir>/wt/workflows/<id>.toml` save coordinated execution.
  Optional top-level `title`, `body`, and `[origin]` record the larger human
  context for the saved plan. Workflow `[origin]` belongs to the large
  issue-like unit represented by the Workflow; TaskDocument `[origin]` belongs
  only to a runnable slice that is itself a provider issue.
  `single` shares one workspace, `batch` runs independent branches from one
  base, and `stack` runs ordered branches as a parent chain.
- `wt workflow list` is the canonical saved Workflow inventory. It lists valid
  Workflow files whether or not they are runnable and reports invalid workflow
  TOML files instead of hiding parse failures.
- `TaskRun` files in `<git-common-dir>/wt/task-runs/<id>.toml` record execution attempts.
  Execution state is separate from branch landing.
- `wt ui [--port <port>]` starts a read-only loopback web UI for personal `wt`
  ideas, TaskDocuments, Workflows, TaskRuns, profile summaries, and effective
  config source paths. It serves embedded assets, exposes
  `GET /api/snapshot`, reports invalid TOML records, and does not write state
  or serve arbitrary repo files.
- `wt inspect [<target>]` is the read-only work dossier for a branch, worktree,
  or TaskRun. `wt inspect <target> --pr` adds nested pull request review
  evidence without changing lifecycle state or exit-code semantics.
- `wt agent status [<target>]` observes the current agent/cmux state, and
  `wt agent watch [<target>]` polls it. `wt agent watch` prints state
  transitions by default; `--timeout <seconds>` bounds the wait, and
  `--heartbeat <seconds>` opts into unchanged running reports. Agent state is
  separate from `TaskRun.status`.
- `wt agent hook install claude` installs a Claude-specific Claude Code
  `UserPromptSubmit` hook dispatcher in worktree-local
  `.claude/settings.local.json`. The hook reads `WT_AGENT_ID` at runtime, runs
  `wt msg check-inbox --agent "$WT_AGENT_ID"` when it is set, and otherwise
  exits successfully without output. The generated command stores its wt marker
  after `#`, so shell execution treats the marker as a comment rather than as
  an argument. `--agent <agent>` is only a manual/test override; normal
  `wt run issue`, `wt run task`, and `wt run workflow` sessions bind the
  per-run agent by setting `WT_AGENT_ID=agents/<branch_slug>` in the cmux
  `new-workspace --command` string when launching Claude.
- `wt agent hook install codex` installs a Codex-specific user-level
  `UserPromptSubmit` hook dispatcher in `$CODEX_HOME/hooks.json` or
  `~/.codex/hooks.json`. The hook reads `WT_AGENT_ID` at runtime, runs
  `wt msg check-inbox --agent "$WT_AGENT_ID"` when it is set, and otherwise
  exits successfully without output. It also writes the matching trusted hook
  state to Codex `config.toml`, preserving existing cmux and non-wt hooks/trust
  entries. `--agent <agent>` is only a manual/test override; normal
  `wt run issue`, `wt run task`, and `wt run workflow` sessions bind the
  per-run agent by setting `WT_AGENT_ID=agents/<branch_slug>` in the cmux
  `new-workspace --command` string when launching Codex. wt-launched Claude and
  future agent CLI commands receive the same launch-time environment binding.
  `wt agent hook uninstall codex` removes only wt-managed Codex hook and trust
  entries.

`wt workflow` is the canonical prepared-work surface. `single`, `batch`,
`stack`, and `matrix` are workflow mode values, not separate command surfaces. Use
`wt inspect` for read-only dossiers and `wt agent status` / `wt agent watch`
for runtime observation.

## Coordinator Handoff

Task prompts started by `wt run task` and `wt run workflow` include coordinator
handoff instructions when cmux coordinates are available. The prompt gives the
agent a `cmux send --workspace ... --surface ...` report command and a matching
`cmux send-key ... enter` command. It also gives the coordinator file inbox
target, `wt msg send --to coordinator ...`, where `coordinator` normalizes to
`agents/coordinator`.

Agents report back in this shape and then keep ownership of review follow-up
for their task:

```text
Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>
```

Immediate `wt run task` work reports `PR=none`. Workflow tasks follow the
prepared workflow policy. Omit `--pr` to use the effective `[workflow]`
configuration, pass `--pr none` to report `PR=none`, pass `--pr draft` to create
a draft PR and leave it draft, or pass `--pr ready` to create a review-ready PR
directly. PR-opening tasks create a body file from
`.github/pull_request_template.md`, fill a review-focused description, and pass
it to `gh pr create --body-file <pr-body-file>`. If the TaskDocument has
`[origin]`, the PR body includes an issue-closing keyword for that provider
issue. Workflow-level `[origin]` is workflow context only and is not copied into
child TaskDocuments or added as a closing keyword to child PR bodies. If
pull-request review or
coordinator feedback asks for changes, the same agent updates the branch, reruns
checks, pushes, refreshes the PR body only if it became stale, and sends an
updated report. Review always happens. The prepared landing policy only decides
whether the coordinator stops after review or proceeds to landing and cleanup
after review passes; automatic landing still has to satisfy dirty-worktree,
check, pull-request review, review-thread, and ancestry safety checks. When a PR
exists, review passing must be proved from the PR conversation immediately before
landing. Review-agent inline comments are conversational: after replying,
refresh the thread and wait for follow-up before resolving it. Thread-specific
addressed markers can satisfy that follow-up check; PR-level tool-specific
reactions or markers are provider status signals, not a replacement for checking
threads, comments, and checks. Examples:

- CodeRabbit inline comments need a follow-up refresh before resolution.
- Codex reactions are status signals to record before the normal review gate
  continues.

## Configuration

`wt` loads config from:

1. `--config <path>`
2. `.wt.toml` as shared project config
3. `<git-common-dir>/wt/config.toml` as personal repo config

Inspect the effective config:

```bash
wt config
wt config --profile codex
```

Treat `wt config` output as the source of truth for runtime behavior. Config
files store user intent and overrides, while `wt config` shows merged profile
layers plus built-in defaults in the shape users should copy and edit.

Workflow preparation reads policy from the effective config:

```toml
[workflow]
pull_request = "none"  # none | draft | ready
landing = "manual"     # manual | auto
```

`pull_request = "none"` means workflow agents report `PR=none`,
`pull_request = "draft"` means they create draft PRs, and
`pull_request = "ready"` means they create review-ready PRs.
`landing = "manual"` means review completes and the coordinator waits for an
explicit landing direction. `landing = "auto"` means review passing is enough
approval for the coordinator to proceed to landing and cleanup, without
bypassing dirty-worktree, check, pull-request review, review-thread, or ancestry
safety gates.

`wt config` prints the effective `[workflow]` policy, including the built-in
defaults above. `wt init` may include a commented optional `[workflow]` block so
the policy is discoverable, but generated config does not actively enable PR
creation or automatic landing by default.

When `[workspace]` is configured, `wt config` also prints effective workspace
colors, including built-in defaults. To change or disable a color, copy that
line into the owning config file and override the value there.
When `[workspace.browser]` is configured with `mode = "system"` or
`mode = "chrome_devtools"`, `wt config` prints the effective browser launch URL.
For Chrome DevTools mode, `wt config` also prints the effective Chrome user data
directory. The port is shown only when it is configured; otherwise setup
reserves an available localhost port at runtime.

When an active `[site]` provider is configured, `wt config` prints the site
defaults runtime setup uses, such as the generated name template, root,
security, URL template, and Traefik target. A disabled `provider = "none"` site
section is omitted from effective output. Browser launch behavior belongs to
`[workspace.browser]`, not `[site]`.
When `[editor]` is configured, `wt config` prints the effective editor
placement default, `cmux_surface`, unless it is overridden.

`wt workflow task` and `wt workflow issue` snapshot the effective workflow
policy into `<git-common-dir>/wt/workflows/<id>.toml` for the prepared workflow.
`wt workflow show` reads that prepared policy from the workflow file, not from
the current `.wt.toml`, so later config edits do not rewrite the meaning of
existing workflow files. An explicit `--pr none|draft|ready` overrides the
effective config for that prepared workflow only.

Use `--title <text>` and `--body <text>` with `wt workflow task` or
`wt workflow issue` to write top-level workflow context for the larger goal
without changing runnable selection, TaskRun lifecycle, landing policy, or
cleanup behavior. Use `--origin-provider` with `--origin-id` when the Workflow
itself has a durable provider source. `wt workflow issue` treats selected
provider issues as executable slice TaskDocuments and writes their origins on
those TaskDocuments; it does not lift selected issue ids into Workflow
`[origin]` automatically. To split one broad provider issue into local child
slices, prepare child TaskDocuments and pass the broad issue as explicit
workflow-level origin.

Small private agent config can stay inline:

```toml
[profile.agent]
cli = "codex"
args = ["--model", "gpt-5.5"]

[workspace]
tabs = ["lazygit", "nvim"]
# Workspace colors have built-in defaults; set this only to override.
# Use "" to disable a color kind.
# colors = { task = "blue", issue = "blue", branch = "green", pr = "magenta" }
```

Agent prompt scopes stay under `[agent.prompt]`. `common` is prepended to
`issue`, `branch`, and `pr` prompts. `workflow` is a separate workflow-started
task scope: `wt run workflow` sends it after the built-in workflow handoff and
TaskDocument snapshot, before the existing `issue` or `branch` setup-mode prompts.
It does not apply to direct `wt run task`, `wt run issue`, `wt run branch`, or `wt run pr`.

```toml
[agent.prompt]
workflow = ["Wait for external PR review before reporting workflow completion."]

[agent.prompt.append]
workflow = ["Mention the PR state and any remaining review risk in the report."]
```

Workspaces can opt into an isolated debuggable Chrome instance during setup:

```toml
[workspace.browser]
mode = "chrome_devtools"
# url = "{{site_url}}"

[workspace.chrome_devtools]
# port = 9222
# user_data_dir = "{{worktree_parent}}/.chrome-devtools/{{worktree_name}}"
```

In Chrome DevTools browser mode, `wt` reserves a localhost port, launches Chrome
with `--remote-debugging-address=127.0.0.1`, and uses a non-default per-worktree
user data directory under the worktree parent, outside the repository checkout.
Setup templates, post-deps tabs, local context, and agent bootstrap can use
`{{chrome_debug_port}}`, `{{chrome_debug_url}}`, and
`{{chrome_user_data_dir}}`. A localhost Chrome remote debugging endpoint lets
local processes control that browser instance, so use this mode only for
workspaces where that local access is acceptable.

Use named profiles only when prompt files, scaffold files, or reusable runtime
bundles are needed:

```bash
wt profile create codex
wt config extract "$(git rev-parse --git-common-dir)/wt/config.toml"
wt config inline "$(git rev-parse --git-common-dir)/wt/profiles/codex/profile.toml"
```

Named profile `profile.toml` is an override layer: omitted `[agent]` fields
inherit lower-precedence config, present fields override, and `args = []`
explicitly clears inherited agent args.

Omitting `--profile` means the effective config. `default` is not a profile
name.

Selected profile subsets for local TaskDocuments belong to saved workflow matrix
mode:

```bash
wt workflow task --mode matrix --profiles devtools-port,mcp-owned chrome-devtools-isolation
wt run workflow
```

Workflow matrix mode stores `profiles = [...]` in the Workflow TOML and supports
exactly one local TaskDocument across the named profiles. Profile order is the
user-provided order. Duplicate names, missing profiles, and reserved `default`
fail before Workflow files, TaskRuns, or worktrees are created. Direct
`wt run task` remains the immediate single-worktree path; use `--profile <name>`
there for one named profile.

## Command Map

| Command | Purpose |
| --- | --- |
| `wt init` | Create or preview repository config |
| `wt doctor` | Check configured providers and local tools |
| `wt run issue` | Start work from a provider issue |
| `wt run pr` | Start worktrees from pull requests |
| `wt run branch` | Start work from branch-name text |
| `wt task list` | List saved local TaskDocuments |
| `wt task import` | Import provider issues as local TaskDocuments |
| `wt run task` | Start work from local TaskDocuments |
| `wt task publish` | Publish local TaskDocuments as provider issues |
| `wt workflow` | Prepare, inspect, repair, and complete saved workflows |
| `wt run workflow` | Start runnable tasks from saved workflows |
| `wt ui` | Start the read-only personal state web UI |
| `wt inspect` | Read a work dossier for a branch, worktree, or TaskRun |
| `wt agent status` | Observe the matching task agent surface once |
| `wt agent watch` | Poll the matching task agent surface, with optional timeout and heartbeat |
| `wt agent hook install claude` | Install Claude Code file inbox delivery through local hooks |
| `wt agent hook uninstall claude` | Remove wt-managed Claude Code inbox hook entries |
| `wt agent hook install codex` | Install Codex file inbox delivery through a user-level trusted dispatcher hook |
| `wt agent hook uninstall codex` | Remove wt-managed Codex inbox hook and trust entries |
| `wt send` | Send a message to the matching task agent surface |
| `wt done` | Remove completed or disposable worktrees and branches |
| `wt config` | Print, edit, extract, or inline config |
| `wt profile` | List named profiles (omission default for `wt profile list`) or scaffold a new one with `wt profile create <name>` |
| `wt site` | Inspect and manage local site provider helpers |

Run `wt <command> --help` for the current contract.

## Project Status

`wt` is pre-1.0. Breaking user-facing changes to the CLI, config format, or
persisted state files are released as `0.x.0` minor versions until the model
stabilizes.

Development happens on `develop`. `master` is the release branch. Regular
development commits do not bump `Cargo.toml`; release PRs bump `Cargo.toml` and
`Cargo.lock` once, then the release commit is merged back into `develop`.

## Development

```bash
git clone https://github.com/hoetaek/wt.git
cd wt
cargo fmt --all --check
cargo check --locked --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked --all-features
```

Behavior-sensitive changes should update help text, tests, and docs together.
For user-facing model changes, read [docs/consistency.md](docs/consistency.md).

## More Information

- [CHANGELOG.md](CHANGELOG.md) for release notes
- [CONTRIBUTING.md](CONTRIBUTING.md) for development and release checks
- [SECURITY.md](SECURITY.md) for vulnerability reporting
- [docs/architecture.md](docs/architecture.md) for implementation boundaries
- [docs/consistency.md](docs/consistency.md) for the canonical UX model

## License

Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))
