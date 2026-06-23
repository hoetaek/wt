# Retrospect Reference

Use this reference from `wt-land` to capture a closed work item, or a blocked
`wt-autopilot` lesson, as a structured retrospective that future coordination,
review, landing, or skill guidance can learn from. Do not use it to track
in-flight state — that belongs in `<repo-root>/.wt/execution/tasks`,
`<repo-root>/.wt/execution/task-runs`, and
`<repo-root>/.wt/execution/workflows`.

`wt-land` owns the retrospect step after landing or explicit discard. It can
also record a blocked-loop lesson when the loop stops earlier and the blocker
itself is worth preserving.

## When to Use

- After `wt-land` proves a branch landed and cleanup ran.
- After an intentional discard with explicit user direction.
- At the end of a `wt-autopilot` loop, even when a step blocked progress, if
  the blocker is a reusable lesson for the next cycle.
- When the user explicitly says "retrospect" or "retrospective 작성".

Write a timing entry for every closed work item, even when no broader
keep/problem/try lesson emerged. If no useful keep/problem/try emerged, keep
those sections short; a retrospect that restates the diff is noise.

## Retrospect Types

Choose the type before writing:

- Closed retrospect: the work item is landed, explicitly discarded, or
  otherwise closed after `wt-land` proved integration/discard and cleanup
  safety. Record outcome, evidence, merge or discard proof, cleanup, and
  lessons.
- Blocked lesson retrospect: the `wt-autopilot` loop stopped before landing —
  missing execution handoff, failed launch, unresolved review evidence,
  landing conflict ownership, or unclear policy. Record the missing step and
  the reusable lesson only. Do not turn the retrospective into TaskRun,
  Workflow, or branch state.

If the work is merely waiting on an active agent, a check, or a normal review
round, it is not a blocked lesson retrospect yet; continue with the matching
lifecycle skill.

## Boundary

- Retrospectives are execution-learning artifacts, not execution state.
- Never write TaskDocument/TaskRun/Workflow status into a retrospect file.
- Future product ideas and open thinking belong outside wt (the `leaf-work`
  workspace). Promote a retrospect action candidate into a leaf seed or a
  TaskDocument only when the pattern is clear enough to act on.
- Prefer one closed work item or blocked lesson per file. If one run produced
  unrelated lessons, split into separate files and cross-link with
  `related_retrospective` where the format supports it.

## Place and Name

- Single home: `<repo-root>/.wt/execution/retrospectives/`.
- Path: `YYYY-MM-DD-<slug>.md` (narrative) or `YYYY-MM-DD-<slug>.toml`
  (structured). Choose TOML when metrics/action-candidate tables matter;
  Markdown when the lesson is mostly prose.
- Slug is the work item's canonical short name (branch, PR title topic, or the
  concept the lesson centers on). Avoid generic slugs like
  `2026-05-19-cleanup`.
- One file per work item. Do not append a new retrospect to an existing file
  unless it is the same work item.
- Write the file directly. The wt CLI does not scaffold retrospectives.
- Legacy note: retrospectives previously lived under
  `<repo-root>/.wt/planning/retrospectives/` and spec-local
  `04-Feedback/10-retrospect.md` files. Treat those as historical reading
  material; new files always go to `execution/retrospectives/`.

## Format

Markdown shape:

```markdown
# <title>

## Outcome
- target:
- result:
- proof:

## Time / watch
- task:
- TaskRun:
- branch / worktree:
- agent / profile:
- expected duration:
- estimate basis:
- started / ended / actual duration:
- first meaningful signal:
- watch strategy:
- observed watch evidence:
- intervention / feedback:
- cadence judgment:
- next estimate adjustment:

## Keep
-

## Problem
-

## Try
-

## Action candidates
-

## Harness tuning
-

## Unknown surfacing misses
-
```

TOML shape (match the conventions in
`<repo-root>/.wt/execution/retrospectives/README.md` when present):

