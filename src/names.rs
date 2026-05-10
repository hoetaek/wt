use crate::template;
use anyhow::{Result, bail};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct WorktreeNames {
    pub path: PathBuf,
    pub branch: String,
    pub workspace: String,
    pub site: Option<String>,
}

impl WorktreeNames {
    /// Build all three names from branch, repo info, and optional title.
    /// `site_template` comes from config, None if no site provider is configured.
    pub fn new(
        branch: &str,
        parent_dir: &Path,
        repo_name: &str,
        title: Option<&str>,
        site_template: Option<&str>,
    ) -> Self {
        let path = Self::build_path(branch, parent_dir, repo_name);
        let workspace = Self::build_workspace_name(branch, title);
        let site = site_template.map(|_| Self::build_site_name(branch, repo_name));
        Self {
            path,
            branch: branch.to_string(),
            workspace,
            site,
        }
    }

    pub fn new_with_workspace(
        branch: &str,
        parent_dir: &Path,
        repo_name: &str,
        workspace: Option<&str>,
        site_template: Option<&str>,
    ) -> Self {
        let path = Self::build_path(branch, parent_dir, repo_name);
        let workspace = workspace
            .map(str::to_string)
            .unwrap_or_else(|| Self::build_workspace_name(branch, None));
        let site = site_template.map(|_| Self::build_site_name(branch, repo_name));
        Self {
            path,
            branch: branch.to_string(),
            workspace,
            site,
        }
    }

    pub fn new_with_config(
        branch: &str,
        parent_dir: &Path,
        repo_root: &Path,
        repo_name: &str,
        title: Option<&str>,
        site_template: Option<&str>,
        path_template: Option<&str>,
    ) -> Result<Self> {
        let path =
            Self::build_configured_path(branch, parent_dir, repo_root, repo_name, path_template)?;
        let workspace = Self::build_workspace_name(branch, title);
        let site = site_template.map(|_| Self::build_site_name(branch, repo_name));
        Ok(Self {
            path,
            branch: branch.to_string(),
            workspace,
            site,
        })
    }

    pub fn new_with_workspace_config(
        branch: &str,
        parent_dir: &Path,
        repo_root: &Path,
        repo_name: &str,
        workspace: Option<&str>,
        site_template: Option<&str>,
        path_template: Option<&str>,
    ) -> Result<Self> {
        let path =
            Self::build_configured_path(branch, parent_dir, repo_root, repo_name, path_template)?;
        let workspace = workspace
            .map(str::to_string)
            .unwrap_or_else(|| Self::build_workspace_name(branch, None));
        let site = site_template.map(|_| Self::build_site_name(branch, repo_name));
        Ok(Self {
            path,
            branch: branch.to_string(),
            workspace,
            site,
        })
    }

    fn build_path(branch: &str, parent_dir: &Path, repo_name: &str) -> PathBuf {
        let sanitized = branch.replace('/', "-");
        parent_dir.join(format!("{repo_name}-{sanitized}"))
    }

