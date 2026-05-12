use crate::commands::issue;
use crate::context::Ctx;
use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectedIssue {
    pub(crate) identifier: String,
    pub(crate) display: String,
}

pub(crate) fn select_issues(ctx: &Ctx, prompt: &str) -> Result<Vec<SelectedIssue>> {
    let provider = issue::build_provider(ctx)?;
    let issues = provider.list_issues()?;
    if issues.is_empty() {
        bail!("No issues found");
    }

    let items = issues
        .iter()
        .map(|issue| issue.display.clone())
        .collect::<Vec<_>>();
    let selections = ctx.ui.multi_select(prompt, &items)?;
    let mut selected = Vec::new();
    for idx in selections {
        let issue = issues
            .get(idx)
            .ok_or_else(|| anyhow::anyhow!("Selected issue index out of range: {idx}"))?;
        selected.push(SelectedIssue {
            identifier: issue.identifier.clone(),
            display: issue.display.clone(),
        });
    }
    Ok(selected)
}
