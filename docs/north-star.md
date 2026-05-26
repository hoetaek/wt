# wt North Star

> `wt` is not a project marching toward 1.0. It is a personal harness that gets
> clearer through use. There is no finished model, only a direction.

This document records the direction `wt` is currently moving toward and the
principles that should guide model changes. When the model evolves, this
document evolves with it. Old versions live in git history.

Relationship to other documents:

- `README.md`: public identity, persona, and what `wt` is not.
- `docs/consistency.md`: canonical user-facing UX and state model.
- `docs/architecture.md`: module boundaries, layer ownership, and refactor
  targets.
- `docs/north-star.md`: this document. Deeper rationale, design principles, and
  model direction.

If this document and `docs/consistency.md` disagree about a path, command, or
state shape, the branch is incomplete. Resolve the disagreement before merge;
do not leave two canonical surfaces in the repository.

## Identity

`wt` is a worktree-based agent orchestration harness for software engineers
working with AI coding agents.

The identity has six parts:

1. Working with AI coding agents is the usage context.
2. Software engineers are the persona.
3. Git worktrees are the substrate.
4. Agent orchestration is the core job.
5. Harness is the tool type: it supports agents, it does not become the agent.
6. Easy setup is the UX promise.

`wt` is a personal tool. It can integrate with team systems such as GitHub and
Linear, but it does not require team-wide adoption.

## What wt Is Not

- `wt` is not a team standard tool.
- `wt` is not an agent runtime; Codex, Claude, Gemini, and similar tools do the
  agent work.
- `wt` is not a chatbot or general AI agent framework.
- `wt` is not a hosted service.
- `wt` is not a daemon.
- `wt` is not organized around a 1.0 finish line.

## Persona

The primary user is a software engineer who:

- uses AI coding agents as part of daily development,
- is comfortable with Git worktrees and CLI workflows,
- can manage dotfiles and local tooling,
- wants to run multiple agents in parallel,
- wants to design their own collaboration workflow,
- does not want a heavy GUI, daemon, hosted service, or team-wide tool mandate,
- accepts breaking model changes when they make the tool clearer.

The recurring decision test is:

> Would this persona want it, understand it, and use it? Does it make the model
> clearer now?

## Harness Principles

1. `wt` is not the active worker. Agents do the work; `wt` provides worktrees,
   state, messages, setup, and handoff.
2. `wt` must not force an external tool dependency. cmux and specific agent CLIs
   are replaceable integrations.
3. `wt` is a stateless CLI by default. It runs, records durable state, and exits.
4. `wt` mediates through data. Messages, status, tasks, workflows, and config
   should be visible, inspectable files.
5. `wt` makes location a contract. Where something lives should explain what it
   means.
6. `wt` does not automatically modify Git-tracked source. Agent adapters may
   create worktree-local, Git-excluded files, but tracked file edits require
   explicit user opt-in.
7. `wt` follows direction, not a finish line. Compatibility is a cost; clarity
   is an asset.

The seventh principle is the meta-principle. The model is allowed to evolve, but
only toward greater clarity.

## Direction-Driven Design

`wt` should choose clarity over compatibility while it remains a personal
pre-1.0 tool.

- Prefer one clear canonical model over aliases and fallback behavior.
- Reject ambiguous combinations early.
- Remove old state models when a new one becomes canonical.
- Update code, docs, CLI help, tests, and skills together when a user-facing
  model changes.
- Treat breaking changes as normal model evolution, represented by pre-1.0 minor
  releases when releasing.

The question is not "is this needed before 1.0?" The question is:

> Does this make the current model clearer?

## Decision Layers

### Locked Identity Decisions

These define what `wt` is:

- `wt` is a personal agent orchestration harness.
- `wt` integrates with team systems but is not a team standard tool.
- The persona is a software engineer working with AI coding agents.
- The harness principles guide all model changes.

### Current Direction

These are the model directions that should shape upcoming work:

- Stateless CLI remains the default.
- File-based state and messages are the durable contract.
- cmux is a transport and surface detail, not canonical task or workflow state.
- Personal storage should live under the Git common directory rather than
  repo-root `.local` state.
- Activity, inbox messages, and status should remain separate channels.
- Scope should be modeled as global, team integration, personal, and runtime
  actor context.
- Agent adapters must not automatically modify tracked source.

### Open Decisions

These require implementation experience before becoming canonical:

- Exact agent adapter file layout and marker behavior.
- Share/export mechanisms.
- Message lease duration, reclaim policy, and daemon/push delivery timing.
- `wt://` URL shape and implementation timing.
- Runtime trait boundaries and optional capabilities.
- Global config path details across XDG, macOS, and Windows.
- When to make multi-participant worktrees first-class.
- Whether worker-to-sub-worker spawning belongs in the core model.

## Scope Model Direction

The target state model has four scopes:

```text
~/.config/wt/                  # Global: this machine
<repo>/.wt.toml                # Team integration config, committed
<git-common-dir>/wt/           # Personal repo work, not committed
<git-common-dir>/wt/runtime/agents/<name>/
                               # Runtime actor context inside personal storage
```

Tier 2 is team integration configuration, not shared work data. It describes how
this repository connects to systems the team already uses, such as GitHub or
Linear.

The intended config precedence is:

```text
global < team integration < personal < command-line flags
```

`wt config` should show where each effective value came from. Debuggability is
part of the contract.

