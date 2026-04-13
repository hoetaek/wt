use crate::context::CommandRunner;
use anyhow::Result;
use std::path::Path;

pub struct HerdService<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> HerdService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self { Self { runner } }
    pub fn is_available(&self) -> bool { self.runner.has_command("herd") }
    pub fn link(&self, _site_name: &str, _cwd: &Path, _secure: bool) -> Result<()> { todo!() }
    pub fn unlink(&self, _site_name: &str) -> Result<bool> { todo!() }
}
