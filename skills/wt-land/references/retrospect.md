# Retrospect Reference

Use this reference from `wt-land` to capture a closed work item, or a blocked
`wt-lifecycle` lesson, as
a structured retrospective that future planning, coordination, review, landing,
or skill guidance can learn from. Do not use it to track in-flight state — that
belongs in `<repo-root>/.wt/execution/tasks`,
`<repo-root>/.wt/execution/task-runs`, and
`<repo-root>/.wt/execution/workflows`.
In the LEAF model, `wt-land` owns the final retrospect gate after landing or
explicit discard. It can also record a blocked-loop lesson when the loop stops
at an earlier gate and the blocker itself is worth preserving.

## When to Use

- After `wt-land` proves a branch landed and cleanup ran.
- After an intentional discard with explicit user direction.
- At the end of a `wt-lifecycle` loop, even when a phase blocked progress, if the
  blocker is a reusable lesson for the next cycle.
- When the user explicitly says "retrospect" or "retrospective 작성".
- When the user references `planning/specs/<slug>/11-retrospect.md` or
  cross-work `<repo-root>/.wt/planning/retrospectives/`.

Write a timing entry for every closed work item, even when no broader
keep/problem/try lesson emerged. If no useful keep/problem/try emerged, keep
those sections short; a retrospect that restates the diff is noise.

## Retrospect Types

Choose the type before writing:

- Closed retrospect: the work item is landed, explicitly discarded, or otherwise
  closed after `wt-land` proved integration/discard and cleanup safety. Record
  outcome, evidence, merge or discard proof, cleanup, and lessons.
- Blocked lesson retrospect: the `wt-lifecycle` loop stopped at an earlier gate, such
  as missing execution handoff, failed launch, unresolved review evidence,
  landing conflict ownership, or unclear policy. Record the missing gate and the
  reusable lesson only. Do not turn the retrospective into TaskRun, Workflow, or
  branch state.

If the work is merely waiting on an active agent, a check, or a normal review
round, it is not a blocked lesson retrospect yet; continue with the matching
lifecycle skill.

## Boundary

- Retrospectives are learning artifacts, not execution state.
- Never write TaskDocument/TaskRun/Workflow status into a retrospect file.
- Keep future product ideas in `<repo-root>/.wt/planning/ideas/`; promote a
  retrospect action candidate into an idea or task only when the pattern is
  clear enough to act on.
- Prefer one closed work item or blocked gate lesson per file. If one run
  produced unrelated lessons, keep the spec-backed work item lesson in
  `11-retrospect.md` and promote cross-work lessons to
  `<repo-root>/.wt/planning/retrospectives/` only when they are not owned by
  a single spec.

## Scope Choice

Before writing, decide what "one work item" means for this cycle. A multi-PR
sequence can be one item when the PRs share a single goal and the keep/problem
lessons converge. Use the spec-local file when the lesson belongs to one
`planning/specs/<slug>/`; split to the global retrospectives directory only when:

- Different goals produced disjoint lessons (e.g. a profile cockpit decision
  vs. a marker narrowing fix).
- One lesson is about coordination and another is about substring traps in
  code — readers benefit from being able to find them separately.

When you split, cross-link the files in prose or with `related_retrospective`
where the format supports it.

## Place and Name

- Default path for spec-backed work:
  `<repo-root>/.wt/planning/specs/<slug>/11-retrospect.md`.
- Cross-work/spec-less fallback path:
  `<repo-root>/.wt/planning/retrospectives/YYYY-MM-DD-<slug>.toml`.
- Slug is the work item's canonical short name (branch, PR title topic, or the
  concept the lesson centers on). Avoid generic slugs like
  `2026-05-19-cleanup`.
- One file per work item. Do not append a new retrospect to an existing file
  unless it is the same work item.

## Format

For spec-backed work, use Markdown in `11-retrospect.md` with these sections:

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

For cross-work/spec-less retrospectives, use TOML under
`<repo-root>/.wt/planning/retrospectives/`. Match the conventions in
`<repo-root>/.wt/planning/retrospectives/README.md`. Required shape:

