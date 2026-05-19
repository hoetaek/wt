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
    metricIdeas: "Ideas",
    metricTaskDocuments: "TaskDocuments",
    metricWorkflows: "Workflows",
    metricTaskRuns: "TaskRuns",
    metricProfiles: "Profiles",
    metricRetrospecs: "Retrospecs",
    tabOverview: "Overview",
    tabConfig: "Config",
    tabWorkflows: "Workflows",
    tabTaskRuns: "TaskRuns",
    tabIdeas: "Ideas",
    tabRetrospecs: "Retrospecs",
    noteOverview: "Action-focused snapshot across local state, prepared workflows, running TaskRuns, and records that need attention.",
    noteIdeas: "Ideas are planning notes from .local/ideas.",
    noteRetrospecs: "Retrospecs are completed-work reflections from .local/retrospectives.",
    noteWorkflows: "Workflows are grouped by derived state and show linked TaskRuns inside each plan.",
    noteTaskRuns: "TaskRuns are execution records from <git-common-dir>/wt/task-runs with linked TaskDocument content. Failed or broken links are grouped under Needs attention.",
    noteProfiles: "Profiles are effective agent/config overlays from <git-common-dir>/wt/profiles.",
    noteConfig: "Config shows effective config, source .wt.toml layers, and profiles.",
    cockpitConfigTitle: "Config cockpit",
    cockpitConfigSubtitle: "Effective config, workflow policy, profile overlays, and source layers.",
    cockpitWorkflowTitle: "Workflow cockpit",
    cockpitWorkflowSubtitle: "Derived Workflow state, runnable work, linked TaskRuns, and source paths.",
    cockpitTaskRunTitle: "TaskRun cockpit",
    cockpitTaskRunSubtitle: "Execution records grouped for comparison by status, branch, context, and source path.",
    cockpitIdeasTitle: "Ideas cockpit",
    cockpitIdeasSubtitle: "Planning records with status, source, tags, and source paths kept close to the title.",
    cockpitRetrospecsTitle: "Retrospecs cockpit",
    cockpitRetrospecsSubtitle: "Completed-work reflections with outcome, target, date, tags, and source paths.",
    workflowIndex: "Workflow index",
    taskRunIndex: "TaskRun index",
    ideaIndex: "Idea index",
    retrospecIndex: "Retrospec index",
    configIndex: "Config index",
    workflowPolicy: "Workflow policy",
    sourceLayers: "Source layers",
    selectedProfile: "Selected profile",
    noSelectedProfile: "No selected profile",
    validProfiles: "valid profiles",
    sourceFiles: "source files",
    statusGroups: "status groups",
    outcomeGroups: "outcome groups",
    taggedRecords: "tagged records",
    focusTitle: "Focus inspector",
    focusSubtitle: "Current work, prepared work, and records needing attention",
    focusInvalid: "Invalid record",
    focusFailedTaskRun: "Failed TaskRun",
    focusFailedLinkedTaskRun: "Failed linked TaskRun",
    focusMissingTaskRun: "Missing TaskRun",
    focusMissingTaskDocument: "Missing TaskDocument",
    focusContextError: "Context error",
    focusTaskDocumentError: "TaskDocument error",
    focusTaskRunError: "TaskRun error",
    focusUnlinkedTaskDocument: "No TaskRun yet",
    focusRunnableWorkflow: "Runnable Workflow",
    focusRunningTaskRun: "Running TaskRun",
    focusSource: "Source",
    focusMore: "{count} more below",
    preparedWorkflows: "Prepared Workflows",
    runningTaskRuns: "Running TaskRuns",
    needsAttention: "Needs attention",
    localState: "Local state",
    currentWork: "Current work",
    inventory: "Inventory",
    preparedWorkflowCount: "prepared Workflows",
    runningRunCount: "running TaskRuns",
    attentionCount: "need attention",
    invalidRecords: "invalid records",
    ideas: "Ideas",
    invalidIdeas: "Invalid ideas",
    retrospecs: "Retrospecs",
    invalidRetrospecs: "Invalid Retrospecs",
    taskRunState: "TaskRun status",
    taskDocumentToml: "TaskDocument TOML",
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
    noPreparedWorkflows: "No prepared Workflows",
    noRunningTaskRuns: "No running TaskRuns",
    noNeedsAttention: "Nothing needs attention",
    noLocalState: "No local state records",
    noInvalidRecords: "No invalid records",
    noWorkflows: "No Workflows",
    noTaskRuns: "No TaskRuns",
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
    statePrepared: "Prepared",
    stateRunning: "Running",
    stateWaiting: "Waiting",
    stateDone: "Done",
    stateSkipped: "Skipped",
    stateFailed: "Failed",
    stateError: "State error",
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
    metricIdeas: "아이디어",
    metricTaskDocuments: "작업문서",
    metricWorkflows: "워크플로우",
    metricTaskRuns: "작업 실행",
    metricProfiles: "프로필",
    metricRetrospecs: "회고",
    tabOverview: "개요",
    tabConfig: "설정",
    tabWorkflows: "워크플로우",
    tabTaskRuns: "작업 실행",
    tabIdeas: "아이디어",
    tabRetrospecs: "회고",
    noteOverview: "로컬 상태 전체와 준비된 워크플로우, 실행 중인 작업, 확인이 필요한 항목을 우선 보여줍니다.",
    noteIdeas: "Idea는 .local/ideas에 저장된 기획 노트입니다.",
    noteRetrospecs: "회고는 .local/retrospectives에 저장된 완료 작업 기록입니다.",
    noteWorkflows: "워크플로우는 파생 상태별로 정렬하고 각 계획 안에 연결된 작업 실행을 묶어 보여줍니다.",
    noteTaskRuns: "작업 실행은 <git-common-dir>/wt/task-runs 실행 기록입니다. 실패했거나 연결이 깨진 항목은 확인 필요로 묶습니다.",
    noteProfiles: "프로필은 <git-common-dir>/wt/profiles의 agent/config overlay입니다.",
    noteConfig: "설정은 effective config, source .wt.toml 계층, 프로필을 함께 보여줍니다.",
    cockpitConfigTitle: "설정 현황",
    cockpitConfigSubtitle: "effective config, 워크플로우 정책, 프로필 overlay, 설정 원본을 먼저 보여줍니다.",
    cockpitWorkflowTitle: "워크플로우 현황",
    cockpitWorkflowSubtitle: "파생 Workflow 상태, 실행 가능한 작업, 연결된 TaskRun, 원본 경로를 비교합니다.",
    cockpitTaskRunTitle: "TaskRun 현황",
    cockpitTaskRunSubtitle: "실행 기록을 상태, branch, context, 원본 경로 기준으로 비교합니다.",
    cockpitIdeasTitle: "아이디어 현황",
    cockpitIdeasSubtitle: "기획 기록의 상태, 원본, 태그, source path를 제목 가까이에 둡니다.",
    cockpitRetrospecsTitle: "회고 현황",
    cockpitRetrospecsSubtitle: "완료 작업 기록의 결과, 대상, 날짜, 태그, source path를 먼저 보여줍니다.",
    workflowIndex: "워크플로우 색인",
    taskRunIndex: "TaskRun 색인",
    ideaIndex: "아이디어 색인",
    retrospecIndex: "회고 색인",
    configIndex: "설정 색인",
    workflowPolicy: "워크플로우 정책",
    sourceLayers: "설정 원본",
    selectedProfile: "선택된 프로필",
    noSelectedProfile: "선택된 프로필 없음",
    validProfiles: "정상 프로필",
    sourceFiles: "원본 파일",
    statusGroups: "상태 그룹",
    outcomeGroups: "결과 그룹",
    taggedRecords: "태그 있는 기록",
    focusTitle: "중점 확인",
    focusSubtitle: "현재 작업, 준비된 작업, 확인이 필요한 기록",
    focusInvalid: "오류 기록",
    focusFailedTaskRun: "실패한 TaskRun",
    focusFailedLinkedTaskRun: "실패한 연결 TaskRun",
    focusMissingTaskRun: "누락된 TaskRun",
    focusMissingTaskDocument: "누락된 TaskDocument",
    focusContextError: "컨텍스트 오류",
    focusTaskDocumentError: "TaskDocument 오류",
    focusTaskRunError: "TaskRun 오류",
    focusUnlinkedTaskDocument: "아직 TaskRun 없음",
    focusRunnableWorkflow: "실행 가능한 Workflow",
    focusRunningTaskRun: "실행 중인 TaskRun",
    focusSource: "원본",
    focusMore: "아래 {count}개 더",
    preparedWorkflows: "준비된 워크플로우",
    runningTaskRuns: "실행 중인 작업",
    needsAttention: "확인 필요",
    localState: "로컬 상태",
    currentWork: "현재 작업",
    inventory: "인벤토리",
    preparedWorkflowCount: "준비된 워크플로우",
    runningRunCount: "실행 중인 작업",
    attentionCount: "확인 필요",
    invalidRecords: "오류 기록",
    ideas: "아이디어",
    invalidIdeas: "오류 아이디어",
    retrospecs: "회고",
    invalidRetrospecs: "오류 회고",
    taskRunState: "TaskRun 상태",
    taskDocumentToml: "TaskDocument TOML",
    workflowTaskRuns: "워크플로우의 작업 실행",
    unlinkedTaskDocuments: "TaskRun이 없는 TaskDocument",
    invalidTaskDocuments: "오류 작업문서",
    workflows: "워크플로우",
    invalidWorkflows: "오류 워크플로우",
    taskRuns: "작업 실행",
    invalidTaskRuns: "오류 작업 실행",
    profiles: "프로필",
    invalidProfiles: "오류 프로필",
    config: "설정",
    noRunnableWorkflows: "실행 가능한 워크플로우가 없습니다",
    noPreparedWorkflows: "준비된 워크플로우가 없습니다",
    noRunningTaskRuns: "실행 중인 작업이 없습니다",
    noNeedsAttention: "확인할 항목이 없습니다",
    noLocalState: "로컬 상태 기록이 없습니다",
    noInvalidRecords: "오류 기록이 없습니다",
    noWorkflows: "워크플로우가 없습니다",
    noTaskRuns: "작업 실행이 없습니다",
    noIdeas: "Idea가 없습니다",
    noInvalidIdeas: "오류 Idea가 없습니다",
    noRetrospecs: "회고가 없습니다",
    noInvalidRetrospecs: "오류 회고가 없습니다",
    noUnlinkedTaskDocuments: "모든 정상 TaskDocument에 TaskRun이 있습니다",
    noInvalidTaskDocuments: "오류 TaskDocument가 없습니다",
    noInvalidWorkflows: "오류 워크플로우가 없습니다",
    noInvalidTaskRuns: "오류 작업 실행이 없습니다",
    noProfiles: "프로필이 없습니다",
    noInvalidProfiles: "오류 프로필이 없습니다",
    noConfigSummary: "설정 요약이 없습니다",
    noSourceConfig: "설정 원본 파일이 없습니다",
    snapshotUnavailable: "Snapshot을 불러올 수 없습니다",
    loading: "불러오는 중",
    loadingSnapshot: "Snapshot 불러오는 중",
    rendered: "{view} 렌더링 완료",
    statePrepared: "준비됨",
    stateRunning: "실행 중",
    stateWaiting: "대기",
    stateDone: "완료",
    stateSkipped: "건너뜀",
    stateFailed: "실패",
    stateError: "상태 오류",
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
    [t("metricIdeas"), snapshot.ideas.items.length, snapshot.ideas.invalid.length],
    [t("metricTaskDocuments"), snapshot.tasks.items.length, snapshot.tasks.invalid.length],
    [t("metricWorkflows"), snapshot.workflows.items.length, snapshot.workflows.invalid.length],
    [t("metricTaskRuns"), snapshot.task_runs.items.length, snapshot.task_runs.invalid.length],
    [t("metricProfiles"), snapshot.profiles.items.length, snapshot.profiles.invalid.length],
    [t("metricRetrospecs"), snapshot.retrospecs.items.length, snapshot.retrospecs.invalid.length],
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
  const focus = overviewFocusModel(snapshot);
  content.innerHTML = [
    focusPanel(focus),
    optionalSection(t("needsAttention"), focus.attention.map(focusScanRow), "", "overview-attention"),
    optionalSection(t("runningTaskRuns"), focus.running.map(runningFocusItem).map(focusScanRow), "", "overview-task-runs"),
    optionalSection(
      t("preparedWorkflows"),
      focus.prepared.map(preparedWorkflowFocusItem).map(focusScanRow),
      "",
      "overview-workflows"
    ),
  ].join("");
}

