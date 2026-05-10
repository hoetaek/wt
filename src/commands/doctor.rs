use crate::config::{AgentCli, IssueProviderType, SiteProvider};
use crate::context::Ctx;
use anyhow::Result;
use serde::Serialize;
use std::io::Write;

pub fn run(ctx: &Ctx) -> Result<()> {
    if ctx.is_json() {
        return run_json(ctx);
    }

    ctx.ui.print_step("Doctor");
    check_issue_provider(ctx);
    check_github_cli(ctx);
    check_site_provider(ctx);
    check_agent_config(ctx);
    check_worktree_naming(ctx);
    check_cmux(ctx);
    Ok(())
}

fn run_json(ctx: &Ctx) -> Result<()> {
    let mut checks = Vec::new();
    collect_issue_provider_checks(ctx, &mut checks);
    collect_github_cli_check(ctx, &mut checks);
    collect_site_provider_checks(ctx, &mut checks);
    collect_agent_checks(ctx, &mut checks);
    collect_worktree_naming_checks(ctx, &mut checks);
    collect_cmux_check(ctx, &mut checks);

    let report = DoctorReport { checks };
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, &report)?;
    writeln!(handle)?;
    Ok(())
}

#[derive(Serialize)]
struct DoctorReport {
    checks: Vec<DoctorCheck>,
}

#[derive(Serialize)]
struct DoctorCheck {
    name: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl DoctorCheck {
    fn ok(name: impl Into<String>, message: impl Into<Option<String>>) -> Self {
        Self {
            name: name.into(),
            status: "ok",
            message: message.into(),
        }
    }

    fn warning(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: "warning",
            message: Some(message.into()),
        }
    }
}

fn collect_issue_provider_checks(ctx: &Ctx, checks: &mut Vec<DoctorCheck>) {
    match ctx.config.issues.as_ref().map(|issues| &issues.provider) {
        Some(IssueProviderType::Linear) => {
            checks.push(DoctorCheck::ok("issue_provider", Some("linear".into())));
            collect_required_command(
                ctx,
                checks,
                "linear_cli",
                "linear",
                "Install Linear CLI, or change [issues].provider to \"github\"/remove [issues].",
            );
        }
        Some(IssueProviderType::Github) => {
            checks.push(DoctorCheck::ok("issue_provider", Some("github".into())));
            collect_required_command(
                ctx,
                checks,
                "gh_cli",
                "gh",
                "Install GitHub CLI, or change [issues].provider to \"linear\"/remove [issues].",
            );
        }
        None => checks.push(DoctorCheck::ok("issue_provider", Some("none".into()))),
    }
}

fn collect_github_cli_check(ctx: &Ctx, checks: &mut Vec<DoctorCheck>) {
    if ctx
        .config
        .issues
        .as_ref()
        .is_some_and(|issues| issues.provider == IssueProviderType::Github)
    {
        return;
    }

    collect_optional_command(ctx, checks, "gh_cli_for_pr", "gh", "needed for wt pr");
}

