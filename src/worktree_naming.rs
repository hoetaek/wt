use crate::config::WorktreeNamingConfig;
use crate::context::Ctx;
use crate::names::WorktreeNames;
use crate::template;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeNamingResult {
    pub branch: Option<String>,
    pub workspace: Option<String>,
    pub vars: HashMap<String, String>,
}

const PUBLISH_TASK_BRANCH_TEMPLATE: &str =
    "{{branch_prefix}}{{issue_key_lower}}-{{existing_branch_slug}}";

pub fn generate(
    ctx: &Ctx,
    identifier: &str,
    title: &str,
    suggested_branch: Option<&str>,
) -> Result<Option<WorktreeNamingResult>> {
    let Some(config) = ctx.config.worktree.naming.as_ref() else {
        return Ok(None);
    };

    let mut vars = build_vars(ctx, identifier, title, suggested_branch);
    let prompt = template::render(&config.prompt, &vars);
    let stdout = run_naming_command(ctx, config, &prompt)?;
    let generated = parse_generated_vars(&stdout)?;
    vars.extend(generated);

    let branch = render_optional_template("branch", config.branch.as_deref(), &vars)?
        .map(|name| sanitize_branch_name(&name))
        .filter(|name| !name.is_empty());
    let workspace = render_optional_template("workspace", config.workspace.as_deref(), &vars)?
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());

    Ok(Some(WorktreeNamingResult {
        branch,
        workspace,
        vars,
    }))
}

pub(crate) fn publish_task_branch(
    identifier: &str,
    suggested_branch: Option<&str>,
    existing_branch: &str,
) -> Result<String> {
    let vars = build_publish_task_vars(identifier, suggested_branch, existing_branch);
    let rendered = template::render(PUBLISH_TASK_BRANCH_TEMPLATE, &vars);
    if rendered.contains("{{") {
        bail!("publish task branch template has unresolved variables: {rendered}");
    }

    let branch = sanitize_branch_name(&rendered);
    if branch.is_empty() {
        bail!("publish task branch resolved to an empty branch name");
    }
    Ok(branch)
}

fn build_vars(
    ctx: &Ctx,
    identifier: &str,
    title: &str,
    suggested_branch: Option<&str>,
) -> HashMap<String, String> {
    let issue_key = identifier.trim_start_matches('#');
    let issue_number = extract_issue_number(issue_key).unwrap_or_default();
    let suggested_branch = suggested_branch.unwrap_or("");
    let (branch_prefix, branch_name) = suggested_branch
        .rsplit_once('/')
        .map(|(before, after)| (format!("{before}/"), after.to_string()))
        .unwrap_or_else(|| (String::new(), suggested_branch.to_string()));

    HashMap::from([
        ("repo".into(), ctx.repo_name.clone()),
        ("issue_identifier".into(), identifier.to_string()),
        ("issue_id".into(), issue_key.to_string()),
        ("issue_key".into(), issue_key.to_string()),
        ("issue_key_lower".into(), issue_key.to_ascii_lowercase()),
        ("issue_number".into(), issue_number),
        ("issue_title".into(), title.to_string()),
        ("suggested_branch".into(), suggested_branch.to_string()),
        ("branch_prefix".into(), branch_prefix),
        ("branch_name".into(), branch_name),
        (
            "branch_slug".into(),
            if suggested_branch.is_empty() {
                String::new()
            } else {
                WorktreeNames::build_branch_slug(suggested_branch)
            },
        ),
    ])
}

