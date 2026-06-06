#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueOperation {
    Read,
    Create,
    EnsureBranch,
    UpdateFields,
    Comment,
    LifecycleHook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueCapabilities {
    read: bool,
    create: bool,
    ensure_branch: bool,
    update_fields: bool,
    comment: bool,
    lifecycle_hook: bool,
}

impl IssueCapabilities {
    pub fn reader_only() -> Self {
        Self {
            read: true,
            create: false,
            ensure_branch: false,
            update_fields: false,
            comment: false,
            lifecycle_hook: false,
        }
    }

    pub fn commenter_only() -> Self {
        Self {
            comment: true,
            ..Self::reader_only()
        }
    }

    pub fn updater_only() -> Self {
        Self {
            update_fields: true,
            ..Self::reader_only()
        }
    }

    pub fn full() -> Self {
        Self {
            read: true,
            create: true,
            ensure_branch: true,
            update_fields: true,
            comment: true,
            lifecycle_hook: true,
        }
    }

    pub fn provider_default() -> Self {
        Self {
            update_fields: false,
            comment: false,
            ..Self::full()
        }
    }

    pub fn can_read(&self) -> bool {
        self.read
    }

    pub fn can_comment(&self) -> bool {
        self.comment
    }

    pub fn can_update_fields(&self) -> bool {
        self.update_fields
    }

    pub fn supports(&self, operation: IssueOperation) -> bool {
        match operation {
            IssueOperation::Read => self.read,
            IssueOperation::Create => self.create,
            IssueOperation::EnsureBranch => self.ensure_branch,
            IssueOperation::UpdateFields => self.update_fields,
            IssueOperation::Comment => self.comment,
            IssueOperation::LifecycleHook => self.lifecycle_hook,
        }
    }

    pub fn disabled_reason(&self, operation: IssueOperation) -> Option<&'static str> {
        if self.supports(operation) {
            return None;
        }

        Some(match operation {
            IssueOperation::Read => "provider does not support reading issue details",
            IssueOperation::Create => "provider does not support creating issues",
            IssueOperation::EnsureBranch => "provider does not support ensuring issue branches",
            IssueOperation::UpdateFields => "provider does not support updating issue title/body",
            IssueOperation::Comment => "provider does not support creating issue comments",
            IssueOperation::LifecycleHook => "provider does not support issue lifecycle hooks",
        })
    }

    pub fn disabled_write_reasons(&self) -> Vec<&'static str> {
        [IssueOperation::UpdateFields, IssueOperation::Comment]
            .into_iter()
            .filter_map(|operation| self.disabled_reason(operation))
            .collect()
    }
}

#[cfg(test)]
use super::IssueFieldUpdate;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_only_capabilities_explain_disabled_writes() {
        let capabilities = IssueCapabilities::reader_only();

        assert!(capabilities.can_read());
        assert!(!capabilities.can_comment());
        assert!(!capabilities.can_update_fields());
        assert_eq!(
            capabilities
                .disabled_reason(IssueOperation::Comment)
                .unwrap(),
            "provider does not support creating issue comments"
        );
        assert_eq!(
            capabilities
                .disabled_reason(IssueOperation::UpdateFields)
                .unwrap(),
            "provider does not support updating issue title/body"
        );
    }

    #[test]
    fn full_capabilities_enable_comment_and_field_update() {
        let capabilities = IssueCapabilities::full();

        assert!(capabilities.can_comment());
        assert!(capabilities.can_update_fields());
        assert!(
            capabilities
                .disabled_reason(IssueOperation::Comment)
                .is_none()
        );
        assert!(
            capabilities
                .disabled_reason(IssueOperation::UpdateFields)
                .is_none()
        );
    }

    #[test]
    fn issue_field_update_keeps_title_and_body_optional() {
        let update = IssueFieldUpdate {
            title: Some("New title".into()),
            body: None,
        };

        assert_eq!(update.title.as_deref(), Some("New title"));
        assert!(update.body.is_none());
    }

    #[test]
    fn unsupported_provider_write_action_reasons_are_stable() {
        let capabilities = IssueCapabilities::reader_only();
        let reasons = capabilities.disabled_write_reasons();

        assert_eq!(
            reasons,
            vec![
                "provider does not support updating issue title/body",
                "provider does not support creating issue comments",
            ]
        );
    }
}
