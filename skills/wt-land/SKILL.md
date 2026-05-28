---
name: wt-land
description: "Use after wt work is reviewed: respect workflow policy, pass workflow-linked tasks when needed, merge, prove ancestry, and clean with `wt done`."
---

# WT Land

Use this skill after `wt-coordinate` review says the work is acceptable. Do not
use it to monitor active agents or request fixes; use `wt-coordinate` for that.

In the LEAF model, this skill owns the land/close step between
review/sync and retrospect. It proves integration or discard, performs
applicable workflow pass, and cleans only after that closure is safe.

Object model, status boundaries, and pass vs cleanup commands: see
`../wt-lifecycle/references/task-lifecycle.md`.

## Boundaries

- Do not run cleanup before ancestry or explicit discard intent is clear.
- Coordinator landing must not absorb task-branch merge conflict ownership. A
  merge conflict during landing is a task-branch update problem until the user
  explicitly asks the coordinator to take it over.

## Preflight

Inspect the exact target before changing state:

```bash
wt inspect <branch|worktree|task-run-id>
wt agent status <branch|worktree|task-run-id>
git -C <worktree> status --short --branch
git -C <worktree> log --oneline <parent>..<branch>
```

Confirm:

- the worktree is clean
- useful commits exist ahead of the parent
- checks appropriate to the change have passed or known gaps are acceptable
- for workflow work, the prepared workflow `[policy].landing` value is known
- the parent/integration branch is explicit or verified from the current branch
- the branch can merge into the latest integration branch without conflicts
- no unrelated dirty user changes block landing

For workflow work, respect the prepared workflow policy snapshot, not the
current config. Get it from the handoff, `wt workflow show <workflow>`, or the
workflow TOML:

- `landing = "manual"`: stop after review unless the user explicitly directed
  landing for this run.
- `landing = "auto"`: review passing is enough approval to proceed, but still
  enforce dirty-worktree, check, unresolved-review, ancestry, branch-order, and
  cleanup safety gates.

For non-workflow/direct task work, there is no workflow landing policy; require
an explicit user direction, repo policy, or discard intent before landing or
cleanup.

Choose the integration branch without assuming names like `main`, `master`, or
`develop`. Prefer, in order:

- an explicit user instruction, repo policy, or workflow handoff
- the parent branch shown by `wt inspect`
- the current branch at the start of landing, if it is not the task branch and
  it matches the reviewed branch's intended parent

If the current branch and `wt inspect` parent disagree, stop and ask before
landing.

Inside the `wt` repo, compare stale and local binaries when behavior matters:

```bash
wt --version
./target/debug/wt --version
```

Prefer the freshly built repo binary when PATH `wt` is stale.

## Complete When Applicable

Complete only after review passes and the branch has useful committed work.

This step applies to workflow-linked runs after review passes. For stack mode,
use `--run-next` only when the next stack task should start:

```bash
wt workflow pass <workflow> <task> --run-next
```

For single, batch, the final stack task, or a stack task whose successor should
wait, omit `--run-next`:

```bash
wt workflow pass <workflow> <task>
```

For direct TaskRuns, no separate pass command exists — `wt done` during cleanup
also marks running direct TaskRuns passed. See `task-lifecycle.md` for the full
pass vs cleanup boundary.

## Land

First prove whether the branch is already integrated:

```bash
git merge-base --is-ancestor <branch> <integration-branch>
```

If it is not landed, check whether the merge would conflict before doing the
real integration. Use a clean temporary integration worktree when the primary
checkout is dirty, busy, or should not be disturbed:

```bash
git fetch --all --prune
git worktree add <temp-integration-worktree> <integration-branch>
git -C <temp-integration-worktree> pull --ff-only
git -C <temp-integration-worktree> merge --no-commit --no-ff <branch>
git -C <temp-integration-worktree> merge --abort
```

Hard rule: coordinators do not resolve task-branch merge conflicts during
landing.

If a merge conflicts, immediately abort the merge and return the branch to the
task agent. The task agent owns updating its branch against the latest
integration branch, resolving conflicts in its own worktree, committing the
resolution, and rerunning checks.

Return the branch through the canonical inbox route first. For workflow-linked
TaskRuns, prefer review feedback because the conflict blocks landing:

```bash
eval "$(wt session set <coordinator-agent-id>)"
wt task review <task-run-id> --block "Landing blocked: <branch> conflicts with <integration-branch> after <event>. Update the task branch in its worktree, resolve conflicts, rerun checks, push, and report."
```

If the message is not review feedback, use an explicit TaskRun-scoped inbox
message:

```bash
wt msg send \
  --to <task-agent-id> \
  --scope task_run:<task-run-id> \
  "Landing blocked: <branch> conflicts with <integration-branch>. Update the branch, resolve conflicts, rerun checks, push, and report."
```

