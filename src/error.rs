use thiserror::Error;

#[derive(Debug, Error)]
pub enum WtError {
    #[error("Not in a git repository")]
    NotGitRepo,

    #[error("No branch name for {identifier}")]
    NoBranchName { identifier: String },

    #[error("Branch '{branch}' already exists. --base is not applicable")]
    BranchExistsWithBase { branch: String },

    #[error("Command '{cmd}' not found")]
    MissingCommand { cmd: String },

    #[error("User cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_displays_correctly() {
        let err = WtError::Cancelled;
        assert_eq!(err.to_string(), "User cancelled");
    }

    #[test]
    fn no_branch_name_includes_identifier() {
        let err = WtError::NoBranchName {
            identifier: "TECH-680".into(),
        };
        assert_eq!(err.to_string(), "No branch name for TECH-680");
    }

    #[test]
    fn branch_exists_with_base_includes_branch() {
        let err = WtError::BranchExistsWithBase {
            branch: "alice/tech-680".into(),
        };
        assert_eq!(
            err.to_string(),
            "Branch 'alice/tech-680' already exists. --base is not applicable"
        );
    }
}
