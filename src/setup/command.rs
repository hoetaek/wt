use crate::config::DepCommand;
use std::path::{Path, PathBuf};

pub(super) fn command_working_dir(wt_path: &Path, working_dir: Option<&str>) -> PathBuf {
    working_dir.map_or_else(
        || wt_path.to_path_buf(),
        |working_dir| wt_path.join(working_dir),
    )
}

pub(super) fn setup_command_display(dep: &DepCommand) -> String {
    command_display(dep.working_dir.as_deref(), &dep.run)
}

fn command_display(working_dir: Option<&str>, run: &str) -> String {
    working_dir.map_or_else(
        || run.to_string(),
        |working_dir| format!("{working_dir}: {run}"),
    )
}