function renderIdeas(snapshot) {
  content.innerHTML = ideasCockpit(snapshot);
}

function renderRetrospecs(snapshot) {
  content.innerHTML = retrospecsCockpit(snapshot);
}

function renderWorkflows(snapshot) {
  content.innerHTML = workflowsCockpit(snapshot);
}

function renderTaskRuns(snapshot) {
  content.innerHTML = taskRunsCockpit(snapshot);
}

function renderConfig(snapshot) {
  const config = snapshot.config;
  content.innerHTML = [
    configCockpit(snapshot),
    section(t("sourceConfig"), (config.source_files || []).map(sourceFileScanRow), t("noSourceConfig"), "", "config-sources"),
    section(t("profiles"), snapshot.profiles.items.map(profileScanRow), t("noProfiles"), t("noteProfiles"), "config-profiles"),
    optionalSection(t("invalidProfiles"), snapshot.profiles.invalid.map((row) => invalidScanRow(row, t("invalidProfiles"))), "", "config-invalid-profiles"),
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
    pill(stateLabel(workflowUiGroup(row)), groupColor(workflowUiGroup(row))),
    pill(row.task_runs.total ? `${row.task_runs.total} runs` : "0 runs"),
    row.runnable.runnable_count ? pill(`${row.runnable.runnable_count} runnable`, "green") : "",
    row.task_runs.running ? pill(`${row.task_runs.running} ${stateLabel("running").toLowerCase()}`, "green") : "",
    row.task_runs.failed ? pill(`${row.task_runs.failed} ${stateLabel("failed").toLowerCase()}`, "red") : "",
    row.task_runs.missing ? pill(`${row.task_runs.missing} missing`, "red") : "",
    row.profile ? pill(`profile ${row.profile}`, "violet") : "",
    row.profiles.length ? pill(`${row.profiles.length} profiles`, "violet") : "",
    row.origin ? pill(`${row.origin.provider} ${row.origin.id}`, "violet") : "",
    pill(`${row.policy.pull_request}/${row.policy.landing}`, "amber"),
  ], [row.path], row.body_summary || row.state_error, groupColor(workflowUiGroup(row)), [
    detail(t("body"), row.body || row.state_error, "prose", row.body_summary || row.state_error),
    detail(t("workflowTaskRuns"), formatWorkflowTaskRuns(row.task_run_groups || []), "source"),
    detail(t("sourceToml"), row.source_text, "source"),
  ]);
}

