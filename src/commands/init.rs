use crate::cli::{InitAgent, InitIssueProvider, InitSiteProvider};
use crate::config::{
    AgentCli, AgentConfig, Config, CopyAsEntry, OriginPolicy, ReviewCodexBasePolicy,
    ReviewDefaultPolicy, WORKSPACE_DEFAULT_COLORS, WorkflowDefaultLandingPolicy,
    WorkflowDefaultPolicy, WorkflowDefaultPullRequestMode, WorkspaceBrowserMode,
};
use crate::context::{Ctx, PromptItem};
use crate::error::WtError;
use crate::personal_storage;
use crate::storage::StorageRoot;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

const CLAUDE_LOCAL_SETTINGS_PATH: &str = ".claude/settings.local.json";
const CLAUDE_ALLOW_RULES: [&str; 2] = ["Edit(/.wt/**)", "Write(/.wt/**)"];
const DEFAULT_INJECT_LOCAL_CONTEXT: &str = "## Local context\n- site: {{site_url}}\n- worktree: {{worktree_path}}\n- parent: {{parent_branch}}\n";

#[derive(Debug, Default)]
pub struct InitOptions {
    pub local: bool,
    pub shared: bool,
    pub agent: Option<InitAgent>,
    pub agent_args: Vec<String>,
    pub agent_command: Option<String>,
    pub issue_provider: Option<InitIssueProvider>,
    pub site_provider: Option<InitSiteProvider>,
    pub gh_user: Option<String>,
    pub yes: bool,
    pub dry_run: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitTargetKind {
    Local,
    Shared,
}

#[derive(Debug)]
struct InitTarget {
    path: PathBuf,
    kind: InitTargetKind,
}

#[derive(Debug, Clone)]
struct InitProfile {
    agent: AgentConfig,
}

#[derive(Debug)]
struct InitPlan {
    target_path: PathBuf,
    target_kind: InitTargetKind,
    target_exists: bool,
    sections: Vec<InitSection>,
    #[cfg(test)]
    detected_signals: Vec<String>,
    notices: Vec<InitNotice>,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InitNotice {
    level: InitNoticeLevel,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitNoticeLevel {
    Hint,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitSection {
    Issues,
    Site,
    Workflow,
    Review,
    ProfileAgent,
    ProfileAgentPrompt,
    Worktree,
    WorktreeNaming,
    Setup,
    Editor,
    Workspace,
    WorkspaceBrowser,
}

impl InitSection {
    fn name(self) -> &'static str {
        match self {
            InitSection::Issues => "issues",
            InitSection::Site => "site",
            InitSection::Workflow => "workflow",
            InitSection::Review => "review",
            InitSection::ProfileAgent => "profile.agent",
            InitSection::ProfileAgentPrompt => "profile.agent.prompt",
            InitSection::Worktree => "worktree",
            InitSection::WorktreeNaming => "worktree.naming",
            InitSection::Setup => "setup",
            InitSection::Editor => "editor",
            InitSection::Workspace => "workspace",
            InitSection::WorkspaceBrowser => "workspace.browser",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct InitCommonConfig {
    worktree_path: Option<String>,
    worktree_copy: Vec<String>,
    worktree_copy_as: Vec<CopyAsEntry>,
    worktree_link: Vec<String>,
    inject_local_context: Option<String>,
    worktree_naming: bool,
    setup_deps: Vec<InitCommand>,
    editor_command: Option<String>,
    workspace_tabs: Vec<String>,
    post_deps_tabs: Vec<String>,
    workspace_colors: Vec<(String, String)>,
    workspace_browser: Option<InitWorkspaceBrowser>,
}

#[derive(Debug, Clone)]
struct InitWorkspaceBrowser {
    mode: InitWorkspaceBrowserMode,
    url: Option<String>,
    app: Option<String>,
    chrome_devtools_user_data_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitWorkspaceBrowserMode {
    System,
    ChromeDevtools,
}

#[derive(Debug, Clone, Default)]
struct InitDefaults {
    from_existing_config: bool,
    agent: Option<AgentConfig>,
    workflow_policy: Option<WorkflowDefaultPolicy>,
    review_policy: Option<ReviewDefaultPolicy>,
    issue_provider: Option<InitIssueProvider>,
    gh_user: Option<String>,
    origin_policy: OriginPolicy,
    site_provider: Option<InitSiteProvider>,
    common: Option<InitCommonConfig>,
}

#[derive(Debug, Clone)]
struct InitCommand {
    label: Option<String>,
    working_dir: Option<String>,
    run: String,
    if_exists: Option<String>,
    kind: InitCommandKind,
    default_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitCommandKind {
    NodeInstall,
    Other,
}

#[derive(Debug, Clone, Default)]
struct DetectedRepo {
    setup_deps: Vec<InitCommand>,
    post_deps_tabs: Vec<String>,
    issue_provider: Option<InitIssueProvider>,
    site_provider: Option<InitSiteProvider>,
    has_env_file: bool,
    local_links: Vec<String>,
}

impl DetectedRepo {
    fn scan(repo_root: &Path) -> Self {
        Self {
            setup_deps: detect_setup_deps(repo_root),
            post_deps_tabs: detect_post_deps_tabs(repo_root),
            issue_provider: detect_issue_provider(repo_root),
            site_provider: detect_site_provider(repo_root),
            has_env_file: repo_root.join(".env").exists(),
            local_links: detect_local_links(repo_root),
        }
    }

    #[cfg(test)]
    fn signals(&self) -> Vec<String> {
        let mut signals = Vec::new();
        if self.has_env_file {
            push_signal(&mut signals, "env: .env".into());
        }
        for link in &self.local_links {
            push_signal(&mut signals, format!("local link: {link}"));
        }
        if let Some(provider) = &self.issue_provider {
            push_signal(
                &mut signals,
                format!("issue provider: {}", issue_provider_name(provider)),
            );
        }
        if let Some(provider) = &self.site_provider {
            push_signal(
                &mut signals,
                format!("site provider: {}", site_provider_name(provider)),
            );
        }
        for command in &self.setup_deps {
            push_signal(&mut signals, format!("setup: {}", command_display(command)));
        }
        for tab in &self.post_deps_tabs {
            push_signal(&mut signals, format!("post-deps tab: {tab}"));
        }
        signals
    }
}

impl InitCommonConfig {
    fn from_config(config: &Config) -> Self {
        let mut common = InitCommonConfig {
            worktree_path: config.worktree.path.clone(),
            worktree_copy: config.worktree.copy.clone(),
            worktree_copy_as: config.worktree.copy_as.clone(),
            worktree_link: config.worktree.link.clone(),
            inject_local_context: config.worktree.inject_local_context.clone(),
            worktree_naming: config.worktree.naming.is_some(),
            setup_deps: config
                .setup
                .deps
                .iter()
                .map(|command| InitCommand {
                    label: None,
                    working_dir: command.working_dir.clone(),
                    run: command.run.clone(),
                    if_exists: command.if_exists.clone(),
                    kind: InitCommandKind::Other,
                    default_enabled: true,
                })
                .collect(),
            editor_command: config.editor.command.clone(),
            ..InitCommonConfig::default()
        };

        if let Some(workspace) = config.workspace.as_ref() {
            common.workspace_tabs = workspace.tabs.clone();
            common.post_deps_tabs = workspace.post_deps_tabs.clone();
            common.workspace_colors = workspace
                .effective_colors()
                .into_iter()
                .map(|(kind, color)| (kind.to_string(), color.to_string()))
                .collect();
            common.workspace_browser = workspace.browser.as_ref().and_then(|browser| {
                let mode = match browser.mode {
                    WorkspaceBrowserMode::None => return None,
                    WorkspaceBrowserMode::System => InitWorkspaceBrowserMode::System,
                    WorkspaceBrowserMode::ChromeDevtools => {
                        InitWorkspaceBrowserMode::ChromeDevtools
                    }
                };
                Some(InitWorkspaceBrowser {
                    mode,
                    url: browser.url.clone(),
                    app: browser.app.clone(),
                    chrome_devtools_user_data_dir: browser
                        .chrome_devtools
                        .as_ref()
                        .and_then(|chrome| chrome.user_data_dir.clone()),
                })
            });
        }

        common
    }
}

impl InitDefaults {
    fn from_config(config: Config) -> Self {
        let workflow_policy = (config.workflow.pull_request.is_some()
            || config.workflow.landing.is_some())
        .then(|| config.workflow_default_policy());
        let review_policy = config
            .review
            .codex_base
            .map(|codex_base| ReviewDefaultPolicy { codex_base });
        let agent = config
            .profile
            .as_ref()
            .and_then(|profile| profile.agent.clone())
            .or_else(|| config.agent.clone());
        let issue_provider = config.issues.as_ref().map(|issues| match &issues.provider {
            crate::config::IssueProviderType::Github => InitIssueProvider::Github,
            crate::config::IssueProviderType::Linear => InitIssueProvider::Linear,
        });
        let site_provider = config.site.as_ref().map(|site| match &site.provider {
            crate::config::SiteProvider::None => InitSiteProvider::None,
            crate::config::SiteProvider::Herd => InitSiteProvider::Herd,
            crate::config::SiteProvider::Valet => InitSiteProvider::Valet,
            crate::config::SiteProvider::DockerProxy => InitSiteProvider::DockerProxy,
            crate::config::SiteProvider::Traefik => InitSiteProvider::Traefik,
        });
        let gh_user = config
            .issues
            .as_ref()
            .and_then(|issues| issues.gh_user.clone());
        let origin_policy = config
            .issues
            .as_ref()
            .map(|issues| issues.origin_policy)
            .unwrap_or_default();
        let common = InitCommonConfig::from_config(&config);
        Self {
            from_existing_config: true,
            agent,
            workflow_policy,
            review_policy,
            issue_provider,
            gh_user,
            origin_policy,
            site_provider,
            common: Some(common),
        }
    }
}

pub fn run(ctx: &Ctx, options: InitOptions) -> Result<()> {
    validate_options(&options)?;
    ensure_no_legacy_bootstrap_roots(ctx)?;
    let interactive_wizard = is_interactive_wizard(&options);
    if interactive_wizard {
        print_wizard_header(ctx);
        print_wizard_step(
            ctx,
            1,
            "설정 파일 위치",
            "- 개인 설정 파일: <repo-root>/.wt/config/local.toml (보통 .wt/config/local.toml)\n- 팀 공유 설정: ./.wt.toml",
        );
    }
    let target = resolve_target(ctx, &options)?;
    let plan = build_plan(ctx, &options, target)?;
    if plan.target_exists && options.yes && !options.dry_run {
        print_existing_target_warning(ctx, &plan, &options);
    }

    if options.dry_run {
        print_plan(ctx, &plan, false);
        return Ok(());
    }

    if plan.target_exists && options.yes && !options.force {
        bail!(
            "설정 파일이 이미 있습니다: {} (--force로 덮어쓸 수 있습니다)",
            plan.target_path.display()
        );
    }

    if !options.yes {
        print_plan(ctx, &plan, interactive_wizard);
    }
    let confirm_prompt = if plan.target_exists {
        "기존 설정을 덮어쓸까요?"
    } else {
        "설정을 생성할까요?"
    };
    let confirm_default = !plan.target_exists;
    if interactive_wizard {
        print_wizard_step(
            ctx,
            5,
            "쓰기 확인",
            "지금 설정 파일에 쓸지 확인합니다. 취소하면 파일은 바뀌지 않습니다.",
        );
    }
    if !options.yes && !ctx.ui.confirm(confirm_prompt, confirm_default)? {
        return Err(WtError::Cancelled.into());
    }

    personal_storage::ensure_repo_bootstrap(&ctx.storage_root)?;

    if let Some(parent) = plan.target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&plan.target_path, &plan.content)?;
    let action = if plan.target_exists {
        "설정 업데이트됨"
    } else {
        "설정 생성됨"
    };
    ctx.ui
        .print_step(&format!("{action}: {}", plan.target_path.display()));

    bootstrap_core_dirs(&ctx.storage_root)?;
    maybe_scaffold_claude_allow_rules(ctx, &options)?;

    Ok(())
}

fn load_init_defaults(path: &Path) -> InitDefaults {
    let Ok(content) = std::fs::read_to_string(path) else {
        return InitDefaults::default();
    };
    let Ok(config) = toml::from_str::<Config>(&content) else {
        return InitDefaults::default();
    };
    InitDefaults::from_config(config)
}

fn bootstrap_core_dirs(storage_root: &StorageRoot) -> Result<()> {
    for dir in core_state_dirs(storage_root) {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create wt core state dir: {}", dir.display()))?;
    }
    Ok(())
}

fn ensure_no_legacy_bootstrap_roots(ctx: &Ctx) -> Result<()> {
    let legacy_roots = [
        (
            "config",
            ctx.storage_root.detect_legacy_config(&ctx.repo_root),
        ),
        (
            "profile storage",
            ctx.storage_root.detect_legacy_profiles(&ctx.repo_root),
        ),
        (
            "retrospective storage",
            ctx.storage_root
                .detect_legacy_retrospectives(&ctx.repo_root),
        ),
        (
            "TaskDocument storage",
            ctx.storage_root.detect_legacy_tasks(&ctx.repo_root),
        ),
        (
            "Workflow storage",
            ctx.storage_root.detect_legacy_workflows(&ctx.repo_root),
        ),
        (
            "TaskRun storage",
            ctx.storage_root.detect_legacy_task_runs(&ctx.repo_root),
        ),
        (
            "Workflow archive storage",
            ctx.storage_root.detect_legacy_archive(&ctx.repo_root),
        ),
        ("message storage", ctx.storage_root.detect_legacy_messages()),
        (
            "runtime observation storage",
            ctx.storage_root.detect_legacy_agent_state(),
        ),
        (
            "session anchor storage",
            ctx.storage_root.detect_legacy_sessions(),
        ),
    ];
    for (state_name, legacy) in legacy_roots {
        if let Some(legacy) = legacy {
            bail!("{}", legacy.error_message_for(state_name));
        }
    }
    if let Some(legacy) = ctx.storage_root.detect_legacy_local(&ctx.repo_root) {
        bail!("{}", legacy.error_message());
    }
    Ok(())
}

fn core_state_dirs(storage_root: &StorageRoot) -> Vec<PathBuf> {
    vec![
        storage_root.config_dir(),
        storage_root.profiles_dir(),
        storage_root.execution_dir(),
        storage_root.tasks_dir(),
        storage_root.workflows_dir(),
        storage_root.task_runs_dir(),
        storage_root.archive_dir(),
        storage_root.retrospectives_dir(),
        storage_root.runtime_dir(),
        storage_root.runtime_agents_dir(),
    ]
}

fn maybe_scaffold_claude_allow_rules(ctx: &Ctx, options: &InitOptions) -> Result<()> {
    if options.yes {
        return Ok(());
    }

    if !ctx.ui.confirm(
        "Claude가 .wt/**에 Edit/Write할 수 있도록 .claude/settings.local.json에 허용 규칙을 추가할까요?",
        false,
    )? {
        return Ok(());
    }

    let path = ctx.repo_root.join(CLAUDE_LOCAL_SETTINGS_PATH);
    merge_claude_allow_rules(&path)?;
    ctx.ui.print_step(&format!(
        "Claude local settings 업데이트됨: {}",
        path.display()
    ));
    Ok(())
}

fn merge_claude_allow_rules(path: &Path) -> Result<()> {
    let mut settings = read_claude_local_settings(path)?;
    merge_allow_rules_into_settings(&mut settings)?;
    write_claude_local_settings(path, &settings)
}

fn read_claude_local_settings(path: &Path) -> Result<serde_json::Value> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read Claude local settings: {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse Claude local settings JSON: {}",
            path.display()
        )
    })
}

fn write_claude_local_settings(path: &Path, settings: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create Claude local settings dir: {}",
                parent.display()
            )
        })?;
    }
    let rendered = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, format!("{rendered}\n"))
        .with_context(|| format!("Failed to write Claude local settings: {}", path.display()))
}