fn build_publish_task_vars(
    identifier: &str,
    suggested_branch: Option<&str>,
    existing_branch: &str,
) -> HashMap<String, String> {
    let issue_key = identifier.trim_start_matches('#');
    let issue_key_lower = issue_key.to_ascii_lowercase();
    let issue_number = extract_issue_number(issue_key).unwrap_or_default();
    let suggested_branch = suggested_branch.unwrap_or("");
    let (branch_prefix, branch_name) = suggested_branch
        .rsplit_once('/')
        .map(|(before, after)| (format!("{before}/"), after.to_string()))
        .unwrap_or_else(|| (String::new(), suggested_branch.to_string()));
    let existing_branch_slug = publish_existing_branch_slug(existing_branch, &issue_key_lower);

    HashMap::from([
        ("issue_identifier".into(), identifier.to_string()),
        ("issue_id".into(), issue_key.to_string()),
        ("issue_key".into(), issue_key.to_string()),
        ("issue_key_lower".into(), issue_key_lower),
        ("issue_number".into(), issue_number),
        ("suggested_branch".into(), suggested_branch.to_string()),
        ("branch_prefix".into(), branch_prefix),
        ("branch_name".into(), branch_name),
        ("existing_branch".into(), existing_branch.to_string()),
        ("existing_branch_slug".into(), existing_branch_slug),
    ])
}

fn publish_existing_branch_slug(existing_branch: &str, issue_key_lower: &str) -> String {
    let final_segment = existing_branch
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let slug = sanitize_branch_name(final_segment);
    strip_duplicate_issue_key_prefix(&slug, issue_key_lower)
}

fn strip_duplicate_issue_key_prefix(slug: &str, issue_key_lower: &str) -> String {
    if slug == issue_key_lower {
        return String::new();
    }

    let prefix = format!("{issue_key_lower}-");
    slug.strip_prefix(&prefix).unwrap_or(slug).to_string()
}

fn extract_issue_number(issue_key: &str) -> Option<String> {
    if issue_key.chars().all(|c| c.is_ascii_digit()) {
        return Some(issue_key.to_string());
    }

    issue_key
        .rsplit_once('-')
        .map(|(_, after)| after)
        .filter(|after| after.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

fn run_naming_command(ctx: &Ctx, config: &WorktreeNamingConfig, prompt: &str) -> Result<String> {
    let mut parts = shell_words::split(&config.command)
        .with_context(|| format!("Failed to parse issue naming command: {}", config.command))?;
    if parts.is_empty() {
        bail!("worktree.naming.command cannot be empty");
    }

    let cmd = parts.remove(0);
    parts.push(prompt.to_string());
    let args = parts.iter().map(String::as_str).collect::<Vec<_>>();
    let out = ctx.runner.run(&cmd, &args, Some(&ctx.repo_root))?;
    if !out.success {
        bail!(
            "Issue naming command failed: {}",
            if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            }
        );
    }
    if out.stdout.trim().is_empty() {
        bail!("Issue naming command returned empty output");
    }
    Ok(out.stdout)
}

fn parse_generated_vars(stdout: &str) -> Result<HashMap<String, String>> {
    let trimmed = stdout.trim();
    let value = serde_json::from_str::<Value>(trimmed)
        .or_else(|_| {
            let start = trimmed.find('{');
            let end = trimmed.rfind('}');
            match (start, end) {
                (Some(start), Some(end)) if start <= end => {
                    serde_json::from_str::<Value>(&trimmed[start..=end])
                }
                _ => serde_json::from_str::<Value>(trimmed),
            }
        })
        .with_context(|| format!("Issue naming command did not return JSON: {trimmed}"))?;

    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Issue naming command must return a JSON object"))?;

    Ok(object
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|s| (key.clone(), s.trim().to_string())))
        .filter(|(_, value)| !value.is_empty())
        .collect())
}

fn render_optional_template(
    label: &str,
    template_value: Option<&str>,
    vars: &HashMap<String, String>,
) -> Result<Option<String>> {
    let Some(template_value) = template_value else {
        return Ok(None);
    };

    let rendered = template::render(template_value, vars);
    if rendered.contains("{{") {
        bail!("worktree.naming.{label} has unresolved variables: {rendered}");
    }
    Ok(Some(rendered))
}

