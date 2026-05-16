use crate::context::Ctx;
use crate::error::WtError;
use crate::services::git::GitService;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptPolicy {
    Allow,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProfileBranchDecision {
    CreateNew,
    ReuseExisting { path: PathBuf },
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfilePathDecision {
    CreateNew,
    Recreate,
    Skip,
}

pub(crate) fn resolve_profile_branch(
    ctx: &Ctx,
    git: &GitService<'_>,
    profile_name: &str,
    branch: &str,
    path: &Path,
    prompt_policy: PromptPolicy,
) -> Result<ProfileBranchDecision> {
    let path_decision = handle_existing_profile_path(ctx, git, profile_name, path, prompt_policy)?;
    if path_decision == ProfilePathDecision::Skip {
        return Ok(ProfileBranchDecision::Skip);
    }

    if !git.local_branch_exists(branch)? {
        return Ok(ProfileBranchDecision::CreateNew);
    }

    if path_decision == ProfilePathDecision::Recreate {
        ctx.ui.print_warning(&format!(
            "Branch {branch} already exists; deleting because recreate was selected."
        ));
        delete_profile_branch(ctx, git, branch)?;
        return Ok(ProfileBranchDecision::CreateNew);
    }

    handle_existing_profile_branch(ctx, git, profile_name, branch, path, prompt_policy)
}

fn handle_existing_profile_path(
    ctx: &Ctx,
    git: &GitService<'_>,
    profile_name: &str,
    path: &Path,
    prompt_policy: PromptPolicy,
) -> Result<ProfilePathDecision> {
    if !path.exists() {
        return Ok(ProfilePathDecision::CreateNew);
    }

    ctx.ui
        .print_warning(&format!("Worktree {} already exists.", path.display()));
    if prompt_policy == PromptPolicy::Deny {
        bail!(
            "Worktree {} already exists; parallel batch workers cannot prompt to delete, skip, or abort",
            path.display()
        );
    }

    let items = vec![
        "Delete and recreate".into(),
        "Skip".into(),
        "Abort all".into(),
    ];
    let choice = ctx
        .ui
        .select(&format!("[{profile_name}] Worktree already exists"), &items)?;
    match choice {
        0 => {
            ctx.ui.print_step("Removing existing worktree...");
            git.worktree_remove_force(path).ok();
            if path.exists() {
                std::fs::remove_dir_all(path)?;
            }
            Ok(ProfilePathDecision::Recreate)
        }
        1 => Ok(ProfilePathDecision::Skip),
        _ => Err(WtError::Cancelled.into()),
    }
}

fn handle_existing_profile_branch(
    ctx: &Ctx,
    git: &GitService<'_>,
    profile_name: &str,
    branch: &str,
    path: &Path,
    prompt_policy: PromptPolicy,
) -> Result<ProfileBranchDecision> {
    ctx.ui
        .print_warning(&format!("Branch {branch} already exists."));
    if prompt_policy == PromptPolicy::Deny {
        bail!(
            "Branch {branch} already exists; parallel batch workers cannot prompt to reuse, delete, skip, or abort"
        );
    }

    let checked_out_path = git.checked_out_path(branch)?;
    let reuse_label = if checked_out_path.is_some() {
        "Open existing"
    } else {
        "Reuse existing branch"
    };
    let items = vec![
        reuse_label.into(),
        "Delete and recreate".into(),
        "Skip".into(),
        "Abort all".into(),
    ];
    let choice = ctx
        .ui
        .select(&format!("[{profile_name}] Branch already exists"), &items)?;
    match choice {
        0 => reuse_profile_branch(ctx, git, branch, path, checked_out_path),
        1 => {
            delete_profile_branch(ctx, git, branch)?;
            Ok(ProfileBranchDecision::CreateNew)
        }
        2 => Ok(ProfileBranchDecision::Skip),
        _ => Err(WtError::Cancelled.into()),
    }
}

fn reuse_profile_branch(
    ctx: &Ctx,
    git: &GitService<'_>,
    branch: &str,
    path: &Path,
    checked_out_path: Option<PathBuf>,
) -> Result<ProfileBranchDecision> {
    if let Some(existing) = checked_out_path {
        ctx.ui.print_step(&format!(
            "Opening existing branch at: {}",
            existing.display()
        ));
        return Ok(ProfileBranchDecision::ReuseExisting { path: existing });
    }

    ctx.ui
        .print_step(&format!("Reusing existing branch: {branch}"));
    git.worktree_add(path, branch)?;
    Ok(ProfileBranchDecision::ReuseExisting {
        path: path.to_path_buf(),
    })
}

fn delete_profile_branch(ctx: &Ctx, git: &GitService<'_>, branch: &str) -> Result<()> {
    ctx.ui
        .print_step(&format!("Deleting existing branch: {branch}"));
    git.branch_delete_force(branch)
}
