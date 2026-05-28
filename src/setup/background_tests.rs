use super::command::{command_working_dir, test_command_display};
use crate::config::Config;
use crate::context::Ctx;
use anyhow::Result;
use std::path::Path;

pub(super) fn run_background_tests(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    use anyhow::Context as _;
    use std::io::Write as _;

    let test_config = match &config.test {
        Some(tc) => tc,
        None => return Ok(()),
    };
    if test_config.commands.is_empty() {
        return Ok(());
    }

    let worktree_name = wt_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "worktree".to_string());
    let task_slug = worktree_name
        .strip_prefix(&format!("{}-", ctx.repo_name))
        .map(str::to_string)
        .unwrap_or(worktree_name);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("System clock is before the Unix epoch")?
        .as_secs();
    let log_dir = ctx
        .storage_root
        .personal_root()
        .join("runtime")
        .join("launch-tests");
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create launch test log dir {}", log_dir.display()))?;
    let log_path = log_dir.join(format!("{task_slug}-{timestamp}.log"));

    for test_cmd in &test_config.commands {
        let working_dir = command_working_dir(wt_path, test_cmd.working_dir.as_deref());
        if let Some(ref check_file) = test_cmd.if_exists {
            if !working_dir.join(check_file).exists() {
                continue;
            }
        }
        let run_str = &test_cmd.run;
        let needs_shell = ["&&", "||", "|", ";", ">", "<"]
            .iter()
            .any(|operator| run_str.contains(operator));
        let display = test_command_display(test_cmd);
        let label = test_cmd.label.as_deref().unwrap_or(display.as_str());

        let mut log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("Failed to open launch test log {}", log_path.display()))?;
        writeln!(log_file, "[wt] background test started")?;
        writeln!(log_file, "[wt] label: {label}")?;
        writeln!(log_file, "[wt] command: {run_str}")?;
        writeln!(log_file, "[wt] working dir: {}", working_dir.display())?;
        writeln!(log_file)?;
        log_file.flush()?;

        let stdout = log_file
            .try_clone()
            .with_context(|| format!("Failed to clone launch test log {}", log_path.display()))?;
        let stderr = log_file
            .try_clone()
            .with_context(|| format!("Failed to clone launch test log {}", log_path.display()))?;

        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg(
                r#"mode=$1
shift
if [ "$mode" = "shell" ]; then
  sh -c "$1"
else
  "$@"
fi
status=$?
printf '\n[wt] exit code: %s\n' "$status"
exit "$status"
"#,
            )
            .arg("wt-launch-test")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(stdout))
            .stderr(std::process::Stdio::from(stderr))
            .current_dir(&working_dir);

        if needs_shell {
            command.arg("shell").arg(run_str);
        } else {
            let parts: Vec<&str> = run_str.split_whitespace().collect();
            if let Some((cmd, args)) = parts.split_first() {
                command.arg("direct").arg(cmd).args(args);
            } else {
                continue;
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;

            unsafe {
                command.pre_exec(|| {
                    nix::unistd::setsid()
                        .map(|_| ())
                        .map_err(|err| std::io::Error::from_raw_os_error(err as i32))
                });
            }
        }

        let child = command
            .spawn()
            .with_context(|| format!("Failed to start background test command: {label}"))?;
        ctx.ui.print_step(&format!(
            "Tests started in background: see {} (PID {})",
            log_path.display(),
            child.id()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, TestCommand, TestConfig};
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn background_tests_spawn_and_return_before_command_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let wt = dir.path().join("repo-background-test");
        std::fs::create_dir_all(&wt).unwrap();

        let ui = Arc::new(MockUi::new());
        let config = Config {
            test: Some(TestConfig {
                commands: vec![TestCommand {
                    working_dir: None,
                    run: "printf stdout-line && printf stderr-line >&2 && sleep 1 && exit 7".into(),
                    if_exists: None,
                    label: Some("test".into()),
                }],
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            repo.clone(),
            repo,
            config,
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        let start = Instant::now();
        run_background_tests(&ctx, &ctx.config, &wt).unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "background launch should not wait for the test command"
        );

        let steps = ui.steps.lock().unwrap().clone();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].contains("Tests started in background: see "));
        assert!(steps[0].contains("(PID "));

        let log_dir = ctx
            .storage_root
            .personal_root()
            .join("runtime")
            .join("launch-tests");
        let entries = std::fs::read_dir(&log_dir)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        let log_path = entries[0].path();
        assert!(
            log_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("background-test-")
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        let content = loop {
            let content = std::fs::read_to_string(&log_path).unwrap();
            if content.contains("[wt] exit code: 7") {
                break content;
            }
            assert!(Instant::now() < deadline, "background test did not finish");
            std::thread::sleep(Duration::from_millis(50));
        };

        assert!(content.contains("[wt] command: printf stdout-line"));
        assert!(content.contains("stdout-line"));
        assert!(content.contains("stderr-line"));
        assert!(content.contains("[wt] exit code: 7"));
    }

    #[test]
    fn background_tests_respect_if_exists_without_spawning() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let wt = dir.path().join("repo-skip-test");
        std::fs::create_dir_all(&wt).unwrap();

        let ui = Arc::new(MockUi::new());
        let config = Config {
            test: Some(TestConfig {
                commands: vec![TestCommand {
                    working_dir: None,
                    run: "printf should-not-run".into(),
                    if_exists: Some("missing-file".into()),
                    label: Some("test".into()),
                }],
            }),
            ..Config::default()
        };
        let ctx = Ctx::new(
            repo.clone(),
            repo,
            config,
            Box::new(MockRunner::new()),
            Box::new(Arc::clone(&ui)),
        );

        run_background_tests(&ctx, &ctx.config, &wt).unwrap();

        assert!(ui.steps.lock().unwrap().is_empty());
        let log_dir = ctx
            .storage_root
            .personal_root()
            .join("runtime")
            .join("launch-tests");
        assert!(std::fs::read_dir(log_dir).unwrap().next().is_none());
    }
}
