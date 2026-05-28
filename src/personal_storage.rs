use crate::context::CommandRunner;
use crate::storage::StorageRoot;
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

// No trailing slash: linked worktrees use `.wt` as a symlink, and Git
// directory-only ignore patterns do not ignore the symlink itself.
pub(crate) const EXCLUDE_LINE: &str = "/.wt";

pub(crate) fn ensure_repo_bootstrap(storage_root: &StorageRoot) -> Result<bool> {
    let mut changed = false;

    if ensure_directory_path(storage_root.personal_root())? {
        changed = true;
    }

    let exclude_path = exclude_path(storage_root);
    if !line_present(&exclude_path, EXCLUDE_LINE)? {
        append_exact_line(&exclude_path, EXCLUDE_LINE)?;
        changed = true;
    }

    Ok(changed)
}

pub(crate) fn ensure_launch_ready(
    runner: &dyn CommandRunner,
    storage_root: &StorageRoot,
    repo_root: &Path,
) -> Result<()> {
    let mut issues = Vec::new();

    if !directory_path_ready(storage_root.personal_root())? {
        issues.push(format!(
            "{} is not a directory or symlink to a directory",
            storage_root.personal_root().display()
        ));
    }

    let exclude_path = exclude_path(storage_root);
    if !line_present(&exclude_path, EXCLUDE_LINE)? {
        issues.push(format!(
            "{} is missing exact line `{EXCLUDE_LINE}`",
            exclude_path.display()
        ));
    }

    if issues.is_empty() && git_tracks_personal_storage(runner, repo_root)? {
        issues.push("`.wt` is already tracked by git".into());
    }

    if !issues.is_empty() {
        bail!(
            "wt repo bootstrap is not ready. Run `wt init` once before `wt run ...`.\n\n{}",
            issues
                .into_iter()
                .map(|issue| format!("  - {issue}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    Ok(())
}

pub(crate) fn ensure_directory_path(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Ok(false);
            }
            bail!(
                "Cannot prepare wt personal storage at {}: path exists and is not a directory",
                path.display()
            );
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            if let Ok(metadata) = fs::symlink_metadata(path) {
                if metadata.file_type().is_symlink() {
                    bail!(
                        "Cannot prepare wt personal storage at {}: symlink target is missing or not a directory",
                        path.display()
                    );
                }
                bail!(
                    "Cannot prepare wt personal storage at {}: path exists and is not a directory",
                    path.display()
                );
            }
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to inspect wt personal storage directory: {}",
                    path.display()
                )
            });
        }
    }

    fs::create_dir_all(path).with_context(|| {
        format!(
            "Failed to create wt personal storage directory: {}",
            path.display()
        )
    })?;
    Ok(true)
}

pub(crate) fn ensure_linked_worktree_symlink(link: &Path, target: &Path) -> Result<bool> {
    if symlink_points_to(link, target)? {
        return Ok(false);
    }

    match fs::symlink_metadata(link) {
        Ok(_) => {
            bail!(
                "Cannot create wt personal storage symlink at {}: path already exists and does not point to {}",
                link.display(),
                target.display()
            );
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to inspect linked worktree personal storage path: {}",
                    link.display()
                )
            });
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).with_context(|| {
            format!(
                "Failed to create wt personal storage symlink: {} -> {}",
                link.display(),
                target.display()
            )
        })?;
        Ok(true)
    }

    #[cfg(not(unix))]
    {
        let _ = (link, target);
        bail!("wt cannot create linked-worktree personal storage symlinks on this platform")
    }
}

fn exclude_path(storage_root: &StorageRoot) -> PathBuf {
    storage_root.git_common_dir().join("info/exclude")
}

fn directory_path_ready(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| {
            format!(
                "Failed to inspect wt personal storage directory: {}",
                path.display()
            )
        }),
    }
}

