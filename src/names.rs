use regex::Regex;
use std::path::{Path, PathBuf};

pub struct WorktreeNames {
    pub path: PathBuf,
    pub workspace: String,
    pub site: Option<String>,
}

impl WorktreeNames {
    /// Build all three names from branch, repo info, and optional title.
    /// `site_template` comes from config (e.g. `"{{repo}}-{{tech_id}}"`), None if no [herd].
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
        Self { path, workspace, site }
    }

    fn build_path(branch: &str, parent_dir: &Path, repo_name: &str) -> PathBuf {
        let sanitized = branch.replace('/', "-");
        parent_dir.join(format!("{}-{}", repo_name, sanitized))
    }

    pub fn build_workspace_name(branch: &str, title: Option<&str>) -> String {
        match title {
            Some(t) => {
                let re = Regex::new(r"^(C\d+S\d+)\.\s*(.*)").unwrap();
                if let Some(caps) = re.captures(t) {
                    format!("{} ({})", &caps[2], &caps[1])
                } else {
                    t.to_string()
                }
            }
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
        let name = match branch.rsplit_once('/') {
            Some((_, after)) => after,
            None => branch,
        };

        if let Some(tech_id) = Self::extract_tech_id(branch) {
            return format!("{}-{}", repo_name, tech_id);
        }

        let ascii: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let re = Regex::new(r"-{2,}").unwrap();
        let collapsed = re.replace_all(&ascii, "-");
        let trimmed = collapsed.trim_matches('-');
        format!("{}-{}", repo_name, trimmed)
    }

    pub fn extract_tech_id(branch: &str) -> Option<String> {
        let name = match branch.rsplit_once('/') {
            Some((_, after)) => after,
            None => branch,
        };
        let re = Regex::new(r"^(tech-\d+)").unwrap();
        re.captures(name).map(|caps| caps[1].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_replaces_slash_with_hyphen() {
        let p = WorktreeNames::build_path(
            "hoetaek/tech-680-feature",
            Path::new("/home/dev/projects"),
            "hapjeong",
        );
        assert_eq!(
            p,
            PathBuf::from("/home/dev/projects/hapjeong-hoetaek-tech-680-feature")
        );
    }

    #[test]
    fn workspace_name_from_title_moves_chapter_prefix() {
        let name = WorktreeNames::build_workspace_name(
            "hoetaek/tech-680-c11s09-위키",
            Some("C11S09. 위키 에디터는 문서 제목의 유효성 검사를 받을 수 있다"),
        );
        assert_eq!(
            name,
            "위키 에디터는 문서 제목의 유효성 검사를 받을 수 있다 (C11S09)"
        );
    }

    #[test]
    fn workspace_name_from_title_without_chapter_prefix() {
        let name = WorktreeNames::build_workspace_name(
            "hoetaek/tech-680-feature",
            Some("Add user authentication"),
        );
        assert_eq!(name, "Add user authentication");
    }

    #[test]
    fn workspace_name_from_branch_strips_user_prefix() {
        let name = WorktreeNames::build_workspace_name("hoetaek/tech-680-위키-에디터", None);
        assert_eq!(name, "tech 680 위키 에디터");
    }

    #[test]
    fn workspace_name_from_branch_without_user_prefix() {
        let name = WorktreeNames::build_workspace_name("feature-branch", None);
        assert_eq!(name, "feature branch");
    }

    #[test]
    fn site_name_extracts_tech_id() {
        let name =
            WorktreeNames::build_site_name("hoetaek/tech-680-c11s09-위키-에디터", "hapjeong");
        assert_eq!(name, "hapjeong-tech-680");
    }

    #[test]
    fn site_name_strips_non_ascii() {
        let name = WorktreeNames::build_site_name("hoetaek/my-feature-위키", "hapjeong");
        assert_eq!(name, "hapjeong-my-feature");
    }

    #[test]
    fn site_name_collapses_consecutive_hyphens() {
        let name = WorktreeNames::build_site_name("hoetaek/a--b---c", "hapjeong");
        assert_eq!(name, "hapjeong-a-b-c");
    }

    #[test]
    fn extracts_tech_id_from_branch() {
        assert_eq!(
            WorktreeNames::extract_tech_id("hoetaek/tech-680-c11s09-위키"),
            Some("tech-680".into())
        );
    }

    #[test]
    fn no_tech_id_returns_none() {
        assert_eq!(WorktreeNames::extract_tech_id("hoetaek/my-feature"), None);
    }

    #[test]
    fn new_builds_all_names_with_herd() {
        let names = WorktreeNames::new(
            "hoetaek/tech-680-c11s09-위키",
            Path::new("/home/dev/projects"),
            "hapjeong",
            Some("C11S09. 위키 에디터"),
            Some("{{repo}}-{{tech_id}}"),
        );
        assert_eq!(
            names.path,
            PathBuf::from("/home/dev/projects/hapjeong-hoetaek-tech-680-c11s09-위키")
        );
        assert_eq!(names.workspace, "위키 에디터 (C11S09)");
        assert_eq!(names.site.as_deref(), Some("hapjeong-tech-680"));
    }

    #[test]
    fn new_without_herd_config() {
        let names = WorktreeNames::new(
            "hoetaek/my-feature",
            Path::new("/tmp"),
            "myrepo",
            None,
            None,
        );
        assert_eq!(names.path, PathBuf::from("/tmp/myrepo-hoetaek-my-feature"));
        assert_eq!(names.workspace, "my feature");
        assert!(names.site.is_none());
    }
}
