---
name: wt-writing-tasks
description: "Use when authoring or revising a wt TaskDocument body (execution/tasks/*.toml) before launch — after wt-ready slicing, when a body lacks exact file paths, stepwise tests, commands with expected output, or when a launched agent drifted or stalled because the body under-specified the work."
---

# WT Writing Tasks

Write each TaskDocument body as an implementation-grade plan the launched
agent can execute without rediscovering the work. The agent starts in a fresh
worktree with zero conversation context; the body is its only guaranteed
input. External notes support the body, they do not replace it.

## Canonical location

- The TaskDocument body at `<repo-root>/.wt/execution/tasks/<slug>.toml` is
  the canonical home of implementation steps.
- The slice graph (slice titles, dependencies, parallel groups, execution
  shape, TaskDocument paths) lives wherever the prep happened — usually the
  wt-ready handoff report or a leaf workspace. Do not duplicate step detail
  there.
- The body may mention external context paths (for example `.leaf/...` files)
  as human-facing rationale only. wt does not parse or interpret them, and the
  agent must be able to execute from the body alone.

## Workflow handoff boundary

This skill writes TaskDocument bodies; it does not derive execution shape or
create saved Workflow TOML. When the slice graph contains parallel groups,
dependencies, or any slice marked for `batch`, `stack`, `single`, `matrix`, or
separate workflow execution, the prep pass must return to wt-ready after the
bodies are written. wt-ready owns:

- deriving direct vs `single` / `batch` / `stack` / `matrix` / wave-shaped
  orchestration from the slice graph;
- creating `<repo-root>/.wt/execution/workflows/<id>.toml` with
  `wt workflow task --mode ...` when a saved workflow is needed;
- reporting the TaskDocument mapping, linked workflow TOML path, `wt-work`
  launch target, policy source, and watch cadence in the handoff.

Do not call work launch-ready merely because all TaskDocument bodies are
authored. If a workflow decision is still missing, report that execution
handoff remains incomplete.

## Body structure

Top-down order. The agent reads top-down and often acts before reaching the
end, so constraints come before context, and context before steps.

1. `## 계획 (Planning)` — slice metadata from wt-ready: 유형 (type), 예상 소요
   (expected duration), 예상 근거 (estimate basis), 권장 watch cadence (suggested
   watch cadence), 막힘 / 의존성 (blocked by), 실행 형태 (execution shape),
   크기 (size class), 확인 방법 (acceptance checks). Prefer Korean
   human-facing labels with the stable English key in parentheses.
2. `## 필수 준수 (Hard constraints)` — within the first ~30 lines of the body.
   Design language rules, security envelope, cross-cutting prohibitions, and
   base-branch restrictions, each with the canonical contract path so the
   agent can pull detail without scrolling. Background: empirically (2026-05
   wt-studio retrospective, historical), constraints buried in the lower half
   of a long body are silently dropped by the first agent turn even when the
   prep notes fully state them; top-of-body placement is the cheap structural
   fix. Omit the section only when the slice genuinely has no such constraint.
3. `## 맥락 (Context)` — one-line goal, verified evidence with `file:line` or
   command output, and external context references by path.
4. `## 작업 (Tasks)` — implementation-grade tasks, the core of this skill.

## Task structure

Every task declares its files first, then bite-sized checkbox steps. One step
is one action (2-5 minutes).

````text
### Task 1: <component>

**Files:**
- Modify: `src/cli.rs` (ProfileCommand enum)
- Modify: `src/commands/profile.rs:12-40`
- Test: `tests/profile_list.rs` (create)

- [ ] Step 1: 실패 테스트 작성

```rust
#[test]
fn profile_list_orders_valid_profiles_by_name() {
    let ws = TestWorkspace::with_profiles(&["beta", "alpha"]);
    let out = ws.run_wt(&["profile", "list"]);
    assert!(out.status.success());
    assert_eq!(out.stdout_lines(), vec!["alpha", "beta"]);
}
```

- [ ] Step 2: 실패 확인

Run: `cargo test profile_list_orders_valid_profiles_by_name`
Expected: FAIL — clap rejects `list` as an unrecognized subcommand

- [ ] Step 3: 구현 (계약)

- `src/cli.rs`: add a `List` variant to `ProfileCommand` with `long_about`
  describing the inventory contract.
- `src/commands/profile.rs`: route `ProfileCommand::List` to the existing
  `list(ctx)`; do not change its output shape.
