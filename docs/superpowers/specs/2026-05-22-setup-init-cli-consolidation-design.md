# Setup/Init CLI Consolidation Design

- Date: 2026-05-22
- Status: Drafted via brainstorming, pending plan
- Scope: Pre-1.0 breaking change to wt's setup/install command surface

## Problem

Setup-shaped commands are scattered across six surfaces with overlapping responsibilities, no consistent mental model, and unclear lifecycle scope. New users do not know which to run, in which order, or what each one changes.

Current surfaces:

- `wt init` — workspace config wizard (writes `.wt.toml` or `.git/wt/config.toml`)
- `wt install` / `wt uninstall` — compatibility alias for `wt hooks setup`/`uninstall`
- `wt hooks setup` / `wt hooks uninstall` — install agent CLI hooks
- `wt hooks claude` / `wt hooks codex` — per-agent compatibility aliases
- `wt shell-init zsh|bash` — print shell integration eval source
- `wt doctor` — health check

In addition, `.git/wt/` subdirectories are created lazily on first write. Read-side commands and external tools must defensively handle "directory does not exist yet". There is no eager bootstrap step.

## Goals

1. Reduce setup surface to exactly two user-facing commands that map cleanly to the two lifecycle scopes wt actually has.
2. Make state directory presence a property of "this repo has been `wt init`'d", not "wt happens to have written there once".
3. Keep behavior idempotent so users can re-run either command safely without flags.
4. Stay within the pre-1.0 stance from `CLAUDE.md`: prefer consistency over backwards compatibility.

## Non-goals

- Touching the `wt init` config wizard interaction beyond what is required for re-run semantics.
- Changing what hooks actually do or which agent CLIs are supported.
- Changing `wt shell-init` output; it remains the source of truth that the shell integration eval'es.
- Touching `wt doctor`'s check set beyond adding pointers to the new commands.
- Repo-state nuke (deleting `.git/wt/tasks/*.toml` etc.). That is not a setup concern.

## User-facing model

Two commands, two lifecycle scopes:

| Command | Scope | What it changes |
|---|---|---|
| `wt setup` | per-machine, once per user | `~/.claude/settings.json`, `~/.codex/config.toml`, `~/.zshrc` or `~/.bashrc` |
| `wt init` | per-repo, once per repo | `.wt.toml` or `.git/wt/config.toml`, `.git/wt/` core directories |

A user running wt for the first time on a new laptop, on a new repo, runs exactly:

```
wt setup
cd <repo>
wt init
```

`wt doctor` is the cross-check that tells them which of the two (if any) still needs to run.

## `wt setup`

Per-machine setup. Idempotent. Interactive by default with per-step prompts.

### Surface

```
wt setup            # interactive, ask per step
wt setup --yes      # accept every step without prompting (CI, scripts)
wt setup --dry-run  # print what would change, write nothing
wt setup --remove   # reverse what `wt setup` installed
```

No subcommands. No `--only`, `--skip`, `--agent` flags. Per-step `[y/N]` prompts give users in-the-moment fine-grained control; re-running `wt setup` picks up whatever was previously declined.

### Steps

For each step: detect → already-applied check → prompt (`[y/N]`, default No) → apply → report.

1. **Claude hooks** — if `claude` is on `PATH`, prompt to install wt-managed entries to `~/.claude/settings.json`. If wt-managed entries are already present and current, skip silently and report "already installed".
2. **Codex hooks** — same shape against `~/.codex/config.toml`.
3. **Shell integration** — detect login shell via `$SHELL`. If `zsh`, prompt to append `eval "$(wt shell-init zsh)"` to `~/.zshrc`. If `bash`, same against `~/.bashrc`. If neither, print the eval line and instruct the user to add it manually. If the line is already present (exact match against any line in the file), skip silently and report "already integrated".

### Output shape

Use the per-step output convention from `docs/consistency.md` (verify when implementing). Summary at the end:

```
Summary: 2 hooks installed (claude, codex), shell integration added to ~/.zshrc.
Next: run `wt init` inside a git repo to scaffold wt state.
```

If everything was already in place:

```
Summary: nothing to do; wt setup is current.
```

### `--remove`

Reverses each step. Per-step prompts again so the user can choose what to undo. Removes only wt-managed entries; preserves user-managed hooks, cmux hooks, and unrelated trust state (this is already the contract of `wt hooks uninstall` — reuse it).

Does NOT touch:

- `.git/wt/` state directories or any file inside them
- `.wt.toml`, `.git/wt/config.toml`
- Worktrees themselves
- The `wt` binary

If the user wants to remove repo-level state, they use repo-level commands (`wt clean`, `wt done`), not `wt setup --remove`.

### Industry-standard prompt convention

`[y/N]` — capital letter is the default. Enter without input selects No. Standard for prompts that modify user-global files (`~/.claude/...`, `~/.zshrc`). The `--yes` flag is the standard escape hatch for non-interactive contexts.

## `wt init`

Per-repo setup. Idempotent. Re-running re-prompts the config wizard with existing values as defaults; folder bootstrap is silent if already present.

### Surface (unchanged from today)

Existing flags stay: `--local`, `--shared`, `--preset`, `--minimal`, `--agent`, `--agent-arg`, `--agent-command`, `--issue-provider`, `--site-provider`, `--gh-user`, `--yes`, `--dry-run`, `--force`. Their semantics do not change.

