import { h, type ComponentChildren } from "preact";
import clsx from "clsx";
import {
  configDraftEqual,
  draftFromToml,
  emptyConfigDraft,
  serializeConfigDraft,
  type ConfigDraft
} from "./config-form";

export type ProfileDraft = ConfigDraft;

type IconComponent = (props: Record<string, unknown>) => ComponentChildren;
type FieldSpec = {
  key: string;
  label: string;
  value: string;
  kind?: "text" | "textarea" | "select";
  options?: string[];
  placeholder?: string;
};

const transition =
  "transition-[transform,opacity,background-color,color,box-shadow,filter] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]";

export function emptyProfileDraft(): ProfileDraft {
  return emptyConfigDraft();
}

export function draftFromProfileToml(toml: string): ProfileDraft {
  return draftFromToml(toml);
}

export function serializeProfileDraft(draft: ProfileDraft): string {
  return serializeConfigDraft(draft);
}

export function profileDraftEqual(left: ProfileDraft, right: ProfileDraft) {
  return configDraftEqual(left, right);
}

export function profileSummary(draft: ProfileDraft) {
  const enabled = profileSectionNames.filter((name) => draft[name].enabled);
  return enabled.length > 0 ? enabled.join(", ") : "빈 profile.toml";
}

const profileSectionNames = ["worktree", "setup", "site", "workspace", "agent", "test"] as const;

export function ProfileForm(props: {
  draft: ProfileDraft;
  onChange: (draft: ProfileDraft) => void;
  iconComponent: IconComponent;
}) {
  const updateSection = <K extends keyof ProfileDraft>(section: K, patch: Partial<ProfileDraft[K]>) => {
    props.onChange({
      ...props.draft,
      [section]: { ...props.draft[section], ...patch }
    });
  };

  return h("div", { class: "grid gap-4" }, [
    sectionPanel("agent", "Agent", props.draft.agent.enabled, props.iconComponent, (enabled) => updateSection("agent", { enabled }), [
      field({ key: "cli", label: "CLI", value: props.draft.agent.cli, kind: "select", options: ["none", "codex", "claude", "gemini"] }, (value) =>
        updateSection("agent", { cli: value })
      ),
      field({ key: "args", label: "Args", value: props.draft.agent.args, placeholder: "one argument per line" }, (value) => updateSection("agent", { args: value })),
      field({ key: "command", label: "Command", value: props.draft.agent.command }, (value) => updateSection("agent", { command: value })),
      field({ key: "ready", label: "Ready", value: props.draft.agent.ready }, (value) => updateSection("agent", { ready: value })),
      field({ key: "submit", label: "Submit", value: props.draft.agent.submit, kind: "select", options: ["", "auto", "newline", "carriage_return", "none"] }, (value) =>
        updateSection("agent", { submit: value })
      ),
      field({ key: "timeout", label: "Timeout", value: props.draft.agent.timeout }, (value) => updateSection("agent", { timeout: value })),
      field({ key: "sendAfter", label: "Send after", value: props.draft.agent.sendAfter }, (value) => updateSection("agent", { sendAfter: value }))
    ]),
    sectionPanel("workspace", "Workspace", props.draft.workspace.enabled, props.iconComponent, (enabled) => updateSection("workspace", { enabled }), [
      field({ key: "tabs", label: "Tabs", value: props.draft.workspace.tabs, kind: "textarea", placeholder: "one tab per line" }, (value) =>
        updateSection("workspace", { tabs: value })
      ),
      field({ key: "postDepsTabs", label: "Post deps tabs", value: props.draft.workspace.postDepsTabs, kind: "textarea", placeholder: "one tab per line" }, (value) =>
        updateSection("workspace", { postDepsTabs: value })
      ),
      field({ key: "browserMode", label: "Browser mode", value: props.draft.workspace.browserMode, kind: "select", options: ["", "none", "system", "chrome_devtools"] }, (value) =>
        updateSection("workspace", { browserMode: value })
      ),
      field({ key: "browserUrl", label: "Browser URL", value: props.draft.workspace.browserUrl }, (value) => updateSection("workspace", { browserUrl: value })),
      field({ key: "browserApp", label: "Browser app", value: props.draft.workspace.browserApp }, (value) => updateSection("workspace", { browserApp: value })),
      field({ key: "chromePort", label: "Chrome port", value: props.draft.workspace.chromePort }, (value) => updateSection("workspace", { chromePort: value })),
      field({ key: "chromeUserDataDir", label: "Chrome user data dir", value: props.draft.workspace.chromeUserDataDir }, (value) =>
        updateSection("workspace", { chromeUserDataDir: value })
      )
    ]),
    sectionPanel("site", "Site", props.draft.site.enabled, props.iconComponent, (enabled) => updateSection("site", { enabled }), [
      field({ key: "provider", label: "Provider", value: props.draft.site.provider, kind: "select", options: ["none", "herd", "valet", "docker_proxy", "traefik"] }, (value) =>
        updateSection("site", { provider: value })
      ),
      field({ key: "name", label: "Name", value: props.draft.site.name }, (value) => updateSection("site", { name: value })),
      field({ key: "root", label: "Root", value: props.draft.site.root }, (value) => updateSection("site", { root: value })),
      field({ key: "secure", label: "Secure", value: props.draft.site.secure, kind: "select", options: ["", "true", "false"] }, (value) => updateSection("site", { secure: value })),
      field({ key: "url", label: "URL", value: props.draft.site.url }, (value) => updateSection("site", { url: value })),
      field({ key: "target", label: "Target", value: props.draft.site.target }, (value) => updateSection("site", { target: value }))
    ]),
    sectionPanel("worktree", "Worktree", props.draft.worktree.enabled, props.iconComponent, (enabled) => updateSection("worktree", { enabled }), [
      field({ key: "path", label: "Path", value: props.draft.worktree.path }, (value) => updateSection("worktree", { path: value })),
      field({ key: "copy", label: "Copy", value: props.draft.worktree.copy, kind: "textarea", placeholder: "one path per line" }, (value) => updateSection("worktree", { copy: value })),
      field({ key: "link", label: "Link", value: props.draft.worktree.link, kind: "textarea", placeholder: "one path per line" }, (value) => updateSection("worktree", { link: value })),
      field({ key: "injectLocalContext", label: "Inject local context", value: props.draft.worktree.injectLocalContext }, (value) =>
        updateSection("worktree", { injectLocalContext: value })
      )
    ]),
    sectionPanel("setup", "Setup", props.draft.setup.enabled, props.iconComponent, (enabled) => updateSection("setup", { enabled }), [
      field({ key: "env", label: "Env", value: props.draft.setup.env, kind: "textarea", placeholder: "KEY=value" }, (value) => updateSection("setup", { env: value }))
    ]),
    sectionPanel("test", "Test", props.draft.test.enabled, props.iconComponent, (enabled) => updateSection("test", { enabled }), [
      field({ key: "commands", label: "Commands", value: props.draft.test.commands, kind: "textarea", placeholder: "command labels for simple validation" }, (value) =>
        updateSection("test", { commands: value })
      )
    ])
  ]);
}

