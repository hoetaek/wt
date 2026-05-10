use crate::commands::clean;
use crate::context::Ctx;
use anyhow::Result;

pub fn run(ctx: &Ctx, targets: &[String]) -> Result<()> {
    clean::run_with_targets(ctx, targets)
}
