const LOCALE_KEY = "wt-ui-locale";

const state = {
  snapshot: null,
  view: "tasks",
  locale: initialLocale(),
};

const tabs = Array.from(document.querySelectorAll(".tabs button"));
const content = document.querySelector("#content");
const metrics = document.querySelector("#metrics");
const repoLabel = document.querySelector("#repo-label");
const refreshButton = document.querySelector("#refresh");
const languageButton = document.querySelector("#language-toggle");
const statusRegion = document.querySelector("#status");

const STRINGS = {
  en: {
    eyebrow: "Read-only local inventory",
    refresh: "Refresh",
    languageToggle: "한국어",
    readFull: "Read full",
    collapse: "Collapse",
    body: "Body",
    source: "Source",
    sourceToml: "Source TOML",
    effectiveConfig: "Effective config",
    sourceConfig: "Source config files",
    runtime: "Runtime",
    workspace: "Workspace",
    valid: "valid",
    invalid: "invalid",
    record: "record",
    records: "records",
    tabOverview: "Overview",
    tabIdeas: "Ideas",
    tabTasks: "TaskDocuments",
    tabWorkflows: "Workflows",
    tabTaskRuns: "TaskRuns",
    tabProfiles: "Profiles",
    tabConfig: "Config",
    noteOverview: "Action-focused snapshot across runnable workflows, running TaskRuns, and invalid records.",
    noteIdeas: "Ideas are planning notes from .local/ideas.",
    noteTasks: "TaskDocuments are saved work definitions from .local/tasks.",
    noteWorkflows: "Workflows are saved execution plans from .local/workflows.",
    noteTaskRuns: "TaskRuns are execution records from .local/task-runs.",
    noteProfiles: "Profiles are effective agent/config overlays from .local/profiles.",
    noteConfig: "Config shows the rendered effective config plus the source .wt.toml layers.",
    runnableWorkflows: "Runnable workflows",
    runningTaskRuns: "Running TaskRuns",
    invalidRecords: "Invalid records",
    ideas: "Ideas",
    invalidIdeas: "Invalid ideas",
    taskDocuments: "TaskDocuments",
    invalidTaskDocuments: "Invalid TaskDocuments",
    workflows: "Workflows",
    invalidWorkflows: "Invalid workflows",
    taskRuns: "TaskRuns",
    invalidTaskRuns: "Invalid TaskRuns",
    profiles: "Profiles",
    invalidProfiles: "Invalid profiles",
    config: "Config",
    noRunnableWorkflows: "No runnable workflows",
    noRunningTaskRuns: "No running TaskRuns",
    noInvalidRecords: "No invalid records",
    noIdeas: "No ideas",
    noInvalidIdeas: "No invalid ideas",
    noTaskDocuments: "No TaskDocuments",
    noInvalidTaskDocuments: "No invalid TaskDocuments",
    noInvalidWorkflows: "No invalid workflows",
    noInvalidTaskRuns: "No invalid TaskRuns",
    noProfiles: "No profiles",
    noInvalidProfiles: "No invalid profiles",
    noConfigSummary: "No config summary",
    noSourceConfig: "No source config files",
    snapshotUnavailable: "Snapshot unavailable",
    loading: "Loading",
    loadingSnapshot: "Loading snapshot",
    rendered: "{view} rendered",
  },
  ko: {
    eyebrow: "읽기 전용 로컬 인벤토리",
    refresh: "새로고침",
    languageToggle: "English",
    readFull: "전문 보기",
    collapse: "접기",
    body: "본문",
    source: "원본",
    sourceToml: "원본 TOML",
    effectiveConfig: "적용 설정 (Effective config)",
    sourceConfig: "설정 원본 파일",
    runtime: "실행 환경",
    workspace: "워크스페이스",
    valid: "정상",
    invalid: "오류",
    record: "개",
    records: "개",
    tabOverview: "개요",
    tabIdeas: "아이디어",
    tabTasks: "작업문서",
    tabWorkflows: "워크플로",
    tabTaskRuns: "실행기록",
    tabProfiles: "프로필",
    tabConfig: "설정",
    noteOverview: "실행 가능한 Workflow, 실행 중인 TaskRun, 오류 기록을 우선 보여줍니다.",
    noteIdeas: "Idea는 .local/ideas에 저장된 기획 노트입니다.",
    noteTasks: "TaskDocument는 .local/tasks에 저장된 작업 정의입니다.",
    noteWorkflows: "Workflow는 .local/workflows에 저장된 실행 계획입니다.",
    noteTaskRuns: "TaskRun은 .local/task-runs에 저장된 실행 기록입니다.",
    noteProfiles: "Profile은 .local/profiles의 agent/config overlay입니다.",
    noteConfig: "Config는 렌더링된 effective config와 source .wt.toml 계층을 함께 보여줍니다.",
    runnableWorkflows: "실행 가능한 Workflow",
    runningTaskRuns: "실행 중인 TaskRun",
    invalidRecords: "오류 기록",
    ideas: "아이디어 (Ideas)",
    invalidIdeas: "오류 아이디어",
    taskDocuments: "작업문서 (TaskDocuments)",
    invalidTaskDocuments: "오류 작업문서",
    workflows: "워크플로 (Workflows)",
    invalidWorkflows: "오류 Workflow",
    taskRuns: "실행기록 (TaskRuns)",
    invalidTaskRuns: "오류 TaskRun",
    profiles: "프로필 (Profiles)",
    invalidProfiles: "오류 Profile",
    config: "설정 (Config)",
    noRunnableWorkflows: "실행 가능한 Workflow가 없습니다",
    noRunningTaskRuns: "실행 중인 TaskRun이 없습니다",
    noInvalidRecords: "오류 기록이 없습니다",
    noIdeas: "Idea가 없습니다",
    noInvalidIdeas: "오류 Idea가 없습니다",
    noTaskDocuments: "TaskDocument가 없습니다",
    noInvalidTaskDocuments: "오류 TaskDocument가 없습니다",
    noInvalidWorkflows: "오류 Workflow가 없습니다",
    noInvalidTaskRuns: "오류 TaskRun이 없습니다",
    noProfiles: "Profile이 없습니다",
    noInvalidProfiles: "오류 Profile이 없습니다",
    noConfigSummary: "설정 요약이 없습니다",
    noSourceConfig: "설정 원본 파일이 없습니다",
    snapshotUnavailable: "Snapshot을 불러올 수 없습니다",
    loading: "불러오는 중",
    loadingSnapshot: "Snapshot 불러오는 중",
    rendered: "{view} 렌더링 완료",
  },
};

