# Design Critic Pass

Use this reference when a Gate 7 design is high-risk, non-obvious, or likely to
shape several downstream tasks. This is an optional review surface for
`wt-ready`; it is not an automatic consensus loop and does not require a
specific agent runtime. The reviewer may be the user, another human, another
agent, or a subagent.

## When To Request It

Request or prepare a critic pass when the design involves:

- security, privacy, auth, secrets, or permission boundaries
- migrations, destructive changes, data loss risk, or irreversible operations
- public CLI/config/state shape changes
- cross-module or cross-spec coupling
- new user-facing model terms
- large UI/workflow behavior shifts
- one asserted option with weak alternatives or unclear drivers

## Verdicts

Use one verdict:

- `APPROVE`: the design is actionable without guessing.
- `ITERATE`: the design is promising but needs specific revision before Gate 8.
- `REJECT`: the design is not safe or coherent enough to task.

## Review Criteria

Check:

- Principles and drivers: the chosen option follows the stated principles and
  decision drivers.
- Fair alternatives: viable options are real, not strawmen. If only one option
  remains, rejected options have explicit invalidation rationale.
- Steelman antithesis: the strongest argument against the chosen option is
  stated and answered.
- Requirements fit: the design covers the active topology and success criteria
  from Gates 1-5.
- Wireframe fit: the design generalizes the approved concrete case instead of
  silently changing structure.
- Brownfield evidence: assumptions about existing components, state, commands,
  docs, or behavior are checked where cheap.
- Risk mitigation: security, migration, compatibility, performance, and
  operational risks have concrete mitigations or explicit accepted residual
  risk.
- Verification path: Gate 8 can produce tasks with acceptance checks that would
  prove the design works.

## Output Shape

```text
Verdict: APPROVE | ITERATE | REJECT

Reason:
-

Required revisions:
-

Residual risks:
-
```

For `ITERATE` or `REJECT`, name the smallest change that would let the design
return to review. Do not expand into implementation planning; that belongs in
Gate 8 after the design is settled.
