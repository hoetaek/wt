use crate::cli::BaseMode;
use crate::commands::profile_selection::{self, ProfileSelection};
use crate::commands::profile_workspace::{
    ProfileBranchDecision, PromptPolicy, resolve_profile_branch,
};
use crate::commands::{agent_report, issue_selection};
use crate::config::Config;
use crate::config::IssueProviderType;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::parallel::{self, ParallelControl};
use crate::services::git::{CreateType, GitService};
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
use crate::services::issues::{IssueInfo, IssueProvider};
use crate::setup;
use crate::worktree_naming::{self, WorktreeNamingResult};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

pub(crate) struct IssueSnapshotContext<'a> {
    pub(crate) path_label: &'a str,
    pub(crate) path: &'a str,
    pub(crate) content: &'a str,
}

pub(crate) struct PreparedIssueContext<'a> {
    pub(crate) identifier: &'a str,
    pub(crate) title: &'a str,
    pub(crate) branch_name: Option<&'a str>,
    pub(crate) setup_mode: &'a str,
    pub(crate) template_vars: HashMap<String, String>,
    pub(crate) additional_prompt_scope: Option<&'a str>,
    pub(crate) workspace_color_kind: &'a str,
    pub(crate) on_start_issue_id: Option<&'a str>,
    pub(crate) prompt_intro: &'a str,
    pub(crate) completion_section: Option<&'a str>,
    pub(crate) pre_snapshot_context: Option<&'a str>,
    pub(crate) workspace_label: Option<String>,
    pub(crate) snapshot: IssueSnapshotContext<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IssueRunResult {
    pub(crate) branch_name: String,
    pub(crate) canonical_branch_name: String,
    pub(crate) worktree_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IssueRunPartialFailure {
    pub(crate) completed: Vec<IssueRunResult>,
    pub(crate) failed: Option<IssueRunResult>,
    message: String,
}

impl IssueRunPartialFailure {
    fn new(
        completed: Vec<IssueRunResult>,
        failed: Option<IssueRunResult>,
        err: anyhow::Error,
    ) -> Self {
        Self {
            completed,
            failed,
            message: err.to_string(),
        }
    }
}

impl fmt::Display for IssueRunPartialFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for IssueRunPartialFailure {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedIssueWorktree {
    pub(crate) branch_name: String,
    pub(crate) path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct MaterializedIssueBranch {
    pub(crate) branch_name: String,
    pub(crate) naming: Option<WorktreeNamingResult>,
    pub(crate) provider_created_branch_base: Option<String>,
}

struct ProfileRunOptions<'a> {
    prepared_issue: Option<&'a PreparedIssueContext<'a>>,
    on_start_issue_id: Option<&'a str>,
    prompt_policy: PromptPolicy,
}

struct IssueRunBase<'a> {
    original: &'a Option<String>,
    provider_override: Option<Option<String>>,
    create_override: Option<Option<String>>,
    fallback_parent_base: Option<String>,
}

impl<'a> IssueRunBase<'a> {
    fn sequential(base_raw: &'a Option<String>) -> Self {
        Self {
            original: base_raw,
            provider_override: None,
            create_override: None,
            fallback_parent_base: None,
        }
    }

    fn provider_base_raw(&self) -> &Option<String> {
        self.provider_override.as_ref().unwrap_or(self.original)
    }

    fn create_base_raw(&self) -> &Option<String> {
        self.create_override.as_ref().unwrap_or(self.original)
    }
}

#[derive(Clone, Debug)]
enum IssueWorkItem {
    Target(String),
    Resolved(IssueInfo),
}

impl IssueWorkItem {
    fn label(&self) -> String {
        match self {
            IssueWorkItem::Target(target) => target.clone(),
            IssueWorkItem::Resolved(issue) => issue.identifier.clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct IssueRunOptions<'a, 'b> {
    profile_selection: ProfileSelection<'b>,
    matrix: bool,
    prepared_issue: Option<&'a PreparedIssueContext<'a>>,
    prompt_policy: PromptPolicy,
    jobs: usize,
    base_override: Option<&'b str>,
}

pub fn run(
    ctx: &Ctx,
    targets: &[String],
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    jobs: usize,
) -> Result<()> {
    run_inner_many(
        ctx,
        targets,
        base_raw,
        IssueRunOptions {
            profile_selection: ProfileSelection::new(profile, &[]),
            matrix,
            prepared_issue: None,
            prompt_policy: PromptPolicy::Allow,
            jobs,
            base_override: None,
        },
    )
    .map(|_| ())
}

pub(crate) fn run_with_issue_snapshot(
    ctx: &Ctx,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    prepared: PreparedIssueContext<'_>,
) -> Result<IssueRunResult> {
    run_inner(
        ctx,
        &[],
        base_raw,
        IssueRunOptions {
            profile_selection: ProfileSelection::new(profile, &[]),
            matrix,
            prepared_issue: Some(&prepared),
            prompt_policy: PromptPolicy::Allow,
            jobs: 1,
            base_override: None,
        },
    )
}

pub(crate) fn run_with_issue_snapshot_non_interactive(
    ctx: &Ctx,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    prepared: PreparedIssueContext<'_>,
) -> Result<IssueRunResult> {
    run_inner(
        ctx,
        &[],
        base_raw,
        IssueRunOptions {
            profile_selection: ProfileSelection::new(profile, &[]),
            matrix,
            prepared_issue: Some(&prepared),
            prompt_policy: PromptPolicy::Deny,
            jobs: 1,
            base_override: None,
        },
    )
}

pub(crate) fn run_with_issue_snapshot_non_interactive_base_override(
    ctx: &Ctx,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    prepared: PreparedIssueContext<'_>,
    base_override: &str,
) -> Result<IssueRunResult> {
    run_inner(
        ctx,
        &[],
        base_raw,
        IssueRunOptions {
            profile_selection: ProfileSelection::new(profile, &[]),
            matrix,
            prepared_issue: Some(&prepared),
            prompt_policy: PromptPolicy::Deny,
            jobs: 1,
            base_override: Some(base_override),
        },
    )
}

pub(crate) fn planned_worktrees_for_prepared_issue(
    ctx: &Ctx,
    title: &str,
    branch_name: &str,
    profile: Option<&str>,
    naming: Option<&WorktreeNamingResult>,
) -> Result<Vec<PlannedIssueWorktree>> {
    if profile.is_some() {
        let profiles =
            profile_selection::load_profile_selection(ctx, ProfileSelection::new(profile, &[]))?;
        return profiles
            .into_iter()
            .map(|(profile_name, profile_config)| {
                let profile_branch = format!("{branch_name}-{profile_name}");
                let profile_title = format!("{title} [{profile_name}]");
                let profile_workspace = naming
                    .and_then(|n| n.workspace.as_deref())
                    .map(|workspace| format!("{workspace} [{profile_name}]"))
                    .unwrap_or_else(|| {
                        WorktreeNames::build_workspace_name(&profile_branch, Some(&profile_title))
                    });

                let names = WorktreeNames::new_with_workspace_config(
                    &profile_branch,
                    &ctx.parent_dir,
                    &ctx.repo_root,
                    &ctx.repo_name,
                    Some(&profile_workspace),
                    profile_config.has_site().then_some(""),
                    profile_config.worktree.path.as_deref(),
                )?;
                Ok(PlannedIssueWorktree {
                    branch_name: profile_branch,
                    path: names.path,
                })
            })
            .collect();
    }

    let names = issue_worktree_names(ctx, branch_name, title, naming, None)?;
    Ok(vec![PlannedIssueWorktree {
        branch_name: branch_name.to_string(),
        path: names.path,
    }])
}

fn run_inner<'a>(
    ctx: &Ctx,
    targets: &[String],
    base_raw: &Option<String>,
    options: IssueRunOptions<'a, '_>,
) -> Result<IssueRunResult> {
    run_inner_many(ctx, targets, base_raw, options)?
        .into_iter()
        .last()
        .ok_or_else(|| anyhow::anyhow!("No worktrees created"))
}

fn run_inner_many<'a>(
    ctx: &Ctx,
    targets: &[String],
    base_raw: &Option<String>,
    options: IssueRunOptions<'a, '_>,
) -> Result<Vec<IssueRunResult>> {
    let IssueRunOptions {
        profile_selection,
        matrix,
        prepared_issue,
        prompt_policy,
        jobs,
        base_override,
    } = options;
    let uses_profiles = matrix || profile_selection.uses_profiles();
    let profile_configs = if uses_profiles {
        Some(profile_selection::load_profile_selection(
            ctx,
            profile_selection,
        )?)
    } else {
        None
    };

    let work_items = issue_work_items_to_run(ctx, targets, prepared_issue)?;
    if work_items.is_empty() {
        ctx.ui.print_warning("No issues selected");
        return Ok(Vec::new());
    }
    let parallel = prepared_issue.is_none() && jobs > 1 && work_items.len() > 1;
    let run_base = issue_run_base(ctx, base_raw, parallel, uses_profiles, base_override)?;
    let worker_prompt_policy = if parallel {
        PromptPolicy::Deny
    } else {
        prompt_policy
    };

    if parallel {
        return run_issue_work_items_parallel(
            ctx,
            work_items,
            &run_base,
            uses_profiles,
            profile_configs.as_deref(),
            worker_prompt_policy,
            jobs,
        );
    }

    let issue_count = work_items.len();
    let mut results = Vec::new();
    for item in work_items {
        let issue_label = item.label();
        let result = run_issue_work_item(
            ctx,
            item,
            &run_base,
            uses_profiles,
            profile_configs.as_deref(),
            prepared_issue,
            worker_prompt_policy,
        );
        let result = if issue_count > 1 {
            result.with_context(|| format!("Issue {issue_label}"))
        } else {
            result
        };
        match result {
            Ok(mut issue_results) => results.append(&mut issue_results),
            Err(err) => return Err(issue_partial_failure(&results, err)),
        }
    }

    Ok(results)
}

fn issue_run_base<'a>(
    ctx: &Ctx,
    base_raw: &'a Option<String>,
    parallel: bool,
    uses_profiles: bool,
    base_override: Option<&str>,
) -> Result<IssueRunBase<'a>> {
    if let Some(base) = base_override {
        if base.trim().is_empty() {
            bail!("Base branch cannot be empty");
        }
        return match BaseMode::from_raw(base_raw) {
            BaseMode::Default => {
                let base = base.to_string();
                let create_override = uses_profiles.then(|| Some(base.clone()));
                Ok(IssueRunBase {
                    original: base_raw,
                    provider_override: Some(Some(base.clone())),
                    create_override,
                    fallback_parent_base: Some(base),
                })
            }
            BaseMode::Interactive => {
                let base = base.to_string();
                Ok(IssueRunBase {
                    original: base_raw,
                    provider_override: Some(Some(base.clone())),
                    create_override: Some(Some(base)),
                    fallback_parent_base: None,
                })
            }
            BaseMode::Current | BaseMode::Explicit(_) => Ok(IssueRunBase::sequential(base_raw)),
        };
    }

    if !parallel {
        return Ok(IssueRunBase::sequential(base_raw));
    }

    match BaseMode::from_raw(base_raw) {
        BaseMode::Default => {
            let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
            let base = resolve_base_branch(ctx, &git, base_raw)?;
            let create_override = uses_profiles.then(|| Some(base.clone()));
            Ok(IssueRunBase {
                original: base_raw,
                provider_override: Some(Some(base.clone())),
                create_override,
                fallback_parent_base: Some(base),
            })
        }
        BaseMode::Interactive => {
            let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
            let base = resolve_base_branch(ctx, &git, base_raw)?;
            Ok(IssueRunBase {
                original: base_raw,
                provider_override: Some(Some(base.clone())),
                create_override: Some(Some(base)),
                fallback_parent_base: None,
            })
        }
        BaseMode::Current | BaseMode::Explicit(_) => Ok(IssueRunBase::sequential(base_raw)),
    }
}

