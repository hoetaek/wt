use crate::cli::ShellInitShell;
use crate::commands::agent_hook;
use crate::context::MachineCtx;
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct SetupOptions {
    pub yes: bool,
    pub dry_run: bool,
    pub remove: bool,
}

#[derive(Default)]
struct SetupSummary {
    changed: usize,
}

pub fn run(ctx: &MachineCtx<'_>, options: SetupOptions) -> Result<()> {
    if !ctx.quiet {
        ctx.ui.print_step(if options.remove {
            "wt setup --remove"
        } else {
            "wt setup"
        });
    }

    let mut summary = SetupSummary::default();
    step_claude_hooks(ctx, options, &mut summary)?;
    step_codex_hooks(ctx, options, &mut summary)?;
    let shell_target = step_shell_integration(ctx, options, &mut summary)?;
    step_shell_completion(ctx, options, shell_target.as_ref(), &mut summary)?;
    print_summary(ctx, options, &summary);
    Ok(())
}

fn step_claude_hooks(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    summary: &mut SetupSummary,
) -> Result<()> {
    if options.remove {
        let installed = agent_hook::claude_wt_managed_hook_present()?;
        let settings_path = agent_hook::claude_settings_path(false)?;
        if !installed {
            ctx.ui.print_step("Claude hooks: already absent");
            return Ok(());
        }
        if options.dry_run {
            ctx.ui.print_step(&format!(
                "Claude hooks: would remove wt-managed entries from {}",
                settings_path.display()
            ));
            return Ok(());
        }
        let prompt = format!(
            "Remove wt-managed Claude hooks from {}?",
            settings_path.display()
        );
        if should_apply(ctx, options, &prompt)? {
            if agent_hook::uninstall_claude(ctx, None)? {
                summary.changed += 1;
            }
        } else {
            ctx.ui.print_step("Claude hooks: skipped");
        }
        return Ok(());
    }

    if !ctx.runner.has_command("claude") {
        ctx.ui
            .print_dim("Claude hooks: claude CLI not found on PATH; skipping.");
        return Ok(());
    }
    let installed = agent_hook::claude_dispatcher_installed()?;
    let settings_path = agent_hook::claude_settings_path(false)?;
    if installed {
        ctx.ui.print_step("Claude hooks: already installed");
        return Ok(());
    }
    if options.dry_run {
        ctx.ui.print_step(&format!(
            "Claude hooks: would install wt-managed entries in {}",
            settings_path.display()
        ));
        return Ok(());
    }
    let prompt = format!(
        "Install wt-managed Claude hooks in {}?",
        settings_path.display()
    );
    if should_apply(ctx, options, &prompt)? {
        agent_hook::install_claude(ctx, None)?;
        summary.changed += 1;
    } else {
        ctx.ui.print_step("Claude hooks: skipped");
    }
    Ok(())
}

fn step_codex_hooks(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    summary: &mut SetupSummary,
) -> Result<()> {
    if options.remove {
        let installed = agent_hook::codex_wt_managed_hook_or_trust_present()?;
        let codex_home = agent_hook::codex_home_dir()?;
        let hooks_path = codex_home.join("hooks.json");
        if !installed {
            ctx.ui.print_step("Codex hooks: already absent");
            return Ok(());
        }
        if options.dry_run {
            ctx.ui.print_step(&format!(
                "Codex hooks: would remove wt-managed entries from {}",
                hooks_path.display()
            ));
            return Ok(());
        }
        let prompt = format!(
            "Remove wt-managed Codex hooks from {}?",
            hooks_path.display()
        );
        if should_apply(ctx, options, &prompt)? {
            if agent_hook::uninstall_codex(ctx, None)? {
                summary.changed += 1;
            }
        } else {
            ctx.ui.print_step("Codex hooks: skipped");
        }
        return Ok(());
    }

    if !ctx.runner.has_command("codex") {
        ctx.ui
            .print_dim("Codex hooks: codex CLI not found on PATH; skipping.");
        return Ok(());
    }
    let installed = agent_hook::codex_dispatcher_installed()?;
    let codex_home = agent_hook::codex_home_dir()?;
    let hooks_path = codex_home.join("hooks.json");
    if installed {
        ctx.ui.print_step("Codex hooks: already installed");
        return Ok(());
    }
    if options.dry_run {
        ctx.ui.print_step(&format!(
            "Codex hooks: would install wt-managed entries in {}",
            hooks_path.display()
        ));
        return Ok(());
    }
    let prompt = format!(
        "Install wt-managed Codex hooks in {}?",
        hooks_path.display()
    );
    if should_apply(ctx, options, &prompt)? {
        agent_hook::install_codex(ctx, None)?;
        summary.changed += 1;
    } else {
        ctx.ui.print_step("Codex hooks: skipped");
    }
    Ok(())
}

