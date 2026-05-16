---
name: consistency
description: "Review or implement product, CLI, config, documentation, and state-model changes for wt UX consistency. Use when working in the wt repository on consistency, 일관성, UX coherence, naming philosophy, default behavior, help text accuracy, config shape, command design, or whether code follows docs/consistency.md."
---

# Consistency

Use this skill to keep `wt`'s user-facing model predictable. Prefer concept clarity over compatibility aliases, hidden defaults, or implementation-shaped names.

## Canonical Means

In this repo, "canonical" means the official user-facing name, path, command, or
state shape that documentation, help text, tests, and new implementation should
treat as the source of truth.

A canonical model is not merely the newest code path or the most convenient
internal API. It is the one surface users should learn, scripts should target,
and future changes should extend. Competing old names may remain only as
explicit migration context, and they must not silently behave like aliases when
that would make two surfaces look equally valid.

When deciding whether something is canonical, ask:
- What name should a new user remember?
- Which file or command should a script write against?
- Where should new behavior be added?
- If two names conflict, which one remains and which one fails or becomes
  migration-only?

## Workflow

1. Read the `wt` source of truth first.
   - Prefer `docs/consistency.md` when present.
   - Also inspect nearby README, CLI help, config examples, tests, and command implementations that define user-visible behavior.

2. Identify the user-facing concepts before editing.
   - Name each concept in one sentence.
   - Separate "what is being acted on" from "how it runs" and "what state is stored".
   - Treat command names, option names, config keys, generated files, help text, and persisted state as one UX surface.

3. Apply the consistency checks.
   - One concept, one canonical name. Remove or reject competing names unless there is an explicit compatibility policy.
   - Canonical names must be reflected everywhere: command help, README examples, docs, tests, validation errors, state file paths, and agent handoffs.
   - Legacy names that remain for migration must be described as legacy or migration-only, hidden from the primary help surface when possible, and rejected with clear guidance instead of silently redirecting.
   - Different concepts stay separate. Do not let a config key, status field, or command mean two things.
   - Omission means default behavior. Do not persist fake resources such as `default` when the user simply omitted an option.
   - Ambiguity fails early. Reject conflicting options or config forms instead of guessing precedence.
   - Help text is a contract. Update help, docs, examples, and tests with the behavior.
   - Progressive disclosure. Keep the simple path small and make complex structure opt-in without creating a second concept.
   - State is explicit. Persist user intent and resumable status, not internal convenience values.
   - Agent-neutral names stay agent-neutral. Use tool-specific names only for tool-specific behavior.
   - Compatibility is secondary to clarity. If aliases remain, mark the canonical name and the reason.

4. When implementing, search for stale language and legacy paths.
   - Use `rg` for old names, fake default values, obsolete help text, stale docs, tests, and fixture data.
   - Update validation so invalid ambiguous states fail during parsing or command setup.
   - Add focused tests for canonical config, rejected conflicts, omitted defaults, stored state, and help/documentation expectations.

5. Validate through the public interface.
   - Run relevant tests and formatters.
   - For CLIs, inspect `--help` output for commands whose behavior changed.
   - Re-run `rg` for removed names and stale examples before finishing.

## Response Style

When reviewing only, lead with inconsistencies and cite concrete files or commands. When implementing, keep edits scoped to user-visible model alignment and report the behavioral contract that changed plus the validation run.
