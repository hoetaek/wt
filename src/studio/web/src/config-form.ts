import { h, type ComponentChildren } from "preact";
import clsx from "clsx";

export type ConfigDraft = {
  preserved: {
    sectionLines: Record<string, string[]>;
    chunks: string[];
  };
  worktree: {
    enabled: boolean;
    path: string;
    copy: string;
    link: string;
    injectLocalContext: string;
  };
  setup: {
    enabled: boolean;
    env: string;
  };
  workflow: {
    enabled: boolean;
    pullRequest: string;
    landing: string;
  };
  profile: {
    enabled: boolean;
    name: string;
  };
  site: {
    enabled: boolean;
    provider: string;
    name: string;
    root: string;
    secure: string;
    url: string;
    target: string;
  };
  editor: {
    enabled: boolean;
    command: string;
    placement: string;
  };
  workspace: {
    enabled: boolean;
    tabs: string;
    postDepsTabs: string;
    browserMode: string;
    browserUrl: string;
    browserApp: string;
    chromePort: string;
    chromeUserDataDir: string;
  };
  agent: {
    enabled: boolean;
    cli: string;
    args: string;
    command: string;
    ready: string;
    submit: string;
    timeout: string;
    sendAfter: string;
  };
  test: {
    enabled: boolean;
    commands: string;
  };
  issues: {
    enabled: boolean;
    provider: string;
    ghUser: string;
  };
};

type ConfigTable = Record<string, Record<string, unknown>>;
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

export function emptyConfigDraft(): ConfigDraft {
  return {
    preserved: { sectionLines: {}, chunks: [] },
    worktree: { enabled: false, path: "", copy: "", link: "", injectLocalContext: "" },
    setup: { enabled: false, env: "" },
    workflow: { enabled: false, pullRequest: "none", landing: "manual" },
    profile: { enabled: false, name: "" },
    site: { enabled: false, provider: "none", name: "", root: "", secure: "", url: "", target: "" },
    editor: { enabled: false, command: "", placement: "" },
    workspace: {
      enabled: false,
      tabs: "",
      postDepsTabs: "",
      browserMode: "",
      browserUrl: "",
      browserApp: "",
      chromePort: "",
      chromeUserDataDir: ""
    },
    agent: {
      enabled: false,
      cli: "none",
      args: "",
      command: "",
      ready: "",
      submit: "",
      timeout: "",
      sendAfter: ""
    },
    test: { enabled: false, commands: "" },
    issues: { enabled: false, provider: "github", ghUser: "" }
  };
}

