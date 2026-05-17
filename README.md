# wt

`wt` is a Git worktree workspace manager for starting ready-to-code branches
from issues, pull requests, branch-name text, or local task files.

It creates the worktree, applies repo setup, opens a cmux workspace when
configured, registers local development sites, and can hand a prepared prompt to
an agent such as Codex, Claude, or Gemini.

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
```

Start work:

```bash
wt issue
wt issue PROJ-123 --base .
wt pr
wt pr 42 43
wt new add profile docs
wt task run
wt task run add-profile-docs
```

Prepare local TaskDocuments without starting work:

```bash
wt task import PROJ-123
wt task import
wt task publish add-profile-docs
```

Prepare saved workflows when local tasks or issues need coordination:

```bash
wt workflow task --mode single add-schema wire-api --base .
wt workflow task --mode batch add-schema wire-api --base main --objective "Ship search"
wt workflow issue --mode stack 123 456 789 --base main --pr draft
wt workflow run
wt workflow repair 2026-05-16-001
wt workflow complete 2026-05-16-001 add-schema --run-next
```

Inspect, observe, message, and clean worktrees:

```bash
wt list
wt inspect
wt agent status <branch|worktree|task-run-id>
wt agent watch <branch|worktree|task-run-id>
wt send <target> "please report current status"
wt done <target>
```

Omitting a work target opens a selector only in interactive human use. In
`--json`, `--quiet`, or non-TTY automation, pass an explicit branch, worktree
path/name, or TaskRun id.

`wt done` removes worktrees and local branches. It does not merge the branch.
Land reviewed work with Git or pull requests first.

## Core Model

- `wt new <words...>` starts an ad hoc branch worktree from branch-name text.
- `wt issue` starts a worktree from an existing provider issue.
- `wt pr` opens existing pull request branches as worktrees.
- `TaskDocument` files in `.local/tasks/<task>.toml` define prepared local work.
- `wt task import [<issue>...]` imports provider issues as TaskDocuments and
  records `[origin]`; it does not start worktrees.
- `wt task run [<task>...]` starts one worktree per selected TaskDocument.
- `wt task publish [<task>...]` creates provider issues from TaskDocuments and
  records `[origin]`; it does not start worktrees.
- `Workflow` files in `.local/workflows/<id>.toml` save coordinated execution.
  Optional `objective` records the larger human goal for the saved plan.
  `single` shares one workspace, `batch` runs independent branches from one
  base, and `stack` runs ordered branches as a parent chain.
- `TaskRun` files in `.local/task-runs/<id>.toml` record execution attempts.
  Execution state is separate from branch landing.
- `wt inspect [<target>]` is the read-only work dossier for a branch, worktree,
  or TaskRun.
- `wt agent status [<target>]` observes the current agent/cmux state, and
  `wt agent watch [<target>]` polls it. Agent state is separate from
  `TaskRun.status`.

`wt workflow` is the canonical prepared-work surface. `single`, `batch`, and
`stack` are workflow mode values, not separate command surfaces. Use
`wt inspect` for read-only dossiers and `wt agent status` / `wt agent watch`
for runtime observation.

## Coordinator Handoff

Task prompts started by `wt task run` and `wt workflow run` include coordinator
handoff instructions when cmux coordinates are available. The prompt gives the
agent a `cmux send --workspace ... --surface ...` report command and a matching
`cmux send-key ... enter` command.

Agents report back in this shape and then keep ownership of review follow-up
for their task:

```text
Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=<pr>; Risks or follow-ups=<risks>
```

Immediate `wt task run` work reports `PR=none`. Workflow single and batch tasks
also report `PR=none`; stack tasks follow the workflow row's pull-request
handoff intent. Omit `--pr` or pass `--pr none` to avoid opening a PR,
`--pr draft` to create a draft PR and leave it draft, or `--pr ready` to create
a review-ready PR directly. Workflow rows persist PR intent only as
`pull_request = "draft"` or `pull_request = "ready"`; an omitted value means
`PR=none`. PR-opening tasks create a body file from
`.github/pull_request_template.md`, fill a review-focused description, and pass
it to `gh pr create --body-file <pr-body-file>`. If Codex/GitHub review or
coordinator feedback asks for changes, the same agent updates the branch, reruns
checks, pushes, refreshes the PR body only if it became stale, and sends an
updated report. The coordinator advances, lands, and cleans up only after review
passes.

## Configuration

`wt` loads config from:

1. `--config <path>`
2. `.wt.toml` as shared project config
3. `.local/.wt.toml` as private checkout config

Inspect the effective config:

```bash
wt config
wt config --profile codex
```

Workflow preparation reads repository defaults from the effective config:

```toml
[workflow.defaults]
pull_request = "draft"        # none | draft | ready
landing = "after_review"      # manual | after_review
landing_requires_approval = true
```

`wt workflow task --objective <text>` and
`wt workflow issue --objective <text>` write top-level workflow context for the
larger goal. `wt workflow task` and `wt workflow issue` write the effective
landing policy to the prepared workflow file under `[policy]`. Stack-mode tasks
also materialize the effective PR handoff on each task row; an explicit
`--pr none|draft|ready` overrides `workflow.defaults.pull_request` for that
prepared workflow. Single and batch workflows still report `PR=none`.

Small private agent config can stay inline:

```toml
[profile.agent]
cli = "codex"
args = ["--model", "gpt-5.5"]

[workspace]
tabs = ["lazygit", "nvim"]
```

Use named profiles only when prompt files, scaffold files, or reusable runtime
bundles are needed:

```bash
wt profile create codex
wt config extract .local/.wt.toml
wt config inline .local/profiles/codex/profile.toml
```

Omitting `--profile` means the effective config. `default` is not a profile
name.

## Command Map

| Command | Purpose |
| --- | --- |
| `wt init` | Create or preview repository config |
| `wt doctor` | Check configured providers and local tools |
| `wt issue` | Start work from a provider issue |
| `wt pr` | Start worktrees from pull requests |
| `wt new` | Start work from branch-name text |
| `wt task import` | Import provider issues as local TaskDocuments |
| `wt task run` | Start work from local TaskDocuments |
| `wt task publish` | Publish local TaskDocuments as provider issues |
| `wt workflow` | Prepare, inspect, run, repair, and complete saved workflows |
| `wt inspect` | Read a work dossier for a branch, worktree, or TaskRun |
| `wt agent status` | Observe the matching task agent surface once |
| `wt agent watch` | Poll the matching task agent surface |
| `wt send` | Send a message to the matching task agent surface |
| `wt done` | Remove completed or disposable worktrees and branches |
| `wt config` | Print, edit, extract, or inline config |
| `wt profile` | List or create named profile configs |
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
