use crate::config::{AgentCli, AgentConfig, Config, ReadyMode, SubmitMode};
use crate::context::Ctx;
use anyhow::{Result, bail};
use std::fs;

pub fn run(ctx: &Ctx, name: Option<&str>) -> Result<()> {
    match name {
        Some(name) => create(ctx, name),
        None => list(ctx),
    }
}

fn list(ctx: &Ctx) -> Result<()> {
    let variants = Config::load_variants(&ctx.repo_root)?;
    if variants.is_empty() {
        ctx.ui
            .print_step("No variants found. Create one with: wt variant <name>");
        return Ok(());
    }

    for (name, config) in &variants {
        let copy_count = config.worktree.copy.len() + config.worktree.copy_as.len();
        let link_count = config.worktree.link.len();
        let agent = config
            .agent
            .as_ref()
            .map(|agent| agent_cli_name(&agent.cli))
            .unwrap_or("none");
        ctx.ui.print_step(&format!(
            "  {name}  (copy: {copy_count}, link: {link_count}, agent: {agent})"
        ));
    }
    Ok(())
}

fn create(ctx: &Ctx, name: &str) -> Result<()> {
    let local_dir = ctx.repo_root.join(".local");
    let toml_path = local_dir.join(format!(".wt.{name}.toml"));

    if toml_path.exists() {
        bail!("Variant '{name}' already exists: {}", toml_path.display());
    }

    let variant_dir = local_dir.join(name);
    fs::create_dir_all(variant_dir.join("skills/start"))?;
    fs::create_dir_all(variant_dir.join("agents"))?;

    fs::write(
        variant_dir.join("CLAUDE.local.md"),
        generate_claude_local(name),
    )?;

    fs::write(
        variant_dir.join("skills/start/SKILL.md"),
        generate_start_skill(name),
    )?;

    let base_config = Config::load(&ctx.repo_root)?;
    fs::write(&toml_path, generate_toml(name, &base_config))?;

    ctx.ui.print_step(&format!("Created variant '{name}':"));
    ctx.ui
        .print_dim(&format!("  config:  {}", toml_path.display()));
    ctx.ui
        .print_dim(&format!("  dir:     {}", variant_dir.display()));
    ctx.ui
        .print_step("Edit the files to customize this variant's behavior.");

    Ok(())
}

