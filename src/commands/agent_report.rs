pub(crate) const REPORT_HEADING: &str = "Agent Completion Report";

pub(crate) const REPORT_ITEMS: [&str; 4] = [
    "Summary",
    "Changed files",
    "Checks run",
    "Risks or follow-ups",
];

pub(crate) fn prompt_section() -> String {
    let items = REPORT_ITEMS
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("## {REPORT_HEADING}\n\nWhen you finish, report:\n\n{items}")
}