fn merge_allow_rules_into_settings(settings: &mut serde_json::Value) -> Result<()> {
    let Some(root) = settings.as_object_mut() else {
        bail!("Cannot update Claude local settings: root value must be a JSON object.");
    };

    let permissions_value = root
        .entry("permissions")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let Some(permissions) = permissions_value.as_object_mut() else {
        bail!("Cannot update Claude local settings: `permissions` must be a JSON object.");
    };

    if let Some(allow) = permissions.get("allow")
        && !allow.is_array()
    {
        bail!("Cannot update Claude local settings: `permissions.allow` must be a JSON array.");
    }
    permissions
        .entry("allow")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));

    let legacy_allowed = match root.get("allowed") {
        Some(serde_json::Value::Array(_)) => match root.remove("allowed") {
            Some(serde_json::Value::Array(values)) => values,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    let permissions = root
        .get_mut("permissions")
        .and_then(serde_json::Value::as_object_mut)
        .expect("permissions object was validated");
    let allowed = permissions
        .get_mut("allow")
        .and_then(serde_json::Value::as_array_mut)
        .expect("permissions.allow array was validated");

    let mut seen_wt_rules = std::collections::BTreeSet::new();
    let existing = std::mem::take(allowed);
    let mut deduped =
        Vec::with_capacity(existing.len() + legacy_allowed.len() + CLAUDE_ALLOW_RULES.len());
    for item in existing {
        if let Some(rule) = item.as_str()
            && CLAUDE_ALLOW_RULES.contains(&rule)
        {
            if seen_wt_rules.insert(rule.to_string()) {
                deduped.push(serde_json::Value::String(rule.to_string()));
            }
            continue;
        }
        deduped.push(item);
    }

    for item in legacy_allowed {
        if let Some(rule) = item.as_str()
            && CLAUDE_ALLOW_RULES.contains(&rule)
        {
            if seen_wt_rules.insert(rule.to_string()) {
                deduped.push(serde_json::Value::String(rule.to_string()));
            }
            continue;
        }
        if !deduped.iter().any(|existing| existing == &item) {
            deduped.push(item);
        }
    }

    for rule in CLAUDE_ALLOW_RULES {
        if seen_wt_rules.insert(rule.to_string()) {
            deduped.push(serde_json::Value::String(rule.to_string()));
        }
    }

    *allowed = deduped;
    Ok(())
}

fn is_interactive_wizard(options: &InitOptions) -> bool {
    !options.yes && !options.dry_run
}

fn print_wizard_header(ctx: &Ctx) {
    ctx.ui.print_step("wt init");
    ctx.ui
        .print_dim("이 저장소에 맞는 git worktree 프로젝트 설정을 추천합니다.");
}

fn print_wizard_step(ctx: &Ctx, number: usize, title: &str, description: impl AsRef<str>) {
    ctx.ui.print_dim("");
    ctx.ui.print_step(&format!("단계 {number}/5: {title}"));
    let description = description.as_ref();
    if !description.is_empty() {
        for line in description.lines() {
            ctx.ui.print_dim(&format!("  {line}"));
        }
    }
}

fn integration_step_description(
    options: &InitOptions,
    detected: &DetectedRepo,
    defaults: &InitDefaults,
) -> &'static str {
    if explicit_issue_provider(options.issue_provider.as_ref()).is_some()
        || explicit_site_provider(options.site_provider.as_ref()).is_some()
        || matches!(options.issue_provider, Some(InitIssueProvider::None))
        || matches!(options.site_provider, Some(InitSiteProvider::None))
    {
        return "명시한 issue/site 옵션을 사용합니다. 이미 정한 선택은 다시 묻지 않습니다.";
    }

    if detected.issue_provider.is_some()
        || detected.site_provider.is_some()
        || defaults.issue_provider.is_some()
        || defaults.site_provider.is_some()
    {
        return "이 저장소에서 찾은 issue 도구나 local site 설정을 쓸지 고릅니다.";
    }

    "이 저장소에서 issue 도구나 local site 설정을 찾지 못해 관련 section은 만들지 않습니다."
}

fn validate_options(options: &InitOptions) -> Result<()> {
    if matches!(options.agent, Some(InitAgent::None))
        && (options.agent_command.is_some() || !options.agent_args.is_empty())
    {
        bail!("--agent-command and --agent-arg cannot be used when --agent none");
    }
    Ok(())
}

fn resolve_target(ctx: &Ctx, options: &InitOptions) -> Result<InitTarget> {
    if options.local {
        return Ok(InitTarget {
            path: ctx.storage_root.config_toml(),
            kind: InitTargetKind::Local,
        });
    }
    if options.shared {
        return Ok(InitTarget {
            path: ctx.repo_root.join(".wt.toml"),
            kind: InitTargetKind::Shared,
        });
    }
    if options.yes {
        return Ok(InitTarget {
            path: ctx.storage_root.config_toml(),
            kind: InitTargetKind::Local,
        });
    }

    let items = vec![
        PromptItem::with_hint("개인 설정 파일", "보통 .wt/config/local.toml"),
        PromptItem::with_hint("팀 공유 설정", "./.wt.toml"),
    ];
    match ctx
        .ui
        .select_nested_items_without_filter("저장 위치", &items)?
    {
        0 => Ok(InitTarget {
            path: ctx.storage_root.config_toml(),
            kind: InitTargetKind::Local,
        }),
        _ => Ok(InitTarget {
            path: ctx.repo_root.join(".wt.toml"),
            kind: InitTargetKind::Shared,
        }),
    }
}

fn build_plan(ctx: &Ctx, options: &InitOptions, target: InitTarget) -> Result<InitPlan> {
    validate_options(options)?;
    let target_exists = target.path.exists();
    let defaults = if is_interactive_wizard(options) && target_exists {
        load_init_defaults(&target.path)
    } else {
        InitDefaults::default()
    };
    let detected = DetectedRepo::scan(&ctx.repo_root);
    if is_interactive_wizard(options) {
        print_wizard_step(
            ctx,
            2,
            "외부 도구 연결",
            integration_step_description(options, &detected, &defaults),
        );
    }
    let issue_provider = resolve_issue_provider(ctx, options, &detected, &defaults)?;
    let gh_user = if issue_provider == Some(InitIssueProvider::Github) {
        resolve_gh_user(ctx, options, &defaults)?
    } else {
        None
    };
    let site_provider = resolve_site_provider(ctx, options, &detected, &defaults)?;
    let workflow_policy = defaults
        .workflow_policy
        .unwrap_or_else(default_workflow_policy);
    let review_policy = defaults.review_policy;
    if is_interactive_wizard(options) {
        print_wizard_step(
            ctx,
            3,
            "개발 환경 설정",
            "wt가 새 worktree를 만들거나 열 때 쓸 파일, 명령, 탭, editor, browser 설정입니다.",
        );
    }
    let common = resolve_common_config(
        ctx,
        options,
        target.kind,
        &detected,
        &defaults,
        issue_provider.as_ref(),
        site_provider.as_ref(),
    )?;
    let profile = resolve_profile(ctx, options, &defaults)?;

    let mut s = String::new();
    let mut sections = Vec::new();

    if let Some(provider) = &issue_provider {
        sections.push(InitSection::Issues);
        s.push_str("[issues]\n");
        s.push_str(&format!(
            "provider = {}\n",
            toml_quote(issue_provider_name(provider))
        ));
        s.push_str(&format!(
            "origin_policy = {}\n",
            toml_quote(defaults.origin_policy.as_config_value())
        ));
        if *provider == InitIssueProvider::Github {
            if let Some(user) = gh_user.as_deref() {
                s.push_str(&format!("gh_user = {}\n", toml_quote(user)));
            }
        }
        s.push('\n');
    }

    if let Some(provider) = &site_provider {
        sections.push(InitSection::Site);
        s.push_str("[site]\n");
        s.push_str(&format!(
            "provider = {}\n",
            toml_quote(site_provider_name(provider))
        ));
        if matches!(*provider, InitSiteProvider::Traefik) {
            s.push_str("name = \"{{repo}}-{{branch_slug}}.l\"\n");
            s.push_str("url = \"https://{{site_name}}\"\n");
            s.push_str("target = \"http://127.0.0.1:{{vite_port}}\"\n");
        } else {
            s.push_str("name = \"{{repo}}-{{branch_slug}}\"\n");
        }
        if matches!(
            *provider,
            InitSiteProvider::Valet | InitSiteProvider::Traefik
        ) {
            s.push_str("secure = true\n");
        }
        s.push('\n');
    }

    append_workflow_policy(&mut s, workflow_policy, &mut sections);
    if let Some(review_policy) = review_policy {
        append_review_policy(&mut s, review_policy, &mut sections);
    }

    if let Some(profile) = &profile {
        sections.push(InitSection::ProfileAgent);
        if !profile.agent.prompt.is_empty() {
            sections.push(InitSection::ProfileAgentPrompt);
        }
        append_profile_selection(&mut s, profile);
    }

    append_active_common_config(&mut s, &common, &mut sections);

    toml::from_str::<Config>(&s)?;
    let mut notices = build_plan_notices(
        ctx,
        profile.as_ref(),
        issue_provider.as_ref(),
        site_provider.as_ref(),
        target.kind,
        &detected,
        &common,
    );
    if target_exists && (!options.yes || options.dry_run) {
        notices.push(InitNotice {
            level: InitNoticeLevel::Warn,
            message: existing_target_warning_message(&target.path, options),
        });
    }

    Ok(InitPlan {
        target_path: target.path,
        target_kind: target.kind,
        target_exists,
        sections,
        #[cfg(test)]
        detected_signals: detected.signals(),
        notices,
        content: s,
    })
}

fn print_plan(ctx: &Ctx, plan: &InitPlan, interactive_wizard: bool) {
    if interactive_wizard {
        print_wizard_step(
            ctx,
            4,
            "미리보기",
            "파일에 쓰기 전에 어디에 무엇을 저장할지와 생성될 TOML을 확인합니다.",
        );
    } else {
        ctx.ui.print_step("init 계획");
    }
    for line in render_plan_summary(plan) {
        ctx.ui.print_dim(&format!("  {line}"));
    }
    ctx.ui.print_dim("");
    ctx.ui.print_dim("  생성될 TOML:");
    ctx.ui.print_dim("    ---");
    for line in plan.content.lines() {
        ctx.ui.print_dim(&format!("    {line}"));
    }
    ctx.ui.print_dim("    ---");
}

