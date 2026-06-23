---
name: autopilot
description: |
  Use after ready has handed off an approved launch target (launch command,
  inspect target, workflow policy, TaskDocument bodies), when the user wants the
  rest of the wt lifecycle — launch, coordinate, review, land, retrospect — to
  proceed automatically with automatic reviews, hard stops, and evidence. Trigger
  on "$autopilot", "wt autopilot", "launch target approved, run the rest
  automatically", or "after ready, carry it the rest of the way". Do not use
  before the ready handoff, for unclear or unprepared work (route to ready),
  or for destructive / external / credential / cost / security / privacy-sensitive
  actions without explicit pre-authorization.
---

# WT Autopilot

`autopilot` carries one wt work item after the human-reviewed `ready`
handoff. It does not remove wt's judgment boundary; it moves the boundary to the
ready handoff, then runs the remaining lifecycle — execution, review, landing,
and harness settlement — with automatic reviews, hard stops, and evidence.

TaskDocument나 workflow 골격이 필요하면 `wt scaffold <slug> --task` /
`--workflow`로 시드한다. 탐색·사고 산출물은 wt 밖의 일이다 — `leaf-work`와
`.leaf/` workspace를 사용한다.

## Lifecycle Reference

Before starting the loop, read `references/task-lifecycle.md` for
TaskDocument/TaskRun/workflow object model, status boundaries, pass vs cleanup,
and workflow-linked task boundaries. `autopilot` does not decide readiness
gates itself; route unclear or unprepared work through `ready`, which owns
that preparation model.

## Core Contract

- **Handoff first.** Do not start unless `ready` has produced an approved
  launch target: the exact launch command and inspect target, the workflow
  landing policy, and the TaskDocument bodies. If that handoff is missing,
  provisional, stale, or the intent is still vague or the design open, return to
  `ready` (which routes thinking to `leaf-work` when needed).
- **Autopilot after the handoff.** Once the launch target is approved, proceed
  automatically: launch and coordinate with `work`, then land and settle with
  `land`. Stop only when a hard stop or pre-authorization gap appears.
- **Review still happens.** Code review, checks, and spec sync are automatic
  unless a hard stop or pre-authorization gap appears. Leave the evidence — review
  notes, check output, PR/merge links, pass proof — recorded where the lifecycle
  skill keeps it.
- **Do not duplicate the lifecycle skills.** Invoke and follow `ready`,
  `work`, and `land` when their contracts apply. This skill orchestrates
  them; it does not rewrite their rules. When moving from one to the next, load
  and follow that skill body instead of reimplementing it from memory.
- **Carry the original context.** Keep the user's original context through every
  step. Boundaries stay explicit: preparation is not launch, TaskRun pass is not
  landing, cleanup happens only after landing or discard intent is proven, and the
  retrospective is written after the work item is closed — not mid-flight.

## Start Checklist

Before doing work:

1. Read `references/task-lifecycle.md` for the object model and status
   boundaries.
2. Run `git status --short --branch` and confirm the worktree/branch state.
3. Confirm the `ready` handoff exists and is approved: launch command, inspect
   target, workflow policy, and TaskDocument bodies are present and internally
   consistent.
4. Read `references/approval-policy.md` when the request involves execution with
   external side effects, credentials, cost, security, privacy, or ambiguity
   about what autopilot may decide on the user's behalf.

If any start check fails, stop with the smallest needed repair or user question.
Unprepared or vague work returns to `ready`, not into autopilot.

## Workflow

1. **Consume the handoff.** Treat the approved launch target as the current
   contract. If execution reveals it is wrong (bad slicing, wrong policy, changed
   scope), return to `ready`, record why, and do not continue on the old
   contract.
2. **Execute and coordinate with `work`.** Launch the prepared task or
   workflow, capture the inspect target, monitor the run, inspect agent state,
   review code, run checks, sync the living spec, and send focused feedback until
   the work is acceptable.
3. **Audit before landing.** Do not treat "PR opened" or "files changed" as done.
   Map the launch target and acceptance criteria to evidence: review verdict,
   check/test output, and unresolved assumptions.
4. **Land and settle with `land`.** Respect workflow landing policy, perform
   any applicable pass step, land branches in the right order, prove ancestry or
   discard intent, clean up with `wt done`, and record keep/problem/try lessons,
   action candidates, harness-tuning records, and expected vs actual duration with
   watch-cadence evidence. Write the timing entry even when there was no broader
   lesson.

## Hard Stops

Stop and ask for explicit user direction when any of these appear without prior
authorization:

- the `ready` handoff is missing, stale, contradicted, or not approved;
- an unresolved HITL decision the agent surfaced;
- destructive or hard-to-revert changes;
- credentials, secrets, external accounts, production systems, deployment, or
  cost-incurring actions;
- public or external sharing;
- security, privacy, legal, policy, or permission-boundary decisions;
- scope expansion, a split decision, or a changed core of the work;
- merge conflicts owned by the task branch, or unsafe cleanup conditions;
- failed review, failed checks, or a failed completion audit;
- the same failure repeats three times;
- active agent work that still legitimately needs time (wait, do not force).

When progress stops for a reusable reason, write a retrospective to record why,
even though landing did not happen.

## Completion Audit

Before reporting completion, show:

- the launch target consumed (launch command, inspect target, workflow policy);
- the current lifecycle step and TaskRun/workspace state;
- review and check verdicts with evidence paths (review notes, check output,
  PR/merge or pass proof);
- the landing or discard proof: merge/ancestry evidence, or proven discard intent
  plus the `wt done` cleanup command;
- the retrospective file path when written, including timing and watch-cadence
  evidence;
- hard stops checked and not triggered, or the stop that remains;
- any remaining blocker.

Do not hide unresolved assumptions. If something was delegated to a task agent
rather than human-approved, say where that delegation is recorded.

## Anti-Patterns

- Starting before the `ready` handoff is approved.
- Treating "no human approval needed" as "no review needed".
- Calling work landed because a PR exists, without a completion audit.
- Absorbing a task-branch merge conflict into the coordinator without the user
  asking for it.
- Running cleanup before ancestry or explicit discard intent is proven.
- Writing the retrospective mid-flight instead of after the item is closed.
- Rewriting `ready` / `work` / `land` contracts inside this skill.
