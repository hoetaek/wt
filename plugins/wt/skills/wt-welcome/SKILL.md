---
name: wt-welcome
description: "Use to onboard a user to wt by explaining the mental model, first machine setup, the wt skill family, common `wt run` starts, and when to continue into $wt-config or $wt-autopilot without overwhelming them."
---

# WT Welcome

Use this skill when a user is new to `wt`, asks how to start, asks what the wt
skills are for, or needs a high-level map before configuring or running work.

The primary job is to shape the user's mental model. Do not turn the first
answer into a manual, full command reference, or field-by-field config tour.

## Response Goal

Give the user a compact map:

```text
wt setup   = prepare this machine
wt init    = prepare this repo's wt state/config
wt run     = start work
wt-autopilot = after wt-ready, run, review, land, and settle the work
wt-config  = inspect this repo and recommend the right config
```

Explain that `wt run` starts workspaces; it does not own review, landing,
cleanup, or long-running coordination. Those are handled by `wt-autopilot` and its
phase skills.

## First Answer Shape

Use this order:

1. **Big picture first.** Give the five-line model above in the user's language.
2. **Per-machine setup.** Explain that shell integration, agent hooks, cmux,
   and agent CLIs are machine/user concerns. Suggest:

   ```bash
   wt setup
   wt doctor
   ```

   If the user already knows their agent profile, mention:

   ```bash
   wt doctor --profile codex
   ```

3. **wt skills.** Keep this short:
   - `$wt-config`: inspect the current repo and recommend repo config.
   - `$wt-autopilot`: after `$wt-ready` hands off an approved launch target, run
     the work, review it, land or discard it, and settle reusable lessons into
     the harness.
   - Phase skills `$wt-ready`, `$wt-work`, and `$wt-land` can be run on their own
     for per-step control; `$wt-autopilot` chains `$wt-work` and `$wt-land`
     automatically after the `$wt-ready` handoff.
4. **Common `wt run` starts.** Show only the most useful commands:

   ```bash
   wt run branch <words...>
   wt run issue
   wt run issue PROJ-123 --base .
   wt run pr
   wt run task
   wt run workflow
   ```

   Explain each in one short phrase.
5. **Next action.** End the basic onboarding by telling the user to run
   `$wt-config` for the current repo. If they want actual work executed after
   config is ready, point them to `$wt-autopilot`.
6. **Progressive disclosure.** Ask whether they want the deeper model. Only if
   they say yes, explain TaskDocuments, Workflows, TaskRuns, and workflow modes.

## Deeper Model, Only On Request

When the user asks for more detail, explain these concepts compactly:

- **TaskDocument:** a local executable work unit stored under
  `<repo-root>/.wt/execution/tasks/`.
- **Workflow:** a saved execution plan that groups TaskDocuments or provider
  issues.
- **TaskRun:** the record of one launched execution.
- **Modes:** `single` shares one workspace, `batch` runs independent branches,
  `stack` runs ordered dependent branches, and `matrix` runs one task through
  multiple profiles.
- **wt-autopilot loop:** `$wt-ready -> $wt-work -> $wt-land`.

Keep the deeper model conceptual unless the user asks for concrete commands.

## Guardrails

- Prefer "what owns what" over "everything wt can do".
- Do not run `wt setup`, `wt init`, `wt run`, or config edits unless the user
  explicitly asks for execution.
- If the user is already inside a repo and asks what to do next, end with:
  "Run `$wt-config` here first."
- If the user is ready to do real work, suggest `$wt-autopilot` after config is
  healthy.
