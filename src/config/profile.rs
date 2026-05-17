use std::path::Path;

use super::merge::append_prompt_blocks;
use super::schema::PROMPT_COMMON_SCOPE;
use super::{Config, CopyAsEntry};

pub(super) fn apply_profile_conventions(
    repo_root: &Path,
    name: &str,
    profile_dir: &Path,
    config: &mut Config,
) -> anyhow::Result<()> {
    if let Some(agent) = config.agent.as_mut() {
        for mode in [PROMPT_COMMON_SCOPE, "issue", "new", "pr"] {
            let prompt_path = profile_dir.join("prompts").join(format!("{mode}.md"));
            if prompt_path.exists() {
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

    let profile_root = format!(".local/profiles/{name}");
    push_copy_as_if_exists(
        repo_root,
        &mut config.worktree.copy_as,
        &format!("{profile_root}/scaffold"),
        ".",
    );

    Ok(())
}

fn push_copy_as_if_exists(repo_root: &Path, copy_as: &mut Vec<CopyAsEntry>, from: &str, to: &str) {
    if !repo_root.join(from).exists() {
        return;
    }
    if copy_as.iter().any(|entry| entry.to == to) {
        return;
    }
    copy_as.push(CopyAsEntry {
        from: from.into(),
        to: to.into(),
    });
}
