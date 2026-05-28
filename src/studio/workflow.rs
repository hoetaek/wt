use anyhow::{Result, bail};
use regex::Regex;
use std::sync::OnceLock;

pub(crate) fn validate_workflow_id(id: &str) -> Result<()> {
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        bail!("Workflow id must be a date-sequence id like 2026-05-28-001");
    }
    if !workflow_id_regex().is_match(id) {
        bail!("Workflow id must match YYYY-MM-DD-NNN");
    }
    Ok(())
}

fn workflow_id_regex() -> &'static Regex {
    static WORKFLOW_ID_REGEX: OnceLock<Regex> = OnceLock::new();
    WORKFLOW_ID_REGEX.get_or_init(|| {
        Regex::new(r"^\d{4}-\d{2}-\d{2}-\d{3}$").expect("workflow id regex should compile")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_id_validator_accepts_date_sequence_ids() {
        assert!(validate_workflow_id("2026-05-28-001").is_ok());
    }

    #[test]
    fn workflow_id_validator_rejects_traversal_and_non_ids() {
        for id in ["abc", "../etc", "2026-05-28-001/extra", "2026-05-28-1"] {
            assert!(validate_workflow_id(id).is_err(), "{id} should be rejected");
        }
    }
}
