---
name: wt-variants
description: "Use when exploring multiple implementation variants for one wt task by preparing profile-specific matrix workflow runs with distinct workflow prompts, then comparing the resulting PRs before choosing what to land."
---

# WT Variants

Use this skill when one `wt` problem is clear enough to implement, but the best
approach is uncertain and worth comparing through multiple agent runs.

## Model

`variant` is skill shorthand only. Do not add a persisted `variant` concept to
`wt` state.

Object model and TaskRun semantics: see
`../wt-autopilot/references/task-lifecycle.md`. Variants are represented as:

- one shared local TaskDocument
- one `mode = "matrix"` workflow
- one named profile per variant
- one profile-specific TaskRun/worktree/branch per variant

The workflow TOML stores `profiles = [...]` and `[[tasks.runs]]` mappings. The
variant instructions live in `<repo-root>/.wt/config/profiles/<name>/profile.toml` or
`prompts/workflow.append.md`, not in the workflow file.

## Boundaries

| Phase | Where |
|---|---|
| problem still vague | `wt-ready` idea capture |
| prepare variants + matrix workflow | this skill |
| launch, monitor, PR review, bot feedback, complete | `wt-work` |
| land selected branch | `wt-land` |

Do not use variants for dependent slices. If tasks depend on each other, use a
stack. If tasks are independent but not competing answers to the same question,
use batch or separate workflows.

## First Read

Inspect local truth before writing variant artifacts:

```bash
git status --short --branch
find . -maxdepth 2 -name AGENTS.md -o -name AGENTS.override.md
sed -n '90,115p' docs/consistency.md 2>/dev/null || true
repo_root="$(git rev-parse --show-toplevel)"
find "$repo_root/.wt/execution/tasks" "$repo_root/.wt/execution/workflows" -maxdepth 1 -type f 2>/dev/null | sort
wt config show 2>/dev/null || ./target/debug/wt config show 2>/dev/null || true
```

## Preparation

Capture the experiment shape before creating files:

- common problem and acceptance checks
- 2-4 variant hypotheses
- evaluation criteria: correctness, diff size, test coverage, risk, review
  feedback, and fit with `docs/consistency.md`
- non-goals and risks

Prefer focused variants that test genuinely different approaches. Do not create
three profiles that only reword the same instruction.

## TaskDocument

Create one shared task in `<repo-root>/.wt/execution/tasks/<key>.toml`.

The task body should include:

- the common goal
- known evidence
- files or modules to inspect
- acceptance checks
- an instruction that each profile should follow its own hypothesis
- the standard Agent Completion Report format

Do not put profile-specific instructions in the TaskDocument except to say that
each run must follow its selected profile.

## Profiles

Create one profile per variant under `<repo-root>/.wt/config/profiles/<profile>/profile.toml`.

Each matrix profile must include an explicit `[agent]` section copied from the
current effective config or chosen deliberately for the run. Do not rely on a
prompt-only profile overlay; it can accidentally resolve to `agent.cli = "none"`.

Use `[agent.prompt.append].workflow` for the variant-specific hypothesis:

```toml
[agent]
cli = "codex"
args = ["--yolo"]
timeout = 30
send_after = 2

[agent.prompt.append]
workflow = ["""Matrix hypothesis: prefer the smallest sufficient change.

Focus on ...
Report whether this hypothesis is sufficient, weaker than another approach, or
too broad for the observed bug.
"""]
```

Keep profile names short and hypothesis-shaped, for example:

- `<task>-minimal`
- `<task>-gate`
- `<task>-parser`
- `<task>-model`

Avoid `default` as a profile name.

## Validate Profiles

Before preparing the workflow, verify every profile resolves to an agent and the
intended workflow prompt:

```bash
for profile in <profiles>; do
  ./target/debug/wt config show --profile "$profile"
done
```

Check that each output includes:

- `[agent]`
- `cli = "..."`
- the profile-specific `workflow = [...]` prompt

## Prepare Workflow

Use a matrix workflow with exactly one task and the selected profile list:

```bash
./target/debug/wt workflow task \
  --mode matrix \
  --profiles <profile-a>,<profile-b>,<profile-c> \
  --base <base> \
  --pr draft \
  --title "<comparison objective>" \
  <task-key>
```

Then verify:

```bash
./target/debug/wt workflow show <workflow-id>
sed -n '1,180p' "$(git rev-parse --show-toplevel)/.wt/execution/workflows/<workflow-id>.toml"
```

The workflow should store the profile names and one `[[tasks.runs]]` entry per
profile. It should not store variant prose or cmux runtime coordinates.

## Launch And Coordinate

Launch with `wt-work` or the explicit command:

```bash
./target/debug/wt run workflow <workflow-id> --jobs <n>
```

If `--jobs` does not fan out matrix profiles concurrently, record that as a
workflow runtime issue; do not reinterpret `matrix` as sequential by definition.

During coordination:

- inspect each worktree and PR directly
- verify checks and review threads
- wait for bot reviews to finish before resolving comments or landing
- compare the actual diffs, not only completion reports
- prefer the smallest sufficient implementation
- freely borrow tests or evidence from broader variants when useful
- reject variants that broaden the model more than the problem requires

## Decision Record

When all useful variants have reported, summarize:

- chosen branch or PR
- rejected branches and why
- useful tests or ideas to cherry-pick
- checks and review state
- unresolved risks
- exact `wt-work` / `wt-land` next step

Do not mark workflow runs complete or land branches until the selected work has
been reviewed against the evidence above.
