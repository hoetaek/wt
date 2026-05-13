---
name: wt-stack
description: "Use when working in the wt repository or with wt itself to create, reset, run, monitor, or troubleshoot wt stack workflows, including turning a sequence of tasks/issues into a stack file, running `wt stack run latest`, handling `wt stack complete --run-next`, validating branch/worktree state, and diagnosing cmux/agent prompt startup problems."
---

# WT Stack

Use this skill when the user wants Codex to plan or operate an ordered branch stack with `wt stack`.

## Model

- A stack is ordered work: each item branch is based on the previous completed item branch.
- `wt stack new ...` and `wt stack issue ...` create stack TOML without starting worktrees.
- `wt stack run <stack|latest>` starts one runnable item and marks it `running`.
- The item agent should finish by committing its work, then running `wt stack complete <stack|latest> <item> --run-next`.
- `--run-next` marks the current item done and starts the next prepared or failed item.
- Final recovery is Git-native: merge the top branch or open stacked PRs.

## Before Operating

1. Inspect current state:

```bash
git status --short --branch
git worktree list
find .local/stacks -maxdepth 1 -type f | sort
```

2. Read the target stack file before running it:

```bash
sed -n '1,220p' .local/stacks/<file>.toml
```

3. If there is a `running` item, do not start another item blindly. Check its worktree and branch status first.

## Creating A Stack

For branch-name tasks:

```bash
wt stack new "first task" "second task" "third task" --base master
```

For provider issues:

```bash
wt stack issue 123 456 789 --base master
```

Keep stack order base-to-top. Put enabling/refactor work first, dependent work after it, and docs/tests cleanup last when appropriate.

## Running A Stack

When the user explicitly asks to run:

```bash
wt stack run latest
```

After running, verify the result:

```bash
sed -n '1,90p' .local/stacks/<file>.toml
git worktree list
```

Expected first-run result:

- stack status is `running`
- exactly one item is `running`
- a matching worktree exists
- cmux/agent prompt is actually executing, not just pasted

## Monitoring Agent Startup

If `wt stack run` reports `Agent prompt ... sent`, confirm the agent is working when in doubt:

```bash
cmux list-workspaces
cmux tree --workspace <workspace>
cmux read-screen --workspace <workspace> --surface <surface> --lines 80
```

Healthy states include `Working`, tool calls, or code/test activity. Suspicious states:

- ready marker timeout
- input line shows `[Pasted Content ...]` and is idle
- no worktree changes or agent output after prompt submission

If prompt is pasted but idle, send one Enter to the agent surface:

```bash
cmux send-key --workspace <workspace> --surface <surface> enter
```

Then update the experiment or issue notes with the observed cause.

## Resetting A Bad Run

Use this only when the user asks to reset/retry a stack run that started the wrong item, failed during agent startup, or should be discarded before useful commits were made.

1. Check the running worktree:

```bash
git -C <worktree-path> status --short --branch
```

2. If it has no useful work to keep, remove it with `wt done`:

```bash
wt done <item-or-branch>
```

3. Reset only the stack state fields that were changed by the bad run:

```toml
status = "prepared"
...
status = "prepared"
error = ""
```

Do not delete or revert unrelated user edits. Do not delete branches that contain useful commits or work the user wants to keep. If there is useful work, recover or commit it first instead of resetting the stack state.

## Completion Semantics

Before treating an item as complete, verify:

```bash
git -C <worktree-path> status --short --branch
git log --oneline <parent>..<branch>
```

Completion should fail or be delayed if:

- relevant tracked/untracked changes are still dirty
- there are no commits ahead of the parent
- the item did not run tests appropriate for its change

The intended item handoff command is:

```bash
wt stack complete latest <item> --run-next
```

## Troubleshooting

- If `latest` chooses the wrong file, pass the explicit `.local/stacks/<file>.toml`.
- If an item is already `running`, inspect it instead of running the stack again.
- If `complete --run-next` does not start the next item, inspect the stack TOML and current worktree list.
- If cmux startup or prompt submission behaves oddly, record a short experiment under `.local/experiments/` with baseline, hypothesis, command output, and result.

## Validation

For wt code changes touching stack behavior, run focused checks first:

```bash
cargo fmt --check
cargo test stack
cargo run --quiet -- stack --help
cargo run --quiet -- stack complete --help
```

Run full `cargo test` before declaring the stack feature work complete.
