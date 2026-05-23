---
name: wt-idea
description: "Use before wt-ready for vague ideas or future wt work: capture context, evidence, alternatives, risks, and stop before TaskDocuments or workflows."
---

# WT Idea

Use this skill to capture and enrich ideas before they are ready for
`wt-ready`. The goal is to preserve enough evidence and context that a later
`wt-ready` pass can turn the idea into the best available task/workflow plan.
In the work-sequence model, this skill owns raw intent and context/reference
exploration before the work commits to a spec, TaskDocument, or workflow.

Do not implement code, create TaskDocuments, create workflows, launch worktrees,
or decide final scope from this skill. Use `wt-ready` for idea-to-task
conversion, `wt-start` for execution, `wt-coordinate` for running work, and
`wt-land` for reviewed work.

## Kill-able Identity

An idea is exploration. It is allowed to die.

- An idea file at `<git-common-dir>/wt/ideas/<slug>.{md,toml}` may be deleted,
  rewritten, or abandoned at any time without any state transition that other
  components observe.
- No downstream consumer (wt CLI, wt-ready, wt-start, workflows) depends on an
  idea file continuing to exist. Removing one is not breakage.
- Do not promote prematurely. Do not create TaskDocuments, specs, or workflows
  from inside `wt-idea`. Promotion is `wt-ready`'s job: when the user commits
  in `wt-ready`, the idea file is removed and a `specs/<slug>/` directory
  (with `requirements.md`, `design.md`, `tasks.md`) takes its place. That
  directory move is the visible commit gate, not anything `wt-idea` does.
- Treat the idea body as scratch surface, not a contract. Optimise for honest
  exploration, including recording reasons to drop the idea entirely.

## First Read

Inspect local truth before asking questions:

```bash
git status --short --branch
find . -maxdepth 2 -name AGENTS.md -o -name AGENTS.override.md
common_dir="$(git rev-parse --git-common-dir)"
find "$common_dir/wt/ideas" "$common_dir/wt/tasks" "$common_dir/wt/workflows" -maxdepth 1 -type f 2>/dev/null | sort
find "$common_dir/wt/specs" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort
wt config 2>/dev/null || true
```

Never read secret files such as `.env`. If `<git-common-dir>/wt/ideas` does not exist,
create it only when saving a new idea.

## Capture Standard

An idea is not a task. Capture it as a durable research artifact with:

- the user's raw intent, preserving wording when useful
- the user/customer purpose and success criteria, when they are visible
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

## Unknown Surfacing

Before researching anything, list what is missing. Without this step,
Evidence Gathering becomes reactive — the same kinds of research keep
surfacing mid-work and become unplanned detours.

Categorize unknowns:

- **Domain concepts** — meaning of core terms in the user's wording, the
  product domain, or the wt model (e.g., "what does the user mean by
  'profile' here — `[profile]` config layer, matrix profile name, or
  shell-side preset?").
- **Standards / conventions** — accepted patterns, established practice for
  the kind of change being proposed (e.g., "what is the wt convention for
  CLI verb-noun pairs?", "what does `docs/consistency.md` say about this?").
- **External facts** — comparable cases, prior art, authoritative sources,
  recent changes in the relevant ecosystem.
- **Internal facts** — what the repo already has (config shape, existing
  state files, prior decisions, tests, related ideas/specs/tasks) that may
  not have been inventoried yet.

For each unknown, mark `blocking now` or `useful later`. The most expensive
unknowns — the ones that would unravel later prep or execution if unresolved
— get researched first in Evidence Gathering.

Record the surfaced list in the idea body under a `미지 (Unknowns)` section
so `wt-ready` (or a future `wt-idea` pass) can use it as the agenda. Example:

```text
미지 (Unknowns):

Domain (blocking now):
- "profile"이 가리키는 게 [profile] 레이어인가 matrix profile name인가?

Standards (blocking now):
- wt CLI 동사-명사 쌍 규칙(`docs/consistency.md`)이 이 케이스에 적용되나?

External (useful later):
- 같은 문제를 푼 다른 도구(예: jj, sapling)의 명령 모양.

Internal (blocking now):
- 현재 .wt.toml / config.toml에 비슷한 옵션이 이미 있는가?
```

When a new unknown surfaces *after* this step (during ready/start/coordinate),
that is a signal Surfacing was incomplete; the runtime owner logs it to
`<git-common-dir>/wt/specs/<slug>/mid-process-discoveries.md` so the
retrospective can diagnose the missed category.

## Evidence Gathering

Use the **Unknown Surfacing** list as the agenda. Start from the conversation
and repository, searching existing local artifacts before creating a new one:

```bash
common_dir="$(git rev-parse --git-common-dir)"
rg -n "<keyword>|<related term>" "$common_dir/wt/ideas" "$common_dir/wt/tasks" docs app resources tests 2>/dev/null
```

Use external research when the user asks for best practices, the idea concerns
current tooling/frameworks, or the best direction cannot be judged from the repo
alone. Prefer primary or authoritative sources: official docs, established
method writeups, source repositories, standards, and vendor docs. Record URLs in
the idea body.

Context/reference exploration is part of sharpening raw intent, not proof that
the idea is already a task. Keep it bounded: gather enough examples to name
2-4 plausible frames, record the tradeoff for each, and stop before choosing a
final output form unless the user explicitly commits to prep.

Useful discovery lenses:

- Product discovery: purpose -> opportunity -> solution -> experiment.
- Feedback synthesis: raw feedback -> insight -> related idea.
- Triage inbox: review, dedupe, label, clarify, then accept/defer/archive.
- Shape Up style shaping: problem, appetite, solution sketch, rabbit holes,
  no-gos.
- Technical decision records: context, options, decision drivers, consequences.

Separate confirmed facts, source-backed guidance, inference, and unresolved
questions.

## Status Model

Use these statuses in idea files. Every status describes a state of a living
idea file; once `wt-ready` promotes the idea, the file itself is removed (see
"Kill-able Identity"), so there is no post-promotion status to record here.

- `captured`: raw idea saved with minimal context.
- `enriched`: local/external context and alternatives have been gathered.
- `ready_for_wt_ready`: enough information exists for `wt-ready` to prepare
  specs, TaskDocuments, or workflows without rediscovering raw intent,
  references, plausible frames, tradeoffs, and the next unresolved question.
- `archived`: intentionally not pursuing now.

Default to `enriched` when you performed meaningful research. Use
`ready_for_wt_ready` only when scope, unresolved questions, and next checks are
clear enough for `wt-ready` to proceed without rediscovering the basics. There
is no `converted` status: promotion deletes the idea file and creates
`specs/<slug>/`, so a converted idea has no file left to carry a status.

## File Format

Store ideas in `<git-common-dir>/wt/ideas/<slug>.{md,toml}`. Use lowercase ASCII
kebab-case slugs. Pick the extension by what fits the body best:

To seed an empty Markdown skeleton, run `wt scaffold <slug> --idea`. It writes
`<git-common-dir>/wt/ideas/<slug>.md` with the canonical section headings. To
use `.toml` instead, write the file by hand using the schema below.

- `.md` for free-form Markdown notes when prose, links, and loose structure
  serve the exploration best.
- `.toml` when you want a few simple top-level fields plus a `body` string.

If an idea already exists at either extension, update that file instead of
creating a duplicate. Do not write into `<git-common-dir>/wt/specs/` from this
skill; that directory is `wt-ready`'s output.

TOML shape, when you choose `.toml`. Use only simple top-level fields and put
rich planning context in `body`:

```toml
title = "Short Korean title"
status = "enriched"
created_at = "YYYY-MM-DD"
updated_at = "YYYY-MM-DD"
source = "user"
tags = ["wiki", "product-discovery"]

body = """
원문 의도:
- ...

맥락 / 레퍼런스 탐색:
- 로컬: ...
- 외부: ...
- 참고한 방향: ...

목적 / 성공 기준:
- ...

선택지:
- 선택지 A: ...
- 선택지 B: ...

트레이드오프:
- ...

리스크 / 함정:
- ...

비목표:
- ...

열린 질문:
- ...

다음 단계:
- Run wt-ready on this idea when ...
"""
```

Markdown shape, when you choose `.md`. Keep front matter optional and lean,
and put the same sections (원문 의도, 맥락 / 레퍼런스 탐색, 목적 / 성공 기준,
선택지, 트레이드오프, 리스크 / 함정, 비목표, 열린 질문, 다음 단계) as plain
Korean headings or bullets.

Do not add nested TOML tables unless the local schema explicitly adopts them.
Keep the file easy to read, diff, and later hand to `wt-ready`.

### Optional light EARS seed

If, while capturing the idea, you already know a core behaviour clearly, you
MAY include one light EARS-style line such as:

```text
WHEN <condition> THE SYSTEM SHALL <behavior>.
```

This is optional. Do not force EARS phrasing in the idea stage; vague ideas
should stay vague. When it is naturally there, it gives `wt-ready` a head start
on `requirements.md`. When it is not, leave it out.

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
- the missing work-sequence gate when it is not ready
- exact next skill invocation or target, for example:

```text
$wt-ready <git-common-dir>/wt/ideas/<slug>.md
```

When `wt-ready` is later invoked on the idea and the user commits, `wt-ready`
will remove this idea file and create `<git-common-dir>/wt/specs/<slug>/` with
`requirements.md`, `design.md`, and `tasks.md`. That promotion is not done from
inside `wt-idea`.

If the user asked only for a list or review of ideas, do not write files unless
they explicitly ask to register or update an idea.