```toml
title = "<concise title that names the work item>"
date = "YYYY-MM-DD"
kind = "<direct-task | workflow-batch | workflow-stack | matrix | multi-pr-cycle | fix-and-hotfix-sequence | discard | blocked-gate>"
target = "<task key, workflow id, PR number(s), or topic>"
outcome = "<landed | discarded | partial | blocked>"
commit = "<merge commit oid(s)>"
skills = ["wt-autopilot", "wt-ready", ...]   # skills actually used in this loop
tags = ["..."]                          # searchable topic tags

# Optional when split into multiple files
related_retrospective = "<repo-root>/.wt/execution/retrospectives/<other-file>.toml"

[context]
goal = """..."""
scope = """..."""
integration_branch = "develop"   # or actual branch
blocked_gate = ""                 # required for outcome = "blocked"

[metrics]
# Numbers a future coordinator can compare against next time.
# Examples: files_changed, insertions, deletions, commits, feedback_rounds,
# prs_landed, manual_unsticks_before_fix, post_merge_review_findings, etc.
# Skip metrics that are not meaningful for this kind of work.

[timing]
expected_duration = ""
estimate_basis = ""
actual_duration = ""
first_meaningful_signal = ""
watch_cadence = ""
cadence_judgment = ""
next_adjustment = ""

[evidence]
key_observations = ["..."]       # concrete facts established during the loop
commands_that_proved_things = ["..."]
experiments = ["<path to prep/review notes the loop actually used, when any>"]
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
owner = "<wt | wt-autopilot | wt-ready | wt-work | wt-land | coordinator | <user>>"
status = "candidate"               # or "addressed" / "promoted"
promote_to = "<a TaskDocument path, or the leaf workspace, when known>"
done_when = "<observable criterion that closes this candidate>"

[[harness_tuning]]
# One entry per lesson that warrants a permanent behavior change.
# Skip this table entirely when no lesson rises to that bar.
lesson = "<the mistake or friction this entry exists to prevent next time>"
owner = "<wt | wt-autopilot | wt-ready | wt-work | wt-land | coordinator | <user>>"
target_file = "<absolute or repo-relative path of the file that must change>"
target_section = "<heading, anchor, or line range inside target_file>"
change = "<what the edit should say or constrain, in one or two sentences>"
rationale = "<why this belongs in target_file rather than a one-off reminder>"
status = "proposed"                # or "applied" / "rejected"
```

## Timing Evidence

Read enough local evidence to avoid inventing timing:

- TaskDocument `계획 (Planning)` for expected duration, estimate basis, size
  class, execution shape, and acceptance checks
- the work item's prep/review notes when they exist (a leaf workspace, or a
  legacy spec folder for older in-flight work)
- TaskRun, workflow row, branch, worktree, and agent ids from `wt inspect`
- inbox reports and message ids used during coordination
- git commit range and first/last commit timestamps when useful
- checks run and final result
- `wt agent wait-stats` and, only when necessary, the raw
  `<repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl`

Runtime wait observations are supporting evidence for wait/watch behavior, not
the source of truth for actual task duration. They record non-idle samples only
when `wt agent watch` emits a heartbeat or timeout sample. If no watch sample
exists, write that explicitly instead of backfilling from memory.

Separate launch validation from stuck detection. A short post-launch poll such
as a 45-second status/watch check proves that the run started; it does not
prove the agent is stuck when the task estimate is much larger. For a 2h
conservative planning guess, use longer steady cadence such as 10-15 minute
heartbeat unless there is concrete stalled evidence.

## Rolling Timing Baseline

After each task timing entry, update the cross-work timing baseline when the
result teaches anything about future estimates or watch cadence:

```bash
<repo-root>/.wt/execution/retrospectives/timing.md
```

This file is a rolling calibration note, not a replacement for per-work
retrospect files. Keep it small enough for `wt-ready` and `wt-work` to read
quickly. Recommended columns:

```text
| date | slug/task | type | size | agent/profile | expected | actual | first signal | watch cadence | result | next adjustment |
```

