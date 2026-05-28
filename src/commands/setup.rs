use crate::cli::ShellInitShell;
use crate::commands::agent_hook;
use crate::context::MachineCtx;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
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
    changed_steps: Vec<String>,
    skipped_steps: Vec<String>,
}

struct SetupPlan {
    mode: SetupMode,
    steps: Vec<SetupStepPlan>,
    notices: Vec<SetupNotice>,
}

#[derive(Clone, Copy)]
enum SetupMode {
    Install,
    Remove,
}

impl SetupMode {
    fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Remove => "remove",
        }
    }
}

struct SetupStepPlan {
    label: &'static str,
    targets: Vec<PathBuf>,
    action: SetupAction,
    status: String,
    notices: Vec<SetupNotice>,
    prompt: Option<String>,
    operation: SetupOperation,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetupAction {
    Install,
    Remove,
    Repair,
    Skip,
    None,
}

impl SetupAction {
    fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Remove => "remove",
            Self::Repair => "repair",
            Self::Skip => "skip",
            Self::None => "none",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SetupNotice {
    level: SetupNoticeLevel,
    message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SetupNoticeLevel {
    Notice,
    Warning,
}

enum SetupOperation {
    InstallClaudeHooks,
    RemoveClaudeHooks,
    InstallCodexHooks,
    RemoveCodexHooks,
    AddLine { path: PathBuf, line: String },
    RemoveLine { path: PathBuf, line: String },
    Noop,
}

impl SetupOperation {
    fn is_applyable(&self) -> bool {
        !matches!(self, Self::Noop)
    }
}

pub fn run(ctx: &MachineCtx<'_>, options: SetupOptions) -> Result<()> {
    if !ctx.quiet {
        ctx.ui.print_step(if options.remove {
            "wt setup --remove"
        } else {
            "wt setup"
        });
    }

    let plan = build_setup_plan(ctx, options)?;
    print_setup_plan(ctx, options, &plan);

    if options.dry_run {
        print_summary(ctx, options, &SetupSummary::default());
        return Ok(());
    }

    let summary = apply_setup_plan(ctx, options, &plan)?;
    print_summary(ctx, options, &summary);
    Ok(())
}

fn build_setup_plan(ctx: &MachineCtx<'_>, options: SetupOptions) -> Result<SetupPlan> {
    let mut notices = Vec::new();
    let mut steps = Vec::new();

    steps.push(plan_claude_hooks(ctx, options)?);
    steps.push(plan_codex_hooks(ctx, options)?);
    let shell_target = plan_shell_target(ctx, options, &mut notices)?;
    steps.push(plan_shell_integration(options, shell_target.as_ref())?);
    steps.push(plan_shell_completion(ctx, options, shell_target.as_ref())?);

    Ok(SetupPlan {
        mode: if options.remove {
            SetupMode::Remove
        } else {
            SetupMode::Install
        },
        steps,
        notices,
    })
}

fn plan_claude_hooks(ctx: &MachineCtx<'_>, options: SetupOptions) -> Result<SetupStepPlan> {
    if options.remove {
        let settings_path = agent_hook::claude_settings_path(false)?;
        let installed = agent_hook::claude_wt_managed_hook_present()?;
        if !installed {
            return Ok(noop_step(
                "Claude hooks",
                vec![settings_path],
                "already absent",
            ));
        }
        return Ok(SetupStepPlan {
            label: "Claude hooks",
            targets: vec![settings_path.clone()],
            action: SetupAction::Remove,
            status: "remove wt-managed hook entries".into(),
            notices: Vec::new(),
            prompt: Some(format!(
                "Remove wt-managed Claude hooks from {}?",
                settings_path.display()
            )),
            operation: SetupOperation::RemoveClaudeHooks,
        });
    }

    if !ctx.runner.has_command("claude") {
        return Ok(SetupStepPlan {
            label: "Claude hooks",
            targets: Vec::new(),
            action: SetupAction::Skip,
            status: "claude CLI not found on PATH".into(),
            notices: Vec::new(),
            prompt: None,
            operation: SetupOperation::Noop,
        });
    }

    let settings_path = agent_hook::claude_settings_path(false)?;
    if agent_hook::claude_dispatcher_installed()? {
        return Ok(noop_step(
            "Claude hooks",
            vec![settings_path],
            "already installed",
        ));
    }

    let action = if agent_hook::claude_wt_managed_hook_present()? {
        SetupAction::Repair
    } else {
        SetupAction::Install
    };
    let status = if action == SetupAction::Repair {
        "replace partial/stale wt-managed hook entries"
    } else {
        "install wt-managed inbox hooks"
    };

    Ok(SetupStepPlan {
        label: "Claude hooks",
        targets: vec![settings_path.clone()],
        action,
        status: status.into(),
        notices: Vec::new(),
        prompt: Some(format!(
            "Install wt-managed Claude hooks in {}?",
            settings_path.display()
        )),
        operation: SetupOperation::InstallClaudeHooks,
    })
}

fn plan_codex_hooks(ctx: &MachineCtx<'_>, options: SetupOptions) -> Result<SetupStepPlan> {
    if options.remove {
        let codex_home = agent_hook::codex_home_dir()?;
        let hooks_path = codex_home.join("hooks.json");
        let config_path = codex_home.join("config.toml");
        let installed = agent_hook::codex_wt_managed_hook_or_trust_present()?;
        if !installed {
            return Ok(noop_step(
                "Codex hooks",
                vec![hooks_path, config_path],
                "already absent",
            ));
        }
        agent_hook::validate_codex_config_for_trust(&config_path)?;
        return Ok(SetupStepPlan {
            label: "Codex hooks",
            targets: vec![hooks_path.clone(), config_path],
            action: SetupAction::Remove,
            status: "remove wt-managed hook entries and trust state".into(),
            notices: Vec::new(),
            prompt: Some(format!(
                "Remove wt-managed Codex hooks from {}?",
                hooks_path.display()
            )),
            operation: SetupOperation::RemoveCodexHooks,
        });
    }

    if !ctx.runner.has_command("codex") {
        return Ok(SetupStepPlan {
            label: "Codex hooks",
            targets: Vec::new(),
            action: SetupAction::Skip,
            status: "codex CLI not found on PATH".into(),
            notices: Vec::new(),
            prompt: None,
            operation: SetupOperation::Noop,
        });
    }

    let codex_home = agent_hook::codex_home_dir()?;
    let hooks_path = codex_home.join("hooks.json");
    let config_path = codex_home.join("config.toml");
    agent_hook::validate_codex_config_for_trust(&config_path)?;
    if agent_hook::codex_dispatcher_installed()? {
        return Ok(noop_step(
            "Codex hooks",
            vec![hooks_path, config_path],
            "already installed",
        ));
    }

    let action = if agent_hook::codex_wt_managed_hook_or_trust_present()? {
        SetupAction::Repair
    } else {
        SetupAction::Install
    };
    let status = if action == SetupAction::Repair {
        "replace partial/stale wt-managed hooks and trust state"
    } else {
        "install wt-managed inbox hooks and trust state"
    };

    Ok(SetupStepPlan {
        label: "Codex hooks",
        targets: vec![hooks_path.clone(), config_path],
        action,
        status: status.into(),
        notices: Vec::new(),
        prompt: Some(format!(
            "Install wt-managed Codex hooks in {}?",
            hooks_path.display()
        )),
        operation: SetupOperation::InstallCodexHooks,
    })
}

fn plan_shell_target(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    notices: &mut Vec<SetupNotice>,
) -> Result<Option<ShellTarget>> {
    let Some(mut target) = resolve_shell_target()? else {
        return Ok(None);
    };
    notices.extend(maybe_retarget_macos_bash(ctx, options, &mut target)?);
    Ok(Some(target))
}

fn plan_shell_integration(
    options: SetupOptions,
    shell_target: Option<&ShellTarget>,
) -> Result<SetupStepPlan> {
    let Some(target) = shell_target else {
        if options.remove {
            return Ok(noop_step(
                "Shell integration",
                Vec::new(),
                "supported shell not detected; no rc file target",
            ));
        }
        return Ok(SetupStepPlan {
            label: "Shell integration",
            targets: Vec::new(),
            action: SetupAction::Skip,
            status: "supported shell not detected; add the eval line manually".into(),
            notices: vec![
                warning(
                    "Supported login shell not detected. Add the wt shell integration eval line to your shell rc manually.",
                ),
                notice("zsh:  eval \"$(wt shell-init zsh)\""),
                notice("bash: eval \"$(wt shell-init bash)\""),
            ],
            prompt: None,
            operation: SetupOperation::Noop,
        });
    };

    plan_line_step(
        LineStep {
            label: "Shell integration",
            path: target.rc_path.clone(),
            line: shell_integration_line(target.shell),
            add_prompt: line_add_prompt,
            remove_prompt: line_remove_prompt,
        },
        options,
    )
}

fn plan_shell_completion(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    shell_target: Option<&ShellTarget>,
) -> Result<SetupStepPlan> {
    let source = detect_wt_install_source(ctx)?;
    if source == InstallSource::Homebrew && !options.remove {
        let targets = shell_target
            .map(|target| vec![target.rc_path.clone()])
            .unwrap_or_default();
        return Ok(SetupStepPlan {
            label: "Shell completion",
            targets,
            action: SetupAction::Skip,
            status: "Homebrew-managed wt detected; completion provided by formula".into(),
            notices: Vec::new(),
            prompt: None,
            operation: SetupOperation::Noop,
        });
    }

    let Some(target) = shell_target else {
        if options.remove {
            return Ok(noop_step(
                "Shell completion",
                Vec::new(),
                "supported shell not detected; no rc file target",
            ));
        }
        return Ok(SetupStepPlan {
            label: "Shell completion",
            targets: Vec::new(),
            action: SetupAction::Skip,
            status:
                "supported shell not detected; add the completion eval line manually if desired"
                    .into(),
            notices: vec![
                warning(
                    "Supported login shell not detected. Add the wt completion eval line to your shell rc manually if desired.",
                ),
                notice("zsh:  eval \"$(wt completion zsh)\""),
                notice("bash: eval \"$(wt completion bash)\""),
            ],
            prompt: None,
            operation: SetupOperation::Noop,
        });
    };

    plan_line_step(
        LineStep {
            label: "Shell completion",
            path: target.rc_path.clone(),
            line: shell_completion_line(target.shell),
            add_prompt: completion_add_prompt,
            remove_prompt: line_remove_prompt,
        },
        options,
    )
}

struct LineStep {
    label: &'static str,
    path: PathBuf,
    line: String,
    add_prompt: fn(&Path, &str, bool) -> String,
    remove_prompt: fn(&Path, &str, bool) -> String,
}

fn plan_line_step(step: LineStep, options: SetupOptions) -> Result<SetupStepPlan> {
    let exists = step.path.exists();
    let present = line_present(&step.path, &step.line)?;

    if options.remove {
        if !present {
            return Ok(noop_step(step.label, vec![step.path], "already absent"));
        }
        return Ok(SetupStepPlan {
            label: step.label,
            targets: vec![step.path.clone()],
            action: SetupAction::Remove,
            status: format!("remove `{}`", step.line),
            notices: Vec::new(),
            prompt: Some((step.remove_prompt)(&step.path, &step.line, exists)),
            operation: SetupOperation::RemoveLine {
                path: step.path,
                line: step.line,
            },
        });
    }

    if present {
        return Ok(noop_step(step.label, vec![step.path], "already installed"));
    }

    let status = if exists {
        format!("add `{}`", step.line)
    } else {
        format!("create file and add `{}`", step.line)
    };
    Ok(SetupStepPlan {
        label: step.label,
        targets: vec![step.path.clone()],
        action: SetupAction::Install,
        status,
        notices: Vec::new(),
        prompt: Some((step.add_prompt)(&step.path, &step.line, exists)),
        operation: SetupOperation::AddLine {
            path: step.path,
            line: step.line,
        },
    })
}

fn noop_step(
    label: &'static str,
    targets: Vec<PathBuf>,
    status: impl Into<String>,
) -> SetupStepPlan {
    SetupStepPlan {
        label,
        targets,
        action: SetupAction::None,
        status: status.into(),
        notices: Vec::new(),
        prompt: None,
        operation: SetupOperation::Noop,
    }
}

fn notice(message: impl Into<String>) -> SetupNotice {
    SetupNotice {
        level: SetupNoticeLevel::Notice,
        message: message.into(),
    }
}

fn warning(message: impl Into<String>) -> SetupNotice {
    SetupNotice {
        level: SetupNoticeLevel::Warning,
        message: message.into(),
    }
}

fn print_setup_plan(ctx: &MachineCtx<'_>, options: SetupOptions, plan: &SetupPlan) {
    if ctx.quiet {
        return;
    }

    ctx.ui.print_step("Setup plan");
    for line in render_setup_plan(options, plan) {
        ctx.ui.print_dim(&format!("  {line}"));
    }
}

fn render_setup_plan(options: SetupOptions, plan: &SetupPlan) -> Vec<String> {
    let mut lines = vec![
        format!("Mode: {}", plan.mode.name()),
        "Target files:".into(),
    ];

    let target_files = target_files(plan);
    if target_files.is_empty() {
        lines.push("  - none detected".into());
    } else {
        lines.extend(
            target_files
                .into_iter()
                .map(|target| format!("  - {}", target.display())),
        );
    }

    lines.push(String::new());
    lines.push("Planned actions:".into());
    for step in &plan.steps {
        lines.push(format!(
            "  - {}: {} - {}",
            step.label,
            step.action.name(),
            step.status
        ));
        for target in &step.targets {
            lines.push(format!("    target: {}", target.display()));
        }
    }

    let notices = all_notices(plan, SetupNoticeLevel::Notice);
    let warnings = all_notices(plan, SetupNoticeLevel::Warning);
    if !notices.is_empty() {
        lines.push(String::new());
        lines.push("Notices:".into());
        lines.extend(notices.into_iter().map(|message| format!("  - {message}")));
    }
    if !warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings:".into());
        lines.extend(warnings.into_iter().map(|message| format!("  - {message}")));
    }

    lines.push(String::new());
    if options.dry_run {
        lines.push("Summary: dry run only; no files will be changed.".into());
    } else if options.yes {
        lines.push("Summary: planned write steps will be applied without prompts.".into());
    } else {
        lines.push("Summary: write steps will ask before changing files; default is No.".into());
    }

    lines
}

fn target_files(plan: &SetupPlan) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut targets = Vec::new();
    for step in &plan.steps {
        for target in &step.targets {
            let key = target.display().to_string();
            if seen.insert(key) {
                targets.push(target.clone());
            }
        }
    }
    targets
}

fn all_notices(plan: &SetupPlan, level: SetupNoticeLevel) -> Vec<String> {
    plan.notices
        .iter()
        .chain(plan.steps.iter().flat_map(|step| step.notices.iter()))
        .filter(|notice| notice.level == level)
        .map(|notice| notice.message.clone())
        .collect()
}

fn apply_setup_plan(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    plan: &SetupPlan,
) -> Result<SetupSummary> {
    let mut summary = SetupSummary::default();
    for step in &plan.steps {
        if !step.operation.is_applyable() {
            continue;
        }

        let apply = if options.yes {
            true
        } else {
            let prompt = step.prompt.as_deref().unwrap_or(step.label);
            ctx.ui.confirm(prompt, false)?
        };
        if !apply {
            summary.skipped_steps.push(step.label.to_string());
            continue;
        }

        if apply_setup_operation(ctx, &step.operation)? {
            summary.changed_steps.push(step.label.to_string());
        }
    }

    Ok(summary)
}

fn apply_setup_operation(ctx: &MachineCtx<'_>, operation: &SetupOperation) -> Result<bool> {
    match operation {
        SetupOperation::InstallClaudeHooks => {
            agent_hook::install_claude(ctx, None)?;
            Ok(true)
        }
        SetupOperation::RemoveClaudeHooks => agent_hook::uninstall_claude(ctx, None),
        SetupOperation::InstallCodexHooks => {
            agent_hook::install_codex(ctx, None)?;
            Ok(true)
        }
        SetupOperation::RemoveCodexHooks => agent_hook::uninstall_codex(ctx, None),
        SetupOperation::AddLine { path, line } => {
            if line_present(path, line)? {
                return Ok(false);
            }
            append_exact_line(path, line)?;
            Ok(true)
        }
        SetupOperation::RemoveLine { path, line } => {
            if !line_present(path, line)? {
                return Ok(false);
            }
            remove_exact_line(path, line)?;
            Ok(true)
        }
        SetupOperation::Noop => Ok(false),
    }
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
) -> Result<Vec<SetupNotice>> {
    maybe_retarget_macos_bash_with_home(ctx, options, target, &home_dir()?, is_macos_host())
}

fn maybe_retarget_macos_bash_with_home(
    ctx: &MachineCtx<'_>,
    options: SetupOptions,
    target: &mut ShellTarget,
    home: &Path,
    is_macos: bool,
) -> Result<Vec<SetupNotice>> {
    if target.shell != ShellInitShell::Bash || !is_macos {
        return Ok(Vec::new());
    }
    let bashrc = home.join(".bashrc");
    if target.rc_path != bashrc {
        return Ok(Vec::new());
    }

    let notices = vec![warning(
        "macOS Terminal.app opens login shells that read ~/.bash_profile, not ~/.bashrc.",
    )];
    if options.yes || options.dry_run {
        return Ok(notices);
    }

    let bash_profile = home.join(".bash_profile");
    if ctx.ui.confirm(
        &format!("Target {} for this run instead?", bash_profile.display()),
        false,
    )? {
        target.rc_path = bash_profile;
    }
    Ok(notices)
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

fn line_present(path: &Path, line: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read setup target file: {}", path.display()))?;
    Ok(content.lines().any(|existing| existing == line))
}

fn append_exact_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create setup target directory: {}",
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
    fs::write(path, content)
        .with_context(|| format!("Failed to write setup target file: {}", path.display()))
}

fn remove_exact_line(path: &Path, line: &str) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read setup target file: {}", path.display()))?;
    let mut updated = content
        .lines()
        .filter(|existing| *existing != line)
        .collect::<Vec<_>>()
        .join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }
    fs::write(path, updated)
        .with_context(|| format!("Failed to write setup target file: {}", path.display()))
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
    } else if summary.changed_steps.is_empty() {
        ctx.ui.print_step("Setup complete: no changes");
    } else if options.remove {
        ctx.ui.print_step(&format!(
            "Setup removal complete: {} step(s) changed",
            summary.changed_steps.len()
        ));
    } else {
        ctx.ui.print_step(&format!(
            "Setup complete: {} step(s) changed",
            summary.changed_steps.len()
        ));
    }

    if !options.dry_run && !summary.changed_steps.is_empty() {
        for step in &summary.changed_steps {
            ctx.ui.print_dim(&format!("  - {step}"));
        }
    }
    if !options.dry_run && !summary.skipped_steps.is_empty() {
        ctx.ui.print_dim("  Skipped:");
        for step in &summary.skipped_steps {
            ctx.ui.print_dim(&format!("  - {step}"));
        }
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
    fn setup_plan_does_not_include_repo_personal_storage() {
        let runner = MockRunner::new();
        let ui = MockUi::new();
        let ctx = MachineCtx::new(&runner, &ui);

        let plan = build_setup_plan(
            &ctx,
            SetupOptions {
                yes: true,
                dry_run: true,
                remove: false,
            },
        )
        .unwrap();

        assert!(
            !plan
                .steps
                .iter()
                .any(|step| step.label == "Personal storage")
        );
    }

    #[test]
    fn missing_rc_file_is_created_only_when_prompt_is_accepted() {
        let temp = TempDir::new().unwrap();
        let rc_path = temp.path().join(".zshrc");
        let runner = MockRunner::new();
        let mut ui = MockUi::new();
        ui.add_confirm(true);
        let ctx = MachineCtx::new(&runner, &ui);

        let step = plan_line_step(
            LineStep {
                label: "Shell integration",
                path: rc_path.clone(),
                line: shell_integration_line(ShellInitShell::Zsh),
                add_prompt: line_add_prompt,
                remove_prompt: line_remove_prompt,
            },
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
        )
        .unwrap();
        apply_setup_plan(
            &ctx,
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
            &SetupPlan {
                mode: SetupMode::Install,
                steps: vec![step],
                notices: Vec::new(),
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
        let step = plan_line_step(
            LineStep {
                label: "Shell integration",
                path: declined_path.clone(),
                line: shell_integration_line(ShellInitShell::Bash),
                add_prompt: line_add_prompt,
                remove_prompt: line_remove_prompt,
            },
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
        )
        .unwrap();
        apply_setup_plan(
            &ctx,
            SetupOptions {
                yes: false,
                dry_run: false,
                remove: false,
            },
            &SetupPlan {
                mode: SetupMode::Install,
                steps: vec![step],
                notices: Vec::new(),
            },
        )
        .unwrap();

        assert!(!declined_path.exists());
    }

    #[test]
    fn render_plan_summary_lists_targets_actions_and_dry_run_summary() {
        let plan = SetupPlan {
            mode: SetupMode::Install,
            steps: vec![SetupStepPlan {
                label: "Shell integration",
                targets: vec![PathBuf::from("/tmp/.zshrc")],
                action: SetupAction::Install,
                status: "add `eval \"$(wt shell-init zsh)\"`".into(),
                notices: vec![notice("example notice")],
                prompt: None,
                operation: SetupOperation::Noop,
            }],
            notices: vec![warning("example warning")],
        };

        let summary = render_setup_plan(
            SetupOptions {
                yes: false,
                dry_run: true,
                remove: false,
            },
            &plan,
        )
        .join("\n");

        assert!(summary.contains("Target files:"));
        assert!(summary.contains("/tmp/.zshrc"));
        assert!(summary.contains("Planned actions:"));
        assert!(summary.contains("Shell integration: install"));
        assert!(summary.contains("Notices:"));
        assert!(summary.contains("Warnings:"));
        assert!(summary.contains("dry run only; no files will be changed"));
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

        let notices = maybe_retarget_macos_bash_with_home(
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
        assert_eq!(notices.len(), 1);
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
