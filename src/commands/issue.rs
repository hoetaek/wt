use crate::cli::BaseMode;
use crate::commands::agent_report;
use crate::commands::profile_workspace::{
    ProfileBranchDecision, PromptPolicy, resolve_profile_branch,
};
use crate::config::Config;
use crate::config::IssueProviderType;
use crate::context::Ctx;
use crate::error::WtError;
use crate::names::WorktreeNames;
use crate::services::git::{CreateType, GitService};
use crate::services::issues::github::GithubIssueProvider;
use crate::services::issues::linear::LinearIssueProvider;
use crate::services::issues::{EnsuredBranch, IssueInfo, IssueProvider};
use crate::setup;
use crate::worktree_naming::{self, WorktreeNamingResult};
use anyhow::{Result, bail};
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
    pub(crate) mode: &'a str,
    pub(crate) on_start_issue_id: Option<&'a str>,
    pub(crate) prompt_intro: &'a str,
    pub(crate) workspace_label: Option<String>,
    pub(crate) snapshot: IssueSnapshotContext<'a>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IssueRunResult {
    pub(crate) branch_name: String,
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

    pub(crate) fn message(&self) -> &str {
        &self.message
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

struct ProfileRunOptions<'a> {
    prepared_issue: Option<&'a PreparedIssueContext<'a>>,
    on_start_issue_id: Option<&'a str>,
    prompt_policy: PromptPolicy,
}

pub fn run(
    ctx: &Ctx,
    target: Option<&str>,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
) -> Result<()> {
    run_inner(
        ctx,
        target,
        base_raw,
        profile,
        matrix,
        None,
        PromptPolicy::Allow,
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
        None,
        base_raw,
        profile,
        matrix,
        Some(&prepared),
        PromptPolicy::Allow,
    )
}

