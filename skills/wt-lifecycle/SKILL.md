---
name: wt-lifecycle
description: |
  Use to carry wt work through the full lifecycle: prepare unclear work with
  wt-ready, launch and coordinate execution, then land or discard safely while
  settling reusable lessons into retrospective and harness updates.
---

# WT Lifecycle

Use this skill when the user wants one wt work item carried from preparation
through execution, landing or discard, and harness settlement.

각 단계에서 per-feature 문서(idea / spec / task / workflow / retrospect)
골격이 필요하면 `wt scaffold <slug> --<kind>` 로 시드한다.

## Lifecycle Reference

Before starting the loop, read `references/task-lifecycle.md` for
TaskDocument/TaskRun/workflow object model, status boundaries, pass vs cleanup,
and workflow-linked task boundaries. `wt-lifecycle` does not decide readiness
gates itself; route unclear or unprepared work through `wt-ready`, which owns
that preparation model.

Apply these skills in order:

1. Prepare unclear work with `wt-ready`. When the user is still exploring,
   `wt-ready` captures or updates a kill-able idea and stops. When the user is
   ready to commit, it settles purpose, requirements, examples/wireframes,
   design, task graph, workflow policy, TaskDocuments, and launch target.
2. Execute and coordinate with `wt-work`: launch the prepared task or workflow,
   capture the inspect target, monitor the run, inspect agent state, review
   code, run checks, sync the living spec, and send focused feedback until the
   work is acceptable.
3. Land, discard, and settle the harness with `wt-land` after review passes:
   respect workflow landing policy, perform any applicable pass step, land
   branches in the right order, prove ancestry or discard intent, clean up with
   `wt done`, and record keep/problem/try lessons, action candidates,
   harness-tuning records, expected vs actual duration, and watch-cadence
   evidence. Write the timing entry even when there was no broader lesson.

Carry the user's original context through every lifecycle step. Stop only when
that step's own guardrail blocks progress, such as unresolved HITL decisions,
active agent work that still needs time, failed review, merge conflicts owned
by the task agent, or unsafe cleanup conditions. When progress stops for a
reusable reason, consider writing a retrospective to record why.

When moving from one lifecycle skill to the next, load and follow that skill
body instead of reimplementing its rules from memory. Keep boundaries explicit:
preparation is not launch, TaskRun pass is not landing, cleanup happens only
after landing or discard intent is proven, and the retrospective is written
after the work item is closed (landed or explicitly discarded), not in the
middle of in-flight state.

Report the final lifecycle state: current skill/step, evidence checked, launch
command and inspect target, review/check result, pass or merge proof, cleanup
command, retrospective file path when written, and any remaining blocker.
