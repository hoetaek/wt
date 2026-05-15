use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct TraefikService {
    sites_dir: PathBuf,
    certs_dir: PathBuf,
    manage_certs: bool,
}

impl Default for TraefikService {
    fn default() -> Self {
        Self::new()
    }
}

impl TraefikService {
    pub fn new() -> Self {
        let sites_dir = default_sites_dir();
        Self {
            certs_dir: default_certs_dir(&sites_dir),
            sites_dir,
            manage_certs: true,
        }
    }

    #[cfg(test)]
    pub fn with_sites_dir(sites_dir: PathBuf) -> Self {
        let certs_dir = sites_dir.join("certs");
        Self {
            sites_dir,
            certs_dir,
            manage_certs: false,
        }
    }

    #[cfg(test)]
    pub fn with_dirs(sites_dir: PathBuf, certs_dir: PathBuf) -> Self {
        Self {
            sites_dir,
            certs_dir,
            manage_certs: false,
        }
    }

    pub fn register(&self, site_name: &str, target: &str, secure: bool) -> Result<PathBuf> {
        if site_name.contains('`') {
            bail!("Traefik site name cannot contain backticks");
        }
        if target.trim().is_empty() {
            bail!("Traefik target is required");
        }

        fs::create_dir_all(&self.sites_dir).with_context(|| {
            format!(
                "failed to create Traefik sites directory: {}",
                self.sites_dir.display()
            )
        })?;

        let path = self.site_path(site_name);
        let cert = if secure && self.manage_certs {
            self.ensure_certificate(site_name)?
        } else {
            None
        };

        fs::write(
            &path,
            render_dynamic_config(site_name, target, secure, cert),
        )
        .with_context(|| format!("failed to write Traefik site config: {}", path.display()))?;
        Ok(path)
    }

    pub fn unregister(&self, site_name: &str) -> Result<bool> {
        let path = self.site_path(site_name);
        let mut removed = false;
        if path.exists() {
            fs::remove_file(&path).with_context(|| {
                format!("failed to delete Traefik site config: {}", path.display())
            })?;
            removed = true;
        }

        for path in self.cert_paths(site_name) {
            if path.exists() {
                fs::remove_file(&path).with_context(|| {
                    format!("failed to delete Traefik certificate: {}", path.display())
                })?;
                removed = true;
            }
        }

        Ok(removed)
    }

    pub fn sites_dir(&self) -> &Path {
        &self.sites_dir
    }

    fn site_path(&self, site_name: &str) -> PathBuf {
        self.sites_dir
            .join(format!("{}.yml", safe_file_stem(site_name)))
    }

    fn cert_paths(&self, site_name: &str) -> [PathBuf; 2] {
        let stem = safe_file_stem(site_name);
        [
            self.certs_dir.join(format!("{stem}.crt")),
            self.certs_dir.join(format!("{stem}.key")),
        ]
    }

    fn ensure_certificate(&self, site_name: &str) -> Result<Option<CertificatePaths>> {
        let [cert_file, key_file] = self.cert_paths(site_name);
        if cert_file.exists() && key_file.exists() {
            return Ok(Some(CertificatePaths {
                cert_file,
                key_file,
            }));
        }
        if !command_exists("mkcert") {
            return Ok(None);
        }

        fs::create_dir_all(&self.certs_dir).with_context(|| {
            format!(
                "failed to create Traefik certificates directory: {}",
                self.certs_dir.display()
            )
        })?;

        let output = Command::new("mkcert")
            .arg("-cert-file")
            .arg(&cert_file)
            .arg("-key-file")
            .arg(&key_file)
            .arg(site_name)
            .output()
            .with_context(|| "failed to run mkcert")?;

        if !output.status.success() {
            bail!(
                "mkcert failed for {site_name}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(Some(CertificatePaths {
            cert_file,
            key_file,
        }))
    }
}

