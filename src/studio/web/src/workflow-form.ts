import clsx from "clsx";
import { h, type ComponentChildren } from "preact";
import type { WorkflowDetail, WorkflowTask } from "./workflow-view";

export type WorkflowDraft = {
  title: string;
  body: string;
  color: string;
  pullRequest: string;
  landing: string;
};

type IconComponent = (props: Record<string, unknown>) => ComponentChildren;

const transition =
  "transition-[transform,opacity,background-color,color,box-shadow,filter] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]";

const workflowColors = [
  "red",
  "crimson",
  "orange",
  "amber",
  "olive",
  "green",
  "teal",
  "aqua",
  "blue",
  "navy",
  "indigo",
  "purple",
  "magenta",
  "rose",
  "brown",
  "charcoal"
];

export function draftFromWorkflow(workflow: WorkflowDetail): WorkflowDraft {
  return {
    title: workflow.title || "",
    body: workflow.body || "",
    color: workflow.color || "",
    pullRequest: workflow.policy.pull_request,
    landing: workflow.policy.landing
  };
}

export function workflowDraftEqual(left: WorkflowDraft, right: WorkflowDraft) {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function serializeWorkflowDraft(workflow: WorkflowDetail, draft: WorkflowDraft): string {
  const next = {
    ...workflow,
    title: optionalString(draft.title),
    body: optionalString(draft.body),
    color: optionalString(draft.color),
    policy: {
      pull_request: draft.pullRequest,
      landing: draft.landing
    }
  };

  const lines: string[] = [];
  if (next.title) lines.push(`title = ${quoteToml(next.title)}`);
  if (next.body) lines.push(`body = ${multilineToml(next.body)}`);
  lines.push(`mode = ${quoteToml(next.mode)}`);
  if (next.profile) lines.push(`profile = ${quoteToml(next.profile)}`);
  if (next.profiles.length > 0) {
    lines.push(`profiles = [${next.profiles.map(quoteToml).join(", ")}]`);
  }
  lines.push(`base_mode = ${quoteToml(next.base_mode)}`);
  if (next.base) lines.push(`base = ${quoteToml(next.base)}`);
  if (next.color) lines.push(`color = ${quoteToml(next.color)}`);
  lines.push(`created_at = ${quoteToml(next.created_at)}`);
  lines.push(`updated_at = ${quoteToml(next.updated_at)}`);

  if (next.origin) {
    lines.push("", "[origin]");
    lines.push(`provider = ${quoteToml(next.origin.provider)}`);
    lines.push(`id = ${quoteToml(next.origin.id)}`);
  }

  lines.push("", "[policy]");
  lines.push(`pull_request = ${quoteToml(next.policy.pull_request)}`);
  lines.push(`landing = ${quoteToml(next.policy.landing)}`);

  for (const task of next.tasks) {
    lines.push("", "[[tasks]]");
    lines.push(`task = ${quoteToml(task.task)}`);
    if (task.run.trim()) lines.push(`run = ${quoteToml(task.run)}`);
    if (task.parent) lines.push(`parent = ${quoteToml(task.parent)}`);
    for (const run of task.runs) {
      lines.push("", "[[tasks.runs]]");
      lines.push(`profile = ${quoteToml(run.profile)}`);
      lines.push(`run = ${quoteToml(run.run)}`);
    }
  }

  return `${lines.join("\n")}\n`;
}

export function WorkflowForm(props: {
  workflow: WorkflowDetail | null;
  draft: WorkflowDraft | null;
  loading: boolean;
  iconComponent: IconComponent;
  onChange: (draft: WorkflowDraft) => void;
}) {
  if (props.loading) {
    return h("div", { class: "grid min-h-[24rem] place-items-center text-sm text-neutral-500 dark:text-neutral-400" }, "Workflow 읽는 중");
  }
  if (!props.workflow || !props.draft) {
    return h("div", { class: "grid min-h-[24rem] place-items-center text-sm text-neutral-500 dark:text-neutral-400" }, "선택된 Workflow가 없습니다.");
  }

  const workflow = props.workflow;
  const draft = props.draft;
  const update = (patch: Partial<WorkflowDraft>) => props.onChange({ ...draft, ...patch });

  return h("div", { class: "grid gap-6" }, [
    h("div", { class: "grid gap-4 md:grid-cols-2" }, [
      field({ key: "title", label: "Title", value: draft.title }, (value) => update({ title: value })),
      field({ key: "color", label: "Color", value: draft.color, kind: "select", options: ["", ...workflowColors] }, (value) =>
        update({ color: value })
      ),
      field({ key: "pull-request", label: "Pull request", value: draft.pullRequest, kind: "select", options: ["none", "draft", "ready"] }, (value) =>
        update({ pullRequest: value })
      ),
      field({ key: "landing", label: "Landing", value: draft.landing, kind: "select", options: ["manual", "auto"] }, (value) => update({ landing: value }))
    ]),
    field({ key: "body", label: "Body", value: draft.body, kind: "textarea" }, (value) => update({ body: value })),
    h("div", { class: "grid gap-4 md:grid-cols-2" }, [
      readOnlyField("ID", workflow.id, "font-mono"),
      readOnlyField("Mode", workflow.mode),
      readOnlyField("Base", workflow.base || workflow.base_mode),
      readOnlyField("Created", workflow.created_at),
      readOnlyField("Updated", workflow.updated_at),
      readOnlyField("Profile", workflow.profile || workflow.profiles.join(", ") || "없음")
    ]),
    h("section", { class: "grid gap-3" }, [
      h("div", { class: "flex items-center gap-3" }, [
        h("span", { class: "grid h-10 w-10 place-items-center rounded-full bg-studio-accent/10 text-studio-accent ring-1 ring-studio-accent/20" }, h(props.iconComponent, { size: 20, weight: "light", className: "h-5 w-5" })),
        h("div", {}, [
          h("p", { class: "text-sm font-medium text-neutral-950 dark:text-neutral-50" }, `${workflow.tasks.length} tasks`),
          h("p", { class: "text-xs text-neutral-500 dark:text-neutral-500" }, workflow.path)
        ])
      ]),
      h("div", { class: "overflow-hidden rounded-[1.5rem] ring-1 ring-black/5 dark:ring-white/10" }, workflow.tasks.map(taskRow))
    ]),
    workflow.origin &&
      h("section", { class: "grid gap-2 rounded-[1.5rem] bg-black/[0.03] p-5 text-sm ring-1 ring-black/5 dark:bg-white/[0.03] dark:ring-white/10" }, [
        h("p", { class: "text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-400 dark:text-neutral-500" }, "Origin"),
        h("p", { class: "font-mono text-neutral-700 dark:text-neutral-300" }, `${workflow.origin.provider}:${workflow.origin.id}`)
      ])
  ]);
}

function field(
  spec: { key: string; label: string; value: string; kind?: "text" | "textarea" | "select"; options?: string[] },
  onInput: (value: string) => void
) {
  const id = `workflow-${spec.key}`;
  const baseClass = clsx(
    "w-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
    spec.kind === "textarea" ? "min-h-[14rem] rounded-[1.5rem] py-4 text-sm leading-6" : "h-12 rounded-full",
    transition
  );
  const control =
    spec.kind === "textarea"
      ? h("textarea", {
          id,
          value: spec.value,
          onInput: (event: Event) => onInput((event.currentTarget as HTMLTextAreaElement).value),
          class: baseClass
        })
      : spec.kind === "select"
        ? h(
            "select",
            {
              id,
              value: spec.value,
              onInput: (event: Event) => onInput((event.currentTarget as HTMLSelectElement).value),
              class: baseClass
            },
            spec.options?.map((option) => h("option", { key: option, value: option }, option || "unset"))
          )
        : h("input", {
            id,
            value: spec.value,
            onInput: (event: Event) => onInput((event.currentTarget as HTMLInputElement).value),
            class: baseClass
          });

  return h("label", { key: spec.key, class: clsx("grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300", spec.kind === "textarea" && "md:col-span-2") }, [
    h("span", {}, spec.label),
    control
  ]);
}

function readOnlyField(label: string, value: string, className = "") {
  return h("div", { class: clsx("grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300", className) }, [
    h("span", {}, label),
    h("div", { class: "flex h-12 items-center gap-3 rounded-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10" }, [
      h("span", { class: "truncate" }, value)
    ])
  ]);
}

function taskRow(task: WorkflowTask, index: number) {
  return h("div", { key: `${task.task}-${index}`, class: "grid gap-1 border-b border-black/5 bg-white/50 px-5 py-4 last:border-b-0 dark:border-white/10 dark:bg-white/[0.03]" }, [
    h("div", { class: "flex flex-wrap items-center gap-2" }, [
      h("span", { class: "font-mono text-sm text-neutral-950 dark:text-neutral-100" }, task.task),
      task.parent && h("span", { class: "rounded-full bg-black/[0.04] px-2 py-0.5 text-[10px] uppercase tracking-[0.2em] text-neutral-500 ring-1 ring-black/5 dark:bg-white/[0.04] dark:ring-white/10" }, `parent ${task.parent}`)
    ]),
    h("p", { class: "font-mono text-xs text-neutral-500 dark:text-neutral-500" }, task.run || task.runs.map((run) => `${run.profile}:${run.run}`).join(", "))
  ]);
}

function optionalString(value: string) {
  const trimmed = value.trim();
  return trimmed ? value : null;
}

function multilineToml(value: string) {
  if (value.startsWith("\n") || value.startsWith("\r")) {
    return quoteToml(value);
  }
  return `"""${value.replace(/\\/g, "\\\\").replace(/"""/g, '\\"\\"\\"')}"""`;
}

function quoteToml(value: string) {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n").replace(/\r/g, "\\r").replace(/\t/g, "\\t")}"`;
}
