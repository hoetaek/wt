use crate::context::Ctx;
use crate::names::WorktreeNames;
use anyhow::Result;
use std::path::Path;

pub fn run_setup(
    _ctx: &Ctx,
    _wt_path: &Path,
    _names: &WorktreeNames,
    _title: Option<&str>,
    _mode: &str,
) -> Result<()> {
    todo!()
}