    fn build_configured_path(
        branch: &str,
        parent_dir: &Path,
        repo_root: &Path,
        repo_name: &str,
        path_template: Option<&str>,
    ) -> Result<PathBuf> {
        let default_path = Self::build_path(branch, parent_dir, repo_name);
        let Some(path_template) = path_template else {
            return Ok(default_path);
        };

        let vars =
            Self::path_template_vars(branch, parent_dir, repo_root, repo_name, &default_path);
        let rendered = template::render(path_template, &vars);
        if rendered.contains("{{") {
            bail!("worktree.path has unresolved variables: {rendered}");
        }

        let expanded = expand_home_and_env(&rendered)?;
        if expanded.trim().is_empty() {
            bail!("worktree.path cannot be empty");
        }

        let path = PathBuf::from(expanded);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(repo_root.join(path))
        }
    }

    fn path_template_vars(
        branch: &str,
        parent_dir: &Path,
        repo_root: &Path,
        repo_name: &str,
        default_path: &Path,
    ) -> HashMap<String, String> {
        let branch_sanitized = branch.replace('/', "-");
        let default_name = format!("{repo_name}-{branch_sanitized}");

        HashMap::from([
            ("repo".into(), repo_name.to_string()),
            ("repo_name".into(), repo_name.to_string()),
            ("branch".into(), branch.to_string()),
            ("branch_sanitized".into(), branch_sanitized),
            ("branch_slug".into(), Self::build_branch_slug(branch)),
            ("default_name".into(), default_name),
            (
                "default_path".into(),
                default_path.to_string_lossy().into_owned(),
            ),
            (
                "parent_dir".into(),
                parent_dir.to_string_lossy().into_owned(),
            ),
            ("repo_root".into(), repo_root.to_string_lossy().into_owned()),
        ])
    }

    pub fn build_workspace_name(branch: &str, title: Option<&str>) -> String {
        match title {
            Some(t) => t.to_string(),
            None => {
                let name = match branch.rsplit_once('/') {
                    Some((_, after)) => after,
                    None => branch,
                };
                name.replace('-', " ")
            }
        }
    }

    pub fn build_site_name(branch: &str, repo_name: &str) -> String {
        format!("{repo_name}-{}", Self::build_branch_slug(branch))
    }

    pub fn build_branch_slug(branch: &str) -> String {
        let name = match branch.rsplit_once('/') {
            Some((_, after)) => after,
            None => branch,
        };

        let ascii: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let re = Regex::new(r"-{2,}").unwrap();
        let collapsed = re.replace_all(&ascii, "-");
        let trimmed = collapsed.trim_matches('-');
        if trimmed.is_empty() {
            "worktree".into()
        } else {
            trimmed.into()
        }
    }

    pub fn extract_issue_slug(branch: &str) -> Option<String> {
        let name = match branch.rsplit_once('/') {
            Some((_, after)) => after,
            None => branch,
        };
        let re = Regex::new(r"(?i)^([a-z][a-z0-9]*-\d+)(?:-|$)").unwrap();
        re.captures(name).map(|caps| caps[1].to_ascii_lowercase())
    }

    pub fn extract_issue_key(branch: &str) -> Option<String> {
        Self::extract_issue_slug(branch).map(|slug| slug.to_ascii_uppercase())
    }
}

fn expand_home_and_env(input: &str) -> Result<String> {
    let input = expand_home(input)?;
    expand_env_vars(&input)
}

fn expand_home(input: &str) -> Result<String> {
    if input == "~" || input.starts_with("~/") {
        let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
        if input == "~" {
            return Ok(home);
        }
        return Ok(format!("{home}/{}", &input[2..]));
    }

    Ok(input.to_string())
}

fn expand_env_vars(input: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            out.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('{') => {
                chars.next();
                let mut name = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    name.push(next);
                }
                if !closed {
                    bail!("Unclosed environment variable in worktree.path: ${{{name}");
                }
                if name.is_empty() {
                    out.push_str("${}");
                } else {
                    out.push_str(&env_var(&name)?);
                }
            }
            Some(next) if is_env_name_start(next) => {
                let mut name = String::new();
                while let Some(next) = chars.peek().copied() {
                    if is_env_name_continue(next) {
                        name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                out.push_str(&env_var(&name)?);
            }
            _ => out.push('$'),
        }
    }

    Ok(out)
}

fn env_var(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| anyhow::anyhow!("Environment variable ${name} is not set"))
}

