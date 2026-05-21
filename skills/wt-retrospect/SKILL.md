---
name: wt-retrospect
description: "Use after wt work has landed (or been intentionally discarded) to capture keep/problem/try lessons and action candidates as a TOML retrospective under <git-common-dir>/wt/retrospectives. Triggers: 'retrospect', 'retrospective 작성', or end of a wt-work loop."
---

# WT Retrospect

Use this skill to capture a completed work item as a structured retrospective
that future planning, coordination, review, landing, or skill guidance can
learn from. Do not use it to track in-flight state — that belongs in
`<git-common-dir>/wt/tasks`, `<git-common-dir>/wt/task-runs`, and `<git-common-dir>/wt/workflows`.

## When to Use

- After `wt-land` proves a branch landed and cleanup ran.
- After an intentional discard with explicit user direction.
- At the end of a `wt-work` loop, even when a phase blocked progress, if the
  lesson is worth preserving for the next cycle.
- When the user explicitly says "retrospect", "retrospective 작성", or
  references `<git-common-dir>/wt/retrospectives/`.

Skip this skill when no useful keep/problem/try emerged. A retrospect that
restates the diff is noise.

## Boundary

- Retrospectives are learning artifacts, not execution state.
- Never write TaskDocument/TaskRun/Workflow status into a retrospect file.
- Keep future product ideas in `<git-common-dir>/wt/ideas/`; promote a retrospect action
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

- Path: `<git-common-dir>/wt/retrospectives/YYYY-MM-DD-<slug>.toml`.
- Slug is the work item's canonical short name (branch, PR title topic, or the
  concept the lesson centers on). Avoid generic slugs like
  `2026-05-19-cleanup`.
- One file per work item. Do not append a new retrospect to an existing file
  unless it is the same work item.

## Format

Use TOML. Match the conventions in `<git-common-dir>/wt/retrospectives/README.md`. Required
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
related_retrospective = "<git-common-dir>/wt/retrospectives/<other-file>.toml"

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
experiments = ["<git-common-dir>/wt/experiments/<name>.md"]
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
promote_to = "<git-common-dir>/wt/ideas/"      # or a specific path when known
done_when = "<observable criterion that closes this candidate>"

[[harness_tuning]]
# One entry per lesson that warrants a permanent behavior change.
# Skip this table entirely when no lesson rises to that bar.
lesson = "<the mistake or friction this entry exists to prevent next time>"
owner = "<wt | wt-coordinate | wt-land | wt-ready | wt-start | coordinator | <user>>"
target_file = "<absolute or repo-relative path of the file that must change>"
target_section = "<heading, anchor, or line range inside target_file>"
change = "<what the edit should say or constrain, in one or two sentences>"
rationale = "<why this belongs in target_file rather than a one-off reminder>"
status = "proposed"                # or "applied" / "rejected"
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

## Harness Tuning

Beyond keep/problem/try and action candidates, a retrospect must produce an
explicit harness-tuning record whenever a lesson calls for a permanent change
in how the agent operates. The principle (Mitchell Hashimoto's harness
engineering loop): every time the agent makes a mistake, engineer a solution
such that the agent never makes that mistake again.

- Distinguish lesson from harness change. Not every lesson needs one — some
  lessons are situational ("the user preferred X this time") and need no
  permanent rule. When a lesson DOES warrant a permanent change, the retrospect
  must name the exact target.
- Name the exact file AND the section, heading, anchor, or line range inside
  it. "Update CLAUDE.md somewhere" is not enough; "Add a bullet under
  `CLAUDE.md` > `## 문제 해결 원칙` after item 5" is.
- Name the owner. Use the same vocabulary as `[[action_candidates]]`
  (`wt | wt-coordinate | wt-land | wt-ready | wt-start | coordinator | <user>`)
  so the reader knows who is responsible for applying the change.
- Record each such change as one `[[harness_tuning]]` entry in the TOML. If no
  lesson rises to that bar, omit the table entirely rather than padding it.

Target files commonly include, but are not limited to:

- `CLAUDE.md` / `AGENTS.md` / `AGENTS.override.md` at the project root or in
  `~/.claude/`.
- Steering files such as `.kiro/steering/*` and equivalents in other dotfile
  setups.
- Workflow rules and config: `.wt.toml`, `<git-common-dir>/wt/config.toml`.
- SKILL.md bodies under `~/.agents/skills/wt-*/SKILL.md` (including this one).
- Profile prompts under `<git-common-dir>/wt/profiles/<name>/prompts/`.

When the finished work item had specs, `<git-common-dir>/wt/specs/<slug>/` may
contain `requirements.md`, `design.md`, and `tasks.md` from `wt-ready`. Cite
them in `evidence` or in the `rationale` of a `[[harness_tuning]]` entry when
the lesson points at the spec template itself (e.g. "the EARS statement in
requirements.md proved ambiguous; tighten the wt-ready template").

## Process

1. Confirm the work item is closed (landed or explicitly discarded). If still
   in flight, stop and let the matching lifecycle skill finish first.
2. Decide scope: one file or split + cross-link.
3. Draft the TOML directly under `<git-common-dir>/wt/retrospectives/YYYY-MM-DD-<slug>.toml`
   using the shape above. Skip optional sections that have no content.
4. Cross-check against `<git-common-dir>/wt/retrospectives/README.md` if conventions are
   uncertain.
5. Re-read for adoptability: each `try` item should be something a future
   coordinator can actually do; each `action_candidate` should have a
   recognizable `done_when`.
6. For every lesson that warrants a permanent behavior change, add a
   `[[harness_tuning]]` entry that names the exact target file and section,
   plus the owner who applies the change. If no lesson rises to that bar,
   leave the table out.
7. Do not commit the file unless the user asks. Retrospectives are local
   learning artifacts by default.

## Report

After writing, report:

- The created file path(s).
- A short list of the most adoptable `try` items.
- The highest-leverage `action_candidate` if any, and whether it should be
  promoted to `<git-common-dir>/wt/ideas/` or a TaskDocument next.
- Any `[[harness_tuning]]` entries, each with the target file and section, so
  the user can decide whether to apply them now.
