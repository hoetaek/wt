use crate::cli::TraefikCommand;
use crate::config::SiteProvider;
use crate::context::Ctx;
use crate::services::traefik::TraefikService;
use anyhow::{Result, bail};
use std::path::Path;

const DEFAULT_BIND_IP: &str = "127.0.0.2";
const DEFAULT_LABEL: &str = "wt.traefik";
const DOCTOR_HOST: &str = "wt-traefik-doctor.l";

pub fn run(ctx: &Ctx, command: &TraefikCommand) -> Result<()> {
    match command {
        TraefikCommand::Doctor => doctor(ctx),
        TraefikCommand::Paths => paths(ctx),
        TraefikCommand::ExampleLaunchd { label, bind_ip } => example_launchd(ctx, label, bind_ip),
    }
}

fn doctor(ctx: &Ctx) -> Result<()> {
    ctx.ui.print_step("Traefik doctor");

    let mut failures = Vec::new();
    let mut warnings = Vec::new();

    if ctx.config.site.as_ref().map(|site| &site.provider) == Some(&SiteProvider::Traefik) {
        ok(ctx, "[site] provider is traefik");
    } else {
        warnings.push("[site] provider is not traefik in the active wt config".to_string());
    }

    if ctx.runner.has_command("traefik") {
        ok(ctx, "traefik is on PATH");
    } else {
        warnings.push(
            "traefik is not on PATH; install it with Homebrew or use an absolute path in launchd"
                .into(),
        );
    }

    let service = TraefikService::new();
    let sites_dir = service.sites_dir();
    if sites_dir.exists() {
        ok(
            ctx,
            &format!("sites directory exists: {}", sites_dir.display()),
        );
    } else {
        warnings.push(format!(
            "sites directory does not exist yet: {}",
            sites_dir.display()
        ));
    }

    if check_loopback_alias(ctx, DEFAULT_BIND_IP)? {
        ok(ctx, &format!("lo0 has alias {DEFAULT_BIND_IP}"));
    } else {
        failures.push(format!(
            "lo0 is missing alias {DEFAULT_BIND_IP}; add it with sudo ifconfig lo0 alias {DEFAULT_BIND_IP}"
        ));
    }

    if check_dns(ctx, DOCTOR_HOST, DEFAULT_BIND_IP)? {
        ok(ctx, &format!("{DOCTOR_HOST} resolves to {DEFAULT_BIND_IP}"));
    } else {
        failures.push(format!(
            "{DOCTOR_HOST} does not resolve to {DEFAULT_BIND_IP}; configure Herd dnsmasq address=/.l/{DEFAULT_BIND_IP}"
        ));
    }

    if check_listener(ctx, DEFAULT_BIND_IP, "443")? {
        ok(ctx, &format!("{DEFAULT_BIND_IP}:443 is listening"));
    } else {
        failures.push(format!(
            "{DEFAULT_BIND_IP}:443 is not listening; run host-native Traefik as a privileged daemon"
        ));
    }

    if check_listener(ctx, DEFAULT_BIND_IP, "80")? {
        ok(ctx, &format!("{DEFAULT_BIND_IP}:80 is listening"));
    } else {
        warnings.push(format!(
            "{DEFAULT_BIND_IP}:80 is not listening; HTTPS can work without it, but redirects from HTTP will not"
        ));
    }

    for warning in warnings {
        ctx.ui.print_warning(&warning);
    }

    if !failures.is_empty() {
        for failure in &failures {
            ctx.ui.print_error(failure);
        }
        bail!(
            "Traefik host setup has {} required issue(s)",
            failures.len()
        );
    }

    ctx.ui.print_step("Traefik host setup looks ready");
    Ok(())
}

fn paths(_ctx: &Ctx) -> Result<()> {
    let service = TraefikService::new();
    let sites_dir = service.sites_dir();
    let root_dir = sites_dir.parent().unwrap_or_else(|| Path::new("."));
    let log_path = root_dir.join("traefik.log");

    println!("Traefik provider paths");
    println!("sites_dir = {}", sites_dir.display());
    println!("log_path = {}", log_path.display());
    println!("launchd_label = {DEFAULT_LABEL}");
    println!("launchd_plist = /Library/LaunchDaemons/{DEFAULT_LABEL}.plist");
    println!("bind_ip = {DEFAULT_BIND_IP}");
    println!("web = {DEFAULT_BIND_IP}:80");
    println!("websecure = {DEFAULT_BIND_IP}:443");
    println!("dns = *.l -> {DEFAULT_BIND_IP}");
    println!("override_sites_dir_env = WT_TRAEFIK_SITES_DIR");
    Ok(())
}

fn example_launchd(ctx: &Ctx, label: &str, bind_ip: &str) -> Result<()> {
    let service = TraefikService::new();
    let sites_dir = service.sites_dir();
    let root_dir = sites_dir.parent().unwrap_or_else(|| Path::new("."));
    let log_path = root_dir.join("traefik.log");
    let traefik_bin = traefik_binary(ctx);

    println!(
        "{}",
        render_launchd_plist(label, bind_ip, &traefik_bin, sites_dir, &log_path)
    );

    Ok(())
}

fn check_loopback_alias(ctx: &Ctx, bind_ip: &str) -> Result<bool> {
    let output = ctx.runner.run("ifconfig", &["lo0"], None)?;
    Ok(output.success && output.stdout.contains(bind_ip))
}

fn check_dns(ctx: &Ctx, host: &str, bind_ip: &str) -> Result<bool> {
    if !ctx.runner.has_command("dscacheutil") {
        return Ok(false);
    }
    let output = ctx
        .runner
        .run("dscacheutil", &["-q", "host", "-a", "name", host], None)?;
    Ok(output.success && output.stdout.contains(&format!("ip_address: {bind_ip}")))
}

