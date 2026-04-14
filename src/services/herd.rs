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

    pub fn link(&self, site_name: &str, cwd: &Path, secure: bool) -> Result<()> {
        let mut args = vec!["link", site_name];
        if secure {
            args.push("--secure");
        }
        // herd link can fail for various reasons, we don't treat it as fatal
        let _ = self.runner.run("herd", &args, Some(cwd));
        Ok(())
    }

    pub fn unlink(&self, site_name: &str) -> Result<bool> {
        let out = self.runner.run("herd", &["unlink", site_name], None)?;
        Ok(out.success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn link_passes_secure_flag() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        svc.link("hapjeong-tech-680", Path::new("/tmp"), true)
            .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["link", "hapjeong-tech-680", "--secure"]);
    }

    #[test]
    fn link_without_secure() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        svc.link("hapjeong-tech-680", Path::new("/tmp"), false)
            .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].1, vec!["link", "hapjeong-tech-680"]);
    }

    #[test]
    fn unlink_returns_success_status() {
        let mut runner = MockRunner::new();
        runner.add_response("", true);

        let svc = HerdService::new(&runner);
        assert!(svc.unlink("hapjeong-tech-680").unwrap());
    }

    #[test]
    fn is_available_checks_command() {
        let mut runner = MockRunner::new();
        runner.add_command("herd");

        let svc = HerdService::new(&runner);
        assert!(svc.is_available());
    }
}
