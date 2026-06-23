# WT Autopilot Approval Policy

Use this reference when deciding whether `wt-autopilot` may continue without
asking the user.

## Decision Model

`wt-autopilot` uses three layers:

1. **Human-reviewed handoff** — the `wt-ready` launch target (launch command,
   inspect target, workflow policy, TaskDocument bodies) is reviewed before
   autopilot starts. This is the ownership boundary.
2. **Automatic coordination and review** — after the handoff, the agent may
   launch, monitor, inspect, review code, run checks, sync the spec, and send
   feedback without asking at every step.
3. **Hard stops** — when risk or ownership exceeds the pre-authorized lane, stop
   and ask.

## May Continue Automatically

Continue without asking when all are true:

- the action follows the approved launch target and workflow policy;
- the change is local to the repo, a worktree, or `.wt/` state;
- the action is reversible or reviewable before it lands;
- no credential, production, public sharing, cost, legal, security, privacy, or
  permission boundary is crossed;
- the required evidence can be gathered locally (review notes, check output);
- landing follows the workflow policy already approved at handoff.

Examples:

- launching the prepared task or workflow and capturing the inspect target;
- monitoring a run, inspecting agent state, and sending focused feedback;
- running the project's checks, tests, lint, or build inside the worktree;
- reviewing the diff and syncing the living spec;
- landing via the approved workflow policy when review and checks pass.

## Must Stop

Stop when any of these are true:

- the handoff is absent, provisional, stale, or contradicted;
- execution reveals a different deliverable, slicing, or policy is needed;
- destructive or hard-to-revert operations are needed;
- secrets, credentials, external accounts, production systems, deployment, or
  paid services are involved;
- public/external communication or sharing is involved;
- security, privacy, legal, policy, or permission boundaries are affected;
- the task requires user taste, organizational risk tolerance, or stakeholder
  judgment not encoded in the handoff;
- a merge conflict is owned by the task branch, or cleanup would be unsafe;
- review fails, checks fail, or the completion audit fails;
- the same failure repeats three times.

## Recording Delegation

When continuing automatically at a former approval point, record:

- the pre-authorized basis: the approved launch target and this policy;
- what was reviewed automatically and what evidence passed;
- what hard stops were checked;
- what remains unresolved or was delegated to the task agent.

Use the nearest durable surface:

- the TaskRun / inspect target for current execution state;
- the review and check output for gate-specific evidence;
- the `wt-land` retrospective for close-out findings, timing, and watch cadence.

## Return Rule

If coordination finds the handoff is wrong (bad slicing, wrong policy, changed
scope or core), return to `wt-ready`. Do not patch around it downstream. Record
what falsified the handoff, reopen only the affected part, and wait for human
review before continuing.