fn check_listener(ctx: &Ctx, bind_ip: &str, port: &str) -> Result<bool> {
    if ctx.runner.has_command("nc") {
        let output = ctx.runner.run("nc", &["-vz", bind_ip, port], None)?;
        return Ok(output.success);
    }

    if !ctx.runner.has_command("lsof") {
        return Ok(false);
    }
    let address = format!("-iTCP@{bind_ip}:{port}");
    let output = ctx
        .runner
        .run("lsof", &["-nP", &address, "-sTCP:LISTEN"], None)?;
    Ok(output.success && output.stdout.contains(&format!("{bind_ip}:{port}")))
}

fn traefik_binary(ctx: &Ctx) -> String {
    if ctx.runner.has_command("traefik") {
        if let Ok(output) = ctx.runner.run("which", &["traefik"], None) {
            if output.success && !output.stdout.trim().is_empty() {
                return output.stdout.trim().to_string();
            }
        }
    }

    "/opt/homebrew/bin/traefik".into()
}

fn render_launchd_plist(
    label: &str,
    bind_ip: &str,
    traefik_bin: &str,
    sites_dir: &Path,
    log_path: &Path,
) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{traefik_bin}</string>
    <string>--entrypoints.web.address={bind_ip}:80</string>
    <string>--entrypoints.websecure.address={bind_ip}:443</string>
    <string>--providers.file.directory={sites_dir}</string>
    <string>--providers.file.watch=true</string>
    <string>--log.level=INFO</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{log_path}</string>
  <key>StandardErrorPath</key>
  <string>{log_path}</string>
</dict>
</plist>
"#,
        label = xml_escape(label),
        bind_ip = xml_escape(bind_ip),
        traefik_bin = xml_escape(traefik_bin),
        sites_dir = xml_escape(&sites_dir.display().to_string()),
        log_path = xml_escape(&log_path.display().to_string())
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ok(ctx: &Ctx, msg: &str) {
    ctx.ui.print_step(&format!("OK: {msg}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, SiteConfig};
    use crate::context::CommandRunner;
    use crate::context::mock::{MockRunner, MockUi};
    use std::path::PathBuf;

    fn test_ctx(runner: MockRunner, config: Config) -> Ctx {
        Ctx::new(
            PathBuf::from("/tmp/repo"),
            PathBuf::from("/tmp/repo"),
            config,
            Box::new(runner),
            Box::new(MockUi::new()),
        )
    }

    #[test]
    fn render_launchd_plist_uses_bind_ip_and_sites_dir() {
        let plist = render_launchd_plist(
            "dev.wt-traefik",
            "127.0.0.3",
            "/opt/homebrew/bin/traefik",
            Path::new("/Users/alice/.config/wt/traefik/sites"),
            Path::new("/Users/alice/.config/wt/traefik/traefik.log"),
        );

        assert!(plist.contains("<string>dev.wt-traefik</string>"));
        assert!(plist.contains("--entrypoints.web.address=127.0.0.3:80"));
        assert!(plist.contains("--entrypoints.websecure.address=127.0.0.3:443"));
        assert!(plist.contains("--providers.file.directory=/Users/alice/.config/wt/traefik/sites"));
        assert!(plist.contains("/Users/alice/.config/wt/traefik/traefik.log"));
    }

    #[test]
    fn traefik_binary_uses_which_when_available() {
        let mut runner = MockRunner::new();
        runner.add_command("traefik");
        runner.add_response("/usr/local/bin/traefik", true);
        let ctx = test_ctx(runner, Config::default());

        assert_eq!(traefik_binary(&ctx), "/usr/local/bin/traefik");
    }

    #[test]
    fn doctor_succeeds_when_required_host_checks_pass() {
        let mut runner = MockRunner::new();
        runner.add_command("traefik");
        runner.add_command("dscacheutil");
        runner.add_command("lsof");
        runner.add_response("inet 127.0.0.2 netmask 0xff000000", true);
        runner.add_response("name: wt-traefik-doctor.l\nip_address: 127.0.0.2", true);
        runner.add_response("traefik 1 root TCP 127.0.0.2:443 (LISTEN)", true);
        runner.add_response("traefik 1 root TCP 127.0.0.2:80 (LISTEN)", true);

        let config = Config {
            site: Some(SiteConfig {
                provider: SiteProvider::Traefik,
                ..SiteConfig::default()
            }),
            ..Config::default()
        };
        let ctx = test_ctx(runner, config);

        doctor(&ctx).unwrap();
    }

    #[test]
    fn doctor_fails_when_loopback_alias_is_missing() {
        let mut runner = MockRunner::new();
        runner.add_command("dscacheutil");
        runner.add_command("lsof");
        runner.add_response("inet 127.0.0.1 netmask 0xff000000", true);
        runner.add_response("name: wt-traefik-doctor.l\nip_address: 127.0.0.2", true);
        runner.add_response("traefik 1 root TCP 127.0.0.2:443 (LISTEN)", true);
        runner.add_response("traefik 1 root TCP 127.0.0.2:80 (LISTEN)", true);
        let ctx = test_ctx(runner, Config::default());

        let err = doctor(&ctx).unwrap_err().to_string();
        assert!(err.contains("required issue"));
    }

    #[test]
    fn mock_runner_responses_stay_in_expected_order() {
        let mut runner = MockRunner::new();
        runner.add_response("first", true);
        runner.add_response("second", true);

        let first = runner.run("one", &[], None).unwrap();
        let second = runner.run("two", &[], None).unwrap();

        assert_eq!(first.stdout, "first");
        assert_eq!(second.stdout, "second");
    }
}
