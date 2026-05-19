use crate::cli::ProfileCommand;
use crate::config::{
    AgentCli, AgentConfig, Config, IssueProviderType, ReadyMode, SubmitMode, validate_profile_name,
};
use crate::context::Ctx;
use anyhow::{Result, bail};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub fn run(ctx: &Ctx, command: Option<&ProfileCommand>) -> Result<()> {
    match command {
        Some(ProfileCommand::Create { name }) => create(ctx, name),
        Some(ProfileCommand::List) | None => list(ctx),
    }
}

fn list(ctx: &Ctx) -> Result<()> {
    let inventory = Config::load_profile_inventory(&ctx.repo_root, &ctx.base_config)?;

    if ctx.is_json() {
        let summaries = inventory
            .profiles
            .iter()
            .map(|record| ProfileSummary::from_config(&record.name, &record.config))
            .collect::<Vec<_>>();
        let invalid = inventory
            .invalid_profiles
            .iter()
            .map(|record| InvalidProfileSummary {
                name: record.name.clone(),
                path: record.path.display().to_string(),
                error: record.error.clone(),
            })
            .collect::<Vec<_>>();
        write_json(&ProfileListJson {
            profiles: summaries,
            invalid_profiles: invalid,
        })?;
        return Ok(());
    }

    if inventory.profiles.is_empty() && inventory.invalid_profiles.is_empty() {
        ctx.ui
            .print_step("No profiles found. Create one with: wt profile create <name>");
        return Ok(());
    }

    for record in &inventory.profiles {
        let summary = ProfileSummary::from_config(&record.name, &record.config);
        ctx.ui.print_step(&format!(
            "  {}  (copy: {}, link: {}, agent: {})",
            summary.name, summary.copy_count, summary.link_count, summary.agent
        ));
    }

    for invalid in &inventory.invalid_profiles {
        ctx.ui.print_warning(&format!(
            "Invalid profile '{}' at {}: {}",
            invalid.name,
            invalid.path.display(),
            invalid.error
        ));
    }

    Ok(())
}

fn create(ctx: &Ctx, name: &str) -> Result<()> {
    validate_profile_name(name)?;
    let loaded_base_config;
    let base_config = if ctx.base_config == Config::default() {
        loaded_base_config = Config::load_base_and_effective_with_source(&ctx.repo_root)?.0;
        &loaded_base_config
    } else {
        &ctx.base_config
    };

    let created = create_profile(
        ctx,
        ProfileCreateOptions {
            name,
            base_config,
            agent: None,
            include_prompts: true,
        },
    )?;

    if ctx.is_json() {
        write_json(&CreatedProfile {
            name: created.name,
            config_path: created.config_path.display().to_string(),
            dir: created.dir.display().to_string(),
        })?;
        return Ok(());
    }

    ctx.ui.print_step(&format!("Created profile '{name}':"));
    ctx.ui
        .print_dim(&format!("  config:  {}", created.config_path.display()));
    ctx.ui
        .print_dim(&format!("  dir:     {}", created.dir.display()));
    ctx.ui
        .print_step("Edit the files to adjust this profile's behavior.");

    Ok(())
}

pub(crate) struct ProfileCreateOptions<'a> {
    pub name: &'a str,
    pub base_config: &'a Config,
    pub agent: Option<AgentConfig>,
    pub include_prompts: bool,
}

pub(crate) struct CreatedProfileInfo {
    pub name: String,
    pub config_path: PathBuf,
    pub dir: PathBuf,
}

