use crate::context::Ctx;
use crate::error::WtError;
use crate::messages::{AgentId, COORDINATOR_AGENT_ID};
use crate::names::WorktreeNames;
use crate::services::git::GitService;
use anyhow::{Context, Result, bail};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownAgentCli {
    Codex,
    Claude,
}

impl KnownAgentCli {
    fn command(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }
}

pub fn run_known(ctx: &Ctx, cli: KnownAgentCli, args: &[String]) -> Result<()> {
    let parsed = parse_known_agent_args(args)?;
    let agent = derive_agent_id(ctx, parsed.role.as_deref())?;
    run_process(
        ctx,
        cli.command(),
        &parsed.command_args,
        &agent,
        cli.label(),
    )
}

pub fn run_as(ctx: &Ctx, agent: &str, command: &[String]) -> Result<()> {
    if command.is_empty() {
        bail!("wt as requires a command to run");
    }

    let agent = AgentId::parse(agent).context("Invalid agent id")?;
    run_process(ctx, &command[0], &command[1..], &agent, &command[0])
}

#[derive(Debug, PartialEq, Eq)]
struct KnownAgentArgs {
    role: Option<String>,
    command_args: Vec<String>,
}

fn parse_known_agent_args(args: &[String]) -> Result<KnownAgentArgs> {
    let Some(first) = args.first() else {
        return Ok(KnownAgentArgs {
            role: None,
            command_args: Vec::new(),
        });
    };

    if let Some(role) = first.strip_prefix('@') {
        let role = parse_role(role)?;
        return Ok(KnownAgentArgs {
            role: Some(role),
            command_args: args[1..].to_vec(),
        });
    }

    Ok(KnownAgentArgs {
        role: None,
        command_args: args.to_vec(),
    })
}

fn parse_role(role: &str) -> Result<String> {
    if role.is_empty() {
        bail!("Agent role cannot be empty. Use @planner, @reviewer, or another role name.");
    }
    if role != role.trim() {
        bail!("Agent role cannot contain leading or trailing whitespace: @{role:?}");
    }
    if role.contains('/') {
        bail!(
            "Agent role cannot contain `/`; use `wt as <agent-id> -- <command...>` for explicit path-like identities."
        );
    }
    if role == "." || role == ".." || role.starts_with('.') {
        bail!("Agent role must not be a hidden or parent directory segment: @{role}");
    }
    if !role
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        bail!(
            "Agent role may contain only ASCII letters, digits, dots, dashes, and underscores: @{role}"
        );
    }
    Ok(role.to_string())
}

fn derive_agent_id(ctx: &Ctx, role: Option<&str>) -> Result<AgentId> {
    let git = GitService::new(ctx.runner.as_ref(), Some(&ctx.invocation_root));
    let branch = git.current_branch()?;
    let id = agent_id_for_branch(&branch, role)?;
    AgentId::parse(&id)
}

fn agent_id_for_branch(branch: &str, role: Option<&str>) -> Result<String> {
    let branch_slug = WorktreeNames::build_branch_slug(branch);
    let name = match role {
        Some(role) => format!("{branch_slug}-{}", parse_role(role)?),
        None => branch_slug,
    };
    Ok(format!("agents/{name}"))
}

fn run_process(
    ctx: &Ctx,
    command: &str,
    args: &[String],
    agent: &AgentId,
    label: &str,
) -> Result<()> {
    let status = Command::new(command)
        .args(args)
        .current_dir(&ctx.invocation_root)
        .env("WT_AGENT_ID", agent.as_str())
        .env("WT_COORDINATOR_AGENT_ID", COORDINATOR_AGENT_ID)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to launch {label} command `{command}`"))?;

    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(WtError::Exit { code }.into()),
        None => Err(WtError::Exit { code: 1 }.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_agent_id_uses_branch_slug() {
        assert_eq!(
            agent_id_for_branch("alice/feat-add-schema", None).unwrap(),
            "agents/feat-add-schema"
        );
    }

    #[test]
    fn role_agent_id_stays_distinct_from_default_in_same_worktree() {
        assert_eq!(
            agent_id_for_branch("alice/feat-add-schema", Some("planner")).unwrap(),
            "agents/feat-add-schema-planner"
        );
        assert_ne!(
            agent_id_for_branch("alice/feat-add-schema", Some("planner")).unwrap(),
            agent_id_for_branch("alice/feat-add-schema", None).unwrap()
        );
    }

    #[test]
    fn known_agent_args_treat_first_at_arg_as_role() {
        let parsed = parse_known_agent_args(&[
            "@reviewer".to_string(),
            "--model".to_string(),
            "gpt".to_string(),
        ])
        .unwrap();
        assert_eq!(
            parsed,
            KnownAgentArgs {
                role: Some("reviewer".into()),
                command_args: vec!["--model".into(), "gpt".into()]
            }
        );
    }

    #[test]
    fn known_agent_args_pass_non_role_args_to_agent() {
        let parsed = parse_known_agent_args(&["--model".to_string(), "gpt".to_string()]).unwrap();
        assert_eq!(
            parsed,
            KnownAgentArgs {
                role: None,
                command_args: vec!["--model".into(), "gpt".into()]
            }
        );
    }

    #[test]
    fn role_rejects_path_like_identity() {
        let err = parse_known_agent_args(&["@main/coordinator".to_string()]).unwrap_err();
        assert!(
            err.to_string().contains("cannot contain `/`"),
            "unexpected error: {err:#}"
        );
    }
}
