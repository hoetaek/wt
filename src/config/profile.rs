use std::path::Path;

use super::merge::append_prompt_blocks;
use super::schema::{AGENT_PROMPT_WORKFLOW_SCOPE, PROMPT_COMMON_SCOPE};
use super::{Config, PathSpec};

pub(super) fn apply_profile_conventions(
    profile_dir: &Path,
    config: &mut Config,
) -> anyhow::Result<Vec<String>> {
    reject_legacy_branch_prompt_files(profile_dir)?;
    let mut warnings = Vec::new();
    if let Some(agent) = config.agent.as_mut() {
        for mode in [
            PROMPT_COMMON_SCOPE,
            "issue",
            "branch",
            "pr",
            AGENT_PROMPT_WORKFLOW_SCOPE,
        ] {
            let prompt_path = profile_dir.join("prompts").join(format!("{mode}.md"));
            if prompt_path.exists() {
                if agent
                    .prompt
                    .get(mode)
                    .is_some_and(|prompts| !prompts.is_empty())
                {
                    warnings.push(format!(
                        "[agent.prompt].{mode} from profile.toml is overridden by {}",
                        prompt_path.display()
                    ));
                }
                let prompt = std::fs::read_to_string(prompt_path)?;
                agent.prompt.insert(mode.to_string(), vec![prompt]);
            }
            let append_path = profile_dir
                .join("prompts")
                .join(format!("{mode}.append.md"));
            if append_path.exists() {
                let prompt = std::fs::read_to_string(append_path)?;
                append_prompt_blocks(
                    agent.prompt.entry(mode.to_string()).or_default(),
                    vec![prompt],
                );
            }
        }
    }

    push_scaffold_copy_if_exists(
        &mut config.worktree.copy,
        &profile_dir.join("scaffold"),
        ".",
    );

    Ok(warnings)
}

fn reject_legacy_branch_prompt_files(profile_dir: &Path) -> anyhow::Result<()> {
    for file_name in ["new.md", "new.append.md"] {
        let path = profile_dir.join("prompts").join(file_name);
        if path.exists() {
            anyhow::bail!(
                "{} is no longer supported; use prompts/branch.md or prompts/branch.append.md",
                path.display()
            );
        }
    }
    Ok(())
}

fn push_scaffold_copy_if_exists(copy: &mut Vec<PathSpec>, from: &Path, to: &str) {
    if !from.exists() {
        return;
    }
    if copy.iter().any(|entry| entry.to() == to) {
        return;
    }
    copy.push(PathSpec::Rename {
        from: from.display().to_string(),
        to: to.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use std::collections::HashMap;

    fn config_with_inline_prompt(mode: &str, value: &str) -> Config {
        let mut prompt = HashMap::new();
        prompt.insert(mode.to_string(), vec![value.to_string()]);
        let agent = AgentConfig {
            prompt,
            ..AgentConfig::default()
        };
        Config {
            agent: Some(agent),
            ..Config::default()
        }
    }

    #[test]
    fn apply_profile_conventions_warns_when_file_overrides_inline_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().to_path_buf();
        std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();
        std::fs::write(profile_dir.join("prompts/issue.md"), "file issue\n").unwrap();

        let mut config = config_with_inline_prompt("issue", "inline issue");
        let warnings = apply_profile_conventions(&profile_dir, &mut config).unwrap();

        assert_eq!(warnings.len(), 1, "expected one warning, got {warnings:?}");
        assert!(
            warnings[0].contains("[agent.prompt].issue"),
            "warning should mention conflicting mode: {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains("prompts/issue.md") || warnings[0].contains("prompts\\issue.md"),
            "warning should mention overriding file: {}",
            warnings[0]
        );

        let agent = config.agent.unwrap();
        assert_eq!(
            agent.prompt.get("issue").unwrap(),
            &vec!["file issue\n".to_string()],
            "file must still win — warning is non-fatal"
        );
    }

    #[test]
    fn apply_profile_conventions_does_not_warn_when_only_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().to_path_buf();
        std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();
        std::fs::write(profile_dir.join("prompts/issue.md"), "file issue\n").unwrap();

        let mut config = Config {
            agent: Some(AgentConfig::default()),
            ..Config::default()
        };
        let warnings = apply_profile_conventions(&profile_dir, &mut config).unwrap();

        assert!(
            warnings.is_empty(),
            "no inline prompt means no conflict: {warnings:?}"
        );
    }

    #[test]
    fn apply_profile_conventions_does_not_warn_for_append_file() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().to_path_buf();
        std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();
        std::fs::write(profile_dir.join("prompts/issue.append.md"), "file append\n").unwrap();

        let mut config = config_with_inline_prompt("issue", "inline issue");
        let warnings = apply_profile_conventions(&profile_dir, &mut config).unwrap();

        assert!(
            warnings.is_empty(),
            "append file is intentional layering, not a conflict: {warnings:?}"
        );
    }

    #[test]
    fn apply_profile_conventions_does_not_warn_when_inline_is_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let profile_dir = dir.path().to_path_buf();
        std::fs::create_dir_all(profile_dir.join("prompts")).unwrap();
        std::fs::write(profile_dir.join("prompts/issue.md"), "file issue\n").unwrap();

        let mut prompt = HashMap::new();
        prompt.insert("issue".to_string(), Vec::<String>::new());
        let agent = AgentConfig {
            prompt,
            ..AgentConfig::default()
        };
        let mut config = Config {
            agent: Some(agent),
            ..Config::default()
        };
        let warnings = apply_profile_conventions(&profile_dir, &mut config).unwrap();

        assert!(
            warnings.is_empty(),
            "empty inline vec is not a real override target: {warnings:?}"
        );
    }
}
