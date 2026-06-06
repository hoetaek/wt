use crate::context::Ctx;
use anyhow::{Result, bail};

pub(crate) fn attach(_ctx: &Ctx, _workflow: &str, _issue: &str) -> Result<()> {
    reserved("attach")
}

pub(crate) fn fetch(_ctx: &Ctx, _workflows: &[String]) -> Result<()> {
    reserved("fetch")
}

pub(crate) fn diff(_ctx: &Ctx, _workflows: &[String]) -> Result<()> {
    reserved("diff")
}

pub(crate) fn pull(_ctx: &Ctx, _workflows: &[String]) -> Result<()> {
    reserved("pull")
}

pub(crate) fn push(_ctx: &Ctx, _workflows: &[String]) -> Result<()> {
    reserved("push")
}

fn reserved(command: &str) -> Result<()> {
    bail!(
        "`wt workflow origin {command}` is reserved for provider issue origin design until its implementation slice lands. Workflow origin actions affect workflow title/body/origin only"
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
    fn reserved_fetch_reports_workflow_scope() {
        let dir = tempfile::tempdir().unwrap();
        let err = fetch(&ctx(dir.path()), &["2026-06-06-001".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("wt workflow origin fetch"));
        assert!(err.contains("workflow title/body/origin"));
    }
}
