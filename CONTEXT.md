# wt

`wt` coordinates Git worktree-based coding work through workflow files that describe whether a prepared task set runs as one branch, independent branches, or an ordered branch chain.

## Language

**Workflow**:
A prepared execution plan stored under `.local/workflows` with exactly one execution mode.
_Avoid_: Batch file, stack file

**Workflow Mode**:
The shape of branch execution for a workflow: `single`, `batch`, or `stack`.
_Avoid_: Workflow type, command kind

**Workspace Color**:
A cmux-supported visual marker shared by workspaces opened for the same workflow.
_Avoid_: Mode color, task color

**Batch**:
A workflow mode where task branches run independently from the same base.
_Avoid_: Batch file, worker group

**Coordinator**:
The single owner of a workflow who schedules task runs, reviews worker output, resolves integration conflicts, and decides when the workflow advances.
_Avoid_: Master agent, manager agent

**Worker**:
An agent assigned to one branch-scoped unit of work inside a workflow.
_Avoid_: Batch worker, stack agent

**Task Document**:
A reusable definition of branch-scoped work to be performed.
_Avoid_: Issue, run

**Task Run**:
A recorded attempt to execute a task document in a branch.
_Avoid_: Task, job

## Relationships

- A **Workflow** has one **Coordinator**.
- A **Workflow** has exactly one **Workflow Mode**.
- A **Workflow** may define one **Workspace Color**; an automatically rotated color from wt's built-in cmux named-color palette is written back to the workflow file.
- A `single` **Workflow Mode** runs one or more **Task Documents** in one branch.
- A `batch` **Workflow Mode** runs multiple independent **Task Documents** from one base.
- A `stack` **Workflow Mode** runs ordered **Task Documents** as a branch parent chain.
- A **Worker** reports back to the **Coordinator** and does not advance the **Workflow**.
- A **Task Document** owns the branch name used to execute that work.
- A **Task Document** can be executed by many **Task Runs**.

## Example Dialogue

> **Dev:** "Should this be `batch` or `stack`?"
> **Domain expert:** "Create a **Workflow** and choose its **Workflow Mode**. Use `batch` for independent branches and `stack` for an ordered branch chain."

## Flagged Ambiguities

- "workflow" was considered as a replacement for stack-only work; resolved: **Workflow** is the shared container for `single`, `batch`, and `stack` modes.
- "batch" and "stack" remain valid **Workflow Mode** values but should not remain top-level command or state-file nouns.