## Writing Rules

- Keep items observable. "Coordinator should think harder" is not a keep/try
  item; "Run focused tests sequentially because cargo serializes on package
  locks" is.
- Separate keep, problem, and try. A problem is not automatically a try — some
  problems are out of scope or already addressed elsewhere.
- Make `try` items adoptable. If the change belongs in a skill body, say so;
  if it belongs in `wt` code, say so; if it is a coordinator habit, say so.
- For `outcome = "blocked"`, name the missing lifecycle step. Keep active
  execution details in TaskRun/Workflow state; the retrospective should
  explain what should change next time.
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
  permanent rule. When a lesson DOES warrant a permanent change, the
  retrospect must name the exact target.
- Name the exact file AND the section, heading, anchor, or line range inside
  it. "Update CLAUDE.md somewhere" is not enough; "Add a bullet under
  `CLAUDE.md` > `## 문제 해결 원칙` after item 5" is.
- Name the owner. Use the same vocabulary as `[[action_candidates]]`
  (`wt | wt-autopilot | wt-ready | wt-work | wt-land | coordinator | <user>`)
  so the reader knows who is responsible for applying the change.
- Record each such change as one `[[harness_tuning]]` entry in the TOML. If no
  lesson rises to that bar, omit the table entirely rather than padding it.

Target files commonly include, but are not limited to:

- `CLAUDE.md` / `AGENTS.md` / `AGENTS.override.md` at the project root or in
  `~/.claude/`.
- Steering files such as `.kiro/steering/*` and equivalents in other dotfile
  setups.
- Workflow rules and config: `.wt.toml`, `<repo-root>/.wt/config/local.toml`.
- SKILL.md bodies under `<wt-repo>/plugins/wt/skills/wt-*/SKILL.md` (installed views are
  symlinks), especially `wt-land/SKILL.md` when the lesson changes closeout
  behavior.
- Profile prompts under `<repo-root>/.wt/config/profiles/<name>/prompts/`.

## Process

1. Classify the retrospect:
   - Closed: confirm the work item is landed or explicitly discarded. If the
     branch is still active and can continue normally, stop and let the
     matching lifecycle skill finish first.
   - Blocked lesson: confirm the `wt-autopilot` loop is stopped at a named
     step and the blocker is a reusable lesson. Do not write one for ordinary
     waiting, active agent work, or a routine review round.
2. Decide scope: one file or split + cross-link.
3. For `outcome = "blocked"`, set `kind = "blocked-gate"` unless a more
   specific kind is still useful, and fill `context.blocked_gate` with the
   blocked step name.
4. Diagnose unknown-surfacing misses when prep/review notes recorded
   mid-process discoveries: classify each against `domain`, `standards`,
   `external`, `internal`. The recurring category is what the next run's
   preparation should explicitly cover — record it as a `try` item or a
   `[[harness_tuning]]` entry pointing at the relevant SKILL.md section.
5. Draft the file under `<repo-root>/.wt/execution/retrospectives/`. Skip
   optional sections that have no content.
6. Cross-check `<repo-root>/.wt/execution/retrospectives/README.md` when TOML
   conventions are uncertain.
7. Re-read for adoptability: each `try` item should be something a future
   coordinator can actually do; each `action_candidate` should have a
   recognizable `done_when`.
8. For every lesson that warrants a permanent behavior change, add a
   `[[harness_tuning]]` entry that names the exact target file and section,
   plus the owner who applies the change. If no lesson rises to that bar,
   leave the table out.
9. Do not commit the file unless the user asks. Retrospectives are local
   learning artifacts by default.

## Report

After writing, report:

- The created file path(s).
- A short list of the most adoptable `try` items.
- The highest-leverage `action_candidate` if any, and whether it should be
  promoted to a TaskDocument or to the leaf workspace next.
- Any `[[harness_tuning]]` entries, each with the target file and section, so
  the user can decide whether to apply them now.
