use crate::origin_snapshot::origin_label;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginAction {
    Diff,
    Fetch,
    Pull,
    Push,
    Publish,
    Attach,
    KeepLocal,
    Archive,
    OpenInBrowser,
    CopyReference,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginActionMenu {
    title: String,
    items: Vec<OriginActionItem>,
    child_origins: Vec<ChildOriginLabel>,
}

impl OriginActionMenu {
    pub fn for_local_task(_key: impl Into<String>, title: impl Into<String>) -> Self {
        let no_origin_reason = "no origin attached";
        Self {
            title: title.into(),
            items: vec![
                OriginActionItem::enabled(OriginAction::Publish, "Publish as issue", "pub")
                    .external_write(),
                OriginActionItem::enabled(OriginAction::Attach, "Attach existing issue", "A"),
                OriginActionItem::enabled(
                    OriginAction::KeepLocal,
                    "Keep local-only for this task",
                    "L",
                ),
                OriginActionItem::enabled(OriginAction::Archive, "Archive task", "a"),
                OriginActionItem::enabled(OriginAction::CopyReference, "Copy task key", "y"),
                OriginActionItem::disabled(
                    OriginAction::Diff,
                    "Diff with issue",
                    "d",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::Fetch,
                    "Fetch origin",
                    "f",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::Pull,
                    "Pull from issue",
                    "p",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::Push,
                    "Push to issue",
                    "P",
                    no_origin_reason,
                )
                .external_write(),
            ],
            child_origins: Vec::new(),
        }
    }

    pub fn for_origin_task(
        _key: impl Into<String>,
        title: impl Into<String>,
        origin: OriginLabel,
    ) -> Self {
        let origin = origin.render_plain();
        Self {
            title: title.into(),
            items: vec![
                OriginActionItem::enabled(OriginAction::Diff, "Diff with issue", "d"),
                OriginActionItem::enabled(OriginAction::Fetch, "Fetch origin", "f"),
                OriginActionItem::enabled(OriginAction::Pull, "Pull from issue", "p"),
                OriginActionItem::enabled(OriginAction::Push, "Push to issue", "P")
                    .external_write(),
                OriginActionItem::enabled(
                    OriginAction::OpenInBrowser,
                    "Open issue in browser",
                    "o",
                ),
                OriginActionItem::enabled(
                    OriginAction::CopyReference,
                    "Copy origin reference",
                    "y",
                ),
                OriginActionItem::enabled(OriginAction::Archive, "Archive task", "a"),
                OriginActionItem::disabled(
                    OriginAction::Attach,
                    "Attach different origin",
                    "A",
                    "origin replacement not supported",
                ),
                OriginActionItem::disabled(
                    OriginAction::Publish,
                    "Publish as issue",
                    "pub",
                    format!("already has origin {origin}"),
                )
                .external_write(),
            ],
            child_origins: Vec::new(),
        }
    }

    pub fn for_origin_issue_placeholder(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            items: vec![OriginActionItem::disabled(
                OriginAction::Attach,
                "Import as task",
                "i",
                "import is handled by a follow-up task",
            )],
            child_origins: Vec::new(),
        }
    }

    pub fn for_workflow(
        _id: impl Into<String>,
        title: impl Into<String>,
        origin: Option<OriginLabel>,
        child_origins: Vec<ChildOriginLabel>,
    ) -> Self {
        let items = if let Some(_origin) = origin {
            vec![
                OriginActionItem::enabled(OriginAction::Diff, "Diff workflow with issue", "d"),
                OriginActionItem::enabled(OriginAction::Fetch, "Fetch workflow origin", "f"),
                OriginActionItem::enabled(OriginAction::Pull, "Pull selected workflow fields", "p"),
                OriginActionItem::enabled(OriginAction::Push, "Push selected workflow fields", "P")
                    .external_write(),
                OriginActionItem::enabled(
                    OriginAction::OpenInBrowser,
                    "Open workflow issue in browser",
                    "o",
                ),
                OriginActionItem::enabled(
                    OriginAction::CopyReference,
                    "Copy workflow origin reference",
                    "y",
                ),
                OriginActionItem::enabled(
                    OriginAction::Attach,
                    "Attach different workflow origin",
                    "A",
                ),
            ]
        } else {
            let no_origin_reason = "no workflow origin attached";
            vec![
                OriginActionItem::enabled(
                    OriginAction::Attach,
                    "Attach existing workflow issue",
                    "A",
                ),
                OriginActionItem::disabled(
                    OriginAction::Diff,
                    "Diff workflow with issue",
                    "d",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::Fetch,
                    "Fetch workflow origin",
                    "f",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::Pull,
                    "Pull selected workflow fields",
                    "p",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::Push,
                    "Push selected workflow fields",
                    "P",
                    no_origin_reason,
                )
                .external_write(),
                OriginActionItem::disabled(
                    OriginAction::OpenInBrowser,
                    "Open workflow issue in browser",
                    "o",
                    no_origin_reason,
                ),
                OriginActionItem::disabled(
                    OriginAction::CopyReference,
                    "Copy workflow origin reference",
                    "y",
                    no_origin_reason,
                ),
            ]
        };

        Self {
            title: title.into(),
            items,
            child_origins,
        }
    }

    pub fn enabled(&self, label: &str) -> bool {
        self.items
            .iter()
            .find(|item| item.label == label)
            .is_some_and(|item| item.enabled)
    }

    pub fn disabled_reason(&self, label: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|item| item.label == label)
            .and_then(|item| item.disabled_reason.as_deref())
    }

    pub fn action_for(&self, label: &str) -> Option<OriginAction> {
        self.items
            .iter()
            .find(|item| item.label == label)
            .map(|item| item.action)
    }

    pub fn enabled_action(&self, action: OriginAction) -> Option<&OriginActionItem> {
        self.items
            .iter()
            .find(|item| item.enabled && item.action == action)
    }

    pub fn action_for_shortcut(&self, shortcut: &str) -> Option<OriginAction> {
        self.items
            .iter()
            .find(|item| item.enabled && item.shortcut == shortcut)
            .map(|item| item.action)
    }

    pub fn items(&self) -> &[OriginActionItem] {
        &self.items
    }

    pub fn item(&self, index: usize) -> Option<&OriginActionItem> {
        self.items.get(index)
    }

    pub fn first_enabled_index(&self) -> Option<usize> {
        self.items.iter().position(|item| item.enabled)
    }

    pub fn render_plain(&self) -> String {
        let mut lines = vec![format!("Actions: {}", self.title)];

        let enabled = self
            .items
            .iter()
            .filter(|item| item.enabled)
            .collect::<Vec<_>>();
        if !enabled.is_empty() {
            lines.push(String::new());
            for item in enabled {
                lines.push(item.render_plain());
            }
        }

        let disabled = self
            .items
            .iter()
            .filter(|item| !item.enabled)
            .collect::<Vec<_>>();
        if !disabled.is_empty() {
            lines.push(String::new());
            lines.push("disabled:".into());
            for item in disabled {
                lines.push(item.render_plain());
            }
        }

        if !self.child_origins.is_empty() {
            lines.push(String::new());
            lines.push("child task actions:".into());
            for child in &self.child_origins {
                lines.push(format!(
                    "Enter on child {} to inspect task origin {}",
                    child.task,
                    child.origin_label()
                ));
            }
        }

        lines.join("\n")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginActionItem {
    action: OriginAction,
    label: String,
    shortcut: String,
    enabled: bool,
    disabled_reason: Option<String>,
    external_write: bool,
}

impl OriginActionItem {
    pub fn enabled(
        action: OriginAction,
        label: impl Into<String>,
        shortcut: impl Into<String>,
    ) -> Self {
        Self {
            action,
            label: label.into(),
            shortcut: shortcut.into(),
            enabled: true,
            disabled_reason: None,
            external_write: false,
        }
    }

    pub fn disabled(
        action: OriginAction,
        label: impl Into<String>,
        shortcut: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            action,
            label: label.into(),
            shortcut: shortcut.into(),
            enabled: false,
            disabled_reason: Some(reason.into()),
            external_write: false,
        }
    }

    pub fn external_write(mut self) -> Self {
        self.external_write = true;
        self
    }

    pub fn shortcut(&self) -> &str {
        &self.shortcut
    }

    pub fn action(&self) -> OriginAction {
        self.action
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn disabled_reason(&self) -> Option<&str> {
        self.disabled_reason.as_deref()
    }

    pub fn is_external_write(&self) -> bool {
        self.external_write
    }

    pub fn render_plain(&self) -> String {
        let mut parts = vec![self.shortcut.clone(), self.label.clone()];
        if let Some(reason) = &self.disabled_reason {
            parts.push(reason.clone());
        }
        if self.external_write {
            parts.push("External write; confirmation required".into());
        }
        format!("  {}", parts.join("  "))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginLabel {
    provider: String,
    id: String,
}

impl OriginLabel {
    pub fn new(provider: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            id: id.into(),
        }
    }

    pub fn render_plain(&self) -> String {
        origin_label(&self.provider, &self.id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildOriginLabel {
    task: String,
    origin: Option<OriginLabel>,
    label: Option<String>,
}

impl ChildOriginLabel {
    pub fn new(task: impl Into<String>, origin: Option<OriginLabel>) -> Self {
        Self {
            task: task.into(),
            origin,
            label: None,
        }
    }

    pub fn with_label(task: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            origin: None,
            label: Some(label.into()),
        }
    }

    fn origin_label(&self) -> String {
        self.origin
            .as_ref()
            .map(OriginLabel::render_plain)
            .or_else(|| self.label.clone())
            .unwrap_or_else(|| "not published".into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginActionPreview {
    kind: OriginActionPreviewKind,
    owner: String,
    origin: Option<OriginLabel>,
    operations: Vec<PreviewOperation>,
    pub external_write: bool,
}

impl OriginActionPreview {
    pub fn diff(
        owner: impl Into<String>,
        origin: OriginLabel,
        operations: Vec<PreviewOperation>,
    ) -> Self {
        Self::new(
            OriginActionPreviewKind::Diff,
            owner,
            Some(origin),
            operations,
            false,
        )
    }

    pub fn pull_selected_fields(
        owner: impl Into<String>,
        origin: OriginLabel,
        operations: Vec<PreviewOperation>,
    ) -> Self {
        Self::new(
            OriginActionPreviewKind::PullSelectedFields,
            owner,
            Some(origin),
            operations,
            false,
        )
    }

    pub fn push_task(
        task: impl Into<String>,
        origin: OriginLabel,
        operations: Vec<PreviewOperation>,
    ) -> Self {
        Self::new(
            OriginActionPreviewKind::PushSelectedFields,
            task,
            Some(origin),
            operations,
            true,
        )
    }

    pub fn push_workflow(
        workflow: impl Into<String>,
        origin: OriginLabel,
        operations: Vec<PreviewOperation>,
    ) -> Self {
        Self::new(
            OriginActionPreviewKind::PushSelectedFields,
            workflow,
            Some(origin),
            operations,
            true,
        )
    }

    pub fn attach(
        owner: impl Into<String>,
        origin: OriginLabel,
        operations: Vec<PreviewOperation>,
    ) -> Self {
        Self::new(
            OriginActionPreviewKind::Attach,
            owner,
            Some(origin),
            operations,
            false,
        )
    }

    pub fn publish_task(task: impl Into<String>, operations: Vec<PreviewOperation>) -> Self {
        Self::new(
            OriginActionPreviewKind::Publish,
            task,
            None,
            operations,
            true,
        )
    }

    fn new(
        kind: OriginActionPreviewKind,
        owner: impl Into<String>,
        origin: Option<OriginLabel>,
        operations: Vec<PreviewOperation>,
        external_write: bool,
    ) -> Self {
        Self {
            kind,
            owner: owner.into(),
            origin,
            operations,
            external_write,
        }
    }

    pub fn render_plain(&self) -> String {
        let mut lines = vec![format!("{}: {}", self.kind.title(), self.owner)];
        if let Some(origin) = &self.origin {
            lines.push(format!("Origin {}", origin.render_plain()));
        }
        if self.external_write {
            lines.push("External write: provider issue update; confirmation required".into());
        }
        if !self.operations.is_empty() {
            lines.push("Operations:".into());
            for operation in &self.operations {
                lines.push(format!("- {}", operation.render_plain()));
            }
        }
        lines.join("\n")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginActionPreviewKind {
    Diff,
    PullSelectedFields,
    PushSelectedFields,
    Attach,
    Publish,
}

impl OriginActionPreviewKind {
    fn title(self) -> &'static str {
        match self {
            Self::Diff => "Diff with origin",
            Self::PullSelectedFields => "Pull selected fields",
            Self::PushSelectedFields => "Push selected fields",
            Self::Attach => "Attach origin",
            Self::Publish => "Publish as issue",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewOperation {
    action: String,
    target: String,
    value: Option<String>,
}

impl PreviewOperation {
    pub fn compare_field(field: impl Into<String>) -> Self {
        Self::new("compare field", field, None::<String>)
    }

    pub fn apply_field(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new("apply field", field, Some(value.into()))
    }

    pub fn append_comment(comment: impl Into<String>) -> Self {
        Self::new("append comment", comment, None::<String>)
    }

    pub fn overwrite_field(field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new("overwrite field", field, Some(value.into()))
    }

    pub fn attach_origin(origin: OriginLabel) -> Self {
        Self::new("attach origin", origin.render_plain(), None::<String>)
    }

    pub fn create_issue(title: impl Into<String>) -> Self {
        Self::new("create issue", title, None::<String>)
    }

    fn new(action: impl Into<String>, target: impl Into<String>, value: Option<String>) -> Self {
        Self {
            action: action.into(),
            target: target.into(),
            value,
        }
    }

    fn render_plain(&self) -> String {
        if let Some(value) = &self.value {
            format!("{} {} -> {}", self.action, self.target, value)
        } else {
            format!("{} {}", self.action, self.target)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_item_carries_a_typed_action() {
        let local = OriginActionMenu::for_local_task("scratch-clean", "Scratch cleanup");
        assert_eq!(
            local.action_for("Publish as issue"),
            Some(OriginAction::Publish)
        );
        assert_eq!(
            local.action_for("Attach existing issue"),
            Some(OriginAction::Attach)
        );
        assert_eq!(
            local.action_for("Keep local-only for this task"),
            Some(OriginAction::KeepLocal)
        );
        assert_eq!(
            local.action_for("Archive task"),
            Some(OriginAction::Archive)
        );
        assert_eq!(
            local.action_for("Copy task key"),
            Some(OriginAction::CopyReference)
        );
        assert_eq!(
            local.action_for("Diff with issue"),
            Some(OriginAction::Diff)
        );

        let origin = OriginActionMenu::for_origin_task(
            "origin-sync-tui",
            "Origin sync TUI",
            OriginLabel::new("linear", "WT-142"),
        );
        assert_eq!(origin.action_for("Fetch origin"), Some(OriginAction::Fetch));
        assert_eq!(
            origin.action_for("Pull from issue"),
            Some(OriginAction::Pull)
        );
        assert_eq!(origin.action_for("Push to issue"), Some(OriginAction::Push));
        assert_eq!(
            origin.action_for("Open issue in browser"),
            Some(OriginAction::OpenInBrowser)
        );
        assert_eq!(
            origin.action_for("Archive task"),
            Some(OriginAction::Archive)
        );
    }

    #[test]
    fn enabled_action_lookup_skips_disabled_items() {
        let origin = OriginActionMenu::for_origin_task(
            "origin-sync-tui",
            "Origin sync TUI",
            OriginLabel::new("linear", "WT-142"),
        );
        assert_eq!(origin.enabled_action(OriginAction::Publish), None);

        let item = origin.enabled_action(OriginAction::Diff).unwrap();
        assert_eq!(item.shortcut(), "d");
    }

    #[test]
    fn shortcut_resolves_to_enabled_action() {
        let origin = OriginActionMenu::for_origin_task(
            "origin-sync-tui",
            "Origin sync TUI",
            OriginLabel::new("linear", "WT-142"),
        );
        assert_eq!(origin.action_for_shortcut("P"), Some(OriginAction::Push));
        assert_eq!(origin.action_for_shortcut("a"), Some(OriginAction::Archive));
        let local = OriginActionMenu::for_local_task("scratch-clean", "Scratch cleanup");
        assert_eq!(local.action_for_shortcut("a"), Some(OriginAction::Archive));
        assert_eq!(local.action_for_shortcut("P"), None);
    }

    #[test]
    fn task_menu_disables_diff_without_origin() {
        let menu = OriginActionMenu::for_local_task("scratch-clean", "Scratch cleanup");

        assert!(menu.enabled("Publish as issue"));
        assert!(menu.enabled("Attach existing issue"));
        assert_eq!(
            menu.disabled_reason("Diff with issue").unwrap(),
            "no origin attached"
        );
        assert_eq!(
            menu.disabled_reason("Fetch origin").unwrap(),
            "no origin attached"
        );
    }

    #[test]
    fn workflow_menu_lists_child_origin_inspection() {
        let menu = OriginActionMenu::for_workflow(
            "2026-06-06-001",
            "Ship provider-origin UX",
            Some(OriginLabel::new("Linear", "WT-100")),
            vec![ChildOriginLabel::new(
                "origin-sync-tui",
                Some(OriginLabel::new("Linear", "WT-142")),
            )],
        );

        assert!(menu.enabled("Diff workflow with issue"));
        assert!(menu.render_plain().contains("child task actions"));
        assert!(menu.render_plain().contains("origin-sync-tui"));
        assert!(menu.render_plain().contains("Linear WT-142"));
    }

    #[test]
    fn task_menu_disables_reattach_for_existing_origin() {
        let menu = OriginActionMenu::for_origin_task(
            "origin-sync-tui",
            "Origin sync TUI",
            OriginLabel::new("Linear", "WT-142"),
        );

        assert!(!menu.enabled("Attach different origin"));
        assert_eq!(
            menu.disabled_reason("Attach different origin").unwrap(),
            "origin replacement not supported"
        );
    }

    #[test]
    fn workflow_menu_omits_unsupported_publish_action() {
        let menu = OriginActionMenu::for_workflow(
            "2026-06-06-001",
            "Ship provider-origin UX",
            Some(OriginLabel::new("Linear", "WT-100")),
            vec![],
        );

        assert!(menu.disabled_reason("Publish workflow as issue").is_none());
        assert!(!menu.render_plain().contains("Publish workflow as issue"));
    }

    #[test]
    fn push_preview_marks_external_write() {
        let preview = OriginActionPreview::push_task(
            "origin-sync-tui",
            OriginLabel::new("Linear", "WT-142"),
            vec![
                PreviewOperation::append_comment("wt local update"),
                PreviewOperation::overwrite_field("issue title", "Origin sync TUI"),
            ],
        );

        assert!(preview.external_write);
        assert!(preview.render_plain().contains("External write"));
        assert!(preview.render_plain().contains("append comment"));
        assert!(preview.render_plain().contains("issue title"));
    }
}