fn render_plan_summary(plan: &InitPlan) -> Vec<String> {
    let saved_settings = if plan.sections.is_empty() {
        "없음".to_string()
    } else {
        plan.sections
            .iter()
            .map(|section| section.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let planned_write = if plan.target_exists {
        "기존 설정 덮어쓰기"
    } else {
        "새 설정 생성"
    };

    let mut lines = vec![
        "저장 대상".to_string(),
        format!("  저장할 파일: {}", plan.target_path.display()),
        format!("  저장 범위: {}", target_kind_name(plan.target_kind)),
        format!("  작업: {planned_write}"),
        format!("  저장될 설정: {saved_settings}"),
    ];

    let hints = plan
        .notices
        .iter()
        .filter(|notice| notice.level == InitNoticeLevel::Hint)
        .collect::<Vec<_>>();
    let warnings = plan
        .notices
        .iter()
        .filter(|notice| notice.level == InitNoticeLevel::Warn)
        .collect::<Vec<_>>();

    if !hints.is_empty() {
        lines.push("".to_string());
        lines.push("안내".to_string());
        lines.extend(hints.iter().map(|notice| format!("  - {}", notice.message)));
    }
    if !warnings.is_empty() {
        lines.push("".to_string());
        lines.push("경고".to_string());
        lines.extend(
            warnings
                .iter()
                .map(|notice| format!("  - {}", notice.message)),
        );
    }

    lines
}

fn print_existing_target_warning(ctx: &Ctx, plan: &InitPlan, options: &InitOptions) {
    ctx.ui
        .print_warning(&existing_target_warning_message(&plan.target_path, options));
}

fn existing_target_warning_message(path: &Path, options: &InitOptions) -> String {
    let suffix = if options.dry_run {
        "dry run이라 덮어쓰지 않음"
    } else if options.force {
        "--force로 덮어씀"
    } else if options.yes {
        "덮어쓰려면 --force 사용"
    } else {
        "계속하려면 덮어쓰기 확인 필요"
    };
    format!("설정 파일이 이미 있습니다: {} ({suffix})", path.display())
}

fn build_plan_notices(
    ctx: &Ctx,
    profile: Option<&InitProfile>,
    issue_provider: Option<&InitIssueProvider>,
    site_provider: Option<&InitSiteProvider>,
    target_kind: InitTargetKind,
    detected: &DetectedRepo,
    common: &InitCommonConfig,
) -> Vec<InitNotice> {
    let mut notices = Vec::new();

    push_shared_target_omission_notice(
        &mut notices,
        target_kind,
        detected,
        issue_provider,
        site_provider,
    );

    if let Some(profile) = profile {
        push_agent_tool_notice(ctx, &mut notices, &profile.agent);
        push_missing_command_warning(
            ctx,
            &mut notices,
            "cmux",
            "cmux CLI가 없습니다. 생성된 workspace 설정은 저장할 수 있지만 agent workspace를 열려면 cmux가 필요합니다",
        );
    }

    if let Some(provider) = issue_provider {
        match provider {
            InitIssueProvider::Github => push_missing_command_warning(
                ctx,
                &mut notices,
                "gh",
                "gh CLI가 없습니다. 생성된 GitHub issue 설정은 저장할 수 있지만 issue 선택에는 gh가 필요합니다",
            ),
            InitIssueProvider::Linear => push_missing_command_warning(
                ctx,
                &mut notices,
                "linear",
                "linear CLI가 없습니다. 생성된 Linear issue 설정은 저장할 수 있지만 issue 선택에는 linear가 필요합니다",
            ),
            InitIssueProvider::None => {}
        }

        let readiness = profile.map_or_else(
            || {
                "issue agent prompt: 선택된 agent runtime이 없습니다. issue 작업에서 agent를 바로 실행하려면 --agent <name>을 추가하세요".to_string()
            },
            |profile| {
                format!(
                    "issue agent prompt: {}로 실행 준비됨",
                    agent_cli_name(&profile.agent.cli)
                )
            },
        );
        push_notice(&mut notices, InitNoticeLevel::Hint, readiness);
    }

    if let Some(provider) = site_provider {
        match provider {
            InitSiteProvider::Herd => push_missing_command_warning(
                ctx,
                &mut notices,
                "herd",
                "herd CLI가 없습니다. 생성된 Herd site 설정은 저장할 수 있지만 site setup에는 herd가 필요합니다",
            ),
            InitSiteProvider::Valet => push_missing_command_warning(
                ctx,
                &mut notices,
                "valet",
                "valet CLI가 없습니다. 생성된 Valet site 설정은 저장할 수 있지만 site setup에는 valet이 필요합니다",
            ),
            InitSiteProvider::Traefik => push_missing_command_warning(
                ctx,
                &mut notices,
                "traefik",
                "traefik CLI가 없습니다. 생성된 Traefik site 설정은 저장할 수 있지만 site setup에는 traefik이 필요합니다",
            ),
            InitSiteProvider::DockerProxy | InitSiteProvider::None => {}
        }
    }

    if common.worktree_naming {
        push_missing_command_warning(
            ctx,
            &mut notices,
            "claude",
            "claude command가 없습니다. 생성된 worktree naming 설정은 저장할 수 있지만 AI assisted naming에는 claude가 필요합니다",
        );
    }

    if !common.post_deps_tabs.is_empty() {
        push_notice(
            &mut notices,
            InitNoticeLevel::Hint,
            format!("dev 탭: {}", common.post_deps_tabs.join("; ")),
        );
        push_missing_command_warning(
            ctx,
            &mut notices,
            "cmux",
            "cmux CLI가 없습니다. 생성된 dev 탭 설정은 저장할 수 있지만 자동 탭 실행에는 cmux가 필요합니다",
        );
    }

    if common.workspace_browser.is_some() {
        push_missing_command_warning(
            ctx,
            &mut notices,
            "cmux",
            "cmux CLI가 없습니다. 생성된 browser 설정은 저장할 수 있지만 자동 browser 실행에는 workspace setup이 필요합니다",
        );
    }

    notices
}

fn push_shared_target_omission_notice(
    notices: &mut Vec<InitNotice>,
    target_kind: InitTargetKind,
    detected: &DetectedRepo,
    issue_provider: Option<&InitIssueProvider>,
    site_provider: Option<&InitSiteProvider>,
) {
    if target_kind != InitTargetKind::Shared {
        return;
    }

    let mut omitted = Vec::new();
    if detected.has_env_file {
        omitted.push(".env copy".to_string());
    }
    if !detected.local_links.is_empty() {
        omitted.push(format!("local links ({})", detected.local_links.join(", ")));
    }
    if issue_provider.is_some() {
        omitted.push("worktree.naming".to_string());
    }
    if site_provider.is_some() {
        omitted.push("workspace browser profile".to_string());
    }

    if !omitted.is_empty() {
        push_notice(
            notices,
            InitNoticeLevel::Hint,
            format!(
                "팀 공유 설정에는 개인 helper를 쓰지 않습니다: {}; 머신별 setup까지 저장하려면 개인 설정 파일을 선택하세요",
                omitted.join(", ")
            ),
        );
    }
}

fn push_agent_tool_notice(ctx: &Ctx, notices: &mut Vec<InitNotice>, agent: &AgentConfig) {
    match required_agent_command(agent) {
        Ok(Some(command)) => push_missing_command_warning(
            ctx,
            notices,
            &command,
            format!(
                "{command} command가 없습니다. 생성된 agent 설정은 저장할 수 있지만 agent 실행에는 {command}가 필요합니다"
            ),
        ),
        Ok(None) => {}
        Err(err) => push_notice(
            notices,
            InitNoticeLevel::Warn,
            format!("agent command를 해석할 수 없습니다 ({err}); init 후 wt doctor를 실행하세요"),
        ),
    }
}

fn required_agent_command(agent: &AgentConfig) -> std::result::Result<Option<String>, String> {
    if let Some(command) = agent.command.as_deref() {
        return shell_words::split(command)
            .map(|parts| parts.first().cloned())
            .map_err(|err| err.to_string());
    }

    Ok(match agent.cli {
        AgentCli::Codex => Some("codex".into()),
        AgentCli::Claude => Some("claude".into()),
        AgentCli::Gemini => Some("gemini".into()),
        AgentCli::None => None,
    })
}

fn push_missing_command_warning(
    ctx: &Ctx,
    notices: &mut Vec<InitNotice>,
    command: &str,
    message: impl Into<String>,
) {
    if !ctx.runner.has_command(command) {
        push_notice(notices, InitNoticeLevel::Warn, message.into());
    }
}

fn push_notice(notices: &mut Vec<InitNotice>, level: InitNoticeLevel, message: String) {
    if notices
        .iter()
        .any(|notice| notice.level == level && notice.message == message)
    {
        return;
    }
    notices.push(InitNotice { level, message });
}

fn append_active_common_config(
    s: &mut String,
    common: &InitCommonConfig,
    sections: &mut Vec<InitSection>,
) {
    if common.worktree_path.is_some()
        || !common.worktree_copy.is_empty()
        || !common.worktree_copy_as.is_empty()
        || !common.worktree_link.is_empty()
        || common.inject_local_context.is_some()
    {
        sections.push(InitSection::Worktree);
        s.push_str("[worktree]\n");
        if let Some(path) = common.worktree_path.as_deref() {
            s.push_str(&format!("path = {}\n", toml_quote(path)));
        }
        if !common.worktree_copy.is_empty() {
            s.push_str(&format!("copy = {}\n", toml_array(&common.worktree_copy)));
        }
        if !common.worktree_copy_as.is_empty() {
            s.push_str("copy_as = [\n");
            for entry in &common.worktree_copy_as {
                s.push_str(&format!(
                    "    {{ from = {}, to = {} }},\n",
                    toml_quote(&entry.from),
                    toml_quote(&entry.to)
                ));
            }
            s.push_str("]\n");
        }
        if !common.worktree_link.is_empty() {
            s.push_str(&format!("link = {}\n", toml_array(&common.worktree_link)));
        }
        if let Some(context) = common.inject_local_context.as_deref() {
            append_multiline_string(s, "inject_local_context", context);
        }
        s.push('\n');
    }

    if common.worktree_naming {
        sections.push(InitSection::WorktreeNaming);
        s.push_str("[worktree.naming]\n");
        s.push_str("command = \"claude -p\"\n");
        s.push_str("branch = \"{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}\"\n\n");
    }

    if !common.setup_deps.is_empty() {
        sections.push(InitSection::Setup);
        s.push_str("[setup]\n");
        s.push_str("deps = [\n");
        for command in &common.setup_deps {
            append_command_entry(s, command);
        }
        s.push_str("]\n\n");
    }

    if let Some(command) = common.editor_command.as_deref() {
        sections.push(InitSection::Editor);
        s.push_str("[editor]\n");
        s.push_str(&format!("command = {}\n", toml_quote(command)));
        s.push_str("placement = \"cmux_surface\"\n\n");
    }

    sections.push(InitSection::Workspace);
    s.push_str("[workspace]\n");
    s.push_str("# cmux로 함께 열 보조 탭입니다. 없거나 불필요하면 []로 둡니다.\n");
    s.push_str(&format!("tabs = {}\n", toml_array(&common.workspace_tabs)));
    if !common.post_deps_tabs.is_empty() {
        s.push_str("# setup.deps 뒤에 시작할 개발 서버 탭입니다.\n");
        s.push_str(&format!(
            "post_deps_tabs = {}\n",
            toml_array(&common.post_deps_tabs)
        ));
    }
    s.push_str("# task/issue/branch/pr workspace 색상입니다.\n");
    s.push_str(&format!(
        "colors = {}\n",
        toml_inline_string_entries(&workspace_colors(common))
    ));
    s.push('\n');

    if let Some(browser) = common.workspace_browser.as_ref() {
        sections.push(InitSection::WorkspaceBrowser);
        s.push_str("[workspace.browser]\n");
        s.push_str(&format!(
            "mode = {}\n",
            toml_quote(workspace_browser_mode_name(browser.mode))
        ));
        if let Some(url) = browser.url.as_deref() {
            s.push_str(&format!("url = {}\n", toml_quote(url)));
        }
        if let Some(app) = browser.app.as_deref()
            && browser.mode == InitWorkspaceBrowserMode::System
        {
            s.push_str(&format!("app = {}\n", toml_quote(app)));
        }
        s.push('\n');

        if browser.mode == InitWorkspaceBrowserMode::ChromeDevtools {
            s.push_str("[workspace.browser.chrome_devtools]\n");
            if let Some(user_data_dir) = browser.chrome_devtools_user_data_dir.as_deref() {
                s.push_str(&format!("user_data_dir = {}\n", toml_quote(user_data_dir)));
            } else {
                s.push_str(
                    "user_data_dir = \"{{worktree_parent}}/.chrome-devtools/{{worktree_name}}\"\n",
                );
            }
            s.push('\n');
        }
    }
}

fn append_workflow_policy(
    s: &mut String,
    policy: WorkflowDefaultPolicy,
    sections: &mut Vec<InitSection>,
) {
    sections.push(InitSection::Workflow);
    s.push_str("[workflow]\n");
    s.push_str("# workflow task의 PR 처리: none | draft | ready\n");
    s.push_str(&format!(
        "pull_request = {}\n",
        toml_quote(workflow_pull_request_name(policy.pull_request))
    ));
    s.push_str("# review 통과 뒤 처리: manual은 대기, auto는 landing 진행\n");
    s.push_str(&format!(
        "landing = {}\n\n",
        toml_quote(workflow_landing_name(policy.landing))
    ));
}

fn append_review_policy(
    s: &mut String,
    policy: ReviewDefaultPolicy,
    sections: &mut Vec<InitSection>,
) {
    sections.push(InitSection::Review);
    s.push_str("[review]\n");
    s.push_str("# Codex base-diff review evidence 수집: none | advisory | required\n");
    s.push_str(&format!(
        "codex_base = {}\n\n",
        toml_quote(review_codex_base_name(policy.codex_base))
    ));
}

fn append_command_entry(s: &mut String, command: &InitCommand) {
    s.push_str("    { ");
    if let Some(label) = command.label.as_deref() {
        s.push_str(&format!("label = {}, ", toml_quote(label)));
    }
    if let Some(working_dir) = command.working_dir.as_deref() {
        s.push_str(&format!("working_dir = {}, ", toml_quote(working_dir)));
    }
    s.push_str(&format!("run = {}", toml_quote(&command.run)));
    if let Some(if_exists) = command.if_exists.as_deref() {
        s.push_str(&format!(", if_exists = {}", toml_quote(if_exists)));
    }
    s.push_str(" },\n");
}

fn workspace_colors(common: &InitCommonConfig) -> Vec<(String, String)> {
    if common.workspace_colors.is_empty() {
        return default_workspace_colors();
    }
    common.workspace_colors.clone()
}

fn default_workspace_colors() -> Vec<(String, String)> {
    WORKSPACE_DEFAULT_COLORS
        .iter()
        .map(|(kind, color)| (kind.to_string(), color.to_string()))
        .collect()
}

fn toml_inline_string_entries(entries: &[(String, String)]) -> String {
    let rendered = entries
        .iter()
        .map(|(key, value)| format!("{} = {}", toml_key(key), toml_quote(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {rendered} }}")
}

fn toml_key(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        value.into()
    } else {
        toml_quote(value)
    }
}

fn append_multiline_string(s: &mut String, key: &str, value: &str) {
    s.push_str(key);
    s.push_str(" = \"\"\"\n");
    s.push_str(value);
    if !value.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("\"\"\"\n");
}

fn resolve_common_config(
    ctx: &Ctx,
    options: &InitOptions,
    target_kind: InitTargetKind,
    detected: &DetectedRepo,
    defaults: &InitDefaults,
    issue_provider: Option<&InitIssueProvider>,
    site_provider: Option<&InitSiteProvider>,
) -> Result<InitCommonConfig> {
    let existing_common = defaults.common.clone();
    let mut config = existing_common.clone().unwrap_or_else(|| {
        detected_project_common_config(ctx, target_kind, detected, issue_provider, site_provider)
    });
    if issue_provider.is_none() || target_kind != InitTargetKind::Local {
        config.worktree_naming = false;
    }
    if site_provider.is_none() {
        config.workspace_browser = None;
    }
    let has_existing_defaults = defaults.from_existing_config && existing_common.is_some();

    if options.yes {
        return Ok(config);
    }

    ctx.ui.print_dim(
        "생성되는 TOML에는 선택한 active 설정만 들어가고, 주석 처리된 예시는 넣지 않습니다.",
    );
    let recommended_item = if has_existing_defaults {
        PromptItem::with_description(
            "기존 설정 파일 값 유지하기",
            "1단계에서 고른 설정 파일에 저장된 값을 유지하고 editor/browser만 확인합니다.",
        )
    } else if cmux_available(ctx) {
        PromptItem::with_description(
            "감지한 개발 설정 저장",
            "감지한 setup 명령, 로컬 파일, workspace 설정을 저장합니다.",
        )
    } else {
        PromptItem::with_description(
            "감지한 개발 설정 저장",
            "감지한 setup 명령과 로컬 파일을 저장하고 workspace 자동화는 비워둡니다.",
        )
    };
    let items = vec![
        recommended_item,
        PromptItem::with_description(
            "개발 설정 직접 고르기",
            "worktree 위치, 파일, 명령, 탭, editor/browser를 직접 고릅니다.",
        ),
        PromptItem::with_description(
            "자동화 없이 최소 설정",
            "setup/editor/browser 없이 빈 workspace 설정만 저장합니다.",
        ),
    ];
    match ctx
        .ui
        .select_nested_items_without_filter("개발 환경 설정을 어떻게 만들까요?", &items)?
    {
        0 => resolve_recommended_common_config(ctx, target_kind, config, site_provider),
        1 => resolve_custom_common_config(ctx, target_kind, config, detected, site_provider),
        _ => Ok(InitCommonConfig::default()),
    }
}

fn detected_project_common_config(
    ctx: &Ctx,
    target_kind: InitTargetKind,
    detected: &DetectedRepo,
    issue_provider: Option<&InitIssueProvider>,
    site_provider: Option<&InitSiteProvider>,
) -> InitCommonConfig {
    let local_target = target_kind == InitTargetKind::Local;
    let workspace_automation = cmux_available(ctx);
    let worktree_copy = if local_target && detected.has_env_file {
        vec![".env".into()]
    } else {
        Vec::new()
    };
    InitCommonConfig {
        worktree_copy,
        worktree_copy_as: Vec::new(),
        worktree_link: if local_target {
            detected.local_links.clone()
        } else {
            Vec::new()
        },
        inject_local_context: local_target.then(|| DEFAULT_INJECT_LOCAL_CONTEXT.into()),
        worktree_naming: local_target && issue_provider.is_some(),
        setup_deps: default_enabled_setup_deps(detected),
        post_deps_tabs: if workspace_automation {
            detected.post_deps_tabs.clone()
        } else {
            Vec::new()
        },
        workspace_tabs: if local_target {
            recommended_workspace_tabs(ctx)
        } else {
            Vec::new()
        },
        workspace_browser: recommended_workspace_browser(ctx, target_kind, site_provider),
        ..InitCommonConfig::default()
    }
}

fn resolve_recommended_common_config(
    ctx: &Ctx,
    target_kind: InitTargetKind,
    mut config: InitCommonConfig,
    site_provider: Option<&InitSiteProvider>,
) -> Result<InitCommonConfig> {
    if target_kind == InitTargetKind::Local {
        config.editor_command = resolve_editor_command(ctx, config.editor_command.as_deref())?;
    }
    if cmux_available(ctx) && site_provider.is_some() {
        config.workspace_browser = resolve_workspace_browser(ctx, config.workspace_browser)?;
    } else {
        config.workspace_browser = None;
    }
    Ok(config)
}

fn resolve_custom_common_config(
    ctx: &Ctx,
    target_kind: InitTargetKind,
    mut config: InitCommonConfig,
    detected: &DetectedRepo,
    site_provider: Option<&InitSiteProvider>,
) -> Result<InitCommonConfig> {
    config.worktree_path = resolve_worktree_path(ctx, config.worktree_path.as_deref())?;
    if target_kind == InitTargetKind::Local {
        config.worktree_copy = resolve_worktree_copy(ctx, &config.worktree_copy, detected)?;
        config.worktree_link = resolve_worktree_link(ctx, &config.worktree_link, detected)?;
    } else {
        config.worktree_copy.clear();
        config.worktree_link.clear();
    }
    config.workspace_tabs = resolve_workspace_tabs(ctx, target_kind, &config.workspace_tabs)?;

    config.setup_deps = resolve_setup_deps(ctx, detected, &config.setup_deps)?;

    if !detected.post_deps_tabs.is_empty() {
        ctx.ui.print_dim(
            "dev 탭은 dependency setup이 끝난 뒤 개발 서버 command를 별도 탭에서 시작합니다.",
        );
        print_detected_dev_server_commands(ctx, &detected.post_deps_tabs);
        let prompt = dev_server_confirm_prompt(&detected.post_deps_tabs);
        if ctx.ui.confirm(&prompt, !config.post_deps_tabs.is_empty())? {
            config.post_deps_tabs = detected.post_deps_tabs.clone();
        } else {
            config.post_deps_tabs.clear();
        }
    }

    config.editor_command = resolve_editor_command(ctx, config.editor_command.as_deref())?;
    if cmux_available(ctx) && site_provider.is_some() {
        config.workspace_browser = resolve_workspace_browser(ctx, config.workspace_browser)?;
    } else {
        config.workspace_browser = None;
    }
    Ok(config)
}

fn default_enabled_setup_deps(detected: &DetectedRepo) -> Vec<InitCommand> {
    detected
        .setup_deps
        .iter()
        .filter(|command| command.default_enabled)
        .cloned()
        .collect()
}

#[cfg(test)]
fn push_signal(signals: &mut Vec<String>, signal: String) {
    if !signals.contains(&signal) {
        signals.push(signal);
    }
}

fn cmux_available(ctx: &Ctx) -> bool {
    ctx.runner.has_command("cmux")
}

fn recommended_workspace_tabs(ctx: &Ctx) -> Vec<String> {
    if !cmux_available(ctx) {
        return Vec::new();
    }
    ["lazygit", "nvim"]
        .into_iter()
        .filter(|command| ctx.runner.has_command(command))
        .map(str::to_string)
        .collect()
}

fn recommended_workspace_browser(
    ctx: &Ctx,
    target_kind: InitTargetKind,
    site_provider: Option<&InitSiteProvider>,
) -> Option<InitWorkspaceBrowser> {
    if target_kind != InitTargetKind::Local || site_provider.is_none() || !cmux_available(ctx) {
        return None;
    }
    Some(InitWorkspaceBrowser {
        mode: InitWorkspaceBrowserMode::ChromeDevtools,
        url: None,
        app: None,
        chrome_devtools_user_data_dir: Some(
            "{{worktree_parent}}/.chrome-devtools/{{worktree_name}}".into(),
        ),
    })
}

fn resolve_worktree_path(ctx: &Ctx, default: Option<&str>) -> Result<Option<String>> {
    let mut options = vec![
        (
            PromptItem::with_hint("현재 저장소 옆에 만들기", "../{{default_name}}"),
            None,
        ),
        (
            PromptItem::with_hint(
                "홈 worktrees 폴더에 만들기",
                "$HOME/worktrees/{{default_name}}",
            ),
            Some("$HOME/worktrees/{{default_name}}".to_string()),
        ),
    ];
    if let Some(default) = default
        && !options
            .iter()
            .any(|(_, value)| value.as_deref() == Some(default))
    {
        options.insert(
            0,
            (
                PromptItem::with_hint("현재 설정값 유지", default),
                Some(default.to_string()),
            ),
        );
    }
    options.push((PromptItem::new("직접 입력"), None));

    let items = options
        .iter()
        .map(|(item, _)| item.clone())
        .collect::<Vec<_>>();
    ctx.ui
        .print_dim("새 branch checkout을 어느 폴더에 만들지 고릅니다.");
    let selection = ctx
        .ui
        .select_nested_items_without_filter("worktree 만들 위치", &items)?;
    if selection < options.len() - 1 {
        return Ok(options[selection].1.clone());
    }

    let input = ctx.ui.input(
        "worktree 만들 위치 템플릿",
        default.or(Some("$HOME/worktrees/{{default_name}}")),
    )?;
    let input = input.trim();
    Ok((!input.is_empty()).then(|| input.to_string()))
}

fn resolve_worktree_copy(
    ctx: &Ctx,
    current: &[String],
    detected: &DetectedRepo,
) -> Result<Vec<String>> {
    if !detected.has_env_file && current.is_empty() {
        return Ok(Vec::new());
    }

    let mut recommended = current.to_vec();
    if detected.has_env_file && !recommended.iter().any(|path| path == ".env") {
        recommended.push(".env".into());
    }
    let default = recommended.join(", ");
    let input = ctx.ui.input(
        "각 worktree로 복사할 파일",
        Some(if default.is_empty() {
            ""
        } else {
            default.as_str()
        }),
    )?;
    Ok(split_list(&input))
}

fn resolve_worktree_link(
    ctx: &Ctx,
    current: &[String],
    detected: &DetectedRepo,
) -> Result<Vec<String>> {
    if detected.local_links.is_empty() && current.is_empty() {
        return Ok(Vec::new());
    }

    let mut recommended = current.to_vec();
    for link in &detected.local_links {
        if !recommended.contains(link) {
            recommended.push(link.clone());
        }
    }
    let default = recommended.join(", ");
    let input = ctx.ui.input(
        "각 worktree에 링크할 로컬 파일",
        Some(if default.is_empty() {
            ""
        } else {
            default.as_str()
        }),
    )?;
    Ok(split_list(&input))
}

fn resolve_workspace_tabs(
    ctx: &Ctx,
    target_kind: InitTargetKind,
    default_tabs: &[String],
) -> Result<Vec<String>> {
    let default = default_tabs.join(", ");
    if target_kind == InitTargetKind::Local {
        ctx.ui.print_dim(
            "worktree를 열 때 cmux 안에 같이 띄울 개인 보조 명령입니다. 없으면 비워둡니다.",
        );
    } else {
        ctx.ui.print_dim(
            "팀에 공유할 cmux 보조 탭입니다. 개인 도구는 local 설정에 두는 편이 안전합니다.",
        );
    }
    let input = ctx.ui.input(
        "worktree 열 때 같이 띄울 명령",
        Some(if default_tabs.is_empty() {
            ""
        } else {
            default.as_str()
        }),
    )?;
    let tabs = split_list(&input);
    if tabs.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(tabs)
    }
}

fn resolve_editor_command(ctx: &Ctx, default: Option<&str>) -> Result<Option<String>> {
    let mut options = vec![
        ("시스템 editor 사용".to_string(), None),
        ("vim {{path}}".to_string(), Some("vim {{path}}".to_string())),
        (
            "code {{path}}".to_string(),
            Some("code {{path}}".to_string()),
        ),
        (
            "phpstorm {{path}}".to_string(),
            Some("phpstorm {{path}}".to_string()),
        ),
        (
            "pstorm {{path}}".to_string(),
            Some("pstorm {{path}}".to_string()),
        ),
    ];
    if let Some(default) = default
        && !options
            .iter()
            .any(|(_, value)| value.as_deref() == Some(default))
    {
        options.insert(0, (format!("현재값: {default}"), Some(default.to_string())));
    }
    options.push(("editor command 직접 입력".to_string(), None));

    let items = options
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    ctx.ui.print_dim(
        "editor command는 wt가 관리하는 설정 파일을 엽니다. {{path}}는 파일 경로로 바뀝니다.",
    );
    let selection = ctx
        .ui
        .select_nested_without_filter("설정 editor command", &items)?;
    if selection < options.len() - 1 {
        return Ok(options[selection].1.clone());
    }

    let input = ctx
        .ui
        .input("editor command 직접 입력", default.or(Some("vim {{path}}")))?;
    let input = input.trim();
    Ok((!input.is_empty()).then(|| input.to_string()))
}

fn resolve_workspace_browser(
    ctx: &Ctx,
    default: Option<InitWorkspaceBrowser>,
) -> Result<Option<InitWorkspaceBrowser>> {
    let mut options = Vec::new();
    if let Some(default) = default.as_ref() {
        options.push((
            format!("현재값: {}", workspace_browser_choice_label(default)),
            Some(default.clone()),
        ));
    }
    push_browser_option(
        &mut options,
        "Chrome DevTools".into(),
        Some(InitWorkspaceBrowser {
            mode: InitWorkspaceBrowserMode::ChromeDevtools,
            url: None,
            app: None,
            chrome_devtools_user_data_dir: Some(
                "{{worktree_parent}}/.chrome-devtools/{{worktree_name}}".into(),
            ),
        }),
    );
    push_browser_option(
        &mut options,
        "시스템 browser".into(),
        Some(InitWorkspaceBrowser {
            mode: InitWorkspaceBrowserMode::System,
            url: None,
            app: Some("Google Chrome".into()),
            chrome_devtools_user_data_dir: None,
        }),
    );
    push_browser_option(&mut options, "browser 열지 않음".into(), None);

    let items = options
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    ctx.ui
        .print_dim("workspace browser는 workspace에서 local site를 열 browser surface를 정합니다.");
    Ok(options[ctx
        .ui
        .select_nested_without_filter("workspace browser", &items)?]
    .1
    .clone())
}

fn push_browser_option(
    options: &mut Vec<(String, Option<InitWorkspaceBrowser>)>,
    label: String,
    browser: Option<InitWorkspaceBrowser>,
) {
    if !options
        .iter()
        .any(|(_, existing)| same_browser_choice(existing.as_ref(), browser.as_ref()))
    {
        options.push((label, browser));
    }
}

fn same_browser_choice(
    left: Option<&InitWorkspaceBrowser>,
    right: Option<&InitWorkspaceBrowser>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.mode == right.mode && left.url == right.url && left.app == right.app
        }
        _ => false,
    }
}