export function draftFromToml(toml: string): ConfigDraft {
  const draft = emptyConfigDraft();
  const parsed = parseTomlDocument(toml);
  const tables = parsed.tables;
  draft.preserved = parsed.preserved;

  if (tables.worktree) {
    draft.worktree.enabled = true;
    draft.worktree.path = stringValue(tables.worktree.path);
    draft.worktree.copy = listValue(tables.worktree.copy);
    draft.worktree.link = listValue(tables.worktree.link);
    draft.worktree.injectLocalContext = stringValue(tables.worktree.inject_local_context);
  }
  if (tables.setup) {
    draft.setup.enabled = true;
    draft.setup.env = mapValue(tables.setup.env);
  }
  if (tables.workflow) {
    draft.workflow.enabled = true;
    draft.workflow.pullRequest = stringValue(tables.workflow.pull_request) || "none";
    draft.workflow.landing = stringValue(tables.workflow.landing) || "manual";
  }
  if (tables.profile) {
    draft.profile.enabled = true;
    draft.profile.name = stringValue(tables.profile.name);
  }
  if (tables.site) {
    draft.site.enabled = true;
    draft.site.provider = stringValue(tables.site.provider) || "none";
    draft.site.name = stringValue(tables.site.name);
    draft.site.root = stringValue(tables.site.root);
    draft.site.secure = stringValue(tables.site.secure);
    draft.site.url = stringValue(tables.site.url);
    draft.site.target = stringValue(tables.site.target);
  }
  if (tables.editor) {
    draft.editor.enabled = true;
    draft.editor.command = stringValue(tables.editor.command);
    draft.editor.placement = stringValue(tables.editor.placement);
  }
  if (tables.workspace) {
    draft.workspace.enabled = true;
    draft.workspace.tabs = listValue(tables.workspace.tabs);
    draft.workspace.postDepsTabs = listValue(tables.workspace.post_deps_tabs);
  }
  if (tables["workspace.browser"]) {
    draft.workspace.enabled = true;
    draft.workspace.browserMode = stringValue(tables["workspace.browser"].mode);
    draft.workspace.browserUrl = stringValue(tables["workspace.browser"].url);
    draft.workspace.browserApp = stringValue(tables["workspace.browser"].app);
  }
  if (tables["workspace.chrome_devtools"]) {
    draft.workspace.enabled = true;
    draft.workspace.chromePort = stringValue(tables["workspace.chrome_devtools"].port);
    draft.workspace.chromeUserDataDir = stringValue(tables["workspace.chrome_devtools"].user_data_dir);
  }
  if (tables.agent) {
    draft.agent.enabled = true;
    draft.agent.cli = stringValue(tables.agent.cli) || "none";
    draft.agent.args = listValue(tables.agent.args);
    draft.agent.command = stringValue(tables.agent.command);
    draft.agent.ready = stringValue(tables.agent.ready);
    draft.agent.submit = stringValue(tables.agent.submit);
    draft.agent.timeout = stringValue(tables.agent.timeout);
    draft.agent.sendAfter = stringValue(tables.agent.send_after);
  }
  if (tables.test) {
    draft.test.enabled = true;
    draft.test.commands = listValue(tables.test.commands);
  }
  if (tables.issues) {
    draft.issues.enabled = true;
    draft.issues.provider = stringValue(tables.issues.provider) || "github";
    draft.issues.ghUser = stringValue(tables.issues.gh_user);
  }

  return draft;
}

export function serializeConfigDraft(draft: ConfigDraft): string {
  const chunks: string[] = [];

  if (draft.worktree.enabled) {
    const lines = tableLines("worktree", [
      stringLine("path", draft.worktree.path),
      arrayLine("copy", linesValue(draft.worktree.copy)),
      arrayLine("link", linesValue(draft.worktree.link)),
      stringLine("inject_local_context", draft.worktree.injectLocalContext)
    ], draft.preserved.sectionLines.worktree);
    pushChunk(chunks, lines);
  }
  if (draft.setup.enabled) {
    pushChunk(chunks, tableLines("setup", [inlineMapLine("env", draft.setup.env)], draft.preserved.sectionLines.setup));
  }
  if (draft.workflow.enabled) {
    pushChunk(chunks, tableLines("workflow", [
      stringLine("pull_request", draft.workflow.pullRequest),
      stringLine("landing", draft.workflow.landing)
    ], draft.preserved.sectionLines.workflow));
  }
  if (draft.profile.enabled) {
    pushChunk(chunks, tableLines("profile", [stringLine("name", draft.profile.name)], draft.preserved.sectionLines.profile));
  }
  if (draft.site.enabled) {
    pushChunk(chunks, tableLines("site", [
      stringLine("provider", draft.site.provider),
      stringLine("name", draft.site.name),
      stringLine("root", draft.site.root),
      booleanLine("secure", draft.site.secure),
      stringLine("url", draft.site.url),
      stringLine("target", draft.site.target)
    ], draft.preserved.sectionLines.site));
  }
  if (draft.editor.enabled) {
    pushChunk(chunks, tableLines("editor", [
      stringLine("command", draft.editor.command),
      stringLine("placement", draft.editor.placement)
    ], draft.preserved.sectionLines.editor));
  }
  if (draft.workspace.enabled) {
    pushChunk(chunks, tableLines("workspace", [
      arrayLine("tabs", linesValue(draft.workspace.tabs)),
      arrayLine("post_deps_tabs", linesValue(draft.workspace.postDepsTabs))
    ], draft.preserved.sectionLines.workspace));
    if (draft.workspace.browserMode || draft.workspace.browserUrl || draft.workspace.browserApp) {
      pushChunk(chunks, tableLines("workspace.browser", [
        stringLine("mode", draft.workspace.browserMode),
        stringLine("url", draft.workspace.browserUrl),
        stringLine("app", draft.workspace.browserApp)
      ], draft.preserved.sectionLines["workspace.browser"]));
    }
    if (draft.workspace.chromePort || draft.workspace.chromeUserDataDir) {
      pushChunk(chunks, tableLines("workspace.chrome_devtools", [
        numberLine("port", draft.workspace.chromePort),
        stringLine("user_data_dir", draft.workspace.chromeUserDataDir)
      ], draft.preserved.sectionLines["workspace.chrome_devtools"]));
    }
  }
  if (draft.agent.enabled) {
    pushChunk(chunks, tableLines("agent", [
      stringLine("cli", draft.agent.cli),
      arrayLine("args", linesValue(draft.agent.args)),
      stringLine("command", draft.agent.command),
      stringLine("ready", draft.agent.ready),
      stringLine("submit", draft.agent.submit),
      numberLine("timeout", draft.agent.timeout),
      numberLine("send_after", draft.agent.sendAfter)
    ], draft.preserved.sectionLines.agent));
  }
  if (draft.test.enabled) {
    const commands = linesValue(draft.test.commands);
    pushChunk(
      chunks,
      commands.length > 0
        ? ["[test]", ...(draft.preserved.sectionLines.test || []), ...commands.flatMap((command) => ["", "[[test.commands]]", `run = ${quoteToml(command)}`])]
        : tableLines("test", [], draft.preserved.sectionLines.test)
    );
  }
  if (draft.issues.enabled) {
    pushChunk(chunks, tableLines("issues", [
      stringLine("provider", draft.issues.provider),
      stringLine("gh_user", draft.issues.ghUser)
    ], draft.preserved.sectionLines.issues));
  }
  chunks.push(...draft.preserved.chunks);

  return chunks.length > 0 ? `${chunks.join("\n\n")}\n` : "";
}