fn step_shell_integration(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    summary: &mut SetupSummary,
) -> Result<Option<ShellTarget>> {
    let Some(mut target) = resolve_shell_target()? else {
        if options.remove {
            ctx.ui
                .print_step("Shell integration: supported shell not detected; no rc file target");
        } else {
            print_manual_shell_instructions(ctx);
        }
        return Ok(None);
    };
    maybe_retarget_macos_bash(ctx, options, &mut target)?;

    let line = shell_integration_line(target.shell);
    apply_line_step(
        ctx,
        options,
        summary,
        LineStep {
            label: "Shell integration",
            path: target.rc_path.clone(),
            line,
            add_prompt: line_add_prompt,
            remove_prompt: line_remove_prompt,
        },
    )?;
    Ok(Some(target))
}

fn step_shell_completion(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    shell_target: Option<&ShellTarget>,
    summary: &mut SetupSummary,
) -> Result<()> {
    let source = detect_wt_install_source(ctx)?;
    if source == InstallSource::Homebrew && !options.remove {
        ctx.ui
            .print_step("Homebrew-managed wt detected; completion provided by formula. Skipping.");
        return Ok(());
    }

    let Some(target) = shell_target else {
        if options.remove {
            ctx.ui
                .print_step("Shell completion: supported shell not detected; no rc file target");
        } else {
            print_manual_completion_instructions(ctx);
        }
        return Ok(());
    };

    apply_line_step(
        ctx,
        options,
        summary,
        LineStep {
            label: "Shell completion",
            path: target.rc_path.clone(),
            line: shell_completion_line(target.shell),
            add_prompt: completion_add_prompt,
            remove_prompt: line_remove_prompt,
        },
    )
}

struct LineStep {
    label: &'static str,
    path: PathBuf,
    line: String,
    add_prompt: fn(&Path, &str, bool) -> String,
    remove_prompt: fn(&Path, &str, bool) -> String,
}

fn apply_line_step(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    summary: &mut SetupSummary,
    step: LineStep,
) -> Result<()> {
    let exists = step.path.exists();
    let present = line_present(&step.path, &step.line)?;

    if options.remove {
        if !present {
            ctx.ui
                .print_step(&format!("{}: already absent", step.label));
            return Ok(());
        }
        if options.dry_run {
            ctx.ui.print_step(&format!(
                "{}: would remove `{}` from {}",
                step.label,
                step.line,
                step.path.display()
            ));
            return Ok(());
        }
        let prompt = (step.remove_prompt)(&step.path, &step.line, exists);
        if should_apply(ctx, options, &prompt)? {
            remove_exact_line(&step.path, &step.line)?;
            ctx.ui
                .print_step(&format!("{} removed: {}", step.label, step.path.display()));
            summary.changed += 1;
        } else {
            ctx.ui.print_step(&format!("{}: skipped", step.label));
        }
        return Ok(());
    }

    if present {
        ctx.ui
            .print_step(&format!("{}: already installed", step.label));
        return Ok(());
    }
    if options.dry_run {
        let action = if exists { "would add" } else { "would create" };
        ctx.ui.print_step(&format!(
            "{}: {action} `{}` in {}",
            step.label,
            step.line,
            step.path.display()
        ));
        return Ok(());
    }
    let prompt = (step.add_prompt)(&step.path, &step.line, exists);
    if should_apply(ctx, options, &prompt)? {
        append_exact_line(&step.path, &step.line)?;
        ctx.ui
            .print_step(&format!("{} added: {}", step.label, step.path.display()));
        summary.changed += 1;
    } else {
        ctx.ui.print_step(&format!("{}: skipped", step.label));
    }
    Ok(())
}

