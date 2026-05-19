---
name: wt-retrospect
description: "Use after wt work has landed (or been intentionally discarded) to capture keep/problem/try lessons and action candidates as a TOML retrospective under .local/retrospectives. Triggers: 'retrospect', 'retrospective 작성', or end of a wt-work loop."
---

# WT Retrospect

Use this skill to capture a completed work item as a structured retrospective
that future planning, coordination, review, landing, or skill guidance can
learn from. Do not use it to track in-flight state — that belongs in
`.local/tasks`, `.local/task-runs`, and `.local/workflows`.

## When to Use

- After `wt-land` proves a branch landed and cleanup ran.
- After an intentional discard with explicit user direction.
- At the end of a `wt-work` loop, even when a phase blocked progress, if the
  lesson is worth preserving for the next cycle.
- When the user explicitly says "retrospect", "retrospective 작성", or
  references `.local/retrospectives/`.

Skip this skill when no useful keep/problem/try emerged. A retrospect that
restates the diff is noise.

## Boundary

- Retrospectives are learning artifacts, not execution state.
- Never write TaskDocument/TaskRun/Workflow status into a retrospect file.
- Keep future product ideas in `.local/ideas/`; promote a retrospect action
  candidate into an idea or task only when the pattern is clear enough to act
  on.
- Prefer one completed work item per file. If one run produced unrelated
  lessons, split them into separate files and cross-link with the
  `related_retrospective` field.

## Scope Choice

Before writing, decide what "one work item" means for this cycle. A multi-PR
sequence can be one item when the PRs share a single goal and the keep/problem
lessons converge. Split when:

- Different goals produced disjoint lessons (e.g. a profile cockpit decision
  vs. a marker narrowing fix).
- One lesson is about coordination and another is about substring traps in
  code — readers benefit from being able to find them separately.

When you split, set `related_retrospective` on each file to the other path.

## Place and Name

- Path: `.local/retrospectives/YYYY-MM-DD-<slug>.toml`.
- Slug is the work item's canonical short name (branch, PR title topic, or the
  concept the lesson centers on). Avoid generic slugs like
  `2026-05-19-cleanup`.
- One file per work item. Do not append a new retrospect to an existing file
  unless it is the same work item.

## Format

Use TOML. Match the conventions in `.local/retrospectives/README.md`. Required
shape:

```toml
title = "<concise title that names the work item>"
date = "YYYY-MM-DD"
kind = "<direct-task | workflow-batch | workflow-stack | matrix | multi-pr-cycle | fix-and-hotfix-sequence | discard>"
target = "<task key, workflow id, PR number(s), or topic>"
outcome = "<landed | discarded | partial | blocked>"
commit = "<merge commit oid(s)>"
skills = ["wt-work", "wt-ready", ...]   # skills actually used in this loop
tags = ["..."]                          # searchable topic tags

# Optional when split into multiple files
related_retrospective = ".local/retrospectives/<other-file>.toml"

[context]
goal = """..."""
scope = """..."""
integration_branch = "develop"   # or actual branch

[metrics]
# Numbers a future coordinator can compare against next time.
# Examples: files_changed, insertions, deletions, commits, feedback_rounds,
# prs_landed, manual_unsticks_before_fix, post_merge_review_findings, etc.
# Skip metrics that are not meaningful for this kind of work.

[evidence]
key_observations = ["..."]       # concrete facts established during the loop
commands_that_proved_things = ["..."]
experiments = [".local/experiments/<name>.md"]
prs = ["#<n>", ...]              # optional

[keep]
items = [
  "<what worked and is worth doing the same way next time>",
]

[problem]
items = [
  "<what caused friction, surprise, or false-positive — say what, not how to fix it>",
]

[try]
items = [
  "<concrete behavior change to try next time, expressed so the next coordinator can adopt it>",
]

[[action_candidates]]
summary = "<one-line action this retrospective recommends>"
owner = "<wt | wt-coordinate | wt-land | wt-ready | wt-start | coordinator | <user>>"
status = "candidate"               # or "addressed" / "promoted"
promote_to = ".local/ideas/"      # or a specific path when known
done_when = "<observable criterion that closes this candidate>"
```

## Writing Rules

- Keep items observable. "Coordinator should think harder" is not a keep/try
  item; "Run focused tests sequentially because cargo serializes on package
  locks" is.
- Separate keep, problem, and try. A problem is not automatically a try — some
  problems are out of scope or already addressed elsewhere.
- Make `try` items adoptable. If the change belongs in a skill body, say so;
  if it belongs in `wt` code, say so; if it is a coordinator habit, say so.
- Use `action_candidates` for items that should turn into work later. Each
  candidate gets a `done_when` so the future coordinator can recognize when
  the candidate is satisfied.
- When a candidate is already handled (e.g. by a PR landed in the same loop),
  use `status = "addressed"` and link to the addressing artifact in
  `promote_to`.

## Process

1. Confirm the work item is closed (landed or explicitly discarded). If still
   in flight, stop and let the matching lifecycle skill finish first.
2. Decide scope: one file or split + cross-link.
3. Draft the TOML directly under `.local/retrospectives/YYYY-MM-DD-<slug>.toml`
   using the shape above. Skip optional sections that have no content.
4. Cross-check against `.local/retrospectives/README.md` if conventions are
   uncertain.
5. Re-read for adoptability: each `try` item should be something a future
   coordinator can actually do; each `action_candidate` should have a
   recognizable `done_when`.
6. Do not commit the file unless the user asks. Retrospectives are local
   learning artifacts by default.

## Report

After writing, report:

- The created file path(s).
- A short list of the most adoptable `try` items.
- The highest-leverage `action_candidate` if any, and whether it should be
  promoted to `.local/ideas/` or a TaskDocument next.
