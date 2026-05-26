---
name: wt-work
description: "Use for the full wt loop by sequencing wt-idea when needed, wt-ready, wt-start, wt-coordinate, wt-land, and wt-retrospect."
---

# WT Work

Use this skill when the user wants the whole wt operating loop handled for the
given context, not just one lifecycle phase.

각 단계에서 per-feature 문서(idea / spec / task / workflow / retrospect) 골격이 필요하면 `wt scaffold <slug> --<kind>` 로 시드한다.

## Work Sequence Reference

Before starting the loop, read `references/work-sequence.md` and locate the
current gate. For lifecycle invariants (TaskDocument/TaskRun/workflow object
model, status boundaries, completion vs cleanup), see
`references/task-lifecycle.md`. The sequence is not a waterfall; it is a guardrail for deciding
which lifecycle skill owns the next artifact. If raw intent or context is still
too vague, run `wt-idea` before `wt-ready`. If the work is already committed to
prep, use `wt-ready` to reconstruct any missing earlier gates instead of
launching prematurely.

Apply these skills in order:

1. `wt-idea` when needed: preserve raw intent, explore references/options, and
   stop before committed TaskDocuments or workflows.
2. `wt-ready`: gather evidence, settle purpose/success criteria, requirements,
   output concept, scope, and workflow policy; split work,
   and prepare TaskDocuments or a saved workflow when needed.
3. `wt-start`: launch the prepared task or workflow and capture the inspect
   target.
4. `wt-coordinate`: monitor the run, inspect agent state, review code, run
   checks, sync the living spec, and send focused feedback until the work is
   acceptable.
5. `wt-land`: respect workflow landing policy, perform any applicable
   completion step, land branches in the right order, prove ancestry, and clean
   up with `wt done`.
6. `wt-retrospect`: capture keep/problem/try lessons, action candidates, and
   harness-tuning records as a TOML retrospective when a useful lesson emerged.

Carry the user's original context through every phase. Stop only when a
phase's own guardrail blocks progress, such as unresolved HITL decisions, active
agent work that still needs time, failed review, merge conflicts owned by the
task agent, or unsafe cleanup conditions. When a phase blocks progress, still
consider writing a retrospective to record why — a blocked loop is also a
lesson.

When moving from one skill phase to the next, load and follow that phase's skill
body instead of reimplementing its rules from memory. Keep lifecycle boundaries
explicit: preparation is not launch, TaskRun completion is not landing, cleanup
happens only after landing or discard intent is proven, and the retrospective is
written after the work item is closed (landed or explicitly discarded), not in
the middle of in-flight state.

Report the final lifecycle state: evidence checked, launch command and inspect
target, review/check result, completion or merge proof, cleanup command, the
retrospective file path (when written), and any remaining blocker.