fn line_add_prompt(path: &Path, line: &str, exists: bool) -> String {
    if exists {
        format!("Add '{line}' to {}?", path.display())
    } else {
        format!("Create {} and add '{line}'?", path.display())
    }
}

fn completion_add_prompt(path: &Path, line: &str, exists: bool) -> String {
    if exists {
        format!("Add '{line}' to {}?", path.display())
    } else {
        format!("Create {} and add '{line}'?", path.display())
    }
}

fn line_remove_prompt(path: &Path, line: &str, _exists: bool) -> String {
    format!("Remove '{line}' from {}?", path.display())
}

fn should_apply(ctx: &MachineCtx<'_>, options: SetupOptions, prompt: &str) -> Result<bool> {
    if options.yes {
        return Ok(true);
    }
    ctx.ui.confirm(prompt, false)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShellTarget {
    shell: ShellInitShell,
    rc_path: PathBuf,
}

fn resolve_shell_target() -> Result<Option<ShellTarget>> {
    let shell = env::var_os("SHELL")
        .and_then(|shell| PathBuf::from(shell).file_name().map(|name| name.to_owned()))
        .and_then(|name| name.to_str().map(str::to_string));
    let Some(shell) = shell else {
        return Ok(None);
    };

    match shell.as_str() {
        "zsh" => {
            let dir = env::var_os("ZDOTDIR")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(absolute_path)
                .transpose()?
                .unwrap_or(home_dir()?);
            Ok(Some(ShellTarget {
                shell: ShellInitShell::Zsh,
                rc_path: dir.join(".zshrc"),
            }))
        }
        "bash" => Ok(Some(ShellTarget {
            shell: ShellInitShell::Bash,
            rc_path: home_dir()?.join(".bashrc"),
        })),
        _ => Ok(None),
    }
}

fn maybe_retarget_macos_bash(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    target: &mut ShellTarget,
) -> Result<()> {
    maybe_retarget_macos_bash_with_home(ctx, options, target, &home_dir()?, is_macos_host())
}

fn maybe_retarget_macos_bash_with_home(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    target: &mut ShellTarget,
    home: &Path,
    is_macos: bool,
) -> Result<()> {
    if target.shell != ShellInitShell::Bash || !is_macos {
        return Ok(());
    }
    let bashrc = home.join(".bashrc");
    if target.rc_path != bashrc {
        return Ok(());
    }

    ctx.ui.print_warning(
        "macOS Terminal.app opens login shells that read ~/.bash_profile, not ~/.bashrc.",
    );
    if options.yes || options.dry_run {
        return Ok(());
    }
    let bash_profile = home.join(".bash_profile");
    if ctx.ui.confirm(
        &format!("Target {} for this run instead?", bash_profile.display()),
        false,
    )? {
        target.rc_path = bash_profile;
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .map(absolute_path)
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("Cannot resolve shell rc target: HOME is unset."))
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn shell_integration_line(shell: ShellInitShell) -> String {
    format!("eval \"$(wt shell-init {})\"", shell_name(shell))
}

fn shell_completion_line(shell: ShellInitShell) -> String {
    format!("eval \"$(wt completion {})\"", shell_name(shell))
}

fn shell_name(shell: ShellInitShell) -> &'static str {
    match shell {
        ShellInitShell::Zsh => "zsh",
        ShellInitShell::Bash => "bash",
    }
}

fn print_manual_shell_instructions(ctx: &MachineCtx<'_>) {
    ctx.ui.print_warning(
        "Supported login shell not detected. Add the wt shell integration eval line to your shell rc manually.",
    );
    ctx.ui.print_dim("  zsh:  eval \"$(wt shell-init zsh)\"");
    ctx.ui.print_dim("  bash: eval \"$(wt shell-init bash)\"");
}

fn print_manual_completion_instructions(ctx: &MachineCtx<'_>) {
    ctx.ui.print_warning(
        "Supported login shell not detected. Add the wt completion eval line to your shell rc manually if desired.",
    );
    ctx.ui.print_dim("  zsh:  eval \"$(wt completion zsh)\"");
    ctx.ui.print_dim("  bash: eval \"$(wt completion bash)\"");
}

fn line_present(path: &Path, line: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read shell rc file: {}", path.display()))?;
    Ok(content.lines().any(|existing| existing == line))
}

fn append_exact_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create shell rc directory: {}", parent.display())
        })?;
    }
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line);
    content.push('\n');
    fs::write(path, content)
        .with_context(|| format!("Failed to write shell rc file: {}", path.display()))
}

