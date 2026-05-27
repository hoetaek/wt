import {
  ArrowsClockwise,
  CheckCircle,
  FileText,
  FloppyDisk,
  List,
  Plus,
  WarningCircle
} from "@phosphor-icons/react";
import clsx from "clsx";
import { h, render, type ComponentChildren } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import "./style.css";

type TaskOrigin = {
  provider: string;
  id: string;
};

type TaskDocument = {
  title: string;
  branch: string;
  body: string;
  origin?: TaskOrigin | null;
};

type Fingerprint = {
  mtime_ns: string | null;
  hash: string;
};

type TaskDocumentItem = {
  key: string;
  path: string;
  content: string;
  document: TaskDocument;
  fingerprint: Fingerprint;
};

type Inventory = {
  items: TaskDocumentItem[];
  invalid: Array<{ path: string; error: string }>;
};

type PlanResponse = {
  path: string;
  operation: "create" | "update";
  valid: boolean;
  validation_errors: string[];
  before: string;
  after: string;
  diff: string;
  precondition: Fingerprint;
};

type Mode = "create" | "update";
type PlanStatus = "idle" | "planning" | "stale";

type EditorDraft = {
  slug: string;
  title: string;
  branch: string;
  body: string;
  originProvider: string;
  originId: string;
};

type IconComponent = (props: Record<string, unknown>) => ComponentChildren;
type DetailMetric = { label: string; value: string };

const StudioList = List as unknown as IconComponent;
const StudioPlus = Plus as unknown as IconComponent;
const StudioRefresh = ArrowsClockwise as unknown as IconComponent;
const StudioFile = FileText as unknown as IconComponent;
const StudioSave = FloppyDisk as unknown as IconComponent;
const StudioWarning = WarningCircle as unknown as IconComponent;
const StudioCheck = CheckCircle as unknown as IconComponent;
const PLAN_DEBOUNCE_MS = 500;

const emptyDraft: EditorDraft = {
  slug: "new-task",
  title: "",
  branch: "new-task",
  body: "",
  originProvider: "",
  originId: ""
};

const transition =
  "transition-[transform,opacity,background-color,color,box-shadow,filter] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]";
const eyebrow =
  "inline-flex w-fit items-center rounded-full bg-black/[0.04] px-3 py-1 text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-500 ring-1 ring-black/5 dark:bg-white/[0.04] dark:text-neutral-400 dark:ring-white/10";

