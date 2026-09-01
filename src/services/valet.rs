use crate::context::CommandRunner;
use anyhow::Result;
use std::path::Path;

pub struct ValetService<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> ValetService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    pub fn is_available(&self) -> bool {
        self.runner.has_command("valet")
    }

    pub fn link(&self, site_name: &str, cwd: &Path) -> Result<()> {
        let out = self.runner.run("valet", &["link", site_name], Some(cwd))?;
        if !out.success && !out.stderr.is_empty() {
            anyhow::bail!("{}", out.stderr);
        }
        Ok(())
    }

    pub fn secure(&self, site_name: &str, cwd: &Path) -> Result<()> {
        let out = self
            .runner
            .run("valet", &["secure", site_name], Some(cwd))?;
        if !out.success && !out.stderr.is_empty() {
            anyhow::bail!("{}", out.stderr);
        }
        Ok(())
    }

    pub fn unlink(&self, site_name: &str, cwd: &Path) -> Result<bool> {
        if cwd.is_dir()
            && self
                .runner
                .run("valet", &["unlink"], Some(cwd))
                .is_ok_and(|out| out.success)
        {
            return Ok(true);
        }

        let out = self.runner.run("valet", &["unlink", site_name], None)?;
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

        let svc = ValetService::new(&runner);
        svc.link("sample-app-feature", Path::new("/tmp")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["link", "sample-app-feature"]);
    }

    #[test]
    fn secure_passes_site_name() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = ValetService::new(&runner);
        svc.secure("sample-app-feature", Path::new("/tmp")).unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["secure", "sample-app-feature"]);
    }

    #[test]
    fn unlink_uses_cwd_and_returns_success_status() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = ValetService::new(&runner);
        let cwd = tempfile::tempdir().unwrap();
        assert!(svc.unlink("sample-app-feature", cwd.path()).unwrap());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["unlink"]);
        assert_eq!(calls[0].2.as_deref(), Some(cwd.path()));
    }

    #[test]
    fn unlink_falls_back_to_site_name() {
        let mut runner = MockRunner::new();
        runner.add_response("", false);
        runner.add_response("", true);

        let svc = ValetService::new(&runner);
        let cwd = tempfile::tempdir().unwrap();
        assert!(svc.unlink("sample-app-feature", cwd.path()).unwrap());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, vec!["unlink"]);
        assert_eq!(calls[0].2.as_deref(), Some(cwd.path()));
        assert_eq!(calls[1].1, vec!["unlink", "sample-app-feature"]);
        assert_eq!(calls[1].2, None);
    }

    #[test]
    fn unlink_falls_back_to_site_name_when_cwd_call_errors() {
        let mut runner = MockRunner::new();
        runner.add_error("cwd disappeared");
        runner.add_response("", true);

        let svc = ValetService::new(&runner);
        let cwd = tempfile::tempdir().unwrap();
        assert!(svc.unlink("sample-app-feature", cwd.path()).unwrap());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, vec!["unlink"]);
        assert_eq!(calls[0].2.as_deref(), Some(cwd.path()));
        assert_eq!(calls[1].1, vec!["unlink", "sample-app-feature"]);
        assert_eq!(calls[1].2, None);
    }

    #[test]
    fn unlink_uses_site_name_when_cwd_is_missing() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = ValetService::new(&runner);
        let parent = tempfile::tempdir().unwrap();
        let cwd = parent.path().join("missing");
        assert!(svc.unlink("sample-app-feature", &cwd).unwrap());

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, vec!["unlink", "sample-app-feature"]);
        assert_eq!(calls[0].2, None);
    }

    #[test]
    fn is_available_checks_command() {
        let mut runner = MockRunner::new();
        runner.add_command("valet");

        let svc = ValetService::new(&runner);
        assert!(svc.is_available());
    }
}