pub fn default_sites_dir() -> PathBuf {
    if let Some(path) = env::var_os("WT_TRAEFIK_SITES_DIR") {
        return PathBuf::from(path);
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join(".config/wt/traefik/sites")
}

fn default_certs_dir(sites_dir: &Path) -> PathBuf {
    if let Some(path) = env::var_os("WT_TRAEFIK_CERTS_DIR") {
        return PathBuf::from(path);
    }

    sites_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("certs")
}

#[derive(Clone)]
struct CertificatePaths {
    cert_file: PathBuf,
    key_file: PathBuf,
}

fn render_dynamic_config(
    site_name: &str,
    target: &str,
    secure: bool,
    cert: Option<CertificatePaths>,
) -> String {
    let resource_name = resource_name(site_name);
    let entrypoint = if secure { "websecure" } else { "web" };
    let tls = if secure { "      tls: {}\n" } else { "" };
    let certificate = cert
        .map(|cert| {
            format!(
                "tls:\n  certificates:\n    - certFile: \"{}\"\n      keyFile: \"{}\"\n",
                yaml_escape(&cert.cert_file.display().to_string()),
                yaml_escape(&cert.key_file.display().to_string())
            )
        })
        .unwrap_or_default();

    format!(
        "http:\n  routers:\n    {resource_name}:\n      rule: \"Host(`{site_name}`)\"\n      entryPoints:\n        - {entrypoint}\n      service: {resource_name}\n{tls}  services:\n    {resource_name}:\n      loadBalancer:\n        servers:\n          - url: \"{}\"\n",
        yaml_escape(target)
    )
    .to_string()
        + &certificate
}

fn resource_name(site_name: &str) -> String {
    let mut result = String::new();
    let mut last_was_dash = false;

    for ch in site_name.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            result.push('-');
            last_was_dash = true;
        }
    }

    let trimmed = result.trim_matches('-');
    if trimmed.is_empty() {
        "wt-site".into()
    } else {
        trimmed.into()
    }
}

fn safe_file_stem(site_name: &str) -> String {
    let stem = site_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let trimmed = stem.trim_matches('-');
    if trimmed.is_empty() {
        "wt-site".into()
    } else {
        trimmed.into()
    }
}

fn yaml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn command_exists(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_writes_secure_dynamic_config() {
        let dir = tempfile::tempdir().unwrap();
        let svc = TraefikService::with_sites_dir(dir.path().to_path_buf());

        let path = svc
            .register("istat-feature-report.l", "http://127.0.0.1:5173", true)
            .unwrap();

        assert_eq!(path.file_name().unwrap(), "istat-feature-report.l.yml");
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("rule: \"Host(`istat-feature-report.l`)\""));
        assert!(content.contains("- websecure"));
        assert!(content.contains("tls: {}"));
        assert!(content.contains("url: \"http://127.0.0.1:5173\""));
    }

    #[test]
    fn render_dynamic_config_includes_certificate_paths() {
        let content = render_dynamic_config(
            "istat-feature-report.l",
            "http://127.0.0.1:5173",
            true,
            Some(CertificatePaths {
                cert_file: PathBuf::from("/Users/alice/.config/wt/traefik/certs/site.crt"),
                key_file: PathBuf::from("/Users/alice/.config/wt/traefik/certs/site.key"),
            }),
        );

        assert!(content.contains("certFile: \"/Users/alice/.config/wt/traefik/certs/site.crt\""));
        assert!(content.contains("keyFile: \"/Users/alice/.config/wt/traefik/certs/site.key\""));
    }

    #[test]
    fn register_writes_insecure_dynamic_config() {
        let dir = tempfile::tempdir().unwrap();
        let svc = TraefikService::with_sites_dir(dir.path().to_path_buf());

        let path = svc
            .register("istat-feature-report.l", "http://127.0.0.1:5173", false)
            .unwrap();

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("- web"));
        assert!(!content.contains("tls: {}"));
    }

    #[test]
    fn unregister_deletes_site_config() {
        let dir = tempfile::tempdir().unwrap();
        let svc = TraefikService::with_sites_dir(dir.path().to_path_buf());
        let path = svc
            .register("istat-feature-report.l", "http://127.0.0.1:5173", true)
            .unwrap();

        assert!(svc.unregister("istat-feature-report.l").unwrap());
        assert!(!path.exists());
        assert!(!svc.unregister("istat-feature-report.l").unwrap());
    }

    #[test]
    fn unregister_deletes_managed_certificate_files() {
        let dir = tempfile::tempdir().unwrap();
        let sites_dir = dir.path().join("sites");
        let certs_dir = dir.path().join("certs");
        fs::create_dir_all(&sites_dir).unwrap();
        fs::create_dir_all(&certs_dir).unwrap();
        let svc = TraefikService::with_dirs(sites_dir.clone(), certs_dir.clone());

        let [cert, key] = svc.cert_paths("istat-feature-report.l");
        fs::write(&cert, "cert").unwrap();
        fs::write(&key, "key").unwrap();

        assert!(svc.unregister("istat-feature-report.l").unwrap());
        assert!(!cert.exists());
        assert!(!key.exists());
    }
}