fn workspace_browser_choice_label(browser: &InitWorkspaceBrowser) -> &'static str {
    match browser.mode {
        InitWorkspaceBrowserMode::System => "시스템 browser",
        InitWorkspaceBrowserMode::ChromeDevtools => "Chrome DevTools",
    }
}

fn split_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn resolve_setup_deps(
    ctx: &Ctx,
    detected: &DetectedRepo,
    current: &[InitCommand],
) -> Result<Vec<InitCommand>> {
    let mut selected = Vec::new();
    if !detected.setup_deps.is_empty() {
        ctx.ui
            .print_dim("setup command는 wt가 새 worktree를 만든 뒤 실행됩니다.");
    }
    for mut command in detected.setup_deps.clone() {
        if let Some(existing) = current.iter().find(|existing| {
            existing.working_dir == command.working_dir
                && existing.if_exists == command.if_exists
                && existing.kind == command.kind
                && (existing.kind == InitCommandKind::NodeInstall || existing.run == command.run)
        }) {
            command.run = existing.run.clone();
            command.default_enabled = true;
        }
        let display = command_display(&command);
        if !ctx.ui.confirm(
            &format!("감지된 setup command를 사용할까요 ({display})?"),
            command.default_enabled,
        )? {
            continue;
        }
        if command.kind == InitCommandKind::NodeInstall {
            command.run = resolve_node_install_command(ctx, &command)?;
        }
        selected.push(command);
    }
    Ok(selected)
}

fn resolve_node_install_command(ctx: &Ctx, command: &InitCommand) -> Result<String> {
    let detected = command.run.as_str();
    let mut options = Vec::new();
    push_node_install_option(&mut options, format!("{detected} (감지됨)"), detected);
    push_node_install_option(&mut options, "npm install".into(), "npm install");
    push_node_install_option(&mut options, "pnpm install".into(), "pnpm install");
    push_node_install_option(&mut options, "yarn install".into(), "yarn install");
    push_node_install_option(&mut options, "bun install".into(), "bun install");
    push_node_install_option(
        &mut options,
        "nvm use && npm install".into(),
        "bash -lc 'source \"$HOME/.nvm/nvm.sh\" && nvm use && npm install'",
    );

    let mut items = options
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    items.push("직접 입력".into());

    let prompt = command.working_dir.as_deref().map_or_else(
        || "패키지 설치 명령".to_string(),
        |working_dir| format!("패키지 설치 명령 ({working_dir})"),
    );
    let selection = ctx.ui.select_nested_without_filter(&prompt, &items)?;
    if selection < options.len() {
        return Ok(options[selection].1.clone());
    }

    let input = ctx.ui.input("설치 명령 직접 입력", Some(detected))?;
    let input = input.trim();
    Ok(if input.is_empty() {
        detected.to_string()
    } else {
        input.to_string()
    })
}

fn push_node_install_option(options: &mut Vec<(String, String)>, label: String, run: &str) {
    if !options.iter().any(|(_, existing)| existing == run) {
        options.push((label, run.to_string()));
    }
}

fn print_detected_dev_server_commands(ctx: &Ctx, commands: &[String]) {
    if commands.len() == 1 {
        ctx.ui
            .print_dim(&format!("감지한 dev server command: {}", commands[0]));
        return;
    }

    ctx.ui.print_dim("감지한 dev server commands:");
    for command in commands {
        ctx.ui.print_dim(&format!("  - {command}"));
    }
}

fn dev_server_confirm_prompt(commands: &[String]) -> String {
    if commands.len() == 1 {
        return format!("setup 후 {}를 dev 탭에서 시작할까요?", commands[0]);
    }
    "setup 후 감지한 dev server 명령들을 dev 탭에서 시작할까요?".into()
}

fn detect_setup_deps(repo_root: &Path) -> Vec<InitCommand> {
    let mut commands = Vec::new();
    for rel_dir in detect_manifest_dirs(
        repo_root,
        &["package.json", "composer.json", "Gemfile", "pyproject.toml"],
    ) {
        let project_root = repo_root.join(&rel_dir);
        let working_dir = relative_dir(&rel_dir);
        if project_root.join("package.json").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir: working_dir.clone(),
                run: node_install_command(&project_root),
                if_exists: None,
                kind: InitCommandKind::NodeInstall,
                default_enabled: true,
            });
        }
        if project_root.join("composer.json").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir: working_dir.clone(),
                run: "composer install --no-interaction --no-progress".into(),
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: true,
            });
        }
        if project_root.join("Gemfile").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir: working_dir.clone(),
                run: "bundle install".into(),
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: true,
            });
        }
        if project_root.join("pyproject.toml").exists() {
            commands.push(InitCommand {
                label: None,
                working_dir,
                run: "uv sync".into(),
                if_exists: None,
                kind: InitCommandKind::Other,
                default_enabled: is_probable_uv_project(&project_root),
            });
        }
    }
    commands
}

fn detect_post_deps_tabs(repo_root: &Path) -> Vec<String> {
    detect_package_roots(repo_root)
        .into_iter()
        .filter_map(|rel_dir| {
            let project_root = repo_root.join(&rel_dir);
            let working_dir = relative_dir(&rel_dir);
            package_script_command(&project_root, "dev")
                .map(|run| command_for_workspace_tab(working_dir.as_deref(), &run))
        })
        .collect()
}

fn detect_issue_provider(repo_root: &Path) -> Option<InitIssueProvider> {
    repo_root
        .join(".linear.toml")
        .exists()
        .then_some(InitIssueProvider::Linear)
}

fn detect_site_provider(repo_root: &Path) -> Option<InitSiteProvider> {
    if is_laravel_project(repo_root) {
        return Some(InitSiteProvider::Herd);
    }
    None
}

fn detect_local_links(repo_root: &Path) -> Vec<String> {
    [".local", ".linear.toml", "CLAUDE.local.md"]
        .into_iter()
        .filter(|path| repo_root.join(path).exists())
        .map(str::to_string)
        .collect()
}

fn is_laravel_project(repo_root: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(repo_root.join("composer.json")) else {
        return false;
    };
    content.contains("\"laravel/framework\"")
}

fn command_display(command: &InitCommand) -> String {
    let command_part = command.working_dir.as_deref().map_or_else(
        || command.run.clone(),
        |working_dir| format!("{working_dir}: {}", command.run),
    );
    command
        .if_exists
        .as_deref()
        .map_or(command_part.clone(), |guard| {
            let guard = command.working_dir.as_deref().map_or_else(
                || guard.to_string(),
                |working_dir| format!("{working_dir}/{guard}"),
            );
            format!("{command_part} (when {guard} exists)")
        })
}

fn command_for_workspace_tab(working_dir: Option<&str>, run: &str) -> String {
    working_dir.map_or_else(
        || run.to_string(),
        |working_dir| format!("cd {} && {run}", shell_words::quote(working_dir)),
    )
}

fn package_script_command(project_root: &Path, script: &str) -> Option<String> {
    if !package_script_exists(project_root, script) {
        return None;
    }

    Some(format!(
        "{} run {script}",
        node_package_manager(project_root)
    ))
}

fn package_script_exists(project_root: &Path, script: &str) -> bool {
    let Ok(content) = std::fs::read_to_string(project_root.join("package.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    value
        .get("scripts")
        .and_then(|scripts| scripts.get(script))
        .and_then(|value| value.as_str())
        .is_some()
}

fn detect_package_roots(repo_root: &Path) -> Vec<PathBuf> {
    detect_manifest_dirs(repo_root, &["package.json"])
}

fn detect_manifest_dirs(repo_root: &Path, manifests: &[&str]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    scan_manifest_dirs(repo_root, Path::new(""), 0, manifests, &mut roots);
    roots.sort_by_key(|path| path.to_string_lossy().to_string());
    roots.dedup();
    roots
}

fn scan_manifest_dirs(
    repo_root: &Path,
    rel_dir: &Path,
    depth: usize,
    manifests: &[&str],
    roots: &mut Vec<PathBuf>,
) {
    const MAX_MANIFEST_SCAN_DEPTH: usize = 4;

    let dir = repo_root.join(rel_dir);
    if manifests.iter().any(|manifest| dir.join(manifest).exists()) {
        roots.push(rel_dir.to_path_buf());
    }
    if depth >= MAX_MANIFEST_SCAN_DEPTH {
        return;
    }

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut child_dirs = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then(|| entry.file_name())
        })
        .filter_map(|name| name.into_string().ok())
        .filter(|name| !is_ignored_manifest_dir(name))
        .collect::<Vec<_>>();
    child_dirs.sort();

    for child in child_dirs {
        let child_rel = if rel_dir.as_os_str().is_empty() {
            PathBuf::from(child)
        } else {
            rel_dir.join(child)
        };
        scan_manifest_dirs(repo_root, &child_rel, depth + 1, manifests, roots);
    }
}

fn is_ignored_manifest_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".local"
            | ".next"
            | ".nuxt"
            | ".turbo"
            | ".cache"
            | "node_modules"
            | "references"
            | "vendor"
            | "target"
            | "dist"
            | "build"
            | "coverage"
            | "tmp"
    )
}

fn relative_dir(rel_dir: &Path) -> Option<String> {
    (!rel_dir.as_os_str().is_empty()).then(|| rel_dir.to_string_lossy().replace('\\', "/"))
}

fn is_probable_uv_project(project_root: &Path) -> bool {
    if project_root.join("uv.lock").exists() {
        return true;
    }
    let Ok(pyproject) = std::fs::read_to_string(project_root.join("pyproject.toml")) else {
        return false;
    };
    pyproject.contains("[tool.uv") || pyproject.contains("[dependency-groups]")
}

fn node_package_manager(project_root: &Path) -> &'static str {
    package_manager_from_package_json(project_root).unwrap_or_else(|| {
        if project_root.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else if project_root.join("bun.lock").exists() || project_root.join("bun.lockb").exists()
        {
            "bun"
        } else if project_root.join("yarn.lock").exists() {
            "yarn"
        } else {
            "npm"
        }
    })
}

fn node_install_command(project_root: &Path) -> String {
    let manager = node_package_manager(project_root);
    if manager == "npm" && project_root.join("package-lock.json").exists() {
        "npm ci".into()
    } else {
        format!("{manager} install")
    }
}

fn package_manager_from_package_json(project_root: &Path) -> Option<&'static str> {
    let content = std::fs::read_to_string(project_root.join("package.json")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    let package_manager = value.get("packageManager")?.as_str()?;
    let name = package_manager.split('@').next().unwrap_or(package_manager);
    known_node_package_manager(name)
}

fn known_node_package_manager(name: &str) -> Option<&'static str> {
    match name {
        "npm" => Some("npm"),
        "pnpm" => Some("pnpm"),
        "yarn" => Some("yarn"),
        "bun" => Some("bun"),
        _ => None,
    }
}