tabs.forEach((button) => {
  button.addEventListener("click", () => activateTab(button, { scroll: true }));
  button.addEventListener("keydown", handleTabKeydown);
});

refreshButton.addEventListener("click", loadSnapshot);
languageButton.addEventListener("click", () => {
  state.locale = state.locale === "ko" ? "en" : "ko";
  localStorage.setItem(LOCALE_KEY, state.locale);
  applyLocale();
  render();
});

content.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }
  const button = target.closest("[data-collapse]");
  if (!button) {
    return;
  }
  const details = button.closest("details");
  if (!details) {
    return;
  }
  details.open = false;
  const summary = details.querySelector("summary");
  if (summary) {
    summary.focus();
    summary.scrollIntoView({ block: "nearest" });
  }
});

applyLocale();
loadSnapshot();

function initialLocale() {
  const stored = localStorage.getItem(LOCALE_KEY);
  if (stored === "ko" || stored === "en") {
    return stored;
  }
  return navigator.language && navigator.language.toLowerCase().startsWith("ko") ? "ko" : "en";
}

function t(key) {
  return STRINGS[state.locale][key] || STRINGS.en[key] || key;
}

function tr(key, values) {
  return Object.entries(values).reduce(
    (text, [name, value]) => text.replaceAll(`{${name}}`, String(value)),
    t(key)
  );
}

function applyLocale() {
  document.documentElement.lang = state.locale;
  document.querySelector(".eyebrow").textContent = t("eyebrow");
  refreshButton.textContent = t("refresh");
  languageButton.textContent = t("languageToggle");
  languageButton.setAttribute("aria-pressed", state.locale === "ko" ? "true" : "false");
  tabs.forEach((tab) => {
    tab.textContent = t(tabLabelKey(tab.dataset.view));
  });
}