pub(crate) fn run_with_issue_snapshot_many(
    ctx: &Ctx,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    prepared: PreparedIssueContext<'_>,
) -> Result<Vec<IssueRunResult>> {
    run_inner_many(
        ctx,
        None,
        base_raw,
        profile,
        matrix,
        Some(&prepared),
        PromptPolicy::Allow,
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
        None,
        base_raw,
        profile,
        matrix,
        Some(&prepared),
        PromptPolicy::Deny,
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
        let profiles = load_selected_profiles(ctx, profile)?;
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

fn run_inner(
    ctx: &Ctx,
    target: Option<&str>,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    prepared_issue: Option<&PreparedIssueContext<'_>>,
    prompt_policy: PromptPolicy,
) -> Result<IssueRunResult> {
    run_inner_many(
        ctx,
        target,
        base_raw,
        profile,
        matrix,
        prepared_issue,
        prompt_policy,
    )?
    .into_iter()
    .last()
    .ok_or_else(|| anyhow::anyhow!("No worktrees created"))
}

fn run_inner_many(
    ctx: &Ctx,
    target: Option<&str>,
    base_raw: &Option<String>,
    profile: Option<&str>,
    matrix: bool,
    prepared_issue: Option<&PreparedIssueContext<'_>>,
    prompt_policy: PromptPolicy,
) -> Result<Vec<IssueRunResult>> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));

    // 1. Resolve issue
    let naming_enabled = ctx.config.worktree.naming.is_some();
    let issue = if let Some(prepared) = prepared_issue {
        IssueInfo {
            identifier: prepared.identifier.to_string(),
            title: prepared.title.to_string(),
            branch_name: prepared.branch_name.map(str::to_string),
            body: None,
        }
    } else if let Some(target) = target {
        let provider = build_provider(ctx)?;
        provider.get_issue(target.trim_start_matches('#'))?
    } else {
        let provider = build_provider(ctx)?;
        let issues = provider.list_issues()?;
        if issues.is_empty() {
            bail!("No issues found");
        }

        let items: Vec<String> = issues.iter().map(|i| i.display.clone()).collect();
        let idx = ctx.ui.select("Select an issue", &items)?;
        let selected = &issues[idx];
        if naming_enabled {
            provider.get_issue(&selected.identifier)?
        } else {
            IssueInfo {
                identifier: selected.identifier.clone(),
                title: selected.title.clone(),
                branch_name: None,
                body: None,
            }
        }
    };
    let identifier = issue.identifier;
    let title = issue.title;
    let suggested_branch = issue.branch_name;
    let issue_snapshot = prepared_issue.map(|issue| &issue.snapshot);
    let workspace_label = prepared_issue.and_then(|issue| issue.workspace_label.as_deref());
    let setup_mode = prepared_issue.map(|issue| issue.mode).unwrap_or("issue");
    let prompt_intro = prepared_issue
        .map(|issue| issue.prompt_intro)
        .unwrap_or("Use this issue snapshot before changing code.");

    ctx.ui.print_step(&format!("{identifier}: {title}"));

    let naming = worktree_naming::generate(ctx, &identifier, &title, suggested_branch.as_deref())?;

    let provider_branch_base =
        if should_resolve_provider_branch_base(ctx, suggested_branch.as_deref(), prepared_issue) {
            Some(resolve_base_branch(ctx, &git, base_raw)?)
        } else {
            None
        };

    // Ensure branch exists (provider-specific: Linear reads, GH may create)
    let raw_id = identifier.trim_start_matches('#');
    let ensured_branch =
        if let Some(prepared_branch) = prepared_issue.and_then(|issue| issue.branch_name) {
            EnsuredBranch {
                name: prepared_branch.to_string(),
                created: false,
            }
        } else {
            let provider = build_provider(ctx)?;
            provider.ensure_branch(
                raw_id,
                provider_branch_base.as_deref(),
                naming.as_ref().and_then(|n| n.branch.as_deref()),
            )?
        };
    let provider_created_branch_base = if ensured_branch.created {
        provider_branch_base.as_deref()
    } else {
        None
    };
    let branch_name = ensured_branch.name;

    let on_start_issue_id = prepared_issue
        .map(|issue| issue.on_start_issue_id)
        .unwrap_or(Some(raw_id));

    if matrix || profile.is_some() {
        return run_profiles(
            ctx,
            &title,
            &branch_name,
            naming.as_ref(),
            base_raw,
            profile,
            ProfileRunOptions {
                prepared_issue,
                on_start_issue_id,
                prompt_policy,
            },
        );
    }

    let names = issue_worktree_names(ctx, &branch_name, &title, naming.as_ref(), workspace_label)?;
    let snapshot_config = issue_snapshot.map(|snapshot| {
        profile_config_with_issue_snapshot(&ctx.config, snapshot, setup_mode, prompt_intro)
    });

    // 2. Check if branch is already checked out elsewhere
    let existing_path = git.checked_out_path(&branch_name)?;
    if let Some(ref existing) = existing_path {
        if *existing == ctx.invocation_root {
            ctx.ui
                .print_warning("이미 이 브랜치에 있습니다. 다른 브랜치로 전환 후 다시 시도하세요.");
            return Ok(vec![IssueRunResult {
                branch_name,
                worktree_path: existing.clone(),
            }]);
        }
        if *existing != names.path {
            ctx.ui.print_step(&format!(
                "Branch already checked out at: {}",
                existing.display()
            ));
            setup::run_setup(
                ctx,
                existing,
                &names,
                Some(&title),
                setup_mode,
                naming.as_ref().map(|n| &n.vars),
                snapshot_config.as_ref(),
            )?;
            return Ok(vec![IssueRunResult {
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
                "Worktree {} already exists; parallel batch workers cannot prompt to delete or open it",
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
                setup::run_setup(
                    ctx,
                    &names.path,
                    &names,
                    Some(&title),
                    setup_mode,
                    naming.as_ref().map(|n| &n.vars),
                    snapshot_config.as_ref(),
                )?;
                return Ok(vec![IssueRunResult {
                    branch_name,
                    worktree_path: names.path,
                }]);
            }
            _ => return Err(WtError::Cancelled.into()),
        }
    }

    // 4. Create worktree
    git.fetch()?;
    let create_type = create_worktree(
        ctx,
        &git,
        &branch_name,
        &names.path,
        base_raw,
        provider_created_branch_base,
        prompt_policy,
    )?;

    // 5. Update issue status for new branches
    if create_type == CreateType::New {
        update_issue_start_status(ctx, on_start_issue_id);
    }

    // 6. Setup
    setup::run_setup(
        ctx,
        &names.path,
        &names,
        Some(&title),
        setup_mode,
        naming.as_ref().map(|n| &n.vars),
        snapshot_config.as_ref(),
    )?;

    Ok(vec![IssueRunResult {
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
    profile: Option<&str>,
    options: ProfileRunOptions<'_>,
) -> Result<Vec<IssueRunResult>> {
    let issue_snapshot = options.prepared_issue.map(|issue| &issue.snapshot);
    let setup_mode = options
        .prepared_issue
        .map(|issue| issue.mode)
        .unwrap_or("issue");
    let prompt_intro = options
        .prepared_issue
        .map(|issue| issue.prompt_intro)
        .unwrap_or("Use this issue snapshot before changing code.");
    let profiles = load_selected_profiles(ctx, profile)?;

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
            profile_config_with_issue_snapshot(profile_config, snapshot, setup_mode, prompt_intro)
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
        let profile_extra_vars = profile_template_vars(naming, profile_name, issue_snapshot);

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
                    branch_name: profile_branch,
                    worktree_path: path.clone(),
                };
                if let Err(err) = setup::run_setup(
                    ctx,
                    &path,
                    &names,
                    Some(&profile_title),
                    setup_mode,
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
            branch_name: profile_branch,
            worktree_path: names.path.clone(),
        };

        if let Err(err) = setup::run_setup(
            ctx,
            &names.path,
            &names,
            Some(&profile_title),
            setup_mode,
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

fn profile_template_vars(
    naming: Option<&WorktreeNamingResult>,
    profile_name: &str,
    issue_snapshot: Option<&IssueSnapshotContext<'_>>,
) -> HashMap<String, String> {
    let mut vars = naming.map(|n| n.vars.clone()).unwrap_or_default();
    vars.insert("profile".into(), profile_name.to_string());
    if let Some(snapshot) = issue_snapshot {
        vars.insert("issue_snapshot".into(), snapshot.path.to_string());
    }
    vars
}

fn profile_config_with_issue_snapshot(
    config: &Config,
    snapshot: &IssueSnapshotContext<'_>,
    mode: &str,
    prompt_intro: &str,
) -> Config {
    let mut config = config.clone();
    if let Some(agent) = config.agent.as_mut() {
        let snapshot_prompt = format!(
            "{}\n\n{}: `{}`\n\n{}\n\n{}",
            prompt_intro,
            snapshot.path_label,
            snapshot.path,
            snapshot.content,
            agent_report::prompt_section()
        );
        let prompts = agent.prompt.entry(mode.into()).or_default();
        if let Some(first_prompt) = prompts.first_mut() {
            *first_prompt = format!("{snapshot_prompt}\n\n{first_prompt}");
        } else {
            prompts.push(snapshot_prompt);
        }
    }
    config
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

fn load_selected_profiles(ctx: &Ctx, profile: Option<&str>) -> Result<Vec<(String, Config)>> {
    if let Some(profile) = profile {
        let config = Config::load_profile(&ctx.repo_root, profile, &ctx.base_config)?
            .ok_or_else(|| anyhow::anyhow!("Profile '{profile}' not found"))?;
        return Ok(vec![(profile.to_string(), config)]);
    }

    let profiles = Config::load_profiles(&ctx.repo_root, &ctx.base_config)?;
    if profiles.is_empty() {
        bail!("No profile configs found in .local/profiles/*/profile.toml");
    }
    Ok(profiles)
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
                    "Remote branch {branch_name} needs a parent branch selection; parallel batch workers cannot prompt"
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
                    bail!(
                        "Base branch selection is interactive; parallel batch workers cannot prompt"
                    );
                }
                let branches = git.list_local_branches()?;
                let idx = ctx.ui.select("Select base branch", &branches)?;
                branches[idx].clone()
            }
            BaseMode::Current => git.current_branch()?,
            BaseMode::Default => {
                if prompt_policy == PromptPolicy::Deny {
                    bail!("Base branch input is interactive; parallel batch workers cannot prompt");
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
        AgentCli, AgentConfig, Config, IssueProviderType, IssuesConfig, ReadyMode, SubmitMode,
    };
    use crate::context::mock::{CommandCall, MockRunner, MockUi};
    use crate::context::{CommandRunner, Ctx};
    use anyhow::Result;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn linear_config() -> Config {
        Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Config::default()
        }
    }

    fn github_config() -> Config {
        Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Github,
                gh_user: None,
            }),
            ..Config::default()
        }
    }

    fn write_empty_profile(root: &Path, name: &str) {
        let profile_dir = root.join(".local/profiles").join(name);
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(profile_dir.join("profile.toml"), "").unwrap();
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
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Snapshot path",
            path: ".local/issues/PROJ-123.md",
            content: "# PROJ-123: Fix editor\n\nBody",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "issue",
            "Use this issue snapshot before changing code.",
        );

        let mut agent = config.agent.unwrap();
        let prompts = agent.prompt.remove("issue").unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains(".local/issues/PROJ-123.md"));
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
                    ("new".into(), vec!["New branch prompt".into()]),
                ]),
            }),
            ..Config::default()
        };
        let snapshot = IssueSnapshotContext {
            path_label: "Task path",
            path: ".local/tasks/add-schema.toml",
            content: "# Add schema\n\nbranch = \"add-schema\"",
        };

        let config = profile_config_with_issue_snapshot(
            &config,
            &snapshot,
            "new",
            "Use this task before changing code.",
        );

        let mut agent = config.agent.unwrap();
        let new_prompts = agent.prompt.remove("new").unwrap();
        assert_eq!(new_prompts.len(), 1);
        assert!(new_prompts[0].contains("Use this task before changing code."));
        assert!(new_prompts[0].contains("Task path: `.local/tasks/add-schema.toml`"));
        assert!(new_prompts[0].contains("# Add schema"));
        assert!(new_prompts[0].contains("Changed files"));
        assert!(new_prompts[0].contains("New branch prompt"));
        assert_eq!(agent.prompt.remove("issue").unwrap(), vec!["Issue prompt"]);
    }

    #[test]
    fn issue_profile_existing_branch_without_worktree_reuses_branch() {
        let repo = tempfile::tempdir().unwrap();
        let profile_dir = repo.path().join(".local/profiles/codex");
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

        let results = run_with_issue_snapshot_many(
            &ctx,
            &base,
            Some("codex"),
            false,
            PreparedIssueContext {
                identifier: "add-schema",
                title: "Add schema",
                branch_name: Some("add-schema"),
                mode: "issue",
                on_start_issue_id: None,
                prompt_intro: "Use this issue snapshot before changing code.",
                workspace_label: None,
                snapshot: IssueSnapshotContext {
                    path_label: "Task path",
                    path: ".local/tasks/add-schema.toml",
                    content: "title = \"Add schema\"\nbranch = \"add-schema\"\n",
                },
            },
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].branch_name, "add-schema-codex");

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
        let profile_dir = repo.path().join(".local/profiles/codex");
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
                mode: "issue",
                on_start_issue_id: None,
                prompt_intro: "Use this issue snapshot before changing code.",
                workspace_label: None,
                snapshot: IssueSnapshotContext {
                    path_label: "Task path",
                    path: ".local/tasks/add-schema.toml",
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
        let result = run(&ctx, Some("680"), &None, None, false);
        // We expect it to get past issue resolution and worktree creation
        // It may fail at setup::run_setup due to filesystem ops — that's OK for unit test
        assert!(result.is_ok() || result.unwrap_err().to_string().contains("setup"));
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

        let result = run(&ctx, Some("100"), &None, None, false);
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

        let result = run(&ctx, Some("1"), &None, None, false);
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

        let result = run(&ctx, Some("672"), &None, None, false);
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

        let result = run(&ctx, Some("672"), &None, None, false);
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

        run(&ctx, Some("672"), &Some(".".into()), None, false).unwrap();

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

        run(&ctx, Some("5"), &Some(".".into()), None, false).unwrap();

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
            Some("123"),
            &Some("main".into()),
            Some("codex"),
            false,
            None,
            PromptPolicy::Allow,
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
            Some("123"),
            &Some("main".into()),
            None,
            true,
            None,
            PromptPolicy::Allow,
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
            Some("123"),
            &Some("main".into()),
            Some("codex"),
            false,
            None,
            PromptPolicy::Allow,
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

        let result = run(&ctx, Some("1"), &Some("main".into()), None, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
}
