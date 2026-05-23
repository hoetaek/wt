# Spec Review Surfaces

Use this reference when displaying a spec draft for the user to review during
`wt-ready`'s Grill The Spec cycle. The goal is verification of the substance
without burning conversation tokens on bytes the user has already approved.

## Default: summarize, don't dump

- List what changed and where (file path + section heading + one-line gist
  per change: "added", "moved", "rewrote", "removed").
- Quote only the 2–10 lines that carry the substantive decision or wording
  the user must confirm — never the whole file.
- Goal: user verifies the substance of the revision in seconds.

## For end-to-end review, prefer a zero-token surface (in order)

1. **`wt config`-configured editor** — if `[editor].command` is set
   (e.g., `code {{path}}`, `vi {{path}}`, `cursor {{path}}`), invoke that
   command with the file path. Zero conversation tokens; full editor tooling
   (folding, syntax highlight).
2. **cmux markdown pane** — if cmux is active, open the file in a pane
   configured for markdown rendering. Zero conversation tokens; lets the user
   keep the agent conversation visible while reviewing.
3. **`Ctrl+E` to expand existing tool output** — if you just ran
   Read/Edit/Write, the file content is already in the Claude Code UI
   (collapsed by default). Direct the user to press `Ctrl+E` to expand it in
   place, instead of re-Reading and re-printing.

## Avoid

Re-Reading a file solely to print it back into chat — Claude Code collapses
tool output by default, so the dump is invisible without `Ctrl+E` anyway.
Acceptable only for very short files (< 50 lines) when no editor or cmux
pane exists.

When in doubt, ask once which mechanism the user prefers and apply that
choice consistently for the rest of the wt-ready cycle.