function sectionPanel(
  key: string,
  title: string,
  enabled: boolean,
  IconComponent: IconComponent,
  onToggle: (enabled: boolean) => void,
  children: ComponentChildren[]
) {
  return h(
    "section",
    {
      key,
      class: clsx(
        "grid gap-4 rounded-[1.5rem] p-4 ring-1",
        enabled
          ? "bg-white/70 ring-black/5 dark:bg-neutral-900/70 dark:ring-white/10"
          : "bg-black/[0.02] opacity-70 ring-black/5 dark:bg-white/[0.02] dark:ring-white/10"
      )
    },
    [
      h("div", { class: "flex items-center justify-between gap-4" }, [
        h("div", { class: "flex items-center gap-3" }, [
          h("span", { class: "grid h-9 w-9 place-items-center rounded-full bg-studio-accent/10 text-studio-accent ring-1 ring-studio-accent/20" }, h(IconComponent, { size: 18, weight: "light", className: "h-4 w-4" })),
          h("h3", { class: "text-base font-medium text-neutral-950 dark:text-neutral-50" }, title)
        ]),
        h("label", { class: "inline-flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-neutral-500 dark:text-neutral-400" }, [
          h("input", {
            type: "checkbox",
            checked: enabled,
            onChange: (event: Event) => onToggle((event.currentTarget as HTMLInputElement).checked),
            class: "h-4 w-4 accent-blue-500"
          }),
          enabled ? "active" : "inactive"
        ])
      ]),
      enabled && h("div", { class: "grid gap-4 md:grid-cols-2" }, children)
    ]
  );
}

function field(spec: FieldSpec, onInput: (value: string) => void) {
  const id = `profile-config-${spec.key}`;
  const baseClass = clsx(
    "w-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 dark:bg-neutral-950/70 dark:text-neutral-50 dark:ring-white/10",
    spec.kind === "textarea" ? "min-h-28 rounded-[1.25rem] py-4 font-mono text-sm leading-6" : "h-12 rounded-full",
    transition
  );
  const control =
    spec.kind === "textarea"
      ? h("textarea", {
          id,
          value: spec.value,
          placeholder: spec.placeholder,
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
            placeholder: spec.placeholder,
            onInput: (event: Event) => onInput((event.currentTarget as HTMLInputElement).value),
            class: baseClass
          });
  return h("label", { key: spec.key, class: clsx("grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300", spec.kind === "textarea" && "md:col-span-2") }, [
    h("span", {}, spec.label),
    control
  ]);
}
