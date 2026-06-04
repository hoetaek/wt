use crate::workflow::{WORKFLOW_COLOR_ROTATION, WorkflowMetadata};
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

pub(crate) fn validate_workflow_candidate(
    disk: &WorkflowMetadata,
    candidate: &str,
) -> std::result::Result<WorkflowMetadata, Vec<String>> {
    let candidate = match toml::from_str::<WorkflowMetadata>(candidate) {
        Ok(candidate) => candidate,
        Err(err) => return Err(vec![err.to_string()]),
    };

    let mut errors = Vec::new();
    push_read_only_error(
        &mut errors,
        "created_at",
        candidate.created_at != disk.created_at,
    );
    push_read_only_error(
        &mut errors,
        "updated_at",
        candidate.updated_at != disk.updated_at,
    );
    push_read_only_error(&mut errors, "mode", candidate.mode != disk.mode);
    push_read_only_error(
        &mut errors,
        "base_mode",
        candidate.base_mode != disk.base_mode,
    );
    push_read_only_error(&mut errors, "base", candidate.base != disk.base);
    push_read_only_error(&mut errors, "profile", candidate.profile != disk.profile);
    push_read_only_error(&mut errors, "profiles", candidate.profiles != disk.profiles);
    push_read_only_error(&mut errors, "tasks", candidate.tasks != disk.tasks);
    push_read_only_error(&mut errors, "origin", candidate.origin != disk.origin);

    if let Some(color) = candidate.color.as_deref() {
        if !workflow_color_allowed(color) {
            errors.push(format!("invalid color: {color}"));
        }
    }

    if errors.is_empty() {
        Ok(candidate)
    } else {
        Err(errors)
    }
}

fn push_read_only_error(errors: &mut Vec<String>, field: &str, changed: bool) {
    if changed {
        errors.push(format!("field '{field}' is read-only in studio"));
    }
}

fn workflow_color_allowed(color: &str) -> bool {
    WORKFLOW_COLOR_ROTATION
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(color))
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
