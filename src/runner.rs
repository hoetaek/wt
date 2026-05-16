use crate::context::{CmdOutput, CommandRunner};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct RealRunner;

impl CommandRunner for RealRunner {
    fn run(&self, cmd: &str, args: &[&str], cwd: Option<&Path>) -> Result<CmdOutput> {
        let mut command = Command::new(cmd);
        command.args(args);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }

        let output = command
            .output()
            .with_context(|| format!("Failed to execute: {} {}", cmd, args.join(" ")))?;

        Ok(CmdOutput {
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            success: output.status.success(),
        })
    }

    fn run_with_timeout(
        &self,
        cmd: &str,
        args: &[&str],
        cwd: Option<&Path>,
        timeout: Duration,
    ) -> Result<CmdOutput> {
        if timeout.is_zero() {
            return self.run(cmd, args, cwd);
        }

        let mut command = Command::new(cmd);
        command.args(args);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("Failed to execute: {} {}", cmd, args.join(" ")))?;
        let started = Instant::now();

        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                return Ok(CmdOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    success: output.status.success(),
                });
            }

            if started.elapsed() >= timeout {
                let _ = child.kill();
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let timeout_message = format!("timed out after {}ms", timeout.as_millis());
                let stderr = if stderr.is_empty() {
                    timeout_message
                } else {
                    format!("{stderr}\n{timeout_message}")
                };

                return Ok(CmdOutput {
                    stdout,
                    stderr,
                    success: false,
                });
            }

            thread::sleep(Duration::from_millis(20));
        }
    }

    fn has_command(&self, cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}