fn resolve_profile(
    ctx: &Ctx,
    options: &InitOptions,
    defaults: &InitDefaults,
) -> Result<Option<InitProfile>> {
    if !explicit_agent_requested(options)
        && defaults.agent.is_none()
        && (options.yes || options.dry_run)
    {
        return Ok(None);
    }

    let agent = resolve_agent(ctx, options, defaults)?;
    let command = resolve_agent_command(options, &agent, defaults)?;
    if agent == InitAgent::None {
        if command.is_some() {
            bail!("--agent-command cannot be used when --agent none");
        }
        if !options.agent_args.is_empty() {
            bail!("--agent-arg cannot be used when --agent none");
        }
        return Ok(None);
    }
    let args = resolve_agent_args(ctx, &agent, options, defaults)?;
    Ok(build_profile(&agent, args, command, defaults))
}

fn resolve_agent(ctx: &Ctx, options: &InitOptions, defaults: &InitDefaults) -> Result<InitAgent> {
    if let Some(agent) = &options.agent {
        return Ok(agent.clone());
    }
    if options.yes {
        return Ok(InitAgent::None);
    }

    let default_agent = defaults
        .agent
        .as_ref()
        .map(|agent| init_agent_from_cli(&agent.cli));
    let choices = ordered_agents(default_agent);
    let items = choices.iter().map(agent_choice_label).collect::<Vec<_>>();
    Ok(choices[ctx.ui.select_nested_without_filter("코딩 agent", &items)?].clone())
}

fn resolve_agent_command(
    options: &InitOptions,
    agent: &InitAgent,
    defaults: &InitDefaults,
) -> Result<Option<String>> {
    if let Some(command) = &options.agent_command {
        return Ok(Some(command.clone()));
    }
    if *agent == InitAgent::None {
        return Ok(None);
    }
    if let Some(default_agent) = matching_default_agent(defaults, agent) {
        return Ok(default_agent.command.clone());
    }
    Ok(None)
}

fn resolve_agent_args(
    ctx: &Ctx,
    agent: &InitAgent,
    options: &InitOptions,
    defaults: &InitDefaults,
) -> Result<Vec<String>> {
    if !options.agent_args.is_empty() {
        return Ok(options.agent_args.clone());
    }
    if options.yes {
        return Ok(Vec::new());
    }
    if *agent == InitAgent::None {
        return Ok(Vec::new());
    }

    let default_args = matching_default_agent(defaults, agent)
        .map(|agent| agent.args.clone())
        .unwrap_or_default();
    let default_args_input = (!default_args.is_empty()).then(|| default_args.join(" "));
    if default_args.is_empty() {
        if !ctx.ui.confirm("agent 실행 args를 추가할까요?", false)? {
            return Ok(Vec::new());
        }
        return input_agent_args(ctx, None);
    }

    let mut items = Vec::new();
    let default = default_args_input.as_deref().unwrap_or_default();
    items.push(format!("기존 args 유지: {default}"));
    items.push("새 args 입력".into());
    items.push("args 비우기".into());

    let selection = ctx
        .ui
        .select_nested_without_filter("agent 실행 args", &items)?;
    match selection {
        0 => Ok(default_args),
        1 => input_agent_args(ctx, default_args_input.as_deref()),
        _ => Ok(Vec::new()),
    }
}

fn input_agent_args(ctx: &Ctx, default: Option<&str>) -> Result<Vec<String>> {
    let input = ctx.ui.input("agent args 직접 입력", default)?;
    Ok(input
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>())
}

fn resolve_issue_provider(
    ctx: &Ctx,
    options: &InitOptions,
    detected: &DetectedRepo,
    defaults: &InitDefaults,
) -> Result<Option<InitIssueProvider>> {
    if let Some(provider) = explicit_issue_provider(options.issue_provider.as_ref()) {
        return Ok(Some(provider));
    }
    if matches!(options.issue_provider, Some(InitIssueProvider::None)) {
        return Ok(None);
    }
    let recommended = defaults
        .issue_provider
        .clone()
        .or_else(|| detected.issue_provider.clone());
    if recommended.is_none() {
        return Ok(None);
    }
    if options.yes {
        return Ok(recommended);
    }

    let choices = ordered_issue_providers(recommended.as_ref());
    let items = choices
        .iter()
        .map(issue_provider_choice_label)
        .collect::<Vec<_>>();
    let provider = choices[ctx.ui.select_nested_without_filter("issue 도구", &items)?].clone();
    if provider == InitIssueProvider::None {
        Ok(None)
    } else {
        Ok(Some(provider))
    }
}

fn resolve_site_provider(
    ctx: &Ctx,
    options: &InitOptions,
    detected: &DetectedRepo,
    defaults: &InitDefaults,
) -> Result<Option<InitSiteProvider>> {
    if let Some(provider) = explicit_site_provider(options.site_provider.as_ref()) {
        return Ok(Some(provider));
    }
    if matches!(options.site_provider, Some(InitSiteProvider::None)) {
        return Ok(None);
    }
    let recommended = defaults
        .site_provider
        .clone()
        .or_else(|| detected.site_provider.clone());
    if recommended.is_none() {
        return Ok(None);
    }
    if options.yes {
        return Ok(recommended);
    }

    let choices = ordered_site_providers(recommended.as_ref());
    let items = choices
        .iter()
        .map(site_provider_choice_label)
        .collect::<Vec<_>>();
    let provider = choices[ctx
        .ui
        .select_nested_without_filter("local site 설정", &items)?]
    .clone();
    if provider == InitSiteProvider::None {
        Ok(None)
    } else {
        Ok(Some(provider))
    }
}

fn resolve_review_policy(
    ctx: &Ctx,
    options: &InitOptions,
    defaults: &InitDefaults,
) -> Result<Option<ReviewDefaultPolicy>> {
    if options.yes {
        return Ok(defaults.review_policy);
    }

    let default_codex_base = defaults
        .review_policy
        .map(|policy| policy.codex_base)
        .unwrap_or(ReviewCodexBasePolicy::None);
    let choices = ordered_values(
        &[
            ReviewCodexBasePolicy::None,
            ReviewCodexBasePolicy::Advisory,
            ReviewCodexBasePolicy::Required,
        ],
        Some(&default_codex_base),
    );
    let items = choices
        .iter()
        .map(|policy| review_policy_choice_item(*policy))
        .collect::<Vec<_>>();

    ctx.ui.print_dim(
        "review 정책 — Codex base-diff 리뷰 증거 수집 (none/advisory/required)",
    );
    let selected = choices[ctx
        .ui
        .select_nested_items_without_filter("Codex base-diff 리뷰 증거 정책", &items)?];

    if selected == ReviewCodexBasePolicy::None {
        Ok(None)
    } else {
        Ok(Some(ReviewDefaultPolicy {
            codex_base: selected,
        }))
    }
}

fn resolve_gh_user(
    ctx: &Ctx,
    options: &InitOptions,
    defaults: &InitDefaults,
) -> Result<Option<String>> {
    if let Some(user) = options.gh_user.as_deref() {
        let user = user.trim();
        return Ok((!user.is_empty()).then(|| user.to_string()));
    }
    if options.yes {
        return Ok(None);
    }

    let user = ctx.ui.input(
        "GitHub 사용자 필터 (선택)",
        Some(defaults.gh_user.as_deref().unwrap_or("")),
    )?;
    let user = user.trim();
    Ok((!user.is_empty()).then(|| user.to_string()))
}

fn ordered_agents(default: Option<InitAgent>) -> Vec<InitAgent> {
    ordered_values(&[InitAgent::Codex, InitAgent::Claude], default.as_ref())
}

fn review_policy_choice_item(policy: ReviewCodexBasePolicy) -> PromptItem {
    match policy {
        ReviewCodexBasePolicy::None => PromptItem::with_description("none", "안 씀"),
        ReviewCodexBasePolicy::Advisory => PromptItem::with_description(
            "advisory",
            "코디네이터가 codex review를 보내고 증거를 기록합니다. 미가용은 차단하지 않습니다.",
        ),
        ReviewCodexBasePolicy::Required => PromptItem::with_description(
            "required",
            "wt workflow pass 전 codex base-diff review 증거가 필수입니다.",
        ),
    }
}

fn ordered_issue_providers(default: Option<&InitIssueProvider>) -> Vec<InitIssueProvider> {
    ordered_values(
        &[
            InitIssueProvider::Github,
            InitIssueProvider::Linear,
            InitIssueProvider::None,
        ],
        default,
    )
}

fn ordered_site_providers(default: Option<&InitSiteProvider>) -> Vec<InitSiteProvider> {
    ordered_values(
        &[
            InitSiteProvider::Herd,
            InitSiteProvider::Valet,
            InitSiteProvider::DockerProxy,
            InitSiteProvider::Traefik,
            InitSiteProvider::None,
        ],
        default,
    )
}

fn ordered_values<T: Clone + PartialEq>(values: &[T], default: Option<&T>) -> Vec<T> {
    let mut ordered = Vec::with_capacity(values.len());
    if let Some(default) = default
        && values.contains(default)
    {
        ordered.push(default.clone());
    }
    for value in values {
        if ordered.iter().all(|existing| existing != value) {
            ordered.push(value.clone());
        }
    }
    ordered
}

fn agent_choice_label(agent: &InitAgent) -> String {
    match agent {
        InitAgent::Codex => "Codex",
        InitAgent::Claude => "Claude",
        InitAgent::Gemini => "Gemini",
        InitAgent::None => "코딩 agent 없음",
    }
    .into()
}

fn issue_provider_choice_label(provider: &InitIssueProvider) -> String {
    match provider {
        InitIssueProvider::Github => "GitHub issues",
        InitIssueProvider::Linear => "Linear issues",
        InitIssueProvider::None => "건너뛰기",
    }
    .into()
}

fn site_provider_choice_label(provider: &InitSiteProvider) -> String {
    match provider {
        InitSiteProvider::None => "건너뛰기",
        InitSiteProvider::Herd => "Herd",
        InitSiteProvider::Valet => "Valet",
        InitSiteProvider::DockerProxy => "Docker proxy",
        InitSiteProvider::Traefik => "Traefik",
    }
    .into()
}

fn init_agent_from_cli(agent: &AgentCli) -> InitAgent {
    match agent {
        AgentCli::Codex => InitAgent::Codex,
        AgentCli::Claude => InitAgent::Claude,
        AgentCli::Gemini => InitAgent::Gemini,
        AgentCli::None => InitAgent::None,
    }
}

fn matching_default_agent<'a>(
    defaults: &'a InitDefaults,
    selected: &InitAgent,
) -> Option<&'a AgentConfig> {
    defaults
        .agent
        .as_ref()
        .filter(|agent| agent.cli != AgentCli::None && init_agent_from_cli(&agent.cli) == *selected)
}

fn build_profile(
    agent: &InitAgent,
    args: Vec<String>,
    command: Option<String>,
    defaults: &InitDefaults,
) -> Option<InitProfile> {
    let default_agent = matching_default_agent(defaults, agent);
    let prompt = default_agent
        .map(|agent| agent.prompt.clone())
        .filter(|prompt| !prompt.is_empty())
        .unwrap_or_else(default_agent_prompts);

    (*agent != InitAgent::None).then(|| {
        let mut config = AgentConfig {
            cli: init_agent_cli(agent),
            args,
            command,
            prompt,
            ..AgentConfig::default()
        };
        config.presence.cli = true;
        config.presence.args = !config.args.is_empty();
        config.presence.command = config.command.is_some();
        if let Some(default_agent) = default_agent {
            inherit_runtime_field_presence(&mut config, default_agent);
        }
        InitProfile { agent: config }
    })
}

fn inherit_runtime_field_presence(config: &mut AgentConfig, default_agent: &AgentConfig) {
    if default_agent.presence.ready {
        config.ready = default_agent.ready.clone();
        config.presence.ready = true;
    }
    if default_agent.presence.submit {
        config.submit = default_agent.submit.clone();
        config.presence.submit = true;
    }
    if default_agent.presence.timeout {
        config.timeout = default_agent.timeout;
        config.presence.timeout = true;
    }
    if default_agent.presence.send_after {
        config.send_after = default_agent.send_after;
        config.presence.send_after = true;
    }
}

fn append_profile_selection(s: &mut String, profile: &InitProfile) {
    append_inline_agent_section(s, &profile.agent, true);
    s.push('\n');
}

fn append_inline_agent_section(s: &mut String, agent: &AgentConfig, include_prompt: bool) {
    s.push_str("[profile.agent]\n");
    s.push_str("# 이 프로필로 실행할 coding agent CLI입니다.\n");
    s.push_str(&format!(
        "cli = {}\n",
        toml_quote(agent_cli_name(&agent.cli))
    ));
    if !agent.args.is_empty() {
        s.push_str("# agent를 실행할 때 항상 추가할 CLI args입니다.\n");
        s.push_str(&format!("args = {}\n", toml_array(&agent.args)));
    }
    if let Some(command) = agent.command.as_deref() {
        s.push_str("# cli 이름만으로 부족할 때 사용할 전체 실행 command입니다.\n");
        s.push_str(&format!("command = {}\n", toml_quote(command)));
    }
    let schema_defaults = AgentConfig::default();
    if agent.presence.timeout && agent.timeout != schema_defaults.timeout {
        s.push_str("# agent 준비 신호 대기 최대 초입니다.\n");
        s.push_str(&format!("timeout = {}\n", agent.timeout));
    }
    if agent.presence.send_after && agent.send_after != schema_defaults.send_after {
        s.push_str("# ready marker가 없을 때 prompt 전 대기 초입니다.\n");
        s.push_str(&format!("send_after = {}\n", agent.send_after));
    }
    if include_prompt && !agent.prompt.is_empty() {
        s.push('\n');
        s.push_str("[profile.agent.prompt]\n");
        s.push_str("# wt run 기본 prompt 뒤에 붙는 추가 지침입니다.\n");
        s.push_str("# common은 모든 run, 나머지는 같은 이름의 run 모드에 붙습니다.\n");
        append_agent_prompts(s, &agent.prompt);
    }
}

fn append_agent_prompts(s: &mut String, prompts: &std::collections::HashMap<String, Vec<String>>) {
    let mut entries = prompts.iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| prompt_mode_order(left).cmp(&prompt_mode_order(right)));
    for (mode, prompt_blocks) in entries {
        append_prompt_array(s, mode, prompt_blocks);
    }
}

fn append_prompt_array(s: &mut String, mode: &str, prompt_blocks: &[String]) {
    s.push_str(&format!("{mode} = [\n"));
    for block in prompt_blocks {
        s.push_str(&format!("    {},\n", toml_quote(block)));
    }
    s.push_str("]\n");
}

fn prompt_mode_order(mode: &str) -> (usize, &str) {
    let order = match mode {
        "common" => 0,
        "issue" => 1,
        "branch" => 2,
        "pr" => 3,
        "workflow" => 4,
        _ => 5,
    };
    (order, mode)
}

fn default_agent_prompts() -> std::collections::HashMap<String, Vec<String>> {
    [
        (
            "common",
            "Before editing, identify the intended outcome, the smallest coherent change, and the checks that should prove it.",
        ),
        (
            "issue",
            "Use the linked issue as the contract: extract the user-visible problem, acceptance criteria, constraints, and comments that change scope before coding.",
        ),
        (
            "branch",
            "Use the current branch and local task context as the contract: inspect recent commits and existing diff, then continue only the requested line of work.",
        ),
        (
            "pr",
            "Use review comments, CI failures, and the PR diff as the contract: fix correctness and regressions first, and explain any non-code decisions.",
        ),
    ]
    .into_iter()
    .map(|(mode, prompt)| (mode.to_string(), vec![prompt.to_string()]))
    .collect()
}

fn init_agent_cli(agent: &InitAgent) -> AgentCli {
    match agent {
        InitAgent::Codex => AgentCli::Codex,
        InitAgent::Claude => AgentCli::Claude,
        InitAgent::Gemini => AgentCli::Gemini,
        InitAgent::None => AgentCli::None,
    }
}

fn explicit_agent_requested(options: &InitOptions) -> bool {
    matches!(
        options.agent.as_ref(),
        Some(InitAgent::Codex | InitAgent::Claude | InitAgent::Gemini)
    ) || options.agent_command.is_some()
        || !options.agent_args.is_empty()
}

fn explicit_issue_provider(provider: Option<&InitIssueProvider>) -> Option<InitIssueProvider> {
    match provider {
        Some(InitIssueProvider::Github) => Some(InitIssueProvider::Github),
        Some(InitIssueProvider::Linear) => Some(InitIssueProvider::Linear),
        Some(InitIssueProvider::None) | None => None,
    }
}

fn explicit_site_provider(provider: Option<&InitSiteProvider>) -> Option<InitSiteProvider> {
    match provider {
        Some(InitSiteProvider::Herd) => Some(InitSiteProvider::Herd),
        Some(InitSiteProvider::Valet) => Some(InitSiteProvider::Valet),
        Some(InitSiteProvider::DockerProxy) => Some(InitSiteProvider::DockerProxy),
        Some(InitSiteProvider::Traefik) => Some(InitSiteProvider::Traefik),
        Some(InitSiteProvider::None) | None => None,
    }
}