Then observe the automatic idle wake before using live cmux:

```bash
wt agent status <target>
wt agent watch <target> --interval 5 --timeout 30 --heartbeat 30
```

Do not use `wt send` merely because the target agent is idle. A correctly scoped
inbox message should wake an idle live TaskRun agent. Use `wt send` only if the
scoped inbox route cannot be trusted, hooks do not deliver after the short wake
window, or the TaskRun route is missing, invalid, or ambiguous.

Only resolve conflicts in the coordinator checkout when the user explicitly
instructs the coordinator to take over after the conflict is known. If you
already entered a conflicted merge state, stop, report it, and abort the merge
before sending the task agent the update request.

After the task branch is conflict-free, merge deliberately from a clean
integration checkout:

```bash
git switch <integration-branch>
git pull --ff-only
git merge --ff-only <branch>
```

### Stack-mode landing — base-to-top, with explicit base re-target

For stack-mode work, slices are dependency-ordered. Given a stack such as

```
integration-branch  <-  feature-a  <-  feature-b  <-  feature-c
```

`feature-a` is the slice closest to the integration branch and `feature-c` is
the stack tip. Build order is left-to-right (a, then b, then c). **Land order
is the same left-to-right order: land `feature-a` first, then `feature-b`,
then `feature-c`.** Never merge a child slice before its stack parent has
landed into the integration branch.

Each stack PR was originally opened with its **stack parent** as the PR base
(GitHub `baseRefName`), not the integration branch. That is correct during
review (the diff reflects the slice's own changes against its parent), but it
is **wrong at land time**: a squash-merge with `baseRefName = <stack parent>`
puts the squash commit on the stack-parent branch on origin, **not on the
integration branch**, so the slice content never reaches integration even
though GitHub marks the PR `MERGED`.

The base re-target rule applies before every stack child merge:

1. Land the stack root (`feature-a` → integration-branch) normally. The root
   PR's base is already the integration branch.
2. After the root lands, for each subsequent stack child (`feature-b`,
   `feature-c`, ...): **re-target the PR base to the integration branch**
   before merging.

```bash
# Verify the current base
gh pr view <number> --json baseRefName

# If baseRefName is the stack parent (not the integration branch), re-target.
# `gh pr edit` errors on the legacy Projects classic field, so use the REST API:
gh api repos/<owner>/<repo>/pulls/<number> -X PATCH -f base=<integration-branch>

# Confirm the change took effect
gh pr view <number> --json baseRefName
```

Only after `baseRefName` equals the integration branch should the squash-merge
run. Confirm parent/diff once more before merging:

```bash
git log --oneline <parent>..<branch>
git diff --stat <parent>...<branch>
```

If the primary checkout is dirty or busy, create a temporary integration
worktree instead of disturbing user state.

### Recovery when a stack child was merged into the wrong base

If a stack child was already merged with the wrong `baseRefName` (squash
commit landed on the stack-parent branch, not the integration branch), the
slice's content is on origin but missing from the integration branch.
Reverting the merge is usually noisier than recovering forward.

Recover via cherry-pick onto the integration branch:

```bash
# Identify each mis-targeted PR's squash merge commit (the content carrier)
gh pr view <number> --json mergeCommit,baseRefName

# Create a fresh recovery branch off the integration branch
git fetch --all --prune
git switch -c land-<slug>-recover origin/<integration-branch>

# Cherry-pick each mis-targeted squash commit in build order
git cherry-pick <slice-N-squash-sha>
git cherry-pick <slice-N+1-squash-sha>

# Push and open a single recovery PR against the integration branch
git push -u origin land-<slug>-recover
gh pr create --base <integration-branch> --head land-<slug>-recover \
  --title "Land <slug> slices (stack base re-target recovery)" \
  --body "Cherry-picks the squash merge commits from the mis-targeted stack PRs onto current <integration-branch>. References the original PR numbers."
```

The original PRs stay `MERGED` (just into the wrong base); the recovery PR is
the one that actually lands the content. Document the recovery in the
workflow's spec or `09-execution.md` so future readers understand the history.

The robust path is to prevent mis-targeting up front via the explicit
re-target step above. Treat the recovery procedure as a fallback, not a
regular step.

## Cleanup

Clean only after landing is proven:

```bash
git merge-base --is-ancestor <branch> <integration-branch>
wt done <branch-or-worktree>
git worktree list
git branch --list '<branch-pattern>'
```

For intentionally discarded work, state that it is discard cleanup and confirm
there is no useful unmerged work before `wt done`.

Leave TaskDocument and TaskRun files alone unless a `wt` command owns that
state transition or the user explicitly asks to remove them.

## Report

Report:

- branch landed
- integration branch and merge commit, or already-landed proof
- pass command used, if any
- cleanup command used
- checks run and remaining gaps
- remaining related worktrees or branches