fn is_env_name_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_env_name_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_replaces_slash_with_hyphen() {
        let p = WorktreeNames::build_path(
            "alice/proj-680-feature",
            Path::new("/home/dev/projects"),
            "sample-app",
        );
        assert_eq!(
            p,
            PathBuf::from("/home/dev/projects/sample-app-alice-proj-680-feature")
        );
    }

    #[test]
    fn configured_path_defaults_to_sibling_path() {
        let path = WorktreeNames::build_configured_path(
            "alice/proj-680-feature",
            Path::new("/home/dev/projects"),
            Path::new("/home/dev/projects/sample-app"),
            "sample-app",
            None,
        )
        .unwrap();
        assert_eq!(
            path,
            PathBuf::from("/home/dev/projects/sample-app-alice-proj-680-feature")
        );
    }

    #[test]
    fn configured_path_renders_template_relative_to_repo_root() {
        let path = WorktreeNames::build_configured_path(
            "alice/proj-680-feature",
            Path::new("/home/dev/projects"),
            Path::new("/home/dev/projects/sample-app"),
            "sample-app",
            Some("worktrees/{{default_name}}"),
        )
        .unwrap();
        assert_eq!(
            path,
            PathBuf::from(
                "/home/dev/projects/sample-app/worktrees/sample-app-alice-proj-680-feature"
            )
        );
    }

    #[test]
    fn configured_path_expands_home_env_var() {
        let home = std::env::var("HOME").unwrap();
        let path = WorktreeNames::build_configured_path(
            "alice/proj-680-feature",
            Path::new("/home/dev/projects"),
            Path::new("/home/dev/projects/sample-app"),
            "sample-app",
            Some("$HOME/worktrees/{{default_name}}"),
        )
        .unwrap();
        assert_eq!(
            path,
            PathBuf::from(home).join("worktrees/sample-app-alice-proj-680-feature")
        );
    }

    #[test]
    fn configured_path_rejects_unknown_template_variable() {
        let err = WorktreeNames::build_configured_path(
            "alice/proj-680-feature",
            Path::new("/home/dev/projects"),
            Path::new("/home/dev/projects/sample-app"),
            "sample-app",
            Some("worktrees/{{missing}}"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("unresolved variables"));
    }

    #[test]
    fn workspace_name_from_title_uses_title_as_workspace() {
        let name = WorktreeNames::build_workspace_name(
            "alice/proj-680-document-editor",
            Some("Validate document title"),
        );
        assert_eq!(name, "Validate document title");
    }

    #[test]
    fn workspace_name_from_title_without_chapter_prefix() {
        let name = WorktreeNames::build_workspace_name(
            "alice/proj-680-feature",
            Some("Add user authentication"),
        );
        assert_eq!(name, "Add user authentication");
    }

    #[test]
    fn workspace_name_from_branch_strips_user_prefix() {
        let name = WorktreeNames::build_workspace_name("alice/proj-680-document-editor", None);
        assert_eq!(name, "proj 680 document editor");
    }

    #[test]
    fn workspace_name_from_branch_without_user_prefix() {
        let name = WorktreeNames::build_workspace_name("feature-branch", None);
        assert_eq!(name, "feature branch");
    }

    #[test]
    fn site_name_uses_branch_slug() {
        let name = WorktreeNames::build_site_name("alice/proj-680-document-editor", "sample-app");
        assert_eq!(name, "sample-app-proj-680-document-editor");
    }

    #[test]
    fn site_name_strips_non_ascii() {
        let name = WorktreeNames::build_site_name("alice/my-feature-문서", "sample-app");
        assert_eq!(name, "sample-app-my-feature");
    }

    #[test]
    fn site_name_collapses_consecutive_hyphens() {
        let name = WorktreeNames::build_site_name("alice/a--b---c", "sample-app");
        assert_eq!(name, "sample-app-a-b-c");
    }

    #[test]
    fn branch_slug_falls_back_for_non_ascii_branch() {
        let slug = WorktreeNames::build_branch_slug("alice/문서-편집기");
        assert_eq!(slug, "worktree");
    }

    #[test]
    fn extracts_issue_slug_from_branch() {
        assert_eq!(
            WorktreeNames::extract_issue_slug("alice/proj-680-document-editor"),
            Some("proj-680".into())
        );
    }

    #[test]
    fn no_issue_slug_returns_none() {
        assert_eq!(WorktreeNames::extract_issue_slug("alice/my-feature"), None);
    }

    #[test]
    fn new_builds_all_names_with_site() {
        let names = WorktreeNames::new(
            "alice/proj-680-document-editor",
            Path::new("/home/dev/projects"),
            "sample-app",
            Some("Document editor"),
            Some("{{repo}}-{{branch_slug}}"),
        );
        assert_eq!(
            names.path,
            PathBuf::from("/home/dev/projects/sample-app-alice-proj-680-document-editor")
        );
        assert_eq!(names.workspace, "Document editor");
        assert_eq!(
            names.site.as_deref(),
            Some("sample-app-proj-680-document-editor")
        );
    }

    #[test]
    fn site_name_uses_canonical_repo_prefix_for_nested_worktree_branch() {
        let names = WorktreeNames::new(
            "alice/proj-672-nested-worktree-bug",
            Path::new("sample-app"),
            "sample-app",
            None,
            Some("{{repo}}-{{branch_slug}}"),
        );

        assert_eq!(
            names.site.as_deref(),
            Some("sample-app-proj-672-nested-worktree-bug")
        );
    }

    #[test]
    fn new_without_site_config() {
        let names = WorktreeNames::new("alice/my-feature", Path::new("/tmp"), "myrepo", None, None);
        assert_eq!(names.path, PathBuf::from("/tmp/myrepo-alice-my-feature"));
        assert_eq!(names.workspace, "my feature");
        assert!(names.site.is_none());
    }

    #[test]
    fn new_with_workspace_uses_exact_workspace() {
        let names = WorktreeNames::new_with_workspace(
            "alice/proj-680-validate-document-title",
            Path::new("/tmp"),
            "myrepo",
            Some("Validate document title"),
            None,
        );
        assert_eq!(names.workspace, "Validate document title");
        assert_eq!(
            names.path,
            PathBuf::from("/tmp/myrepo-alice-proj-680-validate-document-title")
        );
    }
}