export function configDraftEqual(left: ConfigDraft, right: ConfigDraft) {
  return JSON.stringify(left) === JSON.stringify(right);
}

export function configSummary(draft: ConfigDraft) {
  const enabled = configSectionNames.filter((name) => draft[name].enabled);
  return enabled.length > 0 ? enabled.join(", ") : "빈 local.toml";
}

const configSectionNames = [
  "worktree",
  "setup",
  "workflow",
  "profile",
  "site",
  "editor",
  "workspace",
  "agent",
  "test",
  "issues"
] as const;

export function ConfigForm(props: {
  draft: ConfigDraft;
  onChange: (draft: ConfigDraft) => void;
  iconComponent: IconComponent;
}) {
  const updateSection = <K extends keyof ConfigDraft>(section: K, patch: Partial<ConfigDraft[K]>) => {
    props.onChange({
      ...props.draft,
      [section]: { ...props.draft[section], ...patch }
    });
  };

  return h("div", { class: "grid gap-4" }, [
    sectionPanel("workflow", "워크플로우", props.draft.workflow.enabled, props.iconComponent, (enabled) => updateSection("workflow", { enabled }), [
      field({ key: "pullRequest", label: "PR", value: props.draft.workflow.pullRequest, kind: "select", options: ["none", "draft", "ready"] }, (value) =>
        updateSection("workflow", { pullRequest: value })
      ),
      field({ key: "landing", label: "랜딩", value: props.draft.workflow.landing, kind: "select", options: ["manual", "auto"] }, (value) =>
        updateSection("workflow", { landing: value })
      )
    ]),
    sectionPanel("agent", "에이전트", props.draft.agent.enabled, props.iconComponent, (enabled) => updateSection("agent", { enabled }), [
      field({ key: "cli", label: "CLI", value: props.draft.agent.cli, kind: "select", options: ["none", "codex", "claude", "gemini"] }, (value) =>
        updateSection("agent", { cli: value })
      ),
      field({ key: "args", label: "인자", value: props.draft.agent.args, placeholder: "한 줄에 하나씩" }, (value) => updateSection("agent", { args: value })),
      field({ key: "command", label: "명령", value: props.draft.agent.command }, (value) => updateSection("agent", { command: value })),
      field({ key: "ready", label: "준비 신호", value: props.draft.agent.ready }, (value) => updateSection("agent", { ready: value })),
      field({ key: "submit", label: "제출 방식", value: props.draft.agent.submit, kind: "select", options: ["", "auto", "newline", "carriage_return", "none"] }, (value) =>
        updateSection("agent", { submit: value })
      ),
      field({ key: "timeout", label: "제한 시간", value: props.draft.agent.timeout }, (value) => updateSection("agent", { timeout: value })),
      field({ key: "sendAfter", label: "전송 대기", value: props.draft.agent.sendAfter }, (value) => updateSection("agent", { sendAfter: value }))
    ]),
    sectionPanel("workspace", "워크스페이스", props.draft.workspace.enabled, props.iconComponent, (enabled) => updateSection("workspace", { enabled }), [
      field({ key: "tabs", label: "탭", value: props.draft.workspace.tabs, kind: "textarea", placeholder: "한 줄에 하나씩" }, (value) =>
        updateSection("workspace", { tabs: value })
      ),
      field({ key: "postDepsTabs", label: "의존성 후 탭", value: props.draft.workspace.postDepsTabs, kind: "textarea", placeholder: "한 줄에 하나씩" }, (value) =>
        updateSection("workspace", { postDepsTabs: value })
      ),
      field({ key: "browserMode", label: "브라우저 모드", value: props.draft.workspace.browserMode, kind: "select", options: ["", "none", "system", "chrome_devtools"] }, (value) =>
        updateSection("workspace", { browserMode: value })
      ),
      field({ key: "browserUrl", label: "브라우저 URL", value: props.draft.workspace.browserUrl }, (value) => updateSection("workspace", { browserUrl: value })),
      field({ key: "browserApp", label: "브라우저 앱", value: props.draft.workspace.browserApp }, (value) => updateSection("workspace", { browserApp: value })),
      field({ key: "chromePort", label: "Chrome 포트", value: props.draft.workspace.chromePort }, (value) => updateSection("workspace", { chromePort: value })),
      field({ key: "chromeUserDataDir", label: "Chrome 사용자 데이터 디렉터리", value: props.draft.workspace.chromeUserDataDir }, (value) =>
        updateSection("workspace", { chromeUserDataDir: value })
      )
    ]),
    sectionPanel("site", "사이트", props.draft.site.enabled, props.iconComponent, (enabled) => updateSection("site", { enabled }), [
      field({ key: "provider", label: "제공자", value: props.draft.site.provider, kind: "select", options: ["none", "herd", "valet", "docker_proxy", "traefik"] }, (value) =>
        updateSection("site", { provider: value })
      ),
      field({ key: "name", label: "이름", value: props.draft.site.name }, (value) => updateSection("site", { name: value })),
      field({ key: "root", label: "루트", value: props.draft.site.root }, (value) => updateSection("site", { root: value })),
      field({ key: "secure", label: "보안 연결", value: props.draft.site.secure, kind: "select", options: ["", "true", "false"] }, (value) => updateSection("site", { secure: value })),
      field({ key: "url", label: "URL", value: props.draft.site.url }, (value) => updateSection("site", { url: value })),
      field({ key: "target", label: "대상", value: props.draft.site.target }, (value) => updateSection("site", { target: value }))
    ]),
    sectionPanel("worktree", "워크트리", props.draft.worktree.enabled, props.iconComponent, (enabled) => updateSection("worktree", { enabled }), [
      field({ key: "path", label: "경로", value: props.draft.worktree.path }, (value) => updateSection("worktree", { path: value })),
      field({ key: "copy", label: "복사", value: props.draft.worktree.copy, kind: "textarea", placeholder: "한 줄에 하나씩" }, (value) => updateSection("worktree", { copy: value })),
      field({ key: "link", label: "링크", value: props.draft.worktree.link, kind: "textarea", placeholder: "한 줄에 하나씩" }, (value) => updateSection("worktree", { link: value })),
      field({ key: "injectLocalContext", label: "로컬 컨텍스트 주입", value: props.draft.worktree.injectLocalContext }, (value) =>
        updateSection("worktree", { injectLocalContext: value })
      )
    ]),
    sectionPanel("editor", "에디터", props.draft.editor.enabled, props.iconComponent, (enabled) => updateSection("editor", { enabled }), [
      field({ key: "command", label: "명령", value: props.draft.editor.command }, (value) => updateSection("editor", { command: value })),
      field({ key: "placement", label: "배치", value: props.draft.editor.placement, kind: "select", options: ["", "cmux_surface", "process"] }, (value) =>
        updateSection("editor", { placement: value })
      )
    ]),
    sectionPanel("setup", "셋업", props.draft.setup.enabled, props.iconComponent, (enabled) => updateSection("setup", { enabled }), [
      field({ key: "env", label: "Env", value: props.draft.setup.env, kind: "textarea", placeholder: "KEY=value" }, (value) => updateSection("setup", { env: value }))
    ]),
    sectionPanel("profile", "프로필", props.draft.profile.enabled, props.iconComponent, (enabled) => updateSection("profile", { enabled }), [
      field({ key: "name", label: "이름", value: props.draft.profile.name }, (value) => updateSection("profile", { name: value }))
    ]),
    sectionPanel("issues", "이슈", props.draft.issues.enabled, props.iconComponent, (enabled) => updateSection("issues", { enabled }), [
      field({ key: "provider", label: "제공자", value: props.draft.issues.provider, kind: "select", options: ["linear", "github"] }, (value) => updateSection("issues", { provider: value })),
      field({ key: "ghUser", label: "GitHub 사용자", value: props.draft.issues.ghUser }, (value) => updateSection("issues", { ghUser: value }))
    ]),
    sectionPanel("test", "테스트", props.draft.test.enabled, props.iconComponent, (enabled) => updateSection("test", { enabled }), [
      field({ key: "commands", label: "명령", value: props.draft.test.commands, kind: "textarea", placeholder: "간단한 검증용 명령 라벨" }, (value) =>
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
          enabled ? "활성" : "비활성"
        ])
      ]),
      enabled && h("div", { class: "grid gap-4 md:grid-cols-2" }, children)
    ]
  );
}