fn generate_toml(name: &str, base: &Config) -> String {
    let mut copy_items: Vec<String> = base
        .worktree
        .copy
        .iter()
        .filter(|f| *f != "CLAUDE.local.md")
        .map(|f| format!("    {}", toml_quote(f)))
        .collect();

    if copy_items.is_empty() {
        copy_items.push("    \".env\"".into());
    }

    let copy_list = copy_items.join(",\n");

    let link_list = base
        .worktree
        .link
        .iter()
        .filter(|f| *f != ".local")
        .map(|f| format!("    {}", toml_quote(f)))
        .collect::<Vec<_>>()
        .join(",\n");

    let claude_context = base
        .worktree
        .claude_local_context
        .as_deref()
        .unwrap_or("")
        .to_string();

    let claude_context_section = if claude_context.is_empty() {
        String::new()
    } else {
        format!("claude_local_context = \"\"\"\n{claude_context}\"\"\"\n")
    };

    let herd_section = base
        .herd
        .as_ref()
        .map(|h| {
            let mut s = String::from("\n[herd]\n");
            s.push_str(&format!("site_name = {}\n", toml_quote(&h.site_name)));
            if let Some(secure) = h.secure {
                s.push_str(&format!("secure = {secure}\n"));
            }
            if let Some(open) = h.open_browser {
                s.push_str(&format!("open_browser = {open}\n"));
            }
            if let Some(ref browser) = h.browser {
                s.push_str(&format!("browser = {}\n", toml_quote(browser)));
            }
            s
        })
        .unwrap_or_default();

    let setup_section = if base.setup.deps.is_empty() {
        String::new()
    } else {
        let deps: Vec<String> = base
            .setup
            .deps
            .iter()
            .map(|d| {
                if let Some(ref check) = d.if_exists {
                    format!(
                        "    {{ run = {}, if_exists = {} }}",
                        toml_quote(&d.run),
                        toml_quote(check)
                    )
                } else {
                    format!("    {{ run = {} }}", toml_quote(&d.run))
                }
            })
            .collect();
        let env_lines: Vec<String> = base
            .setup
            .env
            .iter()
            .map(|(k, v)| format!("{k} = {}", toml_quote(v)))
            .collect();
        let env_section = if env_lines.is_empty() {
            String::new()
        } else {
            format!("\n[setup.env]\n{}\n", env_lines.join("\n"))
        };
        format!(
            "\n[setup]\ndeps = [\n{},\n]\n{env_section}",
            deps.join(",\n")
        )
    };

    let workspace_section = base
        .workspace
        .as_ref()
        .map(|ws| {
            let tabs: Vec<String> = ws.tabs.iter().map(|t| toml_quote(t)).collect();
            let post_deps: Vec<String> = ws.post_deps_tabs.iter().map(|t| toml_quote(t)).collect();
            let colors: Vec<String> = ws
                .colors
                .iter()
                .map(|(k, v)| format!("{k} = {}", toml_quote(v)))
                .collect();

            let mut s = String::from("\n[workspace]\n");
            s.push_str(&format!("tabs = [{}]\n", tabs.join(", ")));
            if !post_deps.is_empty() {
                s.push_str(&format!("post_deps_tabs = [{}]\n", post_deps.join(", ")));
            }
            if !colors.is_empty() {
                s.push_str(&format!("colors = {{ {} }}\n", colors.join(", ")));
            }
            if let Some(ref open_url) = ws.open_url {
                s.push_str(&format!("open_url = {}\n", toml_quote(open_url)));
            }
            if let Some(open_browser) = ws.open_browser {
                s.push_str(&format!("open_browser = {open_browser}\n"));
            }
            if let Some(ref browser) = ws.browser {
                s.push_str(&format!("browser = {}\n", toml_quote(browser)));
            }

            s
        })
        .unwrap_or_default();

    let agent_section = base
        .agent
        .as_ref()
        .map(generate_agent_section)
        .unwrap_or_default();

    let test_section = base
        .test
        .as_ref()
        .map(|tc| {
            let cmds: Vec<String> = tc
                .commands
                .iter()
                .map(|c| {
                    let mut parts = vec![format!("run = {}", toml_quote(&c.run))];
                    if let Some(ref check) = c.if_exists {
                        parts.push(format!("if_exists = {}", toml_quote(check)));
                    }
                    if let Some(ref label) = c.label {
                        parts.push(format!("label = {}", toml_quote(label)));
                    }
                    format!("    {{ {} }}", parts.join(", "))
                })
                .collect();
            format!("\n[test]\ncommands = [\n{},\n]\n", cmds.join(",\n"))
        })
        .unwrap_or_default();

    let link_section = if link_list.is_empty() {
        "link = []".to_string()
    } else {
        format!("link = [\n{link_list},\n]")
    };

    format!(
        r#"[worktree]
copy = [
{copy_list},
]
copy_as = [
    {{ from = ".local/{name}/CLAUDE.local.md", to = "CLAUDE.local.md" }},
    {{ from = ".local/{name}/skills", to = ".local/skills" }},
    {{ from = ".local/{name}/agents", to = ".local/agents" }},
]
{link_section}
    {claude_context_section}{setup_section}{herd_section}{workspace_section}{agent_section}{test_section}"#
    )
}

