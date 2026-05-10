use crate::config::Config;
use crate::context::Ctx;
use crate::{commands::issue, commands::new, commands::pr};
use anyhow::{Result, bail};

pub fn run(
    ctx: &Ctx,
    target_words: &[String],
    base: &Option<String>,
    profile: Option<&str>,
    parallel: bool,
) -> Result<()> {
    let profile = selected_profile(&ctx.config, profile, parallel);
    match classify_target(target_words)? {
        StartTarget::Issue(target) => {
            issue::run(ctx, target.as_deref(), base, profile.as_deref(), parallel)
        }
        StartTarget::Pr(number) => pr::run(ctx, number, profile.as_deref()),
        StartTarget::Branch(words) => new::run(ctx, &words, base, profile.as_deref(), parallel),
    }
}

fn selected_profile(config: &Config, explicit: Option<&str>, parallel: bool) -> Option<String> {
    if let Some(profile) = explicit {
        return Some(profile.to_string());
    }
    if parallel {
        return None;
    }
    config
        .profiles
        .as_ref()
        .and_then(|profiles| profiles.default.clone())
}

#[derive(Debug, PartialEq)]
enum StartTarget {
    Issue(Option<String>),
    Pr(Option<u32>),
    Branch(Vec<String>),
}

fn classify_target(target_words: &[String]) -> Result<StartTarget> {
    if target_words.is_empty() {
        return Ok(StartTarget::Issue(None));
    }

    let target = target_words.join(" ");
    if let Some(number) = parse_prefixed_number(&target, &["pr:", "pr/", "pull:", "pull/"])? {
        return Ok(StartTarget::Pr(Some(number)));
    }
    if matches!(target_words[0].as_str(), "pr" | "pull") {
        return parse_pr_words(target_words);
    }
    if let Some(issue) = parse_prefixed_issue(&target, &["issue:", "issue/", "#"])? {
        return Ok(StartTarget::Issue(Some(issue)));
    }
    if target_words[0] == "issue" {
        return parse_issue_words(target_words);
    }
    if target_words.len() == 1 {
        if let Ok(number) = target.parse::<u32>() {
            return Ok(StartTarget::Issue(Some(number.to_string())));
        }
    }

    Ok(StartTarget::Branch(target_words.to_vec()))
}

fn parse_pr_words(target_words: &[String]) -> Result<StartTarget> {
    match target_words {
        [_] => Ok(StartTarget::Pr(None)),
        [kind, number] => {
            let number = number
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("{kind} target requires a numeric PR number"))?;
            Ok(StartTarget::Pr(Some(number)))
        }
        [kind, ..] => bail!("{kind} target accepts at most one PR number"),
        [] => unreachable!("empty targets are handled before parse_pr_words"),
    }
}

fn parse_issue_words(target_words: &[String]) -> Result<StartTarget> {
    match target_words {
        [_] => Ok(StartTarget::Issue(None)),
        [_, issue] => Ok(StartTarget::Issue(Some(issue.to_string()))),
        [kind, ..] => bail!("{kind} target accepts at most one issue identifier"),
        [] => unreachable!("empty targets are handled before parse_issue_words"),
    }
}

fn parse_prefixed_issue(target: &str, prefixes: &[&str]) -> Result<Option<String>> {
    for prefix in prefixes {
        let Some(rest) = target.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            bail!("{prefix} target requires an issue identifier");
        }
        return Ok(Some(rest.to_string()));
    }
    Ok(None)
}

fn parse_prefixed_number(target: &str, prefixes: &[&str]) -> Result<Option<u32>> {
    for prefix in prefixes {
        let Some(rest) = target.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            bail!("{prefix} target requires a number");
        }
        let number = rest
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("{target:?} is not a valid numeric target"))?;
        return Ok(Some(number));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProfilesConfig;

    fn words(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn selected_profile_uses_explicit_profile_first() {
        let config = Config {
            profiles: Some(ProfilesConfig {
                default: Some("codex".into()),
            }),
            ..Config::default()
        };

        assert_eq!(
            selected_profile(&config, Some("claude"), false).as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn selected_profile_uses_default_when_not_parallel() {
        let config = Config {
            profiles: Some(ProfilesConfig {
                default: Some("codex".into()),
            }),
            ..Config::default()
        };

        assert_eq!(
            selected_profile(&config, None, false).as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn selected_profile_ignores_default_for_parallel() {
        let config = Config {
            profiles: Some(ProfilesConfig {
                default: Some("codex".into()),
            }),
            ..Config::default()
        };

        assert_eq!(selected_profile(&config, None, true), None);
    }

    #[test]
    fn empty_target_starts_interactive_issue_flow() {
        assert_eq!(classify_target(&[]).unwrap(), StartTarget::Issue(None));
    }

    #[test]
    fn numeric_target_starts_issue() {
        assert_eq!(
            classify_target(&words(&["42"])).unwrap(),
            StartTarget::Issue(Some("42".into()))
        );
    }

    #[test]
    fn bare_issue_target_starts_interactive_issue_flow() {
        assert_eq!(
            classify_target(&words(&["issue"])).unwrap(),
            StartTarget::Issue(None)
        );
    }

    #[test]
    fn split_issue_target_starts_issue() {
        assert_eq!(
            classify_target(&words(&["issue", "PROJ-123"])).unwrap(),
            StartTarget::Issue(Some("PROJ-123".into()))
        );
    }

    #[test]
    fn split_issue_target_rejects_extra_words() {
        let err = classify_target(&words(&["issue", "PROJ-123", "extra"])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "issue target accepts at most one issue identifier"
        );
    }

    #[test]
    fn prefixed_pr_target_starts_pr() {
        assert_eq!(
            classify_target(&words(&["pr:42"])).unwrap(),
            StartTarget::Pr(Some(42))
        );
    }

    #[test]
    fn bare_pr_target_starts_interactive_pr_flow() {
        assert_eq!(
            classify_target(&words(&["pr"])).unwrap(),
            StartTarget::Pr(None)
        );
    }

    #[test]
    fn bare_pull_target_starts_interactive_pr_flow() {
        assert_eq!(
            classify_target(&words(&["pull"])).unwrap(),
            StartTarget::Pr(None)
        );
    }

    #[test]
    fn split_pr_target_starts_pr() {
        assert_eq!(
            classify_target(&words(&["pr", "42"])).unwrap(),
            StartTarget::Pr(Some(42))
        );
    }

    #[test]
    fn split_pull_target_starts_pr() {
        assert_eq!(
            classify_target(&words(&["pull", "42"])).unwrap(),
            StartTarget::Pr(Some(42))
        );
    }

    #[test]
    fn split_pr_target_rejects_non_numeric_number() {
        let err = classify_target(&words(&["pr", "feature"])).unwrap_err();
        assert_eq!(err.to_string(), "pr target requires a numeric PR number");
    }

    #[test]
    fn split_pr_target_rejects_extra_words() {
        let err = classify_target(&words(&["pr", "42", "extra"])).unwrap_err();
        assert_eq!(err.to_string(), "pr target accepts at most one PR number");
    }

    #[test]
    fn branch_words_start_new_worktree() {
        assert_eq!(
            classify_target(&words(&["my", "feature"])).unwrap(),
            StartTarget::Branch(words(&["my", "feature"]))
        );
    }
}
