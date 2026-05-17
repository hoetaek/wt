use crate::config::Config;
use crate::context::Ctx;
use anyhow::Result;
use std::fs;
use std::path::Path;

pub(super) fn copy_files(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    for file in &config.worktree.copy {
        let src = ctx.repo_root.join(file);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src.clone());
            let dest = wt_path.join(file);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if real_src.is_dir() {
                copy_dir_recursive(&real_src, &dest)?;
            } else {
                fs::copy(&real_src, &dest)?;
            }
        }
    }
    for entry in &config.worktree.copy_as {
        let src = ctx.repo_root.join(&entry.from);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src.clone());
            let dest = wt_path.join(&entry.to);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if real_src.is_dir() {
                copy_dir_recursive(&real_src, &dest)?;
            } else {
                fs::copy(&real_src, &dest)?;
            }
        }
    }
    Ok(())
}

pub(super) fn link_files(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    for file in &config.worktree.link {
        let src = ctx.repo_root.join(file);
        if src.exists() {
            let real_src = fs::canonicalize(&src).unwrap_or(src);
            let dest = wt_path.join(file);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(&real_src, &dest).ok();
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dst)?;
        } else {
            fs::copy(entry.path(), dst)?;
        }
    }
    Ok(())
}