fn collect_site_provider_checks(ctx: &Ctx, checks: &mut Vec<DoctorCheck>) {
    let Some(site) = ctx.config.effective_site() else {
        checks.push(DoctorCheck::ok("site_provider", Some("none".into())));
        return;
    };

    match site.provider {
        SiteProvider::None => checks.push(DoctorCheck::ok("site_provider", Some("none".into()))),
        SiteProvider::DockerProxy => {
            checks.push(DoctorCheck::ok(
                "site_provider",
                Some("docker_proxy".into()),
            ));
            checks.push(DoctorCheck::ok("docker_proxy", Some("ok".into())));
        }
        SiteProvider::Herd => {
            checks.push(DoctorCheck::ok("site_provider", Some("herd".into())));
            collect_required_command(
                ctx,
                checks,
                "herd_cli",
                "herd",
                "Install Herd CLI, or change [site].provider to \"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Valet => {
            checks.push(DoctorCheck::ok("site_provider", Some("valet".into())));
            collect_required_command(
                ctx,
                checks,
                "valet_cli",
                "valet",
                "Install Valet CLI, or change [site].provider to \"herd\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Traefik => {
            checks.push(DoctorCheck::ok("site_provider", Some("traefik".into())));
            collect_required_command(
                ctx,
                checks,
                "traefik_cli",
                "traefik",
                "Install Traefik CLI, or change [site].provider to \"herd\"/\"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
    }
}

fn collect_agent_checks(ctx: &Ctx, checks: &mut Vec<DoctorCheck>) {
    let Some(agent) = ctx.config.agent.as_ref() else {
        checks.push(DoctorCheck::ok("agent", Some("none".into())));
        return;
    };

    checks.push(DoctorCheck::ok(
        "agent",
        Some(agent_cli_name(&agent.cli).into()),
    ));

    if agent.cli == AgentCli::None {
        if agent.command.is_some() {
            checks.push(DoctorCheck::warning(
                "agent_command",
                "Agent command is ignored when agent.cli is \"none\".",
            ));
        }
        return;
    }

    if agent.command.is_some() {
        checks.push(DoctorCheck::ok("agent_command", Some("override".into())));
        if let Some(command) = agent.command.as_deref() {
            collect_command_string(
                ctx,
                checks,
                command,
                "agent_command",
                "Install the configured agent command, or update [agent].command.",
            );
        }
        if !agent.args.is_empty() {
            checks.push(DoctorCheck::warning(
                "agent_args",
                "Agent args are ignored when agent.command is set.",
            ));
        }
        return;
    }

    if let Some(command) = agent_cli_command(&agent.cli) {
        collect_required_command(
            ctx,
            checks,
            &format!("{command}_cli"),
            command,
            format!("Install {command}, or change [agent].cli/[agent].command."),
        );
    }

    if agent.args.is_empty() {
        checks.push(DoctorCheck::ok("agent_args", Some("none".into())));
        return;
    }

    let mut ok = true;
    for arg in &agent.args {
        if arg.is_empty() {
            ok = false;
            checks.push(DoctorCheck::warning(
                "agent_args",
                "Agent args contain an empty value. Remove it unless you intentionally want to pass ''.",
            ));
            continue;
        }

        if arg.starts_with('-') && arg.chars().any(char::is_whitespace) {
            ok = false;
            checks.push(DoctorCheck::warning(
                "agent_args",
                format!(
                    "Agent arg {arg:?} is a single argument. Split flags and values, e.g. {}.",
                    split_arg_hint(arg)
                ),
            ));
        }
    }

    if ok {
        checks.push(DoctorCheck::ok("agent_args", Some("ok".into())));
    }
}

fn collect_worktree_naming_checks(ctx: &Ctx, checks: &mut Vec<DoctorCheck>) {
    let Some(naming) = ctx.config.worktree.naming.as_ref() else {
        return;
    };

    collect_command_string(
        ctx,
        checks,
        &naming.command,
        "worktree_naming_command",
        "Install the configured naming command, or update [worktree.naming].command.",
    );
}

fn collect_cmux_check(ctx: &Ctx, checks: &mut Vec<DoctorCheck>) {
    let needs_cmux = ctx.config.workspace.is_some()
        || ctx
            .config
            .agent
            .as_ref()
            .is_some_and(|agent| !agent.prompt.is_empty());

    if needs_cmux {
        collect_required_command(
            ctx,
            checks,
            "cmux_cli",
            "cmux",
            "Install cmux, or remove [workspace]/[agent.prompt] automation.",
        );
    }
}

fn collect_optional_command(
    ctx: &Ctx,
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    cmd: &str,
    message: &str,
) {
    if ctx.runner.has_command(cmd) {
        checks.push(DoctorCheck::ok(name, Some("ok".into())));
    } else {
        checks.push(DoctorCheck::warning(name, format!("missing; {message}")));
    }
}

fn collect_required_command(
    ctx: &Ctx,
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    cmd: &str,
    fix: impl Into<String>,
) {
    if ctx.runner.has_command(cmd) {
        checks.push(DoctorCheck::ok(name, Some("ok".into())));
    } else {
        checks.push(DoctorCheck::warning(
            name,
            format!("missing. {}", fix.into()),
        ));
    }
}

fn collect_command_string(
    ctx: &Ctx,
    checks: &mut Vec<DoctorCheck>,
    command: &str,
    label: &str,
    fix: &str,
) {
    match first_command_token(command) {
        Ok(Some(cmd)) => collect_required_command(ctx, checks, label, &cmd, fix),
        Ok(None) => checks.push(DoctorCheck::warning(label, format!("empty command. {fix}"))),
        Err(e) => checks.push(DoctorCheck::warning(
            label,
            format!("invalid command ({e}). {fix}"),
        )),
    }
}

fn check_issue_provider(ctx: &Ctx) {
    match ctx.config.issues.as_ref().map(|issues| &issues.provider) {
        Some(IssueProviderType::Linear) => {
            ctx.ui.print_step("Issue provider: linear");
            check_required_command(
                ctx,
                "linear",
                "linear CLI",
                "Install Linear CLI, or change [issues].provider to \"github\"/remove [issues].",
            );
        }
        Some(IssueProviderType::Github) => {
            ctx.ui.print_step("Issue provider: github");
            check_required_command(
                ctx,
                "gh",
                "gh CLI",
                "Install GitHub CLI, or change [issues].provider to \"linear\"/remove [issues].",
            );
        }
        None => ctx.ui.print_step("Issue provider: none"),
    }
}

fn check_github_cli(ctx: &Ctx) {
    if ctx
        .config
        .issues
        .as_ref()
        .is_some_and(|issues| issues.provider == IssueProviderType::Github)
    {
        return;
    }

    let status = if ctx.runner.has_command("gh") {
        "ok"
    } else {
        "missing"
    };
    ctx.ui
        .print_step(&format!("gh CLI: {status} (needed for wt pr)"));
}

fn check_site_provider(ctx: &Ctx) {
    let Some(site) = ctx.config.effective_site() else {
        ctx.ui.print_step("Site provider: none");
        return;
    };

    match site.provider {
        SiteProvider::None => ctx.ui.print_step("Site provider: none"),
        SiteProvider::DockerProxy => {
            ctx.ui.print_step("Site provider: docker_proxy");
            ctx.ui.print_step("Docker proxy: ok");
        }
        SiteProvider::Herd => {
            ctx.ui.print_step("Site provider: herd");
            check_required_command(
                ctx,
                "herd",
                "herd CLI",
                "Install Herd CLI, or change [site].provider to \"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Valet => {
            ctx.ui.print_step("Site provider: valet");
            check_required_command(
                ctx,
                "valet",
                "valet CLI",
                "Install Valet CLI, or change [site].provider to \"herd\"/\"docker_proxy\"/\"none\".",
            );
        }
        SiteProvider::Traefik => {
            ctx.ui.print_step("Site provider: traefik");
            check_required_command(
                ctx,
                "traefik",
                "Traefik CLI",
                "Install Traefik CLI, or change [site].provider to \"herd\"/\"valet\"/\"docker_proxy\"/\"none\".",
            );
        }
    }
}

fn check_required_command(ctx: &Ctx, cmd: &str, label: &str, fix: &str) {
    if ctx.runner.has_command(cmd) {
        ctx.ui.print_step(&format!("{label}: ok"));
    } else {
        ctx.ui.print_warning(&format!("{label}: missing. {fix}"));
    }
}

fn check_agent_config(ctx: &Ctx) {
    let Some(agent) = ctx.config.agent.as_ref() else {
        ctx.ui.print_step("Agent: none");
        return;
    };

    ctx.ui
        .print_step(&format!("Agent: {}", agent_cli_name(&agent.cli)));

    if agent.cli == AgentCli::None {
        if agent.command.is_some() {
            ctx.ui
                .print_warning("Agent command is ignored when agent.cli is \"none\".");
        }
        return;
    }

    if agent.command.is_some() {
        ctx.ui.print_step("Agent command: override");
        if let Some(command) = agent.command.as_deref() {
            check_command_string(
                ctx,
                command,
                "Agent command",
                "Install the configured agent command, or update [agent].command.",
            );
        }
        if !agent.args.is_empty() {
            ctx.ui
                .print_warning("Agent args are ignored when agent.command is set.");
        }
        return;
    }

    if let Some(command) = agent_cli_command(&agent.cli) {
        check_required_command(
            ctx,
            command,
            &format!("{command} CLI"),
            &format!("Install {command}, or change [agent].cli/[agent].command."),
        );
    }

    if agent.args.is_empty() {
        ctx.ui.print_step("Agent args: none");
        return;
    }

    let mut ok = true;
    for arg in &agent.args {
        if arg.is_empty() {
            ok = false;
            ctx.ui.print_warning(
                "Agent args contain an empty value. Remove it unless you intentionally want to pass ''.",
            );
            continue;
        }

        if arg.starts_with('-') && arg.chars().any(char::is_whitespace) {
            ok = false;
            ctx.ui.print_warning(&format!(
                "Agent arg {arg:?} is a single argument. Split flags and values, e.g. {}.",
                split_arg_hint(arg)
            ));
        }
    }

    if ok {
        ctx.ui.print_step("Agent args: ok");
    }
}

fn check_worktree_naming(ctx: &Ctx) {
    let Some(naming) = ctx.config.worktree.naming.as_ref() else {
        return;
    };

    check_command_string(
        ctx,
        &naming.command,
        "worktree.naming.command",
        "Install the configured naming command, or update [worktree.naming].command.",
    );
}

fn check_cmux(ctx: &Ctx) {
    let needs_cmux = ctx.config.workspace.is_some()
        || ctx
            .config
            .agent
            .as_ref()
            .is_some_and(|agent| !agent.prompt.is_empty());

    if needs_cmux {
        check_required_command(
            ctx,
            "cmux",
            "cmux CLI",
            "Install cmux, or remove [workspace]/[agent.prompt] automation.",
        );
    }
}

fn agent_cli_name(cli: &AgentCli) -> &'static str {
    match cli {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::None => "none",
    }
}

fn agent_cli_command(cli: &AgentCli) -> Option<&'static str> {
    match cli {
        AgentCli::Codex => Some("codex"),
        AgentCli::Claude => Some("claude"),
        AgentCli::Gemini => Some("gemini"),
        AgentCli::None => None,
    }
}

fn check_command_string(ctx: &Ctx, command: &str, label: &str, fix: &str) {
    match first_command_token(command) {
        Ok(Some(cmd)) => check_required_command(ctx, &cmd, label, fix),
        Ok(None) => ctx
            .ui
            .print_warning(&format!("{label}: empty command. {fix}")),
        Err(e) => ctx
            .ui
            .print_warning(&format!("{label}: invalid command ({e}). {fix}")),
    }
}

fn first_command_token(command: &str) -> Result<Option<String>> {
    let mut parts = shell_words::split(command)?;
    Ok(parts.drain(..).next())
}

fn split_arg_hint(arg: &str) -> String {
    let parts = arg
        .split_whitespace()
        .map(toml_quote)
        .collect::<Vec<_>>()
        .join(", ");
    format!("args = [{parts}]")
}

fn toml_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentConfig, Config, IssuesConfig, ReadyMode, SiteConfig, SubmitMode, WorktreeNamingConfig,
    };
    use crate::context::mock::MockRunner;
    use crate::context::{Ctx, UserInterface};
    use anyhow::{Result, bail};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingUi {
        steps: Arc<Mutex<Vec<String>>>,
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingUi {
        fn new() -> Self {
            Self::default()
        }
    }

    impl UserInterface for RecordingUi {
        fn select(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
            bail!("unexpected select")
        }

        fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
            bail!("unexpected multi_select")
        }

        fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
            bail!("unexpected confirm")
        }

        fn input(&self, _prompt: &str, _default: Option<&str>) -> Result<String> {
            bail!("unexpected input")
        }

        fn print_step(&self, msg: &str) {
            self.steps.lock().unwrap().push(msg.into());
        }

        fn print_dim(&self, _msg: &str) {}

        fn print_warning(&self, msg: &str) {
            self.warnings.lock().unwrap().push(msg.into());
        }

        fn print_error(&self, _msg: &str) {}
    }

    fn ctx_with(config: Config, runner: MockRunner, ui: RecordingUi) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(ui),
        )
    }

    #[test]
    fn warns_when_linear_provider_cli_is_missing() {
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("linear CLI: missing"));
        assert!(warnings.contains("[issues].provider"));
    }

    #[test]
    fn reports_github_issue_provider_when_gh_is_available() {
        let config = Config {
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Github,
                gh_user: Some("alice".into()),
            }),
            ..Default::default()
        };
        let mut runner = MockRunner::new();
        runner.add_command("gh");
        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, runner, ui);

        run(&ctx).unwrap();

        let steps = steps.lock().unwrap().join("\n");
        assert!(steps.contains("Issue provider: github"));
        assert!(steps.contains("gh CLI: ok"));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn warns_when_configured_site_provider_cli_is_missing() {
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Valet,
                ..Default::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("valet CLI: missing"));
        assert!(warnings.contains("[site].provider"));
    }

    #[test]
    fn docker_proxy_site_provider_does_not_require_cli() {
        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::DockerProxy,
                ..Default::default()
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let steps = steps.lock().unwrap().join("\n");
        assert!(steps.contains("Site provider: docker_proxy"));
        assert!(steps.contains("Docker proxy: ok"));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn warns_when_agent_arg_combines_flag_and_value() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: vec!["--plugin-dir .local".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("claude CLI: missing"));
        assert!(warnings.contains("--plugin-dir .local"));
        assert!(warnings.contains("args = [\"--plugin-dir\", \".local\"]"));
    }

    #[test]
    fn reports_agent_args_ok_when_flag_and_value_are_split() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: vec!["--plugin-dir".into(), ".local".into()],
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Default::default()
        };
        let mut runner = MockRunner::new();
        runner.add_command("claude");
        let ui = RecordingUi::new();
        let steps = Arc::clone(&ui.steps);
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, runner, ui);

        run(&ctx).unwrap();

        let steps = steps.lock().unwrap().join("\n");
        assert!(steps.contains("claude CLI: ok"));
        assert!(steps.contains("Agent args: ok"));
        assert!(warnings.lock().unwrap().is_empty());
    }

    #[test]
    fn warns_when_configured_agent_cli_is_missing() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("claude CLI: missing"));
        assert!(warnings.contains("[agent].cli"));
    }

    #[test]
    fn warns_when_agent_command_override_is_missing() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Codex,
                args: Vec::new(),
                command: Some("my-agent --flag".into()),
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Agent command: missing"));
        assert!(warnings.contains("[agent].command"));
    }

    #[test]
    fn warns_when_none_agent_has_command_override() {
        let config = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::None,
                args: Vec::new(),
                command: Some("codex".into()),
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 15,
                send_after: 3,
                prompt: Default::default(),
            }),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("Agent command is ignored"));
        assert!(warnings.contains("agent.cli"));
    }

    #[test]
    fn checks_worktree_naming_command() {
        let config = Config {
            worktree: crate::config::WorktreeConfig {
                naming: Some(WorktreeNamingConfig::default()),
                ..Default::default()
            },
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("worktree.naming.command: missing"));
        assert!(warnings.contains("[worktree.naming].command"));
    }

    #[test]
    fn checks_cmux_when_workspace_is_configured() {
        let config = Config {
            workspace: Some(Default::default()),
            ..Default::default()
        };
        let ui = RecordingUi::new();
        let warnings = Arc::clone(&ui.warnings);
        let ctx = ctx_with(config, MockRunner::new(), ui);

        run(&ctx).unwrap();

        let warnings = warnings.lock().unwrap().join("\n");
        assert!(warnings.contains("cmux CLI: missing"));
        assert!(warnings.contains("[workspace]"));
    }
}
