use crate::config::Config;
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
        ctx.ui.print_step("No variants found. Create one with: wt variant <name>");
        return Ok(());
    }

    for (name, config) in &variants {
        let copy_count = config.worktree.copy.len() + config.worktree.copy_as.len();
        let link_count = config.worktree.link.len();
        ctx.ui.print_step(&format!(
            "  {name}  (copy: {copy_count}, link: {link_count})"
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
    ctx.ui.print_dim(&format!("  config:  {}", toml_path.display()));
    ctx.ui.print_dim(&format!("  dir:     {}", variant_dir.display()));
    ctx.ui.print_step("Edit the files to customize this variant's behavior.");

    Ok(())
}

fn generate_toml(name: &str, base: &Config) -> String {
    let mut copy_items: Vec<String> = base
        .worktree
        .copy
        .iter()
        .filter(|f| *f != "CLAUDE.local.md")
        .map(|f| format!("    \"{f}\""))
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
        .map(|f| format!("    \"{f}\""))
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
        format!(
            "claude_local_context = \"\"\"\n{claude_context}\"\"\"\n"
        )
    };

    let herd_section = base
        .herd
        .as_ref()
        .map(|h| {
            let mut s = String::from("\n[herd]\n");
            s.push_str(&format!("site_name = \"{}\"\n", h.site_name));
            if let Some(secure) = h.secure {
                s.push_str(&format!("secure = {secure}\n"));
            }
            if let Some(open) = h.open_browser {
                s.push_str(&format!("open_browser = {open}\n"));
            }
            if let Some(ref browser) = h.browser {
                s.push_str(&format!("browser = \"{browser}\"\n"));
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
                    format!("    {{ run = \"{}\", if_exists = \"{}\" }}", d.run, check)
                } else {
                    format!("    {{ run = \"{}\" }}", d.run)
                }
            })
            .collect();
        let env_lines: Vec<String> = base
            .setup
            .env
            .iter()
            .map(|(k, v)| format!("{k} = \"{v}\""))
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
            let tabs: Vec<String> = ws.tabs.iter().map(|t| format!("\"{t}\"")).collect();
            let post_deps: Vec<String> =
                ws.post_deps_tabs.iter().map(|t| format!("\"{t}\"")).collect();
            let colors: Vec<String> = ws
                .colors
                .iter()
                .map(|(k, v)| format!("{k} = \"{v}\""))
                .collect();

            let mut s = format!("\n[workspace]\ncommand = \"{}\"\n", ws.command);
            s.push_str(&format!("tabs = [{}]\n", tabs.join(", ")));
            if !post_deps.is_empty() {
                s.push_str(&format!("post_deps_tabs = [{}]\n", post_deps.join(", ")));
            }
            if !colors.is_empty() {
                s.push_str(&format!("colors = {{ {} }}\n", colors.join(", ")));
            }

            if let Some(ref post) = ws.post_ready {
                s.push_str(&format!(
                    "\n[workspace.post_ready]\nwait_for = \"{}\"\n",
                    post.wait_for
                ));
                if let Some(timeout) = post.timeout {
                    s.push_str(&format!("timeout = {timeout}\n"));
                }
                s.push_str("\n[workspace.post_ready.send]\n");
                s.push_str(&format!("issue = [\"/start\\n\"]\n"));
                s.push_str(&format!("new = [\"/start\\n\"]\n"));
            }

            s
        })
        .unwrap_or_default();

    let test_section = base
        .test
        .as_ref()
        .map(|tc| {
            let cmds: Vec<String> = tc
                .commands
                .iter()
                .map(|c| {
                    let mut parts = vec![format!("run = \"{}\"", c.run)];
                    if let Some(ref check) = c.if_exists {
                        parts.push(format!("if_exists = \"{check}\""));
                    }
                    if let Some(ref label) = c.label {
                        parts.push(format!("label = \"{label}\""));
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
{claude_context_section}{setup_section}{herd_section}{workspace_section}{test_section}"#
    )
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
    use crate::config::Config;
    use crate::context::Ctx;
    use crate::context::mock::{MockRunner, MockUi};

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
        assert!(dir
            .path()
            .join(".local/baseline/skills/start/SKILL.md")
            .exists());
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
        let copy_section = toml
            .split("copy_as")
            .next()
            .unwrap();
        assert!(!copy_section.contains("CLAUDE.local.md"));
    }
}