All worktrees for one repository should resolve the same personal storage root
through `git rev-parse --git-common-dir`. This avoids symlink conventions and
repo-root ignore-file patches.

Inside the personal root, the current direction is a four-bucket contract:

```text
config/       # local config and profiles
planning/     # ideas, specs, retrospectives
execution/    # TaskDocuments, Workflows, TaskRuns, archive
runtime/      # agent-owned inboxes, sessions, supervisors, observations
```

## Identity Model Direction

Worktree identity should separate stable machine identity from human labels:

```toml
id = "wt_20260519_103045_a3f8"
display_name = "feat-add-schema"
branch = "feat-add-schema"
path = "/abs/path/to/worktree"
created_at = "2026-05-19T10:30:45Z"
```

- `id` is opaque, stable, and immutable.
- `display_name` is human-facing and may change.
- `branch` and `path` are recorded facts and may change.

Agent IDs identify communication actors with one agent-name segment:

```text
agents/<display_name>
agents/<display_name>-<role>
```

`runtime/agents/<name>` is the filesystem form of `AgentId` `agents/<name>`.
When display names collide, agent naming can fall back to the opaque worktree id.

## Communication Model Direction

Activity, inbox, and status are separate concepts:

| Channel | Writer | Reader | Data | Location |
| --- | --- | --- | --- | --- |
| Activity log | Hooks | UI and debugging | Append-only JSONL | `runtime/agents/<name>/activity.jsonl` |
| Inbox | Intended sender or delivery owner | Target agent and scope owner | Scoped TOML file queue | `runtime/agents/<name>/inbox/<state>/` |
| Status | Hooks | Anyone | Single TOML snapshot | `runtime/agents/<name>/status.toml` |

Activity is not communication. Hooks may automatically update activity and
status, but inbox messages should represent intentional signals or defined
lifecycle events.

A scoped message schema can borrow A2A vocabulary while staying local:

```toml
[meta]
id = "msg_abc123"
created_at = "2026-05-19T10:30:00Z"
from = "agents/feat-wire-api"
to = "agents/feat-add-schema"

[envelope]
kind = "request"
priority = "normal"
expects_response = false
correlates_with = "msg_xyz"

[scope]
kind = "workflow"
id = "2026-05-19-001"

[delivery]
state = "new"
attempts = 0

[body]
summary = "Schema review needed"

[[body.parts]]
type = "text"
content = "..."
```

Delivery state should be visible in state directories such as `inbox/new`,
`inbox/claimed`, `inbox/delivered`, `inbox/retry`, and `inbox/failed`. Hook
polling can remain a compatibility consumer, but it should use the same scoped
delivery lifecycle rather than a separate unread/read model. Daemon or push
delivery timing remains an open decision, not the default model.

## Agent Adapter Policy

Agent adapters are the one area where `wt` may need worktree-local files. The
policy is strict:

Allowed automatic writes:

- files under `<git-common-dir>/wt/`,
- per-worktree Git exclude files from `git rev-parse --git-path info/exclude`,
- untracked adapter files that are already excluded.

Disallowed automatic writes:

- tracked `CLAUDE.md` or `AGENTS.md`,
- tracked `.gitignore`,
- tracked `.claude/settings.json` or similar agent config,
- any other tracked source file.

If a tracked agent instruction file already exists, `wt` should use an
agent-supported local override, ask for explicit opt-in, or fail with clear
guidance. It should not silently patch tracked files.

Code that writes adapter files should be isolated under an agent-adapter layer.
The rest of `wt` should write personal state under the storage root.

## Runtime Direction

Runtime integration should separate core runtime behavior from optional
capabilities:

```rust
trait Runtime {
    fn open_workspace(name, cwd) -> WorkspaceHandle;
    fn spawn_in_workspace(workspace, command) -> SurfaceHandle;
    fn send(surface, text);
    fn send_key(surface, key);
    fn close_workspace(workspace);
}

trait ScreenCapture {
    fn read_screen(surface) -> String;
}
```

Screen capture is not core. A headless runtime may not support it. The model is
validated only when a second runtime exists.

## Provider Direction

Provider traits should match real external capabilities:

```rust
trait IssueProvider {
    fn fetch_issue(id: &str) -> Issue;
    fn create_issue(task: &TaskDocument) -> Issue;
}

trait PullRequestProvider {
    fn fetch_pr(id: &str) -> PullRequest;
    fn create_pr(branch: &str, body: &str) -> PullRequest;
}

trait ReviewProvider {
    fn comment_on_pr(id: &str, body: &str);
    fn read_reviews(id: &str) -> Vec<Review>;
}
```

GitHub may implement all three. Linear may implement only issue behavior. This
keeps team-system integration from turning into one overbroad provider concept.

## Evolution Order

Each step should make the model clearer:

1. Lock identity across README, Cargo metadata, CLI help, and agent guidance.
2. Move the four-scope model into `docs/consistency.md`.
3. Introduce a storage-root abstraction.
4. Move personal storage toward the Git common directory.
5. Add global config and worktree registry.
6. Add message bus and agent hooks.
7. Isolate cmux behind runtime/surface boundaries.
8. Split provider traits by external capability.
9. Add share/export only when real usage demands it.

Each model-change PR should update the relevant code, docs, help text, tests,
and skills together.

## Summary

`wt` is a personal harness that gets clearer through use. Identity is stable;
the model evolves. Compatibility is a cost, and clarity is an asset.
