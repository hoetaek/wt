use crate::context::CommandRunner;
use anyhow::Result;
use std::path::Path;

pub struct HerdService<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> HerdService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    pub fn is_available(&self) -> bool {
        self.runner.has_command("herd")
    }

    pub fn link(&self, site_name: &str, cwd: &Path) -> Result<()> {
        let out = self.runner.run("herd", &["link", site_name], Some(cwd))?;
        if !out.success && !out.stderr.is_empty() {
            anyhow::bail!("{}", out.stderr);
        }
        Ok(())
    }

    pub fn secure(&self, site_name: &str, cwd: &Path) -> Result<()> {
        let out = self.runner.run("herd", &["secure", site_name], Some(cwd))?;
        if !out.success && !out.stderr.is_empty() {
            anyhow::bail!("{}", out.stderr);
        }
        Ok(())
    }

    pub fn unlink(&self, site_name: &str, cwd: &Path) -> Result<bool> {
        let out = self.runner.run("herd", &["unlink"], Some(cwd))?;
        if out.success {
            return Ok(true);
        }

        let out = self.runner.run("herd", &["unlink", site_name], Some(cwd))?;
        Ok(out.success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn link_passes_site_name() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        svc.link("sample-app-proj-680", Path::new("/tmp")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["link", "sample-app-proj-680"]);
    }

    #[test]
    fn secure_passes_site_name() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        svc.secure("sample-app-proj-680", Path::new("/tmp"))
            .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["secure", "sample-app-proj-680"]);
    }

    #[test]
    fn unlink_uses_cwd_and_returns_success_status() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        let cwd = Path::new("/tmp/sample-app-proj-680");
        assert!(svc.unlink("sample-app-proj-680", cwd).unwrap());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["unlink"]);
        assert_eq!(calls[0].2.as_deref(), Some(cwd));
    }

    #[test]
    fn unlink_falls_back_to_site_name() {
        let mut runner = MockRunner::new();
        runner.add_response("", false);
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        let cwd = Path::new("/tmp/sample-app-proj-680");
        assert!(svc.unlink("sample-app-proj-680", cwd).unwrap());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["unlink"]);
        assert_eq!(calls[1].1, vec!["unlink", "sample-app-proj-680"]);
        assert_eq!(calls[1].2.as_deref(), Some(cwd));
    }

    #[test]
    fn is_available_checks_command() {
        let mut runner = MockRunner::new();
        runner.add_command("herd");

        let svc = HerdService::new(&runner);
        assert!(svc.is_available());
    }
}