fn generate_agent_section(agent: &AgentConfig) -> String {
    let mut s = String::from("\n[agent]\n");
    s.push_str(&format!(
        "cli = {}\n",
        toml_quote(agent_cli_name(&agent.cli))
    ));
    if !agent.args.is_empty() {
        s.push_str(&format!("args = {}\n", toml_array(&agent.args)));
    }
    if let Some(command) = agent.command.as_deref() {
        s.push_str(&format!("command = {}\n", toml_quote(command)));
    }
    s.push_str(&format!(
        "ready = {}\n",
        toml_quote(&ready_mode_value(&agent.ready))
    ));
    s.push_str(&format!(
        "submit = {}\n",
        toml_quote(submit_mode_value(&agent.submit))
    ));
    s.push_str(&format!("timeout = {}\n", agent.timeout));
    s.push_str(&format!("send_after = {}\n", agent.send_after));

    if !agent.prompt.is_empty() {
        s.push_str("\n[agent.prompt]\n");
        let mut entries: Vec<_> = agent.prompt.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (mode, prompts) in entries {
            s.push_str(&format!("{mode} = {}\n", toml_array(prompts)));
        }
    }

    s
}

fn agent_cli_name(cli: &AgentCli) -> &'static str {
    match cli {
        AgentCli::Codex => "codex",
        AgentCli::Claude => "claude",
        AgentCli::Gemini => "gemini",
        AgentCli::Custom => "custom",
        AgentCli::None => "none",
    }
}

fn ready_mode_value(ready: &ReadyMode) -> String {
    match ready {
        ReadyMode::Auto => "auto".into(),
        ReadyMode::Marker(marker) => marker.clone(),
    }
}

fn submit_mode_value(submit: &SubmitMode) -> &'static str {
    match submit {
        SubmitMode::Auto => "auto",
        SubmitMode::Newline => "newline",
        SubmitMode::CarriageReturn => "carriage_return",
        SubmitMode::None => "none",
    }
}