function tabLabelKey(view) {
  if (view === "overview") return "tabOverview";
  if (view === "ideas") return "tabIdeas";
  if (view === "tasks") return "tabTasks";
  if (view === "workflows") return "tabWorkflows";
  if (view === "task-runs") return "tabTaskRuns";
  if (view === "profiles") return "tabProfiles";
  if (view === "config") return "tabConfig";
  return view;
}

function handleTabKeydown(event) {
  const currentIndex = tabs.indexOf(event.currentTarget);
  if (currentIndex < 0) {
    return;
  }
  let nextIndex = currentIndex;
  if (event.key === "ArrowRight") {
    nextIndex = (currentIndex + 1) % tabs.length;
  } else if (event.key === "ArrowLeft") {
    nextIndex = (currentIndex - 1 + tabs.length) % tabs.length;
  } else if (event.key === "Home") {
    nextIndex = 0;
  } else if (event.key === "End") {
    nextIndex = tabs.length - 1;
  } else {
    return;
  }
  event.preventDefault();
  activateTab(tabs[nextIndex], { focus: true, scroll: true });
}

function activateTab(button, options = {}) {
  state.view = button.dataset.view;
  tabs.forEach((tab) => {
    const active = tab === button;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", active ? "true" : "false");
    tab.tabIndex = active ? 0 : -1;
  });
  content.setAttribute("aria-labelledby", button.id);
  render();
  if (options.focus) {
    button.focus();
  }
  if (options.scroll && window.matchMedia("(max-width: 680px)").matches) {
    content.scrollIntoView({ block: "start" });
  }
}

