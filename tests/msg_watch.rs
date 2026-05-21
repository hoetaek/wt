use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

const GIT_LOCAL_ENV_KEYS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

fn git_command() -> StdCommand {
    let mut command = StdCommand::new("git");
    for key in GIT_LOCAL_ENV_KEYS {
        command.env_remove(key);
    }
    command
}

fn wt_command() -> Command {
    let mut command = Command::cargo_bin("wt").unwrap();
    for key in GIT_LOCAL_ENV_KEYS {
        command.env_remove(key);
    }
    command.env_remove("WT_AGENT_ID");
    command.env_remove("WT_COORDINATOR_AGENT_ID");
    command
}

fn wt_std_command() -> StdCommand {
    let mut command = StdCommand::new(assert_cmd::cargo::cargo_bin("wt"));
    for key in GIT_LOCAL_ENV_KEYS {
        command.env_remove(key);
    }
    command.env_remove("WT_AGENT_ID");
    command.env_remove("WT_COORDINATOR_AGENT_ID");
    command
}

fn git_init(path: &Path) {
    let status = git_command()
        .arg("init")
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn send_message(repo: &Path, agent: &str, message: &str) {
    wt_command()
        .args([
            "-C",
            repo.to_str().unwrap(),
            "msg",
            "send",
            "--to",
            agent,
            message,
        ])
        .assert()
        .success();
}

fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

#[test]
fn msg_watch_drains_pending_messages_and_exits_quickly() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    for message in ["one", "two", "three"] {
        send_message(temp.path(), "codex", message);
    }

    let started = Instant::now();
    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert!(started.elapsed() < Duration::from_millis(500));
    let stdout = String::from_utf8(output).unwrap();
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("summary=\"one\""));
    assert!(lines[1].contains("summary=\"two\""));
    assert!(lines[2].contains("summary=\"three\""));
}

#[test]
fn msg_watch_waits_for_one_new_arrival() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    let child = wt_std_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "5",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    std::thread::sleep(Duration::from_millis(100));
    send_message(temp.path(), "codex", "arrived");
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("summary=\"arrived\""));
}

#[test]
fn msg_watch_times_out_silently() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    let started = Instant::now();
    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "1",
        ])
        .assert()
        .success()
        .stdout("");

    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(900));
    assert!(elapsed < Duration::from_secs(3));
}

#[test]
fn msg_watch_skips_message_that_is_claimed_by_a_racing_reader() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    send_message(temp.path(), "codex", "raced");
    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let claimed = inbox.join("claimed");
    std::fs::create_dir_all(&claimed).unwrap();

    let child = wt_std_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut moved = false;
    for _ in 0..100 {
        let files = toml_files(&inbox.join("new"));
        if let Some(path) = files.first() {
            let target = claimed.join(path.file_name().unwrap());
            std::fs::rename(path, target).unwrap();
            moved = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let output = child.wait_with_output().unwrap();
    assert!(moved, "expected race setup to move message into claimed");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn msg_watch_resolves_omitted_agent_from_coordinator_env() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    send_message(temp.path(), "coord-a", "env identity");

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--timeout",
            "1",
        ])
        .env("WT_COORDINATOR_AGENT_ID", "agents/coord-a")
        .assert()
        .success()
        .stdout(predicate::str::contains("summary=\"env identity\""));
}

#[test]
fn msg_watch_without_identity_errors_with_resolution_chain() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--timeout",
            "1",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("explicit --agent"))
        .stderr(predicate::str::contains("WT_COORDINATOR_AGENT_ID"))
        .stderr(predicate::str::contains("WT_AGENT_ID"));
}

#[test]
fn msg_watch_json_emits_ndjson_with_list_message_fields() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    send_message(temp.path(), "codex", "json row");

    let watch_output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "1",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let watch_row: serde_json::Value =
        serde_json::from_str(String::from_utf8(watch_output).unwrap().trim()).unwrap();

    let list_output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "msg",
            "list",
            "--agent",
            "codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list: serde_json::Value = serde_json::from_slice(&list_output).unwrap();

    assert_eq!(watch_row, list["messages"][0]);
}

#[test]
fn msg_watch_default_format_matches_msg_list_row() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    send_message(temp.path(), "codex", "golden row");

    let list_output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "list",
            "--agent",
            "codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let list_stdout = String::from_utf8(list_output).unwrap();
    let list_row = list_stdout.lines().nth(1).unwrap();

    let watch_output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let watch_stdout = String::from_utf8(watch_output).unwrap();

    assert_eq!(watch_stdout.trim(), list_row);
}

#[test]
fn msg_watch_rejects_zero_timeout() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "watch",
            "--agent",
            "codex",
            "--timeout",
            "0",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value '0'"))
        .stderr(predicate::str::contains("--timeout <TIMEOUT>"))
        .stderr(predicate::str::contains("0 is not in 1.."));
}

#[test]
fn msg_watch_help_documents_flags() {
    wt_command()
        .args(["msg", "watch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--agent"))
        .stdout(predicate::str::contains("--timeout"))
        .stdout(predicate::str::contains("--json"));
}
