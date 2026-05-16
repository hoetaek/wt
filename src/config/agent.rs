use std::collections::HashMap;

use crate::template;

use super::{AgentCli, AgentConfig, ReadyMode, SubmitMode};

impl AgentConfig {
    pub fn command_line(&self) -> anyhow::Result<Option<String>> {
        self.command_line_with_vars(None)
    }

    pub fn command_line_with_vars(
        &self,
        vars: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<Option<String>> {
        if self.cli == AgentCli::None {
            return Ok(None);
        }

        if let Some(command) = &self.command {
            return Ok(Some(render_agent_value(command, vars)));
        }

        let base = match self.cli {
            AgentCli::Codex => "codex",
            AgentCli::Claude => "claude",
            AgentCli::Gemini => "gemini",
            AgentCli::None => unreachable!(),
        };

        if self.args.is_empty() {
            return Ok(Some(base.into()));
        }

        let args = self
            .args
            .iter()
            .map(|arg| render_agent_value(arg, vars))
            .map(|arg| shell_escape_arg(&arg))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(Some(format!("{base} {args}")))
    }

    pub fn effective_ready(&self) -> Option<String> {
        match &self.ready {
            ReadyMode::Marker(marker) => Some(marker.clone()),
            ReadyMode::Auto => match self.cli {
                AgentCli::Codex => Some("›".into()),
                AgentCli::Claude => Some("❯".into()),
                AgentCli::Gemini | AgentCli::None => None,
            },
        }
    }

    pub fn apply_submit_suffix(&self, mut prompt: String) -> String {
        if prompt.ends_with('\n') || prompt.ends_with('\r') {
            return prompt;
        }

        match self.submit {
            SubmitMode::Auto => match self.cli {
                AgentCli::Codex => prompt.push('\r'),
                AgentCli::Claude | AgentCli::Gemini => prompt.push('\n'),
                AgentCli::None => {}
            },
            SubmitMode::Newline => prompt.push('\n'),
            SubmitMode::CarriageReturn => prompt.push('\r'),
            SubmitMode::None => {}
        }
        prompt
    }
}

fn render_agent_value(value: &str, vars: Option<&HashMap<String, String>>) -> String {
    match vars {
        Some(vars) => template::render(value, vars),
        None => value.to_string(),
    }
}

fn shell_escape_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".into();
    }

    if arg.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '@' | '+')
    }) {
        return arg.into();
    }

    format!("'{}'", arg.replace('\'', "'\\''"))
}
