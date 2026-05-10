use crate::context::{CmdOutput, CommandRunner};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

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

    fn has_command(&self, cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}