fn symlink_points_to(link: &Path, target: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(link) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to inspect linked worktree personal storage path: {}",
                    link.display()
                )
            });
        }
    };

    if !metadata.file_type().is_symlink() {
        return Ok(false);
    }

    let linked_to = fs::read_link(link).with_context(|| {
        format!(
            "Failed to read linked worktree personal storage symlink: {}",
            link.display()
        )
    })?;
    let absolute_target = if linked_to.is_absolute() {
        linked_to
    } else {
        link.parent().unwrap_or(Path::new(".")).join(linked_to)
    };

    Ok(equivalent_paths(&absolute_target, target))
}

fn equivalent_paths(left: &Path, right: &Path) -> bool {
    comparable_path(left) == comparable_path(right)
}

fn comparable_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| crate::storage::normalize_path_lexically(path))
}

fn line_present(path: &Path, line: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).with_context(|| {
        format!(
            "Failed to read wt personal storage file: {}",
            path.display()
        )
    })?;
    Ok(content.lines().any(|existing| existing == line))
}

fn append_exact_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create wt personal storage directory: {}",
                parent.display()
            )
        })?;
    }
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line);
    content.push('\n');
    fs::write(path, content).with_context(|| {
        format!(
            "Failed to write wt personal storage file: {}",
            path.display()
        )
    })
}

fn git_tracks_personal_storage(runner: &dyn CommandRunner, repo_root: &Path) -> Result<bool> {
    let output = runner
        .run("git", &["ls-files", "--", ".wt"], Some(repo_root))
        .context("Failed to check whether `.wt` is tracked by git")?;
    if !output.success {
        bail!(
            "Failed to check whether `.wt` is tracked by git: {}",
            command_error(&output.stdout, &output.stderr)
        );
    }
    Ok(!output.stdout.trim().is_empty())
}

fn command_error(stdout: &str, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        stderr.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn repo_bootstrap_creates_real_root_and_exact_git_exclude_line_once() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());

        assert!(ensure_repo_bootstrap(&storage).unwrap());
        assert!(storage.personal_root().is_dir());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/.wt\n"
        );

        assert!(!ensure_repo_bootstrap(&storage).unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/.wt\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn repo_bootstrap_accepts_existing_symlink_to_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external-wt-state");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(".wt")).unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());

        assert!(ensure_repo_bootstrap(&storage).unwrap());
        assert_eq!(std::fs::read_link(dir.path().join(".wt")).unwrap(), target);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap(),
            "/.wt\n"
        );
        assert!(!ensure_repo_bootstrap(&storage).unwrap());
    }

    #[test]
    fn launch_readiness_rejects_missing_bootstrap_without_git_query() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());
        let runner = MockRunner::new();

        let err = ensure_launch_ready(&runner, &storage, dir.path()).unwrap_err();
        let report = format!("{err:#}");

        assert!(report.contains("Run `wt init` once before `wt run ...`"));
        assert!(report.contains("is not a directory or symlink to a directory"));
        assert!(report.contains("is missing exact line `/.wt`"));
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn launch_readiness_rejects_tracked_personal_storage() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());
        ensure_repo_bootstrap(&storage).unwrap();
        let mut runner = MockRunner::new();
        runner.add_response(".wt/config/local.toml\n", true);

        let err = ensure_launch_ready(&runner, &storage, dir.path()).unwrap_err();

        assert!(err.to_string().contains("`.wt` is already tracked by git"));
    }

    #[test]
    fn launch_readiness_accepts_bootstrapped_untracked_storage() {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());
        ensure_repo_bootstrap(&storage).unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        ensure_launch_ready(&runner, &storage, dir.path()).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn launch_readiness_accepts_symlink_to_directory_storage() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external-wt-state");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(".wt")).unwrap();
        let storage =
            StorageRoot::from_git_common_dir_and_repo_root(dir.path().join(".git"), dir.path());
        ensure_repo_bootstrap(&storage).unwrap();
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        ensure_launch_ready(&runner, &storage, dir.path()).unwrap();
    }
}
