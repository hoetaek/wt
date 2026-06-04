const LOCALE_KEY = "wt-ui-locale";

const state = {
  snapshot: null,
  view: "overview",
  locale: initialLocale(),
  selection: {},
};

const tabs = Array.from(document.querySelectorAll(".tabs button"));
const content = document.querySelector("#content");
const metrics = document.querySelector("#metrics");
const workspaceLabel = document.querySelector("#workspace-label");
const repoLabel = document.querySelector("#repo-label");
const languageButton = document.querySelector("#language-toggle");
const statusRegion = document.querySelector("#status");

const WORKFLOW_CANVAS = {
  margin: 44,
  workflowW: 214,
  workflowH: 112,
  taskW: 260,
  taskH: 216,
  agentW: 144,
  agentH: 76,
  matrixRunW: 238,
  matrixRunH: 140,
  gapX: 60,
  gapY: 70,
  agentGap: 22,
};

const STRINGS = {
  en: {
    eyebrow: "Read-only personal inventory",
    switchToKorean: "Switch language to Korean",
    switchToEnglish: "Switch language to English",
    readFull: "Read full",
    collapse: "Collapse",
    body: "Body",
    source: "Source",
    sourceToml: "Source TOML",
    effectiveConfig: "Final values",
    workspace: "Workspace",
    sourceConfig: "Config source file",
    localSettings: "Local settings",
    sharedSettings: "Shared settings",
    otherSettings: "Other settings",
    detailSummary: "Summary",
    settingCards: "Setting cards",
    detailRelationships: "Related info",
    localTomlPath: "Local TOML path",
    profileTomlPath: "Profile TOML path",
    appliedSettingsLayers: "Applied settings",
    settingsPath: "File location",
    settingsToml: "Settings TOML",
    settingsRole: "What this controls",
    profileToml: "Profile TOML",
    profileOnlyBadge: "profile.toml",
    profileLocation: "path",
    sourceContent: "Source content",
    renderedContent: "Rendered content",
    sourcePaths: "Source paths",
    sourceHome: "Source home",
    statusLabel: "status",
    kindLabel: "kind",
    tagsLabel: "tags",
    updatedAtLabel: "updated_at",
    outcomeLabel: "outcome",
    dateLabel: "date",
    scopeLabel: "scope",
    specLabel: "spec",
    profileName: "Profile name",
    agentLabel: "cli",
    issuesLabel: "Issues",
    siteLabel: "provider",
    pullRequestLabel: "pull_request",
    landingLabel: "landing",
    reviewLabel: "review",
    codexBaseLabel: "codex base",
    worktreePathLabel: "path",
    namingLabel: "naming",
    namingCommandLabel: "command",
    namingBranchLabel: "branch",
    namingWorkspaceLabel: "workspace",
    namingPromptLabel: "prompt",
    copyAsLabel: "copy_as",
    localContextLabel: "inject_local_context",
    setupLabel: "Setup",
    depsLabel: "deps",
    envLabel: "env",
    envFilesLabel: "env_files",
    ifExistsLabel: "if_exists",
    workingDirLabel: "working_dir",
    siteNameLabel: "name",
    rootLabel: "root",
    secureLabel: "secure",
    urlLabel: "url",
    targetLabel: "target",
    tabsLabel: "tabs",
    postDepsTabsLabel: "post_deps_tabs",
    colorsLabel: "colors",
    browserLabel: "browser",
    chromeDevtoolsLabel: "chrome_devtools",
    userDataDirLabel: "user_data_dir",
    portLabel: "port",
    commandLabel: "command",
    placementLabel: "placement",
    argsLabel: "args",
    readyLabel: "ready",
    submitLabel: "submit",
    timeoutLabel: "timeout",
    sendAfterLabel: "send_after",
    promptModesLabel: "prompt",
    testLabel: "test",
    copyLabel: "copy",
    githubUserLabel: "gh_user",
    issuesProviderLabel: "provider",
    linkLabel: "link",
    testsLabel: "commands",
    errorLabel: "Error",
    configuredLabel: "Configured",
    omittedLabel: "Omitted",
    notConfiguredLabel: "Not configured",
    yes: "yes",
    no: "no",
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
    noteOverview: "Action-focused snapshot across personal wt state, prepared workflows, running TaskRuns, and records that need attention.",
    noteIdeas: "Ideas are planning notes included for read-only context.",
    noteRetrospecs: "Retrospectives include spec-local work lessons and cross-work learning records.",
    noteWorkflows: "Workflows are grouped by derived state and show linked TaskRuns inside each plan.",
    noteTaskRuns: "TaskRuns are execution records from <repo-root>/.wt/execution/task-runs with linked TaskDocument content. Failed or broken links are grouped under Needs attention.",
    noteProfiles: "Profiles are effective agent/config overlays from <repo-root>/.wt/config/profiles.",
    noteConfig: "Config shows final settings, settings layers, and profiles.",
    cockpitConfigTitle: "Config cockpit",
    cockpitConfigSubtitle: "Review final settings, settings layers, and profiles.",
    cockpitWorkflowTitle: "Workflow cockpit",
    cockpitWorkflowSubtitle: "Derived Workflow state, runnable work, linked TaskRuns, and source paths.",
    cockpitTaskRunTitle: "TaskRun cockpit",
    cockpitTaskRunSubtitle: "Execution records grouped for comparison by status, branch, context, and source path.",
    cockpitIdeasTitle: "Ideas cockpit",
    cockpitIdeasSubtitle: "Planning records with status, source, tags, and source paths kept close to the title.",
    cockpitRetrospecsTitle: "Retrospecs cockpit",
    cockpitRetrospecsSubtitle: "Spec-local and cross-work lessons with outcome, target, date, tags, and source paths.",
    workflowIndex: "Workflow index",
    taskRunIndex: "TaskRun index",
    ideaIndex: "Idea index",
    retrospecIndex: "Retrospec index",
    configIndex: "Config list",
    configGroupAttention: "Needs attention",
    configGroupCurrentSettings: "Current settings",
    configAppliedSummary: "Final settings wt will use after applying the colored settings layers below.",
    worktreeHelp: "Files and local context prepared inside each new worktree.",
    worktreePathHelp: "Template for where new worktrees are created.",
    worktreeCopyHelp: "Files copied into the new worktree.",
    worktreeCopyAsHelp: "Files copied to a different destination in the new worktree.",
    worktreeLinkHelp: "Paths linked instead of copied.",
    localContextHelp: "Injects rendered site/worktree/parent context into the agent context file.",
    namingHelp: "How wt asks for stable branch/workspace names.",
    namingCommandHelp: "Command used to generate naming variables.",
    namingBranchHelp: "Template used for generated branch names.",
    namingWorkspaceHelp: "Template used for workspace titles when configured.",
    namingPromptHelp: "Prompt template is configured; full text stays in the TOML source.",
    setupHelp: "Commands and environment variables prepared before agent work.",
    depsHelp: "Dependency/setup commands wt runs when their condition matches.",
    envHelp: "Environment variables rendered for setup and workspace commands.",
    envFilesHelp: "Values written into configured environment files.",
    ifExistsHelp: "Runs only when this path exists.",
    workingDirHelp: "Runs from this directory.",
    workflowCardHelp: "Default review and landing behavior for workflow tasks.",
    reviewCardHelp: "Default Codex native review evidence collection before coordinator pass or landing.",
    pullRequestHelp: "ready means create a PR that is immediately ready for review.",
    landingHelp: "landing means the post-review step that merges accepted work and cleans up the worktree.",
    codexBaseHelp: "required means the coordinator must run /review --base <resolved-parent> in a Codex surface; advisory records evidence when available.",
    agentHelp: "CLI used for new agent work.",
    agentRuntimeHelp: "Agent launch and prompt delivery behavior.",
    issuesHelp: "Issue provider wt uses for issue-backed work.",
    githubUserHelp: "GitHub issue lists are filtered to this user when configured.",
    localSiteHelp: "Local web site integration for new worktrees.",
    siteNameHelp: "Template for the local site name.",
    siteRootHelp: "Directory served as the local site root.",
    siteSecureHelp: "Whether wt uses HTTPS for the local site.",
    siteUrlHelp: "URL template exposed to setup, browser, and injected context.",
    siteTargetHelp: "Target backend for proxy-style site providers.",
    editorHelp: "How wt opens config, task, workflow, and other local files for editing.",
    editorCommandHelp: "Editor command template used when opening a file.",
    editorPlacementHelp: "Where wt opens the editor.",
    workspaceHelp: "Workspace surfaces wt prepares for a new worktree.",
    workspaceTabsHelp: "Tabs opened with each new workspace.",
    postDepsTabsHelp: "Tabs opened after setup/dependency commands finish.",
    colorsHelp: "Color labels used when wt opens workspace tabs.",
    browserHelp: "Browser behavior for opening the workspace URL.",
    chromeDevtoolsHelp: "Chrome DevTools profile used when browser mode is chrome_devtools.",
    userDataDirHelp: "Chrome profile directory template.",
    portHelp: "Fixed debugging port when configured.",
    agentArgsHelp: "Extra CLI arguments passed to the agent process.",
    agentCommandHelp: "Overrides the generated agent launch command.",
    agentReadyHelp: "How wt decides the agent is ready to receive a prompt.",
    agentSubmitHelp: "How wt submits the prompt after sending it.",
    agentTimeoutHelp: "Maximum wait for the agent ready signal.",
    agentSendAfterHelp: "Delay before submitting after the prompt is sent.",
    agentPromptHelp: "Prompt scopes configured for agent startup.",
    testHelp: "Commands reviewers or agents can run for validation.",
    localSettingsSummary: "Private settings for this workspace from .local/.wt.toml.",
    sharedSettingsSummary: "Shared repository defaults from .wt.toml.",
    otherSettingsSummary: "Additional settings loaded for this workspace.",
    selectedProfileSummary: "This profile is currently applied to the effective settings.",
    availableProfileSummary: "This profile is not currently selected.",
    configProfileSummary: "Profile values that can change agent, files, links, local site, or tests.",
    profileSiteLabel: "site",
    configInvalidProfileSummary: "This profile cannot be read and needs attention.",
    renderedEffectiveConfig: "Final TOML",
    workflowPolicy: "Workflow policy",
    configSourceMode: "Applied settings",
    configSourceDefault: "Built-in defaults",
    configSourceLocal: "Local settings only",
    configSourceShared: "Shared settings only",
    configSourceSharedLocal: "Shared + local settings",
    configSourceMultiple: "Multiple config sources",
    sourceLayers: "Source layers",
    selectedProfile: "Selected profile",
    noSelectedProfile: "No selected profile",
    validProfiles: "valid profiles",
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
    workflowRelationships: "Workflow relationship summary",
    workflowRelationshipPreview: "{workflow} {id} - {taskDocuments} - {taskRuns}",
    workflowEntityLabel: "Workflow",
    taskDocumentCountOne: "{count} TaskDocument",
    taskDocumentCountMany: "{count} TaskDocuments",
    taskRunCountOne: "{count} TaskRun",
    taskRunCountMany: "{count} TaskRuns",
    workflowCanvas: "Workflow canvas",
    workflowCanvasAria: "Workflow relationship canvas",
    workflowReadOnly: "Read-only",
    workflowCanvasFit: "Fit",
    workflowCanvasCenter: "Center",
    workflowCanvasSource: "Source",
    workflowCanvasInspector: "Inspector",
    workflowCanvasLegend: "Legend",
    workflowCanvasSolidEdge: "solid: Workflow/task",
    workflowCanvasDashedEdge: "dashed: Agent observation",
    workflowCanvasAttentionEdge: "red: missing or invalid link",
    workflowContainsLabel: "contains",
    workflowObservedByLabel: "observed by",
    workflowModeLabel: "mode",
    workflowBaseLabel: "base",
    workflowPolicyLabel: "policy",
    workflowRunnableLabel: "runnable",
    workflowUpdatedAtLabel: "updated_at",
    taskDocumentLabel: "TaskDocument",
    taskRunLabel: "TaskRun",
    workflowAnchorLabel: "Workflow",
    agentObservationLabel: "Agent",
    agentNotObserved: "not observed",
    missingTaskDocumentLabel: "TaskDocument missing",
    missingTaskRunLabel: "TaskRun missing",
    profileLabel: "profile",
    parentLabel: "parent",
    stackOrderLabel: "stack order",
    batchSiblingLabel: "sibling",
    matrixProfileLabel: "profile run",
    relationshipEmpty: "No linked task rows",
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
    noLocalState: "No personal state records",
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
    statePassed: "Passed",
    stateSkipped: "Skipped",
    stateFailed: "Failed",
    stateError: "State error",
  },
  ko: {
    switchToKorean: "한국어로 전환",
    switchToEnglish: "영어로 전환",
    readFull: "전문 보기",
    collapse: "접기",
    body: "본문",
    source: "원본",
    sourceToml: "원본 TOML",
    effectiveConfig: "최종 적용값",
    workspace: "워크스페이스",
    sourceConfig: "설정 원본 파일",
    localSettings: "로컬 설정",
    sharedSettings: "공유 설정",
    otherSettings: "기타 설정",
    detailSummary: "요약",
    settingCards: "설정 카드",
    detailRelationships: "관련 정보",
    localTomlPath: "로컬 TOML 경로",
    profileTomlPath: "프로필 TOML 경로",
    appliedSettingsLayers: "적용된 설정층",
    settingsPath: "파일 위치",
    settingsToml: "설정 TOML",
    settingsRole: "이 설정의 역할",
    profileToml: "프로필 TOML",
    profileOnlyBadge: "profile.toml",
    profileLocation: "경로",
    sourceContent: "원본 내용",
    renderedContent: "렌더링된 내용",
    sourcePaths: "원본 경로",
    sourceHome: "원본 위치",
    statusLabel: "status",
    kindLabel: "kind",
    tagsLabel: "tags",
    updatedAtLabel: "updated_at",
    outcomeLabel: "outcome",
    dateLabel: "date",
    scopeLabel: "scope",
    specLabel: "spec",
    profileName: "프로필 이름",
    agentLabel: "cli",
    issuesLabel: "이슈",
    siteLabel: "provider",
    pullRequestLabel: "pull_request",
    landingLabel: "landing",
    reviewLabel: "review",
    codexBaseLabel: "codex base",
    worktreePathLabel: "path",
    namingLabel: "naming",
    namingCommandLabel: "command",
    namingBranchLabel: "branch",
    namingWorkspaceLabel: "workspace",
    namingPromptLabel: "prompt",
    copyAsLabel: "copy_as",
    localContextLabel: "inject_local_context",
    setupLabel: "setup",
    depsLabel: "deps",
    envLabel: "env",
    envFilesLabel: "env_files",
    ifExistsLabel: "if_exists",
    workingDirLabel: "working_dir",
    siteNameLabel: "name",
    rootLabel: "root",
    secureLabel: "secure",
    urlLabel: "url",
    targetLabel: "target",
    tabsLabel: "tabs",
    postDepsTabsLabel: "post_deps_tabs",
    colorsLabel: "colors",
    browserLabel: "browser",
    chromeDevtoolsLabel: "chrome_devtools",
    userDataDirLabel: "user_data_dir",
    portLabel: "port",
    commandLabel: "command",
    placementLabel: "placement",
    argsLabel: "args",
    readyLabel: "ready",
    submitLabel: "submit",
    timeoutLabel: "timeout",
    sendAfterLabel: "send_after",
    promptModesLabel: "prompt",
    testLabel: "test",
    copyLabel: "copy",
    githubUserLabel: "gh_user",
    issuesProviderLabel: "provider",
    linkLabel: "link",
    testsLabel: "commands",
    errorLabel: "오류",
    configuredLabel: "설정됨",
    omittedLabel: "생략됨",
    notConfiguredLabel: "설정 없음",
    yes: "예",
    no: "아니오",
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
    noteOverview: "개인 wt 상태와 준비된 워크플로우, 실행 중인 작업, 확인이 필요한 항목을 우선 보여줍니다.",
    noteIdeas: "Idea는 읽기 전용 맥락으로 보여주는 기획 노트입니다.",
    noteRetrospecs: "회고는 spec-local 작업 교훈과 cross-work 학습 기록을 함께 보여줍니다.",
    noteWorkflows: "워크플로우는 파생 상태별로 정렬하고 각 계획 안에 연결된 작업 실행을 묶어 보여줍니다.",
    noteTaskRuns: "작업 실행은 <repo-root>/.wt/execution/task-runs 실행 기록입니다. 실패했거나 연결이 깨진 항목은 확인 필요로 묶습니다.",
    noteProfiles: "프로필은 <repo-root>/.wt/config/profiles의 agent/config overlay입니다.",
    noteConfig: "설정은 최종 적용값, 설정층, 프로필을 보여줍니다.",
    cockpitConfigTitle: "설정 현황",
    cockpitConfigSubtitle: "최종 적용값, 설정층, 프로필을 확인합니다.",
    cockpitWorkflowTitle: "워크플로우 현황",
    cockpitWorkflowSubtitle: "파생 Workflow 상태, 실행 가능한 작업, 연결된 TaskRun, 원본 경로를 비교합니다.",
    cockpitTaskRunTitle: "TaskRun 현황",
    cockpitTaskRunSubtitle: "실행 기록을 상태, branch, context, 원본 경로 기준으로 비교합니다.",
    cockpitIdeasTitle: "아이디어 현황",
    cockpitIdeasSubtitle: "기획 기록의 상태, 원본, 태그, source path를 제목 가까이에 둡니다.",
    cockpitRetrospecsTitle: "회고 현황",
    cockpitRetrospecsSubtitle: "spec-local과 cross-work 회고의 결과, 대상, 날짜, 태그, source path를 먼저 보여줍니다.",
    workflowIndex: "워크플로우 색인",
    taskRunIndex: "TaskRun 색인",
    ideaIndex: "아이디어 색인",
    retrospecIndex: "회고 색인",
    configIndex: "설정 목록",
    configGroupAttention: "확인 필요",
    configGroupCurrentSettings: "현재 설정",
    configAppliedSummary: "아래 색상으로 구분한 설정층을 합쳐 wt가 실제로 사용할 값입니다.",
    worktreeHelp: "새 worktree 안에 준비할 파일과 로컬 컨텍스트입니다.",
    worktreePathHelp: "새 worktree를 만들 위치 템플릿입니다.",
    worktreeCopyHelp: "새 worktree에 복사할 파일입니다.",
    worktreeCopyAsHelp: "새 worktree에서 다른 위치나 이름으로 복사할 파일입니다.",
    worktreeLinkHelp: "복사하지 않고 링크로 연결할 경로입니다.",
    localContextHelp: "site, worktree, parent 정보를 렌더링해 agent 컨텍스트 파일에 주입합니다.",
    namingHelp: "wt가 안정적인 branch/workspace 이름을 만드는 방식입니다.",
    namingCommandHelp: "이름 변수를 생성할 때 실행하는 명령입니다.",
    namingBranchHelp: "생성된 branch 이름에 쓰는 템플릿입니다.",
    namingWorkspaceHelp: "설정되어 있으면 workspace 제목에 쓰는 템플릿입니다.",
    namingPromptHelp: "prompt 템플릿이 설정되어 있습니다. 전문은 TOML 원문에 둡니다.",
    setupHelp: "agent 작업 전에 준비하는 명령과 환경 변수입니다.",
    depsHelp: "조건이 맞을 때 wt가 실행하는 의존성/setup 명령입니다.",
    envHelp: "setup과 workspace 명령에 렌더링되는 환경 변수입니다.",
    envFilesHelp: "설정된 환경 파일에 쓸 값입니다.",
    ifExistsHelp: "이 경로가 있을 때만 실행합니다.",
    workingDirHelp: "이 디렉터리에서 명령을 실행합니다.",
    workflowCardHelp: "워크플로우 작업의 기본 PR 생성과 landing 동작입니다.",
    reviewCardHelp: "coordinator pass 또는 landing 전 Codex native review evidence 수집 기본값입니다.",
    pullRequestHelp: "ready는 PR을 만들 때 바로 리뷰 가능한 상태로 만든다는 뜻입니다.",
    landingHelp: "landing은 리뷰가 끝난 작업을 parent branch에 합치고 worktree를 정리하는 단계입니다.",
    codexBaseHelp: "required는 coordinator가 Codex surface에서 /review --base <resolved-parent>를 반드시 실행한다는 뜻이고, advisory는 가능한 경우 evidence로 남긴다는 뜻입니다.",
    agentHelp: "새 agent 작업을 시작할 때 사용할 CLI입니다.",
    agentRuntimeHelp: "agent 실행과 prompt 전달 방식입니다.",
    issuesHelp: "이슈 기반 작업에서 wt가 사용할 이슈 제공자입니다.",
    githubUserHelp: "설정하면 GitHub 이슈 목록을 이 사용자 기준으로 가져옵니다.",
    localSiteHelp: "새 worktree에서 사용할 로컬 웹사이트 연동입니다.",
    siteNameHelp: "로컬 사이트 이름 템플릿입니다.",
    siteRootHelp: "로컬 사이트 루트로 제공할 디렉터리입니다.",
    siteSecureHelp: "로컬 사이트에 HTTPS를 쓸지 정합니다.",
    siteUrlHelp: "setup, browser, local context에 전달되는 URL 템플릿입니다.",
    siteTargetHelp: "proxy 계열 site provider가 바라볼 대상입니다.",
    editorHelp: "설정, 작업문서, 워크플로우 같은 로컬 파일을 열 때 쓰는 편집 방식입니다.",
    editorCommandHelp: "파일을 열 때 사용할 editor 명령 템플릿입니다.",
    editorPlacementHelp: "editor를 어디에 띄울지 정합니다.",
    workspaceHelp: "새 worktree를 열 때 wt가 준비하는 workspace 화면입니다.",
    workspaceTabsHelp: "새 workspace를 열 때 함께 여는 탭입니다.",
    postDepsTabsHelp: "setup/dependency 명령이 끝난 뒤 여는 탭입니다.",
    colorsHelp: "wt가 workspace 탭을 열 때 쓰는 색상 라벨입니다.",
    browserHelp: "workspace URL을 열 때의 브라우저 동작입니다.",
    chromeDevtoolsHelp: "browser mode가 chrome_devtools일 때 쓰는 Chrome 프로필입니다.",
    userDataDirHelp: "Chrome 프로필 디렉터리 템플릿입니다.",
    portHelp: "설정되어 있으면 고정 디버깅 포트로 씁니다.",
    agentArgsHelp: "agent 프로세스에 추가로 전달할 CLI 인자입니다.",
    agentCommandHelp: "자동 생성되는 agent 실행 명령을 대체합니다.",
    agentReadyHelp: "wt가 prompt를 보내도 된다고 판단하는 기준입니다.",
    agentSubmitHelp: "prompt를 보낸 뒤 어떤 키 입력으로 제출할지 정합니다.",
    agentTimeoutHelp: "agent 준비 신호를 기다리는 최대 시간입니다.",
    agentSendAfterHelp: "prompt를 보낸 뒤 제출하기 전 대기 시간입니다.",
    agentPromptHelp: "agent 시작 prompt가 적용되는 작업 범위입니다.",
    testHelp: "리뷰어나 agent가 검증에 사용할 수 있는 명령입니다.",
    localSettingsSummary: ".local/.wt.toml에서 현재 worktree에만 적용되는 설정입니다.",
    sharedSettingsSummary: ".wt.toml에서 저장소와 함께 공유되는 기본 설정입니다.",
    otherSettingsSummary: "현재 worktree에 추가로 적용된 설정입니다.",
    selectedProfileSummary: "현재 적용된 프로필입니다.",
    availableProfileSummary: "현재 적용되지는 않은 프로필입니다.",
    configProfileSummary: "이 프로필이 에이전트, 복사/링크 파일, 로컬 사이트, 테스트를 어떻게 바꾸는지 보여줍니다.",
    profileSiteLabel: "site",
    configInvalidProfileSummary: "읽을 수 없는 프로필입니다. 이 항목은 확인이 필요합니다.",
    renderedEffectiveConfig: "최종 TOML",
    workflowPolicy: "워크플로우 정책",
    configSourceMode: "적용 설정",
    configSourceDefault: "내장 기본값",
    configSourceLocal: "로컬 설정만",
    configSourceShared: "공유 설정만",
    configSourceSharedLocal: "공유 + 로컬 설정",
    configSourceMultiple: "여러 설정 원본",
    sourceLayers: "설정 원본",
    selectedProfile: "선택된 프로필",
    noSelectedProfile: "선택된 프로필 없음",
    validProfiles: "정상 프로필",
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
    workflowRelationships: "Workflow 관계 요약",
    workflowRelationshipPreview: "{workflow} {id} - {taskDocuments} - {taskRuns}",
    workflowEntityLabel: "Workflow",
    taskDocumentCountOne: "TaskDocument {count}개",
    taskDocumentCountMany: "TaskDocument {count}개",
    taskRunCountOne: "TaskRun {count}개",
    taskRunCountMany: "TaskRun {count}개",
    workflowCanvas: "Workflow 캔버스",
    workflowCanvasAria: "Workflow 관계 캔버스",
    workflowReadOnly: "읽기 전용",
    workflowCanvasFit: "맞춤",
    workflowCanvasCenter: "중앙",
    workflowCanvasSource: "원본",
    workflowCanvasInspector: "검사",
    workflowCanvasLegend: "범례",
    workflowCanvasSolidEdge: "실선: Workflow/task",
    workflowCanvasDashedEdge: "점선: Agent 관찰",
    workflowCanvasAttentionEdge: "빨강: 누락 또는 오류 링크",
    workflowContainsLabel: "포함",
    workflowObservedByLabel: "관찰",
    workflowModeLabel: "mode",
    workflowBaseLabel: "base",
    workflowPolicyLabel: "policy",
    workflowRunnableLabel: "runnable",
    workflowUpdatedAtLabel: "updated_at",
    taskDocumentLabel: "TaskDocument",
    taskRunLabel: "TaskRun",
    workflowAnchorLabel: "Workflow",
    agentObservationLabel: "Agent",
    agentNotObserved: "관찰 없음",
    missingTaskDocumentLabel: "TaskDocument 누락",
    missingTaskRunLabel: "TaskRun 누락",
    profileLabel: "profile",
    parentLabel: "parent",
    stackOrderLabel: "stack 순서",
    batchSiblingLabel: "동시 항목",
    matrixProfileLabel: "profile 실행",
    relationshipEmpty: "연결된 작업 행이 없습니다",
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
    statePassed: "통과",
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
content.addEventListener("click", handleMasterDetailSelection);
content.addEventListener("click", handleWorkflowCanvasControl);
content.addEventListener("pointerdown", handleWorkflowCanvasPointerDown);
content.addEventListener("keydown", handleWorkflowCanvasNodeKeydown);
content.addEventListener("keydown", handleMasterDetailKeydown);

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

function compactPath(path) {
  if (!path) return "";
  return path.replace(/^\/Users\/[^/]+/, "~");
}

function applyLocale() {
  document.documentElement.lang = state.locale;
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

function handleMasterDetailSelection(event) {
  const button = event.target.closest("[data-md-record]");
  if (!button || !content.contains(button)) {
    return;
  }
  selectMasterDetailRecord(button.dataset.mdTab, button.dataset.mdRecord, { focusRecord: true });
}

function handleWorkflowCanvasControl(event) {
  const control = event.target.closest("[data-workflow-canvas-control]");
  if (!control || !content.contains(control)) {
    return;
  }
  const canvas = control.closest("[data-workflow-canvas]");
  const viewport = canvas?.querySelector(".workflow-canvas-viewport");
  if (!canvas || !viewport) {
    return;
  }

  event.preventDefault();
  const action = control.dataset.workflowCanvasControl;
  if (action === "fit") {
    viewport.scrollTo({ left: 0, top: 0, behavior: "smooth" });
    viewport.focus({ preventScroll: true });
    return;
  }
  if (action === "center") {
    const anchor = canvas.querySelector(".workflow-canvas-node.is-workflow");
    if (anchor) {
      anchor.focus({ preventScroll: true });
      anchor.scrollIntoView({ block: "nearest", inline: "center", behavior: "smooth" });
    }
    return;
  }
  if (action === "source") {
    const source = canvas.querySelector(".workflow-canvas-inspector details");
    const summary = source?.querySelector("summary");
    if (source) {
      source.open = true;
    }
    if (summary) {
      summary.focus({ preventScroll: false });
    }
  }
}

function handleWorkflowCanvasPointerDown(event) {
  if (event.button !== 0) {
    return;
  }
  const node = event.target.closest(".workflow-canvas-node");
  if (!node || !content.contains(node)) {
    return;
  }
  const plane = node.closest(".workflow-canvas-plane");
  if (!plane) {
    return;
  }

  event.preventDefault();
  node.focus({ preventScroll: true });
  const drag = {
    node,
    plane,
    pointerId: event.pointerId,
    startClientX: event.clientX,
    startClientY: event.clientY,
    startX: workflowCanvasNodeLeft(node),
    startY: workflowCanvasNodeTop(node),
  };
  node.classList.add("is-dragging");
  node.setPointerCapture?.(event.pointerId);
  node.addEventListener("pointermove", handleWorkflowCanvasPointerMove);
  node.addEventListener("pointerup", handleWorkflowCanvasPointerEnd);
  node.addEventListener("pointercancel", handleWorkflowCanvasPointerEnd);
  node._workflowCanvasDrag = drag;
}

function handleWorkflowCanvasPointerMove(event) {
  const drag = event.currentTarget._workflowCanvasDrag;
  if (!drag || event.pointerId !== drag.pointerId) {
    return;
  }
  event.preventDefault();
  const nextX = drag.startX + event.clientX - drag.startClientX;
  const nextY = drag.startY + event.clientY - drag.startClientY;
  workflowCanvasMoveNode(drag.node, drag.plane, nextX, nextY, { avoidOverlap: true });
}

function handleWorkflowCanvasPointerEnd(event) {
  const node = event.currentTarget;
  const drag = node._workflowCanvasDrag;
  if (drag && event.pointerId === drag.pointerId) {
    node.releasePointerCapture?.(event.pointerId);
  }
  node.classList.remove("is-dragging", "is-blocked");
  node.removeEventListener("pointermove", handleWorkflowCanvasPointerMove);
  node.removeEventListener("pointerup", handleWorkflowCanvasPointerEnd);
  node.removeEventListener("pointercancel", handleWorkflowCanvasPointerEnd);
  delete node._workflowCanvasDrag;
}

function handleWorkflowCanvasNodeKeydown(event) {
  const node = event.target.closest(".workflow-canvas-node");
  if (!node || !content.contains(node)) {
    return;
  }
  const keyMoves = {
    ArrowLeft: [-1, 0],
    ArrowRight: [1, 0],
    ArrowUp: [0, -1],
    ArrowDown: [0, 1],
  };
  const move = keyMoves[event.key];
  if (!move) {
    return;
  }
  const plane = node.closest(".workflow-canvas-plane");
  if (!plane) {
    return;
  }
  event.preventDefault();
  const step = event.shiftKey ? 48 : 12;
  workflowCanvasMoveNode(node, plane, workflowCanvasNodeLeft(node) + move[0] * step, workflowCanvasNodeTop(node) + move[1] * step, { avoidOverlap: true });
}

function workflowCanvasMoveNode(node, plane, left, top, options = {}) {
  const next = workflowCanvasClampedPosition(node, plane, left, top);
  if (options.avoidOverlap && workflowCanvasOverlaps(node, plane, next.left, next.top)) {
    node.classList.add("is-blocked");
    return false;
  }
  node.classList.remove("is-blocked");
  node.style.left = `${Math.round(next.left)}px`;
  node.style.top = `${Math.round(next.top)}px`;
  workflowCanvasUpdateEdges(plane);
  return true;
}

function workflowCanvasClampedPosition(node, plane, left, top) {
  const maxLeft = Math.max(0, plane.offsetWidth - node.offsetWidth);
  const maxTop = Math.max(0, plane.offsetHeight - node.offsetHeight);
  return {
    left: Math.min(Math.max(0, left), maxLeft),
    top: Math.min(Math.max(0, top), maxTop),
  };
}

function workflowCanvasOverlaps(node, plane, left, top) {
  const padding = 12;
  const next = {
    left: left - padding,
    right: left + node.offsetWidth + padding,
    top: top - padding,
    bottom: top + node.offsetHeight + padding,
  };
  return Array.from(plane.querySelectorAll(".workflow-canvas-node")).some((other) => {
    if (other === node) {
      return false;
    }
    const otherLeft = workflowCanvasNodeLeft(other);
    const otherTop = workflowCanvasNodeTop(other);
    const box = {
      left: otherLeft,
      right: otherLeft + other.offsetWidth,
      top: otherTop,
      bottom: otherTop + other.offsetHeight,
    };
    return next.left < box.right && next.right > box.left && next.top < box.bottom && next.bottom > box.top;
  });
}

function workflowCanvasUpdateEdges(plane) {
  const nodesById = new Map(
    Array.from(plane.querySelectorAll(".workflow-canvas-node[data-workflow-canvas-node]")).map((node) => [node.dataset.workflowCanvasNode, node])
  );
  plane.querySelectorAll(".workflow-canvas-edge[data-edge-from][data-edge-to]").forEach((edge) => {
    const from = nodesById.get(edge.dataset.edgeFrom);
    const to = nodesById.get(edge.dataset.edgeTo);
    if (!from || !to) {
      return;
    }
    edge.setAttribute("d", workflowCanvasEdgePath(workflowCanvasElementEdgePoints(from, to)));
  });
}

function workflowCanvasElementEdgePoints(from, to) {
  return workflowCanvasEdgePoints(workflowCanvasElementBox(from), workflowCanvasElementBox(to));
}

function workflowCanvasElementBox(node) {
  return {
    x: workflowCanvasNodeLeft(node),
    y: workflowCanvasNodeTop(node),
    w: node.offsetWidth,
    h: node.offsetHeight,
  };
}

function workflowCanvasNodeLeft(node) {
  return Number.parseFloat(node.style.left || "0") || 0;
}

function workflowCanvasNodeTop(node) {
  return Number.parseFloat(node.style.top || "0") || 0;
}

function handleMasterDetailKeydown(event) {
  const button = event.target.closest("[data-md-record]");
  if (!button || !content.contains(button)) {
    return;
  }

  const list = button.closest("[data-md-list]");
  if (!list) {
    return;
  }

  const rows = Array.from(list.querySelectorAll("[data-md-record]"));
  const currentIndex = rows.indexOf(button);
  if (currentIndex < 0) {
    return;
  }

  let nextIndex = currentIndex;
  if (event.key === "ArrowDown") {
    nextIndex = Math.min(rows.length - 1, currentIndex + 1);
  } else if (event.key === "ArrowUp") {
    nextIndex = Math.max(0, currentIndex - 1);
  } else if (event.key === "Home") {
    nextIndex = 0;
  } else if (event.key === "End") {
    nextIndex = rows.length - 1;
  } else {
    return;
  }

  event.preventDefault();
  const next = rows[nextIndex];
  selectMasterDetailRecord(next.dataset.mdTab, next.dataset.mdRecord, { focusRecord: true });
}

function selectMasterDetailRecord(tabKey, recordId, options = {}) {
  if (!tabKey || !recordId) {
    return;
  }
  state.selection[tabKey] = recordId;
  render();
  if (options.focusRecord) {
    focusMasterDetailRecord(tabKey, recordId);
  }
}

function focusMasterDetailRecord(tabKey, recordId) {
  const selected = Array.from(content.querySelectorAll("[data-md-record][data-md-tab]")).find(
    (row) => row.dataset.mdRecord === recordId && row.dataset.mdTab === tabKey
  );
  if (selected) {
    selected.focus();
  }
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
  workspaceLabel.textContent = "wt ui";
  document.title = "wt ui";
  repoLabel.textContent = repoContextLabel(snapshot.repo);
  repoLabel.title = snapshot.repo.root || "";
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

function repoContextLabel(repo) {
  const parts = [repo.name, compactPath(repo.root)].filter(Boolean);
  return parts.join(" · ");
}

function configSourceLabel(config) {
  if (config.source === "default") return t("configSourceDefault");
  const kinds = (config.paths || []).map(settingsFileKind);
  if (kinds.includes("shared") && kinds.includes("local")) return t("configSourceSharedLocal");
  if (kinds.length === 1 && kinds[0] === "local") return t("configSourceLocal");
  if (kinds.length === 1 && kinds[0] === "shared") return t("configSourceShared");
  if (config.source === "files") return t("configSourceMultiple");
  return config.source;
}

function localSiteValue(site) {
  if (!site) {
    return t("notConfiguredLabel");
  }
  if (!site.active) {
    return `${site.provider} (${t("notConfiguredLabel")})`;
  }
  return site.provider;
}

function configuredValue(value) {
  return value || t("notConfiguredLabel");
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
  content.innerHTML = configCockpit(snapshot);
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
    pill(`${row.policy.pull_request}/${row.policy.landing}/review:${row.policy.review_codex_base}`, "amber"),
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
    pill(`${t("agentLabel")} ${row.agent}`, "blue"),
    valuesPill(t("copyLabel"), profileCopyValues(row)),
    valuesPill(t("copyAsLabel"), profileCopyAsValues(row)),
    valuesPill(t("linkLabel"), profileLinkValues(row)),
    row.has_site ? pill(t("profileSiteLabel"), "green") : "",
    row.test_count ? pill(`${row.test_count} ${t("testsLabel")}`, "amber") : "",
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
  const invalidProfileCount = snapshot.profiles.invalid.length;
  const stats = [attentionStat(invalidProfileCount)];
  return masterDetailPanel({
    id: "config-cockpit",
    tabKey: "config",
    title: t("cockpitConfigTitle"),
    subtitle: t("cockpitConfigSubtitle"),
    stats,
    listTitle: t("configIndex"),
    records: configMasterDetailRecords(snapshot),
    emptyText: t("noConfigSummary"),
    showCount: false,
  });
}

function configMasterDetailRecords(snapshot) {
  const config = snapshot.config;
  const selectedProfileName = config.selected_profile || "";
  const profileRecords = snapshot.profiles.items
    .slice()
    .sort((a, b) => {
      const selectedDiff = Number(b.name === selectedProfileName) - Number(a.name === selectedProfileName);
      return selectedDiff || String(a.name).localeCompare(String(b.name));
    })
    .map((row) => profileMasterDetailRecord(row, row.name === selectedProfileName));
  return snapshot.profiles.invalid
    .map(invalidProfileMasterDetailRecord)
    .concat(configEffectiveRecord(config))
    .concat(profileRecords)
    .filter(Boolean);
}

function configEffectiveRecord(config) {
  return {
    id: "config-effective",
    group: t("configGroupCurrentSettings"),
    kicker: "",
    title: t("effectiveConfig"),
    listPills: [
      pill(`${t("pullRequestLabel")} ${config.workflow.pull_request}`, "green"),
      pill(`${t("landingLabel")} ${config.workflow.landing}`, "amber"),
      config.review ? pill(`${t("reviewLabel")} ${config.review.codex_base}`, "violet") : "",
      ...configSourceLayerPills(config),
    ],
    summary: t("configAppliedSummary"),
    pills: [
      pill(`${t("pullRequestLabel")} ${config.workflow.pull_request}`, "green"),
      pill(`${t("landingLabel")} ${config.workflow.landing}`, "amber"),
      config.review ? pill(`${t("reviewLabel")} ${config.review.codex_base}`, "violet") : "",
      ...configSourceLayerPills(config),
      config.selected_profile ? pill(`${t("selectedProfile")}: ${config.selected_profile}`, "violet") : "",
    ],
    paths: [],
    hideSummarySectionTitle: true,
    cards: configEffectiveCards(config),
    fields: [],
    relationshipsSectionTitle: t("localTomlPath"),
    relationships: config.paths.slice().sort((a, b) => settingsFileOrder(a) - settingsFileOrder(b)).map(sourceLayerField),
    hideSourceSectionTitle: true,
    collapseSources: true,
    sources: [{ label: t("renderedEffectiveConfig"), text: config.effective_text, kind: "source" }],
  };
}

function configEffectiveCards(config, options = {}) {
  const includeWorkflow = options.includeWorkflow !== false;
  const includeIssues = options.includeIssues !== false;
  const includeEditor = options.includeEditor !== false;
  const worktree = config.worktree;
  const setup = config.setup;
  const workspace = config.workspace;
  const agent = config.agent;
  const cards = [];
  if (worktree) {
    const worktreeItems = [];
    if (worktree.path) {
      worktreeItems.push({
        label: t("worktreePathLabel"),
        value: worktree.path,
        description: t("worktreePathHelp"),
      });
    }
    if (worktree.copy?.length) {
      worktreeItems.push({
        label: t("copyLabel"),
        value: joinValues(worktree.copy),
        description: t("worktreeCopyHelp"),
      });
    }
    if (worktree.copy_as?.length) {
      worktreeItems.push({
        label: t("copyAsLabel"),
        value: joinValues(worktree.copy_as.map(copyAsValue)),
        description: t("worktreeCopyAsHelp"),
      });
    }
    if (worktree.link?.length) {
      worktreeItems.push({
        label: t("linkLabel"),
        value: joinValues(worktree.link),
        description: t("worktreeLinkHelp"),
      });
    }
    if (worktree.inject_local_context) {
      worktreeItems.push({
        label: t("localContextLabel"),
        value: t("configuredLabel"),
        description: t("localContextHelp"),
      });
    }
    if (worktree.naming) {
      worktreeItems.push({
        label: t("namingLabel"),
        value: worktreeNamingValue(worktree.naming),
        description: t("namingHelp"),
      });
      worktreeItems.push(...worktreeNamingItems(worktree.naming));
    }
    if (worktreeItems.length) {
      cards.push({
        kicker: "[worktree]",
        value: "",
        description: t("worktreeHelp"),
        tone: "blue",
        items: worktreeItems,
      });
    }
  }
  if (setup) {
    const setupItems = [];
    if (setup.deps?.length) {
      setupItems.push({
        label: t("depsLabel"),
        value: setup.deps.map(commandSummaryValue).join(", "),
        description: t("depsHelp"),
      });
    }
    if (setup.env?.length) {
      setupItems.push({
        label: t("envLabel"),
        value: setup.env.map(keyValueSummary).join(", "),
        description: t("envHelp"),
      });
    }
    if (setup.env_files?.length) {
      setupItems.push({
        label: t("envFilesLabel"),
        value: setup.env_files.map(envFileSummaryValue).join(", "),
        description: t("envFilesHelp"),
      });
    }
    if (setupItems.length) {
      cards.push({
        kicker: "[setup]",
        value: "",
        description: t("setupHelp"),
        tone: "amber",
        items: setupItems,
      });
    }
  }
  if (includeWorkflow && config.workflow) {
    cards.push({
      kicker: "[workflow]",
      value: "",
      description: t("workflowCardHelp"),
      tone: "green",
      items: [
        { label: t("pullRequestLabel"), value: config.workflow.pull_request, description: t("pullRequestHelp") },
        { label: t("landingLabel"), value: config.workflow.landing, description: t("landingHelp") },
      ],
    });
  }
  if (config.review) {
    cards.push({
      kicker: "[review]",
      value: "",
      description: t("reviewCardHelp"),
      tone: "amber",
      items: [
        { label: t("codexBaseLabel"), value: config.review.codex_base, description: t("codexBaseHelp") },
      ],
    });
  }
  const issues = typeof config.issues === "string" ? { provider: config.issues } : config.issues;
  if (includeIssues && issues) {
    const issueItems = [
      { label: t("issuesProviderLabel"), value: issues.provider, description: t("issuesHelp") },
      issues.gh_user
        ? { label: t("githubUserLabel"), value: issues.gh_user, description: t("githubUserHelp") }
        : null,
    ].filter(Boolean);
    cards.push({
      kicker: "[issues]",
      value: "",
      description: t("issuesHelp"),
      tone: "blue",
      items: issueItems,
    });
  }
  if (config.site) {
    const siteItems = [
      { label: t("siteLabel"), value: config.site.provider, description: t("localSiteHelp") },
      { label: t("siteNameLabel"), value: config.site.name, description: t("siteNameHelp") },
      { label: t("rootLabel"), value: config.site.root, description: t("siteRootHelp") },
      { label: t("secureLabel"), value: yesNo(config.site.secure), description: t("siteSecureHelp") },
      { label: t("urlLabel"), value: config.site.url, description: t("siteUrlHelp") },
      config.site.target ? { label: t("targetLabel"), value: config.site.target, description: t("siteTargetHelp") } : null,
    ].filter(Boolean);
    cards.push({
      kicker: "[site]",
      value: "",
      description: t("localSiteHelp"),
      tone: config.site.active ? "green" : "amber",
      items: siteItems,
    });
  }
  if (includeEditor && config.editor) {
    const editorItems = [];
    if (config.editor.command) {
      editorItems.push({
        label: t("commandLabel"),
        value: config.editor.command,
        description: t("editorCommandHelp"),
      });
    }
    if (config.editor.placement) {
      editorItems.push({
        label: t("placementLabel"),
        value: config.editor.placement,
        description: t("editorPlacementHelp"),
      });
    }
    cards.push({
      kicker: "[editor]",
      value: "",
      description: t("editorHelp"),
      tone: "amber",
      items: editorItems,
    });
  }
  if (workspace) {
    const workspaceItems = [];
    if (workspace.tabs?.length) {
      workspaceItems.push({
        label: t("tabsLabel"),
        value: workspace.tabs.join(", "),
        description: t("workspaceTabsHelp"),
      });
    }
    if (workspace.post_deps_tabs?.length) {
      workspaceItems.push({
        label: t("postDepsTabsLabel"),
        value: workspace.post_deps_tabs.join(", "),
        description: t("postDepsTabsHelp"),
      });
    }
    if (workspace.colors?.length) {
      workspaceItems.push({
        label: t("colorsLabel"),
        value: workspace.colors.map((row) => `${row.kind}: ${row.color}`).join(", "),
        description: t("colorsHelp"),
        swatches: workspace.colors,
      });
    }
    if (workspace.browser) {
      workspaceItems.push({
        label: t("browserLabel"),
        value: browserSummaryValue(workspace.browser),
        description: t("browserHelp"),
      });
    }
    if (workspace.chrome_devtools) {
      workspaceItems.push({
        label: t("chromeDevtoolsLabel"),
        value: chromeDevtoolsValue(workspace.chrome_devtools),
        description: t("chromeDevtoolsHelp"),
      });
    }
    if (workspaceItems.length) {
      cards.push({
        kicker: "[workspace]",
        value: "",
        description: t("workspaceHelp"),
        tone: "amber",
        items: workspaceItems,
      });
    }
  }
  if (agent) {
    const agentItems = [
      { label: t("agentLabel"), value: agent.cli, description: t("agentHelp") },
    ];
    if (agent.args?.length) {
      agentItems.push({
        label: t("argsLabel"),
        value: joinValues(agent.args),
        description: t("agentArgsHelp"),
      });
    }
    if (agent.command) {
      agentItems.push({
        label: t("commandLabel"),
        value: agent.command,
        description: t("agentCommandHelp"),
      });
    }
    agentItems.push(
      { label: t("readyLabel"), value: agent.ready, description: t("agentReadyHelp") },
      { label: t("submitLabel"), value: agent.submit, description: t("agentSubmitHelp") },
      { label: t("timeoutLabel"), value: `${agent.timeout}s`, description: t("agentTimeoutHelp") },
      { label: t("sendAfterLabel"), value: `${agent.send_after}s`, description: t("agentSendAfterHelp") },
    );
    if (agent.prompt_modes?.length) {
      agentItems.push({
        label: t("promptModesLabel"),
        value: agentPromptSummary(agent),
        description: t("agentPromptHelp"),
      });
    }
    cards.push({
      kicker: "[agent]",
      value: "",
      description: t("agentRuntimeHelp"),
      tone: "blue",
      items: agentItems,
    });
  }
  if (config.test?.commands?.length) {
    cards.push({
      kicker: "[test]",
      value: "",
      description: t("testHelp"),
      tone: "green",
      items: config.test.commands.map((command) => ({
        label: "run",
        value: commandSummaryValue(command),
        description: commandConditionText(command) || t("testHelp"),
      })),
    });
  }
  return cards;
}

function profileEffectiveCards(row) {
  return configEffectiveCards({
    worktree: row.worktree,
    setup: row.setup,
    site: row.site,
    workspace: row.workspace,
    agent: row.agent_settings,
    test: row.test,
  }, { includeWorkflow: false, includeIssues: false, includeEditor: false })
    .map(profileTomlCard);
}

function profileTomlCard(card) {
  return {
    ...card,
    emphasis: "profile-only",
  };
}

function joinValues(values) {
  return values.filter(Boolean).join(", ");
}

function copyAsValue(entry) {
  return `${entry.from} -> ${entry.to}`;
}

function shortValues(values, maxItems = 2) {
  const clean = values.filter(Boolean);
  if (clean.length <= maxItems) {
    return clean.join(", ");
  }
  return `${clean.slice(0, maxItems).join(", ")} +${clean.length - maxItems}`;
}

function profileCopyValues(row) {
  return row.copy || [];
}

function profileCopyAsValues(row) {
  return (row.copy_as || []).map(copyAsValue);
}

function profileLinkValues(row) {
  return row.link || [];
}

function valuesPill(label, values, tone = "") {
  const value = shortValues(values);
  return value ? pill(`${label} ${value}`, tone) : "";
}

function configSourceLayerPills(config) {
  return config.paths
    .slice()
    .sort((a, b) => settingsFileOrder(a) - settingsFileOrder(b))
    .map((path, index) => pill(sourceLayerLabel(path, index), settingsLayerTone(path)));
}

function worktreeNamingValue(naming) {
  return [
    naming.command ? `${t("commandLabel")} ${naming.command}` : "",
    naming.branch ? `${t("namingBranchLabel")} ${naming.branch}` : "",
  ].filter(Boolean).join(", ");
}

function worktreeNamingItems(naming) {
  return [
    naming.command ? { label: t("namingCommandLabel"), value: naming.command, description: t("namingCommandHelp") } : null,
    naming.branch ? { label: t("namingBranchLabel"), value: naming.branch, description: t("namingBranchHelp") } : null,
    naming.workspace ? { label: t("namingWorkspaceLabel"), value: naming.workspace, description: t("namingWorkspaceHelp") } : null,
    naming.prompt_configured ? { label: t("namingPromptLabel"), value: t("configuredLabel"), description: t("namingPromptHelp") } : null,
  ].filter(Boolean);
}

function keyValueSummary(row) {
  return `${row.key}=${row.value}`;
}

function envFileSummaryValue(row) {
  return `${row.path}: ${row.values.map(keyValueSummary).join(", ")}`;
}

function commandSummaryValue(command) {
  return [command.run, commandConditionText(command)].filter(Boolean).join(" · ");
}

function commandConditionText(command) {
  return [
    command.working_dir ? `${t("workingDirLabel")} ${command.working_dir}` : "",
    command.if_exists ? `${t("ifExistsLabel")} ${command.if_exists}` : "",
  ].filter(Boolean).join(", ");
}

function browserSummaryValue(browser) {
  return [
    browser.mode ? `mode ${browser.mode}` : "",
    browser.url ? `url ${browser.url}` : "",
    browser.app ? `app ${browser.app}` : "",
  ].filter(Boolean).join(" · ");
}

function chromeDevtoolsValue(chrome) {
  return [
    chrome.user_data_dir ? `${t("userDataDirLabel")} ${chrome.user_data_dir}` : "",
    chrome.port ? `${t("portLabel")} ${chrome.port}` : "",
  ].filter(Boolean).join(", ");
}

function agentPromptSummary(agent) {
  if (agent.prompt_counts?.length) {
    return agent.prompt_counts.map((row) => `${row.mode} ${row.count}`).join(", ");
  }
  return joinValues(agent.prompt_modes || []);
}

function settingsFileKind(path) {
  if (path === ".local/.wt.toml") return "local";
  if (path === ".wt.toml") return "shared";
  return "other";
}

function settingsFileOrder(path) {
  const kind = settingsFileKind(path);
  if (kind === "local") return 0;
  if (kind === "shared") return 1;
  return 2;
}

function sourceLayerLabel(path, index) {
  const kind = settingsFileKind(path);
  if (kind === "local") return t("localSettings");
  if (kind === "shared") return t("sharedSettings");
  return `${t("sourceLayers")} ${index + 1}`;
}

function settingsLayerTone(path) {
  const kind = settingsFileKind(path);
  if (kind === "local") return "layer-local";
  if (kind === "shared") return "layer-shared";
  return "layer-other";
}

function sourceLayerField(path, index) {
  return {
    label: sourceLayerLabel(path, index),
    value: path,
    tone: settingsLayerTone(path),
  };
}

function profileMasterDetailRecord(row, selected) {
  return {
    id: `profile-${row.name}`,
    group: t("profiles"),
    kicker: selected ? t("selectedProfile") : t("profiles"),
    title: row.name,
    listMarker: selected ? t("selectedProfile") : "",
    listKicker: "",
    listPills: [
      pill(`${t("agentLabel")} ${row.agent}`, "blue"),
      valuesPill(t("copyLabel"), profileCopyValues(row)),
      valuesPill(t("copyAsLabel"), profileCopyAsValues(row)),
      valuesPill(t("linkLabel"), profileLinkValues(row)),
      row.has_site ? pill(t("profileSiteLabel"), "green") : "",
      row.test_count ? pill(`${row.test_count} ${t("testsLabel")}`, "amber") : "",
    ],
    summary: selected ? t("selectedProfileSummary") : t("availableProfileSummary"),
    pills: [
      selected ? pill(t("selectedProfile"), "violet") : "",
      pill(t("profileOnlyBadge"), "green"),
      pill(`${t("agentLabel")} ${row.agent}`, "blue"),
      valuesPill(t("copyLabel"), profileCopyValues(row)),
      valuesPill(t("copyAsLabel"), profileCopyAsValues(row)),
      valuesPill(t("linkLabel"), profileLinkValues(row)),
      row.has_site ? pill(t("profileSiteLabel"), "green") : "",
      row.test_count ? pill(`${row.test_count} ${t("testsLabel")}`, "amber") : "",
    ],
    paths: [],
    hideSummarySectionTitle: true,
    cards: profileEffectiveCards(row),
    fields: [],
    relationshipsSectionTitle: t("profileTomlPath"),
    relationships: [{ label: t("profileLocation"), value: row.path }],
    hideSourceSectionTitle: true,
    collapseSources: true,
    sources: [{ label: t("profileToml"), text: row.source_text, kind: "source" }],
  };
}

function invalidProfileMasterDetailRecord(row) {
  return {
    id: `invalid-profile-${row.key}`,
    group: t("configGroupAttention"),
    tone: "red",
    needsAttention: true,
    kicker: t("invalidProfiles"),
    listKicker: "",
    title: row.key,
    summary: t("configInvalidProfileSummary"),
    pills: [pill(t("invalid"), "red")],
    paths: [],
    summarySectionTitle: t("needsAttention"),
    fields: [
      { label: t("profileName"), value: row.key },
      { label: t("errorLabel"), value: row.error },
    ],
    relationshipsSectionTitle: t("profileTomlPath"),
    relationships: [{ label: t("profileLocation"), value: row.path }],
    hideSourceSectionTitle: true,
    collapseSources: true,
    sources: [{ label: t("profileToml"), text: [row.error, row.source_text].filter(Boolean).join("\n\n"), kind: "source" }],
  };
}

function workflowsCockpit(snapshot) {
  const workflows = sortedWorkflows(snapshot.workflows.items);
  const groups = countBy(workflows, workflowUiGroup);
  const records = workflows
    .map(workflowMasterDetailRecord)
    .concat(snapshot.workflows.invalid.map(invalidWorkflowMasterDetailRecord));
  const attentionCount = (groups.needs_attention || 0) + snapshot.workflows.invalid.length;
  const stats = [
    attentionStat(attentionCount),
    { label: t("runningTaskRuns"), value: groups.running || 0 },
    { label: t("preparedWorkflows"), value: groups.prepared || 0 },
    { label: t("metricWorkflows"), value: workflows.length },
  ];
  return masterDetailPanel({
    id: "workflow-cockpit",
    tabKey: "workflows",
    title: t("cockpitWorkflowTitle"),
    subtitle: t("cockpitWorkflowSubtitle"),
    stats,
    listTitle: t("workflowIndex"),
    records,
    emptyText: t("noWorkflows"),
  });
}

function workflowMasterDetailRecord(row) {
  const group = workflowUiGroup(row);
  const needsAttention = group === "needs_attention";
  return {
    id: `workflow-${row.id}`,
    group: stateLabel(group),
    tone: needsAttention ? "red" : "",
    needsAttention,
    kicker: `Workflow ${row.id}`,
    listKicker: "",
    title: row.title,
    listPills: workflowPills(row, group),
    summary: row.state_error || workflowRelationshipPreview(row) || row.body_summary || "",
    pills: workflowPills(row, group),
    paths: [row.path],
    summarySectionTitle: t("workflowRelationships"),
    summaryHtml: workflowRelationshipSummary(row),
    canvasSectionTitle: t("workflowCanvas"),
    canvasHtml: workflowCanvasSection(row),
    fields: workflowFactFields(row),
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: workflowSourceFields(row),
    collapseSources: true,
    sources: [
      { label: t("body"), text: row.body, kind: "prose" },
      { label: t("sourceToml"), text: row.source_text, kind: "source" },
    ],
  };
}

function invalidWorkflowMasterDetailRecord(row) {
  return {
    id: `invalid-workflow-${row.key}`,
    group: t("needsAttention"),
    tone: "red",
    needsAttention: true,
    kicker: t("invalidWorkflows"),
    listKicker: "",
    title: row.key,
    summary: row.error,
    pills: [pill(t("invalid"), "red")],
    paths: [row.path],
    summarySectionTitle: t("needsAttention"),
    fields: [
      { label: "Workflow", value: row.key },
      { label: t("errorLabel"), value: row.error },
    ],
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: [{ label: t("source"), value: row.path, tone: "red" }],
    collapseSources: true,
    sources: [{ label: t("sourceToml"), text: [row.error, row.source_text].filter(Boolean).join("\n\n"), kind: "source" }],
  };
}

function workflowPills(row, group = workflowUiGroup(row)) {
  return [
    pill(stateLabel(group), groupColor(group)),
    pill(row.mode, "blue"),
    pill(`${row.task_runs.total} ${t("metricTaskRuns")}`),
    row.runnable.runnable_count ? pill(`${row.runnable.runnable_count} runnable`, "green") : "",
    row.task_runs.running ? pill(`${row.task_runs.running} ${stateLabel("running").toLowerCase()}`, "green") : "",
    row.task_runs.failed ? pill(`${row.task_runs.failed} ${stateLabel("failed").toLowerCase()}`, "red") : "",
    row.task_runs.missing ? pill(`${row.task_runs.missing} missing`, "red") : "",
    row.profile ? pill(`${t("profileLabel")} ${row.profile}`, "violet") : "",
    row.profiles.length ? pill(`${row.profiles.length} profiles`, "violet") : "",
    pill(`${row.policy.pull_request}/${row.policy.landing}/review:${row.policy.review_codex_base}`, "amber"),
  ];
}

function workflowRelationshipPreview(row) {
  const rows = row.relationship_rows || [];
  if (!rows.length) {
    return "";
  }
  const taskCount = row.mode === "matrix" ? new Set(rows.map((item) => item.task)).size : rows.length;
  const runCount = rows.length;
  return tr("workflowRelationshipPreview", {
    workflow: t("workflowEntityLabel"),
    id: row.id,
    taskDocuments: countLabel(taskCount, "taskDocumentCountOne", "taskDocumentCountMany"),
    taskRuns: countLabel(runCount, "taskRunCountOne", "taskRunCountMany"),
  });
}

function countLabel(count, oneKey, manyKey) {
  return tr(count === 1 ? oneKey : manyKey, { count });
}

function workflowFactFields(row) {
  return [
    { label: "Workflow", value: row.id },
    { label: t("workflowModeLabel"), value: row.mode },
    { label: t("workflowBaseLabel"), value: row.base || row.base_mode },
    { label: t("workflowPolicyLabel"), value: `${row.policy.pull_request}/${row.policy.landing}/review:${row.policy.review_codex_base}` },
    { label: t("workflowRunnableLabel"), value: row.runnable.runnable_count },
    { label: t("workflowUpdatedAtLabel"), value: row.updated_at },
    row.profile ? { label: t("profileLabel"), value: row.profile } : null,
    row.profiles.length ? { label: t("profileLabel"), value: row.profiles.join(", ") } : null,
    row.state_error ? { label: t("errorLabel"), value: row.state_error, tone: "red" } : null,
  ].filter(Boolean);
}

function workflowSourceFields(row) {
  const fields = [{ label: "Workflow", value: row.path }];
  const seen = new Set([row.path]);
  (row.relationship_rows || []).forEach((item) => {
    const documentPath = item.task_document?.path;
    if (documentPath && !seen.has(documentPath)) {
      seen.add(documentPath);
      fields.push({ label: t("taskDocumentLabel"), value: documentPath });
    }
    const runPath = item.task_run?.path || item.task_run_path;
    if (runPath && !seen.has(runPath)) {
      seen.add(runPath);
      fields.push({ label: t("taskRunLabel"), value: runPath, tone: item.task_run_error ? "red" : "" });
    }
  });
  return fields;
}

function workflowRelationshipSummary(row) {
  const relationships = row.relationship_rows || [];
  if (!relationships.length) {
    return `<div class="relationship-empty">${escapeHtml(t("relationshipEmpty"))}</div>`;
  }
  return `<div class="workflow-relationship-summary mode-${escapeHtml(domId(row.mode))}" role="list" aria-label="${escapeHtml(t("workflowRelationships"))}">${relationships.map((item) => workflowRelationshipRow(row, item)).join("")}</div>`;
}

function workflowRelationshipRow(workflow, item) {
  const taskRun = item.task_run;
  const taskDocument = item.task_document;
  const attention = Boolean(item.task_document_error || item.task_run_error || taskRun?.error || taskRun?.status === "failed");
  const tone = attention ? "red" : statusColor(taskRun?.status || "waiting");
  return `<article class="workflow-relationship-row tone-${tone || "neutral"}" role="listitem">${workflowRelationshipRail(workflow, item)}<div class="relationship-segments">${workflowTaskDocumentSegment(item, taskDocument)}${workflowTaskRunSegment(item, taskRun)}${workflowAgentSegment()}</div></article>`;
}

function workflowRelationshipRail(workflow, item) {
  if (workflow.mode === "stack") {
    return `<div class="relationship-rail"><span>${escapeHtml(item.index)}</span><small>${escapeHtml(t("stackOrderLabel"))}</small></div>`;
  }
  if (workflow.mode === "matrix") {
    return `<div class="relationship-rail is-profile"><span aria-hidden="true"></span><small>${escapeHtml(t("matrixProfileLabel"))}</small></div>`;
  }
  if (workflow.mode === "batch") {
    return `<div class="relationship-rail is-peer"><span aria-hidden="true"></span><small>${escapeHtml(t("batchSiblingLabel"))}</small></div>`;
  }
  return `<div class="relationship-rail is-single"><span aria-hidden="true"></span><small>${escapeHtml(workflow.mode)}</small></div>`;
}

function workflowTaskDocumentSegment(item, taskDocument) {
  const title = taskDocument ? taskDocument.title : t("missingTaskDocumentLabel");
  const meta = [
    pill(`task ${taskDocument?.key || item.task}`, "blue"),
    taskDocument?.branch ? pill(`branch ${taskDocument.branch}`) : "",
  ];
  return relationshipSegment({
    label: t("taskDocumentLabel"),
    title,
    meta,
    path: taskDocument?.path,
    error: item.task_document_error,
    tone: taskDocument ? "blue" : "red",
  });
}

function workflowTaskRunSegment(item, taskRun) {
  const title = taskRun ? taskRun.id : (item.run_id || t("missingTaskRunLabel"));
  const meta = [
    taskRun ? pill(stateLabel(taskRun.status), statusColor(taskRun.status)) : pill(t("missingTaskRunLabel"), "red"),
    item.profile ? pill(`${t("profileLabel")} ${item.profile}`, "violet") : "",
    taskRun?.branch ? pill(`branch ${taskRun.branch}`) : "",
    item.parent ? pill(`${t("parentLabel")} ${item.parent}`, "violet") : "",
  ];
  return relationshipSegment({
    label: t("taskRunLabel"),
    title,
    meta,
    path: taskRun?.path || item.task_run_path,
    error: [item.task_run_error, taskRun?.error].filter(Boolean).join("\n"),
    tone: taskRun ? statusColor(taskRun.status) : "red",
  });
}

function workflowAgentSegment() {
  return relationshipSegment({
    label: t("agentObservationLabel"),
    title: t("agentNotObserved"),
    meta: [],
    path: "",
    error: "",
    tone: "",
  });
}

function relationshipSegment({ label: labelText, title, meta, path, error, tone }) {
  const metaHtml = meta.filter(Boolean).join("");
  const pathHtml = path ? `<p class="relationship-path">${escapeHtml(path)}</p>` : "";
  const errorHtml = error ? `<p class="relationship-error">${escapeHtml(error)}</p>` : "";
  return `<section class="relationship-segment tone-${tone || "neutral"}"><span>${escapeHtml(labelText)}</span><strong>${escapeHtml(title)}</strong>${metaHtml ? `<div class="relationship-meta meta">${metaHtml}</div>` : ""}${pathHtml}${errorHtml}</section>`;
}

function workflowCanvasSection(row) {
  const graph = workflowCanvasGraph(row);
  if (!graph) {
    return "";
  }
  const planeStyle = `style="width:${graph.width}px;height:${graph.height}px"`;
  const controls = ["fit", "center", "source"]
    .map((action) => {
      const key = action === "fit" ? "workflowCanvasFit" : action === "center" ? "workflowCanvasCenter" : "workflowCanvasSource";
      return `<button type="button" class="workflow-canvas-control" data-workflow-canvas-control="${action}">${escapeHtml(t(key))}</button>`;
    })
    .join("");
  return `<div class="workflow-canvas mode-${escapeHtml(domId(row.mode))}" data-workflow-canvas><div class="workflow-canvas-shell"><div class="workflow-canvas-frame"><div class="workflow-canvas-toolbar"><span class="workflow-canvas-badge">${escapeHtml(t("workflowReadOnly"))}</span><div class="workflow-canvas-controls">${controls}</div></div><div class="workflow-canvas-viewport" tabindex="0" aria-label="${escapeHtml(t("workflowCanvasAria"))}"><div class="workflow-canvas-plane" ${planeStyle}>${workflowCanvasEdges(row, graph)}${graph.nodes.map(workflowCanvasNode).join("")}</div></div></div>${workflowCanvasInspector(row)}</div></div>`;
}

function workflowCanvasGraph(row) {
  const relationships = row.relationship_rows || [];
  if (!relationships.length) {
    return null;
  }
  if (row.mode === "matrix") {
    return workflowMatrixCanvasGraph(row, relationships);
  }
  if (row.mode === "batch") {
    return workflowBatchCanvasGraph(row, relationships);
  }
  if (row.mode === "stack") {
    return workflowStackCanvasGraph(row, relationships);
  }
  return workflowSingleCanvasGraph(row, relationships);
}

function workflowStackCanvasGraph(row, relationships) {
  const c = WORKFLOW_CANVAS;
  const nodes = [];
  const edges = [];
  const taskStartX = c.margin + c.workflowW + c.gapX;
  const taskY = 44;
  const agentY = taskY + c.taskH + c.agentGap;
  const workflowY = taskY + 36;
  nodes.push(workflowCanvasWorkflowNode(row, c.margin, workflowY));

  relationships.forEach((item, index) => {
    const taskX = taskStartX + index * (c.taskW + c.gapX);
    const taskNode = workflowCanvasTaskNode(item, index, taskX, taskY);
    const agentNode = workflowCanvasAgentNode(item, index, taskX + (c.taskW - c.agentW) / 2, agentY);
    nodes.push(taskNode, agentNode);
    edges.push(workflowCanvasEdge(index === 0 ? "workflow" : `task-${index - 1}`, taskNode.id, index === 0 ? t("workflowContainsLabel") : t("parentLabel"), index === 0 ? "solid" : "parent"));
    edges.push(workflowCanvasEdge(taskNode.id, agentNode.id, t("workflowObservedByLabel"), "dashed"));
  });

  return workflowCanvasResolveGraph(nodes, edges, {
    width: Math.max(720, taskStartX + relationships.length * c.taskW + Math.max(0, relationships.length - 1) * c.gapX + c.margin),
    height: agentY + c.agentH + c.margin,
  });
}

function workflowBatchCanvasGraph(row, relationships) {
  const c = WORKFLOW_CANVAS;
  const nodes = [];
  const edges = [];
  const taskStartX = c.margin + c.workflowW + c.gapX;
  const columns = Math.min(3, Math.max(1, relationships.length));
  const laneH = c.taskH + c.agentGap + c.agentH + c.gapY;
  const rows = Math.ceil(relationships.length / columns);
  const height = Math.max(330, c.margin * 2 + rows * laneH - c.gapY);
  nodes.push(workflowCanvasWorkflowNode(row, c.margin, Math.round(height / 2 - c.workflowH / 2)));

  relationships.forEach((item, index) => {
    const col = index % columns;
    const rowIndex = Math.floor(index / columns);
    const taskX = taskStartX + col * (c.taskW + c.gapX);
    const taskY = c.margin + rowIndex * laneH;
    const taskNode = workflowCanvasTaskNode(item, index, taskX, taskY);
    const agentNode = workflowCanvasAgentNode(item, index, taskX + (c.taskW - c.agentW) / 2, taskY + c.taskH + c.agentGap);
    nodes.push(taskNode, agentNode);
    edges.push(workflowCanvasEdge("workflow", taskNode.id, t("workflowContainsLabel"), "solid"));
    edges.push(workflowCanvasEdge(taskNode.id, agentNode.id, t("workflowObservedByLabel"), "dashed"));
  });

  return workflowCanvasResolveGraph(nodes, edges, {
    width: Math.max(720, taskStartX + columns * c.taskW + Math.max(0, columns - 1) * c.gapX + c.margin),
    height,
  });
}

function workflowMatrixCanvasGraph(row, relationships) {
  const c = WORKFLOW_CANVAS;
  const nodes = [];
  const edges = [];
  const documentX = c.margin + c.workflowW + c.gapX;
  const runStartX = documentX + c.taskW + c.gapX;
  const columns = relationships.length <= 3 ? relationships.length : 2;
  const safeColumns = Math.max(1, columns);
  const laneH = c.matrixRunH + c.agentGap + c.agentH + c.gapY;
  const rows = Math.ceil(relationships.length / safeColumns);
  const height = Math.max(360, c.margin * 2 + rows * laneH - c.gapY);
  const docItem = relationships.find((item) => item.task_document) || relationships[0];
  const docNode = workflowCanvasMatrixDocumentNode(docItem, 0, documentX, Math.round(height / 2 - 64));
  nodes.push(workflowCanvasWorkflowNode(row, c.margin, Math.round(height / 2 - c.workflowH / 2)), docNode);
  edges.push(workflowCanvasEdge("workflow", docNode.id, t("workflowContainsLabel"), "solid"));

  relationships.forEach((item, index) => {
    const col = index % safeColumns;
    const rowIndex = Math.floor(index / safeColumns);
    const runX = runStartX + col * (c.matrixRunW + c.gapX);
    const runY = c.margin + rowIndex * laneH;
    const runNode = workflowCanvasMatrixRunNode(item, index, runX, runY);
    const agentNode = workflowCanvasAgentNode(item, index, runX + (c.matrixRunW - c.agentW) / 2, runY + c.matrixRunH + c.agentGap);
    nodes.push(runNode, agentNode);
    edges.push(workflowCanvasEdge(docNode.id, runNode.id, item.profile || t("matrixProfileLabel"), "solid"));
    edges.push(workflowCanvasEdge(runNode.id, agentNode.id, t("workflowObservedByLabel"), "dashed"));
  });

  return workflowCanvasResolveGraph(nodes, edges, {
    width: Math.max(900, runStartX + safeColumns * c.matrixRunW + Math.max(0, safeColumns - 1) * c.gapX + c.margin),
    height,
  });
}

function workflowSingleCanvasGraph(row, relationships) {
  const c = WORKFLOW_CANVAS;
  const nodes = [];
  const edges = [];
  const taskStartX = c.margin + c.workflowW + c.gapX;
  const laneH = c.taskH + c.agentGap + c.agentH + c.gapY;
  const height = Math.max(320, c.margin * 2 + relationships.length * laneH - c.gapY);
  nodes.push(workflowCanvasWorkflowNode(row, c.margin, Math.round(height / 2 - c.workflowH / 2)));

  relationships.forEach((item, index) => {
    const taskX = taskStartX;
    const taskY = c.margin + index * laneH;
    const taskNode = workflowCanvasTaskNode(item, index, taskX, taskY);
    const agentNode = workflowCanvasAgentNode(item, index, taskX + c.taskW + c.gapX, taskY + Math.round((c.taskH - c.agentH) / 2));
    nodes.push(taskNode, agentNode);
    edges.push(workflowCanvasEdge("workflow", taskNode.id, t("workflowContainsLabel"), "solid"));
    edges.push(workflowCanvasEdge(taskNode.id, agentNode.id, t("workflowObservedByLabel"), "dashed"));
  });

  return workflowCanvasResolveGraph(nodes, edges, {
    width: Math.max(720, taskStartX + c.taskW + c.gapX + c.agentW + c.margin),
    height,
  });
}

function workflowCanvasWorkflowNode(row, x, y) {
  return { id: "workflow", kind: "workflow", row, x, y, w: WORKFLOW_CANVAS.workflowW, h: WORKFLOW_CANVAS.workflowH, tone: workflowNeedsAttention(row) ? "red" : "green" };
}

function workflowCanvasTaskNode(item, index, x, y) {
  const attention = workflowRelationshipAttention(item);
  return { id: `task-${index}`, kind: "task", item, x, y, w: WORKFLOW_CANVAS.taskW, h: WORKFLOW_CANVAS.taskH, tone: attention ? "red" : statusColor(item.task_run?.status || "waiting") };
}

function workflowCanvasMatrixDocumentNode(item, index, x, y) {
  const attention = Boolean(item.task_document_error || !item.task_document);
  return { id: `matrix-document-${index}`, kind: "matrix-document", item, x, y, w: WORKFLOW_CANVAS.taskW, h: WORKFLOW_CANVAS.matrixRunH, tone: attention ? "red" : "blue" };
}

function workflowCanvasMatrixRunNode(item, index, x, y) {
  const attention = Boolean(item.task_run_error || item.task_run?.error || item.task_run?.status === "failed" || !item.task_run);
  return { id: `matrix-run-${index}`, kind: "matrix-run", item, x, y, w: WORKFLOW_CANVAS.matrixRunW, h: WORKFLOW_CANVAS.matrixRunH, tone: attention ? "red" : statusColor(item.task_run?.status || "waiting") };
}

function workflowCanvasAgentNode(item, index, x, y) {
  return { id: `agent-${index}`, kind: "agent", item, x, y, w: WORKFLOW_CANVAS.agentW, h: WORKFLOW_CANVAS.agentH, tone: "" };
}

function workflowCanvasEdge(from, to, labelText, kind) {
  return { from, to, label: labelText, kind };
}

function workflowCanvasResolveGraph(nodes, edges, size) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  return {
    width: size.width,
    height: size.height,
    nodes,
    edges: edges
      .map((edge) => {
        const from = byId.get(edge.from);
        const to = byId.get(edge.to);
        if (!from || !to) {
          return null;
        }
        const points = workflowCanvasEdgePoints(from, to);
        return { ...edge, ...points };
      })
      .filter(Boolean),
  };
}

function workflowCanvasEdgePoints(from, to) {
  const horizontal = Math.abs(to.x - from.x) >= Math.abs(to.y - from.y);
  if (horizontal && to.x >= from.x) {
    return {
      x1: from.x + from.w,
      y1: from.y + from.h / 2,
      x2: to.x,
      y2: to.y + to.h / 2,
    };
  }
  return {
    x1: from.x + from.w / 2,
    y1: from.y + from.h,
    x2: to.x + to.w / 2,
    y2: to.y,
  };
}

function workflowCanvasEdges(row, graph) {
  const markerId = `workflow-canvas-arrow-${domId(row.id)}`;
  const edgeHtml = graph.edges
    .map((edge) => {
      const className = edge.kind === "dashed" ? "is-dashed" : edge.kind === "parent" ? "is-parent" : "is-solid";
      return `<path class="workflow-canvas-edge ${className}" data-edge-from="${escapeHtml(edge.from)}" data-edge-to="${escapeHtml(edge.to)}" d="${workflowCanvasEdgePath(edge)}" marker-end="url(#${markerId})"></path>`;
    })
    .join("");
  return `<svg class="workflow-canvas-edges" viewBox="0 0 ${graph.width} ${graph.height}" width="${graph.width}" height="${graph.height}" aria-hidden="true"><defs><marker id="${markerId}" viewBox="0 0 12 12" refX="10.5" refY="6" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 2 2 L 10 6 L 2 10 z"></path></marker></defs>${edgeHtml}</svg>`;
}

function workflowCanvasEdgePath(edge) {
  const x1 = Math.round(edge.x1);
  const y1 = Math.round(edge.y1);
  const x2 = Math.round(edge.x2);
  const y2 = Math.round(edge.y2);
  const dx = x2 - x1;
  const dy = y2 - y1;
  if (Math.abs(dx) >= Math.abs(dy)) {
    const bend = Math.min(72, Math.max(36, Math.abs(dx) * 0.42));
    return `M ${x1} ${y1} C ${Math.round(x1 + bend)} ${y1}, ${Math.round(x2 - bend)} ${y2}, ${x2} ${y2}`;
  }
  const bend = Math.min(64, Math.max(34, Math.abs(dy) * 0.42));
  return `M ${x1} ${y1} C ${x1} ${Math.round(y1 + bend)}, ${x2} ${Math.round(y2 - bend)}, ${x2} ${y2}`;
}

function workflowCanvasNode(node) {
  const style = `style="left:${Math.round(node.x)}px;top:${Math.round(node.y)}px;width:${Math.round(node.w)}px;height:${Math.round(node.h)}px"`;
  const classes = `workflow-canvas-node is-${node.kind} tone-${node.tone || "neutral"}`;
  const labelText = workflowCanvasNodeLabel(node);
  return `<article class="${classes}" data-workflow-canvas-node="${escapeHtml(node.id)}" tabindex="0" ${style} aria-label="${escapeHtml(labelText)}">${workflowCanvasNodePorts(node)}${workflowCanvasNodeBody(node)}</article>`;
}

function workflowCanvasNodePorts(node) {
  if (node.kind === "workflow") {
    return `<span class="workflow-canvas-port is-output" aria-hidden="true"></span>`;
  }
  if (node.kind === "agent") {
    return `<span class="workflow-canvas-port is-top-input" aria-hidden="true"></span>`;
  }
  return `<span class="workflow-canvas-port is-input" aria-hidden="true"></span><span class="workflow-canvas-port is-output" aria-hidden="true"></span><span class="workflow-canvas-port is-bottom-output" aria-hidden="true"></span>`;
}

function workflowCanvasNodeLabel(node) {
  if (node.kind === "workflow") {
    return `Workflow ${node.row.id}`;
  }
  if (node.kind === "agent") {
    return `${t("agentObservationLabel")} ${t("agentNotObserved")}`;
  }
  const item = node.item || {};
  return `${item.task || item.run_id || node.id}`;
}

function workflowCanvasNodeBody(node) {
  if (node.kind === "workflow") {
    const row = node.row;
    const meta = [
      pill(row.mode, "blue"),
      row.base || row.base_mode ? pill(compactText(row.base || row.base_mode, 24)) : "",
    ].filter(Boolean).join("");
    return `<div class="workflow-canvas-node-head"><span>${escapeHtml(t("workflowAnchorLabel"))}</span><strong>${escapeHtml(row.title || row.id)}</strong></div><div class="workflow-canvas-meta meta">${meta}</div>`;
  }
  if (node.kind === "agent") {
    return `<div class="workflow-canvas-agent"><span>${escapeHtml(t("agentObservationLabel"))}</span><strong>${escapeHtml(t("agentNotObserved"))}</strong></div>`;
  }
  if (node.kind === "matrix-document") {
    return workflowCanvasDocumentBand(node.item, { standalone: true });
  }
  if (node.kind === "matrix-run") {
    return workflowCanvasRunBand(node.item, { standalone: true });
  }
  return `${workflowCanvasDocumentBand(node.item)}${workflowCanvasRunBand(node.item)}`;
}

function workflowCanvasDocumentBand(item, options = {}) {
  const taskDocument = item.task_document;
  const title = taskDocument ? taskDocument.title : t("missingTaskDocumentLabel");
  const meta = [
    pill(`task ${compactText(taskDocument?.key || item.task, 24)}`, "blue"),
    item.profile ? pill(`${t("profileLabel")} ${compactText(item.profile, 18)}`, "violet") : "",
  ].filter(Boolean).join("");
  const error = item.task_document_error ? `<p class="workflow-canvas-error">${escapeHtml(item.task_document_error)}</p>` : "";
  const standalone = options.standalone ? " is-standalone" : "";
  const tone = taskDocument ? "blue" : "red";
  return `<section class="workflow-canvas-band is-document tone-${tone}${standalone}"><span>${escapeHtml(t("taskDocumentLabel"))}</span><strong>${escapeHtml(compactText(title, 56))}</strong>${meta ? `<div class="workflow-canvas-meta meta">${meta}</div>` : ""}${error}</section>`;
}

function workflowCanvasRunBand(item, options = {}) {
  const taskRun = item.task_run;
  const title = taskRun ? taskRun.id : (item.run_id || t("missingTaskRunLabel"));
  const runTone = taskRun ? statusColor(taskRun.status) : "red";
  const meta = [
    taskRun ? pill(stateLabel(taskRun.status), statusColor(taskRun.status)) : pill(t("missingTaskRunLabel"), "red"),
    item.profile ? pill(`${t("profileLabel")} ${compactText(item.profile, 18)}`, "violet") : "",
  ].filter(Boolean).join("");
  const error = [item.task_run_error, taskRun?.error].filter(Boolean).join("\n");
  const errorHtml = error ? `<p class="workflow-canvas-error">${escapeHtml(error)}</p>` : "";
  const standalone = options.standalone ? " is-standalone" : "";
  return `<section class="workflow-canvas-band is-run tone-${runTone || "neutral"}${standalone}"><span>${escapeHtml(t("taskRunLabel"))}</span><strong>${escapeHtml(compactText(title, 34))}</strong>${meta ? `<div class="workflow-canvas-meta meta">${meta}</div>` : ""}${errorHtml}</section>`;
}

function workflowCanvasInspector(row) {
  const sourceFields = workflowSourceFields(row);
  const sourceHtml = sourceFields.length
    ? `<div class="workflow-canvas-source-list">${sourceFields.map((field) => `<p><span>${escapeHtml(field.label)}</span><code>${escapeHtml(field.value)}</code></p>`).join("")}</div>`
    : "";
  const facts = detailFields([
    { label: t("workflowModeLabel"), value: row.mode },
    { label: t("workflowPolicyLabel"), value: `${row.policy.pull_request}/${row.policy.landing}/review:${row.policy.review_codex_base}` },
    { label: t("workflowRunnableLabel"), value: row.runnable.runnable_count },
  ]);
  return `<aside class="workflow-canvas-inspector" aria-label="${escapeHtml(t("workflowCanvasInspector"))}"><h5>${escapeHtml(t("workflowCanvasInspector"))}</h5>${facts}<details><summary>${escapeHtml(t("workflowCanvasSource"))}</summary>${sourceHtml}</details><div class="workflow-canvas-legend"><span>${escapeHtml(t("workflowCanvasLegend"))}</span><p>${escapeHtml(t("workflowCanvasSolidEdge"))}</p><p>${escapeHtml(t("workflowCanvasDashedEdge"))}</p><p>${escapeHtml(t("workflowCanvasAttentionEdge"))}</p></div></aside>`;
}

function workflowRelationshipAttention(item) {
  return Boolean(item.task_document_error || item.task_run_error || item.task_run?.error || item.task_run?.status === "failed" || !item.task_document || !item.task_run);
}

function taskRunsCockpit(snapshot) {
  const runs = sortedTaskRuns(snapshot.task_runs.items);
  const groups = countBy(runs, taskRunUiGroup);
  const unlinkedTasks = unlinkedTaskDocuments(snapshot);
  const records = runs
    .map(taskRunMasterDetailRecord)
    .concat(snapshot.task_runs.invalid.map(invalidTaskRunMasterDetailRecord))
    .concat(unlinkedTasks.map(unlinkedTaskDocumentMasterDetailRecord));
  const attentionCount = (groups.needs_attention || 0) + snapshot.task_runs.invalid.length + unlinkedTasks.length;
  const stats = [
    attentionStat(attentionCount),
    { label: t("stateRunning"), value: groups.running || 0 },
    { label: t("statePrepared"), value: groups.prepared || 0 },
    { label: t("metricTaskRuns"), value: runs.length },
  ];
  return masterDetailPanel({
    id: "task-runs-cockpit",
    tabKey: "task-runs",
    title: t("cockpitTaskRunTitle"),
    subtitle: t("cockpitTaskRunSubtitle"),
    stats,
    listTitle: t("taskRunIndex"),
    records,
    emptyText: t("noTaskRuns"),
  });
}

function taskRunMasterDetailRecord(row) {
  const taskDocument = row.task_document;
  const group = taskRunUiGroup(row);
  const needsAttention = group === "needs_attention";
  const title = taskDocument ? taskDocument.title : row.task;
  const summary = row.error || row.context.error || row.task_document_error || taskDocument?.body_summary || row.context.label || row.branch;
  return {
    id: `task-run-${row.id}`,
    group: stateLabel(group),
    tone: needsAttention ? "red" : statusColor(group),
    needsAttention,
    kicker: `TaskRun ${row.id}`,
    title,
    summary,
    listPills: taskRunPills(row, group),
    pills: taskRunPills(row, group),
    paths: [row.path, row.context.workflow_path, taskDocument && taskDocument.path].filter(Boolean),
    fields: taskRunFactFields(row),
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: taskRunSourceFields(row),
    collapseSources: true,
    sources: [
      { label: t("taskRunState"), text: formatTaskRunState(row), kind: "source" },
      { label: t("taskDocumentToml"), text: taskDocument?.source_text, kind: "source" },
    ],
  };
}

function invalidTaskRunMasterDetailRecord(row) {
  return {
    id: `invalid-task-run-${row.key}`,
    group: t("needsAttention"),
    tone: "red",
    needsAttention: true,
    kicker: t("invalidTaskRuns"),
    title: row.key,
    summary: row.error,
    pills: [pill(t("invalid"), "red")],
    paths: [row.path],
    fields: [
      { label: t("taskRunLabel"), value: row.key },
      { label: t("errorLabel"), value: row.error, tone: "red" },
    ],
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: [{ label: t("source"), value: row.path, tone: "red" }],
    collapseSources: true,
    sources: [{ label: t("sourceToml"), text: [row.error, row.source_text].filter(Boolean).join("\n\n"), kind: "source" }],
  };
}

function unlinkedTaskDocumentMasterDetailRecord(row) {
  return {
    id: `unlinked-task-document-${row.key}`,
    group: t("needsAttention"),
    tone: "red",
    needsAttention: true,
    kicker: t("taskDocumentLabel"),
    title: row.title,
    summary: row.body_summary,
    listPills: [
      pill(t("focusUnlinkedTaskDocument"), "amber"),
      pill(`task ${row.key}`, "blue"),
      row.branch ? pill(`branch ${row.branch}`) : "",
    ],
    pills: [
      pill(t("focusUnlinkedTaskDocument"), "amber"),
      pill(`task ${row.key}`, "blue"),
      row.branch ? pill(`branch ${row.branch}`) : "",
    ],
    paths: [row.path],
    fields: [
      { label: t("taskDocumentLabel"), value: row.key },
      { label: "branch", value: row.branch },
    ],
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: [{ label: t("taskDocumentLabel"), value: row.path, tone: "red" }],
    collapseSources: true,
    sources: [{ label: t("taskDocumentToml"), text: row.source_text, kind: "source" }],
  };
}

function taskRunPills(row, group = taskRunUiGroup(row)) {
  return [
    pill(stateLabel(group), statusColor(group)),
    pill(`task ${row.task}`, "blue"),
    row.branch ? pill(`branch ${row.branch}`) : "",
    row.context.workflow_id ? pill(`workflow ${row.context.workflow_id}`, "violet") : pill(row.context.label || "direct"),
    row.context.mode ? pill(row.context.mode, "violet") : "",
    row.group ? pill(`group ${row.group}`, "violet") : "",
    row.context.error ? pill(t("focusContextError"), "red") : "",
    row.task_document_error || !row.task_document ? pill(t("focusMissingTaskDocument"), "red") : "",
  ];
}

function taskRunFactFields(row) {
  return [
    { label: t("taskRunLabel"), value: row.id },
    { label: t("taskDocumentLabel"), value: row.task },
    { label: "status", value: stateLabel(taskRunUiGroup(row)) },
    row.status !== taskRunUiGroup(row) ? { label: "stored_status", value: row.status } : null,
    { label: "branch", value: row.branch },
    row.group ? { label: "group", value: row.group } : null,
    row.context.label ? { label: "context", value: row.context.label } : null,
    row.context.workflow_id ? { label: t("workflowEntityLabel"), value: row.context.workflow_id } : null,
    row.context.mode ? { label: t("workflowModeLabel"), value: row.context.mode } : null,
    row.context.error ? { label: t("focusContextError"), value: row.context.error, tone: "red" } : null,
    row.error ? { label: t("focusTaskRunError"), value: row.error, tone: "red" } : null,
    row.task_document_error ? { label: t("focusTaskDocumentError"), value: row.task_document_error, tone: "red" } : null,
    !row.task_document && !row.task_document_error ? { label: t("focusMissingTaskDocument"), value: row.task, tone: "red" } : null,
  ].filter(Boolean);
}

function taskRunSourceFields(row) {
  const taskDocument = row.task_document;
  return [
    { label: t("taskRunLabel"), value: row.path },
    row.context.workflow_path ? { label: t("workflowEntityLabel"), value: row.context.workflow_path, tone: row.context.error ? "red" : "" } : null,
    taskDocument?.path ? { label: t("taskDocumentLabel"), value: taskDocument.path, tone: row.task_document_error ? "red" : "" } : null,
  ].filter(Boolean);
}

function ideasCockpit(snapshot) {
  const ideas = sortedIdeas(snapshot.ideas.items);
  const statusGroups = uniqueCount(ideas, (row) => row.status || "unspecified");
  const tagged = ideas.filter((row) => row.tags.length).length;
  const records = snapshot.ideas.invalid
    .map((row) => invalidIdeaMasterDetailRecord(row))
    .concat(ideas.map(ideaMasterDetailRecord));
  const stats = [
    attentionStat(snapshot.ideas.invalid.length),
    { label: t("ideas"), value: ideas.length },
    { label: t("statusGroups"), value: statusGroups },
    { label: t("taggedRecords"), value: tagged },
  ];
  return masterDetailPanel({
    id: "ideas-cockpit",
    tabKey: "ideas",
    title: t("cockpitIdeasTitle"),
    subtitle: t("cockpitIdeasSubtitle"),
    stats,
    listTitle: t("ideaIndex"),
    records,
    emptyText: t("noIdeas"),
  });
}

function retrospecsCockpit(snapshot) {
  const retrospecs = sortedRetrospecs(snapshot.retrospecs.items);
  const outcomeGroups = uniqueCount(retrospecs, (row) => row.outcome || "unspecified");
  const tagged = retrospecs.filter((row) => row.tags.length).length;
  const records = snapshot.retrospecs.invalid
    .map((row) => invalidRetrospecMasterDetailRecord(row))
    .concat(retrospecs.map(retrospecMasterDetailRecord));
  const stats = [
    attentionStat(snapshot.retrospecs.invalid.length),
    { label: t("retrospecs"), value: retrospecs.length },
    { label: t("outcomeGroups"), value: outcomeGroups },
    { label: t("taggedRecords"), value: tagged },
  ];
  return masterDetailPanel({
    id: "retrospecs-cockpit",
    tabKey: "retrospecs",
    title: t("cockpitRetrospecsTitle"),
    subtitle: t("cockpitRetrospecsSubtitle"),
    stats,
    listTitle: t("retrospecIndex"),
    records,
    emptyText: t("noRetrospecs"),
  });
}

function ideaMasterDetailRecord(row) {
  const status = row.status || "unspecified";
  return {
    id: planningMasterDetailRecordId("idea", row),
    group: status,
    tone: statusColor(row.status),
    kicker: row.kind,
    title: row.title,
    summary: row.body_summary,
    listPills: ideaPills(row),
    pills: ideaPills(row),
    paths: [row.path],
    fields: ideaFactFields(row),
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: [{ label: t("source"), value: row.path }],
    summarySectionTitle: t("renderedContent"),
    collapseSources: true,
    sources: [
      { label: t("body"), text: row.body, kind: "prose" },
      { label: t("sourceToml"), text: row.source_text, kind: "source" },
    ],
  };
}

function invalidIdeaMasterDetailRecord(row) {
  return invalidPlanningMasterDetailRecord(row, {
    idPrefix: "invalid-idea",
    group: t("needsAttention"),
    kicker: t("invalidIdeas"),
    entityLabel: t("ideas"),
  });
}

function ideaPills(row) {
  return [
    pill(row.status || "unspecified", statusColor(row.status)),
    pill(row.kind),
    row.source ? pill(row.source, "blue") : "",
    ...row.tags.slice(0, 4).map((tag) => pill(tag, "violet")),
  ];
}

function ideaFactFields(row) {
  return [
    { label: t("kindLabel"), value: row.kind },
    { label: t("statusLabel"), value: row.status || "unspecified" },
    row.source ? { label: t("source"), value: row.source } : null,
    row.tags.length ? { label: t("tagsLabel"), value: row.tags.join(", ") } : null,
    row.updated_at ? { label: t("updatedAtLabel"), value: row.updated_at } : null,
  ].filter(Boolean);
}

function retrospecMasterDetailRecord(row) {
  const outcome = row.outcome || "unspecified";
  return {
    id: planningMasterDetailRecordId("retrospec", row),
    group: outcome,
    tone: statusColor(row.outcome),
    kicker: row.kind,
    title: row.title,
    summary: row.body_summary,
    listPills: retrospecPills(row),
    pills: retrospecPills(row),
    paths: [row.path],
    fields: retrospecFactFields(row),
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: [{ label: t("source"), value: row.path }],
    summarySectionTitle: t("renderedContent"),
    collapseSources: true,
    sources: [
      { label: t("body"), text: row.body, kind: "prose" },
      { label: t("sourceToml"), text: row.source_text, kind: "source" },
    ],
  };
}

function invalidRetrospecMasterDetailRecord(row) {
  return invalidPlanningMasterDetailRecord(row, {
    idPrefix: "invalid-retrospec",
    group: t("needsAttention"),
    kicker: t("invalidRetrospecs"),
    entityLabel: t("retrospecs"),
  });
}

function retrospecPills(row) {
  return [
    row.outcome ? pill(row.outcome, statusColor(row.outcome)) : pill("unspecified"),
    row.scope ? pill(row.scope, row.scope === "spec-local" ? "green" : "blue") : "",
    row.spec ? pill(row.spec, "violet") : "",
    row.target ? pill(row.target, "blue") : "",
    row.date ? pill(row.date, "amber") : "",
    pill(row.kind),
    ...row.tags.slice(0, 4).map((tag) => pill(tag, "violet")),
  ];
}

function retrospecFactFields(row) {
  return [
    { label: t("kindLabel"), value: row.kind },
    row.scope ? { label: t("scopeLabel"), value: row.scope } : null,
    row.spec ? { label: t("specLabel"), value: row.spec } : null,
    { label: t("outcomeLabel"), value: row.outcome || "unspecified" },
    row.target ? { label: t("targetLabel"), value: row.target } : null,
    row.date ? { label: t("dateLabel"), value: row.date } : null,
    row.tags.length ? { label: t("tagsLabel"), value: row.tags.join(", ") } : null,
  ].filter(Boolean);
}

function invalidPlanningMasterDetailRecord(row, options) {
  return {
    id: planningMasterDetailRecordId(options.idPrefix, row),
    group: options.group,
    tone: "red",
    needsAttention: true,
    kicker: options.kicker,
    title: row.key,
    summary: row.error,
    pills: [pill(t("invalid"), "red")],
    paths: [row.path],
    fields: [
      { label: options.entityLabel, value: row.key },
      { label: t("errorLabel"), value: row.error, tone: "red" },
    ],
    relationshipsSectionTitle: t("sourcePaths"),
    relationships: [{ label: t("source"), value: row.path, tone: "red" }],
    collapseSources: true,
    sources: [{ label: t("sourceToml"), text: [row.error, row.source_text].filter(Boolean).join("\n\n"), kind: "source" }],
  };
}

function planningMasterDetailRecordId(prefix, row) {
  return `${prefix}-${stableRecordToken(row.path || row.key || row.title)}`;
}

function stableRecordToken(value) {
  const input = String(value || "record");
  let hash = 0x811c9dc5;
  for (const char of input) {
    hash ^= char.codePointAt(0);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `${hash.toString(16).padStart(8, "0")}-${input.length.toString(36)}`;
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
    focus.attention.length
      ? {
          key: "attention",
          title: t("needsAttention"),
          count: focus.attention.length,
          tone: "red",
          sectionId: "overview-attention",
          emptyText: t("noNeedsAttention"),
          items: focus.attention,
        }
      : null,
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
  ].filter(Boolean);
  const stats = groups.map((group) => ({ label: group.title, value: group.count, tone: group.tone }));
  return `<section class="focus-panel" aria-labelledby="focus-heading"><div class="focus-heading"><div><h2 id="focus-heading" class="section-title">${escapeHtml(t("focusTitle"))}</h2><p class="section-note">${escapeHtml(t("focusSubtitle"))}</p></div>${statusStrip(stats)}</div>${priorityFlow(groups)}</section>`;
}

function cockpitPanel(title, subtitle, stats, listTitle, rows, emptyText, id) {
  const body = rows.length ? `<div class="scan-list">${rows.join("")}</div>` : `<div class="focus-empty">${escapeHtml(emptyText)}</div>`;
  const count = rows.length === 1 ? `1 ${t("record")}` : `${rows.length} ${t("records")}`;
  return `<section class="focus-panel view-cockpit" id="${escapeHtml(id)}" aria-labelledby="${escapeHtml(id)}-heading"><div class="focus-heading"><div><h2 id="${escapeHtml(id)}-heading" class="section-title">${escapeHtml(title)}</h2><p class="section-note">${escapeHtml(subtitle)}</p></div>${statusStrip(stats)}</div><div class="scan-heading"><h3>${escapeHtml(listTitle)}</h3><span>${escapeHtml(count)}</span></div>${body}</section>`;
}

function masterDetailPanel({ id, tabKey, title, subtitle, stats, listTitle, records, emptyText, showCount = true }) {
  const visibleRecords = records.filter(Boolean);
  const count = visibleRecords.length === 1 ? `1 ${t("record")}` : `${visibleRecords.length} ${t("records")}`;
  const countHtml = showCount ? `<span>${escapeHtml(count)}</span>` : "";
  const selectedId = selectedMasterDetailId(tabKey, visibleRecords);
  const selected = visibleRecords.find((record) => record.id === selectedId);
  const body = visibleRecords.length
    ? `<div class="master-detail-shell" data-md-shell="${escapeHtml(tabKey)}"><div class="master-list" data-md-list="${escapeHtml(tabKey)}" aria-label="${escapeHtml(listTitle)}">${masterDetailList(tabKey, visibleRecords, selectedId)}</div>${selected ? masterDetailPane(selected) : ""}</div>`
    : `<div class="focus-empty">${escapeHtml(emptyText)}</div>`;
  return `<section class="focus-panel view-cockpit master-detail-panel" id="${escapeHtml(id)}" aria-labelledby="${escapeHtml(id)}-heading"><div class="focus-heading"><div><h2 id="${escapeHtml(id)}-heading" class="section-title">${escapeHtml(title)}</h2><p class="section-note">${escapeHtml(subtitle)}</p></div>${statusStrip(stats)}</div><div class="scan-heading"><h3>${escapeHtml(listTitle)}</h3>${countHtml}</div>${body}</section>`;
}

function masterDetailList(tabKey, records, selectedId) {
  let currentGroup = "";
  return records
    .map((record) => {
      const group = record.group || "";
      const heading = group && group !== currentGroup
        ? `<div class="master-list-group" role="presentation">${escapeHtml(group)}</div>`
        : "";
      currentGroup = group;
      return `${heading}${masterDetailListRow(tabKey, record, record.id === selectedId)}`;
    })
    .join("");
}

function selectedMasterDetailId(tabKey, records) {
  const current = state.selection[tabKey];
  if (records.some((record) => record.id === current)) {
    return current;
  }
  const firstAttention =
    records.find((record) => record.needsAttention) || records.find((record) => record.tone === "red");
  const selected = firstAttention || records[0];
  if (selected) {
    state.selection[tabKey] = selected.id;
    return selected.id;
  }
  return "";
}

function masterDetailListRow(tabKey, record, selected) {
  const meta = (record.listPills || record.pills || []).filter(Boolean).join("");
  const pathsHtml = record.paths && record.paths.length
    ? `<span class="master-paths">${record.paths.slice(0, 3).map((path) => `<span class="focus-path">${escapeHtml(path)}</span>`).join("")}</span>`
    : "";
  const summary = record.summary ? `<span class="focus-summary">${escapeHtml(record.summary)}</span>` : "";
  const kickerText = record.listKicker === undefined ? record.kicker : record.listKicker;
  const kicker = kickerText ? `<span class="focus-kicker">${escapeHtml(kickerText)}</span>` : "";
  const marker = record.listMarker
    ? `<span class="master-dot" aria-hidden="true"></span><span class="sr-only">${escapeHtml(record.listMarker)}</span>`
    : "";
  const selectedAttr = selected ? ` aria-pressed="true" aria-current="true"` : ` aria-pressed="false"`;
  return `<button class="master-list-row tone-${record.tone || "neutral"}${selected ? " is-selected" : ""}${record.listMarker ? " has-marker" : ""}" type="button" data-md-tab="${escapeHtml(tabKey)}" data-md-record="${escapeHtml(record.id)}"${selectedAttr}><span class="master-main">${kicker}<span class="master-title-line">${marker}<span class="master-title">${escapeHtml(record.title)}</span></span>${summary}</span><span class="master-meta meta">${meta}</span>${pathsHtml}</button>`;
}

function masterDetailPane(record) {
  const titleId = `detail-${domId(record.id)}-title`;
  const meta = (record.pills || []).filter(Boolean).join("");
  const summary = record.summary ? `<p class="focus-summary">${escapeHtml(record.summary)}</p>` : "";
  const kicker = record.kicker ? `<span class="focus-kicker">${escapeHtml(record.kicker)}</span>` : "";
  const pathsHtml = record.paths && record.paths.length
    ? `<div class="detail-path-block"><span class="detail-label">${escapeHtml(record.pathLabel || t("sourcePaths"))}</span>${paths(record.paths)}</div>`
    : "";
  const relationshipsHtml = detailFields(record.relationships || []);
  const relationshipBody = [pathsHtml, relationshipsHtml].filter(Boolean).join("");
  const sourceBody = detailSourceBlocks(record.sources || [], { collapsed: record.collapseSources });
  const summaryBody = [detailCards(record.cards || []), record.summaryHtml || "", detailFields(record.fields || [])].filter(Boolean).join("");
  const summarySectionTitle = record.hideSummarySectionTitle ? "" : (record.summarySectionTitle || t("detailSummary"));
  const sourceSectionTitle = record.hideSourceSectionTitle ? "" : (record.sourceSectionTitle || t("sourceContent"));
  return `<article class="detail-pane tone-${record.tone || "neutral"}" aria-labelledby="${escapeHtml(titleId)}"><header class="detail-header">${kicker}<h3 id="${escapeHtml(titleId)}">${escapeHtml(record.title)}</h3>${summary}<div class="meta">${meta}</div></header>${detailSection(record.canvasSectionTitle || "", record.canvasHtml || "", "workflow-canvas-detail-section")}${detailSection(summarySectionTitle, summaryBody)}${detailSection(record.relationshipsSectionTitle || t("detailRelationships"), relationshipBody)}${detailSection(sourceSectionTitle, sourceBody)}</article>`;
}

function detailSection(title, body, className = "") {
  if (!body) {
    return "";
  }
  const heading = title ? `<h4>${escapeHtml(title)}</h4>` : "";
  const classAttr = className ? ` ${escapeHtml(className)}` : "";
  return `<section class="detail-section${classAttr}">${heading}${body}</section>`;
}

function detailFields(fields) {
  const visible = fields.filter((field) => {
    if (!field || field.value === null || field.value === undefined || field.value === "") {
      return false;
    }
    return !(field.omitIfZero && Number(field.value) === 0);
  });
  if (!visible.length) {
    return "";
  }
  return `<dl class="detail-fields">${visible.map((field) => {
    const classAttr = field.tone ? ` class="${escapeHtml(field.tone)}"` : "";
    return `<div${classAttr}><dt>${escapeHtml(field.label)}</dt><dd>${escapeHtml(field.value)}</dd></div>`;
  }).join("")}</dl>`;
}

function detailCards(cards) {
  const visible = cards.filter(Boolean);
  if (!visible.length) {
    return "";
  }
  return `<div class="detail-cards">${visible.map(detailCard).join("")}</div>`;
}

function detailCard(card) {
  const value = card.value ? `<strong>${escapeHtml(card.value)}</strong>` : "";
  const items = card.items?.length
    ? `<div class="detail-card-items">${card.items.map(detailCardItem).join("")}</div>`
    : "";
  const heading = card.title || card.kicker || "";
  const emphasis = card.emphasis ? ` is-${escapeHtml(card.emphasis)}` : "";
  return `<article class="detail-card tone-${card.tone || "neutral"}${emphasis}"><div class="detail-card-head"><h5>${escapeHtml(heading)}</h5>${value}</div><p>${escapeHtml(card.description || "")}</p>${items}</article>`;
}

function detailCardItem(item) {
  const swatches = item.swatches?.length
    ? `<div class="detail-swatches">${item.swatches.map((swatch) => `<span><i style="background:${safeCssColor(swatch.color)}"></i>${escapeHtml(swatch.kind)}</span>`).join("")}</div>`
    : "";
  return `<div class="detail-card-item"><div><span>${escapeHtml(item.label)}</span><strong>${escapeHtml(item.value)}</strong></div><p>${escapeHtml(item.description || "")}</p>${swatches}</div>`;
}

function safeCssColor(value) {
  const color = String(value || "").trim();
  return /^[#a-zA-Z0-9(),.%\s-]+$/.test(color) ? color : "transparent";
}

function detailSourceBlocks(sources, options = {}) {
  const visible = sources.filter((source) => source && source.text);
  if (!visible.length) {
    return "";
  }
  return visible
    .map((source) => {
      const labelText = source.label || t("sourceContent");
      if (options.collapsed) {
        return `<details class="detail-source"><summary>${escapeHtml(labelText)}</summary><div class="source-panel full-text">${formatFullText(source.text, source.kind || "source")}</div></details>`;
      }
      const label = visible.length > 1 && source.label
        ? `<span class="detail-label">${escapeHtml(source.label)}</span>`
        : "";
      return `<div class="detail-source">${label}<div class="source-panel full-text">${formatFullText(source.text, source.kind || "source")}</div></div>`;
    })
    .join("");
}

function attentionStat(count) {
  if (!count) {
    return null;
  }
  return { label: t("needsAttention"), value: count, tone: "red" };
}

function statusStrip(stats) {
  const items = stats
    .filter(Boolean)
    .map((stat) => `<div class="status-counter tone-${stat.tone || "neutral"}"><span>${escapeHtml(stat.label)}</span><strong>${escapeHtml(stat.value)}</strong></div>`)
    .join("");
  if (!items) {
    return "";
  }
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
      pill(`${row.policy.pull_request}/${row.policy.landing}/review:${row.policy.review_codex_base}`, "amber"),
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
      row.scope ? pill(row.scope, row.scope === "spec-local" ? "green" : "blue") : "",
      row.spec ? pill(row.spec, "violet") : "",
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
    kicker: t("profiles"),
    title: row.name,
    summary: bodyPreview(row.source_text),
    pills: [
      pill(`${t("agentLabel")} ${row.agent}`, "blue"),
      valuesPill(t("copyLabel"), profileCopyValues(row)),
      valuesPill(t("copyAsLabel"), profileCopyAsValues(row)),
      valuesPill(t("linkLabel"), profileLinkValues(row)),
      row.has_site ? pill(t("profileSiteLabel"), "green") : "",
      row.test_count ? pill(`${row.test_count} ${t("testsLabel")}`, "amber") : "",
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
  if (row.presentation_group === "passed") return "passed";
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
    const status = taskRunStatusOrder(taskRunUiGroup(left)) - taskRunStatusOrder(taskRunUiGroup(right));
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
  return ["needs_attention", "running", "prepared", "passed", "skipped", "failed"].indexOf(status) === -1
    ? 99
    : ["needs_attention", "running", "prepared", "passed", "skipped", "failed"].indexOf(status);
}

function workflowGroupOrder(group) {
  return ["needs_attention", "running", "prepared", "waiting", "passed"].indexOf(group) === -1
    ? 99
    : ["needs_attention", "running", "prepared", "waiting", "passed"].indexOf(group);
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
  if (status === "passed") return "blue";
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
    passed: "statePassed",
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

function yesNo(value) {
  return value ? t("yes") : t("no");
}

function domId(value) {
  return String(value)
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "") || "record";
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