pub(crate) fn create_profile(
    ctx: &Ctx,
    options: ProfileCreateOptions<'_>,
) -> Result<CreatedProfileInfo> {
    let name = options.name;
    validate_profile_name(name)?;
    let profile_dir = ctx.repo_root.join(".local/profiles").join(name);
    let toml_path = profile_dir.join("profile.toml");

    if toml_path.exists() {
        bail!("Profile '{name}' already exists: {}", profile_dir.display());
    }

    let agent = options
        .agent
        .or_else(|| options.base_config.agent.clone())
        .unwrap_or_else(default_profile_agent);
    let scaffold_dir = profile_dir.join("scaffold");

    fs::create_dir_all(profile_dir.join("prompts"))?;

    match agent.cli {
        AgentCli::Codex => {
            fs::create_dir_all(scaffold_dir.join(".codex/skills/start"))?;
            let agents_override = scaffold_dir.join("AGENTS.override.md");
            let root_agents = ctx.repo_root.join("AGENTS.md");
            if root_agents.exists() {
                fs::copy(root_agents, &agents_override)?;
            } else {
                fs::write(&agents_override, generate_codex_override(name))?;
            }
            fs::write(
                scaffold_dir.join(".codex/skills/start/SKILL.md"),
                generate_start_skill(
                    name,
                    options
                        .base_config
                        .issues
                        .as_ref()
                        .map(|issues| &issues.provider),
                ),
            )?;
        }
        AgentCli::Claude => {
            fs::create_dir_all(scaffold_dir.join(".claude/agents"))?;
            fs::create_dir_all(scaffold_dir.join(".claude/commands"))?;
            fs::create_dir_all(scaffold_dir.join(".claude/skills"))?;
            fs::write(
                scaffold_dir.join("CLAUDE.local.md"),
                generate_claude_local(name),
            )?;
        }
        AgentCli::Gemini | AgentCli::None => {}
    }

    if options.include_prompts {
        fs::write(
            profile_dir.join("prompts/issue.md"),
            generate_issue_prompt(name),
        )?;
        fs::write(
            profile_dir.join("prompts/new.md"),
            generate_new_prompt(name),
        )?;
        fs::write(profile_dir.join("prompts/pr.md"), generate_pr_prompt(name))?;
    }

    fs::write(&toml_path, generate_toml(&agent))?;

    Ok(CreatedProfileInfo {
        name: name.to_string(),
        config_path: toml_path,
        dir: profile_dir,
    })
}

#[derive(Serialize)]
struct ProfileListJson {
    profiles: Vec<ProfileSummary>,
    invalid_profiles: Vec<InvalidProfileSummary>,
}

#[derive(Serialize)]
struct InvalidProfileSummary {
    name: String,
    path: String,
    error: String,
}

#[derive(Serialize)]
struct ProfileSummary {
    name: String,
    copy_count: usize,
    link_count: usize,
    agent: String,
}

impl ProfileSummary {
    fn from_config(name: &str, config: &Config) -> Self {
        Self {
            name: name.to_string(),
            copy_count: config.worktree.copy.len() + config.worktree.copy_as.len(),
            link_count: config.worktree.link.len(),
            agent: config
                .agent
                .as_ref()
                .map(|agent| agent_cli_name(&agent.cli))
                .unwrap_or("none")
                .to_string(),
        }
    }
}

#[derive(Serialize)]
struct CreatedProfile {
    name: String,
    config_path: String,
    dir: String,
}

fn write_json<T: Serialize>(value: &T) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    writeln!(handle)?;
    Ok(())
}