- Read inventory via `Config::load_profile_inventory`, not `load_profiles`,
  so invalid profiles surface as warnings instead of failing the list.

- [ ] Step 4: 통과 확인

Run: `cargo test profile_list_orders_valid_profiles_by_name`
Expected: PASS

- [ ] Step 5: 커밋

```bash
git add src/cli.rs src/commands/profile.rs tests/profile_list.rs
git commit -m "feat: add explicit wt profile list command"
```
````

### Adaptive code level

- **Test code is complete.** The failing test is the acceptance instrument; a
  contract-shaped test cannot fail meaningfully. Write the real assertions
  with realistic data.
- **Implementation is a contract**, not a code dump: exact file paths, the
  symbols to add or change, signatures, and behavior rules. The launched
  agent is a full agent with repo access — it writes the code; the body pins
  what the code must satisfy.
- Every symbol the contract names must either already exist in the repo
  (verify with grep/read while authoring) or be defined by an earlier task in
  the same body.

For non-code slices (docs-only, config, prototype), keep the same skeleton and
swap the instrument: the "test" becomes the observable check (rendered output,
generated TOML, command transcript) and "implement" becomes the authoring
contract.

## No placeholders

These are plan failures; never write them in a body:

- "TBD", "TODO", "나중에 결정", "적절히 처리", "엣지 케이스 처리"
- "테스트 추가" without the actual test code
- "Task N과 동일/유사" — repeat the contract; the agent may read out of order
- a step that says what to do without how (code, contract, or command required)
- a `Run:` command without its expected outcome
- a referenced type/function/path that no earlier task defines and the repo
  does not contain
- hedged file lists ("변경 예상 파일", "likely involved") or unresolved
  alternatives ("`src/lib.rs` 또는 `src/commands/task.rs`") — read the repo
  while authoring and commit to one

## Self-review before handoff

Run this checklist on the finished body; fix inline.

1. **Criteria coverage** — when prep notes state acceptance criteria (for
   example EARS sentences in a leaf workspace), every criterion maps to a
   task. List gaps.
2. **Placeholder scan** — search the body for the patterns above.
3. **Symbol consistency** — every referenced path/symbol verifies against the
   repo (`grep -rn "<symbol>" src/`) or an earlier task; names match across
   tasks exactly.
4. **Body order** — hard constraints appear within the first ~30 lines.
5. **Command check** — every `Run:` line is executable verbatim from the
   worktree root, and the `계획` acceptance checks match commands the steps
   actually run.
6. **Workflow handoff check** — if the slice graph has dependencies, parallel
   groups, or any non-direct execution shape, verify that wt-ready will create
   any required `.wt/execution/workflows/*.toml` and report the launch target
   before `wt-work`.

## Rationalizations

| Excuse | Reality |
|---|---|
| "Agent가 repo를 읽으니 경로는 생략해도 된다" | 경로 탐색이 드리프트의 첫 진입점이다. 경로를 적는 데 10초, agent가 잘못 찾으면 한 turn을 잃는다. |
| "파일 목록은 '예상(likely)'으로 적고 배치는 agent가 정한다" | Files는 확정 목록이다. 대안이 둘이면 prep에서 repo를 읽고 하나로 결정한다. 결정 위임은 드리프트 위임이다. |
| "구현이 계약이면 테스트도 계약이면 충분하다" | 테스트는 acceptance 기준 그 자체다. 완전하지 않으면 무엇이 통과인지 아무도 모른다. |
| "출력 shape는 '~같은' 예시면 충분하다" | 계약은 정확히 고정한다. "같은/유사한" 표기는 변형을 허용한다는 신호로 읽힌다. |
| "body가 길어지니 spec 참조로 대체한다" | 참조는 보조다. agent는 body만으로 실행 가능해야 한다. |
| "작은 슬라이스라 스텝 분해가 과하다" | small에도 실패 확인 → 구현 → 통과 확인 → 커밋 골격은 유지된다. 스텝 수가 줄 뿐이다. |
| "예상 출력은 실행해봐야 안다" | 그러면 prep에서 지금 실행해 확인한다. 금지되는 것은 추측 출력을 적는 것이다. |

## Red flags — stop and fix the body

- A task with no **Files** list
- A test step written in prose instead of code
- "구현은 agent가 알아서" 류의 위임 문장
- Acceptance checks that no step actually runs
- Hard constraints below the fold