fn agent_cli_name(agent: &AgentCli) -> &'static str {
    match agent {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::None => "none",
    }
}

fn target_kind_name(kind: InitTargetKind) -> &'static str {
    match kind {
        InitTargetKind::Local => "개인 설정",
        InitTargetKind::Shared => "팀 공유 설정",
    }
}

fn default_workflow_policy() -> WorkflowDefaultPolicy {
    WorkflowDefaultPolicy {
        pull_request: WorkflowDefaultPullRequestMode::None,
        landing: WorkflowDefaultLandingPolicy::Manual,
        review: ReviewDefaultPolicy {
            codex_base: ReviewCodexBasePolicy::None,
        },
    }
}

fn workflow_pull_request_name(mode: WorkflowDefaultPullRequestMode) -> &'static str {
    match mode {
        WorkflowDefaultPullRequestMode::None => "none",
        WorkflowDefaultPullRequestMode::Draft => "draft",
        WorkflowDefaultPullRequestMode::Ready => "ready",
    }
}

fn workflow_landing_name(policy: WorkflowDefaultLandingPolicy) -> &'static str {
    match policy {
        WorkflowDefaultLandingPolicy::Manual => "manual",
        WorkflowDefaultLandingPolicy::Auto => "auto",
    }
}

fn review_codex_base_name(policy: ReviewCodexBasePolicy) -> &'static str {
    match policy {
        ReviewCodexBasePolicy::None => "none",
        ReviewCodexBasePolicy::Advisory => "advisory",
        ReviewCodexBasePolicy::Required => "required",
    }
}

fn issue_provider_name(provider: &InitIssueProvider) -> &'static str {
    match provider {
        InitIssueProvider::Github => "github",
        InitIssueProvider::Linear => "linear",
        InitIssueProvider::None => "none",
    }
}

fn site_provider_name(provider: &InitSiteProvider) -> &'static str {
    match provider {
        InitSiteProvider::None => "none",
        InitSiteProvider::Herd => "herd",
        InitSiteProvider::Valet => "valet",
        InitSiteProvider::DockerProxy => "docker_proxy",
        InitSiteProvider::Traefik => "traefik",
    }
}