fn generate_toml(agent: &AgentConfig) -> String {
    generate_agent_section(agent)
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

fn default_profile_agent() -> AgentConfig {
    AgentConfig {
        cli: AgentCli::Codex,
        args: Vec::new(),
        command: None,
        ready: ReadyMode::Auto,
        submit: SubmitMode::Auto,
        timeout: 30,
        send_after: 2,
        prompt: Default::default(),
    }
}

fn generate_codex_override(name: &str) -> String {
    format!(
        r#"# Profile: {name}

This worktree is running with the `{name}` profile.

Add Codex-specific instructions for this profile here.
"#
    )
}

fn generate_claude_local(name: &str) -> String {
    format!(
        r#"## Profile: {name}

이 워크트리는 [{name}] profile로 생성되었다.

## 작업 원칙

<!-- profile별 행동 규칙을 여기에 작성 -->
"#
    )
}

fn generate_issue_prompt(name: &str) -> String {
    format!(
        r#"Use the `{name}` profile.

Review the current issue context, inspect the codebase, make the required change, verify it, and report the result.
"#
    )
}

fn generate_new_prompt(name: &str) -> String {
    format!(
        r#"Use the `{name}` profile.

Review the requested branch/task context, inspect the codebase, make the required change, verify it, and report the result.
"#
    )
}

fn generate_pr_prompt(name: &str) -> String {
    format!(
        r#"Use the `{name}` profile.

Review the current pull request context, inspect the codebase, make the required change or review, verify it, and report the result.
"#
    )
}

fn generate_start_skill(name: &str, provider: Option<&IssueProviderType>) -> String {
    match provider {
        Some(IssueProviderType::Github) => generate_github_start_skill(name),
        Some(IssueProviderType::Linear) => generate_linear_start_skill(name),
        None => generate_context_start_skill(name),
    }
}

fn generate_github_start_skill(name: &str) -> String {
    format!(
        r#"---
name: start
description: >
  [{name}] profile용 시작 워크플로우.
  현재 브랜치의 GitHub 이슈를 조회하고 작업을 시작한다.
allowed-tools: Bash(git:*), Bash(gh issue view *), Bash(gh issue list *), Glob, Grep, Read
---

# start ({name})

## 사전 정보

### 현재 브랜치

!`git branch --show-current`

## 워크플로우

### Step 1: 이슈 조회

현재 브랜치명에서 GitHub 이슈 번호를 추정한다.
이슈 번호가 확인되면 `gh issue view <이슈번호> --comments --json number,title,body,state,url,comments`를 실행하여 제목, 설명, 상태, URL, 댓글을 확인한다.
브랜치명만으로 이슈 번호가 불명확하면 `gh issue list --state all --json number,title,url`로 연결 가능한 이슈를 확인한다.

### Step 2: 코드베이스 탐색

이슈의 제목과 설명에서 핵심 키워드를 추출하고, Glob과 Grep으로 관련 코드를 탐색한다.

### Step 3: 작업 시작

<!-- [{name}] profile 고유의 작업 시작 방식을 정의 -->
"#
    )
}

fn generate_linear_start_skill(name: &str) -> String {
    format!(
        r#"---
name: start
description: >
  [{name}] profile용 시작 워크플로우.
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

<!-- [{name}] profile 고유의 작업 시작 방식을 정의 -->
"#
    )
}

fn generate_context_start_skill(name: &str) -> String {
    format!(
        r#"---
name: start
description: >
  [{name}] profile용 시작 워크플로우.
  현재 브랜치와 작업 컨텍스트를 확인하고 작업을 시작한다.
allowed-tools: Bash(git:*), Glob, Grep, Read
---

# start ({name})

## 사전 정보

### 현재 브랜치

!`git branch --show-current`

## 워크플로우

### Step 1: 작업 컨텍스트 확인

현재 브랜치명과 wt 설정을 확인한다.
연결된 이슈가 명확하지 않으면 이슈 조회를 건너뛰고 현재 작업 컨텍스트를 기준으로 진행한다.

### Step 2: 코드베이스 탐색

브랜치명과 작업 컨텍스트에서 핵심 키워드를 추출하고, Glob과 Grep으로 관련 코드를 탐색한다.

### Step 3: 작업 시작

<!-- [{name}] profile 고유의 작업 시작 방식을 정의 -->
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentCli, AgentConfig, Config, ReadyMode, SubmitMode};
    use crate::context::Ctx;
    use crate::context::UserInterface;
    use crate::context::mock::{MockRunner, MockUi};
    use anyhow::Result;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[test]
    fn list_shows_no_profiles_message() {
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
    fn list_shows_agent_summary_for_profiles() {
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
        let profile_dir = dir.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            r#"
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
        assert!(output.contains("codex  (copy: 0, link: 0, agent: codex)"));
    }

    #[test]
    fn list_subcommand_dispatches_to_inventory() {
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
        let claude_dir = dir.path().join(".local/profiles/claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("profile.toml"),
            "[agent]\ncli = \"claude\"\n",
        )
        .unwrap();

        let codex_dir = dir.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(codex_dir.join("profile.toml"), "[agent]\ncli = \"codex\"\n").unwrap();

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

        run(&ctx, Some(&ProfileCommand::List)).unwrap();

        let recorded = steps.lock().unwrap().clone();
        assert!(
            recorded
                .iter()
                .any(|line| line.contains("claude  (copy: 0, link: 0, agent: claude)")),
            "expected claude profile row, got: {recorded:?}"
        );
        assert!(
            recorded
                .iter()
                .any(|line| line.contains("codex  (copy: 0, link: 0, agent: codex)")),
            "expected codex profile row, got: {recorded:?}"
        );

        let claude_idx = recorded
            .iter()
            .position(|line| line.contains("claude  ("))
            .expect("claude row missing");
        let codex_idx = recorded
            .iter()
            .position(|line| line.contains("codex  ("))
            .expect("codex row missing");
        assert!(
            claude_idx < codex_idx,
            "expected deterministic alphabetical order (claude before codex), got: {recorded:?}"
        );
    }

    #[test]
    fn list_surfaces_invalid_profiles_as_warnings() {
        struct SharedUi {
            steps: Arc<Mutex<Vec<String>>>,
            warnings: Arc<Mutex<Vec<String>>>,
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

            fn print_warning(&self, msg: &str) {
                self.warnings.lock().unwrap().push(msg.into());
            }

            fn print_error(&self, _msg: &str) {}
        }

        let dir = tempfile::tempdir().unwrap();
        let valid_dir = dir.path().join(".local/profiles/codex");
        std::fs::create_dir_all(&valid_dir).unwrap();
        std::fs::write(valid_dir.join("profile.toml"), "[agent]\ncli = \"codex\"\n").unwrap();

        let invalid_dir = dir.path().join(".local/profiles/broken");
        std::fs::create_dir_all(&invalid_dir).unwrap();
        std::fs::write(
            invalid_dir.join("profile.toml"),
            "this is not valid toml = [\n",
        )
        .unwrap();

        let steps = Arc::new(Mutex::new(Vec::new()));
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(SharedUi {
                steps: Arc::clone(&steps),
                warnings: Arc::clone(&warnings),
            }),
        );

        run(&ctx, Some(&ProfileCommand::List)).unwrap();

        let step_output = steps.lock().unwrap().join("\n");
        assert!(step_output.contains("codex  (copy: 0, link: 0, agent: codex)"));

        let warning_output = warnings.lock().unwrap().join("\n");
        assert!(
            warning_output.contains("Invalid profile 'broken'"),
            "expected invalid profile warning, got: {warning_output}"
        );
    }

    #[test]
    fn create_scaffolds_profile_structure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".local")).unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "shared instructions\n").unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let command = ProfileCommand::Create {
            name: "baseline".into(),
        };
        assert!(run(&ctx, Some(&command)).is_ok());

        let profile_dir = dir.path().join(".local/profiles/baseline");
        assert!(profile_dir.join("profile.toml").exists());
        assert!(profile_dir.join("prompts/issue.md").exists());
        assert!(profile_dir.join("prompts/new.md").exists());
        assert!(profile_dir.join("prompts/pr.md").exists());
        assert_eq!(
            std::fs::read_to_string(profile_dir.join("scaffold/AGENTS.override.md")).unwrap(),
            "shared instructions\n"
        );
        assert!(!profile_dir.join("scaffold/CLAUDE.local.md").exists());
        assert!(
            profile_dir
                .join("scaffold/.codex/skills/start/SKILL.md")
                .exists()
        );
        let skill =
            std::fs::read_to_string(profile_dir.join("scaffold/.codex/skills/start/SKILL.md"))
                .unwrap();
        assert!(skill.contains("작업 컨텍스트"));
        assert!(!skill.contains("Linear 이슈"));
        assert!(!skill.contains("linear issue"));
        assert!(!profile_dir.join("scaffold/.claude/agents").exists());
        assert!(!profile_dir.join("scaffold/.claude/commands").exists());
    }

    #[test]
    fn create_scaffolds_github_start_skill_when_base_provider_is_github() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".wt.toml"),
            "[issues]\nprovider = \"github\"\n",
        )
        .unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let command = ProfileCommand::Create {
            name: "baseline".into(),
        };
        run(&ctx, Some(&command)).unwrap();

        let skill = std::fs::read_to_string(
            dir.path()
                .join(".local/profiles/baseline/scaffold/.codex/skills/start/SKILL.md"),
        )
        .unwrap();
        assert!(skill.contains("GitHub 이슈"));
        assert!(skill.contains("gh issue view"));
        assert!(!skill.contains("linear issue"));
    }

    #[test]
    fn create_scaffolds_claude_profile_structure() {
        let dir = tempfile::tempdir().unwrap();

        let base = Config {
            agent: Some(AgentConfig {
                cli: AgentCli::Claude,
                args: Vec::new(),
                command: None,
                ready: ReadyMode::Auto,
                submit: SubmitMode::Auto,
                timeout: 30,
                send_after: 2,
                prompt: Default::default(),
            }),
            ..Config::default()
        };

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        create_profile(
            &ctx,
            ProfileCreateOptions {
                name: "claude-plan",
                base_config: &base,
                agent: None,
                include_prompts: true,
            },
        )
        .unwrap();

        let profile_dir = dir.path().join(".local/profiles/claude-plan");
        assert!(profile_dir.join("scaffold/CLAUDE.local.md").exists());
        assert!(profile_dir.join("scaffold/.claude/agents").is_dir());
        assert!(profile_dir.join("scaffold/.claude/commands").is_dir());
        assert!(profile_dir.join("scaffold/.claude/skills").is_dir());
        assert!(!profile_dir.join("scaffold/AGENTS.override.md").exists());
        assert!(!profile_dir.join("scaffold/.codex").exists());
    }

    #[test]
    fn generated_start_skill_matches_issue_provider() {
        let github = generate_start_skill("github", Some(&IssueProviderType::Github));
        assert!(github.contains("GitHub 이슈"));
        assert!(github.contains("gh issue view"));
        assert!(!github.contains("linear issue"));

        let linear = generate_start_skill("linear", Some(&IssueProviderType::Linear));
        assert!(linear.contains("Linear 이슈"));
        assert!(linear.contains("linear issue view"));

        let context = generate_start_skill("context", None);
        assert!(context.contains("작업 컨텍스트"));
        assert!(!context.contains("GitHub 이슈"));
        assert!(!context.contains("Linear 이슈"));
        assert!(!context.contains("linear issue"));
    }

    #[test]
    fn create_rejects_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().join(".local/profiles/tdd");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("profile.toml"),
            "[agent]\ncli = \"codex\"\n",
        )
        .unwrap();

        let ctx = Ctx::new(
            dir.path().to_path_buf(),
            dir.path().to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        let command = ProfileCommand::Create { name: "tdd".into() };
        let result = run(&ctx, Some(&command));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn generated_toml_includes_agent_only_and_escapes_values() {
        let agent = AgentConfig {
            cli: AgentCli::Codex,
            args: vec!["--prompt".into(), "hello \"world\"".into()],
            command: Some("env FOO=\"bar baz\" codex --model gpt-5.5".into()),
            ready: ReadyMode::Marker("READY_MARKER".into()),
            submit: SubmitMode::CarriageReturn,
            timeout: 22,
            send_after: 4,
            prompt: HashMap::from([("issue".into(), vec!["say \"hi\"\npath C:\\tmp".into()])]),
        };

        let generated = generate_toml(&agent);
        assert!(generated.contains("[agent]"));
        assert!(generated.contains("cli = \"codex\""));
        assert!(generated.contains("[agent.prompt]"));
        assert!(!generated.contains("[worktree]"));

        let parsed: Config = toml::from_str(&generated).unwrap();
        let agent = parsed.agent.unwrap();
        assert_eq!(agent.cli, AgentCli::Codex);
        assert_eq!(agent.args, vec!["--prompt", "hello \"world\""]);
        assert_eq!(
            agent.command.as_deref(),
            Some("env FOO=\"bar baz\" codex --model gpt-5.5")
        );
        assert_eq!(agent.ready, ReadyMode::Marker("READY_MARKER".into()));
        assert_eq!(agent.submit, SubmitMode::CarriageReturn);
        assert_eq!(
            agent.prompt.get("issue").unwrap(),
            &vec!["say \"hi\"\npath C:\\tmp"]
        );
    }
}
