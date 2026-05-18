const LOCALE_KEY = "wt-ui-locale";

const state = {
  snapshot: null,
  view: "overview",
  locale: initialLocale(),
};

const tabs = Array.from(document.querySelectorAll(".tabs button"));
const content = document.querySelector("#content");
const metrics = document.querySelector("#metrics");
const repoLabel = document.querySelector("#repo-label");
const languageButton = document.querySelector("#language-toggle");
const statusRegion = document.querySelector("#status");

const STRINGS = {
  en: {
    eyebrow: "Read-only local inventory",
    switchToKorean: "Switch language to Korean",
    switchToEnglish: "Switch language to English",
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
    tabConfig: "Config",
    tabWorkflows: "Workflow",
    tabTaskRuns: "TaskRuns",
    tabIdeas: "Ideas",
    tabRetrospecs: "Retrospecs",
    noteOverview: "Action-focused snapshot across local state, runnable workflows, running TaskRuns, and invalid records.",
    noteIdeas: "Ideas are planning notes from .local/ideas.",
    noteRetrospecs: "Retrospecs are completed-work reflections from .local/retrospectives.",
    noteWorkflows: "Workflow rows are grouped by derived state and show linked TaskRuns inside each plan.",
    noteTaskRuns: "TaskRuns are execution records from .local/task-runs with linked TaskDocument content.",
    noteProfiles: "Profiles are effective agent/config overlays from .local/profiles.",
    noteConfig: "Config shows effective config, source .wt.toml layers, and profiles.",
    runnableWorkflows: "Runnable workflows",
    runningTaskRuns: "Running TaskRuns",
    localState: "Local state",
    currentWork: "Current work",
    inventory: "Inventory",
    runnableWorkflowCount: "runnable workflows",
    runningRunCount: "running TaskRuns",
    failedRunCount: "failed TaskRuns",
    invalidRecords: "Invalid records",
    ideas: "Ideas",
    invalidIdeas: "Invalid ideas",
    retrospecs: "Retrospecs",
    invalidRetrospecs: "Invalid Retrospecs",
    linkedTaskDocument: "Linked TaskDocument",
    taskDocumentToml: "TaskDocument TOML",
    taskRunToml: "TaskRun TOML",
    workflowTaskRuns: "Workflow TaskRuns",
    unlinkedTaskDocuments: "TaskDocuments without TaskRuns",
    invalidTaskDocuments: "Invalid TaskDocuments",
    workflows: "Workflow",
    invalidWorkflows: "Invalid workflows",
    taskRuns: "TaskRuns",
    invalidTaskRuns: "Invalid TaskRuns",
    profiles: "Profiles",
    invalidProfiles: "Invalid profiles",
    config: "Config",
    noRunnableWorkflows: "No runnable workflows",
    noRunningTaskRuns: "No running TaskRuns",
    noLocalState: "No local state records",
    noInvalidRecords: "No invalid records",
    noIdeas: "No ideas",
    noInvalidIdeas: "No invalid ideas",
    noRetrospecs: "No Retrospecs",
    noInvalidRetrospecs: "No invalid Retrospecs",
    noUnlinkedTaskDocuments: "Every valid TaskDocument has at least one TaskRun",
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
    switchToKorean: "한국어로 전환",
    switchToEnglish: "영어로 전환",
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
    tabConfig: "설정",
    tabWorkflows: "Workflow",
    tabTaskRuns: "TaskRuns",
    tabIdeas: "아이디어",
    tabRetrospecs: "Retrospecs",
    noteOverview: "로컬 상태 전체와 실행 가능한 Workflow, 실행 중인 TaskRun, 오류 기록을 우선 보여줍니다.",
    noteIdeas: "Idea는 .local/ideas에 저장된 기획 노트입니다.",
    noteRetrospecs: "Retrospecs는 .local/retrospectives에 저장된 완료 작업 회고입니다.",
    noteWorkflows: "Workflow는 파생 상태별로 정렬하고 각 계획 안에 연결된 TaskRun을 묶어 보여줍니다.",
    noteTaskRuns: "TaskRun은 .local/task-runs 실행 기록이며 연결된 TaskDocument 본문을 함께 보여줍니다.",
    noteProfiles: "Profile은 .local/profiles의 agent/config overlay입니다.",
    noteConfig: "Config는 effective config, source .wt.toml 계층, Profile을 함께 보여줍니다.",
    runnableWorkflows: "실행 가능한 Workflow",
    runningTaskRuns: "실행 중인 TaskRun",
    localState: "로컬 상태",
    currentWork: "현재 작업",
    inventory: "인벤토리",
    runnableWorkflowCount: "실행 가능 Workflow",
    runningRunCount: "실행 중 TaskRun",
    failedRunCount: "실패 TaskRun",
    invalidRecords: "오류 기록",
    ideas: "아이디어 (Ideas)",
    invalidIdeas: "오류 아이디어",
    retrospecs: "회고 (Retrospecs)",
    invalidRetrospecs: "오류 Retrospecs",
    linkedTaskDocument: "연결된 TaskDocument",
    taskDocumentToml: "TaskDocument TOML",
    taskRunToml: "TaskRun TOML",
    workflowTaskRuns: "Workflow 안의 TaskRun",
    unlinkedTaskDocuments: "TaskRun이 없는 TaskDocument",
    invalidTaskDocuments: "오류 작업문서",
    workflows: "Workflow",
    invalidWorkflows: "오류 Workflow",
    taskRuns: "실행기록 (TaskRuns)",
    invalidTaskRuns: "오류 TaskRun",
    profiles: "프로필 (Profiles)",
    invalidProfiles: "오류 Profile",
    config: "설정 (Config)",
    noRunnableWorkflows: "실행 가능한 Workflow가 없습니다",
    noRunningTaskRuns: "실행 중인 TaskRun이 없습니다",
    noLocalState: "로컬 상태 기록이 없습니다",
    noInvalidRecords: "오류 기록이 없습니다",
    noIdeas: "Idea가 없습니다",
    noInvalidIdeas: "오류 Idea가 없습니다",
    noRetrospecs: "Retrospecs가 없습니다",
    noInvalidRetrospecs: "오류 Retrospecs가 없습니다",
    noUnlinkedTaskDocuments: "모든 정상 TaskDocument에 TaskRun이 있습니다",
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

languageButton.addEventListener("click", () => {
  state.locale = state.locale === "ko" ? "en" : "ko";
  localStorage.setItem(LOCALE_KEY, state.locale);
  applyLocale();
  render();
});

content.addEventListener("click", handleReadToggle);

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
  languageButton.dataset.current = state.locale;
  languageButton.setAttribute("aria-pressed", state.locale === "ko" ? "true" : "false");
  languageButton.setAttribute(
    "aria-label",
    state.locale === "ko" ? t("switchToEnglish") : t("switchToKorean")
  );
  tabs.forEach((tab) => {
    tab.textContent = t(tabLabelKey(tab.dataset.view));
  });
}

function tabLabelKey(view) {
  if (view === "overview") return "tabOverview";
  if (view === "config") return "tabConfig";
  if (view === "workflows") return "tabWorkflows";
  if (view === "task-runs") return "tabTaskRuns";
  if (view === "ideas") return "tabIdeas";
  if (view === "retrospecs") return "tabRetrospecs";
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

function handleReadToggle(event) {
  const explicitButton = event.target.closest("[data-read-toggle]");
  const preview = event.target.closest(".summary-preview");
  const block = (explicitButton || preview)?.closest(".read-more");
  if (!block || !content.contains(block)) {
    return;
  }
  const button = block.querySelector("[data-read-toggle]");
  const previewText = block.querySelector(".summary-preview");
  const fullText = block.querySelector(".summary-full");
  if (!button || !previewText || !fullText) {
    return;
  }
  const isOpen = !block.classList.contains("is-open");
  block.classList.toggle("is-open", isOpen);
  button.setAttribute("aria-expanded", isOpen ? "true" : "false");
  previewText.hidden = isOpen;
  fullText.hidden = !isOpen;
}

async function loadSnapshot() {
  content.innerHTML = `<div class="loading">${escapeHtml(t("loading"))}</div>`;
  setStatus(t("loadingSnapshot"));
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
  if (view === "config") renderConfig(snapshot);
  if (view === "workflows") renderWorkflows(snapshot);
  if (view === "task-runs") renderTaskRuns(snapshot);
  if (view === "ideas") renderIdeas(snapshot);
  if (view === "retrospecs") renderRetrospecs(snapshot);
  setStatus(tr("rendered", { view: viewLabel(view) }));
}

function renderMetrics(snapshot) {
  const rows = [
    ["Ideas", snapshot.ideas.items.length, snapshot.ideas.invalid.length],
    ["TaskDocuments", snapshot.tasks.items.length, snapshot.tasks.invalid.length],
    ["Workflows", snapshot.workflows.items.length, snapshot.workflows.invalid.length],
    ["TaskRuns", snapshot.task_runs.items.length, snapshot.task_runs.invalid.length],
    ["Profiles", snapshot.profiles.items.length, snapshot.profiles.invalid.length],
    ["Retrospecs", snapshot.retrospecs.items.length, snapshot.retrospecs.invalid.length],
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
  const runnable = sortedWorkflows(snapshot.workflows.items).filter((row) => row.presentation_group === "runnable");
  const running = sortedTaskRuns(snapshot.task_runs.items).filter((row) => row.status === "running");
  const invalid = [
    ...snapshot.ideas.invalid,
    ...snapshot.tasks.invalid,
    ...snapshot.workflows.invalid,
    ...snapshot.task_runs.invalid,
    ...snapshot.profiles.invalid,
    ...snapshot.retrospecs.invalid,
  ];
  content.innerHTML = [
    section(t("localState"), overviewCards(snapshot), t("noLocalState"), t("noteOverview"), "overview-state"),
    section(
      t("runnableWorkflows"),
      runnable.map(workflowCard),
      t("noRunnableWorkflows"),
      "",
      "overview-workflows"
    ),
    section(t("runningTaskRuns"), running.map(taskRunCard), t("noRunningTaskRuns"), "", "overview-task-runs"),
    section(t("invalidRecords"), invalid.map(invalidCard), t("noInvalidRecords"), "", "overview-invalid"),
  ].join("");
}

function overviewCards(snapshot) {
  const sourcePaths = [
    snapshot.sources.tasks,
    snapshot.sources.task_runs,
    snapshot.sources.workflows,
    snapshot.sources.ideas,
    snapshot.sources.profiles,
    snapshot.sources.retrospecs,
  ];
  const counts = [
    `TaskDocuments ${snapshot.tasks.items.length}`,
    `TaskRuns ${snapshot.task_runs.items.length}`,
    `Workflows ${snapshot.workflows.items.length}`,
    `Ideas ${snapshot.ideas.items.length}`,
    `Profiles ${snapshot.profiles.items.length}`,
    `Retrospecs ${snapshot.retrospecs.items.length}`,
  ];
  const invalidCount = [
    snapshot.tasks.invalid,
    snapshot.task_runs.invalid,
    snapshot.workflows.invalid,
    snapshot.ideas.invalid,
    snapshot.profiles.invalid,
    snapshot.retrospecs.invalid,
  ].reduce((total, rows) => total + rows.length, 0);
  const runnableCount = snapshot.workflows.items.filter((row) => row.presentation_group === "runnable").length;
  const runningCount = snapshot.task_runs.items.filter((row) => row.status === "running").length;
  const failedCount = snapshot.task_runs.items.filter((row) => row.status === "failed").length;
  return [
    card(t("currentWork"), [
      pill(`${runnableCount} ${t("runnableWorkflowCount")}`, runnableCount ? "green" : ""),
      pill(`${runningCount} ${t("runningRunCount")}`, runningCount ? "green" : ""),
      pill(`${failedCount} ${t("failedRunCount")}`, failedCount ? "red" : ""),
      invalidCount ? pill(`${invalidCount} invalid`, "red") : pill(t("valid"), "green"),
    ], [snapshot.sources.workflows, snapshot.sources.task_runs], snapshot.repo.root, runningCount || runnableCount ? "green" : "blue"),
    card(t("config"), [
      pill(snapshot.config.source, "blue"),
      pill(`PR ${snapshot.config.workflow.pull_request}`, "green"),
      pill(`Landing ${snapshot.config.workflow.landing}`, "amber"),
      snapshot.config.selected_profile ? pill(`Profile ${snapshot.config.selected_profile}`, "violet") : "",
    ], snapshot.config.paths, bodyPreview(snapshot.config.effective_text), "blue", [
      detail(t("effectiveConfig"), snapshot.config.effective_text, "source"),
    ]),
    card(t("inventory"), counts.map((count) => pill(count)), sourcePaths, t("noteOverview"), "violet"),
  ];
}

function renderIdeas(snapshot) {
  content.innerHTML = [
    section(t("ideas"), snapshot.ideas.items.map(ideaCard), t("noIdeas"), t("noteIdeas"), "ideas-valid"),
    section(t("invalidIdeas"), snapshot.ideas.invalid.map(invalidCard), t("noInvalidIdeas"), "", "ideas-invalid"),
  ].join("");
}

function renderRetrospecs(snapshot) {
  content.innerHTML = [
    section(t("retrospecs"), snapshot.retrospecs.items.map(retrospecCard), t("noRetrospecs"), t("noteRetrospecs"), "retrospecs-valid"),
    section(t("invalidRetrospecs"), snapshot.retrospecs.invalid.map(invalidCard), t("noInvalidRetrospecs"), "", "retrospecs-invalid"),
  ].join("");
}

function renderWorkflows(snapshot) {
  const groups = ["state_error", "runnable", "waiting", "done"];
  const sections = [
    section(t("invalidWorkflows"), snapshot.workflows.invalid.map(invalidCard), t("noInvalidWorkflows"), t("noteWorkflows"), "workflow-invalid"),
  ].concat(groups.map((group) => {
    const rows = sortedWorkflows(snapshot.workflows.items).filter((row) => row.presentation_group === group);
    return section(label(group), rows.map(workflowCard), `No ${label(group).toLowerCase()} workflows`, "", `workflow-${group}`);
  }));
  content.innerHTML = jumpNav([
    ["workflow-invalid", `${t("invalid")} ${snapshot.workflows.invalid.length}`],
    ...groups.map((group) => [`workflow-${group}`, `${label(group)} ${snapshot.workflows.items.filter((row) => row.presentation_group === group).length}`]),
  ]) + sections.join("");
}

function renderTaskRuns(snapshot) {
  const statuses = ["failed", "running", "prepared", "skipped", "done"];
  const linkedTasks = new Set(snapshot.task_runs.items.map((row) => row.task));
  const unlinkedTasks = snapshot.tasks.items.filter((row) => !linkedTasks.has(row.key));
  const sections = [
    section(t("invalidTaskRuns"), snapshot.task_runs.invalid.map(invalidCard), t("noInvalidTaskRuns"), t("noteTaskRuns"), "task-runs-invalid"),
  ].concat(statuses.map((status) => {
    const rows = sortedTaskRuns(snapshot.task_runs.items).filter((row) => row.status === status);
    return section(label(status), rows.map(taskRunCard), `No ${status} TaskRuns`, "", `task-runs-${status}`);
  })).concat([
    section(t("unlinkedTaskDocuments"), unlinkedTasks.map(taskCard), t("noUnlinkedTaskDocuments"), "", "task-runs-unlinked-documents"),
    section(t("invalidTaskDocuments"), snapshot.tasks.invalid.map(invalidCard), t("noInvalidTaskDocuments"), "", "task-runs-invalid-documents"),
  ]);
  content.innerHTML = jumpNav([
    ["task-runs-invalid", `${t("invalid")} ${snapshot.task_runs.invalid.length}`],
    ...statuses.map((status) => [`task-runs-${status}`, `${label(status)} ${snapshot.task_runs.items.filter((row) => row.status === status).length}`]),
    ["task-runs-unlinked-documents", `${t("unlinkedTaskDocuments")} ${unlinkedTasks.length}`],
  ]) + sections.join("");
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
    section(t("config"), cards, t("noConfigSummary"), t("noteConfig"), "config-summary"),
    section(t("sourceConfig"), (config.source_files || []).map(sourceFileCard), t("noSourceConfig"), "", "config-sources"),
    section(t("profiles"), snapshot.profiles.items.map(profileCard), t("noProfiles"), t("noteProfiles"), "config-profiles"),
    section(t("invalidProfiles"), snapshot.profiles.invalid.map(invalidCard), t("noInvalidProfiles"), "", "config-invalid-profiles"),
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
  return card(row.title, [
    pill(`workflow ${row.id}`, "blue"),
    pill(row.mode, "blue"),
    pill(row.presentation_group, groupColor(row.presentation_group)),
    pill(row.task_runs.total ? `${row.task_runs.total} runs` : "0 runs"),
    row.runnable.runnable_count ? pill(`${row.runnable.runnable_count} runnable`, "green") : "",
    row.profile ? pill(`profile ${row.profile}`, "violet") : "",
    row.profiles.length ? pill(`${row.profiles.length} profiles`, "violet") : "",
    row.origin ? pill(`${row.origin.provider} ${row.origin.id}`, "violet") : "",
    pill(`${row.policy.pull_request}/${row.policy.landing}`, "amber"),
  ], [row.path], row.body_summary || row.state_error, groupColor(row.presentation_group), [
    detail(t("body"), row.body || row.state_error, "prose", row.body_summary || row.state_error),
    detail(t("workflowTaskRuns"), formatWorkflowTaskRuns(row.task_run_groups || []), "source"),
    detail(t("sourceToml"), row.source_text, "source"),
  ]);
}

function taskRunCard(row) {
  const taskDocument = row.task_document;
  const taskTitle = taskDocument ? taskDocument.title : row.task;
  return card(row.id, [
    pill(row.status, statusColor(row.status)),
    pill(`task ${row.task}`, "blue"),
    taskDocument ? pill(`document ${taskTitle}`, "blue") : pill("document missing", "red"),
    pill(`branch ${row.branch}`),
    row.context.workflow_id ? pill(`workflow ${row.context.workflow_id}`, "violet") : "",
    row.context.mode ? pill(`mode ${row.context.mode}`, "violet") : "",
    row.group ? pill(`group ${row.group}`, "violet") : pill(row.context.label || "direct"),
    row.context.error ? pill("context error", "red") : "",
  ], [row.path, row.context.workflow_path, taskDocument && taskDocument.path].filter(Boolean), row.error || row.context.error || row.task_document_error || taskDocument?.body_summary || row.branch, statusColor(row.status), [
    detail(t("linkedTaskDocument"), formatLinkedDocument(row), "prose", taskDocument?.body_summary || row.task_document_error),
    detail(t("taskDocumentToml"), taskDocument?.source_text, "source"),
    detail(t("taskRunToml"), row.source_text, "source"),
  ]);
}

function retrospecCard(row) {
  return card(row.title, [
    row.outcome ? pill(row.outcome, statusColor(row.outcome)) : "",
    row.target ? pill(row.target, "blue") : "",
    pill(row.kind),
    row.date ? pill(row.date, "amber") : "",
    ...row.tags.map((tag) => pill(tag, "violet")),
  ], [row.path], row.body_summary, statusColor(row.outcome), [
    detail(t("body"), row.body, "prose", row.body_summary),
    detail(t("source"), row.source_text, "source"),
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

function formatLinkedDocument(row) {
  if (row.task_document_error) {
    return row.task_document_error;
  }
  const document = row.task_document;
  if (!document) {
    return "";
  }
  const lines = [
    `${t("linkedTaskDocument")}: ${document.title}`,
    `task: ${document.key}`,
    document.branch ? `branch: ${document.branch}` : "",
    document.origin ? `origin: ${document.origin.provider} ${document.origin.id}` : "",
  ].filter(Boolean);
  if (document.body) {
    lines.push("", document.body);
  }
  return lines.join("\n");
}

function formatWorkflowTaskRuns(groups) {
  if (!groups.length) {
    return "";
  }
  return groups
    .map((group) => {
      const rows = group.items.map((run) => {
        const title = run.task_document ? ` - ${run.task_document.title}` : "";
        return `- ${run.id} | task ${run.task} | branch ${run.branch}${title}`;
      });
      return `[${group.status}]\n${rows.join("\n")}`;
    })
    .join("\n\n");
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

function section(title, rows, emptyText, note = "", id = "") {
  const body = rows.length ? `<div class="record-list">${rows.join("")}</div>` : `<div class="empty">${escapeHtml(emptyText)}</div>`;
  const count = rows.length === 1 ? `1 ${t("record")}` : `${rows.length} ${t("records")}`;
  const noteHtml = note ? `<p class="section-note">${escapeHtml(note)}</p>` : "";
  const idAttr = id ? ` id="${escapeHtml(id)}"` : "";
  return `<section class="section-block"${idAttr}><div class="section-heading"><div><h2 class="section-title">${escapeHtml(title)}</h2>${noteHtml}</div><span class="section-count">${count}</span></div>${body}</section>`;
}

function jumpNav(items) {
  const links = items
    .map(([id, text]) => `<a href="#${escapeHtml(id)}">${escapeHtml(text)}</a>`)
    .join("");
  return `<nav class="jump-nav" aria-label="Section shortcuts">${links}</nav>`;
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
  return `<div class="read-more">${labelHtml}<div class="summary-text"><div class="summary-preview">${escapeHtml(preview)}</div><div class="summary-full full-text" hidden>${formatFullText(text, kind)}</div></div><button class="summary-action" type="button" data-read-toggle aria-expanded="false"><span class="when-closed" lang="${state.locale}">${escapeHtml(t("readFull"))}<span class="sr-only">${escapeHtml(context)}</span></span><span class="when-open" lang="${state.locale}">${escapeHtml(t("collapse"))}<span class="sr-only">${escapeHtml(context)}</span></span></button></div>`;
}

function formatFullText(text, kind) {
  if (kind === "source") {
    return `<pre>${escapeHtml(text)}</pre>`;
  }
  return `<div class="markdown-body">${formatBodyText(text)}</div>`;
}

function formatBodyText(text) {
  const lines = String(text)
    .trim()
    .split(/\r?\n/);
  let html = "";
  let paragraph = [];
  let listItems = [];
  let orderedList = false;
  let codeLines = [];
  let inCode = false;

  const flushParagraph = () => {
    if (!paragraph.length) {
      return;
    }
    html += `<p>${formatInlineMarkdown(paragraph.join(" "))}</p>`;
    paragraph = [];
  };
  const flushList = () => {
    if (!listItems.length) {
      return;
    }
    const tag = orderedList ? "ol" : "ul";
    html += `<${tag}>${listItems.map((item) => `<li>${formatInlineMarkdown(item)}</li>`).join("")}</${tag}>`;
    listItems = [];
  };
  const flushCode = () => {
    html += `<pre><code>${escapeHtml(codeLines.join("\n"))}</code></pre>`;
    codeLines = [];
  };

  lines.forEach((line) => {
    const trimmed = line.trim();
    if (trimmed.startsWith("```")) {
      if (inCode) {
        flushCode();
        inCode = false;
      } else {
        flushParagraph();
        flushList();
        inCode = true;
      }
      return;
    }
    if (inCode) {
      codeLines.push(line);
      return;
    }
    if (!trimmed) {
      flushParagraph();
      flushList();
      return;
    }

    const heading = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      const level = Math.min(3, heading[1].length);
      html += `<h${level}>${formatInlineMarkdown(heading[2].trim())}</h${level}>`;
      return;
    }

    const unordered = trimmed.match(/^[-*]\s+(.+)$/);
    const ordered = trimmed.match(/^\d+[.)]\s+(.+)$/);
    if (unordered || ordered) {
      flushParagraph();
      const nextOrdered = Boolean(ordered);
      if (listItems.length && orderedList !== nextOrdered) {
        flushList();
      }
      orderedList = nextOrdered;
      listItems.push((ordered || unordered)[1].trim());
      return;
    }

    flushList();
    paragraph.push(trimmed);
  });

  if (inCode) {
    flushCode();
  }
  flushParagraph();
  flushList();
  return html;
}

function formatInlineMarkdown(value) {
  return escapeHtml(value)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
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

function sortedTaskRuns(rows) {
  return [...rows].sort((left, right) => {
    const status = taskRunStatusOrder(left.status) - taskRunStatusOrder(right.status);
    if (status !== 0) return status;
    const order = (right.creation_order || 0) - (left.creation_order || 0);
    if (order !== 0) return order;
    return String(right.updated_at).localeCompare(String(left.updated_at)) || String(left.id).localeCompare(String(right.id));
  });
}

function sortedWorkflows(rows) {
  return [...rows].sort((left, right) => {
    const group = workflowGroupOrder(left.presentation_group) - workflowGroupOrder(right.presentation_group);
    if (group !== 0) return group;
    return String(right.updated_at).localeCompare(String(left.updated_at)) || String(left.id).localeCompare(String(right.id));
  });
}

function taskRunStatusOrder(status) {
  return ["failed", "running", "prepared", "skipped", "done"].indexOf(status) === -1
    ? 99
    : ["failed", "running", "prepared", "skipped", "done"].indexOf(status);
}

function workflowGroupOrder(group) {
  return ["state_error", "runnable", "waiting", "done"].indexOf(group) === -1
    ? 99
    : ["state_error", "runnable", "waiting", "done"].indexOf(group);
}

function statusColor(status) {
  if (status === "prepared" || status === "ready" || status === "running" || status === "landed") return "green";
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