function field(spec: FieldSpec, onInput: (value: string) => void) {
  const id = `personal-config-${spec.key}`;
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
            spec.options?.map((option) => h("option", { key: option, value: option }, option || "미설정"))
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

function parseTomlDocument(toml: string): {
  tables: ConfigTable;
  preserved: ConfigDraft["preserved"];
} {
  const tables: ConfigTable = {};
  const preserved: ConfigDraft["preserved"] = { sectionLines: {}, chunks: [] };
  for (const chunk of splitTomlChunks(toml)) {
    const header = chunk[0]?.trim() || "";
    const tableMatch = header.match(/^\[([A-Za-z0-9_.-]+)\]$/);
    const section = tableMatch?.[1] || "";
    if (!section || !modeledKeys[section]) {
      const text = chunk.join("\n").trim();
      if (text) preserved.chunks.push(text);
      continue;
    }

    tables[section] ||= {};
    const extraLines: string[] = [];
    for (let index = 1; index < chunk.length; index += 1) {
      const rawLine = chunk[index];
      const line = stripComment(rawLine).trim();
      const keyMatch = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
      if (!keyMatch || !modeledKeys[section].has(keyMatch[1]) || shouldPreserveMultilineValue(keyMatch[2])) {
        extraLines.push(rawLine);
        if (keyMatch && shouldPreserveMultilineValue(keyMatch[2])) {
          const terminator = keyMatch[2].trim().startsWith("[") ? "]" : keyMatch[2].trim().slice(0, 3);
          while (index + 1 < chunk.length && !chunk[index].trim().endsWith(terminator)) {
            index += 1;
            extraLines.push(chunk[index]);
          }
        }
        continue;
      }
      tables[section][keyMatch[1]] = parseTomlValue(keyMatch[2].trim());
    }
    if (extraLines.some((line) => line.trim())) {
      preserved.sectionLines[section] = extraLines;
    }
  }
  return { tables, preserved };
}

const modeledKeys: Record<string, Set<string>> = {
  worktree: new Set(["path", "copy", "link", "inject_local_context"]),
  setup: new Set(["env"]),
  workflow: new Set(["pull_request", "landing"]),
  profile: new Set(["name"]),
  site: new Set(["provider", "name", "root", "secure", "url", "target"]),
  editor: new Set(["command", "placement"]),
  workspace: new Set(["tabs", "post_deps_tabs"]),
  "workspace.browser": new Set(["mode", "url", "app"]),
  "workspace.chrome_devtools": new Set(["port", "user_data_dir"]),
  agent: new Set(["cli", "args", "command", "ready", "submit", "timeout", "send_after"]),
  test: new Set([]),
  issues: new Set(["provider", "gh_user"])
};

function splitTomlChunks(toml: string) {
  const chunks: string[][] = [];
  let current: string[] = [];
  for (const line of toml.split(/\r?\n/)) {
    if (/^\s*\[/.test(line) && current.length > 0) {
      chunks.push(current);
      current = [line];
    } else {
      current.push(line);
    }
  }
  if (current.length > 0) {
    chunks.push(current);
  }
  return chunks;
}

function shouldPreserveMultilineValue(value: string) {
  const trimmed = value.trim();
  if (trimmed.startsWith("\"\"\"") || trimmed.startsWith("'''")) {
    return !trimmed.endsWith(trimmed.slice(0, 3)) || trimmed.length === 3;
  }
  return trimmed.startsWith("[") && !trimmed.endsWith("]");
}

function parseTomlValue(value: string): unknown {
  if (value.startsWith("[") && value.endsWith("]")) {
    return value
      .slice(1, -1)
      .split(",")
      .map((item) => unquote(item.trim()))
      .filter(Boolean);
  }
  if (value.startsWith("{") && value.endsWith("}")) {
    const result: Record<string, string> = {};
    for (const part of value.slice(1, -1).split(",")) {
      const [key, raw] = part.split("=").map((piece) => piece.trim());
      if (key && raw) result[key] = unquote(raw);
    }
    return result;
  }
  if (value === "true" || value === "false") return value;
  if (/^\d+$/.test(value)) return value;
  return unquote(value);
}

function stripComment(line: string) {
  let inString = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === "\"" && line[index - 1] !== "\\") inString = !inString;
    if (char === "#" && !inString) return line.slice(0, index);
  }
  return line;
}