fn sanitize_branch_name(input: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    let mut previous_slash = false;

    for ch in input.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            previous_dash = false;
            previous_slash = false;
            continue;
        }

        if ch == '/' {
            trim_branch_separators(&mut out);
            if !out.is_empty() && !previous_slash {
                out.push('/');
                previous_slash = true;
                previous_dash = false;
            }
            continue;
        }

        if matches!(ch, '-' | '_' | '.') || ch.is_whitespace() {
            if !out.is_empty() && !previous_dash && !previous_slash {
                out.push('-');
                previous_dash = true;
            }
            continue;
        }

        if !out.is_empty() && !previous_dash && !previous_slash {
            out.push('-');
            previous_dash = true;
        }
    }

    trim_branch_separators(&mut out);
    out
}

fn trim_branch_separators(value: &mut String) {
    while value.ends_with(['-', '/', '.', '_']) {
        value.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, IssueProviderType, IssuesConfig};
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;

    fn config_with_naming(naming: WorktreeNamingConfig) -> Config {
        Config {
            worktree: crate::config::WorktreeConfig {
                naming: Some(naming),
                ..Default::default()
            },
            issues: Some(IssuesConfig {
                provider: IssueProviderType::Linear,
                gh_user: None,
                origin_policy: Default::default(),
            }),
            ..Config::default()
        }
    }

    #[test]
    fn generate_renders_prompt_and_templates() {
        let mut runner = MockRunner::new();
        runner.add_response(r#"{"english_slug":"validate-document-title"}"#, true);
        let config = config_with_naming(WorktreeNamingConfig::default());
        let ctx = Ctx::new(
            PathBuf::from("/tmp/sample-app"),
            PathBuf::from("/tmp/sample-app"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        );

        let result = generate(
            &ctx,
            "PROJ-680",
            "Validate document title",
            Some("alice/proj-680-document-title"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            result.branch.as_deref(),
            Some("alice/proj-680-validate-document-title")
        );
        assert!(result.workspace.is_none());
        assert_eq!(
            result.vars.get("english_slug").unwrap(),
            "validate-document-title"
        );
    }

    #[test]
    fn generate_is_disabled_without_config() {
        let ctx = Ctx::new(
            PathBuf::from("/tmp/sample-app"),
            PathBuf::from("/tmp/sample-app"),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
        );

        assert!(generate(&ctx, "PROJ-1", "Title", None).unwrap().is_none());
    }

    #[test]
    fn parse_generated_vars_accepts_json_inside_text() {
        let vars = parse_generated_vars(
            "```json\n{\"english_title\":\"Title\",\"english_slug\":\"title\"}\n```",
        )
        .unwrap();
        assert_eq!(vars.get("english_title").unwrap(), "Title");
        assert_eq!(vars.get("english_slug").unwrap(), "title");
    }

    #[test]
    fn sanitize_branch_name_keeps_prefix_and_ascii_slug() {
        assert_eq!(
            sanitize_branch_name("Alice/PROJ-680 Document title!!"),
            "alice/proj-680-document-title"
        );
    }

    #[test]
    fn publish_task_branch_adds_issue_key_to_existing_slug() {
        assert_eq!(
            publish_task_branch("PROJ-123", None, "add-profile-docs").unwrap(),
            "proj-123-add-profile-docs"
        );
    }

    #[test]
    fn publish_task_branch_uses_final_existing_branch_segment() {
        assert_eq!(
            publish_task_branch("PROJ-123", None, "alice/add-profile-docs").unwrap(),
            "proj-123-add-profile-docs"
        );
    }

    #[test]
    fn publish_task_branch_avoids_duplicate_issue_key_prefix() {
        assert_eq!(
            publish_task_branch("PROJ-123", None, "proj-123-add-profile-docs").unwrap(),
            "proj-123-add-profile-docs"
        );
    }

    #[test]
    fn publish_task_branch_keeps_provider_prefix_from_suggested_branch() {
        assert_eq!(
            publish_task_branch(
                "PROJ-123",
                Some("provider/proj-123-suggested-branch"),
                "alice/add-profile-docs"
            )
            .unwrap(),
            "provider/proj-123-add-profile-docs"
        );
    }
}
