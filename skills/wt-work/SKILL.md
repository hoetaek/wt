---
name: wt-work
description: "Use for the full wt loop by sequencing wt-ready, wt-start, wt-coordinate, wt-land, and wt-retrospect."
---

# WT Work

Use this skill when the user wants the whole wt operating loop handled for the
given context, not just one lifecycle phase.

Apply these skills in order:

1. `wt-ready`: gather evidence, settle scope and workflow policy, split work,
   and prepare TaskDocuments or a saved workflow when needed.
2. `wt-start`: launch the prepared task or workflow and capture the inspect
   target.
3. `wt-coordinate`: monitor the run, inspect agent state, review code, run
   checks, and send focused feedback until the work is acceptable.
4. `wt-land`: respect workflow landing policy, perform any applicable
   completion step, land branches in the right order, prove ancestry, and clean
   up with `wt done`.
5. `wt-retrospect`: capture keep/problem/try lessons and action candidates as a
   TOML retrospective under `<git-common-dir>/wt/retrospectives/` so future loops can learn
   from this one. Skip only when no useful lesson emerged (a retrospect that
   restates the diff is noise).

Carry the user's original context through all five phases. Stop only when a
phase's own guardrail blocks progress, such as unresolved HITL decisions, active
agent work that still needs time, failed review, merge conflicts owned by the
task agent, or unsafe cleanup conditions. When a phase blocks progress, still
consider running `wt-retrospect` to record why — a blocked loop is also a
lesson.

When moving from one phase to the next, load and follow that phase's skill body
instead of reimplementing its rules from memory. Keep lifecycle boundaries
explicit: preparation is not launch, TaskRun completion is not landing,
cleanup happens only after landing or discard intent is proven, and the
retrospective is written after the work item is closed (landed or explicitly
discarded), not in the middle of in-flight state.

Report the final lifecycle state: evidence checked, launch command and inspect
target, review/check result, completion or merge proof, cleanup command, the
retrospective file path (when written), and any remaining blocker.