### Behavior changes

1. After the wizard writes the config file (or detects an existing one), eagerly create the following directories under `<git-common-dir>/wt/` if they do not exist. Directory creation is idempotent.

   **Core (eagerly created):**
   - `tasks/`
   - `messages/`
   - `task-runs/`
   - `agent.state/`
   - `worktrees/`

   **Lazy (left for first user action to create):**
   - `workflows/` — first created by `wt workflow new`
   - `archive/workflows/` — first created by `wt workflow complete`
   - `ideas/` — first created by `wt-idea`
   - `retrospectives/` — first created by `wt-work` retrospective
   - `profiles/` — first created by `wt profile create`

   Rationale: the core set is what read-side commands (`wt list`, UI snapshot, `wt msg list`) walk on every invocation. Having them eagerly present means read commands do not have to special-case "not bootstrapped yet". The lazy set has a clear single user action that triggers first creation, so lazy stays cleaner than `.keep` placeholders.

2. Re-running `wt init` on an already-initialized repo:
   - Wizard prompts again, with previous values prefilled as defaults.
   - Folder bootstrap step is silent (no "skipped" noise) if directories already exist.
   - `--force` only governs overwriting an existing config file. Folder bootstrap never needs `--force`.

### What `wt init` does NOT do

- Does not install hooks (that's `wt setup`).
- Does not modify shell rc files.
- Does not touch global user state outside the repo.

## Removed surfaces

Pre-1.0 breaking change. No hidden aliases, no deprecation warnings — the commands disappear from the binary.

- `wt install`
- `wt uninstall`
- `wt hooks` (entire subtree: `setup`, `uninstall`, `codex`, `claude`)

Migration message: none in-binary. README and `docs/architecture.md` get updated. `wt doctor` mentions `wt setup` when relevant.

## Doctor integration

`wt doctor` already checks "configured providers and required local tools". Extend its report so that:

- Missing claude/codex hooks → "Run `wt setup` to install agent hooks."
- Missing shell integration → "Run `wt setup` to add shell integration."
- Missing `.git/wt/config.toml` (or `.wt.toml`) in current repo → "Run `wt init` to configure this repo."
- Missing core directories under `.git/wt/` → "Run `wt init` to bootstrap state directories."

`wt doctor` itself does not auto-fix; it points at the right command.

## Idempotency contract

Both commands satisfy:

> Running the command twice in a row from the same starting state produces the same final state, with the second run reporting "nothing to do" or silently skipping each step.

Concretely:

- `wt setup` re-run after a complete first run: every step reports "already installed" or "already integrated". Exit 0.
- `wt setup --remove` re-run after a complete first removal: every step reports "already absent". Exit 0.
- `wt init` re-run with the same answers: config file is byte-identical (or skipped if unchanged), all core directories already exist. Exit 0.

## Implementation surfaces affected

This is a planning sketch, not a contract — the plan step will confirm.

- `src/cli.rs` — remove `Install`, `Uninstall`, `Hooks` variants and `HooksCommand`, `HookAgentCommand`, `HookAgent` enums. Add `Setup { yes: bool, dry_run: bool, remove: bool }`.
- `src/commands/install.rs` — repurpose or replace with `src/commands/setup.rs`. Move hook install/uninstall logic from `install.rs` into shared helpers callable by `setup`.
- `src/commands/init.rs` — add a "bootstrap core dirs" step after config write. Adjust re-run semantics so wizard prefills from existing config.
- `src/commands/shell_init.rs` — keep as-is. `setup` calls into this to get the eval line, then writes it to the detected rc file with idempotent line-presence check.
- `src/commands/doctor.rs` — add checks for hooks/shell/core-dirs with command pointers.
- `src/setup/` — existing helpers stay; `setup` orchestrates them.
- Documentation: `docs/architecture.md`, `docs/consistency.md`, `README.md` (verify name) — update command references.
- Tests: integration tests for `wt setup` idempotency, `wt setup --remove` reversal, `wt init` core dir bootstrap, doctor pointers. Remove tests for deleted surfaces.

## Open implementation details (for plan)

- Detecting "wt-managed entries" in `~/.claude/settings.json` and `~/.codex/config.toml` — current `wt hooks` logic already does this; confirm it's reusable.
- Detecting "shell integration line already present" — exact-line match or marker comment? Exact-line match is simpler and what most tools use.
- Behavior when `$SHELL` is something other than zsh/bash (fish, nu, etc.) — print eval line + manual instructions, do not error.
- Whether `wt setup` should refuse to run inside a git repo and prompt the user that they probably meant `wt init` — out of scope for first cut; leave both runnable from anywhere.

## Verification plan

- Unit: each step's idempotency check returns correct already-installed/already-absent verdict.
- Integration: full `wt setup` → `wt setup` (idempotent) → `wt setup --remove` → `wt setup --remove` (idempotent) cycle against a temp `$HOME`.
- Integration: `wt init` in a fresh repo creates all core dirs and no lazy dirs.
- Integration: `wt init` re-run after a user creates `workflows/<id>.toml` does not disturb that file.
- Doctor: with each piece missing, doctor names the correct next command.
