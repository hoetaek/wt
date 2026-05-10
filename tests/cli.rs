use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::process::Command as StdCommand;

#[test]
fn version_flag_prints_package_version() {
    Command::cargo_bin("wt")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("wt {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_subcommand_prints_package_version() {
    Command::cargo_bin("wt")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(format!("wt {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_subcommand_supports_json() {
    let output = Command::cargo_bin("wt")
        .unwrap()
        .args(["--json", "version"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["name"], "wt");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn no_args_prints_help_successfully() {
    Command::cargo_bin("wt")
        .unwrap()
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: wt [OPTIONS] [COMMAND]"))
        .stdout(predicate::str::contains("start"));
}

#[test]
fn completion_alias_generates_script() {
    Command::cargo_bin("wt")
        .unwrap()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_wt"));
}

#[test]
fn doctor_supports_json_and_directory_override() {
    let temp = TempDir::new().unwrap();
    let status = StdCommand::new("git")
        .arg("init")
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());

    let output = Command::cargo_bin("wt")
        .unwrap()
        .args(["-C", temp.path().to_str().unwrap(), "--json", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["checks"]
            .as_array()
            .is_some_and(|checks| !checks.is_empty())
    );
}

#[test]
fn doctor_uses_config_override() {
    let temp = TempDir::new().unwrap();
    let status = StdCommand::new("git")
        .arg("init")
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(
        temp.path().join("override.toml"),
        "[issues]\nprovider = \"github\"\n",
    )
    .unwrap();

    let output = Command::cargo_bin("wt")
        .unwrap()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--config",
            "override.toml",
            "--json",
            "doctor",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let issue_provider = value["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "issue_provider")
        .unwrap();
    assert_eq!(issue_provider["message"], "github");
}

#[test]
fn list_supports_json_and_directory_override() {
    let temp = TempDir::new().unwrap();
    let status = StdCommand::new("git")
        .arg("init")
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());

    let output = Command::cargo_bin("wt")
        .unwrap()
        .args(["-C", temp.path().to_str().unwrap(), "--json", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value.as_array().is_some());
}