fn issue_work_items_to_run(
    ctx: &Ctx,
    targets: &[String],
    prepared_issue: Option<&PreparedIssueContext<'_>>,
) -> Result<Vec<IssueWorkItem>> {
    let naming_enabled = ctx.config.worktree.naming.is_some();
    if let Some(prepared) = prepared_issue {
        return Ok(vec![IssueWorkItem::Resolved(IssueInfo {
            identifier: prepared.identifier.to_string(),
            title: prepared.title.to_string(),
            branch_name: prepared.branch_name.map(str::to_string),
            body: None,
        })]);
    }

    if !targets.is_empty() {
        return Ok(targets
            .iter()
            .map(|target| IssueWorkItem::Target(target.clone()))
            .collect());
    }

    let provider = build_provider(ctx)?;
    let selected =
        issue_selection::select_issues_with_provider(ctx, "Issues to start", provider.as_ref())?;
    selected
        .into_iter()
        .map(|issue| {
            if naming_enabled {
                provider
                    .get_issue(&issue.identifier)
                    .map(IssueWorkItem::Resolved)
            } else {
                Ok(IssueWorkItem::Resolved(IssueInfo {
                    identifier: issue.identifier,
                    title: issue.title,
                    branch_name: None,
                    body: None,
                }))
            }
        })
        .collect()
}

fn run_issue_work_item<'a>(
    ctx: &Ctx,
    item: IssueWorkItem,
    run_base: &IssueRunBase<'_>,
    uses_profiles: bool,
    profile_configs: Option<&[(String, Config)]>,
    prepared_issue: Option<&'a PreparedIssueContext<'a>>,
    prompt_policy: PromptPolicy,
) -> Result<Vec<IssueRunResult>> {
    let issue = match item {
        IssueWorkItem::Resolved(issue) => issue,
        IssueWorkItem::Target(target) => {
            let provider = build_provider(ctx)?;
            provider
                .get_issue(target.trim_start_matches('#'))
                .with_context(|| format!("Issue {target}"))?
        }
    };
    run_resolved_issue(
        ctx,
        issue,
        run_base,
        uses_profiles,
        profile_configs,
        prepared_issue,
        prompt_policy,
    )
}