function App() {
  const [inventory, setInventory] = useState<Inventory>({ items: [], invalid: [] });
  const [mode, setMode] = useState<Mode>("create");
  const [selectedPath, setSelectedPath] = useState("");
  const [drawerOpen, setDrawerOpen] = useState(true);
  const [draft, setDraft] = useState<EditorDraft>(emptyDraft);
  const [plan, setPlan] = useState<PlanResponse | null>(null);
  const [planStatus, setPlanStatus] = useState<PlanStatus>("idle");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const selected = useMemo(
    () => inventory.items.find((item) => item.path === selectedPath) || null,
    [inventory.items, selectedPath]
  );
  const draftIssues = useMemo(() => validateDraft(mode, draft), [draft, mode]);
  const displaySlug = mode === "create" ? draft.slug.trim() || "new-task" : selected?.key || draft.slug;
  const displayTitle = draft.title.trim() || "제목 없는 TaskDocument";
  const currentPath = mode === "update" && selected ? selected.path : targetPath(mode, draft, selected);
  const cleanUpdateDraft = mode === "update" && selected ? draftsEqual(draft, draftFromItem(selected)) : false;
  const planSignature = useMemo(() => planRequestSignature(mode, currentPath, draft), [currentPath, draft, mode]);
  const latestPlanSignature = useRef(planSignature);
  const recoveryPlanController = useRef<AbortController | null>(null);
  latestPlanSignature.current = planSignature;
  const status = statusDescriptor(planStatus, plan);
  const detailMetrics = useMemo(
    () => buildDetailMetrics(currentPath, draft, selected, plan, draftIssues, planStatus),
    [currentPath, draft, selected, plan, draftIssues, planStatus]
  );

  useEffect(() => {
    void loadInventory();
  }, []);

  useEffect(() => {
    return () => {
      recoveryPlanController.current?.abort();
      recoveryPlanController.current = null;
    };
  }, []);

  useEffect(() => {
    if (mode === "update" && !selected) {
      setPlanStatus("idle");
      return;
    }
    if (cleanUpdateDraft || draftIssues.length > 0) {
      setPlanStatus("idle");
      return;
    }

    const controller = new AbortController();
    const signature = planSignature;
    const timer = window.setTimeout(() => {
      if (latestPlanSignature.current !== signature) {
        return;
      }
      recoveryPlanController.current?.abort();
      recoveryPlanController.current = null;
      setPlanStatus("planning");
      setError("");
      void planDraft(controller.signal, signature);
    }, PLAN_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [cleanUpdateDraft, draftIssues.length, mode, planSignature, selected]);

  useEffect(() => {
    const nodes = Array.from(document.querySelectorAll<HTMLElement>("[data-reveal]"));
    if (!("IntersectionObserver" in window)) {
      nodes.forEach((node) => node.setAttribute("data-reveal", "visible"));
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.setAttribute("data-reveal", "visible");
            observer.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.16 }
    );
    nodes.forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [inventory.items.length, plan?.diff, error]);

  async function loadInventory(nextSelectedPath?: string) {
    setBusy(true);
    setError("");
    try {
      const next = await api<Inventory>("/api/task-documents", { method: "POST" });
      setInventory(next);
      const fallbackPath = next.items[0]?.path || "";
      const resolvedPath = nextSelectedPath ?? selectedPath;
      const nextPath = next.items.some((item) => item.path === resolvedPath) ? resolvedPath : fallbackPath;
      setSelectedPath(nextPath);
      if (mode === "update") {
        const nextSelected = next.items.find((item) => item.path === nextPath);
        if (nextSelected) {
          setDraft(draftFromItem(nextSelected));
        }
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  function selectCreate() {
    setMode("create");
    resetPlanState();
    setError("");
    setDraft(emptyDraft);
  }

  function selectUpdate(path: string) {
    const item = inventory.items.find((candidate) => candidate.path === path);
    if (!item) return;
    setMode("update");
    setSelectedPath(path);
    resetPlanState();
    setError("");
    setDraft(draftFromItem(item));
  }

  function resetPlanState() {
    recoveryPlanController.current?.abort();
    recoveryPlanController.current = null;
    setPlan(null);
    setPlanStatus("idle");
  }

  function triggerConflictRecoveryPlan() {
    recoveryPlanController.current?.abort();
    const controller = new AbortController();
    recoveryPlanController.current = controller;
    const signature = latestPlanSignature.current;
    setPlanStatus("planning");
    void planDraft(controller.signal, signature, { failureStatus: "stale" }).finally(() => {
      if (recoveryPlanController.current === controller) {
        recoveryPlanController.current = null;
      }
    });
  }

  async function planDraft(
    signal: AbortSignal,
    signature: string,
    options: { failureStatus?: PlanStatus } = {}
  ) {
    try {
      const response = await api<PlanResponse>("/api/task-documents/plan", {
        method: "POST",
        signal,
        body: JSON.stringify({
          path: targetPath(mode, draft, selected),
          mode,
          document: documentFromDraft(draft)
        })
      });
      if (latestPlanSignature.current !== signature) {
        return;
      }
      setPlan(response);
      setError("");
      setPlanStatus("idle");
    } catch (err) {
      if (isAbortError(err) || latestPlanSignature.current !== signature) {
        return;
      }
      setError(errorMessage(err));
      setPlan(null);
      setPlanStatus(options.failureStatus ?? "idle");
    }
  }

  async function applyPlan() {
    if (!plan || !plan.valid) return;
    setBusy(true);
    setError("");
    try {
      await api("/api/task-documents/apply", {
        method: "POST",
        body: JSON.stringify({
          path: plan.path,
          before: plan.before,
          after: plan.after,
          precondition: plan.precondition
        })
      });
      setPlan(null);
      setPlanStatus("idle");
      setMode("update");
      setSelectedPath(plan.path);
      await loadInventory(plan.path);
    } catch (err) {
      const apiErr = err as ApiFailure;
      setError(apiErr.diff ? `${apiErr.message}\n\n${apiErr.diff}` : errorMessage(err));
      if (apiErr.status === 409) {
        setPlan(null);
        triggerConflictRecoveryPlan();
      }
    } finally {
      setBusy(false);
    }
  }

  function updateDraft(key: keyof EditorDraft, value: string) {
    setDraft((current) => {
      const next = { ...current, [key]: value };
      if (key === "slug" && mode === "create") {
        next.branch = value;
      }
      return next;
    });
    resetPlanState();
  }

  return h("main", { class: "relative min-h-[100dvh] overflow-hidden px-4 py-12 text-neutral-950 dark:text-neutral-50 sm:py-16" }, [
    h("div", { class: "studio-noise", "aria-hidden": "true" }),
    h(
      "header",
      {
        class:
          "sticky top-4 z-20 mx-auto mb-12 flex w-full max-w-7xl items-center justify-between rounded-full bg-white/70 px-3 py-3 shadow-[0_20px_50px_-28px_rgba(0,0,0,0.28)] ring-1 ring-black/5 backdrop-blur-xl dark:bg-neutral-950/70 dark:ring-white/10 md:mb-16",
        "data-reveal": ""
      },
      [
        h("button", {
          type: "button",
          "aria-label": drawerOpen ? "TaskDocument 목록 숨기기" : "TaskDocument 목록 보이기",
          onClick: () => setDrawerOpen((open) => !open),
          class: clsx(
            "group flex h-12 w-12 items-center justify-center rounded-full bg-black/[0.04] text-neutral-700 ring-1 ring-black/5 active:scale-[0.98] dark:bg-white/[0.08] dark:text-neutral-200 dark:ring-white/10",
            transition
          )
        }, icon(StudioList, "h-5 w-5 group-hover:translate-x-1 group-hover:-translate-y-px " + transition)),
        h("div", { class: "text-center" }, [
          h("p", { class: "text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-500 dark:text-neutral-400" }, "wt studio"),
          h("p", { class: "mt-1 text-sm font-medium text-neutral-800 dark:text-neutral-100" }, "TaskDocument 작성")
        ]),
        h("div", { class: "flex items-center gap-2" }, [
          h(IconButton, { label: "새로 만들기", iconComponent: StudioPlus, onClick: selectCreate }),
          h(IconButton, { label: "새로고침", iconComponent: StudioRefresh, onClick: () => void loadInventory(), disabled: busy })
        ])
      ]
    ),
    h("section", { class: "mx-auto grid max-w-7xl gap-10 md:grid-cols-12 md:items-start", "data-reveal": "" }, [
      h("aside", { class: "md:col-span-5" }, [
        h("div", { class: "flex min-h-[42rem] flex-col justify-between gap-12 py-4" }, [
          h("div", { class: "space-y-8" }, [
            h("p", { class: eyebrow }, mode === "create" ? "새 초안" : "선택됨"),
            h("div", {}, [
              h(
                "h1",
                {
                  class:
                    "max-w-[10ch] [overflow-wrap:anywhere] text-[clamp(3rem,6vw,6rem)] font-medium leading-[0.95] tracking-normal text-neutral-950 dark:text-neutral-50"
                },
                displaySlug
              ),
              h("p", { class: "mt-6 max-w-[34ch] text-base leading-7 text-neutral-500 dark:text-neutral-400" }, displayTitle)
            ]),
            h("div", { class: "flex flex-wrap gap-2" }, [
              h(MetaPill, { label: mode === "create" ? "생성 Plan" : "수정 Plan" }),
              h(MetaPill, {
                label: planSummaryLabel(planStatus, plan),
                tone: planStatus === "stale" ? "amber" : planStatus === "planning" || plan?.valid ? "blue" : "neutral"
              }),
              inventory.invalid.length > 0 && h(MetaPill, { label: `오류 ${inventory.invalid.length}개`, tone: "amber" })
            ]),
            h("dl", { class: "grid gap-4 text-sm text-neutral-500 dark:text-neutral-400" }, detailMetrics.map((item) => metric(item.label, item.value)))
          ]),
          drawerOpen &&
            h(Bezel, { className: "animate-[studio-spring_700ms_var(--ease-studio)_both]" }, [
              h("div", { class: "flex items-center justify-between gap-4" }, [
                h("div", {}, [
                  h("p", { class: eyebrow }, "디스크"),
                  h("p", { class: "mt-3 text-2xl font-medium text-neutral-950 dark:text-neutral-50" }, `${inventory.items.length} TaskDocuments`)
                ]),
                h("button", {
                  type: "button",
                  onClick: selectCreate,
                  class: clsx(
                    "group flex h-12 w-12 items-center justify-center rounded-full bg-studio-accent text-white active:scale-[0.98]",
                    transition
                  )
                }, icon(StudioPlus, "h-5 w-5 group-hover:translate-x-1 group-hover:-translate-y-px " + transition))
              ]),
              h(TaskList, { inventory, mode, selectedPath, selectUpdate })
            ])
        ])
      ]),
      h("section", { class: "grid gap-8 md:col-span-7", "aria-label": "TaskDocument editor" }, [
        h(Bezel, {}, [
          h("div", { class: "flex flex-col gap-6 md:flex-row md:items-center md:justify-between" }, [
            h("div", {}, [
              h("p", { class: eyebrow }, "편집기"),
              h("h2", { class: "mt-4 text-3xl font-medium tracking-normal text-neutral-950 dark:text-neutral-50" }, "Apply 전 Plan")
            ]),
            h("span", { "aria-live": "polite", "aria-atomic": "true" }, [
              h("span", { class: statusClass(status.tone, status.pulse) }, status.label)
            ])
          ]),
          h("div", { class: "grid gap-4 md:grid-cols-2" }, [
            h(Field, {
              name: "slug",
              label: "Slug",
              value: draft.slug,
              onInput: (value: string) => updateDraft("slug", value),
              disabled: mode === "update"
            }),
            h(Field, { name: "title", label: "Title", value: draft.title, onInput: (value: string) => updateDraft("title", value) }),
            h(Field, { name: "branch", label: "Branch", value: draft.branch, onInput: (value: string) => updateDraft("branch", value) }),
            h(Field, {
              name: "origin-provider",
              label: "Origin provider",
              value: draft.originProvider,
              onInput: (value: string) => updateDraft("originProvider", value)
            }),
            h(Field, {
              name: "origin-id",
              label: "Origin id",
              value: draft.originId,
              onInput: (value: string) => updateDraft("originId", value),
              className: "md:col-span-2"
            })
          ]),
          h("label", { class: "grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300" }, [
            h("span", {}, "Body"),
            h("textarea", {
              id: "task-document-body",
              name: "body",
              value: draft.body,
              onInput: (event: Event) => updateDraft("body", formValue(event)),
              rows: 13,
              class: clsx(
                "min-h-[22rem] rounded-[1.5rem] bg-white/70 px-5 py-4 font-mono text-sm leading-6 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
                transition
              )
            })
          ]),
          draftIssues.length > 0 && h(ValidationList, { items: draftIssues }),
          h("div", { class: "flex flex-col gap-3 pt-2 sm:flex-row" }, [
            h(ActionButton, {
              label: "Apply",
              iconComponent: StudioSave,
              onClick: applyPlan,
              disabled: busy || planStatus === "planning" || !plan?.valid,
              tone: "primary"
            })
          ])
        ]),
        error && h(MessagePanel, { message: error, tone: "error" }),
        plan && h(PlanPreview, { plan })
      ])
    ])
  ]);
}

function TaskList(props: {
  inventory: Inventory;
  mode: Mode;
  selectedPath: string;
  selectUpdate: (path: string) => void;
}) {
  if (props.inventory.items.length === 0) {
    return h("p", { class: "mt-6 text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, "디스크에 TaskDocument가 없습니다.");
  }
  return h("div", { class: "mt-6 grid max-h-[24rem] gap-2 overflow-auto pr-1" }, [
    ...props.inventory.items.map((item) => {
      const selected = props.mode === "update" && item.path === props.selectedPath;
      return h(
        "button",
        {
          key: item.path,
          type: "button",
          onClick: () => props.selectUpdate(item.path),
          class: clsx(
            "group grid w-full gap-1 rounded-[1.25rem] px-4 py-3 text-left ring-1 active:scale-[0.98]",
            selected
              ? "bg-studio-accent text-white ring-studio-accent"
              : "bg-white/40 text-neutral-700 ring-black/5 hover:bg-white/80 dark:bg-white/[0.03] dark:text-neutral-200 dark:ring-white/10 dark:hover:bg-white/[0.06]",
            transition
          )
        },
        [
          h("span", { class: "truncate text-sm font-medium" }, item.document.title || item.key),
          h("span", { class: clsx("truncate text-xs", selected ? "text-white/70" : "text-neutral-500 dark:text-neutral-500") }, item.document.branch || item.key)
        ]
      );
    }),
    props.inventory.invalid.length > 0 &&
      h("div", { class: "mt-4 rounded-[1.25rem] bg-amber-400/10 p-4 text-sm text-amber-700 ring-1 ring-amber-500/20 dark:text-amber-300" }, [
        h("div", { class: "flex items-center gap-2 font-medium" }, [icon(StudioWarning, "h-4 w-4"), "오류 TaskDocuments"]),
        ...props.inventory.invalid.map((item) =>
          h("p", { key: item.path, class: "mt-2 [overflow-wrap:anywhere] text-xs leading-5" }, `${item.path}: ${item.error}`)
        )
      ])
  ]);
}

function Bezel(props: { children?: ComponentChildren; className?: string; innerClassName?: string }) {
  return h(
    "div",
    {
      class: clsx(
        "rounded-[2rem] bg-black/[0.04] p-1.5 shadow-[0_30px_60px_-20px_rgba(0,0,0,0.08)] ring-1 ring-black/5 dark:bg-white/[0.04] dark:ring-white/10",
        props.className
      )
    },
    h(
      "div",
      {
        class: clsx(
          "grid gap-6 rounded-[calc(2rem-0.375rem)] bg-white p-6 shadow-[inset_0_1px_1px_rgba(255,255,255,0.6)] dark:bg-neutral-950 dark:shadow-[inset_0_1px_1px_rgba(255,255,255,0.04)] sm:p-8",
          props.innerClassName
        )
      },
      props.children
    )
  );
}

function IconButton(props: {
  label: string;
  iconComponent: IconComponent;
  onClick: () => void;
  disabled?: boolean;
}) {
  return h(
    "button",
    {
      type: "button",
      onClick: props.onClick,
      disabled: props.disabled,
      class: clsx(
        "group flex items-center gap-2 rounded-full bg-black/[0.04] py-2 pl-4 pr-2 text-sm font-medium text-neutral-700 ring-1 ring-black/5 active:scale-[0.98] disabled:opacity-45 dark:bg-white/[0.08] dark:text-neutral-200 dark:ring-white/10",
        transition
      )
    },
    [
      h("span", {}, props.label),
      h("span", { class: "grid h-8 w-8 place-items-center rounded-full bg-white/80 dark:bg-white/10" }, icon(props.iconComponent, "h-4 w-4 group-hover:translate-x-1 group-hover:-translate-y-px " + transition))
    ]
  );
}

function ActionButton(props: {
  label: string;
  iconComponent: IconComponent;
  onClick: () => void;
  disabled?: boolean;
  tone?: "primary" | "plain";
}) {
  const primary = props.tone === "primary";
  return h(
    "button",
    {
      type: "button",
      onClick: props.onClick,
      disabled: props.disabled,
      class: clsx(
        "group flex w-full items-center justify-between gap-4 rounded-full px-6 py-3 text-sm font-medium active:scale-[0.98] disabled:opacity-45 sm:w-auto",
        primary
          ? "bg-studio-accent text-white shadow-[0_18px_44px_-22px_rgba(59,130,246,0.9)]"
          : "bg-neutral-950 text-white dark:bg-white dark:text-neutral-950",
        transition
      )
    },
    [
      h("span", {}, props.label),
      h("span", { class: "grid h-8 w-8 place-items-center rounded-full bg-white/18 dark:bg-black/10" }, icon(props.iconComponent, "h-4 w-4 group-hover:translate-x-1 group-hover:-translate-y-px " + transition))
    ]
  );
}

function Field(props: {
  name: string;
  label: string;
  value: string;
  onInput: (value: string) => void;
  disabled?: boolean;
  className?: string;
}) {
  const id = `task-document-${props.name}`;
  return h("label", { class: clsx("grid gap-2 text-sm font-medium text-neutral-600 dark:text-neutral-300", props.className) }, [
    h("span", {}, props.label),
    h("input", {
      id,
      name: props.name,
      value: props.value,
      disabled: props.disabled,
      onInput: (event: Event) => props.onInput(formValue(event)),
      class: clsx(
        "h-12 rounded-full bg-white/70 px-5 text-neutral-950 shadow-[inset_0_1px_1px_rgba(255,255,255,0.75)] ring-1 ring-black/5 placeholder:text-neutral-400 focus:ring-2 focus:ring-studio-accent/50 disabled:text-neutral-500 dark:bg-neutral-900/70 dark:text-neutral-50 dark:ring-white/10",
        transition
      )
    })
  ]);
}

function MetaPill(props: { label: string; tone?: "blue" | "amber" | "neutral" }) {
  return h(
    "span",
    {
      class: clsx(
        "rounded-full px-3 py-1 text-[10px] font-medium uppercase tracking-[0.2em] ring-1",
        props.tone === "blue"
          ? "bg-studio-accent/10 text-studio-accent ring-studio-accent/20"
          : props.tone === "amber"
            ? "bg-amber-400/10 text-amber-700 ring-amber-500/20 dark:text-amber-300"
            : "bg-black/[0.04] text-neutral-500 ring-black/5 dark:bg-white/[0.04] dark:text-neutral-400 dark:ring-white/10"
      )
    },
    props.label
  );
}

function ValidationList(props: { items: string[] }) {
  return h("div", { class: "rounded-[1.25rem] bg-amber-400/10 p-4 text-sm text-amber-700 ring-1 ring-amber-500/20 dark:text-amber-300" }, [
    h("div", { class: "flex items-center gap-2 font-medium" }, [icon(StudioWarning, "h-4 w-4"), "검증"]),
    h("ul", { class: "mt-2 grid gap-1 border-l border-amber-500/30 pl-3" }, props.items.map((item) => h("li", { key: item }, item)))
  ]);
}

function MessagePanel(props: { message: string; tone: "error" }) {
  return h(
    Bezel,
    { className: "animate-[studio-spring_700ms_var(--ease-studio)_both]", innerClassName: "bg-amber-50 dark:bg-neutral-950" },
    [
      h("div", { class: "flex items-center gap-3 text-amber-700 dark:text-amber-300" }, [
        h("span", { class: "grid h-10 w-10 place-items-center rounded-full bg-amber-400/10 ring-1 ring-amber-500/20" }, icon(StudioWarning, "h-5 w-5")),
        h("strong", { class: "font-medium" }, "외부 변경 또는 검증 문제")
      ]),
      h("pre", { class: "overflow-auto whitespace-pre-wrap font-mono text-sm leading-6 text-neutral-700 dark:text-neutral-300" }, props.message)
    ]
  );
}

function PlanPreview(props: { plan: PlanResponse }) {
  const lines = (props.plan.diff || "변경 사항 없음").split("\n");
  return h(Bezel, { className: "animate-[studio-spring_700ms_var(--ease-studio)_both]" }, [
    h("div", { class: "flex flex-col gap-4 md:flex-row md:items-center md:justify-between" }, [
      h("div", {}, [
        h("p", { class: eyebrow }, "Plan 미리보기"),
        h("div", { class: "mt-4 flex items-center gap-3" }, [
          h("span", { class: "grid h-10 w-10 place-items-center rounded-full bg-studio-accent/10 text-studio-accent ring-1 ring-studio-accent/20" }, icon(props.plan.valid ? StudioCheck : StudioWarning, "h-5 w-5")),
          h("strong", { class: "text-lg font-medium text-neutral-950 dark:text-neutral-50" }, props.plan.valid ? "유효한 Plan" : "유효하지 않은 Plan")
        ])
      ]),
      h("p", { class: "max-w-[34ch] [overflow-wrap:anywhere] text-sm leading-6 text-neutral-500 dark:text-neutral-400" }, props.plan.path)
    ]),
    props.plan.validation_errors.length > 0 && h(ValidationList, { items: props.plan.validation_errors }),
    h("div", { class: "overflow-hidden rounded-[1.5rem] bg-neutral-950 text-neutral-100 ring-1 ring-black/10 dark:bg-black dark:ring-white/10" }, [
      h("div", { class: "flex items-center justify-between px-5 py-3 text-[10px] uppercase tracking-[0.2em] text-neutral-400" }, [
        h("span", {}, "Unified diff"),
        h("span", {}, operationLabel(props.plan.operation))
      ]),
      h("pre", { class: "max-h-[30rem] overflow-auto px-0 pb-5 font-mono text-sm leading-6" }, lines.map((line, index) => diffLine(line, index)))
    ])
  ]);
}

function diffLine(line: string, index: number) {
  const tone = line.startsWith("+")
    ? "bg-blue-400/10 text-blue-100"
    : line.startsWith("-")
      ? "bg-amber-400/10 text-amber-100"
      : "text-neutral-300";
  return h(
    "span",
    {
      key: `${index}-${line}`,
      class: clsx("block min-h-6 whitespace-pre px-5 opacity-100 transition-[transform,opacity] duration-700 ease-[cubic-bezier(0.32,0.72,0,1)]", tone),
      style: { transitionDelay: `${Math.min(index * 40, 640)}ms` }
    },
    line || " "
  );
}

function metric(label: string, value: string) {
  return h("div", { key: label }, [
    h("dt", { class: "text-[10px] font-medium uppercase tracking-[0.2em] text-neutral-400 dark:text-neutral-500" }, label),
    h("dd", { class: "mt-1 [overflow-wrap:anywhere] text-neutral-700 dark:text-neutral-300" }, value)
  ]);
}

function buildDetailMetrics(
  currentPath: string,
  draft: EditorDraft,
  selected: TaskDocumentItem | null,
  plan: PlanResponse | null,
  draftIssues: string[],
  planStatus: PlanStatus
): DetailMetric[] {
  const origin = selected?.document.origin || documentFromDraft(draft).origin;
  const metrics: DetailMetric[] = [
    { label: "대상", value: currentPath },
    { label: "Branch", value: draft.branch.trim() || draft.slug.trim() || "task" },
    { label: "Body", value: bodySummary(draft.body) },
    { label: "검증", value: validationSummary(plan, draftIssues, planStatus) }
  ];

  if (origin?.provider || origin?.id) {
    metrics.push({ label: "Origin provider", value: origin.provider || "없음" });
    metrics.push({ label: "Origin id", value: origin.id || "없음" });
    return metrics;
  }

  metrics.push({ label: "Modified", value: formatKoreanMtime(selected?.fingerprint.mtime_ns) });
  metrics.push({ label: "Hash", value: selected?.fingerprint.hash.slice(0, 12) || "새 파일" });
  return metrics;
}

function bodySummary(body: string) {
  const chars = body.length;
  const lines = body.length === 0 ? 0 : body.split(/\r\n|\r|\n/).length;
  return `${chars.toLocaleString("ko-KR")}자 / ${lines.toLocaleString("ko-KR")}줄`;
}

function validationSummary(plan: PlanResponse | null, draftIssues: string[], planStatus: PlanStatus) {
  if (draftIssues.length > 0) {
    return `입력 오류 ${draftIssues.length}개`;
  }
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale") {
    return "Stale";
  }
  if (!plan) {
    return "Plan 대기";
  }
  return plan.valid ? "검증 통과" : `검증 오류 ${plan.validation_errors.length}개`;
}

function planSummaryLabel(planStatus: PlanStatus, plan: PlanResponse | null) {
  if (planStatus === "planning") {
    return "Plan 갱신 중";
  }
  if (planStatus === "stale") {
    return "Stale";
  }
  return plan?.valid ? "유효한 Diff" : "Plan 대기";
}

function formatKoreanMtime(mtimeNs?: string | null) {
  if (!mtimeNs) {
    return "새 파일";
  }
  try {
    const ms = Number(BigInt(mtimeNs) / 1_000_000n);
    return new Intl.DateTimeFormat("ko-KR", {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit"
    }).format(new Date(ms));
  } catch {
    return "수정 시각 없음";
  }
}

function operationLabel(operation: PlanResponse["operation"]) {
  return operation === "create" ? "생성" : "수정";
}

function icon(IconComponent: IconComponent, className: string) {
  return h(IconComponent, { size: 20, weight: "light", className });
}

function draftFromItem(item: TaskDocumentItem): EditorDraft {
  return {
    slug: item.key,
    title: item.document.title || "",
    branch: item.document.branch || "",
    body: item.document.body || "",
    originProvider: item.document.origin?.provider || "",
    originId: item.document.origin?.id || ""
  };
}

function draftsEqual(left: EditorDraft, right: EditorDraft) {
  return (
    left.slug === right.slug &&
    left.title === right.title &&
    left.branch === right.branch &&
    left.body === right.body &&
    left.originProvider === right.originProvider &&
    left.originId === right.originId
  );
}

function documentFromDraft(draft: EditorDraft): TaskDocument {
  const slug = draft.slug.trim() || "task";
  const origin =
    draft.originProvider.trim() || draft.originId.trim()
      ? { provider: draft.originProvider.trim(), id: draft.originId.trim() }
      : null;
  return {
    title: draft.title,
    branch: draft.branch.trim() || slug,
    body: draft.body,
    origin
  };
}

function validateDraft(mode: Mode, draft: EditorDraft) {
  const issues = [];
  if (mode === "create" && !/^[a-z0-9-]+$/.test(draft.slug.trim())) {
    issues.push("Slug는 소문자, 숫자, 하이픈만 사용할 수 있습니다.");
  }
  const titleLength = draft.title.trim().length;
  if (titleLength === 0 || titleLength > 120) {
    issues.push("Title은 1~120자여야 합니다.");
  }
  return issues;
}

function targetPath(mode: Mode, draft: EditorDraft, selected: TaskDocumentItem | null) {
  if (mode === "update" && selected) {
    return selected.path;
  }
  const slug = draft.slug.trim() || "task";
  return slug.endsWith(".toml") ? slug : `${slug}.toml`;
}

function planRequestSignature(mode: Mode, path: string, draft: EditorDraft) {
  return JSON.stringify({
    path,
    mode,
    draft
  });
}

function statusDescriptor(planStatus: PlanStatus, plan: PlanResponse | null) {
  if (planStatus === "stale") {
    return { label: "Stale (재plan 필요)", tone: "amber" as const };
  }
  if (planStatus === "planning") {
    return { label: "Plan 갱신 중", tone: "blue" as const, pulse: true };
  }
  if (plan?.valid) {
    return { label: "적용 가능", tone: "blue" as const };
  }
  return { label: "미저장", tone: "neutral" as const };
}

function statusClass(tone: "blue" | "amber" | "neutral", pulse?: boolean) {
  return clsx(
    "inline-flex w-fit rounded-full px-3 py-1 text-[10px] font-medium uppercase tracking-[0.2em] ring-1",
    pulse && "studio-status-pulse",
    tone === "amber"
      ? "bg-amber-400/10 text-amber-700 ring-amber-500/20 dark:text-amber-300"
      : tone === "blue"
        ? "bg-studio-accent/10 text-studio-accent ring-studio-accent/20"
        : "bg-black/[0.04] text-neutral-500 ring-black/5 dark:bg-white/[0.04] dark:text-neutral-400 dark:ring-white/10"
  );
}

type ApiFailure = Error & {
  status?: number;
  diff?: string;
};

async function api<T = unknown>(path: string, options: RequestInit): Promise<T> {
  const response = await fetch(path, {
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    ...options
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(body.error || `${response.status} ${response.statusText}`) as ApiFailure;
    error.status = response.status;
    error.diff = body.diff;
    throw error;
  }
  return body as T;
}

function errorMessage(err: unknown) {
  return err instanceof Error ? err.message : String(err);
}

function isAbortError(err: unknown) {
  return err instanceof DOMException && err.name === "AbortError";
}

function formValue(event: Event) {
  return (event.currentTarget as HTMLInputElement | HTMLTextAreaElement).value;
}

render(h(App, {}), document.getElementById("app") as HTMLElement);
