use crate::config::SiteProvider;
use crate::context::CommandRunner;
use crate::services::herd::HerdService;
use crate::services::traefik::TraefikService;
use crate::services::valet::ValetService;
use anyhow::{Result, anyhow};
use std::path::Path;

pub struct SiteService<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> SiteService<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    pub fn is_available(&self, provider: &SiteProvider) -> bool {
        match provider {
            SiteProvider::None => false,
            SiteProvider::Herd => HerdService::new(self.runner).is_available(),
            SiteProvider::Valet => ValetService::new(self.runner).is_available(),
            SiteProvider::External => true,
            SiteProvider::Traefik => true,
        }
    }

    pub fn register(
        &self,
        provider: &SiteProvider,
        site_name: &str,
        cwd: &Path,
        secure: bool,
        target: Option<&str>,
    ) -> Result<()> {
        match provider {
            SiteProvider::None | SiteProvider::External => Ok(()),
            SiteProvider::Herd => {
                let herd = HerdService::new(self.runner);
                herd.link(site_name, cwd)?;
                if secure {
                    herd.secure(site_name, cwd)?;
                }
                Ok(())
            }
            SiteProvider::Valet => {
                let valet = ValetService::new(self.runner);
                valet.link(site_name, cwd)?;
                if secure {
                    valet.secure(site_name, cwd)?;
                }
                Ok(())
            }
            SiteProvider::Traefik => {
                let target = target.ok_or_else(|| anyhow!("Traefik target is required"))?;
                TraefikService::new().register(site_name, target, secure)?;
                Ok(())
            }
        }
    }

    pub fn unregister(&self, provider: &SiteProvider, site_name: &str) -> Result<bool> {
        match provider {
            SiteProvider::None | SiteProvider::External => Ok(false),
            SiteProvider::Herd => HerdService::new(self.runner).unlink(site_name),
            SiteProvider::Valet => ValetService::new(self.runner).unlink(site_name),
            SiteProvider::Traefik => TraefikService::new().unregister(site_name),
        }
    }
}

pub fn provider_label(provider: &SiteProvider) -> &'static str {
    match provider {
        SiteProvider::None => "Site",
        SiteProvider::Herd => "Herd",
        SiteProvider::Valet => "Valet",
        SiteProvider::External => "External",
        SiteProvider::Traefik => "Traefik",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::mock::MockRunner;

    #[test]
    fn register_herd_links_and_secures() {
        let mut runner = MockRunner::new();
        runner.add_command("herd");
        runner.add_response("", true);
        runner.add_response("", true);

        let svc = SiteService::new(&runner);
        svc.register(
            &SiteProvider::Herd,
            "sample-app-feature",
            Path::new("/tmp/app"),
            true,
            None,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls[0].0, "herd");
        assert_eq!(calls[0].1, vec!["link", "sample-app-feature"]);
        assert_eq!(calls[1].0, "herd");
        assert_eq!(calls[1].1, vec!["secure", "sample-app-feature"]);
    }

    #[test]
    fn register_valet_links_without_secure_when_disabled() {
        let mut runner = MockRunner::new();
        runner.add_command("valet");
        runner.add_response("", true);

        let svc = SiteService::new(&runner);
        svc.register(
            &SiteProvider::Valet,
            "sample-app-feature",
            Path::new("/tmp/app"),
            false,
            None,
        )
        .unwrap();

        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "valet");
        assert_eq!(calls[0].1, vec!["link", "sample-app-feature"]);
    }

    #[test]
    fn external_is_noop() {
        let runner = MockRunner::new();

        let svc = SiteService::new(&runner);
        assert!(svc.is_available(&SiteProvider::External));
        svc.register(
            &SiteProvider::External,
            "sample-app-feature",
            Path::new("/tmp/app"),
            true,
            None,
        )
        .unwrap();
        assert!(
            !svc.unregister(&SiteProvider::External, "sample-app-feature")
                .unwrap()
        );
        assert!(runner.calls.lock().unwrap().is_empty());
    }
}