function taskRunCard(row) {
  const taskDocument = row.task_document;
  const taskTitle = taskDocument ? taskDocument.title : row.task;
  return card(row.id, [
    pill(stateLabel(taskRunUiGroup(row)), statusColor(taskRunUiGroup(row))),
    pill(`task ${row.task}`, "blue"),
    taskDocument ? pill(`document ${taskTitle}`, "blue") : pill("document missing", "red"),
    pill(`branch ${row.branch}`),
    row.context.workflow_id ? pill(`workflow ${row.context.workflow_id}`, "violet") : "",
    row.context.mode ? pill(`mode ${row.context.mode}`, "violet") : "",
    row.group ? pill(`group ${row.group}`, "violet") : pill(row.context.label || "direct"),
    row.context.error ? pill("context error", "red") : "",
  ], [row.path, row.context.workflow_path, taskDocument && taskDocument.path].filter(Boolean), row.error || row.context.error || row.task_document_error || taskDocument?.body_summary || row.branch, statusColor(taskRunUiGroup(row)), [
    detail(t("taskRunState"), formatTaskRunState(row), "source", taskRunStatePreview(row)),
    detail(t("taskDocumentToml"), taskDocument?.source_text, "source"),
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

function configCockpit(snapshot) {
  const config = snapshot.config;
  const profileCount = snapshot.profiles.items.length;
  const invalidProfileCount = snapshot.profiles.invalid.length;
  const sourceFileCount = (config.source_files || []).length;
  const stats = [
    { label: t("needsAttention"), value: invalidProfileCount, tone: invalidProfileCount ? "red" : "" },
    { label: t("effectiveConfig"), value: config.source },
    { label: t("workflowPolicy"), value: `${config.workflow.pull_request}/${config.workflow.landing}` },
    { label: t("profiles"), value: profileCount },
  ];
  const rows = [
    scanRow({
      tone: "",
      kicker: t("effectiveConfig"),
      title: `${t("effectiveConfig")} - ${config.source}`,
      summary: bodyPreview(config.effective_text),
      pills: [
        pill(config.source, "blue"),
        pill(`PR ${config.workflow.pull_request}`, "green"),
        pill(`Landing ${config.workflow.landing}`, "amber"),
        config.selected_profile ? pill(`${t("selectedProfile")}: ${config.selected_profile}`, "violet") : pill(t("noSelectedProfile")),
      ],
      paths: config.paths,
      detail: config.effective_text,
    }),
    scanRow({
      tone: "",
      kicker: t("runtime"),
      title: `Agent ${config.agent || "none"}`,
      summary: [`Issues ${config.issues || "none"}`, `Site ${config.site ? config.site.provider : "none"}`].join(" - "),
      pills: [
        pill(`Agent ${config.agent || "none"}`, "blue"),
        pill(`Issues ${config.issues || "none"}`),
        config.site ? pill(`Site ${config.site.provider}`, config.site.active ? "green" : "amber") : pill("Site none"),
      ],
      paths: [],
      detail: "",
    }),
    config.workspace ? scanRow({
      tone: "",
      kicker: t("workspace"),
      title: t("workspace"),
      summary: "",
      pills: [
        pill(`tabs ${config.workspace.tab_count}`, "violet"),
        pill(`post-deps ${config.workspace.post_deps_tab_count}`, "violet"),
        pill(`colors ${config.workspace.color_count}`, "amber"),
      ],
      paths: [],
      detail: "",
    }) : "",
    scanRow({
      tone: invalidProfileCount ? "red" : "",
      kicker: t("profiles"),
      title: `${profileCount} ${t("validProfiles")}`,
      summary: invalidProfileCount ? `${invalidProfileCount} ${t("invalidRecords")}` : t("noInvalidProfiles"),
      pills: [
        pill(`${profileCount} ${t("validProfiles")}`, "violet"),
        invalidProfileCount ? pill(`${invalidProfileCount} ${t("invalid")}`, "red") : pill(t("valid"), "green"),
        pill(`${sourceFileCount} ${t("sourceFiles")}`, "blue"),
      ],
      paths: snapshot.profiles.items.slice(0, 4).map((row) => row.path),
      detail: snapshot.profiles.items.map((row) => `${row.name}: ${row.path}`).join("\n"),
    }),
  ].filter(Boolean);
  return cockpitPanel(t("cockpitConfigTitle"), t("cockpitConfigSubtitle"), stats, t("configIndex"), rows, t("noConfigSummary"), "config-cockpit");
}

function workflowsCockpit(snapshot) {
  const workflows = sortedWorkflows(snapshot.workflows.items);
  const groups = countBy(workflows, workflowUiGroup);
  const invalid = snapshot.workflows.invalid.map((row) => invalidScanRow(row, t("invalidWorkflows")));
  const rows = workflows.map(workflowScanRow).concat(invalid);
  const attentionCount = (groups.needs_attention || 0) + snapshot.workflows.invalid.length;
  const stats = [
    { label: t("needsAttention"), value: attentionCount, tone: attentionCount ? "red" : "" },
    { label: t("runningTaskRuns"), value: groups.running || 0 },
    { label: t("preparedWorkflows"), value: groups.prepared || 0 },
    { label: t("metricWorkflows"), value: workflows.length },
  ];
  return cockpitPanel(t("cockpitWorkflowTitle"), t("cockpitWorkflowSubtitle"), stats, t("workflowIndex"), rows, t("noWorkflows"), "workflow-cockpit");
}

function taskRunsCockpit(snapshot) {
  const runs = sortedTaskRuns(snapshot.task_runs.items);
  const groups = countBy(runs, taskRunUiGroup);
  const rows = runs
    .map(taskRunScanRow)
    .concat(snapshot.task_runs.invalid.map((row) => invalidScanRow(row, t("invalidTaskRuns"))))
    .concat(unlinkedTaskDocuments(snapshot).map(taskDocumentScanRow));
  const attentionCount = (groups.needs_attention || 0) + snapshot.task_runs.invalid.length;
  const stats = [
    { label: t("needsAttention"), value: attentionCount, tone: attentionCount ? "red" : "" },
    { label: t("stateRunning"), value: groups.running || 0 },
    { label: t("statePrepared"), value: groups.prepared || 0 },
    { label: t("metricTaskRuns"), value: runs.length },
  ];
  return cockpitPanel(t("cockpitTaskRunTitle"), t("cockpitTaskRunSubtitle"), stats, t("taskRunIndex"), rows, t("noTaskRuns"), "task-runs-cockpit");
}

function ideasCockpit(snapshot) {
  const ideas = sortedIdeas(snapshot.ideas.items);
  const statusGroups = uniqueCount(ideas, (row) => row.status || "unspecified");
  const tagged = ideas.filter((row) => row.tags.length).length;
  const rows = ideas.map(ideaScanRow).concat(snapshot.ideas.invalid.map((row) => invalidScanRow(row, t("invalidIdeas"))));
  const stats = [
    { label: t("needsAttention"), value: snapshot.ideas.invalid.length, tone: snapshot.ideas.invalid.length ? "red" : "" },
    { label: t("ideas"), value: ideas.length },
    { label: t("statusGroups"), value: statusGroups },
    { label: t("taggedRecords"), value: tagged },
  ];
  return cockpitPanel(t("cockpitIdeasTitle"), t("cockpitIdeasSubtitle"), stats, t("ideaIndex"), rows, t("noIdeas"), "ideas-cockpit");
}

function retrospecsCockpit(snapshot) {
  const retrospecs = sortedRetrospecs(snapshot.retrospecs.items);
  const outcomeGroups = uniqueCount(retrospecs, (row) => row.outcome || "unspecified");
  const tagged = retrospecs.filter((row) => row.tags.length).length;
  const rows = retrospecs.map(retrospecScanRow).concat(snapshot.retrospecs.invalid.map((row) => invalidScanRow(row, t("invalidRetrospecs"))));
  const stats = [
    { label: t("needsAttention"), value: snapshot.retrospecs.invalid.length, tone: snapshot.retrospecs.invalid.length ? "red" : "" },
    { label: t("retrospecs"), value: retrospecs.length },
    { label: t("outcomeGroups"), value: outcomeGroups },
    { label: t("taggedRecords"), value: tagged },
  ];
  return cockpitPanel(t("cockpitRetrospecsTitle"), t("cockpitRetrospecsSubtitle"), stats, t("retrospecIndex"), rows, t("noRetrospecs"), "retrospecs-cockpit");
}

function overviewFocusModel(snapshot) {
  return {
    prepared: sortedWorkflows(snapshot.workflows.items).filter((row) => workflowUiGroup(row) === "prepared"),
    running: sortedTaskRuns(snapshot.task_runs.items).filter((row) => row.status === "running" && !taskRunNeedsAttention(row)),
    attention: overviewAttentionItems(snapshot),
  };
}

function focusPanel(focus) {
  const groups = [
    {
      key: "attention",
      title: t("needsAttention"),
      count: focus.attention.length,
      tone: focus.attention.length ? "red" : "",
      sectionId: "overview-attention",
      emptyText: t("noNeedsAttention"),
      items: focus.attention,
    },
    {
      key: "running",
      title: t("runningTaskRuns"),
      count: focus.running.length,
      tone: "",
      sectionId: "overview-task-runs",
      emptyText: t("noRunningTaskRuns"),
      items: focus.running.map(runningFocusItem),
    },
    {
      key: "prepared",
      title: t("preparedWorkflows"),
      count: focus.prepared.length,
      tone: "",
      sectionId: "overview-workflows",
      emptyText: t("noPreparedWorkflows"),
      items: focus.prepared.map(preparedWorkflowFocusItem),
    },
  ];
  const stats = groups.map((group) => ({ label: group.title, value: group.count, tone: group.tone }));
  return `<section class="focus-panel" aria-labelledby="focus-heading"><div class="focus-heading"><div><h2 id="focus-heading" class="section-title">${escapeHtml(t("focusTitle"))}</h2><p class="section-note">${escapeHtml(t("focusSubtitle"))}</p></div>${statusStrip(stats)}</div>${priorityFlow(groups)}</section>`;
}

function cockpitPanel(title, subtitle, stats, listTitle, rows, emptyText, id) {
  const body = rows.length ? `<div class="scan-list">${rows.join("")}</div>` : `<div class="focus-empty">${escapeHtml(emptyText)}</div>`;
  const count = rows.length === 1 ? `1 ${t("record")}` : `${rows.length} ${t("records")}`;
  return `<section class="focus-panel view-cockpit" id="${escapeHtml(id)}" aria-labelledby="${escapeHtml(id)}-heading"><div class="focus-heading"><div><h2 id="${escapeHtml(id)}-heading" class="section-title">${escapeHtml(title)}</h2><p class="section-note">${escapeHtml(subtitle)}</p></div>${statusStrip(stats)}</div><div class="scan-heading"><h3>${escapeHtml(listTitle)}</h3><span>${escapeHtml(count)}</span></div>${body}</section>`;
}

function statusStrip(stats) {
  const items = stats
    .map((stat) => `<div class="status-counter tone-${stat.tone || "neutral"}"><span>${escapeHtml(stat.label)}</span><strong>${escapeHtml(stat.value)}</strong></div>`)
    .join("");
  return `<div class="status-strip-inline">${items}</div>`;
}

function priorityFlow(groups) {
  return `<div class="priority-flow">${groups.map(priorityGroup).join("")}</div>`;
}

function priorityGroup(group) {
  const limit = group.key === "attention" ? 5 : 3;
  const visible = group.items.slice(0, limit);
  const body = visible.length
    ? `<ul class="focus-list">${visible.map(focusItem).join("")}</ul>`
    : `<div class="focus-empty">${escapeHtml(group.emptyText)}</div>`;
  const more = group.items.length > limit
    ? `<a class="focus-more" href="#${escapeHtml(group.sectionId)}">${escapeHtml(tr("focusMore", { count: group.items.length - limit }))}</a>`
    : "";
  return `<section class="priority-group tone-${group.tone || "neutral"}" aria-labelledby="focus-${group.key}"><div class="priority-heading"><h3 id="focus-${group.key}">${escapeHtml(group.title)}</h3><span>${group.count}</span></div>${body}${more}</section>`;
}

function scanRow(row) {
  const meta = (row.pills || []).filter(Boolean).join("");
  const pathsHtml = row.paths && row.paths.length
    ? `<div class="scan-paths">${row.paths.slice(0, 3).map((path) => `<p class="focus-path">${escapeHtml(path)}</p>`).join("")}</div>`
    : "";
  const summary = row.summary ? `<p class="focus-summary">${escapeHtml(row.summary)}</p>` : "";
  const detailText = scanDetailText(row);
  const details = detailText
    ? `<details class="focus-inspector scan-inspector"><summary>${escapeHtml(t("focusSource"))}</summary><div class="source-panel full-text">${formatFullText(detailText, row.detailKind || "source")}</div></details>`
    : "";
  return `<article class="scan-row tone-${row.tone || "neutral"}"><div class="scan-main"><span class="focus-kicker">${escapeHtml(row.kicker)}</span><h4>${escapeHtml(row.title)}</h4>${summary}</div><div class="scan-meta meta">${meta}</div><div class="scan-side">${pathsHtml}${details}</div></article>`;
}

function scanDetailText(row) {
  const parts = [];
  if (row.detail) {
    parts.push(row.detail);
  }
  if (row.paths && row.paths.length) {
    parts.push(`${t("source")}:\n${row.paths.join("\n")}`);
  }
  return parts.join("\n\n");
}

function workflowScanRow(row) {
  const group = workflowUiGroup(row);
  return scanRow({
    tone: group === "needs_attention" ? "red" : "",
    kicker: `Workflow ${row.id}`,
    title: row.title,
    summary: row.state_error || row.body_summary || "",
    pills: [
      pill(stateLabel(group), groupColor(group)),
      pill(row.mode, "blue"),
      pill(`${row.task_runs.total} ${t("metricTaskRuns")}`),
      row.runnable.runnable_count ? pill(`${row.runnable.runnable_count} runnable`, "green") : "",
      row.task_runs.running ? pill(`${row.task_runs.running} ${stateLabel("running").toLowerCase()}`, "green") : "",
      row.task_runs.failed ? pill(`${row.task_runs.failed} ${stateLabel("failed").toLowerCase()}`, "red") : "",
      row.task_runs.missing ? pill(`${row.task_runs.missing} missing`, "red") : "",
      pill(`${row.policy.pull_request}/${row.policy.landing}`, "amber"),
    ],
    paths: [row.path],
    detail: formatWorkflowTaskRuns(row.task_run_groups || []) || row.source_text,
  });
}

function taskRunScanRow(row) {
  const taskDocument = row.task_document;
  const group = taskRunUiGroup(row);
  return scanRow({
    tone: group === "needs_attention" ? "red" : "",
    kicker: `TaskRun ${row.id}`,
    title: taskDocument ? taskDocument.title : row.task,
    summary: row.error || row.context.error || row.task_document_error || taskDocument?.body_summary || row.branch,
    pills: [
      pill(stateLabel(group), statusColor(group)),
      pill(`task ${row.task}`, "blue"),
      pill(`branch ${row.branch}`),
      row.context.workflow_id ? pill(`workflow ${row.context.workflow_id}`, "violet") : pill(row.context.label || "direct"),
      row.context.mode ? pill(row.context.mode, "violet") : "",
    ],
    paths: [row.path, row.context.workflow_path, taskDocument && taskDocument.path].filter(Boolean),
    detail: formatTaskRunState(row),
  });
}

function taskDocumentScanRow(row) {
  return scanRow({
    tone: "red",
    kicker: "TaskDocument",
    title: row.title,
    summary: row.body_summary,
    pills: [
      pill(t("focusUnlinkedTaskDocument"), "amber"),
      pill(`task ${row.key}`, "blue"),
      row.branch ? pill(`branch ${row.branch}`) : "",
    ],
    paths: [row.path],
    detail: row.source_text,
  });
}

function ideaScanRow(row) {
  return scanRow({
    tone: "",
    kicker: row.kind,
    title: row.title,
    summary: row.body_summary,
    pills: [
      pill(row.status || "unspecified", statusColor(row.status)),
      row.source ? pill(row.source, "blue") : "",
      ...row.tags.slice(0, 4).map((tag) => pill(tag, "violet")),
    ],
    paths: [row.path],
    detail: row.source_text,
  });
}

function retrospecScanRow(row) {
  return scanRow({
    tone: "",
    kicker: row.kind,
    title: row.title,
    summary: row.body_summary,
    pills: [
      row.outcome ? pill(row.outcome, statusColor(row.outcome)) : "",
      row.target ? pill(row.target, "blue") : "",
      row.date ? pill(row.date, "amber") : "",
      ...row.tags.slice(0, 4).map((tag) => pill(tag, "violet")),
    ],
    paths: [row.path],
    detail: row.source_text,
  });
}

function invalidScanRow(row, labelText) {
  return scanRow({
    tone: "red",
    kicker: labelText,
    title: row.key,
    summary: row.error,
    pills: [pill(t("invalid"), "red")],
    paths: [row.path],
    detail: [row.error, row.source_text].filter(Boolean).join("\n\n"),
  });
}

function sourceFileScanRow(row) {
  return scanRow({
    tone: "",
    kicker: t("sourceConfig"),
    title: row.path,
    summary: bodyPreview(row.text),
    pills: [pill(t("source"), "blue")],
    paths: [row.path],
    detail: row.text,
  });
}

function profileScanRow(row) {
  return scanRow({
    tone: "",
    kicker: "profile",
    title: row.name,
    summary: bodyPreview(row.source_text),
    pills: [
      pill(`agent ${row.agent}`, "blue"),
      pill(`${row.copy_count} copy`),
      pill(`${row.link_count} link`),
      row.has_site ? pill("site", "green") : "",
      row.test_count ? pill(`${row.test_count} tests`, "amber") : "",
    ],
    paths: [row.path],
    detail: row.source_text,
  });
}

function focusScanRow(item) {
  return scanRow({
    tone: item.tone === "red" ? "red" : "",
    kicker: item.kicker,
    title: item.title,
    summary: item.summary,
    pills: [
      pill(item.reason, item.tone === "red" ? "red" : ""),
      item.meta ? pill(item.meta) : "",
    ],
    paths: item.paths,
    detail: item.detail,
    detailKind: item.detailKind,
  });
}

function focusItem(item) {
  const path = item.paths.find(Boolean);
  const meta = [item.reason, item.meta].filter(Boolean).map((value, index) => pill(value, index === 0 ? item.tone : "")).join("");
  const pathHtml = path ? `<p class="focus-path">${escapeHtml(path)}</p>` : "";
  const summary = item.summary ? `<p class="focus-summary">${escapeHtml(item.summary)}</p>` : "";
  const detailText = focusDetailText(item);
  const details = detailText
    ? `<details class="focus-inspector"><summary>${escapeHtml(t("focusSource"))}</summary><div class="source-panel full-text">${formatFullText(detailText, item.detailKind || "source")}</div></details>`
    : "";
  return `<li class="focus-item tone-${item.tone || "neutral"}"><div class="focus-item-main"><span class="focus-kicker">${escapeHtml(item.kicker)}</span><h4>${escapeHtml(item.title)}</h4>${summary}<div class="meta">${meta}</div>${pathHtml}</div>${details}</li>`;
}

function focusDetailText(item) {
  const parts = [];
  if (item.detail) {
    parts.push(item.detail);
  }
  if (item.paths.length) {
    parts.push(`${t("source")}:\n${item.paths.join("\n")}`);
  }
  return parts.join("\n\n");
}

function overviewAttentionItems(snapshot) {
  return [
    ...sortedWorkflows(snapshot.workflows.items).filter((row) => workflowUiGroup(row) === "needs_attention").map(attentionWorkflowFocusItem),
    ...sortedTaskRuns(snapshot.task_runs.items).filter(taskRunNeedsAttention).map(attentionTaskRunFocusItem),
    ...unlinkedTaskDocuments(snapshot).map(unlinkedTaskFocusItem),
    ...snapshot.ideas.invalid.map((row) => invalidFocusItem(row, t("invalidIdeas"))),
    ...snapshot.tasks.invalid.map((row) => invalidFocusItem(row, t("invalidTaskDocuments"))),
    ...snapshot.workflows.invalid.map((row) => invalidFocusItem(row, t("invalidWorkflows"))),
    ...snapshot.task_runs.invalid.map((row) => invalidFocusItem(row, t("invalidTaskRuns"))),
    ...snapshot.profiles.invalid.map((row) => invalidFocusItem(row, t("invalidProfiles"))),
    ...snapshot.retrospecs.invalid.map((row) => invalidFocusItem(row, t("invalidRetrospecs"))),
  ];
}

function attentionWorkflowFocusItem(row) {
  return {
    kicker: `Workflow ${row.id}`,
    title: row.title,
    reason: workflowAttentionReason(row),
    meta: `${row.mode} - ${row.task_runs.total} runs`,
    summary: row.state_error || row.body_summary,
    paths: [row.path],
    detail: formatWorkflowTaskRuns(row.task_run_groups || []) || row.source_text,
    tone: "red",
  };
}

function attentionTaskRunFocusItem(row) {
  const taskDocument = row.task_document;
  return {
    kicker: `TaskRun ${row.id}`,
    title: taskDocument ? taskDocument.title : row.task,
    reason: taskRunAttentionReason(row),
    meta: `branch ${row.branch}`,
    summary: row.error || row.context.error || row.task_document_error || taskDocument?.body_summary || "",
    paths: [row.path, row.context.workflow_path, taskDocument && taskDocument.path].filter(Boolean),
    detail: formatTaskRunState(row),
    tone: "red",
  };
}

function unlinkedTaskFocusItem(row) {
  return {
    kicker: "TaskDocument",
    title: row.title,
    reason: t("focusUnlinkedTaskDocument"),
    meta: row.branch ? `branch ${row.branch}` : row.key,
    summary: row.body_summary,
    paths: [row.path],
    detail: row.source_text,
    tone: "amber",
  };
}

function invalidFocusItem(row, labelText) {
  return {
    kicker: labelText,
    title: row.key,
    reason: t("focusInvalid"),
    meta: "",
    summary: row.error,
    paths: [row.path],
    detail: [row.error, row.source_text].filter(Boolean).join("\n\n"),
    tone: "red",
  };
}

function runningFocusItem(row) {
  const taskDocument = row.task_document;
  return {
    kicker: `TaskRun ${row.id}`,
    title: taskDocument ? taskDocument.title : row.task,
    reason: t("focusRunningTaskRun"),
    meta: `branch ${row.branch}`,
    summary: taskDocument?.body_summary || row.context.label || "",
    paths: [row.path, row.context.workflow_path, taskDocument && taskDocument.path].filter(Boolean),
    detail: formatTaskRunState(row),
    tone: "",
  };
}

function preparedWorkflowFocusItem(row) {
  return {
    kicker: `Workflow ${row.id}`,
    title: row.title,
    reason: t("focusRunnableWorkflow"),
    meta: `${row.mode} - ${row.runnable.runnable_count} runnable`,
    summary: row.body_summary,
    paths: [row.path],
    detail: formatWorkflowTaskRuns(row.task_run_groups || []) || row.source_text,
    tone: "",
  };
}

function taskRunAttentionReason(row) {
  if (row.status === "failed") return t("focusFailedTaskRun");
  if (!row.task_document) return t("focusMissingTaskDocument");
  if (row.context.error) return t("focusContextError");
  if (row.task_document_error) return t("focusTaskDocumentError");
  if (row.error) return t("focusTaskRunError");
  return t("needsAttention");
}

function overviewAttentionRows(snapshot) {
  return [
    ...sortedWorkflows(snapshot.workflows.items).filter((row) => workflowUiGroup(row) === "needs_attention").map(workflowCard),
    ...sortedTaskRuns(snapshot.task_runs.items).filter(taskRunNeedsAttention).map(taskRunCard),
    ...unlinkedTaskDocuments(snapshot).map(taskCard),
    ...snapshot.ideas.invalid.map(invalidCard),
    ...snapshot.tasks.invalid.map(invalidCard),
    ...snapshot.workflows.invalid.map(invalidCard),
    ...snapshot.task_runs.invalid.map(invalidCard),
    ...snapshot.profiles.invalid.map(invalidCard),
    ...snapshot.retrospecs.invalid.map(invalidCard),
  ];
}

function workflowAttentionReason(row) {
  if (row.state_error || row.presentation_group === "state_error") return t("stateError");
  if (row.task_runs.missing) return t("focusMissingTaskRun");
  if (row.task_runs.failed) return t("focusFailedLinkedTaskRun");
  return t("needsAttention");
}

function taskRunAttentionRows(snapshot) {
  return [
    ...sortedTaskRuns(snapshot.task_runs.items).filter(taskRunNeedsAttention).map(taskRunCard),
    ...snapshot.task_runs.invalid.map(invalidCard),
    ...unlinkedTaskDocuments(snapshot).map(taskCard),
    ...snapshot.tasks.invalid.map(invalidCard),
  ];
}

function unlinkedTaskDocuments(snapshot) {
  const linkedTasks = new Set(snapshot.task_runs.items.map((row) => row.task));
  return snapshot.tasks.items.filter((row) => !linkedTasks.has(row.key));
}

function taskRunNeedsAttention(row) {
  return Boolean(
    row.status === "failed" ||
      row.error ||
      row.context.error ||
      row.task_document_error ||
      !row.task_document
  );
}

function taskRunUiGroup(row) {
  return taskRunNeedsAttention(row) ? "needs_attention" : row.status;
}

function workflowNeedsAttention(row) {
  return Boolean(
    row.presentation_group === "state_error" ||
      row.state_error ||
      row.task_runs.failed ||
      row.task_runs.missing
  );
}

function workflowUiGroup(row) {
  if (workflowNeedsAttention(row)) return "needs_attention";
  if (row.task_runs.running) return "running";
  if (row.presentation_group === "runnable") return "prepared";
  if (row.presentation_group === "done") return "done";
  return "waiting";
}

function formatTaskRunState(row) {
  const lines = [
    `task: ${row.task}`,
    `status: ${stateLabel(taskRunUiGroup(row))}`,
    row.status !== taskRunUiGroup(row) ? `stored_status: ${row.status}` : "",
    `branch: ${row.branch}`,
    row.group ? `group: ${row.group}` : "",
    row.context.label ? `context: ${row.context.label}` : "",
    row.context.error ? `context_error: ${row.context.error}` : "",
    row.error ? `error: ${row.error}` : "",
    row.task_document_error ? `task_document_error: ${row.task_document_error}` : "",
    !row.task_document && !row.task_document_error ? "task_document: missing" : "",
  ].filter(Boolean);
  return lines.join("\n");
}

function taskRunStatePreview(row) {
  const parts = [
    stateLabel(taskRunUiGroup(row)),
    `task ${row.task}`,
    `branch ${row.branch}`,
    row.error || row.context.error || row.task_document_error || "",
  ].filter(Boolean);
  return compactText(parts.join(" - "), 190);
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
      return `[${stateLabel(group.status)}]\n${rows.join("\n")}`;
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

function optionalSection(title, rows, note = "", id = "") {
  if (!rows.length) {
    return "";
  }
  return section(title, rows, "", note, id);
}

function jumpNav(items) {
  if (!items.length) {
    return "";
  }
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

function sortedIdeas(rows) {
  return [...rows].sort((left, right) => {
    const updated = String(right.updated_at || "").localeCompare(String(left.updated_at || ""));
    if (updated !== 0) return updated;
    return String(left.title).localeCompare(String(right.title));
  });
}

function sortedRetrospecs(rows) {
  return [...rows].sort((left, right) => {
    const dated = String(right.date || "").localeCompare(String(left.date || ""));
    if (dated !== 0) return dated;
    return String(left.title).localeCompare(String(right.title));
  });
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
    const group = workflowGroupOrder(workflowUiGroup(left)) - workflowGroupOrder(workflowUiGroup(right));
    if (group !== 0) return group;
    return String(right.updated_at).localeCompare(String(left.updated_at)) || String(left.id).localeCompare(String(right.id));
  });
}

function taskRunStatusOrder(status) {
  return ["prepared", "running", "done", "skipped", "failed", "needs_attention"].indexOf(status) === -1
    ? 99
    : ["prepared", "running", "done", "skipped", "failed", "needs_attention"].indexOf(status);
}

function workflowGroupOrder(group) {
  return ["prepared", "running", "waiting", "done", "needs_attention"].indexOf(group) === -1
    ? 99
    : ["prepared", "running", "waiting", "done", "needs_attention"].indexOf(group);
}

function countBy(rows, mapper) {
  return rows.reduce((counts, row) => {
    const key = mapper(row);
    counts[key] = (counts[key] || 0) + 1;
    return counts;
  }, {});
}

function uniqueCount(rows, mapper) {
  return new Set(rows.map(mapper).filter(Boolean)).size;
}

function statusColor(status) {
  if (status === "prepared" || status === "ready" || status === "running" || status === "landed") return "green";
  if (status === "failed" || status === "state_error" || status === "needs_attention") return "red";
  if (status === "skipped" || status === "waiting") return "amber";
  if (status === "done") return "blue";
  return "";
}

function groupColor(group) {
  return statusColor(group);
}

function stateLabel(value) {
  const keys = {
    prepared: "statePrepared",
    runnable: "statePrepared",
    running: "stateRunning",
    waiting: "stateWaiting",
    done: "stateDone",
    skipped: "stateSkipped",
    failed: "stateFailed",
    state_error: "stateError",
    needs_attention: "needsAttention",
  };
  return keys[value] ? t(keys[value]) : label(value);
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
