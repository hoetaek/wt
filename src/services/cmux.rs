use crate::context::CommandRunner;
use anyhow::Result;
use std::path::Path;

pub struct CmuxService<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> CmuxService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self { Self { runner } }
    pub fn is_available(&self) -> bool { self.runner.has_command("cmux") }
    pub fn new_workspace(&self, _cwd: &Path, _name: &str, _command: &str) -> Result<String> { todo!() }
    pub fn list_panes(&self, _workspace: &str) -> Result<Vec<String>> { todo!() }
    pub fn new_surface(&self, _pane: &str, _workspace: &str) -> Result<String> { todo!() }
    pub fn send(&self, _surface: &str, _workspace: &str, _text: &str) -> Result<()> { todo!() }
    pub fn set_color(&self, _workspace: &str, _color: &str) -> Result<()> { todo!() }
    pub fn read_screen(&self, _surface: &str, _workspace: &str) -> Result<String> { todo!() }
}
