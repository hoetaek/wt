use crate::context::Ctx;
use anyhow::{Result, bail};

pub(crate) fn import(ctx: &Ctx, issues: &[String]) -> Result<()> {
    crate::commands::task::import(ctx, issues)
}

pub(crate) fn publish(ctx: &Ctx, tasks: &[String]) -> Result<()> {
    crate::commands::task_publish::run(ctx, tasks)
}

pub(crate) fn attach(_ctx: &Ctx, _task: &str, _issue: &str) -> Result<()> {
    reserved("attach")
}

pub(crate) fn fetch(_ctx: &Ctx, _tasks: &[String]) -> Result<()> {
    reserved("fetch")
}

pub(crate) fn diff(_ctx: &Ctx, _tasks: &[String]) -> Result<()> {
    reserved("diff")
}

pub(crate) fn pull(_ctx: &Ctx, _tasks: &[String]) -> Result<()> {
    reserved("pull")
}

pub(crate) fn push(_ctx: &Ctx, _tasks: &[String]) -> Result<()> {
    reserved("push")
}

fn reserved(command: &str) -> Result<()> {
    bail!(
        "`wt task origin {command}` is reserved for provider issue origin design until its implementation slice lands"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::context::mock::{MockRunner, MockUi};
    use crate::context::{Ctx, CtxOptions};

    fn ctx(root: &std::path::Path) -> Ctx {
        Ctx::new_with_options(
            root.to_path_buf(),
            root.to_path_buf(),
            Config::default(),
            Box::new(MockRunner::new()),
            Box::new(MockUi::new()),
            CtxOptions::default(),
        )
    }

    #[test]
    fn reserved_fetch_reports_canonical_command_name() {
        let dir = tempfile::tempdir().unwrap();
        let err = fetch(&ctx(dir.path()), &["demo".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("wt task origin fetch"));
        assert!(err.contains("reserved for provider issue origin"));
    }
}