function stringValue(value: unknown) {
  return typeof value === "string" ? value : "";
}

function listValue(value: unknown) {
  return Array.isArray(value) ? value.filter((item) => typeof item === "string").join("\n") : "";
}

function mapValue(value: unknown) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return "";
  return Object.entries(value as Record<string, unknown>)
    .map(([key, mapItem]) => `${key}=${String(mapItem)}`)
    .join("\n");
}

function linesValue(value: string) {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function pushChunk(chunks: string[], lines: string[]) {
  if (lines.length > 1) chunks.push(lines.join("\n"));
}

function tableLines(name: string, lines: Array<string | null>, preservedLines: string[] = []) {
  return [`[${name}]`, ...lines.filter((line): line is string => Boolean(line)), ...preservedLines.filter((line) => line.trim())];
}

function stringLine(key: string, value: string) {
  const trimmed = value.trim();
  return trimmed ? `${key} = ${quoteToml(trimmed)}` : null;
}

function numberLine(key: string, value: string) {
  const trimmed = value.trim();
  return trimmed ? `${key} = ${trimmed}` : null;
}

function booleanLine(key: string, value: string) {
  const trimmed = value.trim();
  return trimmed ? `${key} = ${trimmed}` : null;
}

function arrayLine(key: string, value: string[]) {
  return value.length > 0 ? `${key} = [${value.map(quoteToml).join(", ")}]` : null;
}

function inlineMapLine(key: string, value: string) {
  const pairs = linesValue(value)
    .map((line) => line.split("="))
    .filter((pair) => pair.length >= 2 && pair[0].trim())
    .map(([mapKey, ...rest]) => `${mapKey.trim()} = ${quoteToml(rest.join("=").trim())}`);
  return pairs.length > 0 ? `${key} = { ${pairs.join(", ")} }` : null;
}

function quoteToml(value: string) {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, "\\\"")}"`;
}

function unquote(value: string) {
  const trimmed = value.trim();
  if (trimmed.startsWith("\"") && trimmed.endsWith("\"")) {
    return trimmed.slice(1, -1).replace(/\\"/g, "\"").replace(/\\\\/g, "\\");
  }
  return trimmed;
}
