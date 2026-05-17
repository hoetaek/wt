use super::command::{command_working_dir, test_command_display};
use crate::config::Config;
use crate::context::Ctx;
use anyhow::Result;
use std::path::Path;

pub(super) fn run_background_tests(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    let test_config = match &config.test {
        Some(tc) => tc,
        None => return Ok(()),
    };
    if test_config.commands.is_empty() {
        return Ok(());
    }

    ctx.ui.print_step("Running tests in background...");

    for test_cmd in &test_config.commands {
        let working_dir = command_working_dir(wt_path, test_cmd.working_dir.as_deref());
        if let Some(ref check_file) = test_cmd.if_exists {
            if !working_dir.join(check_file).exists() {
                continue;
            }
        }
        let run_str = &test_cmd.run;
        let needs_shell = run_str.contains("&&") || run_str.contains("||") || run_str.contains("|");
        let out = if needs_shell {
            ctx.runner.run("sh", &["-c", run_str], Some(&working_dir))?
        } else {
            let parts: Vec<&str> = run_str.split_whitespace().collect();
            if let Some((cmd, args)) = parts.split_first() {
                ctx.runner.run(cmd, args, Some(&working_dir))?
            } else {
                continue;
            }
        };
        let display = test_command_display(test_cmd);
        let label = test_cmd.label.as_deref().unwrap_or(display.as_str());
        if out.success {
            ctx.ui.print_step(&format!("{label}: PASSED"));
        } else {
            ctx.ui.print_warning(&format!("{label}: FAILED"));
        }
    }

    Ok(())
}
