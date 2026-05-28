import clsx from "clsx";
import { h, type ComponentChildren } from "preact";

type WorkflowOrigin = {
  provider: string;
  id: string;
};

export type WorkflowTask = {
  task: string;
  run: string;
  parent?: string | null;
  runs: Array<{ profile: string; run: string }>;
};

export type WorkflowDetail = {
  id: string;
  path: string;
  title?: string | null;
  body?: string | null;
  origin?: WorkflowOrigin | null;
  mode: string;
  profile?: string | null;
  profiles: string[];
  base_mode: string;
  base?: string | null;
  color?: string | null;
  created_at: string;
  updated_at: string;
  policy: {
    pull_request: string;
    landing: string;
  };
  tasks: WorkflowTask[];
};

type IconComponent = (props: Record<string, unknown>) => ComponentChildren;

const transition =
  "transition-[transform,opacity,background-color,color,box-shadow,filter] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]";

export function WorkflowView(props: {
  workflow: WorkflowDetail | null;
  loading: boolean;
  iconComponent: IconComponent;
}) {
  if (props.loading) {
    return h("div", { class: "grid min-h-[24rem] place-items-center text-sm text-neutral-500 dark:text-neutral-400" }, "Workflow 읽는 중");
  }
  if (!props.workflow) {
    return h("div", { class: "grid min-h-[24rem] place-items-center text-sm text-neutral-500 dark:text-neutral-400" }, "선택된 Workflow가 없습니다.");
  }

  const workflow = props.workflow;
  return h("div", { class: "grid gap-6" }, [
    h("div", { class: "grid gap-4 md:grid-cols-2" }, [
      readOnlyField("ID", workflow.id, "font-mono"),
      readOnlyField("Mode", workflow.mode),
      readOnlyField("Title", workflow.title || "제목 없음", "md:col-span-2"),
      readOnlyField("Base", workflow.base || workflow.base_mode),
      readOnlyField("Color", workflow.color || "없음", "", workflow.color),
      readOnlyField("Pull request", workflow.policy.pull_request),
      readOnlyField("Landing", workflow.policy.landing),
      readOnlyField("Created", workflow.created_at),
      readOnlyField("Updated", workflow.updated_at)
    ]),
    h("section", { class: "grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300" }, [
      h("span", {}, "Body"),
      h(
        "div",
        {
          class: clsx(
            "min-h-[10rem] whitespace-pre-wrap rounded-[1.5rem] bg-white/70 px-5 py-4 text-sm leading-6 text-neutral-700 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 dark:bg-neutral-900/70 dark:text-neutral-300 dark:ring-white/10",
            transition
          )
        },
        workflow.body || "본문 없음"
      )
    ]),
    h("section", { class: "grid gap-3" }, [
      h("div", { class: "flex items-center gap-3" }, [
        h("span", { class: "grid h-10 w-10 place-items-center rounded-full bg-studio-accent/10 text-studio-accent ring-1 ring-studio-accent/20" }, h(props.iconComponent, { size: 20, weight: "light", className: "h-5 w-5" })),
        h("div", {}, [
          h("p", { class: "text-sm font-medium text-neutral-950 dark:text-neutral-50" }, `${workflow.tasks.length} tasks`),
          h("p", { class: "text-xs text-neutral-500 dark:text-neutral-500" }, workflow.path)
        ])
      ]),
      h("div", { class: "overflow-hidden rounded-[1.5rem] ring-1 ring-black/5 dark:ring-white/10" }, [
        ...workflow.tasks.map((task, index) =>
          h("div", { key: `${task.task}-${index}`, class: "grid gap-1 border-b border-black/5 bg-white/50 px-5 py-4 last:border-b-0 dark:border-white/10 dark:bg-white/[0.03]" }, [
            h("div", { class: "flex flex-wrap items-center gap-2" }, [
              h("span", { class: "font-mono text-sm text-neutral-950 dark:text-neutral-100" }, task.task),
              task.parent && h("span", { class: "rounded-full bg-black/[0.04] px-2 py-0.5 text-[10px] uppercase tracking-[0.2em] text-neutral-500 ring-1 ring-black/5 dark:bg-white/[0.04] dark:ring-white/10" }, `parent ${task.parent}`)
            ]),
            h("p", { class: "font-mono text-xs text-neutral-500 dark:text-neutral-500" }, task.run || task.runs.map((run) => `${run.profile}:${run.run}`).join(", "))
          ])
        )
      ])
    ]),
    workflow.origin &&
      h("section", { class: "grid gap-2 rounded-[1.5rem] bg-black/[0.03] p-5 text-sm ring-1 ring-black/5 dark:bg-white/[0.03] dark:ring-white/10" }, [
        h("p", { class: "text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-400 dark:text-neutral-500" }, "Origin"),
        h("p", { class: "font-mono text-neutral-700 dark:text-neutral-300" }, `${workflow.origin.provider}:${workflow.origin.id}`)
      ])
  ]);
}

function readOnlyField(label: string, value: string, className = "", color?: string | null) {
  return h("div", { class: clsx("grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300", className) }, [
    h("span", {}, label),
    h("div", { class: "flex h-12 items-center gap-3 rounded-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10" }, [
      color && h("span", { class: "h-3 w-3 rounded-full ring-1 ring-black/10 dark:ring-white/20", style: { backgroundColor: workflowColor(color) } }),
      h("span", { class: "truncate" }, value)
    ])
  ]);
}

function workflowColor(color: string) {
  const palette: Record<string, string> = {
    red: "#ef4444",
    crimson: "#dc143c",
    orange: "#f97316",
    amber: "#f59e0b",
    olive: "#84cc16",
    green: "#22c55e",
    teal: "#14b8a6",
    aqua: "#06b6d4",
    blue: "#3b82f6",
    navy: "#1d4ed8",
    indigo: "#6366f1",
    purple: "#a855f7",
    magenta: "#d946ef",
    rose: "#f43f5e",
    brown: "#92400e",
    charcoal: "#374151"
  };
  return palette[color.toLowerCase()] || "#3b82f6";
}