fn workspace_browser_mode_name(mode: InitWorkspaceBrowserMode) -> &'static str {
    match mode {
        InitWorkspaceBrowserMode::System => "system",
        InitWorkspaceBrowserMode::ChromeDevtools => "chrome_devtools",
    }
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_array(values: &[String]) -> String {
    let rendered = values
        .iter()
        .map(|value| toml_quote(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentCli, IssueProviderType, OriginPolicy, SiteProvider, WorkspaceBrowserMode,
    };
    use crate::context::mock::{MockRunner, MockUi};
    use std::sync::Arc;

    fn local_target(dir: &tempfile::TempDir) -> InitTarget {
        InitTarget {
            path: dir.path().join(".wt/config/local.toml"),
            kind: InitTargetKind::Local,
        }
    }

    fn ctx_for_dir(dir: &tempfile::TempDir) -> Ctx {
        ctx_for_dir_with_commands(dir, &[])
    }

    fn ctx_for_dir_with_commands(dir: &tempfile::TempDir, commands: &[&str]) -> Ctx {
        let mut runner = MockRunner::new();
        for command in commands {
            runner.add_command(command);
        }
        Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(MockUi::new()),
        )
    }

    #[test]
    fn init_rejects_legacy_flat_roots_before_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_tasks = dir.path().join(".wt/tasks");
        std::fs::create_dir_all(&legacy_tasks).unwrap();
        std::fs::write(legacy_tasks.join("old.toml"), "").unwrap();
        let ctx = ctx_for_dir(&dir);

        let error = run(
            &ctx,
            InitOptions {
                yes: true,
                ..InitOptions::default()
            },
        )
        .unwrap_err();
        let report = format!("{error:#}");

        assert!(report.contains("Found legacy wt personal TaskDocument storage"));
        assert!(report.contains(".wt/tasks"));
        assert!(report.contains(".wt/execution/tasks"));
        assert!(!dir.path().join(".wt/execution").exists());
        assert!(!dir.path().join(".wt/config/local.toml").exists());
    }

    #[test]
    fn init_rejects_legacy_runtime_observation_root_before_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_agent_state = dir.path().join(".wt/agent.state");
        std::fs::create_dir_all(&legacy_agent_state).unwrap();
        std::fs::write(legacy_agent_state.join("wait-observations.jsonl"), "").unwrap();
        let ctx = ctx_for_dir(&dir);

        let error = run(
            &ctx,
            InitOptions {
                yes: true,
                ..InitOptions::default()
            },
        )
        .unwrap_err();
        let report = format!("{error:#}");

        assert!(report.contains("Found legacy wt personal runtime observation storage"));
        assert!(report.contains(".wt/agent.state"));
        assert!(report.contains(".wt/runtime/agents"));
        assert!(!dir.path().join(".wt/runtime/agents").exists());
        assert!(!dir.path().join(".wt/config/local.toml").exists());
    }

    #[test]
    fn init_rejects_legacy_session_anchor_root_before_bootstrap() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_sessions = dir.path().join(".wt/sessions");
        std::fs::create_dir_all(&legacy_sessions).unwrap();
        std::fs::write(legacy_sessions.join("surface%3Aold.toml"), "").unwrap();
        let ctx = ctx_for_dir(&dir);

        let error = run(
            &ctx,
            InitOptions {
                yes: true,
                ..InitOptions::default()
            },
        )
        .unwrap_err();
        let report = format!("{error:#}");

        assert!(report.contains("Found legacy wt personal session anchor storage"));
        assert!(report.contains(".wt/sessions"));
        assert!(report.contains(".wt/runtime/agents"));
        assert!(!dir.path().join(".wt/runtime/agents").exists());
        assert!(!dir.path().join(".wt/config/local.toml").exists());
    }

    #[test]
    fn init_allows_local_directory_without_legacy_wt_state() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".local/cache")).unwrap();
        std::fs::write(dir.path().join(".local/README"), "project-local files\n").unwrap();
        let ctx = ctx_for_dir(&dir);

        run(
            &ctx,
            InitOptions {
                yes: true,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let config = Config::load_file(&dir.path().join(".wt/config/local.toml")).unwrap();
        assert_eq!(config.worktree.link, vec![".local"]);
        assert!(dir.path().join(".wt/execution").is_dir());
    }

    #[test]
    fn init_bootstraps_repo_personal_storage_and_git_exclude_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        run(
            &ctx,
            InitOptions {
                yes: true,
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert!(dir.path().join(".wt").is_dir());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/.wt\n"
        );

        run(
            &ctx,
            InitOptions {
                yes: true,
                force: true,
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/.wt\n"
        );
    }

    #[test]
    fn init_dry_run_does_not_bootstrap_repo_personal_storage() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        run(
            &ctx,
            InitOptions {
                yes: true,
                dry_run: true,
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert!(!dir.path().join(".wt").exists());
        assert!(!dir.path().join(".git/info/exclude").exists());
    }

    #[test]
    #[cfg(unix)]
    fn init_accepts_existing_personal_storage_symlink_to_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external-wt-state");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(".wt")).unwrap();
        let ctx = ctx_for_dir(&dir);

        run(
            &ctx,
            InitOptions {
                yes: true,
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert_eq!(std::fs::read_link(dir.path().join(".wt")).unwrap(), target);
        assert!(dir.path().join(".wt/config/local.toml").is_file());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/.wt\n"
        );
    }

    #[test]
    fn init_recommendation_plan_records_target_mode_and_sections() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert_eq!(plan.target_path, dir.path().join(".wt/config/local.toml"));
        assert_eq!(plan.target_kind, InitTargetKind::Local);
        assert!(!plan.target_exists);
        assert_eq!(
            plan.sections,
            vec![
                InitSection::Workflow,
                InitSection::Worktree,
                InitSection::Workspace
            ]
        );
        assert!(plan.detected_signals.is_empty());
        assert!(plan.content.contains("[workflow]"));
        assert!(!plan.content.contains("[review]"));
        assert!(plan.content.contains("pull_request = \"none\""));
        assert!(plan.content.contains("landing = \"manual\""));
        assert!(plan.content.contains(
            "colors = { task = \"blue\", issue = \"blue\", branch = \"green\", pr = \"magenta\" }"
        ));
        let workspace = config.workspace.unwrap();
        assert_eq!(
            workspace.colors.get("task").map(String::as_str),
            Some("blue")
        );
        assert_eq!(
            workspace.colors.get("issue").map(String::as_str),
            Some("blue")
        );
        assert_eq!(
            workspace.colors.get("branch").map(String::as_str),
            Some("green")
        );
        assert_eq!(
            workspace.colors.get("pr").map(String::as_str),
            Some("magenta")
        );
        assert!(!plan.content.contains("[profile.agent]"));
        assert!(!plan.content.contains("[issues]"));
        assert!(!plan.content.contains("[site]"));
    }

    #[test]
    fn init_recommendation_plan_summary_shows_selected_sections_and_no_signals() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let summary = render_plan_summary(&plan).join("\n");

        assert!(summary.contains("작업: 새 설정 생성"));
        assert!(summary.contains("저장될 설정: workflow, worktree, workspace"));
        assert!(!summary.contains("감지된 신호"));
        assert!(!summary.contains("감지된 명령"));
    }

    #[test]
    fn init_explicit_agent_plan_writes_agent_section() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                agent: Some(InitAgent::Codex),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(
            plan.sections,
            vec![
                InitSection::Workflow,
                InitSection::ProfileAgent,
                InitSection::ProfileAgentPrompt,
                InitSection::Worktree,
                InitSection::Workspace
            ]
        );
        assert!(plan.content.contains("[profile.agent]"));
        assert!(plan.content.contains("[profile.agent.prompt]"));
        assert!(plan.content.contains("cli = \"codex\""));
        assert!(plan.content.contains("common = ["));
        assert!(plan.content.contains("issue = ["));
        assert!(plan.content.contains("branch = ["));
        assert!(plan.content.contains("pr = ["));
        assert!(
            plan.content
                .contains("# 이 프로필로 실행할 coding agent CLI입니다.")
        );
        assert!(
            plan.content
                .contains("# wt run 기본 prompt 뒤에 붙는 추가 지침입니다.")
        );
        assert!(
            plan.content
                .contains("# common은 모든 run, 나머지는 같은 이름의 run 모드에 붙습니다.")
        );
        assert!(
            !plan
                .content
                .contains("# 모든 run 모드에 공통으로 추가됩니다.")
        );
        assert!(
            !plan
                .content
                .contains("# provider issue 기반 작업에 추가됩니다.")
        );
        assert!(
            !plan
                .content
                .contains("# branch/task 기반 작업에 추가됩니다.")
        );
        assert!(!plan.content.contains("# PR review/fix 작업에 추가됩니다."));
        assert!(!plan.content.contains("timeout ="));
        assert!(!plan.content.contains("send_after ="));
        assert!(!plan.content.contains("Read AGENTS.md"));
        assert!(!plan.content.contains("[issues]"));
    }

    #[test]
    fn init_explicit_issue_provider_plan_writes_issue_section() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                issue_provider: Some(InitIssueProvider::Github),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(
            plan.sections,
            vec![
                InitSection::Issues,
                InitSection::Workflow,
                InitSection::Worktree,
                InitSection::WorktreeNaming,
                InitSection::Workspace
            ]
        );
        assert!(plan.content.contains("[issues]"));
        assert!(plan.content.contains("provider = \"github\""));
        assert!(plan.content.contains("[worktree.naming]"));
        assert!(!plan.content.contains("[profile.agent]"));
    }

    #[test]
    fn init_linear_writes_provider_preferred_origin_policy() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Linear),
                site_provider: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let config = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        assert!(config.contains("[issues]"));
        assert!(config.contains("provider = \"linear\""));
        assert!(config.contains("origin_policy = \"provider-preferred\""));
    }

    #[test]
    fn init_rerun_preserves_existing_origin_policy() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(".wt/config");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(
            local.join("local.toml"),
            r#"[issues]
provider = "linear"
origin_policy = "local-only"
"#,
        )
        .unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // keep Linear issue provider
        ui.add_select(0); // keep existing common config defaults
        ui.add_select(0); // use system editor
        ui.add_confirm(true); // overwrite config
        ui.add_confirm(false); // do not add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                force: true,
                agent: Some(InitAgent::None),
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(local.join("local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(
            config.issues.unwrap().origin_policy,
            OriginPolicy::LocalOnly
        );
        assert!(content.contains("origin_policy = \"local-only\""));
    }

    #[test]
    fn init_issue_plan_summary_shows_provider_and_agent_prompt_readiness() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                issue_provider: Some(InitIssueProvider::Github),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let summary = render_plan_summary(&plan).join("\n");

        assert!(
            summary.contains("저장될 설정: issues, workflow, worktree, worktree.naming, workspace")
        );
        assert!(summary.contains("경고\n  - gh CLI가 없습니다"));
        assert!(
            summary.contains(
                "안내\n  - issue agent prompt: 선택된 agent runtime이 없습니다. issue 작업에서 agent를 바로 실행하려면 --agent <name>"
            )
        );
        assert!(!plan.content.contains("[profile.agent]"));
    }

    #[test]
    fn init_issue_with_explicit_agent_marks_agent_prompt_ready() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                agent: Some(InitAgent::Codex),
                issue_provider: Some(InitIssueProvider::Github),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let summary = render_plan_summary(&plan).join("\n");

        assert!(plan.content.contains("[issues]"));
        assert!(plan.content.contains("[profile.agent]"));
        assert!(plan.content.contains("[profile.agent.prompt]"));
        assert!(summary.contains("안내\n  - issue agent prompt: codex로 실행 준비됨"));
    }

    #[test]
    fn init_recommendation_plan_writes_detected_project_sections() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert_eq!(
            plan.sections,
            vec![
                InitSection::Workflow,
                InitSection::Worktree,
                InitSection::Setup,
                InitSection::Workspace
            ]
        );
        assert!(
            plan.detected_signals
                .contains(&"setup: npm install".to_string())
        );
        assert!(
            plan.detected_signals
                .contains(&"post-deps tab: npm run dev".to_string())
        );
        assert!(plan.content.contains("[setup]"));
        assert!(plan.content.contains("run = \"npm install\""));
        assert!(plan.content.contains("post_deps_tabs = [\"npm run dev\"]"));
    }

    #[test]
    fn init_recommendation_without_cmux_omits_workspace_automation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","test":"vitest"}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();
        let workspace = config.workspace.unwrap();

        assert!(
            plan.detected_signals
                .contains(&"post-deps tab: npm run dev".to_string())
        );
        assert!(workspace.tabs.is_empty());
        assert!(workspace.post_deps_tabs.is_empty());
        assert!(!plan.content.contains("post_deps_tabs"));
    }

    #[test]
    fn init_recommendation_uses_only_installed_cmux_workspace_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux", "lazygit"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert_eq!(config.workspace.unwrap().tabs, vec!["lazygit".to_string()]);
        assert!(plan.content.contains("tabs = [\"lazygit\"]"));
    }

    #[test]
    fn init_recommendation_writes_local_project_specific_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "APP_KEY=secret\n").unwrap();
        std::fs::write(dir.path().join(".linear.toml"), "[workspace]\n").unwrap();
        std::fs::create_dir(dir.path().join(".local")).unwrap();
        std::fs::write(dir.path().join("CLAUDE.local.md"), "local notes\n").unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"require":{"laravel/framework":"^13.0"}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert!(plan.content.contains("[worktree]\n"));
        assert_eq!(config.worktree.copy, vec![".env"]);
        assert!(config.worktree.copy_as.is_empty());
        assert_eq!(
            config.worktree.inject_local_context.as_deref(),
            Some(DEFAULT_INJECT_LOCAL_CONTEXT)
        );
        assert!(plan.content.contains("inject_local_context = \"\"\""));
        assert!(plan.content.contains("- site: {{site_url}}"));
        assert!(plan.content.contains("- worktree: {{worktree_path}}"));
        assert!(plan.content.contains("- parent: {{parent_branch}}"));
        assert_eq!(
            config.worktree.link,
            vec![".local", ".linear.toml", "CLAUDE.local.md"]
        );
        let naming = config.worktree.naming.unwrap();
        assert_eq!(naming.command, "claude -p");
        assert_eq!(
            naming.branch.as_deref(),
            Some("{{branch_prefix}}{{issue_key_lower}}-{{english_slug}}")
        );
        assert_eq!(config.issues.unwrap().provider, IssueProviderType::Linear);
        assert_eq!(config.site.unwrap().provider, SiteProvider::Herd);
        let workspace = config.workspace.unwrap();
        let browser = workspace.browser.unwrap();
        assert_eq!(browser.mode, WorkspaceBrowserMode::ChromeDevtools);
        assert_eq!(
            browser.chrome_devtools.unwrap().user_data_dir.as_deref(),
            Some("{{worktree_parent}}/.chrome-devtools/{{worktree_name}}")
        );
        assert!(plan.content.contains("[workspace.browser]"));
        assert!(plan.content.contains("[workspace.browser.chrome_devtools]"));
        assert!(plan.content.contains("mode = \"chrome_devtools\""));
        assert!(!plan.content.contains("php artisan storage:link"));
        assert!(!plan.content.contains("setup-env"));
    }

    #[test]
    fn init_shared_recommendation_omits_private_local_helpers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "APP_KEY=secret\n").unwrap();
        std::fs::write(dir.path().join(".linear.toml"), "[workspace]\n").unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"require":{"laravel/framework":"^13.0"}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux", "lazygit", "nvim"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            InitTarget {
                path: dir.path().join(".wt.toml"),
                kind: InitTargetKind::Shared,
            },
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert!(config.worktree.copy.is_empty());
        assert!(config.worktree.link.is_empty());
        assert!(config.worktree.naming.is_none());
        let workspace = config.workspace.unwrap();
        assert!(workspace.tabs.is_empty());
        assert!(workspace.browser.is_none());
        assert!(!plan.content.contains("lazygit"));
        assert!(!plan.content.contains("nvim"));
        assert_eq!(config.issues.unwrap().provider, IssueProviderType::Linear);
        assert_eq!(config.site.unwrap().provider, SiteProvider::Herd);
    }

    #[test]
    fn init_recommendation_for_rust_only_repo_skips_setup_commands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert_eq!(
            plan.sections,
            vec![
                InitSection::Workflow,
                InitSection::Worktree,
                InitSection::Workspace
            ]
        );
        assert!(config.setup.deps.is_empty());
    }

    #[test]
    fn init_recommendation_detects_node_setup_and_dev_tabs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"dev":"vite","test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("references/demo")).unwrap();
        std::fs::write(
            dir.path().join("references/demo/package.json"),
            r#"{"scripts":{"dev":"vite","lint":"eslint ."}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert!(
            plan.detected_signals
                .contains(&"setup: pnpm install".to_string())
        );
        assert!(
            plan.detected_signals
                .contains(&"post-deps tab: pnpm run dev".to_string())
        );
        assert!(
            !plan
                .detected_signals
                .iter()
                .any(|signal| signal.contains("references/demo"))
        );
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].run, "pnpm install");
        let workspace = config.workspace.unwrap();
        assert_eq!(workspace.post_deps_tabs, vec!["pnpm run dev".to_string()]);
    }

    #[test]
    fn init_recommendation_prefers_npm_ci_with_package_lock() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{}}"#).unwrap();
        std::fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert!(plan.detected_signals.contains(&"setup: npm ci".to_string()));
        assert_eq!(config.setup.deps[0].run, "npm ci");
    }

    #[test]
    fn init_recommendation_detects_uv_subproject_setup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/pyproject.toml"),
            "[project]\nname = \"api\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("api/uv.lock"), "").unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert!(
            plan.detected_signals
                .contains(&"setup: api: uv sync".to_string())
        );
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].working_dir.as_deref(), Some("api"));
        assert_eq!(config.setup.deps[0].run, "uv sync");
    }

    #[test]
    fn init_recommendation_detects_polyglot_repo_without_optional_guards() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/pyproject.toml"),
            "[project]\nname = \"api\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("api/uv.lock"), "").unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::write(
            dir.path().join("apps/web/package.json"),
            r#"{"packageManager":"bun@1.0.0","scripts":{"dev":"vite","test":"vitest"}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert!(
            plan.detected_signals
                .contains(&"setup: composer install --no-interaction --no-progress".to_string())
        );
        assert!(
            plan.detected_signals
                .contains(&"setup: api: uv sync".to_string())
        );
        assert!(
            plan.detected_signals
                .contains(&"setup: apps/web: bun install".to_string())
        );
        assert_eq!(config.setup.deps.len(), 3);
        assert!(
            config
                .setup
                .deps
                .iter()
                .all(|command| command.if_exists.is_none())
        );
        assert!(config.setup.deps.iter().any(|command| {
            command.working_dir.is_none()
                && command.run == "composer install --no-interaction --no-progress"
        }));
        assert!(config.setup.deps.iter().any(|command| {
            command.working_dir.as_deref() == Some("api") && command.run == "uv sync"
        }));
        assert!(config.setup.deps.iter().any(|command| {
            command.working_dir.as_deref() == Some("apps/web") && command.run == "bun install"
        }));
        let workspace = config.workspace.unwrap();
        assert_eq!(
            workspace.post_deps_tabs,
            vec!["cd apps/web && bun run dev".to_string()]
        );
    }

    #[test]
    fn init_recommendation_activates_detected_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir_with_commands(&dir, &["cmux"]);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();

        assert_eq!(
            plan.sections,
            vec![
                InitSection::Workflow,
                InitSection::Worktree,
                InitSection::Setup,
                InitSection::Workspace
            ]
        );
        assert!(
            plan.detected_signals
                .contains(&"setup: npm install".to_string())
        );
        assert!(
            plan.detected_signals
                .contains(&"post-deps tab: npm run dev".to_string())
        );
        assert_eq!(config.setup.deps.len(), 1);
        let workspace = config.workspace.unwrap();
        assert_eq!(workspace.post_deps_tabs, vec!["npm run dev".to_string()]);
        assert!(plan.content.lines().any(|line| line == "[setup]"));
        assert!(
            plan.content
                .lines()
                .any(|line| line == "post_deps_tabs = [\"npm run dev\"]")
        );
    }

    #[test]
    fn init_explicit_docker_proxy_site_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                site_provider: Some(InitSiteProvider::DockerProxy),
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();

        assert!(plan.content.contains("[site]"));
        assert!(plan.content.contains("provider = \"docker_proxy\""));
        assert!(plan.content.contains("name = \"{{repo}}-{{branch_slug}}\""));
        assert!(!plan.content.contains("provider = \"docker-proxy\""));
    }

    #[test]
    fn init_interactive_app_defaults_use_detection_without_generic_prompts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                agent: Some(InitAgent::None),
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                yes: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            *ui.prompts.lock().unwrap(),
            vec![
                "select: 개발 환경 설정을 어떻게 만들까요?".to_string(),
                "select: 설정 editor command".to_string(),
                "confirm: 설정을 생성할까요?".to_string(),
                "confirm: Claude가 .wt/**에 Edit/Write할 수 있도록 .claude/settings.local.json에 허용 규칙을 추가할까요?".to_string(),
            ]
        );

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].run, "npm install");
        let workspace = config.workspace.unwrap();
        assert!(workspace.tabs.is_empty());
        assert!(workspace.post_deps_tabs.is_empty());
    }

    #[test]
    fn init_interactive_detected_integrations_use_select_prompts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".linear.toml"), "[workspace]\n").unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"require":{"laravel/framework":"^11.0"}}"#,
        )
        .unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use detected Linear issue workflow
        ui.add_select(0); // use detected Herd local site
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_select(0); // use Chrome DevTools browser
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);
        let mut runner = MockRunner::new();
        runner.add_command("cmux");
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                agent: Some(InitAgent::None),
                yes: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let prompts = ui.prompts.lock().unwrap().clone();
        assert!(prompts.contains(&"select: issue 도구".to_string()));
        assert!(prompts.contains(&"select: local site 설정".to_string()));
        assert!(
            !prompts
                .iter()
                .any(|prompt| prompt == "confirm: Linear issues를 설정할까요?")
        );
        assert!(
            !prompts
                .iter()
                .any(|prompt| prompt == "confirm: Herd local site를 설정할까요?")
        );

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.issues.unwrap().provider, IssueProviderType::Linear);
        assert_eq!(config.site.unwrap().provider, SiteProvider::Herd);
    }

    #[test]
    fn init_app_plan_summary_keeps_preview_to_saved_settings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let summary = render_plan_summary(&plan).join("\n");

        assert!(summary.contains("저장될 설정: workflow, worktree, setup, workspace"));
        assert!(!summary.contains("[ok] 감지됨"));
        assert!(!summary.contains("감지된 신호"));
    }

    #[test]
    fn init_shared_plan_explains_omitted_private_helpers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "APP_KEY=test").unwrap();
        std::fs::write(dir.path().join(".local"), "local").unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                issue_provider: Some(InitIssueProvider::Linear),
                yes: true,
                ..InitOptions::default()
            },
            InitTarget {
                path: dir.path().join(".wt.toml"),
                kind: InitTargetKind::Shared,
            },
        )
        .unwrap();
        let summary = render_plan_summary(&plan).join("\n");

        assert!(!summary.contains("감지된 신호"));
        assert!(summary.contains(
            "안내\n  - 팀 공유 설정에는 개인 helper를 쓰지 않습니다: .env copy, local links (.local), worktree.naming"
        ));
        assert!(!plan.content.contains("[worktree]"));
        assert!(!plan.content.contains("[worktree.naming]"));
    }

    #[test]
    fn init_local_codex_yes_creates_parseable_config() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(!content.contains("# [worktree.naming]"));
        assert!(!content.contains("# [setup.env]"));
        assert!(!content.contains("# [editor]"));
        assert!(!content.contains("# post_deps_tabs"));
        assert!(!content.contains("# colors ="));
        assert!(config.worktree.path.is_none());
        assert!(config.worktree.naming.is_none());
        let profile = config.profile.unwrap();
        let agent = profile.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(config.agent.is_none());
        assert!(!content.contains("[agent]"));
        assert!(agent.args.is_empty());
        assert!(!content.contains("args ="));
        assert!(!content.contains("timeout ="));
        assert!(!content.contains("send_after ="));
        assert!(content.contains("# workflow task의 PR 처리: none | draft | ready"));
        assert!(content.contains("# review 통과 뒤 처리: manual은 대기, auto는 landing 진행"));
        assert!(content.contains("# cmux로 함께 열 보조 탭입니다. 없거나 불필요하면 []로 둡니다."));
        assert!(content.contains("# task/issue/branch/pr workspace 색상입니다."));
        assert!(
            !dir.path()
                .join(".wt/config/profiles/codex/profile.toml")
                .exists()
        );
        assert!(!content.contains("현재 GitHub 이슈"));
    }

    #[test]
    fn init_shared_with_github_options_creates_only_shared_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: true,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: Some(InitSiteProvider::None),
                gh_user: Some("alice".into()),
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(config.agent.is_none());
        assert!(config.profile.is_none());
        assert!(!content.contains("현재 이슈/작업 컨텍스트"));
        assert!(!content.contains("현재 GitHub 이슈"));
        let issues = config.issues.unwrap();
        assert_eq!(issues.provider, IssueProviderType::Github);
        assert_eq!(issues.gh_user.as_deref(), Some("alice"));
        assert!(!content.contains("[worktree.naming]"));

        assert!(!dir.path().join(".wt/config/local.toml").exists());
        assert!(
            !dir.path()
                .join(".wt/config/profiles/gemini/profile.toml")
                .exists()
        );
    }

    #[test]
    fn init_github_without_user_omits_gh_user() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let issues = config.issues.unwrap();
        assert_eq!(issues.provider, IssueProviderType::Github);
        assert!(issues.gh_user.is_none());
        assert!(!content.contains("alice"));
    }

    #[test]
    fn init_with_site_provider_writes_site_section() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_select(0); // use Chrome DevTools browser
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::Valet),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let site = config.site.unwrap();
        assert_eq!(site.provider, SiteProvider::Valet);
        assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
        assert_eq!(site.secure, Some(true));
        assert!(!content.contains("[herd]"));
    }

    #[test]
    fn init_with_herd_site_provider_omits_default_secure() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_for_dir(&dir);

        let plan = build_plan(
            &ctx,
            &InitOptions {
                yes: true,
                site_provider: Some(InitSiteProvider::Herd),
                ..InitOptions::default()
            },
            local_target(&dir),
        )
        .unwrap();
        let config: Config = toml::from_str(&plan.content).unwrap();
        let site = config.site.unwrap();

        assert_eq!(site.provider, SiteProvider::Herd);
        assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}"));
        assert!(site.secure.is_none());
        assert!(site.effective_secure());
        assert!(!plan.content.contains("secure = true"));
    }

    #[test]
    fn init_with_traefik_site_provider_writes_traefik_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_select(0); // use Chrome DevTools browser
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::Traefik),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let site = config.site.unwrap();
        assert_eq!(site.provider, SiteProvider::Traefik);
        assert_eq!(site.name.as_deref(), Some("{{repo}}-{{branch_slug}}.l"));
        assert_eq!(site.url.as_deref(), Some("https://{{site_name}}"));
        assert_eq!(
            site.target.as_deref(),
            Some("http://127.0.0.1:{{vite_port}}")
        );
        assert_eq!(site.secure, Some(true));
    }

    #[test]
    fn init_interactive_flow_uses_ui_answers() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(1); // shared .wt.toml
        ui.add_select(0); // use project recommendation
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert!(config.agent.is_none());
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(!dir.path().join(".wt/config/local.toml").exists());
        assert!(config.issues.is_none());
    }

    #[test]
    fn init_interactive_flow_renders_wizard_steps_and_prompt_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // private repo config
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let steps = ui.steps.lock().unwrap().clone();
        assert_eq!(
            &steps[..6],
            &[
                "wt init".to_string(),
                "단계 1/5: 설정 파일 위치".to_string(),
                "단계 2/5: 외부 도구 연결".to_string(),
                "단계 3/5: 개발 환경 설정".to_string(),
                "단계 4/5: 미리보기".to_string(),
                "단계 5/5: 쓰기 확인".to_string(),
            ]
        );
        assert!(steps[6].starts_with("설정 생성됨:"));

        let dims = ui.dims.lock().unwrap().clone();
        assert!(
            dims.iter()
                .any(|line| line.contains("이 저장소에 맞는 git worktree 프로젝트 설정"))
        );
        assert!(dims.iter().any(|line| {
            line.contains("  - 개인 설정 파일: <repo-root>/.wt/config/local.toml")
        }));
        assert!(dims.iter().any(|line| line.is_empty()));
        assert!(
            dims.iter().any(|line| {
                line.contains("issue 도구나 local site 설정을 찾지 못해")
            })
        );
        assert!(
            dims.iter().any(|line| {
                line.contains("생성되는 TOML에는 선택한 active 설정만")
            })
        );
        assert!(
            dims.iter()
                .any(|line| line.contains("어디에 무엇을 저장할지"))
        );
        assert!(
            dims.iter()
                .any(|line| line.contains("지금 설정 파일에 쓸지 확인"))
        );
        assert!(!dims.iter().any(|line| line.contains("감지된 명령: 없음")));

        assert_eq!(
            *ui.prompts.lock().unwrap(),
            vec![
                "select: 저장 위치".to_string(),
                "select: 개발 환경 설정을 어떻게 만들까요?".to_string(),
                "select: 설정 editor command".to_string(),
                "confirm: agent 실행 args를 추가할까요?".to_string(),
                "confirm: 설정을 생성할까요?".to_string(),
                "confirm: Claude가 .wt/**에 Edit/Write할 수 있도록 .claude/settings.local.json에 허용 규칙을 추가할까요?".to_string(),
            ]
        );
    }

    #[test]
    fn init_interactive_flow_accepts_manual_agent_args() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .wt/config/local.toml
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(true); // enter agent args
        ui.add_input("--model gpt-5.5");
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(agent.args, vec!["--model", "gpt-5.5"]);
        let prompts = ui.prompts.lock().unwrap().clone();
        assert!(prompts.contains(&"confirm: agent 실행 args를 추가할까요?".to_string()));
        assert!(prompts.contains(&"input: agent args 직접 입력".to_string()));
    }

    #[test]
    fn init_interactive_flow_prompts_for_agent_and_writes_prompt_scope() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .wt/config/local.toml
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_select(0); // Codex agent
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let prompts = ui.prompts.lock().unwrap().clone();
        assert!(prompts.contains(&"select: 코딩 agent".to_string()));
        let select_items = ui.select_items.lock().unwrap().clone();
        assert_eq!(select_items[3], vec!["Codex", "Claude"]);

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(content.contains("[profile.agent.prompt]"));
        assert!(content.contains("# wt run 기본 prompt 뒤에 붙는 추가 지침입니다."));
        assert!(content.contains("# common은 모든 run, 나머지는 같은 이름의 run 모드에 붙습니다."));
        assert!(!content.contains("# 모든 run 모드에 공통으로 추가됩니다."));
        assert!(!content.contains("# provider issue 기반 작업에 추가됩니다."));
        assert!(!content.contains("현재 branch 이름, task context"));
        assert!(agent.prompt.contains_key("common"));
        assert!(agent.prompt.contains_key("issue"));
        assert!(agent.prompt.contains_key("branch"));
        assert!(agent.prompt.contains_key("pr"));
    }

    #[test]
    fn init_interactive_custom_common_config_writes_selected_settings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();

        let mut ui = MockUi::new();
        ui.add_select(1); // customize frequently used settings
        ui.add_select(1); // home worktrees folder
        ui.add_input("lazygit, nvim, pnpm run dev");
        ui.add_confirm(true); // add detected setup commands
        ui.add_select(0); // detected pnpm install
        ui.add_confirm(true); // start detected dev command after deps
        ui.add_select(1); // nvim {{path}}
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(
            config.worktree.path.as_deref(),
            Some("$HOME/worktrees/{{default_name}}")
        );
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].run, "pnpm install");
        assert!(config.setup.deps[0].working_dir.is_none());
        assert!(config.setup.deps[0].if_exists.is_none());
        assert_eq!(config.editor.command.as_deref(), Some("vim {{path}}"));

        let workspace = config.workspace.unwrap();
        assert_eq!(
            workspace.tabs,
            vec![
                "lazygit".to_string(),
                "nvim".to_string(),
                "pnpm run dev".to_string()
            ]
        );
        assert_eq!(workspace.post_deps_tabs, vec!["pnpm run dev".to_string()]);

        let dims = ui.dims.lock().unwrap().clone();
        assert!(dims.iter().any(|line| line
            == "dev 탭은 dependency setup이 끝난 뒤 개발 서버 command를 별도 탭에서 시작합니다."));
        assert!(
            dims.iter()
                .any(|line| line == "감지한 dev server command: pnpm run dev")
        );
        assert!(
            ui.prompts
                .lock()
                .unwrap()
                .contains(&"confirm: setup 후 pnpm run dev를 dev 탭에서 시작할까요?".to_string())
        );
    }

    #[test]
    fn init_interactive_setup_deps_supports_root_polyglot_and_subdir_override() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"scripts":{}}"#).unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::write(
            dir.path().join("apps/web/package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_select(1); // customize frequently used settings
        ui.add_select(0); // next to current repository
        ui.add_input("lazygit, nvim");
        ui.add_confirm(true); // root npm install
        ui.add_select(0); // detected npm install
        ui.add_confirm(true); // root composer install --no-interaction --no-progress
        ui.add_confirm(true); // apps/web pnpm install
        ui.add_select(4); // nvm use && npm install
        ui.add_confirm(false); // do not start detected dev command
        ui.add_select(0); // default editor
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.setup.deps.len(), 3);
        assert!(
            config
                .setup
                .deps
                .iter()
                .any(|command| { command.working_dir.is_none() && command.run == "npm install" })
        );
        assert!(config.setup.deps.iter().any(|command| {
            command.working_dir.is_none()
                && command.run == "composer install --no-interaction --no-progress"
        }));
        assert!(config.setup.deps.iter().any(|command| {
            command.working_dir.as_deref() == Some("apps/web")
                && command.run
                    == "bash -lc 'source \"$HOME/.nvm/nvm.sh\" && nvm use && npm install'"
        }));
        assert!(content.contains("working_dir = \"apps/web\""));
    }

    #[test]
    fn init_interactive_setup_deps_detects_uv_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/pyproject.toml"),
            "[project]\nname = \"api\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("api/uv.lock"), "").unwrap();

        let mut ui = MockUi::new();
        ui.add_select(1); // customize frequently used settings
        ui.add_select(0); // next to current repository
        ui.add_input("lazygit, nvim");
        ui.add_confirm(true); // api uv sync
        ui.add_select(0); // default editor
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.setup.deps.len(), 1);
        assert_eq!(config.setup.deps[0].working_dir.as_deref(), Some("api"));
        assert_eq!(config.setup.deps[0].run, "uv sync");
        assert!(config.setup.deps[0].if_exists.is_none());
    }

    #[test]
    fn init_interactive_agent_args_without_default_uses_confirm_flow() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .wt/config/local.toml
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let prompts = ui.prompts.lock().unwrap().clone();
        assert!(prompts.contains(&"confirm: agent 실행 args를 추가할까요?".to_string()));
        assert!(!prompts.contains(&"select: agent 실행 args".to_string()));
    }

    #[test]
    fn init_interactive_codex_none_agent_args_omits_args() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .wt/config/local.toml
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(false); // no agent args
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert!(agent.args.is_empty());
        assert!(!content.contains("args ="));
    }

    #[test]
    fn init_agent_command_overrides_selected_cli() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: Some("sandvault run -- codex --yolo".into()),
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt/config/local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(
            agent.command.as_deref(),
            Some("sandvault run -- codex --yolo")
        );
    }

    #[test]
    fn init_none_agent_rejects_command_override() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: Vec::new(),
                agent_command: Some("codex".into()),
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--agent-command and --agent-arg cannot be used when --agent none")
        );
    }

    #[test]
    fn init_shared_accepts_agent_runtime_in_selected_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        run(
            &ctx,
            InitOptions {
                local: false,
                shared: true,
                agent: Some(InitAgent::Codex),
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(dir.path().join(".wt.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        assert_eq!(config.profile.unwrap().agent.unwrap().cli, AgentCli::Codex);
        assert!(!dir.path().join(".wt/config/local.toml").exists());
    }

    #[test]
    fn init_none_agent_rejects_agent_args() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = run(
            &ctx,
            InitOptions {
                local: true,
                shared: false,
                agent: Some(InitAgent::None),
                agent_args: vec!["--model".into()],
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: true,
                force: false,
                ..InitOptions::default()
            },
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--agent-command and --agent-arg cannot be used when --agent none")
        );
    }

    #[test]
    fn init_interactive_flow_respects_create_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // .wt/config/local.toml
        ui.add_select(0); // project recommendation
        ui.add_select(0); // use system editor
        ui.add_select(0); // Codex agent
        ui.add_confirm(false); // no agent args
        ui.add_confirm(false); // do not create config

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        let result = run(
            &ctx,
            InitOptions {
                local: false,
                shared: false,
                agent: None,
                agent_args: Vec::new(),
                agent_command: None,
                issue_provider: None,
                site_provider: None,
                gh_user: None,
                yes: false,
                force: false,
                ..InitOptions::default()
            },
        );

        assert!(result.is_err());
        assert!(!dir.path().join(".wt/config/local.toml").exists());
    }

    #[test]
    fn init_interactive_rerun_prefills_existing_config_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(".wt/config");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(
            local.join("local.toml"),
            r#"[profile.agent]
cli = "claude"
args = ["--model", "sonnet"]
command = "claude --resume"
timeout = 45
send_after = 4

[workflow]
pull_request = "draft"
landing = "auto"

[review]
codex_base = "required"

[workspace]
tabs = ["existing", "vim"]
colors = { task = "", issue = "cyan" }
"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_select(0); // use current config defaults
        ui.add_select(0); // use system editor
        ui.add_select(0); // prefilled claude agent
        ui.add_select(0); // keep existing args
        ui.add_confirm(true); // overwrite config
        ui.add_confirm(false); // do not add Claude allow rules
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                force: true,
                ..InitOptions::default()
            },
        )
        .unwrap();

        let select_items = ui.select_items.lock().unwrap().clone();
        assert_eq!(select_items[0][0], "기존 설정 파일 값 유지하기");
        assert_eq!(select_items[1][0], "시스템 editor 사용");
        assert_eq!(select_items[2][0], "Claude");
        assert_eq!(select_items[3][0], "기존 args 유지: --model sonnet");

        let content = std::fs::read_to_string(local.join("local.toml")).unwrap();
        let config: Config = toml::from_str(&content).unwrap();
        let policy = config.workflow_default_policy();
        let agent = config.profile.unwrap().agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Claude);
        assert_eq!(agent.args, vec!["--model", "sonnet"]);
        assert_eq!(agent.command.as_deref(), Some("claude --resume"));
        assert_eq!(policy.pull_request, WorkflowDefaultPullRequestMode::Draft);
        assert_eq!(policy.landing, WorkflowDefaultLandingPolicy::Auto);
        assert_eq!(policy.review.codex_base, ReviewCodexBasePolicy::Required);
        assert!(content.contains("[review]"));
        assert!(content.contains("codex_base = \"required\""));
        let workspace = config.workspace.unwrap();
        assert_eq!(
            workspace.tabs,
            vec!["existing".to_string(), "vim".to_string()]
        );
        assert_eq!(workspace.colors.get("task").map(String::as_str), Some(""));
        assert_eq!(
            workspace.colors.get("issue").map(String::as_str),
            Some("cyan")
        );
        assert_eq!(
            workspace.colors.get("branch").map(String::as_str),
            Some("green")
        );
        assert_eq!(
            workspace.colors.get("pr").map(String::as_str),
            Some("magenta")
        );
    }

    #[test]
    fn resolve_review_policy_omits_section_on_none_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // default none is index 0
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        let policy =
            resolve_review_policy(&ctx, &InitOptions::default(), &InitDefaults::default())
                .unwrap();

        assert!(
            policy.is_none(),
            "none selection must omit the review policy section"
        );
        let items = ui.select_items.lock().unwrap().clone();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].len(), 3);
    }

    #[test]
    fn resolve_review_policy_records_required_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(2); // [none, advisory, required]
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        let policy =
            resolve_review_policy(&ctx, &InitOptions::default(), &InitDefaults::default())
                .unwrap()
                .expect("required selection must record the review policy");

        assert_eq!(policy.codex_base, ReviewCodexBasePolicy::Required);
    }

    #[test]
    fn init_accepting_claude_allow_rules_creates_local_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(true); // create config
        ui.add_confirm(true); // add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                agent: Some(InitAgent::None),
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                ..InitOptions::default()
            },
        )
        .unwrap();

        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        let settings = read_claude_local_settings(&settings_path).unwrap();
        assert_eq!(
            allow_rules(&settings),
            vec!["Edit(/.wt/**)", "Write(/.wt/**)"]
        );
        assert!(settings.get("allowed").is_none());
        assert_eq!(
            settings,
            serde_json::json!({
                "permissions": {
                    "allow": [
                        "Edit(/.wt/**)",
                        "Write(/.wt/**)"
                    ]
                }
            })
        );
    }

    #[test]
    fn init_accepting_claude_allow_rules_migrates_legacy_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
  "allowed": [
    "Edit(/.wt/**)",
    "Write(/.wt/**)",
    "Bash(echo:*)"
  ]
}
"#,
        )
        .unwrap();

        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(true); // create config
        ui.add_confirm(true); // add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                agent: Some(InitAgent::None),
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                ..InitOptions::default()
            },
        )
        .unwrap();

        let settings = read_claude_local_settings(&settings_path).unwrap();
        assert!(settings.get("allowed").is_none());
        assert_eq!(
            allow_rules(&settings),
            vec!["Edit(/.wt/**)", "Write(/.wt/**)", "Bash(echo:*)"]
        );
    }

    #[test]
    fn init_declining_claude_allow_rules_leaves_settings_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mut ui = MockUi::new();
        ui.add_select(0); // use project recommendation
        ui.add_select(0); // use system editor
        ui.add_confirm(true); // create config
        ui.add_confirm(false); // do not add Claude allow rules
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(ui),
        );

        run(
            &ctx,
            InitOptions {
                local: true,
                agent: Some(InitAgent::None),
                issue_provider: Some(InitIssueProvider::None),
                site_provider: Some(InitSiteProvider::None),
                ..InitOptions::default()
            },
        )
        .unwrap();

        assert!(!dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH).exists());
    }

    #[test]
    fn claude_allow_rules_merge_migrates_legacy_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
  "allowed": [
    "Edit(/.wt/**)",
    "Write(/.wt/**)",
    "Bash(echo:*)",
    "Edit(/.wt/**)"
  ]
}
"#,
        )
        .unwrap();

        merge_claude_allow_rules(&settings_path).unwrap();
        let settings = read_claude_local_settings(&settings_path).unwrap();

        assert!(settings.get("allowed").is_none());
        assert_eq!(
            allow_rules(&settings),
            vec!["Edit(/.wt/**)", "Write(/.wt/**)", "Bash(echo:*)"]
        );
    }

    #[test]
    fn claude_allow_rules_merge_preserves_existing_permissions_allow_order_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
  "permissions": {
    "allow": [
      "Bash(npm run *)"
    ]
  }
}
"#,
        )
        .unwrap();

        merge_claude_allow_rules(&settings_path).unwrap();
        let first = std::fs::read_to_string(&settings_path).unwrap();
        merge_claude_allow_rules(&settings_path).unwrap();
        let second = std::fs::read_to_string(&settings_path).unwrap();

        assert_eq!(first, second);
        let settings: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert!(settings.get("allowed").is_none());
        assert_eq!(
            allow_rules(&settings),
            vec!["Bash(npm run *)", "Edit(/.wt/**)", "Write(/.wt/**)"]
        );
    }

    #[test]
    fn claude_allow_rules_merge_migrates_legacy_allowed_into_existing_permissions_allow() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
  "allowed": [
    "Bash(echo:*)",
    "Edit(/.wt/**)"
  ],
  "permissions": {
    "allow": [
      "Bash(git:*)",
      "Edit(/.wt/**)"
    ]
  }
}
"#,
        )
        .unwrap();

        merge_claude_allow_rules(&settings_path).unwrap();
        let settings = read_claude_local_settings(&settings_path).unwrap();

        assert!(settings.get("allowed").is_none());
        assert_eq!(
            allow_rules(&settings),
            vec![
                "Bash(git:*)",
                "Edit(/.wt/**)",
                "Bash(echo:*)",
                "Write(/.wt/**)"
            ]
        );
    }

    #[test]
    fn claude_allow_rules_merge_creates_fresh_schema_for_missing_and_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing_path = dir.path().join("missing").join(CLAUDE_LOCAL_SETTINGS_PATH);
        merge_claude_allow_rules(&missing_path).unwrap();
        assert_fresh_claude_allow_rules(&missing_path);

        let empty_path = dir.path().join("empty").join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(empty_path.parent().unwrap()).unwrap();
        std::fs::write(&empty_path, "\n").unwrap();
        merge_claude_allow_rules(&empty_path).unwrap();
        assert_fresh_claude_allow_rules(&empty_path);
    }

    #[test]
    fn claude_allow_rules_merge_rejects_non_object_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(&settings_path, r#"{"permissions":"not-an-object"}"#).unwrap();

        let err = merge_claude_allow_rules(&settings_path)
            .unwrap_err()
            .to_string();

        assert!(err.contains("`permissions` must be a JSON object"));
    }

    #[test]
    fn claude_allow_rules_merge_rejects_non_array_permissions_allow() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(CLAUDE_LOCAL_SETTINGS_PATH);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"permissions":{"allow":"not-an-array"}}"#,
        )
        .unwrap();

        let err = merge_claude_allow_rules(&settings_path)
            .unwrap_err()
            .to_string();

        assert!(err.contains("`permissions.allow` must be a JSON array"));
    }

    #[test]
    fn init_refuses_existing_file_without_force_and_overwrites_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(".wt/config");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("local.toml"), "[workspace]\ntabs = []\n").unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let options = InitOptions {
            local: true,
            shared: false,
            agent: Some(InitAgent::Claude),
            agent_args: Vec::new(),
            agent_command: None,
            issue_provider: None,
            site_provider: None,
            gh_user: None,
            yes: true,
            force: false,
            ..InitOptions::default()
        };
        assert!(run(&ctx, options).is_err());

        run(
            &ctx,
            InitOptions {
                force: true,
                ..InitOptions {
                    local: true,
                    shared: false,
                    agent: Some(InitAgent::Claude),
                    agent_args: Vec::new(),
                    agent_command: None,
                    issue_provider: None,
                    site_provider: None,
                    gh_user: None,
                    yes: true,
                    force: false,
                    ..InitOptions::default()
                }
            },
        )
        .unwrap();

        let content = std::fs::read_to_string(local.join("local.toml")).unwrap();
        let config = toml::from_str::<Config>(&content).unwrap();
        assert_eq!(config.profile.unwrap().agent.unwrap().cli, AgentCli::Claude);
        assert!(
            !dir.path()
                .join(".wt/config/profiles/claude/profile.toml")
                .exists()
        );
    }

    fn allow_rules(settings: &serde_json::Value) -> Vec<&str> {
        settings["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect()
    }

    fn assert_fresh_claude_allow_rules(path: &std::path::Path) {
        let settings = read_claude_local_settings(path).unwrap();
        assert!(settings.get("allowed").is_none());
        assert_eq!(
            settings,
            serde_json::json!({
                "permissions": {
                    "allow": [
                        "Edit(/.wt/**)",
                        "Write(/.wt/**)"
                    ]
                }
            })
        );
    }
}
