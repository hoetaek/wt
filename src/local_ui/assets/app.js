const state = {
  snapshot: null,
  view: "overview",
};

const tabs = document.querySelectorAll(".tabs button");
const content = document.querySelector("#content");
const metrics = document.querySelector("#metrics");
const repoLabel = document.querySelector("#repo-label");
const refreshButton = document.querySelector("#refresh");

tabs.forEach((button) => {
  button.addEventListener("click", () => {
    state.view = button.dataset.view;
    tabs.forEach((tab) => tab.classList.toggle("active", tab === button));
    render();
  });
});

refreshButton.addEventListener("click", loadSnapshot);

loadSnapshot();

async function loadSnapshot() {
  content.innerHTML = '<div class="loading">Loading</div>';
  refreshButton.disabled = true;
  try {
    const response = await fetch("/api/snapshot", { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    state.snapshot = await response.json();
    render();
  } catch (error) {
    content.innerHTML = `<div class="invalid-card"><h3>Snapshot unavailable</h3><p class="error">${escapeHtml(error.message)}</p></div>`;
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

  const view = state.view;
  if (view === "overview") renderOverview(snapshot);
  if (view === "ideas") renderIdeas(snapshot);
  if (view === "tasks") renderTasks(snapshot);
  if (view === "workflows") renderWorkflows(snapshot);
  if (view === "task-runs") renderTaskRuns(snapshot);
  if (view === "profiles") renderProfiles(snapshot);
  if (view === "config") renderConfig(snapshot);
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
      const invalidText = invalid ? `${invalid} invalid` : "valid";
      return `<div class="metric"><strong>${count}</strong><span>${label} - ${invalidText}</span></div>`;
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
      "Runnable workflows",
      runnable.map(workflowCard),
      "No runnable workflows"
    ),
    section("Running TaskRuns", running.map(taskRunCard), "No running TaskRuns"),
    section("Invalid records", invalid.map(invalidCard), "No invalid records"),
  ].join("");
}

function renderIdeas(snapshot) {
  content.innerHTML = [
    section("Ideas", snapshot.ideas.items.map(ideaCard), "No ideas"),
    section("Invalid ideas", snapshot.ideas.invalid.map(invalidCard), "No invalid ideas"),
  ].join("");
}

function renderTasks(snapshot) {
  content.innerHTML = [
    section("TaskDocuments", snapshot.tasks.items.map(taskCard), "No TaskDocuments"),
    section("Invalid TaskDocuments", snapshot.tasks.invalid.map(invalidCard), "No invalid TaskDocuments"),
  ].join("");
}

function renderWorkflows(snapshot) {
  const groups = ["runnable", "waiting", "done", "state_error"];
  content.innerHTML = groups
    .map((group) => {
      const rows = snapshot.workflows.items.filter((row) => row.presentation_group === group);
      return section(label(group), rows.map(workflowCard), `No ${label(group).toLowerCase()} workflows`);
    })
    .concat(section("Invalid workflows", snapshot.workflows.invalid.map(invalidCard), "No invalid workflows"))
    .join("");
}

function renderTaskRuns(snapshot) {
  const statuses = ["prepared", "running", "failed", "skipped", "done"];
  content.innerHTML = statuses
    .map((status) => {
      const rows = snapshot.task_runs.items.filter((row) => row.status === status);
      return section(label(status), rows.map(taskRunCard), `No ${status} TaskRuns`);
    })
    .concat(section("Invalid TaskRuns", snapshot.task_runs.invalid.map(invalidCard), "No invalid TaskRuns"))
    .join("");
}

function renderProfiles(snapshot) {
  content.innerHTML = [
    section("Profiles", snapshot.profiles.items.map(profileCard), "No profiles"),
    section("Invalid profiles", snapshot.profiles.invalid.map(invalidCard), "No invalid profiles"),
  ].join("");
}

function renderConfig(snapshot) {
  const config = snapshot.config;
  const cards = [
    card("Effective config", [
      pill(config.source, "blue"),
      pill(`pr ${config.workflow.pull_request}`, "green"),
      pill(`landing ${config.workflow.landing}`, "amber"),
      config.selected_profile ? pill(`profile ${config.selected_profile}`, "violet") : "",
    ], config.paths),
    card("Runtime", [
      pill(`agent ${config.agent || "none"}`),
      pill(`issues ${config.issues || "none"}`),
      pill(`site ${config.site ? config.site.provider : "none"}`),
    ], []),
  ];
  if (config.workspace) {
    cards.push(card("Workspace", [
      pill(`${config.workspace.tab_count} tabs`),
      pill(`${config.workspace.post_deps_tab_count} post-deps`),
      pill(`${config.workspace.color_count} colors`),
    ], []));
  }
  content.innerHTML = section("Config", cards, "No config summary");
}

function ideaCard(row) {
  return card(row.title, [
    pill(row.status || "unspecified", statusColor(row.status)),
    pill(row.kind),
    row.source ? pill(row.source) : "",
    ...row.tags.map((tag) => pill(tag, "violet")),
  ], [row.path], row.body_summary);
}

function taskCard(row) {
  return card(row.title, [
    pill(`task ${row.key}`, "blue"),
    row.branch ? pill(`branch ${row.branch}`) : "",
    row.origin ? pill(`${row.origin.provider} ${row.origin.id}`, "violet") : pill(row.source),
  ], [row.path], row.body_summary);
}

function workflowCard(row) {
  return card(row.id, [
    pill(row.mode, "blue"),
    pill(row.presentation_group, groupColor(row.presentation_group)),
    pill(row.task_runs.total ? `${row.task_runs.total} runs` : "0 runs"),
    row.runnable.runnable_count ? pill(`${row.runnable.runnable_count} runnable`, "green") : "",
    row.profile ? pill(`profile ${row.profile}`, "violet") : "",
    row.profiles.length ? pill(`${row.profiles.length} profiles`, "violet") : "",
    pill(`${row.policy.pull_request}/${row.policy.landing}`, "amber"),
  ], [row.path], row.objective_summary || row.state_error);
}

function taskRunCard(row) {
  return card(row.id, [
    pill(row.status, statusColor(row.status)),
    pill(`task ${row.task}`, "blue"),
    row.group ? pill(`group ${row.group}`, "violet") : pill("direct"),
    row.context.error ? pill("context error", "red") : "",
  ], [row.path, row.context.workflow_path].filter(Boolean), row.error || row.context.error || row.branch);
}

function profileCard(row) {
  return card(row.name, [
    pill(`agent ${row.agent}`, "blue"),
    pill(`${row.copy_count} copy`),
    pill(`${row.link_count} link`),
    row.has_site ? pill("site", "green") : "",
    row.test_count ? pill(`${row.test_count} tests`, "amber") : "",
  ], [row.path]);
}

function invalidCard(row) {
  return `<article class="invalid-card"><h3>${escapeHtml(row.key)}</h3><p class="path">${escapeHtml(row.path)}</p><p class="error">${escapeHtml(row.error)}</p></article>`;
}

function card(title, pills, pathRows, summary) {
  const meta = pills.filter(Boolean).join("");
  const pathHtml = paths(pathRows);
  const summaryHtml = summary ? `<p class="summary">${escapeHtml(summary)}</p>` : "";
  return `<article class="card"><h3>${escapeHtml(title)}</h3><div class="meta">${meta}</div>${pathHtml}${summaryHtml}</article>`;
}

function section(title, rows, emptyText) {
  const body = rows.length ? `<div class="grid">${rows.join("")}</div>` : `<div class="empty">${escapeHtml(emptyText)}</div>`;
  return `<section><h2 class="section-title">${escapeHtml(title)}</h2>${body}</section>`;
}

function paths(rows) {
  return rows.map((path) => `<p class="path">${escapeHtml(path)}</p>`).join("");
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

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}
