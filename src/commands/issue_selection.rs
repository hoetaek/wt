use crate::commands::issue;
use crate::context::{Ctx, PromptItem, PromptRow};
use crate::services::issues::{IssueListItem, IssueProvider};
use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectedIssue {
    pub(crate) identifier: String,
    pub(crate) title: String,
    pub(crate) display: String,
}

pub(crate) fn select_issues(ctx: &Ctx, prompt: &str) -> Result<Vec<SelectedIssue>> {
    let provider = issue::build_provider(ctx)?;
    select_issues_with_provider(ctx, prompt, provider.as_ref())
}

pub(crate) fn select_issues_with_provider(
    ctx: &Ctx,
    prompt: &str,
    provider: &dyn IssueProvider,
) -> Result<Vec<SelectedIssue>> {
    let issues = provider.list_issues()?;
    if issues.is_empty() {
        bail!("No issues found");
    }

    let selections = select_issue_indices(ctx, prompt, &issues)?;
    let mut selected = Vec::new();
    for idx in selections {
        let issue = issues
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("Selected issue index out of range: {idx}"))?;
        let item = issue_prompt_item(issue);
        selected.push(SelectedIssue {
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            display: item.render_plain(),
        });
    }
    Ok(selected)
}

fn select_issue_indices(ctx: &Ctx, prompt: &str, issues: &[IssueListItem]) -> Result<Vec<usize>> {
    let rows = issue_prompt_rows(issues);
    ctx.ui.multi_select_rows(prompt, &rows)
}

fn issue_prompt_items(issues: &[IssueListItem]) -> Vec<PromptItem> {
    issues.iter().map(issue_prompt_item).collect()
}

fn issue_prompt_rows(issues: &[IssueListItem]) -> Vec<PromptRow> {
    let mut rows = vec![PromptRow::section("Provider issues")];
    rows.extend(
        issue_prompt_items(issues)
            .into_iter()
            .enumerate()
            .map(|(index, item)| PromptRow::from_indexed_item(index, item)),
    );
    rows
}

fn issue_prompt_item(issue: &IssueListItem) -> PromptItem {
    let mut hint_parts = vec![display_identifier(&issue.identifier)];
    if let Some(hint) = issue.hint.clone() {
        hint_parts.push(hint);
    }
    PromptItem::from_hint_parts(issue.title.clone(), hint_parts)
}

fn display_identifier(identifier: &str) -> String {
    let identifier = identifier.trim();
    if identifier.starts_with('#') || !identifier.chars().all(|ch| ch.is_ascii_digit()) {
        identifier.to_string()
    } else {
        format!("#{identifier}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_prompt_item_uses_title_label_and_metadata_hint() {
        let issue = IssueListItem {
            identifier: "PROJ-123".into(),
            title: "Make prompts beautiful".into(),
            display: "PROJ-123 Todo alice Make prompts beautiful".into(),
            hint: Some("Todo | alice".into()),
        };

        assert_eq!(
            issue_prompt_item(&issue),
            PromptItem::with_hint("Make prompts beautiful", "PROJ-123 | Todo | alice")
        );
    }

    #[test]
    fn issue_prompt_item_formats_numeric_github_identifiers() {
        let issue = IssueListItem {
            identifier: "42".into(),
            title: "Add GitHub issue prompt polish".into(),
            display: "#42 Add GitHub issue prompt polish".into(),
            hint: Some("GitHub".into()),
        };

        assert_eq!(
            issue_prompt_item(&issue),
            PromptItem::with_hint("Add GitHub issue prompt polish", "#42 | GitHub")
        );
    }
}