fn remove_exact_line(path: &Path, line: &str) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read shell rc file: {}", path.display()))?;
    let mut updated = content
        .lines()
        .filter(|existing| *existing != line)
        .collect::<Vec<_>>()
        .join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }
    fs::write(path, updated)
        .with_context(|| format!("Failed to write shell rc file: {}", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallSource {
    Homebrew,
    Other,
}

fn detect_wt_install_source(ctx: &MachineCtx<'_>) -> Result<InstallSource> {
    detect_wt_install_source_with_current_exe(ctx, env::current_exe)
}

fn detect_wt_install_source_with_current_exe(
    ctx: &MachineCtx<'_>,
    current_exe: impl FnOnce() -> std::io::Result<PathBuf>,
) -> Result<InstallSource> {
    let wt_path = match current_exe() {
        Ok(path) => path,
        Err(err) => {
            debug_install_source(
                ctx,
                &format!(
                    "failed to resolve current wt executable: {err}; assuming non-Homebrew install source"
                ),
            );
            return Ok(InstallSource::Other);
        }
    };
    let prefixes = homebrew_prefixes(ctx);
    if prefixes.iter().any(|prefix| wt_path.starts_with(prefix)) {
        Ok(InstallSource::Homebrew)
    } else {
        Ok(InstallSource::Other)
    }
}

fn debug_install_source(ctx: &MachineCtx<'_>, message: &str) {
    if ctx.verbosity > 0 && !ctx.quiet && !ctx.is_json() {
        ctx.ui.print_dim(&format!("debug: {message}"));
    }
}

fn homebrew_prefixes(ctx: &MachineCtx<'_>) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    if let Some(prefix) = env::var_os("HOMEBREW_PREFIX").filter(|prefix| !prefix.is_empty()) {
        prefixes.push(PathBuf::from(prefix));
    }
    if ctx.runner.has_command("brew")
        && let Ok(out) = ctx.runner.run("brew", &["--prefix"], None)
        && out.success
        && !out.stdout.trim().is_empty()
    {
        prefixes.push(PathBuf::from(out.stdout.trim()));
    }
    prefixes.extend([
        PathBuf::from("/opt/homebrew"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/home/linuxbrew/.linuxbrew"),
    ]);
    prefixes
}

fn is_macos_host() -> bool {
    cfg!(target_os = "macos") || env::var_os("WT_TEST_MACOS_HOST").is_some()
}

fn print_summary(ctx: &MachineCtx<'_>, options: SetupOptions, summary: &SetupSummary) {
    if ctx.quiet {
        return;
    }
    if options.dry_run {
        ctx.ui
            .print_step("Setup dry run complete: no files changed");
    } else if summary.changed == 0 {
        ctx.ui.print_step("Setup complete: no changes");
    } else if options.remove {
        ctx.ui.print_step(&format!(
            "Setup removal complete: {} step(s) changed",
            summary.changed
        ));
    } else {
        ctx.ui.print_step(&format!(
            "Setup complete: {} step(s) changed",
            summary.changed
        ));
    }
    if !options.remove {
        ctx.ui.print_step("Next: run `wt init` inside a git repo.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MachineCtxOptions;
    use crate::context::mock::{MockRunner, MockUi};
    use tempfile::TempDir;

    #[test]
    fn missing_rc_file_is_created_only_when_prompt_is_accepted() {
        let temp = TempDir::new().unwrap();
        let rc_path = temp.path().join(".zshrc");
        let runner = MockRunner::new();
        let mut ui = MockUi::new();
        ui.add_confirm(true);
        let ctx = MachineCtx::new(&runner, &ui);
        let mut summary = SetupSummary::default();

        apply_line_step(
            &ctx,
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
            &mut summary,
            LineStep {
                label: "Shell integration",
                path: rc_path.clone(),
                line: shell_integration_line(ShellInitShell::Zsh),
                add_prompt: line_add_prompt,
                remove_prompt: line_remove_prompt,
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(&rc_path).unwrap(),
            "eval \"$(wt shell-init zsh)\"\n"
        );

        let declined_path = temp.path().join(".bashrc");
        let runner = MockRunner::new();
        let mut ui = MockUi::new();
        ui.add_confirm(false);
        let ctx = MachineCtx::new(&runner, &ui);
        apply_line_step(
            &ctx,
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
            &mut SetupSummary::default(),
            LineStep {
                label: "Shell integration",
                path: declined_path.clone(),
                line: shell_integration_line(ShellInitShell::Bash),
                add_prompt: line_add_prompt,
                remove_prompt: line_remove_prompt,
            },
        )
        .unwrap();

        assert!(!declined_path.exists());
    }

    #[test]
    fn macos_bash_prompt_can_retarget_to_bash_profile() {
        let temp = TempDir::new().unwrap();
        let mut target = ShellTarget {
            shell: ShellInitShell::Bash,
            rc_path: temp.path().join(".bashrc"),
        };
        let runner = MockRunner::new();
        let mut ui = MockUi::new();
        ui.add_confirm(true);
        let ctx = MachineCtx::new(&runner, &ui);

        maybe_retarget_macos_bash_with_home(
            &ctx,
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
            &mut target,
            temp.path(),
            true,
        )
        .unwrap();

        let bash_profile = temp.path().join(".bash_profile");
        assert_eq!(target.rc_path, bash_profile);
        assert_eq!(
            ui.prompts.lock().unwrap().as_slice(),
            [format!(
                "confirm: Target {} for this run instead?",
                bash_profile.display()
            )]
        );
    }

    #[test]
    fn install_source_uses_current_exe_for_default_homebrew_prefixes() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = MachineCtx::new(&runner, &ui);

        for path in [
            "/opt/homebrew/bin/wt",
            "/usr/local/bin/wt",
            "/home/linuxbrew/.linuxbrew/bin/wt",
        ] {
            assert_eq!(
                detect_wt_install_source_with_current_exe(&ctx, || Ok(PathBuf::from(path)))
                    .unwrap(),
                InstallSource::Homebrew,
                "{path}"
            );
        }

        assert_eq!(
            detect_wt_install_source_with_current_exe(&ctx, || {
                Ok(PathBuf::from("/Users/alice/.cargo/bin/wt"))
            })
            .unwrap(),
            InstallSource::Other
        );
    }

    #[test]
    fn install_source_falls_back_to_other_when_current_exe_fails() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = MachineCtx::new_with_options(
            &runner,
            &ui,
            MachineCtxOptions {
                verbosity: 1,
                ..MachineCtxOptions::default()
            },
        );

        let source = detect_wt_install_source_with_current_exe(&ctx, || {
            Err(std::io::Error::other("current_exe failed"))
        })
        .unwrap();

        assert_eq!(source, InstallSource::Other);
        assert!(
            ui.dims
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("failed to resolve current wt executable"))
        );
    }
}
