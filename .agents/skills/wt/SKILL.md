---
name: wt
description: "Use when working in the wt repository and Codex needs to operate, explain, configure, or troubleshoot wt itself, including wt issue, wt pr, wt new, wt open, wt done, wt init, profiles, batches, config files, agent prompts/scaffold, local site setup, cmux workspaces, or command/help consistency."
---

# WT

Use this skill to choose `wt` commands and explain behavior without inventing a second UX model.

## Source Of Truth

- Prefer `wt --help` and `wt <command> --help` for current syntax.
- If working inside the `wt` repo, also read `README.md` and `docs/consistency.md` before changing docs or behavior.
- Run `wt doctor` with `-C` or `--config` when validating a target project config.
- Do not assume `default` is a profile name; omitted `--profile` means effective config.

## Concept Model

- Worktree: checkout workspace for an issue, PR, or branch-name text.
- Profile: runtime environment bundle: agent CLI, args, prompts, and scaffold. It answers how to run.
- Batch: prepared issue list and item status record. It answers what to run.
- Agent CLI: behavior class: `codex`, `claude`, `gemini`, or `none`. `command` only overrides the launch string.
- Local context injection: use `inject_local_context`, not agent-specific names, for rendered text appended to local agent context files.

## Command Selection

- Use `wt issue [target]` for issue workflow. Omit `target` to select from the provider list.
- Use `wt pr [number]` for existing PR branch workflow. Omit `number` to select from open PRs.
- Use `wt new <words...>` for a new branch-name workspace.
- Use `wt open [target]` to open an existing worktree; use `wt list` to inspect; use `wt done [target...]` to clean up.
- Use `wt batch prepare <issue>...` to snapshot issues without creating worktrees; use `wt batch run <path|latest>` to execute prepared or failed items.
- Use `wt profile` to list named profiles, `wt profile create <name>` to create scaffold, and `wt profile promote <name>` only to move existing inline `[profile.*]` settings into a named profile.

## Profile Rules

- Keep simple default runtime inline in `.local/.wt.toml` under `[profile.agent]`.
- Use `[profile] name = "<name>"` to select a named profile from `.local/profiles/<name>/profile.toml`.
- Never combine `[profile] name` with inline `[profile.agent]`, `[profile.worktree]`, `[profile.setup]`, `[profile.site]`, `[profile.workspace]`, or `[profile.test]`.
- For `wt issue --profile <name>` and `wt new --profile <name>`, expect a separate profiled worktree with branch and workspace names including the profile name.
- For `wt pr --profile <name>`, expect the PR branch name as-is and only apply profile config.
- Use `--parallel` for all named profiles on `issue` or `new`; do not combine it with `--profile`.

## Config Conventions

- Config load order: explicit `--config`, otherwise shared `.wt.toml` plus private `.local/.wt.toml`.
- `.local/` is for private profiles, issue snapshots, and machine-specific config.
- Canonical site provider spelling is `docker_proxy`.
- Canonical completion command is `wt completion <shell>`.

## Validation

- Before changing `wt` itself, inspect `wt --help`, affected command help, README, docs, tests, and implementation together.
- After config or docs changes, run the smallest useful public-interface check such as `wt doctor`, `wt profile --json`, or `cargo test` in the `wt` repo.
- Keep help text, README examples, config shape, generated files, and stored batch state aligned.
