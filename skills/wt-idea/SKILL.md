---
name: wt-idea
description: "Use before wt-ready for vague ideas or future wt work: capture context, evidence, alternatives, risks, and stop before TaskDocuments or workflows."
---

# WT Idea

Use this skill to capture and enrich ideas before they are ready for
`wt-ready`. The goal is to preserve enough evidence and context that a later
`wt-ready` pass can turn the idea into the best available task/workflow plan.

Do not implement code, create TaskDocuments, create workflows, launch worktrees,
or decide final scope from this skill. Use `wt-ready` for idea-to-task
conversion, `wt-start` for execution, `wt-coordinate` for running work, and
`wt-land` for reviewed work.

## First Read

Inspect local truth before asking questions:

```bash
git status --short --branch
find . -maxdepth 2 -name AGENTS.md -o -name AGENTS.override.md
find .local/ideas .local/tasks .local/workflows -maxdepth 1 -type f 2>/dev/null | sort
wt config 2>/dev/null || true
```

Never read secret files such as `.env`. If `.local/ideas` does not exist,
create it only when saving a new idea.

## Capture Standard

An idea is not a task. Capture it as a durable research artifact with:

- the user's raw intent, preserving wording when useful
- the user/customer problem or desired outcome
- relevant local product/code/docs context
- related existing ideas, tasks, workflows, issues, or docs
- external best-practice references when the decision depends on current or
  domain-specific practice
- possible solution directions, with tradeoffs
- assumptions that need validation
- risks, rabbit holes, and explicit non-goals
- open questions for a later HITL or `wt-ready` pass
- a recommendation for the next step: enrich more, run `wt-ready`, defer, or
  archive

Prefer information density over premature certainty. A good `wt-idea` output
makes later decisions better; it does not force one solution too early.

## Evidence Gathering

Start from the conversation and repository. Search existing local artifacts
before creating a new one:

```bash
rg -n "<keyword>|<related term>" .local/ideas .local/tasks docs app resources tests 2>/dev/null
```

Use external research when the user asks for best practices, the idea concerns
current tooling/frameworks, or the best direction cannot be judged from the repo
alone. Prefer primary or authoritative sources: official docs, established
method writeups, source repositories, standards, and vendor docs. Record URLs in
the idea body.

Useful discovery lenses:

- Product discovery: outcome -> opportunity -> solution -> experiment.
- Feedback synthesis: raw feedback -> insight -> related idea.
- Triage inbox: review, dedupe, label, clarify, then accept/defer/archive.
- Shape Up style shaping: problem, appetite, solution sketch, rabbit holes,
  no-gos.
- Technical decision records: context, options, decision drivers, consequences.

Separate confirmed facts, source-backed guidance, inference, and unresolved
questions.

## Status Model

Use these statuses in idea files:

- `captured`: raw idea saved with minimal context.
- `enriched`: local/external context and alternatives have been gathered.
- `ready_for_wt_ready`: enough information exists for `wt-ready` to prepare
  TaskDocuments/workflows.
- `converted`: already turned into TaskDocuments or workflow by `wt-ready`.
- `archived`: intentionally not pursuing now.

Default to `enriched` when you performed meaningful research. Use
`ready_for_wt_ready` only when scope, unresolved questions, and next checks are
clear enough for `wt-ready` to proceed without rediscovering the basics.

## File Format

Store ideas in `.local/ideas/<date>-<slug>.toml`. Use lowercase ASCII
kebab-case slugs. If an idea already exists, update the existing file instead
of creating a duplicate.

Use only simple top-level TOML fields and put rich planning context in `body`:

```toml
title = "Short Korean title"
status = "enriched"
created_at = "YYYY-MM-DD"
updated_at = "YYYY-MM-DD"
source = "user"
tags = ["wiki", "product-discovery"]

body = """
Raw intent:
- ...

Outcome / problem:
- ...

Evidence:
- Local: ...
- External: ...

Options:
- Option A: ...
- Option B: ...

Tradeoffs:
- ...

Risks / rabbit holes:
- ...

Non-goals:
- ...

Open questions:
- ...

Next step:
- Run wt-ready on this idea when ...
"""
```

Do not add nested TOML tables unless the local schema explicitly adopts them.
Keep the file easy to read, diff, and convert later.

## Questions

Ask only when the idea cannot be captured safely without an answer. Otherwise,
record uncertainty as an open question and proceed.

Ask at most one focused question at a time. Include the recommended default.

## Handoff

End with:

- idea file path
- status
- evidence checked
- duplicates or related artifacts found
- why it is or is not ready for `wt-ready`
- exact next skill invocation or target, for example:

```text
$wt-ready .local/ideas/YYYY-MM-DD-slug.toml
```

If the user asked only for a list or review of ideas, do not write files unless
they explicitly ask to register or update an idea.
