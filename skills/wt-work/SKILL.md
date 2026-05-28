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
current phase and gate. The four phases are Discover, Shape, Execute, and
Improve. For lifecycle invariants (TaskDocument/TaskRun/workflow object model,
status boundaries, pass vs cleanup), see `references/task-lifecycle.md`. The
sequence is not a waterfall; it is a guardrail for deciding which lifecycle
skill owns the next artifact. If raw intent or context is still too vague, run
`wt-idea` before `wt-ready`. If the work is already committed to prep, use
`wt-ready` to reconstruct any missing earlier gates instead of launching
prematurely.

Apply these skills in order:

1. Discover with `wt-idea` when needed: preserve raw intent, surface unknowns,
   explore references/options, and stop before committed TaskDocuments or
   workflows.
2. Shape and Execute-prep with `wt-ready`: gather evidence, settle
   purpose/success criteria, requirements, output form, wireframe with realistic
   examples, design, scope, task graph, and workflow policy; prepare
   TaskDocuments or a saved workflow when needed.
3. Execute launch with `wt-start`: launch the prepared task or workflow and
   capture the inspect target.
4. Improve with `wt-coordinate`: monitor the run, inspect agent state, review
   code, run checks, sync the living spec, and send focused feedback until the
   work is acceptable.
5. Improve with `wt-land`: respect workflow landing policy, perform any
   applicable pass step, land branches in the right order, prove ancestry, and
   clean up with `wt done`.
6. Improve with `wt-retrospect`: after each closed work item, capture
   keep/problem/try lessons, action candidates, harness-tuning records,
   expected vs actual duration, and watch-cadence evidence. Write the timing
   entry even when there was no broader lesson.

Carry the user's original context through every phase. Stop only when a
phase's own guardrail blocks progress, such as unresolved HITL decisions, active
agent work that still needs time, failed review, merge conflicts owned by the
task agent, or unsafe cleanup conditions. When a phase blocks progress, still
consider writing a retrospective to record why — a blocked loop is also a
lesson.

When moving from one skill phase to the next, load and follow that phase's skill
body instead of reimplementing its rules from memory. Keep lifecycle boundaries
explicit: preparation is not launch, TaskRun pass is not landing, cleanup
happens only after landing or discard intent is proven, and the retrospective is
written after the work item is closed (landed or explicitly discarded), not in
the middle of in-flight state.

Report the final lifecycle state: current phase/gate, evidence checked, launch
command and inspect target, review/check result, pass or merge proof, cleanup
command, the retrospective file path (when written), and any remaining blocker.
