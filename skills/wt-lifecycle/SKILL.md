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

TaskDocument나 workflow 골격이 필요하면 `wt scaffold <slug> --task` /
`--workflow`로 시드한다. 탐색·사고 산출물은 wt 밖의 일이다 — `leaf-work`와
`.leaf/` workspace를 사용한다.

## Lifecycle Reference

Before starting the loop, read `references/task-lifecycle.md` for
TaskDocument/TaskRun/workflow object model, status boundaries, pass vs cleanup,
and workflow-linked task boundaries. `wt-lifecycle` does not decide readiness
gates itself; route unclear or unprepared work through `wt-ready`, which owns
that preparation model.

Apply these skills in order:

1. Prepare work with `wt-ready`. It verifies prepared context is executable
   as wt development work; when intent is still vague or design is open, it
   routes the thinking to `leaf-work` and stops. When the work is executable,
   it settles slices, execution shape, workflow policy, TaskDocuments, and the
   launch target.
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
