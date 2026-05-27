use crate::names::WorktreeNames;
use crate::storage::StorageRoot;
use crate::task_run::{self, TaskRunRecord};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(cwd: &Path) -> Result<()> {
    match resolve_binding(cwd)? {
        Some(binding) => {
            println!("export WT_AGENT_ID={};", binding.agent_id);
        }
        None => print_unset(),
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct EnvBinding {
    agent_id: String,
}

fn resolve_binding(cwd: &Path) -> Result<Option<EnvBinding>> {
    let Some(git_common_dir) = git_output(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?
    else {
        return Ok(None);
    };
    let Some(branch) = git_output(cwd, &["branch", "--show-current"])? else {
        return Ok(None);
    };
    if branch.trim().is_empty() {
        return Ok(None);
    }

    let storage_root = StorageRoot::from_git_common_dir(git_common_dir);
    let repo_root = git_output(cwd, &["rev-parse", "--show-toplevel"])?
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.to_path_buf());
    if let Some(legacy) = storage_root.detect_legacy_task_runs(&repo_root) {
        bail!("{}", legacy.error_message_for("TaskRun storage"));
    }
    let Some(record) = latest_task_run_for_branch(storage_root.task_runs_dir(), &branch)? else {
        return Ok(None);
    };

    Ok(Some(EnvBinding {
        agent_id: record
            .run
            .agent_id
            .unwrap_or_else(|| format!("agents/{}", WorktreeNames::build_branch_slug(&branch))),
    }))
}

fn latest_task_run_for_branch(
    task_runs_dir: PathBuf,
    branch: &str,
) -> Result<Option<TaskRunRecord>> {
    if !task_runs_dir.exists() {
        return Ok(None);
    }

    let mut latest: Option<TaskRunRecord> = None;
    for entry in fs::read_dir(&task_runs_dir).with_context(|| {
        format!(
            "Failed to read task run directory: {}",
            task_runs_dir.display()
        )
    })? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let Ok(run) = task_run::read(&path) else {
            continue;
        };
        if run.branch != branch {
            continue;
        }
        let record = TaskRunRecord {
            id: task_run::id_from_path(&path)?,
            path,
            run,
        };
        if latest
            .as_ref()
            .is_none_or(|current| task_run::compare_task_run_records(current, &record).is_lt())
        {
            latest = Some(record);
        }
    }

    Ok(latest)
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("git {} returned non-UTF-8 stdout", args.join(" ")))?;
    let value = stdout.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn print_unset() {
    println!("unset WT_AGENT_ID;");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn latest_task_run_scan_stays_fast_with_200_files() {
        let temp = tempfile::tempdir().unwrap();
        let task_runs_dir = temp.path().join("task-runs");
        fs::create_dir_all(&task_runs_dir).unwrap();

        for index in 0..200 {
            let branch = if index == 199 {
                "target-branch"
            } else {
                "other"
            };
            fs::write(
                task_runs_dir.join(format!("run-{index:03}.toml")),
                format!(
                    r#"task = "task-{index}"
branch = "{branch}"
status = "running"
creation_order = {}
created_at = "2026-05-21T00:00:00.000000000Z"
updated_at = "2026-05-21T00:00:00.000000000Z"
"#,
                    index + 1
                ),
            )
            .unwrap();
        }

        let started = Instant::now();
        let record = latest_task_run_for_branch(task_runs_dir, "target-branch")
            .unwrap()
            .expect("expected matching task run");
        let elapsed = started.elapsed();

        assert_eq!(record.run.branch, "target-branch");
        assert!(
            elapsed.as_millis() < 50,
            "TaskRun scan took {elapsed:?}, expected under 50ms"
        );
    }

    #[test]
    fn latest_task_run_scan_skips_malformed_files() {
        let temp = tempfile::tempdir().unwrap();
        let task_runs_dir = temp.path().join("task-runs");
        fs::create_dir_all(&task_runs_dir).unwrap();
        fs::write(task_runs_dir.join("bad.toml"), "not valid toml").unwrap();
        fs::write(
            task_runs_dir.join("good.toml"),
            r#"task = "task"
branch = "target-branch"
status = "running"
creation_order = 1
created_at = "2026-05-21T00:00:00.000000000Z"
updated_at = "2026-05-21T00:00:00.000000000Z"
"#,
        )
        .unwrap();

        let record = latest_task_run_for_branch(task_runs_dir, "target-branch")
            .unwrap()
            .expect("expected valid matching task run");

        assert_eq!(record.id, "good");
    }
}
