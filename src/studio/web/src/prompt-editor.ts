import { h, type ComponentChildren } from "preact";
import clsx from "clsx";

export type PromptMode = "workflow" | "issue" | "branch" | "pr" | "common";

export const promptModes: PromptMode[] = ["workflow", "issue", "branch", "pr", "common"];

type IconComponent = (props: Record<string, unknown>) => ComponentChildren;

const transition =
  "transition-[transform,opacity,background-color,color,box-shadow,filter] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]";

export function PromptEditor(props: {
  profile: string;
  mode: PromptMode;
  value: string;
  iconComponent: IconComponent;
  onProfileChange: (profile: string) => void;
  onModeChange: (mode: PromptMode) => void;
  onChange: (value: string) => void;
}) {
  return h("div", { class: "grid gap-5" }, [
    h("div", { class: "grid gap-4 md:grid-cols-[minmax(0,1fr)_auto]" }, [
      h("label", { class: "grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300" }, [
        h("span", {}, "프로필"),
        h("input", {
          id: "profile-prompt-name",
          name: "profile-prompt-name",
          value: props.profile,
          onInput: (event: Event) => props.onProfileChange(formValue(event)),
          class: clsx(
            "h-12 rounded-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
            transition
          )
        })
      ]),
      h("div", { class: "grid content-end gap-2" }, [
        h("span", { class: "text-sm font-medium text-neutral-600 dark:text-neutral-300" }, "모드"),
        h(
          "div",
          { class: "flex flex-wrap gap-2" },
          promptModes.map((mode) =>
            h(
              "button",
              {
                key: mode,
                type: "button",
                onClick: () => props.onModeChange(mode),
                class: clsx(
                  "rounded-full px-3 py-2 text-xs font-medium ring-1 active:scale-[0.98]",
                  props.mode === mode
                    ? "bg-studio-accent text-white ring-studio-accent"
                    : "bg-black/[0.04] text-neutral-600 ring-black/5 hover:text-neutral-950 dark:bg-white/[0.05] dark:text-neutral-300 dark:ring-white/10 dark:hover:text-white",
                  transition
                )
              },
              promptModeLabel(mode)
            )
          )
        )
      ])
    ]),
    h("label", { class: "grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300" }, [
      h("span", { class: "flex items-center justify-between gap-4" }, [
        h("span", { class: "flex items-center gap-2" }, [icon(props.iconComponent), "Markdown"]),
        h("span", { class: "font-mono text-xs text-neutral-400 dark:text-neutral-500" }, promptLineSummary(props.value))
      ]),
      h("textarea", {
        id: "profile-prompt-markdown",
        name: "profile-prompt-markdown",
        value: props.value,
        onInput: (event: Event) => props.onChange(formValue(event)),
        rows: 18,
        spellcheck: false,
        class: clsx(
          "min-h-[28rem] rounded-[1.5rem] bg-white/70 px-5 py-4 font-mono text-sm leading-6 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
          transition
        )
      })
    ])
  ]);
}

export function promptLineSummary(value: string) {
  const lines = value.length === 0 ? 0 : value.split(/\r\n|\r|\n/).length;
  return `${lines.toLocaleString("ko-KR")}줄`;
}

export function promptModeLabel(mode: PromptMode) {
  switch (mode) {
    case "workflow":
      return "워크플로우";
    case "issue":
      return "이슈";
    case "branch":
      return "브랜치";
    case "pr":
      return "PR";
    case "common":
      return "공통";
  }
}

export function promptModeSummary(mode: PromptMode) {
  switch (mode) {
    case "workflow":
      return "워크플로우 작업 범위";
    case "issue":
      return "이슈 워크트리 범위";
    case "branch":
      return "브랜치 워크트리 범위";
    case "pr":
      return "PR 범위";
    case "common":
      return "공통 범위";
  }
}

function icon(IconComponent: IconComponent) {
  return h(IconComponent, { size: 16, weight: "light", className: "h-4 w-4" });
}

function formValue(event: Event) {
  return (event.currentTarget as HTMLInputElement | HTMLTextAreaElement).value;
}
