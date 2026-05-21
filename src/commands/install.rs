use crate::cli::HookAgent;
use crate::commands::agent_hook;
use crate::context::Ctx;
use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupportedAgent {
    Claude,
    Codex,
}

impl SupportedAgent {
    fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

pub fn install(ctx: &Ctx) -> Result<()> {
    install_selected(ctx, None)
}

pub fn install_selected(ctx: &Ctx, agent: Option<HookAgent>) -> Result<()> {
    let agents = match agent {
        Some(agent) => vec![SupportedAgent::from(agent)],
        None => detect_supported_agents(ctx),
    };
    if agents.is_empty() {
        bail!(
            "No supported agent CLIs found on PATH. Install Codex or Claude, or run `wt hooks setup <agent>` explicitly."
        );
    }

    if !ctx.quiet {
        let labels = agents
            .iter()
            .map(|agent| agent.command())
            .collect::<Vec<_>>()
            .join(", ");
        ctx.ui
            .print_step(&format!("Installing agent hooks for: {labels}"));
    }

    for agent in agents {
        match agent {
            SupportedAgent::Claude => agent_hook::install_claude(ctx, None)?,
            SupportedAgent::Codex => agent_hook::install_codex(ctx, None)?,
        }
    }

    Ok(())
}

pub fn uninstall(ctx: &Ctx) -> Result<()> {
    uninstall_selected(ctx, None)
}

pub fn uninstall_selected(ctx: &Ctx, agent: Option<HookAgent>) -> Result<()> {
    if !ctx.quiet {
        match agent {
            Some(agent) => ctx.ui.print_step(&format!(
                "Uninstalling wt-managed {} agent hooks",
                SupportedAgent::from(agent).command()
            )),
            None => ctx.ui.print_step("Uninstalling wt-managed agent hooks"),
        }
    }

    match agent {
        Some(HookAgent::Claude) => agent_hook::uninstall_claude(ctx, None)?,
        Some(HookAgent::Codex) => agent_hook::uninstall_codex(ctx, None)?,
        None => {
            agent_hook::uninstall_claude(ctx, None)?;
            agent_hook::uninstall_codex(ctx, None)?;
        }
    }

    Ok(())
}

fn detect_supported_agents(ctx: &Ctx) -> Vec<SupportedAgent> {
    [SupportedAgent::Claude, SupportedAgent::Codex]
        .into_iter()
        .filter(|agent| ctx.runner.has_command(agent.command()))
        .collect()
}

impl From<HookAgent> for SupportedAgent {
    fn from(agent: HookAgent) -> Self {
        match agent {
            HookAgent::Claude => Self::Claude,
            HookAgent::Codex => Self::Codex,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::{CmdOutput, CommandRunner, Ctx, UserInterface};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    #[derive(Clone)]
    struct FakeRunner {
        commands: BTreeSet<String>,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, _cmd: &str, _args: &[&str], _cwd: Option<&Path>) -> Result<CmdOutput> {
            unreachable!("detect_supported_agents should only use has_command")
        }

        fn has_command(&self, cmd: &str) -> bool {
            self.commands.contains(cmd)
        }
    }

    #[derive(Default)]
    struct NullUi;

    impl UserInterface for NullUi {
        fn select(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
            unreachable!()
        }

        fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
            unreachable!()
        }

        fn can_prompt(&self) -> bool {
            false
        }

        fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
            unreachable!()
        }

        fn input(&self, _prompt: &str, _default: Option<&str>) -> Result<String> {
            unreachable!()
        }

        fn print_step(&self, _msg: &str) {}
        fn print_dim(&self, _msg: &str) {}
        fn print_warning(&self, _msg: &str) {}
        fn print_error(&self, _msg: &str) {}
    }

    fn ctx_with_commands(commands: &[&str]) -> Ctx {
        Ctx::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo"),
            Config::default(),
            Box::new(FakeRunner {
                commands: commands.iter().map(|command| command.to_string()).collect(),
            }),
            Box::new(NullUi),
        )
    }

    #[test]
    fn detects_supported_agents_in_stable_order() {
        let ctx = ctx_with_commands(&["codex", "claude", "gemini"]);
        let agents = detect_supported_agents(&ctx);
        assert_eq!(agents, vec![SupportedAgent::Claude, SupportedAgent::Codex]);
    }

    #[test]
    fn ignores_unsupported_agents() {
        let ctx = ctx_with_commands(&["gemini"]);
        assert!(detect_supported_agents(&ctx).is_empty());
    }
}
