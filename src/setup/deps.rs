use super::command::{command_working_dir, setup_command_display};
use crate::config::Config;
use crate::context::Ctx;
use anyhow::Result;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub(super) fn install_deps(ctx: &Ctx, config: &Config, wt_path: &Path) -> Result<()> {
    let applicable: Vec<_> = config
        .setup
        .deps
        .iter()
        .filter_map(|dep| {
            let working_dir = command_working_dir(wt_path, dep.working_dir.as_deref());
            let applies = dep
                .if_exists
                .as_ref()
                .is_none_or(|f| working_dir.join(f).exists());
            applies.then(|| (dep, working_dir, setup_command_display(dep)))
        })
        .collect();

    if applicable.is_empty() {
        return Ok(());
    }

    for (_, _, display) in &applicable {
        ctx.ui.print_dim(&format!("  ⏳ {display}"));
    }

    let results = Arc::new(Mutex::new(Vec::new()));

    std::thread::scope(|s| {
        let handles: Vec<_> = applicable
            .iter()
            .map(|(dep, working_dir, display)| {
                let results = Arc::clone(&results);
                let run_str = dep.run.clone();
                let working_dir = working_dir.clone();
                let display = display.clone();
                s.spawn(move || {
                    let needs_shell =
                        run_str.contains("&&") || run_str.contains("||") || run_str.contains("|");
                    let out = if needs_shell {
                        ctx.runner.run("sh", &["-c", &run_str], Some(&working_dir))
                    } else {
                        let parts: Vec<&str> = run_str.split_whitespace().collect();
                        if let Some((cmd, args)) = parts.split_first() {
                            ctx.runner.run(cmd, args, Some(&working_dir))
                        } else {
                            return;
                        }
                    };
                    match out {
                        Ok(o) if o.success => {
                            results
                                .lock()
                                .unwrap()
                                .push((display.clone(), true, String::new()));
                        }
                        Ok(o) => {
                            results.lock().unwrap().push((
                                display.clone(),
                                false,
                                o.stderr.clone(),
                            ));
                        }
                        Err(e) => {
                            results
                                .lock()
                                .unwrap()
                                .push((display.clone(), false, e.to_string()));
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().ok();
        }
    });

    for (cmd, success, err) in results.lock().unwrap().iter() {
        if *success {
            ctx.ui.print_dim(&format!("  ✓ {cmd}"));
        } else {
            ctx.ui.print_warning(&format!("  ✗ {cmd}"));
            if !err.is_empty() {
                ctx.ui.print_dim(&format!("    {err}"));
            }
        }
    }

    Ok(())
}