```toml
title = "<concise title that names the work item>"
date = "YYYY-MM-DD"
kind = "<direct-task | workflow-batch | workflow-stack | matrix | multi-pr-cycle | fix-and-hotfix-sequence | discard | blocked-gate>"
target = "<task key, workflow id, PR number(s), or topic>"
outcome = "<landed | discarded | partial | blocked>"
commit = "<merge commit oid(s)>"
skills = ["wt-lifecycle", "wt-ready", ...]   # skills actually used in this loop
tags = ["..."]                          # searchable topic tags

# Optional when split into multiple files
related_retrospective = "<repo-root>/.wt/planning/retrospectives/<other-file>.toml"

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
experiments = ["<repo-root>/.wt/planning/specs/<slug>/10-review.md"]
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
owner = "<wt | wt-lifecycle | wt-ready | wt-work | wt-land | coordinator | <user>>"
status = "candidate"               # or "addressed" / "promoted"
promote_to = "<repo-root>/.wt/planning/ideas/"      # or a specific path when known
done_when = "<observable criterion that closes this candidate>"

[[harness_tuning]]
# One entry per lesson that warrants a permanent behavior change.
# Skip this table entirely when no lesson rises to that bar.
lesson = "<the mistake or friction this entry exists to prevent next time>"
owner = "<wt | wt-lifecycle | wt-ready | wt-work | wt-land | coordinator | <user>>"
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
- `09-execution.md` for launch shape and risks
- `10-review.md` for review findings, mid-process discoveries, and coordinator
  observations
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
as a 45-second status/watch check proves that the run started; it does not prove
the agent is stuck when the task estimate is much larger. For a 2h conservative
planning guess, use longer steady cadence such as 10-15 minute heartbeat unless
there is concrete stalled evidence.

## Rolling Timing Baseline

After each task timing entry, update the cross-work timing baseline when the
result teaches anything about future estimates or watch cadence:

```bash
<repo-root>/.wt/planning/retrospectives/timing.md
```

This file is a rolling calibration note, not a replacement for per-work
`11-retrospect.md`. Keep it small enough for `wt-ready` and `wt-work` to
read quickly. Recommended columns:

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
- For `outcome = "blocked"`, name the missing gate using the LEAF
  vocabulary. Keep active execution details in TaskRun/Workflow state; the
  retrospective should explain what should change next time.
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
  (`wt | wt-lifecycle | wt-ready | wt-work | wt-land | coordinator | <user>`)
  so the reader knows who is responsible for applying the change.
- Record each such change as one `[[harness_tuning]]` entry in the TOML. If no
  lesson rises to that bar, omit the table entirely rather than padding it.

Target files commonly include, but are not limited to:

- `CLAUDE.md` / `AGENTS.md` / `AGENTS.override.md` at the project root or in
  `~/.claude/`.
- Steering files such as `.kiro/steering/*` and equivalents in other dotfile
  setups.
- Workflow rules and config: `.wt.toml`, `<repo-root>/.wt/config/local.toml`.
- SKILL.md bodies under `~/.agents/skills/wt-*/SKILL.md`, especially
  `wt-land/SKILL.md` when the lesson changes closeout behavior.
- Profile prompts under `<repo-root>/.wt/config/profiles/<name>/prompts/`.

When the finished work item had specs,
`<repo-root>/.wt/planning/specs/<slug>/` may contain numbered LEAF
files from `wt-ready`. Cite them in `evidence` or in the `rationale` of a
`[[harness_tuning]]` entry when the lesson points at the spec template itself
(e.g. "the EARS statement in 04+05+06-requirements.md proved ambiguous; tighten
the wt-ready template").

## Process

1. Classify the retrospect:
   - Closed: confirm the work item is landed or explicitly discarded. If the
     branch is still active and can continue normally, stop and let the matching
     lifecycle skill finish first.
   - Blocked lesson: confirm the `wt-lifecycle` loop is stopped at a named
     LEAF gate and the blocker is a reusable lesson. Do not write one
     for ordinary waiting, active agent work, or a routine review round.
2. Decide scope: one file or split + cross-link.
3. For `outcome = "blocked"`, set `kind = "blocked-gate"` unless a more
   specific kind is still useful, and fill `context.blocked_gate` with the
   LEAF gate name.
4. Diagnose Unknown surfacing misses, if the spec has them:
   - If `<repo-root>/.wt/planning/specs/<slug>/10-review.md` has a
     `## Mid-process discoveries` section, read it. Each entry is a research
     step that happened mid-work instead of at the Unknown surfacing gate.
   - Classify each discovery against the four surfacing categories: `domain`,
     `standards`, `external`, `internal`.
   - The category(ies) that recur are the ones the next run's surfacing pass
     should explicitly cover. Record this either as a `try` item ("add X
     category to the surfacing checklist for this kind of work") or, when
     the lesson belongs in a skill body, as a `[[harness_tuning]]` entry
     pointing at the relevant SKILL.md section.
5. Draft `11-retrospect.md` under the spec for spec-backed work. Draft TOML
   directly under
   `<repo-root>/.wt/planning/retrospectives/YYYY-MM-DD-<slug>.toml` only for
   cross-work/spec-less retrospectives. Skip optional sections that have no
   content.
6. Cross-check against `<repo-root>/.wt/planning/retrospectives/README.md`
   only for global TOML retrospectives when conventions are uncertain.
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
  promoted to `<repo-root>/.wt/planning/ideas/` or a TaskDocument next.
- Any `[[harness_tuning]]` entries, each with the target file and section, so
  the user can decide whether to apply them now.