async function loadSnapshot() {
  content.innerHTML = `<div class="loading">${escapeHtml(t("loading"))}</div>`;
  setStatus(t("loadingSnapshot"));
  refreshButton.disabled = true;
  try {
    const response = await fetch("/api/snapshot", { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    state.snapshot = await response.json();
    render();
  } catch (error) {
    content.innerHTML = `<div class="invalid-card"><h3>${escapeHtml(t("snapshotUnavailable"))}</h3><p class="error">${escapeHtml(error.message)}</p></div>`;
    setStatus(t("snapshotUnavailable"));
  } finally {
    refreshButton.disabled = false;
  }
}

function render() {
  if (!state.snapshot) {
    return;
  }
  const snapshot = state.snapshot;
  repoLabel.textContent = `${snapshot.repo.name} - ${snapshot.repo.root}`;
  renderMetrics(snapshot);
  metrics.dataset.view = state.view;

  const view = state.view;
  if (view === "overview") renderOverview(snapshot);
  if (view === "ideas") renderIdeas(snapshot);
  if (view === "tasks") renderTasks(snapshot);
  if (view === "workflows") renderWorkflows(snapshot);
  if (view === "task-runs") renderTaskRuns(snapshot);
  if (view === "profiles") renderProfiles(snapshot);
  if (view === "config") renderConfig(snapshot);
  setStatus(tr("rendered", { view: viewLabel(view) }));
}

function renderMetrics(snapshot) {
  const rows = [
    ["Ideas", snapshot.ideas.items.length, snapshot.ideas.invalid.length],
    ["TaskDocuments", snapshot.tasks.items.length, snapshot.tasks.invalid.length],
    ["Workflows", snapshot.workflows.items.length, snapshot.workflows.invalid.length],
    ["TaskRuns", snapshot.task_runs.items.length, snapshot.task_runs.invalid.length],
    ["Profiles", snapshot.profiles.items.length, snapshot.profiles.invalid.length],
  ];
  metrics.innerHTML = rows
    .map(([label, count, invalid]) => {
      const stateText = invalid ? `${invalid} ${t("invalid")}` : t("valid");
      const className = invalid ? "metric invalid" : "metric";
      return `<div class="${className}"><span class="metric-kicker">${escapeHtml(label)}</span><strong>${count}</strong><span class="metric-state">${escapeHtml(stateText)}</span></div>`;
    })
    .join("");
}

function renderOverview(snapshot) {
  const runnable = snapshot.workflows.items.filter((row) => row.presentation_group === "runnable");
  const running = snapshot.task_runs.items.filter((row) => row.status === "running");
  const invalid = [
    ...snapshot.ideas.invalid,
    ...snapshot.tasks.invalid,
    ...snapshot.workflows.invalid,
    ...snapshot.task_runs.invalid,
    ...snapshot.profiles.invalid,
  ];
  content.innerHTML = [
    section(
      t("runnableWorkflows"),
      runnable.map(workflowCard),
      t("noRunnableWorkflows"),
      t("noteOverview")
    ),
    section(t("runningTaskRuns"), running.map(taskRunCard), t("noRunningTaskRuns")),
    section(t("invalidRecords"), invalid.map(invalidCard), t("noInvalidRecords")),
  ].join("");
}

function renderIdeas(snapshot) {
  content.innerHTML = [
    section(t("ideas"), snapshot.ideas.items.map(ideaCard), t("noIdeas"), t("noteIdeas")),
    section(t("invalidIdeas"), snapshot.ideas.invalid.map(invalidCard), t("noInvalidIdeas")),
  ].join("");
}

function renderTasks(snapshot) {
  content.innerHTML = [
    section(t("taskDocuments"), snapshot.tasks.items.map(taskCard), t("noTaskDocuments"), t("noteTasks")),
    section(t("invalidTaskDocuments"), snapshot.tasks.invalid.map(invalidCard), t("noInvalidTaskDocuments")),
  ].join("");
}

function renderWorkflows(snapshot) {
  const groups = ["state_error", "runnable", "waiting", "done"];
  content.innerHTML = [
    section(t("invalidWorkflows"), snapshot.workflows.invalid.map(invalidCard), t("noInvalidWorkflows"), t("noteWorkflows")),
  ]
    .concat(groups.map((group) => {
      const rows = snapshot.workflows.items.filter((row) => row.presentation_group === group);
      return section(label(group), rows.map(workflowCard), `No ${label(group).toLowerCase()} workflows`);
    }))
    .join("");
}

function renderTaskRuns(snapshot) {
  const statuses = ["failed", "running", "prepared", "skipped", "done"];
  content.innerHTML = [
    section(t("invalidTaskRuns"), snapshot.task_runs.invalid.map(invalidCard), t("noInvalidTaskRuns"), t("noteTaskRuns")),
  ]
    .concat(statuses.map((status) => {
      const rows = snapshot.task_runs.items.filter((row) => row.status === status);
      return section(label(status), rows.map(taskRunCard), `No ${status} TaskRuns`);
    }))
    .join("");
}

function renderProfiles(snapshot) {
  content.innerHTML = [
    section(t("profiles"), snapshot.profiles.items.map(profileCard), t("noProfiles"), t("noteProfiles")),
    section(t("invalidProfiles"), snapshot.profiles.invalid.map(invalidCard), t("noInvalidProfiles")),
  ].join("");
}

function renderConfig(snapshot) {
  const config = snapshot.config;
  const cards = [
    card(t("effectiveConfig"), [
      pill(config.source, "blue"),
      pill(`Pull requests: ${config.workflow.pull_request}`, "green"),
      pill(`Landing: ${config.workflow.landing}`, "amber"),
      config.selected_profile ? pill(`Profile: ${config.selected_profile}`, "violet") : "",
    ], config.paths, bodyPreview(config.effective_text), "blue", [
      detail(t("effectiveConfig"), config.effective_text, "source"),
    ]),
    card(t("runtime"), [
      pill(`Agent: ${config.agent || "none"}`),
      pill(`Issues: ${config.issues || "none"}`),
      pill(`Site: ${config.site ? config.site.provider : "none"}`),
    ], [], "", "green"),
  ];
  if (config.workspace) {
    cards.push(card(t("workspace"), [
      pill(`Workspace tabs: ${config.workspace.tab_count}`),
      pill(`Post-deps tabs: ${config.workspace.post_deps_tab_count}`),
      pill(`Colors: ${config.workspace.color_count}`),
    ], [], "", "violet"));
  }
  content.innerHTML = [
    section(t("config"), cards, t("noConfigSummary"), t("noteConfig")),
    section(t("sourceConfig"), (config.source_files || []).map(sourceFileCard), t("noSourceConfig")),
  ].join("");
}

function ideaCard(row) {
  return card(row.title, [
    pill(row.status || "unspecified", statusColor(row.status)),
    pill(row.kind),
    row.source ? pill(row.source) : "",
    ...row.tags.map((tag) => pill(tag, "violet")),
  ], [row.path], row.body_summary, statusColor(row.status), [
    detail(t("body"), row.body, "prose", row.body_summary),
    detail(t("source"), row.source_text, "source"),
  ]);
}

function taskCard(row) {
  return card(row.title, [
    pill(`task ${row.key}`, "blue"),
    row.branch ? pill(`branch ${row.branch}`) : "",
    row.origin ? pill(`${row.origin.provider} ${row.origin.id}`, "violet") : pill(row.source),
  ], [row.path], row.body_summary, "blue", [
    detail(t("body"), row.body, "prose", row.body_summary),
    detail(t("sourceToml"), row.source_text, "source"),
  ]);
}

function workflowCard(row) {
  return card(row.title || row.id, [
    pill(`workflow ${row.id}`, "blue"),
    pill(row.mode, "blue"),
    pill(row.presentation_group, groupColor(row.presentation_group)),
    pill(row.task_runs.total ? `${row.task_runs.total} runs` : "0 runs"),
    row.runnable.runnable_count ? pill(`${row.runnable.runnable_count} runnable`, "green") : "",
    row.profile ? pill(`profile ${row.profile}`, "violet") : "",
    row.profiles.length ? pill(`${row.profiles.length} profiles`, "violet") : "",
    pill(`${row.policy.pull_request}/${row.policy.landing}`, "amber"),
  ], [row.path], row.body_summary || row.state_error, groupColor(row.presentation_group), [
    detail(t("body"), row.body || row.state_error, "prose", row.body_summary || row.state_error),
    detail(t("sourceToml"), row.source_text, "source"),
  ]);
}

function taskRunCard(row) {
  return card(row.id, [
    pill(row.status, statusColor(row.status)),
    pill(`task ${row.task}`, "blue"),
    pill(`branch ${row.branch}`),
    row.context.workflow_id ? pill(`workflow ${row.context.workflow_id}`, "violet") : "",
    row.context.mode ? pill(`mode ${row.context.mode}`, "violet") : "",
    row.group ? pill(`group ${row.group}`, "violet") : pill(row.context.label || "direct"),
    row.context.error ? pill("context error", "red") : "",
  ], [row.path, row.context.workflow_path].filter(Boolean), row.error || row.context.error || row.branch, statusColor(row.status), [
    detail(t("sourceToml"), row.source_text, "source"),
  ]);
}

function profileCard(row) {
  return card(row.name, [
    pill(`agent ${row.agent}`, "blue"),
    pill(`${row.copy_count} copy`),
    pill(`${row.link_count} link`),
    row.has_site ? pill("site", "green") : "",
    row.test_count ? pill(`${row.test_count} tests`, "amber") : "",
  ], [row.path], bodyPreview(row.source_text), "violet", [
    detail(t("sourceToml"), row.source_text, "source"),
  ]);
}

function invalidCard(row) {
  const detailHtml = row.source_text
    ? readableText(row.source_text, bodyPreview(row.source_text), `${row.key} ${t("source")}`, "source", t("source"))
    : "";
  return `<article class="record-card invalid-card"><div class="record-primary"><h3>${escapeHtml(row.key)}</h3><p class="error">${escapeHtml(row.error)}</p>${detailHtml}</div><div class="record-aside">${paths([row.path])}</div></article>`;
}

function sourceFileCard(row) {
  return card(row.path, [
    pill(t("source"), "blue"),
  ], [row.path], bodyPreview(row.text), "blue", [
    detail(t("source"), row.text, "source"),
  ]);
}

function card(title, pills, pathRows, summary, tone, details) {
  const meta = pills.filter(Boolean).join("");
  const pathHtml = paths(pathRows);
  const detailHtml = normalizeDetails(details, summary)
    .map((row) => readableText(row.text, row.summary, `${title} ${row.label}`, row.kind, row.label))
    .join("");
  const summaryHtml = detailHtml || (summary ? `<p class="summary">${escapeHtml(summary)}</p>` : "");
  const desktopMetaHtml = meta ? `<div class="meta desktop-meta">${meta}</div>` : "";
  const mobileMetaHtml = meta ? `<div class="meta mobile-meta">${meta}</div>` : "";
  const toneClass = tone ? ` tone-${tone}` : "";
  return `<article class="record-card${toneClass}"><div class="record-primary"><h3>${escapeHtml(title)}</h3>${mobileMetaHtml}${summaryHtml}</div><div class="record-aside">${desktopMetaHtml}${pathHtml}</div></article>`;
}

function section(title, rows, emptyText, note = "") {
  const body = rows.length ? `<div class="record-list">${rows.join("")}</div>` : `<div class="empty">${escapeHtml(emptyText)}</div>`;
  const count = rows.length === 1 ? `1 ${t("record")}` : `${rows.length} ${t("records")}`;
  const noteHtml = note ? `<p class="section-note">${escapeHtml(note)}</p>` : "";
  return `<section class="section-block"><div class="section-heading"><div><h2 class="section-title">${escapeHtml(title)}</h2>${noteHtml}</div><span class="section-count">${count}</span></div>${body}</section>`;
}

function detail(label, text, kind = "prose", summary = null) {
  return text ? { label, text, kind, summary } : null;
}

function normalizeDetails(details, summary) {
  if (Array.isArray(details)) {
    return details.filter(Boolean);
  }
  if (!details) {
    return [];
  }
  return [detail(t("body"), details, "prose", summary)].filter(Boolean);
}

function bodyPreview(text) {
  return text ? compactText(text, 190) : "";
}

function readableText(fullText, fallbackPreview, contextLabel, kind = "prose", labelText = t("body")) {
  if (!fullText) {
    return "";
  }
  const text = String(fullText).trim();
  if (!text) {
    return "";
  }
  const preview = fallbackPreview || compactText(text, 190);
  const labelHtml = `<span class="detail-label">${escapeHtml(labelText)}</span>`;
  if (text.length <= 220 && !text.includes("\n")) {
    return `<div class="summary-block">${labelHtml}<p class="summary">${escapeHtml(text)}</p></div>`;
  }
  const context = contextLabel ? `: ${contextLabel}` : "";
  return `<details class="read-more"><summary>${labelHtml}<span class="summary-preview">${escapeHtml(preview)}</span><span class="summary-action"><span class="when-closed" lang="${state.locale}">${escapeHtml(t("readFull"))}<span class="sr-only">${escapeHtml(context)}</span></span><span class="when-open" lang="${state.locale}">${escapeHtml(t("collapse"))}<span class="sr-only">${escapeHtml(context)}</span></span></span></summary><div class="full-text">${formatFullText(text, kind)}</div><button type="button" class="collapse-inline" data-collapse lang="${state.locale}">${escapeHtml(t("collapse"))}</button></details>`;
}

function formatFullText(text, kind) {
  if (kind === "source") {
    return `<pre>${escapeHtml(text)}</pre>`;
  }
  return formatBodyText(text);
}

function formatBodyText(text) {
  const blocks = String(text)
    .trim()
    .split(/\n{2,}/)
    .map((block) => block.trim())
    .filter(Boolean);
  return blocks.map((block) => `<p>${escapeHtml(block)}</p>`).join("");
}

function compactText(value, maxChars) {
  const compact = String(value).split(/\s+/).filter(Boolean).join(" ");
  const chars = Array.from(compact);
  if (chars.length <= maxChars) {
    return compact;
  }
  return `${chars.slice(0, maxChars).join("")}...`;
}

function paths(rows) {
  if (!rows.length) {
    return "";
  }
  return `<div class="path-list">${rows.map((path) => `<p class="path">${escapeHtml(path)}</p>`).join("")}</div>`;
}

function pill(text, tone = "") {
  return `<span class="pill ${tone}">${escapeHtml(String(text))}</span>`;
}

function statusColor(status) {
  if (status === "prepared" || status === "ready" || status === "running") return "green";
  if (status === "failed" || status === "state_error") return "red";
  if (status === "skipped" || status === "waiting") return "amber";
  if (status === "done") return "blue";
  return "";
}

function groupColor(group) {
  if (group === "runnable") return "green";
  if (group === "state_error") return "red";
  if (group === "waiting") return "amber";
  if (group === "done") return "blue";
  return "";
}

function label(value) {
  return value
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function viewLabel(value) {
  const tab = tabs.find((button) => button.dataset.view === value);
  return tab ? tab.textContent.trim() : label(value);
}

function setStatus(message) {
  if (statusRegion) {
    statusRegion.textContent = message;
  }
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}