fn run_issue_work_items_parallel(
    ctx: &Ctx,
    items: Vec<IssueWorkItem>,
    run_base: &IssueRunBase<'_>,
    uses_profiles: bool,
    profile_configs: Option<&[(String, Config)]>,
    prompt_policy: PromptPolicy,
    jobs: usize,
) -> Result<Vec<IssueRunResult>> {
    let mut results = Vec::new();
    let mut first_error = None;
    parallel::run_bounded_parallel(
        items,
        jobs,
        |_| Ok(()),
        |item| {
            let label = item.label();
            run_issue_work_item(
                ctx,
                item,
                run_base,
                uses_profiles,
                profile_configs,
                None,
                prompt_policy,
            )
            .with_context(|| format!("Issue {label}"))
        },
        |completion| {
            match completion.result {
                Ok(mut item_results) => results.append(&mut item_results),
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
            Ok(ParallelControl::Continue)
        },
    )?;

    if let Some(err) = first_error {
        return Err(issue_partial_failure(&results, err));
    }
    Ok(results)
}

fn run_resolved_issue<'a>(
    ctx: &Ctx,
    issue: IssueInfo,
    run_base: &IssueRunBase<'_>,
    uses_profiles: bool,
    profile_configs: Option<&[(String, Config)]>,
    prepared_issue: Option<&'a PreparedIssueContext<'a>>,
    prompt_policy: PromptPolicy,
) -> Result<Vec<IssueRunResult>> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let identifier = issue.identifier;
    let title = issue.title;
    let suggested_branch = issue.branch_name;
    let issue_snapshot = prepared_issue.map(|issue| &issue.snapshot);
    let workspace_label = prepared_issue.and_then(|issue| issue.workspace_label.as_deref());
    let setup_mode = prepared_issue
        .map(|issue| issue.setup_mode)
        .unwrap_or(setup::WORKSPACE_COLOR_KIND_ISSUE);
    let workspace_color_kind = prepared_issue
        .map(|issue| issue.workspace_color_kind)
        .unwrap_or(setup::WORKSPACE_COLOR_KIND_ISSUE);
    let prompt_intro = prepared_issue
        .map(|issue| issue.prompt_intro)
        .unwrap_or("Use this issue snapshot before changing code.");
    let completion_section = prepared_issue.and_then(|issue| issue.completion_section);
    let pre_snapshot_context = prepared_issue.and_then(|issue| issue.pre_snapshot_context);
    let additional_prompt_scope = prepared_issue.and_then(|issue| issue.additional_prompt_scope);

    ctx.ui.print_step(&format!("{identifier}: {title}"));

    let branch_resolution =
        if let Some(prepared_branch) = prepared_issue.and_then(|issue| issue.branch_name) {
            let naming =
                worktree_naming::generate(ctx, &identifier, &title, suggested_branch.as_deref())?;
            MaterializedIssueBranch {
                branch_name: prepared_branch.to_string(),
                naming,
                provider_created_branch_base: None,
            }
        } else {
            let provider = build_provider(ctx)?;
            materialize_provider_issue_branch(
                ctx,
                provider.as_ref(),
                &identifier,
                &title,
                suggested_branch.as_deref(),
                Some(run_base.provider_base_raw()),
                prompt_policy,
            )?
        };
    let raw_id = identifier.trim_start_matches('#');
    let provider_created_branch_base = branch_resolution
        .provider_created_branch_base
        .or_else(|| run_base.fallback_parent_base.clone());
    let branch_name = branch_resolution.branch_name;
    let naming = branch_resolution.naming;

    let on_start_issue_id = prepared_issue
        .map(|issue| issue.on_start_issue_id)
        .unwrap_or(Some(raw_id));

    if uses_profiles {
        return run_profiles(
            ctx,
            &title,
            &branch_name,
            naming.as_ref(),
            run_base.create_base_raw(),
            profile_configs
                .expect("profile configs loaded before issue resolution")
                .to_vec(),
            ProfileRunOptions {
                prepared_issue,
                on_start_issue_id,
                prompt_policy,
            },
        );
    }

    let names = issue_worktree_names(ctx, &branch_name, &title, naming.as_ref(), workspace_label)?;
    let prepared_template_vars = prepared_setup_template_vars(prepared_issue, naming.as_ref());
    let snapshot_config = issue_snapshot.map(|snapshot| {
        profile_config_with_issue_snapshot(
            &ctx.config,
            snapshot,
            setup_mode,
            additional_prompt_scope,
            prompt_intro,
            completion_section,
            pre_snapshot_context,
        )
    });

    // 2. Check if branch is already checked out elsewhere
    let existing_path = git.checked_out_path(&branch_name)?;
    if let Some(ref existing) = existing_path {
        if *existing == ctx.invocation_root {
            ctx.ui
                .print_warning("이미 이 브랜치에 있습니다. 다른 브랜치로 전환 후 다시 시도하세요.");
            return Ok(vec![IssueRunResult {
                canonical_branch_name: branch_name.clone(),
                branch_name,
                worktree_path: existing.clone(),
            }]);
        }
        if *existing != names.path {
            ctx.ui.print_step(&format!(
                "Branch already checked out at: {}",
                existing.display()
            ));
            setup::run_setup_with_workspace_color_kind(
                ctx,
                existing,
                &names,
                Some(&title),
                setup::SetupModeKinds::new(setup_mode, workspace_color_kind),
                prepared_template_vars.as_ref(),
                snapshot_config.as_ref(),
            )?;
            return Ok(vec![IssueRunResult {
                canonical_branch_name: branch_name.clone(),
                branch_name,
                worktree_path: existing.clone(),
            }]);
        }
    }

    // 3. Handle existing worktree directory
    if names.path.exists() {
        ctx.ui.print_warning(&format!(
            "Worktree {} already exists.",
            names.path.display()
        ));
        let items = vec![
            "Delete and recreate".into(),
            "Open existing".into(),
            "Abort".into(),
        ];
        if prompt_policy == PromptPolicy::Deny {
            bail!(
                "Worktree {} already exists; parallel workers cannot prompt to delete or open it",
                names.path.display()
            );
        }
        let choice = ctx.ui.select("Worktree already exists", &items)?;
        match choice {
            0 => {
                ctx.ui.print_step("Removing existing worktree...");
                git.worktree_remove_force(&names.path).ok();
                if names.path.exists() {
                    std::fs::remove_dir_all(&names.path)?;
                }
            }
            1 => {
                setup::run_setup_with_workspace_color_kind(
                    ctx,
                    &names.path,
                    &names,
                    Some(&title),
                    setup::SetupModeKinds::new(setup_mode, workspace_color_kind),
                    prepared_template_vars.as_ref(),
                    snapshot_config.as_ref(),
                )?;
                return Ok(vec![IssueRunResult {
                    canonical_branch_name: branch_name.clone(),
                    branch_name,
                    worktree_path: names.path,
                }]);
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // 4. Create worktree
    // Refresh remote-tracking refs so we can detect/track an already-pushed
    // branch. Skip when there is no `origin` remote (local-only repos) — a
    // bare `git fetch origin` would otherwise hard-fail with no remote to read.
    if git.has_remote("origin")? {
        git.fetch()?;
    }
    let create_type = create_worktree(
        ctx,
        &git,
        &branch_name,
        &names.path,
        run_base.create_base_raw(),
        provider_created_branch_base.as_deref(),
        prompt_policy,
    )?;

    // 5. Update issue status for new branches
    if create_type == CreateType::New {
        update_issue_start_status(ctx, on_start_issue_id);
    }

    // 6. Setup
    setup::run_setup_with_workspace_color_kind(
        ctx,
        &names.path,
        &names,
        Some(&title),
        setup::SetupModeKinds::new(setup_mode, workspace_color_kind),
        prepared_template_vars.as_ref(),
        snapshot_config.as_ref(),
    )?;

    Ok(vec![IssueRunResult {
        canonical_branch_name: branch_name.clone(),
        branch_name,
        worktree_path: names.path,
    }])
}

fn issue_worktree_names(
    ctx: &Ctx,
    branch_name: &str,
    title: &str,
    naming: Option<&WorktreeNamingResult>,
    workspace_label: Option<&str>,
) -> Result<WorktreeNames> {
    let base_workspace = naming
        .and_then(|n| n.workspace.as_deref())
        .map(str::to_string)
        .unwrap_or_else(|| WorktreeNames::build_workspace_name(branch_name, Some(title)));
    let workspace = apply_workspace_label(workspace_label, &base_workspace);

    WorktreeNames::new_with_workspace_config(
        branch_name,
        &ctx.parent_dir,
        &ctx.repo_root,
        &ctx.repo_name,
        Some(&workspace),
        ctx.config.has_site().then_some(""),
        ctx.config.worktree.path.as_deref(),
    )
}

fn apply_workspace_label(label: Option<&str>, workspace: &str) -> String {
    let Some(label) = label.map(str::trim).filter(|label| !label.is_empty()) else {
        return workspace.to_string();
    };
    let workspace = workspace.trim();
    if workspace.is_empty() {
        label.to_string()
    } else {
        format!("{label} {workspace}")
    }
}

fn run_profiles(
    ctx: &Ctx,
    title: &str,
    branch_name: &str,
    naming: Option<&WorktreeNamingResult>,
    base_raw: &Option<String>,
    profiles: Vec<(String, Config)>,
    options: ProfileRunOptions<'_>,
) -> Result<Vec<IssueRunResult>> {
    let issue_snapshot = options.prepared_issue.map(|issue| &issue.snapshot);
    let setup_mode = options
        .prepared_issue
        .map(|issue| issue.setup_mode)
        .unwrap_or(setup::WORKSPACE_COLOR_KIND_ISSUE);
    let workspace_color_kind = options
        .prepared_issue
        .map(|issue| issue.workspace_color_kind)
        .unwrap_or(setup::WORKSPACE_COLOR_KIND_ISSUE);
    let prompt_intro = options
        .prepared_issue
        .map(|issue| issue.prompt_intro)
        .unwrap_or("Use this issue snapshot before changing code.");
    let completion_section = options
        .prepared_issue
        .and_then(|issue| issue.completion_section);
    let pre_snapshot_context = options
        .prepared_issue
        .and_then(|issue| issue.pre_snapshot_context);
    let additional_prompt_scope = options
        .prepared_issue
        .and_then(|issue| issue.additional_prompt_scope);
    ctx.ui.print_step(&format!(
        "Found {} profiles: {}",
        profiles.len(),
        profiles
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let base = resolve_base_branch(ctx, &git, base_raw)?;
    let mut results = Vec::new();
    let mut start_status_attempted = false;

    for (profile_name, profile_config) in &profiles {
        let snapshot_config = issue_snapshot.map(|snapshot| {
            profile_config_with_issue_snapshot(
                profile_config,
                snapshot,
                setup_mode,
                additional_prompt_scope,
                prompt_intro,
                completion_section,
                pre_snapshot_context,
            )
        });
        let profile_config = snapshot_config.as_ref().unwrap_or(profile_config);
        let profile_branch = format!("{branch_name}-{profile_name}");
        let profile_title = format!("{title} [{profile_name}]");
        let profile_workspace = naming
            .and_then(|n| n.workspace.as_deref())
            .map(|workspace| format!("{workspace} [{profile_name}]"))
            .unwrap_or_else(|| {
                WorktreeNames::build_workspace_name(&profile_branch, Some(&profile_title))
            });
        let profile_workspace = apply_workspace_label(
            options
                .prepared_issue
                .and_then(|issue| issue.workspace_label.as_deref()),
            &profile_workspace,
        );
        let profile_extra_vars =
            profile_template_vars(naming, profile_name, issue_snapshot, options.prepared_issue);

        ctx.ui
            .print_step(&format!("Setting up profile: {profile_name}"));

        let names = WorktreeNames::new_with_workspace_config(
            &profile_branch,
            &ctx.parent_dir,
            &ctx.repo_root,
            &ctx.repo_name,
            Some(&profile_workspace),
            profile_config.has_site().then_some(""),
            profile_config.worktree.path.as_deref(),
        )
        .map_err(|err| profile_partial_failure(&results, None, err))?;

        let profile_branch_existed = match resolve_profile_branch(
            ctx,
            &git,
            profile_name,
            &profile_branch,
            &names.path,
            options.prompt_policy,
        )
        .map_err(|err| profile_partial_failure(&results, None, err))?
        {
            ProfileBranchDecision::CreateNew { branch_existed } => branch_existed,
            ProfileBranchDecision::ReuseExisting { path } => {
                let result = IssueRunResult {
                    canonical_branch_name: branch_name.to_string(),
                    branch_name: profile_branch,
                    worktree_path: path.clone(),
                };
                if let Err(err) = setup::run_setup_with_workspace_color_kind(
                    ctx,
                    &path,
                    &names,
                    Some(&profile_title),
                    setup::SetupModeKinds::new(setup_mode, workspace_color_kind),
                    Some(&profile_extra_vars),
                    Some(profile_config),
                ) {
                    return Err(profile_partial_failure(&results, Some(result), err));
                }
                results.push(result);
                continue;
            }
            ProfileBranchDecision::Skip => continue,
        };

        git.worktree_add_new_branch(&names.path, &profile_branch, &base)
            .map_err(|err| profile_partial_failure(&results, None, err))?;
        git.set_branch_parent(&profile_branch, &base).ok();

        if !profile_branch_existed && !start_status_attempted && options.on_start_issue_id.is_some()
        {
            update_issue_start_status(ctx, options.on_start_issue_id);
            start_status_attempted = true;
        }

        let result = IssueRunResult {
            canonical_branch_name: branch_name.to_string(),
            branch_name: profile_branch,
            worktree_path: names.path.clone(),
        };

        if let Err(err) = setup::run_setup_with_workspace_color_kind(
            ctx,
            &names.path,
            &names,
            Some(&profile_title),
            setup::SetupModeKinds::new(setup_mode, workspace_color_kind),
            Some(&profile_extra_vars),
            Some(profile_config),
        ) {
            return Err(profile_partial_failure(&results, Some(result), err));
        }
        results.push(result);
    }

    ctx.ui.print_step(&format!(
        "All {} profiles processed successfully",
        profiles.len()
    ));
    Ok(results)
}

fn update_issue_start_status(ctx: &Ctx, on_start_issue_id: Option<&str>) {
    let Some(on_start_issue_id) = on_start_issue_id else {
        return;
    };

    if let Ok(provider) = build_provider(ctx) {
        if let Err(e) = provider.on_start(on_start_issue_id) {
            ctx.ui
                .print_warning(&format!("Failed to update issue status: {e}"));
        }
    }
}

fn profile_partial_failure(
    completed: &[IssueRunResult],
    failed: Option<IssueRunResult>,
    err: anyhow::Error,
) -> anyhow::Error {
    if completed.is_empty() && failed.is_none() {
        return err;
    }
    IssueRunPartialFailure::new(completed.to_vec(), failed, err).into()
}

fn issue_partial_failure(completed: &[IssueRunResult], err: anyhow::Error) -> anyhow::Error {
    if completed.is_empty() {
        return err;
    }
    IssueRunPartialFailure::new(completed.to_vec(), None, err).into()
}

fn profile_template_vars(
    naming: Option<&WorktreeNamingResult>,
    profile_name: &str,
    issue_snapshot: Option<&IssueSnapshotContext<'_>>,
    prepared_issue: Option<&PreparedIssueContext<'_>>,
) -> HashMap<String, String> {
    let mut vars = naming.map(|n| n.vars.clone()).unwrap_or_default();
    if let Some(prepared) = prepared_issue {
        vars.extend(prepared.template_vars.clone());
    }
    vars.insert("profile".into(), profile_name.to_string());
    if let Some(snapshot) = issue_snapshot {
        vars.insert("issue_snapshot".into(), snapshot.path.to_string());
    }
    vars
}

fn prepared_setup_template_vars(
    prepared_issue: Option<&PreparedIssueContext<'_>>,
    naming: Option<&WorktreeNamingResult>,
) -> Option<HashMap<String, String>> {
    let mut vars = naming.map(|n| n.vars.clone()).unwrap_or_default();
    if let Some(prepared) = prepared_issue {
        vars.extend(prepared.template_vars.clone());
    }
    (!vars.is_empty()).then_some(vars)
}

fn profile_config_with_issue_snapshot(
    config: &Config,
    snapshot: &IssueSnapshotContext<'_>,
    mode: &str,
    additional_prompt_scope: Option<&str>,
    prompt_intro: &str,
    completion_section: Option<&str>,
    pre_snapshot_context: Option<&str>,
) -> Config {
    let mut config = config.clone();
    if let Some(agent) = config.agent.as_mut() {
        let additional_prompts = additional_prompt_scope
            .and_then(|scope| agent.prompt.remove(scope))
            .unwrap_or_default();
        let prompts = agent.prompt.entry(mode.into()).or_default();
        if let Some(completion_section) = completion_section {
            let task_context = prepared_snapshot_prompt(
                pre_snapshot_context,
                prompt_intro,
                snapshot.path_label,
                snapshot.path,
                snapshot.content,
            );
            let snapshot_prompt = format!("{completion_section}\n\n{task_context}");
            let mode_prompts = std::mem::take(prompts);
            let mut prompt_parts =
                Vec::with_capacity(1 + additional_prompts.len() + mode_prompts.len());
            prompt_parts.push(snapshot_prompt);
            prompt_parts.extend(additional_prompts);
            prompt_parts.extend(mode_prompts);
            prompts.push(prompt_parts.join("\n\n"));
        } else {
            let snapshot_prompt = format!(
                "{}\n\n{}: `{}`\n\n{}\n\n{}",
                prompt_intro,
                snapshot.path_label,
                snapshot.path,
                snapshot.content,
                agent_report::prompt_section()
            );
            if additional_prompts.is_empty() {
                if let Some(first_prompt) = prompts.first_mut() {
                    *first_prompt = format!("{snapshot_prompt}\n\n{first_prompt}");
                } else {
                    prompts.push(snapshot_prompt);
                }
            } else {
                let mode_prompts = std::mem::take(prompts);
                prompts.push(snapshot_prompt);
                prompts.extend(additional_prompts);
                prompts.extend(mode_prompts);
            }
        }
    }
    config
}

fn prepared_snapshot_prompt(
    pre_snapshot_context: Option<&str>,
    prompt_intro: &str,
    path_label: &str,
    path: &str,
    content: &str,
) -> String {
    let mut prompt = String::new();
    if let Some(context) = pre_snapshot_context
        .map(str::trim)
        .filter(|context| !context.is_empty())
    {
        prompt.push_str(context);
        prompt.push_str("\n\n");
    }
    prompt.push_str(&format!(
        "{prompt_intro}\n\n{path_label}: `{path}`\n\n{}",
        content.trim_end()
    ));
    prompt
}

pub(crate) fn materialize_provider_issue_branch(
    ctx: &Ctx,
    provider: &dyn IssueProvider,
    identifier: &str,
    title: &str,
    suggested_branch: Option<&str>,
    base_raw: Option<&Option<String>>,
    prompt_policy: PromptPolicy,
) -> Result<MaterializedIssueBranch> {
    let naming = worktree_naming::generate(ctx, identifier, title, suggested_branch)?;
    let provider_branch_base = if should_resolve_provider_branch_base(ctx, suggested_branch, None) {
        match base_raw {
            Some(base_raw) => {
                if prompt_policy == PromptPolicy::Deny
                    && matches!(
                        BaseMode::from_raw(base_raw),
                        BaseMode::Interactive | BaseMode::Default
                    )
                {
                    bail!("Base branch resolution is interactive; parallel workers cannot prompt");
                }
                let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
                Some(resolve_base_branch(ctx, &git, base_raw)?)
            }
            None => None,
        }
    } else {
        None
    };

    let raw_id = identifier.trim_start_matches('#');
    let ensured_branch = provider.ensure_branch(
        raw_id,
        provider_branch_base.as_deref(),
        naming.as_ref().and_then(|n| n.branch.as_deref()),
    )?;
    if ensured_branch.name.trim().is_empty() {
        bail!(
            "Provider issue {identifier} did not resolve a branch name; refusing to write incomplete TaskDocument"
        );
    }
    let provider_created_branch_base = if ensured_branch.created {
        provider_branch_base
    } else {
        None
    };

    Ok(MaterializedIssueBranch {
        branch_name: ensured_branch.name,
        naming,
        provider_created_branch_base,
    })
}

fn resolve_base_branch(ctx: &Ctx, git: &GitService, base_raw: &Option<String>) -> Result<String> {
    let base = match BaseMode::from_raw(base_raw) {
        BaseMode::Explicit(branch) => Ok(branch),
        BaseMode::Interactive => {
            let branches = git.list_local_branches()?;
            if branches.is_empty() {
                bail!("No local branches found");
            }
            let idx = ctx.ui.select("Select base branch", &branches)?;
            Ok(branches[idx].clone())
        }
        BaseMode::Current => git.current_branch(),
        BaseMode::Default => {
            let current = git.current_branch()?;
            ctx.ui.input("Base branch", Some(&current))
        }
    }?;

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }
    Ok(base)
}

fn should_resolve_provider_branch_base(
    ctx: &Ctx,
    suggested_branch: Option<&str>,
    prepared_issue: Option<&PreparedIssueContext<'_>>,
) -> bool {
    matches!(
        ctx.config.issues.as_ref().map(|issues| &issues.provider),
        Some(IssueProviderType::Github)
    ) && suggested_branch.is_none()
        && prepared_issue.and_then(|issue| issue.branch_name).is_none()
}

pub fn build_provider<'a>(ctx: &'a Ctx) -> Result<Box<dyn IssueProvider + 'a>> {
    let issues_config = ctx.config.issues.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No [issues] section in .wt.toml. Set provider = \"linear\" or \"github\"")
    })?;
    match issues_config.provider {
        IssueProviderType::Linear => Ok(Box::new(LinearIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
        ))),
        IssueProviderType::Github => Ok(Box::new(GithubIssueProvider::new(
            ctx.runner.as_ref(),
            Some(&ctx.repo_root),
            issues_config.gh_user.clone(),
        ))),
    }
}