fn toml_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| toml_quote(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
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

fn generate_claude_local(name: &str) -> String {
    format!(
        r#"## Variant: {name}

이 워크트리는 [{name}] variant로 생성되었다.

## 작업 원칙

<!-- variant별 행동 규칙을 여기에 작성 -->
"#
    )
}

fn generate_start_skill(name: &str) -> String {
    format!(
        r#"---
name: start
description: >
  [{name}] variant용 시작 워크플로우.
  현재 브랜치의 Linear 이슈를 조회하고 작업을 시작한다.
allowed-tools: Bash(git:*), Bash(linear issue id), Bash(linear issue view *), Bash(linear issue comment list *), Glob, Grep, Read
---

# start ({name})

## 사전 정보

### 현재 브랜치

!`git branch --show-current`

## 워크플로우

### Step 1: 이슈 조회

`linear issue id`로 현재 브랜치의 이슈 번호를 추출한다.
추출된 이슈 번호로 `linear issue view <이슈번호> --json`을 실행하여 제목, 설명, 상태, URL을 확인한다.
`linear issue comment list <이슈번호>`로 댓글도 확인한다.

### Step 2: 코드베이스 탐색

이슈의 제목과 설명에서 핵심 키워드를 추출하고, Glob과 Grep으로 관련 코드를 탐색한다.

### Step 3: 작업 시작

<!-- [{name}] variant 고유의 작업 시작 방식을 정의 -->
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentCli, AgentConfig, Config, ReadyMode, SubmitMode, WorkspaceConfig};
    use crate::context::Ctx;
    use crate::context::UserInterface;
    use crate::context::mock::{MockRunner, MockUi};
    use anyhow::Result;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn list_shows_no_variants_message() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );
        assert!(run(&ctx, None).is_ok());
    }

    #[test]
    fn list_shows_agent_summary_for_variants() {
        struct SharedUi {
            steps: Arc<Mutex<Vec<String>>>,
        }

        impl UserInterface for SharedUi {
            fn select(&self, _prompt: &str, _items: &[String]) -> Result<usize> {
                unreachable!()
            }

            fn multi_select(&self, _prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
                unreachable!()
            }

            fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool> {
                unreachable!()
            }

            fn input(&self, _prompt: &str, _default: Option<&str>) -> Result<String> {
                unreachable!()
            }

            fn print_step(&self, msg: &str) {
                self.steps.lock().unwrap().push(msg.into());
            }

            fn print_dim(&self, _msg: &str) {}

            fn print_warning(&self, _msg: &str) {}

            fn print_error(&self, _msg: &str) {}
        }

        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(".local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(
            local.join(".wt.codex.toml"),
            r#"
[worktree]
copy = [".env"]

[agent]
cli = "codex"
"#,
        )
        .unwrap();

        let steps = Arc::new(Mutex::new(Vec::new()));
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(SharedUi {
                steps: Arc::clone(&steps),
            }),
        );

        run(&ctx, None).unwrap();

        let output = steps.lock().unwrap().join("\n");
        assert!(output.contains("codex  (copy: 1, link: 0, agent: codex)"));
    }

    #[test]
    fn create_scaffolds_variant_structure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".local")).unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert!(run(&ctx, Some("baseline")).is_ok());

        assert!(dir.path().join(".local/.wt.baseline.toml").exists());
        assert!(dir.path().join(".local/baseline/CLAUDE.local.md").exists());
        assert!(
            dir.path()
                .join(".local/baseline/skills/start/SKILL.md")
                .exists()
        );
        assert!(dir.path().join(".local/baseline/agents").is_dir());
    }

    #[test]
    fn create_rejects_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join(".local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join(".wt.tdd.toml"), "[worktree]\n").unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let result = run(&ctx, Some("tdd"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn generated_toml_contains_copy_as() {
        let config = Config::default();
        let toml = generate_toml("baseline", &config);
        assert!(toml.contains("copy_as"));
        assert!(toml.contains(".local/baseline/CLAUDE.local.md"));
        assert!(toml.contains(".local/baseline/skills"));
        assert!(toml.contains(".local/baseline/agents"));
    }

    #[test]
    fn generated_toml_excludes_claude_local_from_copy() {
        let mut config = Config::default();
        config.worktree.copy = vec![".env".into(), "CLAUDE.local.md".into()];
        let toml = generate_toml("test", &config);
        let copy_section = toml.split("copy_as").next().unwrap();
        assert!(!copy_section.contains("CLAUDE.local.md"));
    }

    #[test]
    fn generated_toml_includes_agent_and_escapes_values() {
        let mut config = Config::default();
        config.workspace = Some(WorkspaceConfig {
            tabs: vec!["echo \"tab\"".into()],
            ..WorkspaceConfig::default()
        });
        config.agent = Some(AgentConfig {
            cli: AgentCli::Codex,
            args: vec!["--prompt".into(), "hello \"world\"".into()],
            command: Some("env FOO=\"bar baz\" codex --model gpt-5.5".into()),
            ready: ReadyMode::Marker("CUSTOM".into()),
            submit: SubmitMode::CarriageReturn,
            timeout: 22,
            send_after: 4,
            prompt: HashMap::from([("issue".into(), vec!["say \"hi\"\npath C:\\tmp".into()])]),
        });

        let generated = generate_toml("codex", &config);
        assert!(!generated.contains("[workspace.post_ready]"));
        assert!(generated.contains("[agent]"));
        assert!(generated.contains("cli = \"codex\""));
        assert!(generated.contains("[agent.prompt]"));

        let parsed: Config = toml::from_str(&generated).unwrap();
        let agent = parsed.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(agent.args, vec!["--prompt", "hello \"world\""]);
        assert_eq!(
            agent.command.as_deref(),
            Some("env FOO=\"bar baz\" codex --model gpt-5.5")
        );
        assert_eq!(agent.ready, ReadyMode::Marker("CUSTOM".into()));
        assert_eq!(agent.submit, SubmitMode::CarriageReturn);
        assert_eq!(
            agent.prompt.get("issue").unwrap(),
            &vec!["say \"hi\"\npath C:\\tmp"]
        );
    }
}