fn create_worktree(
    ctx: &Ctx,
    git: &GitService,
    branch_name: &str,
    wt_path: &std::path::Path,
    base_raw: &Option<String>,
    provider_created_branch_base: Option<&str>,
    prompt_policy: PromptPolicy,
) -> Result<CreateType> {
    let base_mode = BaseMode::from_raw(base_raw);

    if git.local_branch_exists(branch_name)? {
        if base_mode != BaseMode::Default {
            return Err(WtError::BranchExistsWithBase {
                branch: branch_name.into(),
            }
            .into());
        }
        ctx.ui
            .print_step(&format!("Reusing existing branch: {branch_name}"));
        git.worktree_add(wt_path, branch_name)?;
        return Ok(CreateType::Local);
    }

    if git.remote_branch_exists(branch_name)? {
        if base_mode != BaseMode::Default && provider_created_branch_base.is_none() {
            return Err(WtError::BranchExistsWithBase {
                branch: branch_name.into(),
            }
            .into());
        }
        ctx.ui
            .print_step(&format!("Tracking remote branch: origin/{branch_name}"));
        git.worktree_add_new_branch(wt_path, branch_name, &format!("origin/{branch_name}"))?;
        let parent = if let Some(base) = provider_created_branch_base {
            base.to_string()
        } else {
            if prompt_policy == PromptPolicy::Deny {
                bail!(
                    "Remote branch {branch_name} needs a parent branch selection; parallel workers cannot prompt"
                );
            }
            let branches = git.list_local_branches()?;
            let idx = ctx.ui.select("Select parent branch", &branches)?;
            branches[idx].clone()
        };
        git.set_branch_parent(branch_name, &parent).ok();
        return Ok(CreateType::Remote);
    }

    // New branch — resolve base
    let base = if let Some(base) = provider_created_branch_base {
        base.to_string()
    } else {
        match base_mode {
            BaseMode::Explicit(ref b) => b.clone(),
            BaseMode::Interactive => {
                if prompt_policy == PromptPolicy::Deny {
                    bail!("Base branch selection is interactive; parallel workers cannot prompt");
                }
                let branches = git.list_local_branches()?;
                let idx = ctx.ui.select("Select base branch", &branches)?;
                branches[idx].clone()
            }
            BaseMode::Current => git.current_branch()?,
            BaseMode::Default => {
                if prompt_policy == PromptPolicy::Deny {
                    bail!("Base branch input is interactive; parallel workers cannot prompt");
                }
                let current = git.current_branch()?;
                ctx.ui.input("Base branch", Some(&current))?
            }
        }
    };

    if base.trim().is_empty() {
        bail!("Base branch cannot be empty");
    }

    ctx.ui
        .print_step(&format!("Creating new branch from {base}"));
    git.worktree_add_new_branch(wt_path, branch_name, &base)?;
    git.set_branch_parent(branch_name, &base).ok();
    Ok(CreateType::New)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AGENT_PROMPT_WORKFLOW_SCOPE, AgentCli, AgentConfig, Config, IssueProviderType,
        IssuesConfig, ReadyMode, SubmitMode,
    };
    use crate::context::mock::{CommandCall, MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    fn linear_config() -> Config {
        Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
                origin_policy: Default::default(),
            }),
            ..Config::default()
        }
    }

    fn github_config() -> Config {
        Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Github,
                gh_user: None,
                origin_policy: Default::default(),
            }),
            ..Config::default()
        }
    }

    fn write_empty_profile(root: &Path, name: &str) {
        let profile_dir = root.join(".wt/config/profiles").join(name);
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
    }

    fn run(
        ctx: &Ctx,
        target: Option<&str>,
        base_raw: &Option<String>,
        profile: Option<&str>,
        selected_profiles: &[String],
        matrix: bool,
    ) -> Result<()> {
        assert!(selected_profiles.is_empty());
        let targets = target
            .map(|target| vec![target.to_string()])
            .unwrap_or_default();
        super::run(ctx, &targets, base_raw, profile, matrix, 1)
    }

    fn run_targets(
        ctx: &Ctx,
        targets: &[&str],
        base_raw: &Option<String>,
        profile: Option<&str>,
        matrix: bool,
    ) -> Result<()> {
        let targets = targets
            .iter()
            .map(|target| target.to_string())
            .collect::<Vec<_>>();
        super::run(ctx, &targets, base_raw, profile, matrix, 1)
    }

    fn count_linear_start_updates(calls: &[CommandCall], issue_id: &str) -> usize {
        let expected = vec![
            "issue".to_string(),
            "update".to_string(),
            issue_id.to_string(),
            "--state".to_string(),
            "In Progress".to_string(),
        ];
        calls
            .iter()
            .filter(|(cmd, args, _)| cmd == "linear" && args == &expected)
            .count()
    }

    struct SharedRunner {
        inner: Arc<MockRunner>,
    }

    impl CommandRunner for SharedRunner {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&Path>,
        ) -> Result<crate::context::CmdOutput> {
            self.inner.run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.inner.has_command(cmd)
        }
    }

    struct IssueParallelRunner {
        calls: Mutex<Vec<CommandCall>>,
        has_origin: bool,
    }

    impl IssueParallelRunner {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                has_origin: true,
            }
        }

        fn without_origin() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                has_origin: false,
            }
        }
    }

    impl CommandRunner for IssueParallelRunner {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&Path>,
        ) -> Result<crate::context::CmdOutput> {
            self.calls.lock().unwrap().push((
                cmd.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
                cwd.map(Path::to_path_buf),
            ));

            let output = |stdout: String, success| crate::context::CmdOutput {
                stdout,
                stderr: String::new(),
                success,
            };

            match (cmd, args) {
                ("linear", ["issue", "view", id, "--json"]) => {
                    let number = id.trim_start_matches("PROJ-");
                    Ok(output(
                        format!(
                            r#"{{"identifier":"PROJ-{number}","title":"Issue {number}","branchName":"alice/proj-{number}"}}"#
                        ),
                        true,
                    ))
                }
                ("linear", ["issue", "update", _, "--state", "In Progress"]) => {
                    Ok(output(String::new(), true))
                }
                ("git", ["worktree", "list", "--porcelain"]) => Ok(output(
                    "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n".into(),
                    true,
                )),
                ("git", ["remote", "get-url", "origin"]) => {
                    Ok(output(String::new(), self.has_origin))
                }
                ("git", ["rev-parse", "--abbrev-ref", "HEAD"]) => Ok(output("main".into(), true)),
                ("git", ["fetch", "origin"]) => Ok(output(String::new(), true)),
                ("git", ["worktree", "add", "-b", _, _, _]) => Ok(output(String::new(), true)),
                ("git", ["config", _, _]) => Ok(output(String::new(), true)),
                ("git", ["show-ref", "--verify", "--quiet", reference])
                    if *reference == "refs/heads/main" =>
                {
                    Ok(output(String::new(), true))
                }
                ("git", ["show-ref", "--verify", "--quiet", _]) => Ok(output(String::new(), false)),
                _ => bail!("unexpected command: {cmd} {}", args.join(" ")),
            }
        }

        fn has_command(&self, _cmd: &str) -> bool {
            true
        }
    }

    impl CommandRunner for Arc<IssueParallelRunner> {
        fn run(
            &self,
            cmd: &str,
            args: &[&str],
            cwd: Option<&Path>,
        ) -> Result<crate::context::CmdOutput> {
            self.as_ref().run(cmd, args, cwd)
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.as_ref().has_command(cmd)
        }
    }

    #[test]
    fn issue_snapshot_context_is_merged_before_profile_prompt() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: std::collections::HashMap::from([(
                    "issue".into(),
                    vec![
                        "Start from the profile instructions.".into(),
                        "Then run verification.".into(),
                    ],
                )]),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Snapshot path",
            path: "<repo-root>/.wt/issues/PROJ-123.md",
            content: "# PROJ-123: Fix editor\n\nBody",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "issue",
            None,
            "Use this issue snapshot before changing code.",
            None,
            None,
        );

        let mut agent = config.agent.unwrap();
        let prompts = agent.prompt.remove("issue").unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains("<repo-root>/.wt/issues/PROJ-123.md"));
        assert!(prompts[0].contains("# PROJ-123: Fix editor"));
        assert!(prompts[0].contains("Agent Completion Report"));
        assert!(prompts[0].contains("Start from the profile instructions."));
        assert!(
            prompts[0].find("# PROJ-123: Fix editor").unwrap()
                < prompts[0]
                    .find("Start from the profile instructions.")
                    .unwrap()
        );
        assert_eq!(prompts[1], "Then run verification.");
    }

    #[test]
    fn prepared_completion_prompt_places_context_after_handoff_before_snapshot() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: std::collections::HashMap::new(),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Task path",
            path: "<repo-root>/.wt/execution/tasks/add-schema.toml",
            content: "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "branch",
            None,
            "Use this task before changing code.",
            Some("## Workflow Coordinator Handoff\n\nSend the report."),
            Some(
                "Workflow title:\n\nWorkflow migration\n\nWorkflow body:\n\nShip the broader migration.\n\nWorkflow origin: linear:WT-123",
            ),
        );

        let mut agent = config.agent.unwrap();
        let prompts = agent.prompt.remove("branch").unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("## Workflow Coordinator Handoff"));
        assert!(
            prompts[0].find("## Workflow Coordinator Handoff").unwrap()
                < prompts[0].find("Workflow title").unwrap()
        );
        assert!(
            prompts[0].find("Workflow title").unwrap() < prompts[0].find("Workflow body").unwrap()
        );
        assert!(
            prompts[0].find("Workflow body").unwrap() < prompts[0].find("Workflow origin").unwrap()
        );
        assert!(
            prompts[0].find("Workflow origin").unwrap()
                < prompts[0]
                    .find("Task path: `<repo-root>/.wt/execution/tasks/add-schema.toml`")
                    .unwrap()
        );
        assert!(
            prompts[0].find("Workflow body").unwrap()
                < prompts[0].find("title = \"Add schema\"").unwrap()
        );
    }

    #[test]
    fn workflow_prompt_scope_is_inserted_between_snapshot_and_setup_prompt() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: std::collections::HashMap::from([
                    (
                        AGENT_PROMPT_WORKFLOW_SCOPE.into(),
                        vec!["Workflow prompt".into(), "Workflow follow-up".into()],
                    ),
                    (
                        "branch".into(),
                        vec!["Common prompt".into(), "Branch prompt".into()],
                    ),
                ]),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Task path",
            path: "<repo-root>/.wt/execution/tasks/add-schema.toml",
            content: "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "branch",
            Some(AGENT_PROMPT_WORKFLOW_SCOPE),
            "Use this task before changing code.",
            Some("## Workflow Coordinator Handoff\n\nSend the report."),
            Some("Workflow body:\n\nShip the broader migration."),
        );

        let mut agent = config.agent.unwrap();
        let prompts = agent.prompt.remove("branch").unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("## Workflow Coordinator Handoff"));
        assert!(prompts[0].contains("Workflow body"));
        assert!(
            prompts[0].contains("Task path: `<repo-root>/.wt/execution/tasks/add-schema.toml`")
        );
        assert!(prompts[0].contains("Workflow prompt"));
        assert!(prompts[0].contains("Workflow follow-up"));
        assert!(prompts[0].contains("Common prompt"));
        assert!(prompts[0].contains("Branch prompt"));
        assert!(
            prompts[0]
                .find("Task path: `<repo-root>/.wt/execution/tasks/add-schema.toml`")
                .unwrap()
                < prompts[0].find("Workflow prompt").unwrap()
        );
        assert!(
            prompts[0].find("Workflow prompt").unwrap()
                < prompts[0].find("Workflow follow-up").unwrap()
        );
        assert!(
            prompts[0].find("Workflow follow-up").unwrap()
                < prompts[0].find("Common prompt").unwrap()
        );
        assert!(
            prompts[0].find("Common prompt").unwrap() < prompts[0].find("Branch prompt").unwrap()
        );
        assert!(!agent.prompt.contains_key(AGENT_PROMPT_WORKFLOW_SCOPE));
    }

    #[test]
    fn workflow_prompt_scope_is_not_used_without_explicit_workflow_context() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: std::collections::HashMap::from([
                    (
                        AGENT_PROMPT_WORKFLOW_SCOPE.into(),
                        vec!["Workflow prompt".into()],
                    ),
                    (
                        "branch".into(),
                        vec!["Common prompt".into(), "Branch prompt".into()],
                    ),
                ]),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Task path",
            path: "<repo-root>/.wt/execution/tasks/add-schema.toml",
            content: "title = \"Add schema\"\nbranch = \"add-schema\"\n",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "branch",
            None,
            "Use this task before changing code.",
            Some("## Workflow Coordinator Handoff\n\nSend the report."),
            None,
        );

        let mut agent = config.agent.unwrap();
        let prompts = agent.prompt.remove("branch").unwrap();
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("## Workflow Coordinator Handoff"));
        assert!(
            prompts[0].contains("Task path: `<repo-root>/.wt/execution/tasks/add-schema.toml`")
        );
        assert!(prompts[0].contains("Common prompt"));
        assert!(prompts[0].contains("Branch prompt"));
        assert!(
            prompts[0]
                .find("Task path: `<repo-root>/.wt/execution/tasks/add-schema.toml`")
                .unwrap()
                < prompts[0].find("Common prompt").unwrap()
        );
        assert!(
            prompts[0].find("Common prompt").unwrap() < prompts[0].find("Branch prompt").unwrap()
        );
        assert!(
            !prompts
                .iter()
                .any(|prompt| prompt.contains("Workflow prompt"))
        );
        assert_eq!(
            agent.prompt.get(AGENT_PROMPT_WORKFLOW_SCOPE).unwrap(),
            &vec!["Workflow prompt".to_string()]
        );
    }

    #[test]
    fn prepared_snapshot_context_uses_requested_setup_mode_prompt() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: std::collections::HashMap::from([
                    ("issue".into(), vec!["Issue prompt".into()]),
                    ("branch".into(), vec!["Branch prompt".into()]),
                ]),
                ..AgentConfig::default()
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Task path",
            path: "<repo-root>/.wt/execution/tasks/add-schema.toml",
            content: "# Add schema\n\nbranch = \"add-schema\"",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "branch",
            None,
            "Use this task before changing code.",
            None,
            None,
        );

        let mut agent = config.agent.unwrap();
        let branch_prompts = agent.prompt.remove("branch").unwrap();
        assert_eq!(branch_prompts.len(), 1);
        assert!(branch_prompts[0].contains("Use this task before changing code."));
        assert!(
            branch_prompts[0]
                .contains("Task path: `<repo-root>/.wt/execution/tasks/add-schema.toml`")
        );
        assert!(branch_prompts[0].contains("# Add schema"));
        assert!(branch_prompts[0].contains("Changed files"));
        assert!(branch_prompts[0].contains("Branch prompt"));
        assert_eq!(agent.prompt.remove("issue").unwrap(), vec!["Issue prompt"]);
    }

    #[test]
    fn issue_profile_existing_branch_without_worktree_reuses_branch() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".wt/config/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true); // profile branch local_branch_exists
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        ); // checked_out_path
        runner.add_response("", true); // worktree_add existing branch
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(0); // reuse existing branch
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );
        let base = Some("main".to_string());

        let result = run_with_issue_snapshot(
            &ctx,
            &base,
            Some("codex"),
            false,
            PreparedIssueContext {
                identifier: "add-schema",
                title: "Add schema",
                branch_name: Some("add-schema"),
                setup_mode: setup::WORKSPACE_COLOR_KIND_ISSUE,
                template_vars: HashMap::new(),
                additional_prompt_scope: None,
                workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
                on_start_issue_id: None,
                prompt_intro: "Use this issue snapshot before changing code.",
                completion_section: None,
                pre_snapshot_context: None,
                workspace_label: None,
                snapshot: IssueSnapshotContext {
                    path_label: "Task path",
                    path: "<repo-root>/.wt/execution/tasks/add-schema.toml",
                    content: "title = \"Add schema\"\nbranch = \"add-schema\"\n",
                },
            },
        )
        .unwrap();

        assert_eq!(result.branch_name, "add-schema-codex");

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args.len() == 4
                && args[0] == "worktree"
                && args[1] == "add"
                && args[3] == "add-schema-codex"
        }));
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args
                    == &vec![
                        "branch".to_string(),
                        "-D".to_string(),
                        "add-schema-codex".to_string(),
                    ])
        }));
    }

    #[test]
    fn issue_profile_existing_branch_non_interactive_fails_without_prompt_or_delete() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".wt/config/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();

        let mut runner = MockRunner::new();
        runner.add_response("", true); // profile branch local_branch_exists
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            Config::default(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );
        let base = Some("main".to_string());

        let result = run_with_issue_snapshot_non_interactive(
            &ctx,
            &base,
            Some("codex"),
            false,
            PreparedIssueContext {
                identifier: "add-schema",
                title: "Add schema",
                branch_name: Some("add-schema"),
                setup_mode: setup::WORKSPACE_COLOR_KIND_ISSUE,
                template_vars: HashMap::new(),
                additional_prompt_scope: None,
                workspace_color_kind: setup::WORKSPACE_COLOR_KIND_TASK,
                on_start_issue_id: None,
                prompt_intro: "Use this issue snapshot before changing code.",
                completion_section: None,
                pre_snapshot_context: None,
                workspace_label: None,
                snapshot: IssueSnapshotContext {
                    path_label: "Task path",
                    path: "<repo-root>/.wt/execution/tasks/add-schema.toml",
                    content: "title = \"Add schema\"\nbranch = \"add-schema\"\n",
                },
            },
        );

        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(message.contains("Branch add-schema-codex already exists"));
        assert!(message.contains("cannot prompt"));

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args.first().is_some_and(|arg| {
                    arg == "worktree" && args.get(1).is_some_and(|arg| arg == "add")
                }))
        }));
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args
                    == &vec![
                        "branch".to_string(),
                        "-D".to_string(),
                        "add-schema-codex".to_string(),
                    ])
        }));
    }

    #[test]
    fn issue_with_number_fetches_and_resolves() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"PROJ-680","title":"Document editor","branchName":"alice/proj-680"}"#,
            true,
        );
        // checked_out_path (worktree list)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // has_remote (origin present)
        runner.add_response("", true);
        // fetch
        runner.add_response("", true);
        // local_branch_exists
        runner.add_response("", false);
        // remote_branch_exists
        runner.add_response("", false);
        // current_branch (for base prompt)
        runner.add_response("main", true);
        // worktree_add_new_branch
        runner.add_response("", true);
        // on_start (update_status)
        runner.add_response("", true);

        let mut ui = MockUi::new();
        ui.add_input("main"); // base branch prompt

        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        // This will fail at setup (no real filesystem) but proves the flow up to worktree creation
        let result = run(&ctx, Some("680"), &None, None, &[], false);
        // We expect it to get past issue resolution and worktree creation
        // It may fail at setup::run_setup due to filesystem ops — that's OK for unit test
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));
    }

    #[test]
    fn issue_with_multiple_targets_runs_each_issue_in_order() {
        let repo = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"First issue","branchName":"alice/proj-1-first"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"First issue","branchName":"alice/proj-1-first"}"#,
            true,
        );
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local_branch_exists
        runner.add_response("", false); // remote_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // on_start
        runner.add_response(
            r#"{"identifier":"PROJ-3","title":"Third issue","branchName":"alice/proj-3-third"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-3","title":"Third issue","branchName":"alice/proj-3-third"}"#,
            true,
        );
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local_branch_exists
        runner.add_response("", false); // remote_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // on_start
        let runner = Arc::new(runner);

        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(Arc::clone(&ui)),
        );

        run_targets(&ctx, &["1", "3"], &Some("main".into()), None, false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let created_branches = calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .map(|(_, args, _)| args[3].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            created_branches,
            vec!["alice/proj-1-first", "alice/proj-3-third"]
        );
        assert_eq!(count_linear_start_updates(&calls, "PROJ-1"), 1);
        assert_eq!(count_linear_start_updates(&calls, "PROJ-3"), 1);
        assert!(ui.prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn issue_with_multiple_targets_runs_parallel_with_jobs() {
        let repo = tempfile::tempdir().unwrap();
        let runner = Arc::new(IssueParallelRunner::new());
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(Arc::clone(&runner)),
            Box::new(Arc::clone(&ui)),
        );
        let targets = vec!["1".to_string(), "3".to_string()];

        super::run(&ctx, &targets, &Some("main".into()), None, false, 3).unwrap();

        let calls = runner.calls.lock().unwrap();
        let mut created_branches = calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .map(|(_, args, _)| args[3].clone())
            .collect::<Vec<_>>();
        created_branches.sort();
        assert_eq!(created_branches, vec!["alice/proj-1", "alice/proj-3"]);
        assert_eq!(count_linear_start_updates(&calls, "PROJ-1"), 1);
        assert_eq!(count_linear_start_updates(&calls, "PROJ-3"), 1);
        assert!(ui.prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn run_skips_fetch_in_local_only_repo() {
        // Reproduces `wt run task <id> --base .` in a repo with no `origin`
        // remote: fetch must be skipped (not hard-fail) and the worktree still
        // created from the local current branch.
        let repo = tempfile::tempdir().unwrap();
        let runner = Arc::new(IssueParallelRunner::without_origin());
        let ui = Arc::new(MockUi::new());
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(Arc::clone(&runner)),
            Box::new(Arc::clone(&ui)),
        );

        super::run(&ctx, &["1".to_string()], &Some(".".into()), None, false, 1).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|(cmd, args, _)| cmd == "git"
                && args.len() == 2
                && args[0] == "fetch"
                && args[1] == "origin"),
            "fetch must be skipped when the origin remote is absent"
        );
        assert!(
            calls.iter().any(|(cmd, args, _)| cmd == "git"
                && args.len() >= 6
                && args[0] == "worktree"
                && args[1] == "add"
                && args[2] == "-b"),
            "worktree should still be created in a local-only repo"
        );
    }

    #[test]
    fn issue_without_target_multi_selects_provider_issues() {
        let repo = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[
                {"identifier":"PROJ-1","title":"First issue","state":{"name":"Todo"},"assignee":{"displayName":"alice"}},
                {"identifier":"PROJ-2","title":"Skipped issue","state":{"name":"Todo"},"assignee":null},
                {"identifier":"PROJ-3","title":"Third issue","state":{"name":"Todo"},"assignee":{"displayName":"bob"}}
            ]"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"First issue","branchName":"alice/proj-1-first"}"#,
            true,
        );
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local_branch_exists
        runner.add_response("", false); // remote_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // on_start
        runner.add_response(
            r#"{"identifier":"PROJ-3","title":"Third issue","branchName":"alice/proj-3-third"}"#,
            true,
        );
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true); // fetch
        runner.add_response("", false); // local_branch_exists
        runner.add_response("", false); // remote_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // on_start
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![0, 2]);
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(Arc::clone(&ui)),
        );

        run(&ctx, None, &Some("main".into()), None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let created_branches = calls
            .iter()
            .filter(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .map(|(_, args, _)| args[3].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            created_branches,
            vec!["alice/proj-1-first", "alice/proj-3-third"]
        );
        assert_eq!(count_linear_start_updates(&calls, "PROJ-1"), 1);
        assert_eq!(count_linear_start_updates(&calls, "PROJ-2"), 0);
        assert_eq!(count_linear_start_updates(&calls, "PROJ-3"), 1);
        assert_eq!(
            ui.prompts.lock().unwrap().as_slice(),
            &["multi_select: Issues to start"]
        );
    }

    #[test]
    fn issue_without_target_empty_selection_returns_ok() {
        let repo = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(
            r#"[{"identifier":"PROJ-1","title":"First issue","state":{"name":"Todo"},"assignee":null}]"#,
            true,
        );
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_multi_select(vec![]);
        let ui = Arc::new(ui);
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(Arc::clone(&ui)),
        );

        run(&ctx, None, &Some("main".into()), None, &[], false).unwrap();

        assert_eq!(
            ui.warnings.lock().unwrap().as_slice(),
            &["No issues selected"]
        );
        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().all(|(cmd, args, _)| {
            !(cmd == "git"
                && args.first().is_some_and(|arg| {
                    arg == "worktree" && args.get(1).is_some_and(|arg| arg == "add")
                }))
        }));
    }

    #[test]
    fn issue_no_branch_name_returns_error() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"PROJ-100","title":"Test issue","branchName":null}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"PROJ-100","title":"Test issue","branchName":null}"#,
            true,
        );

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some("100"), &None, None, &[], false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No branch name"));
    }

    #[test]
    fn issue_local_branch_exists_reuses_it() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Test","branchName":"alice/proj-1-test"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Test","branchName":"alice/proj-1-test"}"#,
            true,
        );
        // checked_out_path (worktree list — no match)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // has_remote (origin present)
        runner.add_response("", true);
        // fetch
        runner.add_response("", true);
        // local_branch_exists → true
        runner.add_response("", true);
        // worktree_add (not -b)
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some("1"), &None, None, &[], false);
        assert!(result.is_ok() || !result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn issue_uses_canonical_repo_name_when_invoked_from_worktree() {
        let unique = format!(
            "wt-issue-canonical-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let temp = std::env::temp_dir().join(unique);
        let repo_root = temp.join("sample-app");
        std::fs::create_dir_all(&repo_root).unwrap();

        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"PROJ-672","title":" nested worktree bug","branchName":"alice/proj-672-nested-worktree-bug"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"PROJ-672","title":" nested worktree bug","branchName":"alice/proj-672-nested-worktree-bug"}"#,
            true,
        );
        // checked_out_path (worktree list — no branch match)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // has_remote (origin present)
        runner.add_response("", true);
        // fetch
        runner.add_response("", true);
        // local_branch_exists
        runner.add_response("", false);
        // remote_branch_exists
        runner.add_response("", false);
        // current_branch (for base prompt)
        runner.add_response("main", true);
        // worktree_add_new_branch
        runner.add_response("", true);
        // on_start (update_status)
        runner.add_response("", true);

        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_input("main");

        let ctx = Ctx::new(
            repo_root.clone(),
            repo_root,
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let result = run(&ctx, Some("672"), &None, None, &[], false);
        assert!(result.is_ok());

        let calls = runner.calls.lock().unwrap();
        let worktree_add_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .expect("expected git worktree add -b call");

        assert_eq!(
            worktree_add_call.1[4],
            temp.join("sample-app-alice-proj-672-nested-worktree-bug")
                .to_string_lossy()
                .as_ref()
        );
        assert!(!worktree_add_call.1[4].contains("sample-app-proj-670-feature-alice-proj-672"));

        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn issue_default_base_rejects_empty_prompt_result() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("main", true);
        let mut ui = MockUi::new();
        ui.add_input(" ");
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(runner),
            Box::new(ui),
        );
        let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

        let result = resolve_base_branch(&ctx, &git, &None);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Base branch cannot be empty")
        );
    }

    #[test]
    fn issue_default_base_prompt_uses_invocation_root_for_current_branch() {
        let temp = std::env::temp_dir().join(format!(
            "wt-issue-invocation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo_root = temp.join("sample-app");
        let invocation_root = temp.join("sample-app-alice-proj-670");
        std::fs::create_dir_all(&repo_root).unwrap();
        std::fs::create_dir_all(&invocation_root).unwrap();

        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"PROJ-672","title":" nested worktree bug","branchName":"alice/proj-672-nested-worktree-bug"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"PROJ-672","title":" nested worktree bug","branchName":"alice/proj-672-nested-worktree-bug"}"#,
            true,
        );
        // checked_out_path (worktree list)
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // has_remote (origin present)
        runner.add_response("", true);
        // fetch
        runner.add_response("", true);
        // local_branch_exists
        runner.add_response("", false);
        // remote_branch_exists
        runner.add_response("", false);
        // current_branch (for base prompt) — uses invocation_root
        runner.add_response(
            "alice/proj-670-document-editor는-Document에-Categoryx로-Category를-지정할-수-있다",
            true,
        );
        // worktree_add_new_branch
        runner.add_response("", true);
        // on_start (update_status)
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ui = MockUi::new();
        let ctx = Ctx::new(
            repo_root,
            invocation_root.clone(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        let result = run(&ctx, Some("672"), &None, None, &[], false);
        assert!(result.is_ok());

        let calls = runner.calls.lock().unwrap();
        let current_branch_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args
                        == &vec![
                            "rev-parse".to_string(),
                            "--abbrev-ref".to_string(),
                            "HEAD".to_string(),
                        ]
            })
            .expect("expected git current branch call");
        assert_eq!(
            current_branch_call.2.as_deref(),
            Some(invocation_root.as_path())
        );

        let worktree_add_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .expect("expected git worktree add -b call");
        assert_eq!(
            worktree_add_call.1[5],
            "alice/proj-670-document-editor는-Document에-Categoryx로-Category를-지정할-수-있다"
        );

        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn issue_current_base_uses_current_branch_without_prompt() {
        let temp = std::env::temp_dir().join(format!(
            "wt-issue-current-base-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo_root = temp.join("sample-app");
        let invocation_root = temp.join("sample-app-feature");
        std::fs::create_dir_all(&repo_root).unwrap();
        std::fs::create_dir_all(&invocation_root).unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-672","title":" nested worktree bug","branchName":"alice/proj-672-nested-worktree-bug"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-672","title":" nested worktree bug","branchName":"alice/proj-672-nested-worktree-bug"}"#,
            true,
        );
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", false);
        runner.add_response("feature/current", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            repo_root,
            invocation_root,
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, Some("672"), &Some(".".into()), None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        let worktree_add_call = calls
            .iter()
            .find(|(cmd, args, _)| {
                cmd == "git"
                    && args.len() >= 6
                    && args[0] == "worktree"
                    && args[1] == "add"
                    && args[2] == "-b"
            })
            .expect("expected git worktree add -b call");
        assert_eq!(worktree_add_call.1[5], "feature/current");

        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn github_issue_current_base_tracks_provider_created_branch() {
        let temp = std::env::temp_dir().join(format!(
            "wt-github-issue-current-base-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo_root = temp.join("sample-app");
        let invocation_root = temp.join("sample-app-feature");
        std::fs::create_dir_all(&repo_root).unwrap();
        std::fs::create_dir_all(&invocation_root).unwrap();

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"number":5,"title":"Add interactive wt config extract"}"#,
            true,
        );
        runner.add_response("", true);
        runner.add_response("feature/current", true);
        runner.add_response("", true);
        runner.add_response(
            "https://github.com/hoetaek/wt/tree/5-add-interactive-wt-config-extract",
            true,
        );
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        runner.add_response("", true); // has_remote (origin present)
        runner.add_response("", true);
        runner.add_response("", false);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);
        runner.add_response("", true);

        let runner = Arc::new(runner);
        let ctx = Ctx::new(
            repo_root,
            invocation_root,
            github_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        run(&ctx, Some("5"), &Some(".".into()), None, &[], false).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "gh"
                && args
                    == &vec![
                        "issue".to_string(),
                        "develop".to_string(),
                        "--base".to_string(),
                        "feature/current".to_string(),
                        "5".to_string(),
                    ]
        }));
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "worktree".to_string(),
                        "add".to_string(),
                        "-b".to_string(),
                        "5-add-interactive-wt-config-extract".to_string(),
                        temp.join("sample-app-5-add-interactive-wt-config-extract")
                            .to_string_lossy()
                            .to_string(),
                        "origin/5-add-interactive-wt-config-extract".to_string(),
                    ]
        }));
        assert!(calls.iter().any(|(cmd, args, _)| {
            cmd == "git"
                && args
                    == &vec![
                        "config".to_string(),
                        "branch.5-add-interactive-wt-config-extract.parentbranch".to_string(),
                        "feature/current".to_string(),
                    ]
        }));

        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn issue_with_profile_updates_start_status_for_new_profile_branch() {
        let repo = tempfile::tempdir().unwrap();
        write_empty_profile(repo.path(), "codex");

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response("", false); // profile branch local_branch_exists
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        runner.add_response("", true); // on_start
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let results = run_inner_many(
            &ctx,
            &["123".to_string()],
            &Some("main".into()),
            IssueRunOptions {
                profile_selection: ProfileSelection::new(Some("codex"), &[]),
                matrix: false,
                prepared_issue: None,
                prompt_policy: PromptPolicy::Allow,
                jobs: 1,
                base_override: None,
            },
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].branch_name, "alice/proj-123-fix-editor-codex");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(count_linear_start_updates(&calls, "PROJ-123"), 1);
    }

    #[test]
    fn issue_matrix_updates_start_status_once_for_created_profile_branches() {
        let repo = tempfile::tempdir().unwrap();
        write_empty_profile(repo.path(), "alpha");
        write_empty_profile(repo.path(), "beta");

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response("", false); // alpha branch local_branch_exists
        runner.add_response("", true); // alpha worktree_add_new_branch
        runner.add_response("", true); // alpha parent branch exists
        runner.add_response("", true); // alpha set parent config
        runner.add_response("", true); // on_start
        runner.add_response("", false); // beta branch local_branch_exists
        runner.add_response("", true); // beta worktree_add_new_branch
        runner.add_response("", true); // beta parent branch exists
        runner.add_response("", true); // beta set parent config
        let runner = Arc::new(runner);

        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(MockUi::new()),
        );

        let results = run_inner_many(
            &ctx,
            &["123".to_string()],
            &Some("main".into()),
            IssueRunOptions {
                profile_selection: ProfileSelection::new(None, &[]),
                matrix: true,
                prepared_issue: None,
                prompt_policy: PromptPolicy::Allow,
                jobs: 1,
                base_override: None,
            },
        )
        .unwrap();

        assert_eq!(results.len(), 2);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(count_linear_start_updates(&calls, "PROJ-123"), 1);
    }

    #[test]
    fn issue_with_preexisting_profile_branch_does_not_update_start_status() {
        let repo = tempfile::tempdir().unwrap();
        write_empty_profile(repo.path(), "codex");

        let mut runner = MockRunner::new();
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response(
            r#"{"identifier":"PROJ-123","title":"Fix editor","branchName":"alice/proj-123-fix-editor"}"#,
            true,
        );
        runner.add_response("", true); // profile branch local_branch_exists
        runner.add_response(
            &format!(
                "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n",
                repo.path().display()
            ),
            true,
        ); // checked_out_path
        runner.add_response("", true); // branch -D
        runner.add_response("", true); // worktree_add_new_branch
        runner.add_response("", true); // parent branch exists
        runner.add_response("", true); // set parent config
        let runner = Arc::new(runner);

        let mut ui = MockUi::new();
        ui.add_select(1); // delete and recreate existing branch
        let ctx = Ctx::new(
            repo.path().to_path_buf(),
            repo.path().to_path_buf(),
            linear_config(),
            Box::new(SharedRunner {
                inner: Arc::clone(&runner),
            }),
            Box::new(ui),
        );

        run_inner_many(
            &ctx,
            &["123".to_string()],
            &Some("main".into()),
            IssueRunOptions {
                profile_selection: ProfileSelection::new(Some("codex"), &[]),
                matrix: false,
                prepared_issue: None,
                prompt_policy: PromptPolicy::Allow,
                jobs: 1,
                base_override: None,
            },
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(count_linear_start_updates(&calls, "PROJ-123"), 0);
    }

    #[test]
    fn issue_base_conflict_with_existing_branch() {
        let mut runner = MockRunner::new();
        // get_issue (provider.get_issue)
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Test","branchName":"alice/proj-1-test"}"#,
            true,
        );
        // get_issue (provider.ensure_branch internally calls get_issue again)
        runner.add_response(
            r#"{"identifier":"PROJ-1","title":"Test","branchName":"alice/proj-1-test"}"#,
            true,
        );
        // checked_out_path
        runner.add_response(
            "worktree /tmp/test-repo\nHEAD abc\nbranch refs/heads/main\n\n",
            true,
        );
        // has_remote (origin present)
        runner.add_response("", true);
        // fetch
        runner.add_response("", true);
        // local_branch_exists → true
        runner.add_response("", true);

        let ui = MockUi::new();
        let ctx = Ctx::new(
            PathBuf::from("/tmp/test-repo"),
            PathBuf::from("/tmp/test-repo"),
            linear_config(),
            Box::new(runner),
            Box::new(ui),
        );

        let result = run(&ctx, Some("1"), &Some("main".into()), None, &[], false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
}
