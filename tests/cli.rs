use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as StdCommand};
use std::time::{Duration, Instant};
#[cfg(unix)]
use wt::services::identity_locator::{percent_encode, process_start_time};

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

const CLAUDE_INBOX_HOOK_COMMAND: &str =
    "wt msg check-inbox --silent 2>/dev/null || true # wt-agent-hook:claude-inbox";
const CLAUDE_SUPERVISOR_SESSION_END_HOOK_COMMAND: &str = "if [ -n \"${WT_AGENT_ID:-}\" ]; then wt agent supervisor stop --owned-by \"$WT_AGENT_ID\"; fi # wt-agent-hook:claude-supervisor-session-end";
const CODEX_INBOX_HOOK_MARKER: &str = "# wt-agent-hook:codex-inbox";
const MANAGED_INBOX_HOOK_EVENTS: &[(&str, &str)] = &[
    ("UserPromptSubmit", "user_prompt_submit"),
    ("PostToolUse", "post_tool_use"),
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
    command.env_remove("WT_TASK_RUN_ID");
    command
}

fn wt_command_at(path: &Path) -> Command {
    let mut command = Command::new(path);
    for key in GIT_LOCAL_ENV_KEYS {
        command.env_remove(key);
    }
    command.env_remove("WT_AGENT_ID");
    command.env_remove("WT_TASK_RUN_ID");
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

fn git_commit(path: &Path) {
    std::fs::write(path.join("README.md"), "sample\n").unwrap();
    let status = git_command()
        .args(["add", "README.md"])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
    let status = git_command()
        .args([
            "-c",
            "user.name=wt test",
            "-c",
            "user.email=wt@example.com",
            "commit",
            "-m",
            "initial",
        ])
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn current_branch(path: &Path) -> String {
    let output = git_command()
        .args(["branch", "--show-current"])
        .current_dir(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn path_with_fake_bin(fake_bin: &Path) -> std::ffi::OsString {
    let mut paths = vec![fake_bin.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap()
}

fn path_with_bins(bins: &[PathBuf]) -> std::ffi::OsString {
    let mut paths = bins.to_vec();
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap()
}

#[cfg(unix)]
fn write_fake_gh(path: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = path.join("fake-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        r#"#!/bin/sh
set -eu
if [ "${WT_FAKE_GH_MARK:-}" != "" ]; then
  echo "$*" >> "$WT_FAKE_GH_MARK"
fi

if [ "$1" = "pr" ] && [ "$2" = "list" ]; then
  printf '%s\n' '[{"number":42,"state":"OPEN","updatedAt":"2026-05-19T00:00:00Z"}]'
  exit 0
fi

if [ "$1" = "pr" ] && [ "$2" = "view" ]; then
  cat <<'JSON'
{"number":42,"title":"Add PR evidence","url":"https://github.com/acme/widgets/pull/42","state":"OPEN","isDraft":false,"headRefName":"feature","headRefOid":"abc123","baseRefName":"main","mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","reviewDecision":"REVIEW_REQUIRED","latestReviews":[{"author":{"login":"coderabbitai"},"state":"COMMENTED","commitId":"abc123","submittedAt":"2026-05-19T00:00:00Z","url":"https://github.com/acme/widgets/pull/42#pullrequestreview-1"}],"reviewRequests":[],"reactionGroups":[{"content":"THUMBS_UP","users":{"totalCount":1,"nodes":[{"login":"chatgpt-codex-connector"}]}}],"comments":[],"statusCheckRollup":[{"name":"CI","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://github.com/acme/widgets/actions/runs/1"}]}
JSON
  exit 0
fi

if [ "$1" = "repo" ] && [ "$2" = "view" ]; then
  printf '%s\n' '{"owner":{"login":"acme"},"name":"widgets"}'
  exit 0
fi

if [ "$1" = "api" ] && [ "$2" = "repos/acme/widgets/pulls/42/reviews?per_page=100" ]; then
  cat <<'JSON'
[{"user":{"login":"coderabbitai"},"state":"COMMENTED","commit_id":"abc123","submitted_at":"2026-05-19T00:00:00Z","html_url":"https://github.com/acme/widgets/pull/42#pullrequestreview-1"}]
JSON
  exit 0
fi

if [ "$1" = "api" ] && [ "$2" = "graphql" ]; then
  printf '%s\n' '{"data":{"repository":{"pullRequest":{"reviewThreads":{"totalCount":0,"nodes":[]}}}}}'
  exit 0
fi

echo "unexpected fake gh invocation: $*" >&2
exit 1
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&gh, permissions).unwrap();
    bin
}

#[cfg(not(unix))]
fn write_fake_gh(_path: &Path) -> std::path::PathBuf {
    panic!("fake gh test helper is only implemented for Unix test environments")
}

#[cfg(unix)]
fn write_fake_agent(path: &Path, name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = path.join("fake-agent-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let agent = bin.join(name);
    std::fs::write(
        &agent,
        r#"#!/bin/sh
printf 'WT_AGENT_ID=%s\n' "${WT_AGENT_ID:-}"
printf 'WT_COORDINATOR_AGENT_ID=%s\n' "${WT_COORDINATOR_AGENT_ID:-}"
printf 'ARGS=%s\n' "$*"
printf 'PWD=%s\n' "$PWD"
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&agent).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&agent, permissions).unwrap();
    bin
}

#[cfg(not(unix))]
fn write_fake_agent(_path: &Path, _name: &str) -> std::path::PathBuf {
    panic!("fake agent test helper is only implemented for Unix test environments")
}

#[cfg(unix)]
fn copy_wt_binary(path: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = path.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let wt = bin.join("wt");
    std::fs::copy(assert_cmd::cargo::cargo_bin("wt"), &wt).unwrap();
    std::fs::File::open(&wt).unwrap().sync_all().unwrap();
    let mut permissions = std::fs::metadata(&wt).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wt, permissions).unwrap();
    std::fs::File::open(&bin).unwrap().sync_all().unwrap();
    wt
}

fn write_task_document(root: &Path, key: &str, branch: &str) {
    let dir = root.join(".git/wt/execution/tasks");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{key}.toml")),
        format!(
            r#"title = "{key}"
branch = "{branch}"
body = "Task body"
"#
        ),
    )
    .unwrap();
}

fn write_task_run_file(root: &Path, id: &str, task: &str, branch: &str, status: &str, group: &str) {
    let dir = root.join(".git/wt/execution/task-runs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{id}.toml")),
        format!(
            r#"task = "{task}"
branch = "{branch}"
status = "{status}"
group = "{group}"
creation_order = 1
created_at = "2026-05-18T00:00:00.000000000Z"
updated_at = "2026-05-18T00:00:00.000000000Z"
"#
        ),
    )
    .unwrap();
}

fn write_task_run_file_with_coordinator(
    root: &Path,
    id: &str,
    task: &str,
    branch: &str,
    status: &str,
    group: &str,
    coordinator_id: Option<&str>,
) {
    write_task_run_file_with_routes(
        root,
        id,
        task,
        branch,
        status,
        group,
        (None, coordinator_id),
    );
}

fn write_task_run_file_with_routes(
    root: &Path,
    id: &str,
    task: &str,
    branch: &str,
    status: &str,
    group: &str,
    routes: (Option<&str>, Option<&str>),
) {
    let dir = root.join(".git/wt/execution/task-runs");
    std::fs::create_dir_all(&dir).unwrap();
    let (agent_id, coordinator_id) = routes;
    let agent = agent_id
        .map(|id| format!("agent_id = \"{id}\"\n"))
        .unwrap_or_default();
    let coordinator = coordinator_id
        .map(|id| format!("coordinator_id = \"{id}\"\n"))
        .unwrap_or_default();
    let group = if group.is_empty() {
        String::new()
    } else {
        format!("group = \"{group}\"\n")
    };
    std::fs::write(
        dir.join(format!("{id}.toml")),
        format!(
            r#"task = "{task}"
branch = "{branch}"
status = "{status}"
{group}creation_order = 1
{agent}{coordinator}created_at = "2026-05-18T00:00:00.000000000Z"
updated_at = "2026-05-18T00:00:00.000000000Z"
"#
        ),
    )
    .unwrap();
}

#[cfg(unix)]
fn spawn_sleeping_child() -> Child {
    StdCommand::new("sh")
        .arg("-c")
        .arg("sleep 30")
        .spawn()
        .unwrap()
}

#[cfg(unix)]
fn spawn_term_ignoring_child() -> Child {
    StdCommand::new("sh")
        .arg("-c")
        .arg("trap '' TERM; while :; do sleep 1; done")
        .spawn()
        .unwrap()
}

#[cfg(unix)]
fn write_supervisor_registration(
    root: &Path,
    agent_id: &str,
    pid: u32,
    started_by: &str,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
) -> PathBuf {
    write_supervisor_registration_with_cleanup(
        root,
        agent_id,
        pid,
        started_by,
        started_by.starts_with("agents/"),
        stale_threshold_secs,
        poll_interval_secs,
    )
}

#[cfg(unix)]
fn write_supervisor_registration_with_cleanup(
    root: &Path,
    agent_id: &str,
    pid: u32,
    started_by: &str,
    cleanup_on_session_end: bool,
    stale_threshold_secs: u64,
    poll_interval_secs: u64,
) -> PathBuf {
    let dir = root.join(".git/wt/supervisors");
    std::fs::create_dir_all(&dir).unwrap();
    let encoded = percent_encode(agent_id);
    let path = dir.join(format!("{encoded}.toml"));
    let log_path = dir.join(format!("{encoded}.log"));
    let pid_start_time = process_start_time(pid as i32).unwrap();
    std::fs::write(
        &path,
        format!(
            r#"agent_id = "{agent_id}"
pid = {pid}
pid_start_time = "{pid_start_time}"
started_at = "2026-05-22T00:00:00Z"
started_by = "{started_by}"
cleanup_on_session_end = {cleanup_on_session_end}
stale_threshold_secs = {stale_threshold_secs}
poll_interval_secs = {poll_interval_secs}
log_path = "{}"
"#,
            log_path.display()
        ),
    )
    .unwrap();
    path
}

fn write_wait_observations(root: &Path, content: &str) {
    let dir = root.join(".git/wt/agent.state");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("wait-observations.jsonl"), content).unwrap();
}

fn write_workflow_file(root: &Path, id: &str, mode: &str, extra: &str, tasks: &str) {
    let dir = root.join(".git/wt/execution/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{id}.toml")),
        format!(
            r#"mode = "{mode}"
{extra}base_mode = "explicit"
base = "main"
color = "red"
created_at = "2026-05-18T00:00:00Z"
updated_at = "2026-05-18T00:00:00Z"

[policy]
pull_request = "draft"
landing = "manual"

{tasks}"#
        ),
    )
    .unwrap();
}

fn write_personal_config(root: &Path, content: &str) {
    let dir = root.join(".git/wt/config");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("local.toml"), content).unwrap();
}

fn wt_state_dir_names(root: &Path) -> Vec<String> {
    let mut dirs = std::fs::read_dir(root.join(".git/wt"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn message_path_with_summary(dir: &Path, summary: &str) -> PathBuf {
    toml_files(dir)
        .into_iter()
        .find(|path| {
            let content = std::fs::read_to_string(path).unwrap();
            let message: toml::Value = toml::from_str(&content).unwrap();
            message["body"]["summary"].as_str() == Some(summary)
        })
        .unwrap()
}

fn claim_message_file(new_path: &Path, claimed_by: &str, lease_expires_at: &str) -> PathBuf {
    let mut message: toml::Value =
        toml::from_str(&std::fs::read_to_string(new_path).unwrap()).unwrap();
    if let Some(delivery) = message
        .get_mut("delivery")
        .and_then(toml::Value::as_table_mut)
    {
        delivery.insert("state".into(), toml::Value::String("claimed".into()));
        delivery.insert("claimed_by".into(), toml::Value::String(claimed_by.into()));
        delivery.insert(
            "lease_expires_at".into(),
            toml::Value::String(lease_expires_at.into()),
        );
        delivery.remove("last_error");
    }

    let inbox = new_path.parent().unwrap().parent().unwrap();
    let claimed_dir = inbox.join("claimed");
    std::fs::create_dir_all(&claimed_dir).unwrap();
    let claimed_path = claimed_dir.join(new_path.file_name().unwrap());
    std::fs::write(&claimed_path, toml::to_string_pretty(&message).unwrap()).unwrap();
    std::fs::remove_file(new_path).unwrap();
    claimed_path
}

fn retry_message_file(new_path: &Path, attempts: i64, last_error: &str) -> PathBuf {
    move_message_file_to_state(new_path, "retry", attempts, None, None, Some(last_error))
}

fn fail_message_file(new_path: &Path, attempts: i64, last_error: &str) -> PathBuf {
    move_message_file_to_state(new_path, "failed", attempts, None, None, Some(last_error))
}

fn move_message_file_to_state(
    new_path: &Path,
    state: &str,
    attempts: i64,
    claimed_by: Option<&str>,
    lease_expires_at: Option<&str>,
    last_error: Option<&str>,
) -> PathBuf {
    let mut message: toml::Value =
        toml::from_str(&std::fs::read_to_string(new_path).unwrap()).unwrap();
    if let Some(delivery) = message
        .get_mut("delivery")
        .and_then(toml::Value::as_table_mut)
    {
        delivery.insert("state".into(), toml::Value::String(state.into()));
        delivery.insert("attempts".into(), toml::Value::Integer(attempts));
        match claimed_by {
            Some(value) => {
                delivery.insert("claimed_by".into(), toml::Value::String(value.into()));
            }
            None => {
                delivery.remove("claimed_by");
            }
        }
        match lease_expires_at {
            Some(value) => {
                delivery.insert("lease_expires_at".into(), toml::Value::String(value.into()));
            }
            None => {
                delivery.remove("lease_expires_at");
            }
        }
        match last_error {
            Some(value) => {
                delivery.insert("last_error".into(), toml::Value::String(value.into()));
            }
            None => {
                delivery.remove("last_error");
            }
        }
    }

    let inbox = new_path.parent().unwrap().parent().unwrap();
    let state_dir = inbox.join(state);
    std::fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join(new_path.file_name().unwrap());
    std::fs::write(&state_path, toml::to_string_pretty(&message).unwrap()).unwrap();
    std::fs::remove_file(new_path).unwrap();
    state_path
}

fn write_conflicting_delivered_message(new_path: &Path) -> PathBuf {
    let mut message: toml::Value =
        toml::from_str(&std::fs::read_to_string(new_path).unwrap()).unwrap();
    message["body"]["summary"] = toml::Value::String("conflicting delivered payload".into());
    message["body"]["parts"][0]["content"] =
        toml::Value::String("conflicting delivered payload".into());
    if let Some(delivery) = message
        .get_mut("delivery")
        .and_then(toml::Value::as_table_mut)
    {
        delivery.insert("state".into(), toml::Value::String("delivered".into()));
        delivery.insert("attempts".into(), toml::Value::Integer(1));
        delivery.remove("claimed_by");
        delivery.remove("lease_expires_at");
        delivery.remove("last_error");
    }

    let inbox = new_path.parent().unwrap().parent().unwrap();
    let delivered_dir = inbox.join("delivered");
    std::fs::create_dir_all(&delivered_dir).unwrap();
    let delivered_path = delivered_dir.join(new_path.file_name().unwrap());
    std::fs::write(&delivered_path, toml::to_string_pretty(&message).unwrap()).unwrap();
    delivered_path
}

fn json_file(path: &Path) -> serde_json::Value {
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn hook_event_commands(hooks: &serde_json::Value, event_name: &str) -> Vec<String> {
    hooks[event_name]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|hook| hook["command"].as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn claude_event_commands(settings: &serde_json::Value, event_name: &str) -> Vec<String> {
    hook_event_commands(&settings["hooks"], event_name)
}

fn codex_event_commands(hooks: &serde_json::Value, event_name: &str) -> Vec<String> {
    hook_event_commands(&hooks["hooks"], event_name)
}

fn claude_managed_inbox_commands(settings: &serde_json::Value) -> Vec<String> {
    MANAGED_INBOX_HOOK_EVENTS
        .iter()
        .flat_map(|(event_name, _)| claude_event_commands(settings, event_name))
        .filter(|command| command.contains("wt-agent-hook:claude-inbox"))
        .collect()
}

fn codex_managed_inbox_commands(hooks: &serde_json::Value) -> Vec<String> {
    MANAGED_INBOX_HOOK_EVENTS
        .iter()
        .flat_map(|(event_name, _)| codex_event_commands(hooks, event_name))
        .filter(|command| command.contains("wt-agent-hook:codex-inbox"))
        .collect()
}

fn codex_dispatcher_command() -> String {
    format!("wt msg check-inbox --silent 2>/dev/null || true {CODEX_INBOX_HOOK_MARKER}")
}

#[test]
fn version_flag_prints_package_version() {
    wt_command()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("wt {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_subcommand_prints_package_version() {
    wt_command()
        .arg("version")
        .assert()
        .success()
        .stdout(format!("wt {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_subcommand_supports_json() {
    let output = wt_command()
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
    wt_command()
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: wt [OPTIONS] [COMMAND]"))
        .stdout(predicate::str::contains("run"))
        .stdout(predicate::str::contains("wt run issue"))
        .stdout(predicate::str::contains("wt run pr"))
        .stdout(predicate::str::contains("wt run branch"))
        .stdout(predicate::str::contains("wt run task"))
        .stdout(predicate::str::contains("wt run workflow"))
        .stdout(predicate::str::contains("new").not())
        .stdout(predicate::str::contains("agent"))
        .stdout(predicate::str::contains("wt help <cmd>"))
        .stdout(predicate::str::contains("\n  ui").not());
}

#[test]
fn session_set_writes_marker_and_prints_exports() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "my-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success()
        .stdout("export WT_AGENT_ID=agents/my-coord;\n")
        .stderr("");

    let files = toml_files(&temp.path().join(".git/wt/sessions"));
    assert_eq!(files.len(), 1);
    let content = std::fs::read_to_string(&files[0]).unwrap();
    let marker: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(marker["id"].as_str(), Some("agents/my-coord"));
    assert_eq!(marker["anchor_kind"].as_str(), Some("surface"));
    assert_eq!(marker["anchor_value"].as_str(), Some("surface-1"));
}

#[test]
fn session_set_rejects_invalid_ids_without_stdout() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "session", "set", ""])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("Agent id cannot be empty"));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "agents/foo/bar",
        ])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains(
            "Agent ids must be NAME or agents/NAME",
        ));
}

#[test]
fn session_whoami_reports_marker_and_json() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "my-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-1")
        .env("CMUX_WORKSPACE_ID", "workspace:1")
        .assert()
        .success();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "session", "whoami"])
        .env("CMUX_SURFACE_ID", "surface-1")
        .env("CMUX_WORKSPACE_ID", "workspace:1")
        .assert()
        .success()
        .stdout(predicate::str::contains("id: agents/my-coord"))
        .stdout(predicate::str::contains("source: marker"))
        .stdout(predicate::str::contains("anchor_kind: surface"))
        .stdout(predicate::str::contains("anchor_value: surface-1"))
        .stdout(predicate::str::contains("marker: "))
        .stdout(predicate::str::contains("cmux_workspace_id: workspace:1"));

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "whoami",
            "--json",
        ])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["id"].as_str(), Some("agents/my-coord"));
    assert_eq!(value["source"].as_str(), Some("marker"));
    assert_eq!(value["anchor_kind"].as_str(), Some("surface"));
    assert_eq!(value["anchor_value"].as_str(), Some("surface-1"));
    assert!(
        value["marker_path"]
            .as_str()
            .unwrap()
            .contains(".git/wt/sessions")
    );
}

#[test]
fn session_unset_removes_marker_and_whoami_reports_none() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "my-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "session", "unset"])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success()
        .stdout("unset WT_AGENT_ID;\n")
        .stderr("");

    assert!(toml_files(&temp.path().join(".git/wt/sessions")).is_empty());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "session", "whoami"])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success()
        .stdout(predicate::str::contains("id: none"))
        .stdout(predicate::str::contains("source: none"));
}

#[test]
fn session_whoami_reports_corrupt_marker_but_unset_can_recover() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "my-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success();

    let files = toml_files(&temp.path().join(".git/wt/sessions"));
    assert_eq!(files.len(), 1);
    std::fs::write(&files[0], "not valid toml = [").unwrap();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "session", "whoami"])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("Failed to parse marker"));

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "session", "unset"])
        .env("CMUX_SURFACE_ID", "surface-1")
        .assert()
        .success()
        .stdout("unset WT_AGENT_ID;\n")
        .stderr("");

    assert!(toml_files(&temp.path().join(".git/wt/sessions")).is_empty());
}

#[test]
fn msg_send_to_agents_coordinator_uses_ordinary_explicit_inbox() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/coordinator",
            "hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/coordinator/inbox/new/",
        ));

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/coordinator/inbox");
    let files = toml_files(&inbox.join("new"));
    assert_eq!(files.len(), 1);
}

#[test]
fn session_set_help_explains_eval_pattern() {
    wt_command()
        .args(["session", "set", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("eval \"$(wt session set <id>)\""));
}

#[test]
fn env_prints_worker_binding_for_matching_task_run() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let status = git_command()
        .args(["checkout", "-b", "feat-env"])
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    write_task_run_file_with_coordinator(
        temp.path(),
        "run-feat-env",
        "feat-env",
        "feat-env",
        "running",
        "2026-05-21-001",
        Some("agents/coord-a"),
    );

    wt_command()
        .current_dir(temp.path())
        .args(["env"])
        .assert()
        .success()
        .stdout("unset WT_COORDINATOR_AGENT_ID;\nexport WT_AGENT_ID=agents/feat-env;\n")
        .stderr("");
}

#[test]
fn env_prints_worker_binding_for_matching_task_run_without_extra_route() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let status = git_command()
        .args(["checkout", "-b", "feat-env"])
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    write_task_run_file_with_coordinator(
        temp.path(),
        "run-feat-env",
        "feat-env",
        "feat-env",
        "running",
        "2026-05-21-001",
        None,
    );

    wt_command()
        .current_dir(temp.path())
        .args(["env"])
        .assert()
        .success()
        .stdout("unset WT_COORDINATOR_AGENT_ID;\nexport WT_AGENT_ID=agents/feat-env;\n")
        .stderr("");
}

#[test]
fn env_unsets_identity_without_matching_task_run() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());

    wt_command()
        .current_dir(temp.path())
        .args(["env"])
        .assert()
        .success()
        .stdout("unset WT_AGENT_ID;\nunset WT_COORDINATOR_AGENT_ID;\n")
        .stderr("");
}

#[test]
fn env_unsets_identity_outside_git_repo() {
    let temp = TempDir::new().unwrap();

    wt_command()
        .current_dir(temp.path())
        .args(["env"])
        .assert()
        .success()
        .stdout("unset WT_AGENT_ID;\nunset WT_COORDINATOR_AGENT_ID;\n")
        .stderr("");
}

#[test]
fn shell_init_prints_valid_bash_source() {
    let temp = TempDir::new().unwrap();
    let output = wt_command()
        .args(["shell-init", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wt-env"))
        .stdout(predicate::str::contains("PROMPT_COMMAND"))
        .stdout(predicate::str::contains("declare -p PROMPT_COMMAND"))
        .stdout(predicate::str::contains("PROMPT_COMMAND+=(wt-env)"))
        .get_output()
        .stdout
        .clone();
    let script = temp.path().join("wt-init.bash");
    std::fs::write(&script, output).unwrap();

    let status = match StdCommand::new("bash")
        .args(["-n", script.to_str().unwrap()])
        .status()
    {
        Ok(status) => status,
        Err(err) => {
            eprintln!("skipping bash syntax check because bash is unavailable: {err}");
            return;
        }
    };
    assert!(status.success());
}

#[test]
fn shell_init_prints_valid_zsh_source_when_zsh_is_available() {
    if StdCommand::new("zsh").arg("--version").status().is_err() {
        return;
    }

    let temp = TempDir::new().unwrap();
    let output = wt_command()
        .args(["shell-init", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("chpwd_functions"))
        .get_output()
        .stdout
        .clone();
    let script = temp.path().join("wt-init.zsh");
    std::fs::write(&script, output).unwrap();

    let status = StdCommand::new("zsh")
        .args(["-n", script.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn shell_init_rejects_unsupported_shell_with_supported_list() {
    wt_command()
        .args(["shell-init", "fish"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("invalid value 'fish'"))
        .stderr(predicate::str::contains("[possible values: zsh, bash]"));
}

#[test]
fn run_branch_without_args_requires_branch_text() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "run", "branch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "wt run branch starts one ad hoc worktree from branch-name text",
        ));
}

#[test]
fn run_branch_task_option_is_unknown() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "run",
            "branch",
            "--task",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument '--task'"));
}

#[test]
fn run_branch_help_explains_branch_text_only() {
    wt_command()
        .args(["run", "branch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("branch-name text"))
        .stdout(predicate::str::contains("wt open"))
        .stdout(predicate::str::contains("--task").not());
}

#[test]
fn run_help_lists_execution_start_surfaces() {
    wt_command()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wt run issue"))
        .stdout(predicate::str::contains("wt run pr"))
        .stdout(predicate::str::contains("wt run branch"))
        .stdout(predicate::str::contains("wt run task"))
        .stdout(predicate::str::contains("wt run workflow"));
}

#[test]
fn run_issue_help_explains_issue_target() {
    wt_command()
        .args(["run", "issue", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Issue numbers or provider-specific keys",
        ))
        .stdout(predicate::str::contains("[ISSUE]..."))
        .stdout(predicate::str::contains("select multiple provider issues"))
        .stdout(predicate::str::contains("--jobs"))
        .stdout(predicate::str::contains("--matrix"))
        .stdout(predicate::str::contains("--profile"));
}

#[test]
fn run_pr_help_explains_multiple_targets() {
    wt_command()
        .args(["run", "pr", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull request numbers"))
        .stdout(predicate::str::contains("[PR]..."))
        .stdout(predicate::str::contains("select multiple open PRs"))
        .stdout(predicate::str::contains("--jobs"));
}

#[test]
fn old_execution_start_commands_are_removed() {
    for (args, old, new) in [
        (&["issue"][..], "wt issue", "wt run issue"),
        (&["pr"][..], "wt pr", "wt run pr"),
        (&["new"][..], "wt new", "wt run branch"),
        (&["task", "run"][..], "wt task run", "wt run task"),
        (
            &["workflow", "run"][..],
            "wt workflow run",
            "wt run workflow",
        ),
    ] {
        wt_command()
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(format!("`{old}` has moved")))
            .stderr(predicate::str::contains(format!("Use `{new}`")))
            .stderr(predicate::str::contains("not an alias"));
    }
}

#[test]
fn old_execution_start_help_surfaces_are_removed() {
    for (args, old, new) in [
        (&["issue", "--help"][..], "wt issue", "wt run issue"),
        (&["pr", "--help"][..], "wt pr", "wt run pr"),
        (&["new", "--help"][..], "wt new", "wt run branch"),
        (&["task", "run", "--help"][..], "wt task run", "wt run task"),
        (
            &["workflow", "run", "--help"][..],
            "wt workflow run",
            "wt run workflow",
        ),
    ] {
        wt_command()
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(format!("`{old}` has moved")))
            .stderr(predicate::str::contains(format!("Use `{new}`")))
            .stderr(predicate::str::contains("not an alias"));
    }
}

#[test]
fn run_task_help_explains_task_execution() {
    wt_command()
        .args(["run", "task", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("one worktree per selected"))
        .stdout(predicate::str::contains("direct TaskRun"))
        .stdout(predicate::str::contains("wt task report"))
        .stdout(predicate::str::contains("Agent Completion Report"))
        .stdout(predicate::str::contains("Task-run agents report PR=none"))
        .stdout(predicate::str::contains("wt workflow task --mode batch"))
        .stdout(predicate::str::contains("wt workflow task --mode single"))
        .stdout(predicate::str::contains("--jobs"));
}

#[test]
fn task_help_lists_list_import_and_publish() {
    wt_command()
        .args(["task", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("import"))
        .stdout(predicate::str::contains("run").not())
        .stdout(predicate::str::contains("publish"));
}

#[test]
fn task_list_help_explains_actionable_default_and_full_inventory() {
    wt_command()
        .args(["task", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("List actionable"))
        .stdout(predicate::str::contains(
            "same selectability rules as wt run task",
        ))
        .stdout(predicate::str::contains("prepared, failed, or skipped"))
        .stdout(predicate::str::contains("done or running are hidden"))
        .stdout(predicate::str::contains("--all"))
        .stdout(predicate::str::contains(
            "full read-only TaskDocument inventory",
        ))
        .stdout(predicate::str::contains(
            "reports invalid TaskDocument TOML files",
        ))
        .stdout(predicate::str::contains("does not start workspaces"))
        .stdout(predicate::str::contains("create TaskRuns"))
        .stdout(predicate::str::contains("prepare workflows"));
}

#[test]
fn task_list_supports_json_and_reports_invalid_tasks() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_document(temp.path(), "done", "feature/done");
    write_task_run_file(temp.path(), "run-done", "done", "feature/done", "done", "");
    write_task_document(temp.path(), "running", "feature/running");
    write_task_run_file(
        temp.path(),
        "run-running",
        "running",
        "feature/running",
        "running",
        "",
    );
    write_task_document(temp.path(), "prepared", "feature/prepared");
    write_task_run_file(
        temp.path(),
        "run-prepared",
        "prepared",
        "feature/prepared",
        "prepared",
        "",
    );
    write_task_document(temp.path(), "failed", "feature/failed");
    write_task_run_file(
        temp.path(),
        "run-failed",
        "failed",
        "feature/failed",
        "failed",
        "",
    );
    write_task_document(temp.path(), "skipped", "feature/skipped");
    write_task_run_file(
        temp.path(),
        "run-skipped",
        "skipped",
        "feature/skipped",
        "skipped",
        "",
    );
    write_task_document(temp.path(), "no-run", "feature/no-run");
    std::fs::write(
        temp.path().join(".git/wt/execution/tasks/provider.toml"),
        r#"title = "Provider task"
branch = "alice/provider-task"
body = "Imported provider task body"

[origin]
provider = "linear"
id = "PROJ-123"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".git/wt/execution/tasks/bad.toml"),
        "unknown = true\n",
    )
    .unwrap();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "task",
            "list",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let tasks = value["tasks"].as_array().unwrap();
    let invalid = value["invalid_tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 5);
    assert_eq!(invalid.len(), 1);
    assert_eq!(value.as_object().unwrap().len(), 2);

    assert!(tasks.iter().all(|row| row["key"] != "done"));
    assert!(tasks.iter().all(|row| row["key"] != "running"));
    for key in ["failed", "no-run", "prepared", "skipped"] {
        assert!(
            tasks.iter().any(|row| row["key"] == key),
            "{key} should be listed by default"
        );
    }

    let no_run = tasks
        .iter()
        .find(|row| row["key"] == "no-run")
        .expect("task without a TaskRun should be listed");
    assert_eq!(
        no_run["path"],
        "<git-common-dir>/wt/execution/tasks/no-run.toml"
    );
    assert_eq!(no_run["branch"], "feature/no-run");
    assert_eq!(no_run["publish_state"], "local");
    assert_eq!(no_run["source"], "local");
    assert!(no_run["origin"].is_null());

    let provider = tasks
        .iter()
        .find(|row| row["key"] == "provider")
        .expect("provider-origin task should be listed");
    assert_eq!(provider["title"], "Provider task");
    assert_eq!(provider["branch"], "alice/provider-task");
    assert_eq!(provider["publish_state"], "published");
    assert_eq!(provider["source"], "provider-origin");
    assert_eq!(provider["origin"]["provider"], "linear");
    assert_eq!(provider["origin"]["id"], "PROJ-123");
    assert_eq!(provider["body_summary"], "Imported provider task body");

    assert_eq!(invalid[0]["key"], "bad");
    assert_eq!(
        invalid[0]["path"],
        "<git-common-dir>/wt/execution/tasks/bad.toml"
    );
    assert!(
        invalid[0]["error"]
            .as_str()
            .unwrap()
            .contains("Failed to parse task")
    );

    let all_output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "task",
            "list",
            "--all",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let all_value: serde_json::Value = serde_json::from_slice(&all_output).unwrap();
    let all_tasks = all_value["tasks"].as_array().unwrap();
    let all_invalid = all_value["invalid_tasks"].as_array().unwrap();
    assert_eq!(all_tasks.len(), 7);
    assert_eq!(all_invalid.len(), 1);
    assert_eq!(all_value.as_object().unwrap().len(), 2);
    assert!(all_tasks.iter().any(|row| row["key"] == "done"));
    assert!(all_tasks.iter().any(|row| row["key"] == "running"));
}

#[test]
fn task_list_text_includes_stable_task_fields_and_invalid_warning() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_document(temp.path(), "done", "feature/done");
    write_task_run_file(temp.path(), "run-done", "done", "feature/done", "done", "");
    write_task_document(temp.path(), "running", "feature/running");
    write_task_run_file(
        temp.path(),
        "run-running",
        "running",
        "feature/running",
        "running",
        "",
    );
    write_task_document(temp.path(), "local", "feature/local");
    std::fs::write(
        temp.path().join(".git/wt/execution/tasks/provider.toml"),
        r#"title = "Provider task"
branch = "alice/provider-task"
body = "Provider task body"

[origin]
provider = "linear"
id = "PROJ-123"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".git/wt/execution/tasks/bad.toml"),
        "unknown = true\n",
    )
    .unwrap();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "task", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("◆ Tasks"))
        .stdout(predicate::str::contains("│ provider-origin"))
        .stdout(predicate::str::contains("│ local"))
        .stdout(predicate::str::contains(
            "•  local  not published  task local  branch feature/local",
        ))
        .stdout(predicate::str::contains(
            "•  Provider task  Linear PROJ-123  task provider  branch alice/provider-task",
        ))
        .stdout(predicate::str::contains(
            "2 tasks hidden; use wt task list --all to show the full inventory",
        ))
        .stdout(predicate::str::contains("task done").not())
        .stdout(predicate::str::contains("task running").not())
        .stdout(predicate::str::contains("Path:").not())
        .stdout(predicate::str::contains("Summary:").not())
        .stderr(predicate::str::contains(
            "Invalid task <git-common-dir>/wt/execution/tasks/bad.toml",
        ));
}

#[test]
fn task_list_all_text_keeps_full_grouped_inventory() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_document(temp.path(), "done", "feature/done");
    write_task_run_file(temp.path(), "run-done", "done", "feature/done", "done", "");
    write_task_document(temp.path(), "running", "feature/running");
    write_task_run_file(
        temp.path(),
        "run-running",
        "running",
        "feature/running",
        "running",
        "",
    );
    write_task_document(temp.path(), "local", "feature/local");
    std::fs::write(
        temp.path().join(".git/wt/execution/tasks/provider.toml"),
        r#"title = "Provider task"
branch = "alice/provider-task"
body = "Provider task body"

[origin]
provider = "linear"
id = "PROJ-123"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".git/wt/execution/tasks/bad.toml"),
        "unknown = true\n",
    )
    .unwrap();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "task", "list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("◆ Tasks"))
        .stdout(predicate::str::contains("│ provider-origin"))
        .stdout(predicate::str::contains("│ local"))
        .stdout(predicate::str::contains("task local"))
        .stdout(predicate::str::contains("branch feature/local"))
        .stdout(predicate::str::contains("task done"))
        .stdout(predicate::str::contains("branch feature/done"))
        .stdout(predicate::str::contains("task running"))
        .stdout(predicate::str::contains("branch feature/running"))
        .stdout(predicate::str::contains(
            "•  Provider task  Linear PROJ-123  task provider  branch alice/provider-task",
        ))
        .stdout(predicate::str::contains("tasks hidden").not())
        .stderr(predicate::str::contains(
            "Invalid task <git-common-dir>/wt/execution/tasks/bad.toml",
        ));
}

#[test]
fn task_list_empty_inventory_uses_plain_output() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "task", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No tasks found in <git-common-dir>/wt/execution/tasks",
        ))
        .stdout(predicate::str::contains("==>").not());
}

#[test]
fn task_import_help_explains_behavior() {
    wt_command()
        .args(["task", "import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Import existing provider issues"))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/execution/tasks/<safe-issue-id>.toml",
        ))
        .stdout(predicate::str::contains(
            "write title, branch, body, and [origin]",
        ))
        .stdout(predicate::str::contains("does not start workspaces"))
        .stdout(predicate::str::contains("create local branches"))
        .stdout(predicate::str::contains("create TaskRuns"))
        .stdout(predicate::str::contains("gh issue develop"))
        .stdout(predicate::str::contains("Omit issue ids"))
        .stdout(predicate::str::contains("duplicate issue ids"))
        .stdout(predicate::str::contains("existing local TaskDocument"));
}

#[test]
fn task_publish_help_explains_behavior() {
    wt_command()
        .args(["task", "publish", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("provider issue"))
        .stdout(predicate::str::contains(
            "rewrite branch to a provider-keyed branch",
        ))
        .stdout(predicate::str::contains("write [origin]"))
        .stdout(predicate::str::contains("does not start workspaces"))
        .stdout(predicate::str::contains("create local branches"))
        .stdout(predicate::str::contains("wt run task and wt run workflow"))
        .stdout(predicate::str::contains("Omit task keys"))
        .stdout(predicate::str::contains(
            "already have [origin] are excluded",
        ))
        .stdout(predicate::str::contains("already has origin"))
        .stdout(predicate::str::contains("checked-out worktree"));
}

#[test]
fn task_review_help_explains_behavior() {
    wt_command()
        .args(["task", "review", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("coordinator review feedback"))
        .stdout(predicate::str::contains("TaskRun.agent_id"))
        .stdout(predicate::str::contains("task_run:<id>"))
        .stdout(predicate::str::contains("--accept"))
        .stdout(predicate::str::contains("--reject"))
        .stdout(predicate::str::contains("--block"));
}

#[test]
fn msg_help_explains_agent_inbox_contract() {
    wt_command()
        .args(["msg", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("file-based agent inbox"))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/<agent>/inbox/<state>",
        ))
        .stdout(predicate::str::contains("wt msg send --to agents/<agent>"))
        .stdout(predicate::str::contains("wt task report <message>"))
        .stdout(predicate::str::contains("wt task review <task-run-id>"))
        .stdout(predicate::str::contains("wt msg list --agent <agent>"))
        .stdout(predicate::str::contains(
            "wt msg read --agent <agent> <message-id>",
        ))
        .stdout(predicate::str::contains("internal hook consumer"))
        .stdout(predicate::str::contains("inbox/new"))
        .stdout(predicate::str::contains("inbox/delivered"));

    wt_command()
        .args(["msg", "send", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Target agent id as NAME or agents/NAME",
        ))
        .stdout(predicate::str::contains(
            "Message ownership scope: direct, repo, workflow:<id>, or task_run:<id>",
        ))
        .stdout(predicate::str::contains(
            "Prefer `wt task report <message>`",
        ))
        .stdout(predicate::str::contains("wt task review <task-run-id>"))
        .stdout(predicate::str::contains("Message text"));

    wt_command()
        .args(["msg", "check-inbox", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hook JSON"))
        .stdout(predicate::str::contains("Explicit single agent id"))
        .stdout(predicate::str::contains("WT_AGENT_ID"))
        .stdout(predicate::str::contains("current live session marker"));

    wt_command()
        .args(["msg", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("without claiming them"))
        .stdout(predicate::str::contains("Agent id as NAME or agents/NAME"));

    wt_command()
        .args(["msg", "read", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("without changing delivery state"))
        .stdout(predicate::str::contains(
            "Message id without the .toml extension",
        ));
}

#[test]
fn msg_send_writes_to_agent_inbox_and_normalizes_agent_id() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "codex",
            "hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/codex/inbox/new/",
        ));

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let files = toml_files(&inbox.join("new"));
    assert_eq!(files.len(), 1);

    let content = std::fs::read_to_string(&files[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["meta"]["to"].as_str(), Some("agents/codex"));
    assert_eq!(message["meta"]["from"].as_str(), Some("agents/user"));
    assert_eq!(message["scope"]["kind"].as_str(), Some("direct"));
    assert_eq!(message["envelope"]["kind"].as_str(), Some("request"));
    assert_eq!(
        message["envelope"]["expects_response"].as_bool(),
        Some(true)
    );
    assert_eq!(message["delivery"]["state"].as_str(), Some("new"));
    assert_eq!(message["delivery"]["attempts"].as_integer(), Some(0));
    assert_eq!(message["body"]["summary"].as_str(), Some("hello"));
    assert_eq!(
        message["body"]["parts"][0]["content"].as_str(),
        Some("hello")
    );
}

#[test]
fn msg_send_to_bare_coordinator_writes_to_ordinary_coordinator_inbox() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "coordinator",
            "hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/coordinator/inbox/new/",
        ));

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/coordinator/inbox");
    let files = toml_files(&inbox.join("new"));
    assert_eq!(files.len(), 1);

    let content = std::fs::read_to_string(&files[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["meta"]["to"].as_str(), Some("agents/coordinator"));
    assert_eq!(message["meta"]["from"].as_str(), Some("agents/user"));
    assert_eq!(message["scope"]["kind"].as_str(), Some("direct"));
    assert!(message["scope"].get("id").is_none());
    assert_eq!(message["delivery"]["state"].as_str(), Some("new"));
    assert_eq!(
        message["body"]["parts"][0]["content"].as_str(),
        Some("hello")
    );
}

#[test]
fn msg_send_accepts_explicit_workflow_scope() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--scope",
            "workflow:2026-05-20-001",
            "--to",
            "agents/coord-a",
            "workflow",
            "owned",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/coord-a/inbox/new/",
        ));

    let inbox = temp.path().join(".git/wt/messages/agents/coord-a/inbox");
    let files = toml_files(&inbox.join("new"));
    assert_eq!(files.len(), 1);

    let content = std::fs::read_to_string(&files[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["meta"]["to"].as_str(), Some("agents/coord-a"));
    assert_eq!(message["scope"]["kind"].as_str(), Some("workflow"));
    assert_eq!(message["scope"]["id"].as_str(), Some("2026-05-20-001"));
    assert_eq!(message["body"]["summary"].as_str(), Some("workflow owned"));
}

#[test]
fn msg_send_rejects_invalid_scope() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--scope",
            "workflow",
            "--to",
            "coordinator",
            "missing",
            "id",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Message scope `workflow` requires an id",
        ));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--scope",
            "direct:2026-05-20-001",
            "--to",
            "coordinator",
            "direct",
            "id",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Message scope `direct` must not include an id",
        ));
}

#[test]
fn msg_send_to_derived_agent_id_targets_runtime_identity_inbox() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "issue-1-test",
            "runtime",
            "identity",
        ])
        .env("WT_AGENT_ID", "agents/issue-1-test")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/issue-1-test/inbox/new/",
        ));

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/issue-1-test/inbox");
    let files = toml_files(&inbox.join("new"));
    assert_eq!(files.len(), 1);

    let content = std::fs::read_to_string(&files[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["meta"]["to"].as_str(), Some("agents/issue-1-test"));
    assert_eq!(
        message["meta"]["from"].as_str(),
        Some("agents/issue-1-test")
    );

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/issue-1-test",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("runtime identity")
    );
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert!(toml_files(&inbox.join("claimed")).is_empty());
    assert_eq!(toml_files(&inbox.join("delivered")).len(), 1);
}

#[test]
fn msg_list_summarizes_lifecycle_states_without_mutating_messages() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    for summary in [
        "new summary",
        "claimed summary",
        "retry summary",
        "failed summary",
    ] {
        wt_command()
            .args([
                "-C",
                temp.path().to_str().unwrap(),
                "msg",
                "send",
                "--to",
                "coordinator",
                summary,
            ])
            .assert()
            .success();
    }

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/coordinator/inbox");
    let new_dir = inbox.join("new");
    let claimed_path = claim_message_file(
        &message_path_with_summary(&new_dir, "claimed summary"),
        "agents/supervisor",
        "2099-01-01T00:00:00Z",
    );
    let retry_path = retry_message_file(
        &message_path_with_summary(&new_dir, "retry summary"),
        2,
        "transport down",
    );
    let failed_path = fail_message_file(
        &message_path_with_summary(&new_dir, "failed summary"),
        3,
        "poison payload",
    );
    let failed_dir = inbox.join("failed");
    std::fs::write(failed_dir.join("z-poison.toml"), "not = [valid\n").unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "list",
        "--agent",
        "coordinator",
    ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "agents/coordinator messages: total 5 (new 1, claimed 1, delivered 0, retry 1, failed 2, invalid 1)",
        ))
        .stdout(predicate::str::contains(
            "new msg_",
        ))
        .stdout(predicate::str::contains("scope=direct"))
        .stdout(predicate::str::contains("summary=\"new summary\""))
        .stdout(predicate::str::contains(
            "claimed_by=agents/supervisor",
        ))
        .stdout(predicate::str::contains(
            "lease_expires_at=2099-01-01T00:00:00Z",
        ))
        .stdout(predicate::str::contains(
            "last_error=\"transport down\"",
        ))
        .stdout(predicate::str::contains(
            "last_error=\"poison payload\"",
        ))
        .stdout(predicate::str::contains("failed z-poison"))
        .stdout(predicate::str::contains("Failed to parse message"));

    assert!(claimed_path.exists());
    assert!(retry_path.exists());
    assert!(failed_path.exists());
    assert_eq!(toml_files(&new_dir).len(), 1);
    assert_eq!(toml_files(&inbox.join("claimed")).len(), 1);
    assert_eq!(toml_files(&inbox.join("retry")).len(), 1);
    assert_eq!(toml_files(&failed_dir).len(), 2);
}

#[test]
fn msg_list_json_uses_stable_lifecycle_inventory_shape() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "json",
            "new",
        ])
        .assert()
        .success();
    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    claim_message_file(
        &message_path_with_summary(&inbox.join("new"), "json new"),
        "agents/supervisor",
        "2099-01-01T00:00:00Z",
    );
    std::fs::create_dir_all(inbox.join("failed")).unwrap();
    std::fs::write(inbox.join("failed/bad.toml"), "not = [valid\n").unwrap();

    let output = wt_command()
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

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["agent"].as_str(), Some("agents/codex"));
    assert_eq!(value["counts"]["total"].as_u64(), Some(2));
    assert_eq!(value["counts"]["claimed"].as_u64(), Some(1));
    assert_eq!(value["counts"]["failed"].as_u64(), Some(1));
    assert_eq!(value["counts"]["invalid"].as_u64(), Some(1));
    assert_eq!(value["messages"][0]["state"].as_str(), Some("claimed"));
    assert_eq!(
        value["messages"][0]["claimed_by"].as_str(),
        Some("agents/supervisor")
    );
    assert_eq!(value["messages"][1]["state"].as_str(), Some("failed"));
    assert_eq!(value["messages"][1]["valid"].as_bool(), Some(false));
    assert!(
        value["messages"][1]["error"]
            .as_str()
            .unwrap()
            .contains("Failed to parse message")
    );
}

#[test]
fn msg_read_shows_claim_and_body_without_changing_state() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "codex",
            "claimed",
            "body",
        ])
        .assert()
        .success();
    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let claimed_path = claim_message_file(
        &message_path_with_summary(&inbox.join("new"), "claimed body"),
        "agents/supervisor",
        "2099-01-01T00:00:00Z",
    );
    let message_id = claimed_path.file_stem().unwrap().to_str().unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "read",
            "--agent",
            "agents/codex",
            message_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("state: claimed"))
        .stdout(predicate::str::contains("valid: true"))
        .stdout(predicate::str::contains("claimed_by: agents/supervisor"))
        .stdout(predicate::str::contains(
            "lease_expires_at: 2099-01-01T00:00:00Z",
        ))
        .stdout(predicate::str::contains("summary: claimed body"))
        .stdout(predicate::str::contains("claimed body"));

    assert!(claimed_path.exists());
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert!(!inbox.join("delivered").exists());
}

#[test]
fn msg_read_json_includes_full_message_payload() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "codex",
            "json",
            "read",
        ])
        .assert()
        .success();
    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let path = message_path_with_summary(&inbox.join("new"), "json read");
    let message_id = path.file_stem().unwrap().to_str().unwrap();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "msg",
            "read",
            "--agent",
            "codex",
            message_id,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["agent"].as_str(), Some("agents/codex"));
    assert_eq!(value["record"]["state"].as_str(), Some("new"));
    assert_eq!(value["record"]["valid"].as_bool(), Some(true));
    assert_eq!(
        value["message"]["body"]["summary"].as_str(),
        Some("json read")
    );
    assert_eq!(value["message"]["delivery"]["state"].as_str(), Some("new"));
    assert!(path.exists());
}

#[test]
fn msg_read_rejects_filename_extension_in_message_id() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "read",
            "--agent",
            "codex",
            "msg_example.toml",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Message id must not include the .toml extension",
        ));
}

#[test]
fn msg_check_inbox_emits_hook_json_and_acknowledges_claimed_messages() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "hello",
            "from",
            "claude",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value["hookSpecificOutput"]["hookEventName"].as_str(),
        Some("UserPromptSubmit")
    );
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/codex: 1 new message"));
    assert!(context.contains("hello from claude"));
    assert!(context.contains("wt msg send --to <agent> <message>"));

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert!(toml_files(&inbox.join("claimed")).is_empty());
    let delivered = toml_files(&inbox.join("delivered"));
    assert_eq!(delivered.len(), 1);
    let content = std::fs::read_to_string(&delivered[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["delivery"]["state"].as_str(), Some("delivered"));
    assert_eq!(message["delivery"]["attempts"].as_integer(), Some(1));
}

#[test]
fn msg_check_inbox_coordinator_delivers_workflow_scoped_messages() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_run_file_with_coordinator(
        temp.path(),
        "run-2026-05-20-001-workflow-report",
        "workflow-report",
        "workflow-report",
        "running",
        "2026-05-20-001",
        Some("agents/coord-a"),
    );
    write_workflow_file(
        temp.path(),
        "2026-05-20-001",
        "batch",
        "",
        r#"[[tasks]]
task = "workflow-report"
run = "run-2026-05-20-001-workflow-report"
"#,
    );

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--scope",
            "workflow:2026-05-20-001",
            "--to",
            "agents/coord-a",
            "workflow",
            "done",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/coord-a",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/coord-a: 1 new message"));
    assert!(context.contains("scope: workflow:2026-05-20-001"));
    assert!(context.contains("workflow done"));

    let inbox = temp.path().join(".git/wt/messages/agents/coord-a/inbox");
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert!(toml_files(&inbox.join("claimed")).is_empty());
    let delivered = toml_files(&inbox.join("delivered"));
    assert_eq!(delivered.len(), 1);
    let content = std::fs::read_to_string(&delivered[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["scope"]["kind"].as_str(), Some("workflow"));
    assert_eq!(message["delivery"]["state"].as_str(), Some("delivered"));
}

#[test]
fn task_report_workflow_scope_delivers_to_current_marker_coordinator_inbox() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_run_file_with_routes(
        temp.path(),
        "run-marker-workflow",
        "marker-workflow",
        "marker-workflow",
        "running",
        "2026-05-20-marker",
        (Some("agents/run-marker"), Some("agents/marker-coord")),
    );

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "marker-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-marker-workflow")
        .assert()
        .success();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "task",
            "report",
            "Agent Completion Report: Summary=workflow done; Changed files=src/lib.rs; Checks run=cargo test; PR=none; Risks or follow-ups=none",
        ])
        .env("WT_AGENT_ID", "agents/run-marker")
        .env("WT_TASK_RUN_ID", "run-marker-workflow")
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--silent",
        ])
        .env("CMUX_SURFACE_ID", "surface-marker-workflow")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/marker-coord: 1 new message"));
    assert!(context.contains("scope: workflow:2026-05-20-marker"));
    assert!(context.contains("Agent Completion Report: Summary=workflow done"));

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/marker-coord/inbox");
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert_eq!(toml_files(&inbox.join("delivered")).len(), 1);
}

#[test]
fn task_report_review_smoke_delivers_accepted_feedback_to_matching_task_agent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_run_file_with_routes(
        temp.path(),
        "run-smoke",
        "smoke",
        "smoke",
        "running",
        "",
        (Some("agents/run-1-smoke"), Some("agents/coord-a")),
    );

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "task",
            "report",
            "Agent Completion Report: Summary=done; Changed files=src/lib.rs; Checks run=cargo test; PR=none; Risks or follow-ups=none",
        ])
        .env("WT_AGENT_ID", "agents/run-1-smoke")
        .env("WT_TASK_RUN_ID", "run-smoke")
        .assert()
        .success();

    let task_run_path = temp
        .path()
        .join(".git/wt/execution/task-runs/run-smoke.toml");
    let task_run: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_run_path).unwrap()).unwrap();
    assert!(task_run["last_report_message_id"].as_str().is_some());
    assert!(task_run["last_reported_at"].as_str().is_some());

    let coordinator_output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("WT_AGENT_ID", "agents/coord-a")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let coordinator_json: serde_json::Value = serde_json::from_slice(&coordinator_output).unwrap();
    let coordinator_context = coordinator_json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(coordinator_context.contains("WT INBOX for agents/coord-a: 1 new message"));
    assert!(coordinator_context.contains("Agent Completion Report: Summary=done"));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "task",
            "review",
            "run-smoke",
            "--accept",
            "accepted",
            "for",
            "landing",
        ])
        .env("WT_AGENT_ID", "agents/coord-a")
        .assert()
        .success();

    let task_run: toml::Value =
        toml::from_str(&std::fs::read_to_string(&task_run_path).unwrap()).unwrap();
    assert_eq!(task_run["last_review_status"].as_str(), Some("accepted"));
    assert!(task_run["last_review_message_id"].as_str().is_some());
    assert!(task_run["last_reviewed_at"].as_str().is_some());

    let task_agent_output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("WT_AGENT_ID", "agents/run-1-smoke")
        .env("WT_TASK_RUN_ID", "run-smoke")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let task_agent_json: serde_json::Value = serde_json::from_slice(&task_agent_output).unwrap();
    let task_agent_context = task_agent_json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(task_agent_context.contains("WT INBOX for agents/run-1-smoke: 1 new message"));
    assert!(task_agent_context.contains("scope: task_run:run-smoke"));
    assert!(
        task_agent_context
            .contains("Coordinator Review: Status=accepted; Message=accepted for landing")
    );

    let task_inbox = temp
        .path()
        .join(".git/wt/messages/agents/run-1-smoke/inbox");
    assert!(toml_files(&task_inbox.join("new")).is_empty());
    assert_eq!(toml_files(&task_inbox.join("delivered")).len(), 1);
}

#[test]
fn msg_check_inbox_requires_runtime_task_run_binding_for_review_scope() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_run_file_with_routes(
        temp.path(),
        "run-review",
        "review",
        "review",
        "running",
        "",
        (Some("agents/run-1-review"), Some("agents/coord-a")),
    );

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "task",
            "review",
            "run-review",
            "--block",
            "waiting",
            "on",
            "approval",
        ])
        .env("WT_AGENT_ID", "agents/coord-a")
        .assert()
        .success();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/run-1-review",
        ])
        .env("WT_TASK_RUN_ID", "run-review")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/run-1-review/inbox");
    assert_eq!(toml_files(&inbox.join("new")).len(), 1);
    assert!(!inbox.join("delivered").exists());

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/run-1-review",
        ])
        .env("WT_AGENT_ID", "agents/run-1-review")
        .env("WT_TASK_RUN_ID", "run-review")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("scope: task_run:run-review"));
    assert!(context.contains("Coordinator Review: Status=blocked; Message=waiting on approval"));
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert_eq!(toml_files(&inbox.join("delivered")).len(), 1);
}

#[test]
fn msg_check_inbox_rejects_invalid_runtime_agent_env() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_run_file_with_routes(
        temp.path(),
        "run-review",
        "review",
        "review",
        "running",
        "",
        (Some("agents/run-1-review"), Some("agents/coord-a")),
    );

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/run-1-review",
        ])
        .env("WT_AGENT_ID", "agents/team/coord")
        .env("WT_TASK_RUN_ID", "run-review")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid WT_AGENT_ID"));
}

#[test]
fn msg_check_inbox_non_coordinator_leaves_workflow_scoped_messages_new() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--scope",
            "workflow:2026-05-20-001",
            "--to",
            "codex",
            "workflow",
            "owned",
        ])
        .assert()
        .success();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "codex",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let new = toml_files(&inbox.join("new"));
    assert_eq!(new.len(), 1);
    assert!(!inbox.join("claimed").exists());
    assert!(!inbox.join("delivered").exists());
    let content = std::fs::read_to_string(&new[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["scope"]["kind"].as_str(), Some("workflow"));
    assert_eq!(message["delivery"]["state"].as_str(), Some("new"));
}

#[test]
fn msg_check_inbox_does_not_steal_active_claims() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "claimed",
            "elsewhere",
        ])
        .assert()
        .success();

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let new = toml_files(&inbox.join("new"));
    let claimed_path = claim_message_file(&new[0], "agents/supervisor", "2099-01-01T00:00:00Z");

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/codex",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    assert!(claimed_path.exists());
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert!(!inbox.join("delivered").exists());
    let content = std::fs::read_to_string(&claimed_path).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(
        message["delivery"]["claimed_by"].as_str(),
        Some("agents/supervisor")
    );
}

#[test]
fn msg_check_inbox_reclaims_expired_claims_and_delivers_them() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "expired",
            "claim",
        ])
        .assert()
        .success();

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let new = toml_files(&inbox.join("new"));
    claim_message_file(&new[0], "agents/supervisor", "1970-01-01T00:00:01Z");

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("expired claim")
    );
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert!(toml_files(&inbox.join("claimed")).is_empty());
    assert!(toml_files(&inbox.join("retry")).is_empty());
    let delivered = toml_files(&inbox.join("delivered"));
    assert_eq!(delivered.len(), 1);
    let content = std::fs::read_to_string(&delivered[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["delivery"]["state"].as_str(), Some("delivered"));
    assert_eq!(message["delivery"]["attempts"].as_integer(), Some(2));
    assert!(message["delivery"].get("claimed_by").is_none());
    assert!(message["delivery"].get("lease_expires_at").is_none());
}

#[test]
fn msg_check_inbox_keeps_hook_stdout_json_when_acknowledge_fails() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "stdout",
            "json",
        ])
        .assert()
        .success();

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let new = toml_files(&inbox.join("new"));
    let delivered_conflict = write_conflicting_delivered_message(&new[0]);

    let assert = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/codex",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "already exists with different content",
        ));

    let output = assert.get_output();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["hookSpecificOutput"]["hookEventName"].as_str(),
        Some("UserPromptSubmit")
    );
    assert!(delivered_conflict.exists());
}

#[test]
fn msg_check_inbox_accepts_global_json_flag() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "codex",
            "json",
            "hook",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "msg",
            "check-inbox",
            "--agent",
            "codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("json hook")
    );
}

#[test]
fn msg_check_empty_inbox_exits_quietly() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/codex",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn msg_check_inbox_without_agent_and_no_marker_exits_quietly_without_creating_marker() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("CMUX_SURFACE_ID", "surface-empty")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let sessions = temp.path().join(".git/wt/sessions");
    assert!(!sessions.exists() || toml_files(&sessions).is_empty());
}

#[test]
fn msg_check_inbox_without_agent_uses_wt_agent_id_env() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "runtime",
            "env",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("WT_AGENT_ID", "agents/codex")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/codex: 1 new message"));
    assert!(context.contains("runtime env"));
}

#[test]
fn msg_check_inbox_without_agent_uses_current_live_session_marker() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "marker-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-marker")
        .assert()
        .success();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/marker-coord",
            "marker",
            "delivery",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--silent",
        ])
        .env("CMUX_SURFACE_ID", "surface-marker")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/marker-coord: 1 new message"));
    assert!(context.contains("marker delivery"));

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/marker-coord/inbox");
    assert!(toml_files(&inbox.join("new")).is_empty());
    assert_eq!(toml_files(&inbox.join("delivered")).len(), 1);
}

#[test]
fn msg_check_inbox_without_agent_ignores_non_current_session_marker() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "marker-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-original")
        .assert()
        .success();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/marker-coord",
            "other",
            "surface",
        ])
        .assert()
        .success();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--silent",
        ])
        .env("CMUX_SURFACE_ID", "surface-other")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let inbox = temp
        .path()
        .join(".git/wt/messages/agents/marker-coord/inbox");
    assert_eq!(toml_files(&inbox.join("new")).len(), 1);
    assert!(!inbox.join("delivered").exists());
}

#[test]
fn msg_check_inbox_without_agent_prefers_wt_agent_id_over_session_marker() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "marker-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-precedence")
        .assert()
        .success();

    for (agent, message) in [
        ("agents/codex", "runtime message"),
        ("agents/marker-coord", "marker message"),
    ] {
        wt_command()
            .args([
                "-C",
                temp.path().to_str().unwrap(),
                "msg",
                "send",
                "--to",
                agent,
                message,
            ])
            .assert()
            .success();
    }

    let output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("WT_AGENT_ID", "agents/codex")
        .env("CMUX_SURFACE_ID", "surface-precedence")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/codex: 1 new message"));
    assert!(context.contains("runtime message"));
    assert!(!context.contains("marker message"));

    let messages_root = temp.path().join(".git/wt/messages/agents");
    assert_eq!(
        toml_files(&messages_root.join("codex/inbox/delivered")).len(),
        1
    );
    assert_eq!(
        toml_files(&messages_root.join("marker-coord/inbox/new")).len(),
        1
    );
}

#[test]
fn msg_check_inbox_without_agent_uses_only_runtime_agent_id() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "runtime",
            "message",
        ])
        .assert()
        .success();
    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "coordinator",
            "coordinator",
            "message",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("WT_AGENT_ID", "agents/codex")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let lines = std::str::from_utf8(&output)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/codex: 1 new message"));
    assert!(context.contains("runtime message"));
    assert!(!context.contains("coordinator message"));

    let messages_root = temp.path().join(".git/wt/messages/agents");
    assert_eq!(
        toml_files(&messages_root.join("codex/inbox/delivered")).len(),
        1
    );
    assert_eq!(
        toml_files(&messages_root.join("coordinator/inbox/new")).len(),
        1
    );
}

#[test]
fn msg_check_inbox_without_agent_delivers_coordinator_when_runtime_agent_matches() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/coordinator",
            "deduped",
            "message",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .env("WT_AGENT_ID", "agents/coordinator")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let lines = std::str::from_utf8(&output)
        .unwrap()
        .lines()
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/coordinator: 1 new message"));
    assert!(context.contains("deduped message"));
}

#[test]
fn msg_check_inbox_explicit_agent_ignores_runtime_env_ids() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    for (agent, message) in [
        ("agents/manual", "manual override"),
        ("agents/codex", "runtime env"),
        ("agents/coordinator", "ordinary coordinator message"),
    ] {
        wt_command()
            .args([
                "-C",
                temp.path().to_str().unwrap(),
                "msg",
                "send",
                "--to",
                agent,
                message,
            ])
            .assert()
            .success();
    }
    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "session",
            "set",
            "marker-coord",
        ])
        .env("CMUX_SURFACE_ID", "surface-explicit")
        .assert()
        .success();
    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/marker-coord",
            "marker fallback",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/manual",
        ])
        .env("WT_AGENT_ID", "agents/codex")
        .env("CMUX_SURFACE_ID", "surface-explicit")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/manual: 1 new message"));
    assert!(context.contains("manual override"));
    assert!(!context.contains("runtime env"));
    assert!(!context.contains("ordinary coordinator message"));
    assert!(!context.contains("marker fallback"));

    let messages_root = temp.path().join(".git/wt/messages/agents");
    assert_eq!(
        toml_files(&messages_root.join("manual/inbox/delivered")).len(),
        1
    );
    assert_eq!(toml_files(&messages_root.join("codex/inbox/new")).len(), 1);
    assert_eq!(
        toml_files(&messages_root.join("coordinator/inbox/new")).len(),
        1
    );
    assert_eq!(
        toml_files(&messages_root.join("marker-coord/inbox/new")).len(),
        1
    );
}

#[test]
fn msg_check_inbox_silent_in_non_git_dir_exits_zero() {
    let temp = TempDir::new().unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--silent",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
}

#[test]
fn msg_check_inbox_silent_with_legacy_local_config_exits_zero() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    std::fs::create_dir_all(temp.path().join(".local")).unwrap();
    std::fs::write(
        temp.path().join(".local/.wt.toml"),
        "[agent]\ncli = \"codex\"\n",
    )
    .unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--silent",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
}

#[test]
fn msg_check_inbox_without_silent_still_errors_on_legacy_local_config() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    std::fs::create_dir_all(temp.path().join(".local")).unwrap();
    std::fs::write(
        temp.path().join(".local/.wt.toml"),
        "[agent]\ncli = \"codex\"\n",
    )
    .unwrap();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "msg", "check-inbox"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("legacy wt personal config"));
}

#[test]
fn msg_check_inbox_silent_still_delivers_when_context_healthy() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex",
            "silent",
            "delivery",
        ])
        .assert()
        .success();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "agents/codex",
            "--silent",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value["hookSpecificOutput"]["hookEventName"].as_str(),
        Some("UserPromptSubmit")
    );
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("WT INBOX for agents/codex: 1 new message"));
    assert!(context.contains("silent delivery"));

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    assert!(toml_files(&inbox.join("new")).is_empty());
    let delivered = toml_files(&inbox.join("delivered"));
    assert_eq!(delivered.len(), 1);
}

#[test]
fn msg_rejects_invalid_or_ambiguous_agent_ids() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex/worker",
            "hello",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid target agent id"))
        .stderr(predicate::str::contains(
            "Agent ids must be NAME or agents/NAME",
        ));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "humans/user",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("path-like ids are ambiguous"));
}

#[test]
fn msg_uses_git_common_messages_from_linked_worktree() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&repo).unwrap();
    git_init(&repo);
    git_commit(&repo);
    let status = git_command()
        .args([
            "worktree",
            "add",
            "-b",
            "linked",
            linked.to_str().unwrap(),
            "HEAD",
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(status.success());

    wt_command()
        .args([
            "-C",
            linked.to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "codex",
            "from",
            "linked",
        ])
        .assert()
        .success();

    let common_inbox = repo.join(".git/wt/messages/agents/codex/inbox");
    assert_eq!(toml_files(&common_inbox.join("new")).len(), 1);
    assert!(!linked.join(".git/wt/messages").exists());

    let output = wt_command()
        .args([
            "-C",
            linked.to_str().unwrap(),
            "msg",
            "check-inbox",
            "--agent",
            "codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("from linked")
    );
    assert!(toml_files(&common_inbox.join("new")).is_empty());
    assert_eq!(toml_files(&common_inbox.join("delivered")).len(), 1);
}

#[test]
fn run_workflow_help_explains_omitted_target_selection() {
    wt_command()
        .args(["run", "workflow", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Omit WORKFLOW"))
        .stdout(predicate::str::contains("choose from runnable workflows"))
        .stdout(predicate::str::contains(
            "does not list, edit, repair, or complete",
        ))
        .stdout(predicate::str::contains(
            "omit to select a runnable workflow",
        ));
}

#[test]
fn workflow_list_help_explains_canonical_inventory() {
    wt_command()
        .args(["workflow", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical read-only inventory"))
        .stdout(predicate::str::contains(
            "whether or not they are currently runnable",
        ))
        .stdout(predicate::str::contains(
            "reports invalid workflow TOML files",
        ))
        .stdout(predicate::str::contains(
            "derived action labels such as runnable, waiting, and done",
        ))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/execution/workflows",
        ))
        .stdout(predicate::str::contains(legacy_local_path("workflows")).not());
}

#[test]
fn workflow_archive_help_explains_visibility_retention_model() {
    wt_command()
        .args(["workflow", "archive", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/execution/archive/workflows",
        ))
        .stdout(predicate::str::contains(
            "Archive is a visibility and retention action",
        ))
        .stdout(predicate::str::contains("not a substitute for landing"))
        .stdout(predicate::str::contains("wt workflow complete"))
        .stdout(predicate::str::contains("wt done"))
        .stdout(predicate::str::contains("--discard").not());
}

#[test]
fn primary_help_surfaces_do_not_teach_legacy_local_storage_paths() {
    let help_surfaces: &[&[&str]] = &[
        &["--help"],
        &["task", "--help"],
        &["run", "task", "--help"],
        &["workflow", "--help"],
        &["config", "--help"],
        &["profile", "--help"],
        &["ui", "--help"],
    ];
    let stale_paths = [
        legacy_local_path("tasks"),
        legacy_local_path("task-runs"),
        legacy_local_path("workflows"),
        legacy_local_path("profiles"),
        legacy_local_path(".wt.toml"),
    ];

    for args in help_surfaces {
        let output = wt_command()
            .args(*args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let help = String::from_utf8(output).unwrap();
        for stale_path in &stale_paths {
            assert!(
                !help.contains(stale_path),
                "wt {} help still contains stale storage path {stale_path}",
                args.join(" ")
            );
        }
    }
}

fn legacy_local_path(child: &str) -> String {
    [".local", child].join("/")
}

#[test]
fn workflow_list_supports_json_and_reports_invalid_workflows() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_document(temp.path(), "schema", "feature/schema");
    write_task_run_file(
        temp.path(),
        "run-2026-05-18-001-schema",
        "schema",
        "feature/schema",
        "prepared",
        "2026-05-18-001",
    );
    write_workflow_file(
        temp.path(),
        "2026-05-18-001",
        "batch",
        r#"title = "Ship search"
profile = "codex"
"#,
        r#"[[tasks]]
task = "schema"
run = "run-2026-05-18-001-schema"
"#,
    );
    std::fs::write(
        temp.path().join(".git/wt/execution/workflows/bad.toml"),
        "mode = \"batch\"\n",
    )
    .unwrap();

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "workflow",
            "list",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let workflows = value["workflows"].as_array().unwrap();
    let invalid = value["invalid_workflows"].as_array().unwrap();
    assert_eq!(workflows.len(), 1);
    assert_eq!(invalid.len(), 1);

    let row = &workflows[0];
    assert_eq!(row["id"], "2026-05-18-001");
    assert_eq!(
        row["path"],
        "<git-common-dir>/wt/execution/workflows/2026-05-18-001.toml"
    );
    assert_eq!(row["mode"], "batch");
    assert_eq!(row["title"], "Ship search");
    assert_eq!(row["task_count"], 1);
    assert_eq!(row["task_runs"]["prepared"], 1);
    assert_eq!(row["task_runs"]["summary"], "1 prepared");
    assert_eq!(row["runnable"]["runnable"], true);
    assert_eq!(row["runnable"]["runnable_count"], 1);
    assert_eq!(row["base"], "main");
    assert_eq!(row["profile"], "codex");
    assert_eq!(row["policy"]["pull_request"], "draft");
    assert_eq!(row["policy"]["landing"], "manual");
    assert!(row["state_error"].is_null());
    assert_eq!(invalid[0]["id"], "bad");
    assert_eq!(
        invalid[0]["path"],
        "<git-common-dir>/wt/execution/workflows/bad.toml"
    );
    assert!(
        invalid[0]["error"]
            .as_str()
            .unwrap()
            .contains("Failed to parse workflow")
    );
}

#[test]
fn workflow_list_empty_inventory_uses_plain_output() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "workflow", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No workflows found in <git-common-dir>/wt/execution/workflows",
        ))
        .stdout(predicate::str::contains("==>").not());
}

#[test]
fn workflow_archive_moves_completed_workflow_out_of_active_inventory() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_document(temp.path(), "archive-unique", "archive-unique");
    write_task_run_file(
        temp.path(),
        "run-archive-unique",
        "archive-unique",
        "archive-unique",
        "done",
        "2026-05-20-009",
    );
    write_workflow_file(
        temp.path(),
        "2026-05-20-009",
        "batch",
        r#"title = "Archive me"
"#,
        r#"[[tasks]]
task = "archive-unique"
run = "run-archive-unique"
"#,
    );

    let output = wt_command()
        .current_dir(temp.path())
        .args(["--json", "workflow", "archive", "2026-05-20-009"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let archive_report: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(archive_report["workflow_id"], "2026-05-20-009");
    assert_eq!(
        archive_report["workflow_source_path"],
        "execution/workflows/2026-05-20-009.toml"
    );

    let archive = temp
        .path()
        .join(".git/wt/execution/archive/workflows/2026-05-20-009");
    assert!(archive.join("workflow.toml").exists());
    assert!(archive.join("task-runs/run-archive-unique.toml").exists());
    assert!(archive.join("tasks/archive-unique.toml").exists());
    assert!(
        !temp
            .path()
            .join(".git/wt/execution/workflows/2026-05-20-009.toml")
            .exists()
    );
    assert!(
        !temp
            .path()
            .join(".git/wt/execution/task-runs/run-archive-unique.toml")
            .exists()
    );
    assert!(
        !temp
            .path()
            .join(".git/wt/execution/tasks/archive-unique.toml")
            .exists()
    );

    let manifest: toml::Value =
        toml::from_str(&std::fs::read_to_string(archive.join("manifest.toml")).unwrap()).unwrap();
    assert_eq!(manifest["workflow_id"].as_str(), Some("2026-05-20-009"));
    assert_eq!(
        manifest["workflow_archive_path"].as_str(),
        Some("execution/archive/workflows/2026-05-20-009/workflow.toml")
    );
    let task_runs = manifest["task_runs"].as_array().unwrap();
    assert_eq!(
        task_runs[0]["source_path"].as_str(),
        Some("execution/task-runs/run-archive-unique.toml")
    );
    assert_eq!(
        task_runs[0]["archive_path"].as_str(),
        Some("execution/archive/workflows/2026-05-20-009/task-runs/run-archive-unique.toml")
    );
    let tasks = manifest["tasks"].as_array().unwrap();
    assert_eq!(
        tasks[0]["source_path"].as_str(),
        Some("execution/tasks/archive-unique.toml")
    );
    assert_eq!(
        tasks[0]["archive_path"].as_str(),
        Some("execution/archive/workflows/2026-05-20-009/tasks/archive-unique.toml")
    );

    let output = wt_command()
        .current_dir(temp.path())
        .args(["--json", "workflow", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let workflows: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(workflows["workflows"].as_array().unwrap().is_empty());

    let output = wt_command()
        .current_dir(temp.path())
        .args(["--json", "task", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let tasks: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(tasks["tasks"].as_array().unwrap().is_empty());
}

#[test]
fn scaffold_supports_json_global_flag() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    let output = wt_command()
        .current_dir(temp.path())
        .args(["--json", "scaffold", "demo", "--retrospect"])
        .assert()
        .success()
        .stderr(predicate::str::contains("JSON output is supported").not())
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["feature"], "demo");
    assert_eq!(
        value["created"][0],
        "<git-common-dir>/wt/planning/specs/demo/11-retrospect.md"
    );
    assert!(value["skipped"].as_array().unwrap().is_empty());
    assert!(
        temp.path()
            .join(".git/wt/planning/specs/demo/11-retrospect.md")
            .is_file()
    );
}

#[test]
fn workflow_prepare_help_uses_pr_mode() {
    wt_command()
        .args(["workflow", "task", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--pr <none|draft|ready>"))
        .stdout(predicate::str::contains("[workflow].pull_request"))
        .stdout(predicate::str::contains(format!("workflow.{}", "defaults")).not())
        .stdout(predicate::str::contains("--pull-request").not());

    wt_command()
        .args(["workflow", "issue", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--pr <none|draft|ready>"))
        .stdout(predicate::str::contains("[workflow].pull_request"))
        .stdout(predicate::str::contains(format!("workflow.{}", "defaults")).not())
        .stdout(predicate::str::contains("--pull-request").not());
}

#[test]
fn workflow_prepare_accepts_pr_on_non_stack_modes() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "workflow",
            "task",
            "--mode",
            "single",
            "--pr",
            "draft",
            "workflow-docs",
            "--base",
            "main",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Prepared workflow:"));

    let workflows = std::fs::read_dir(temp.path().join(".git/wt/execution/workflows"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(workflows.len(), 1);
    let content = std::fs::read_to_string(workflows[0].path()).unwrap();
    assert!(content.contains("pull_request = \"draft\""));
}

#[test]
fn workflow_state_is_visible_from_linked_worktree() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&repo).unwrap();
    git_init(&repo);
    git_commit(&repo);
    let base = current_branch(&repo);

    let status = git_command()
        .args([
            "worktree",
            "add",
            "-b",
            "linked-workflow-state",
            linked.to_str().unwrap(),
            "HEAD",
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(status.success());

    wt_command()
        .args([
            "-C",
            repo.to_str().unwrap(),
            "workflow",
            "task",
            "--mode",
            "batch",
            "linked-workflow-task",
            "--base",
            &base,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Prepared workflow: linked-workflow-task",
        ))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/execution/workflows/",
        ));

    assert!(!repo.join(legacy_local_path("workflows")).exists());
    let workflows = std::fs::read_dir(repo.join(".git/wt/execution/workflows"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(workflows.len(), 1);

    let output = wt_command()
        .args(["-C", linked.to_str().unwrap(), "--json", "workflow", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let workflows = value["workflows"].as_array().unwrap();
    assert_eq!(workflows.len(), 1);
    let path = workflows[0]["path"].as_str().unwrap();
    assert!(path.starts_with("<git-common-dir>/wt/execution/workflows/"));
    assert!(path.ends_with(".toml"));
}

#[test]
fn agent_status_help_explains_polling_target() {
    wt_command()
        .args(["agent", "status", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("task agent"))
        .stdout(predicate::str::contains("[TARGET]"))
        .stdout(predicate::str::contains(
            "Branch, worktree path/name, or TaskRun id",
        ))
        .stdout(predicate::str::contains(
            "Omit TARGET in an interactive terminal",
        ))
        .stdout(predicate::str::contains("read-only"))
        .stdout(predicate::str::contains("cmux hooks codex install --yes"));
}

#[test]
fn agent_watch_help_explains_polling_target() {
    wt_command()
        .args(["agent", "watch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("task agent"))
        .stdout(predicate::str::contains("[TARGET]"))
        .stdout(predicate::str::contains("--interval"))
        .stdout(predicate::str::contains("--timeout"))
        .stdout(predicate::str::contains("--heartbeat"))
        .stdout(predicate::str::contains("--record-wait-observations").not())
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/agent.state/wait-observations.jsonl",
        ))
        .stdout(predicate::str::contains(
            "When --timeout or --heartbeat emits a non-idle sample",
        ))
        .stdout(predicate::str::contains("unchanged running observations"))
        .stdout(predicate::str::contains(
            "Omit TARGET in an interactive terminal",
        ));
}

#[test]
fn agent_wait_stats_help_explains_read_only_summary() {
    wt_command()
        .args(["agent", "wait-stats", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("non-idle wait observations"))
        .stdout(predicate::str::contains("read-only"))
        .stdout(predicate::str::contains("average"))
        .stdout(predicate::str::contains("low-cardinality group data"))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/agent.state/wait-observations.jsonl",
        ))
        .stdout(predicate::str::contains("does not observe agents"))
        .stdout(predicate::str::contains("mutate TaskRuns"));
}

#[test]
fn agent_wait_stats_reports_sample_jsonl_summary() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_wait_observations(
        temp.path(),
        r#"{"recorded_at":"2026-05-20T00:00:00Z","wait_class":"non_idle","wait_reason":"heartbeat","elapsed_seconds":60,"bound_seconds":60,"unchanged_seconds":60,"target":"feature","branch":"feature","worktree":null,"task_run_id":null,"agent_kind":"codex","agent_state":"running","last_tool":null,"last_event_at":null,"session_id":null}
{"recorded_at":"2026-05-20T00:01:00Z","wait_class":"non_idle","wait_reason":"heartbeat","elapsed_seconds":120,"bound_seconds":60,"unchanged_seconds":60,"target":"feature","branch":"feature","worktree":null,"task_run_id":null,"agent_kind":"codex","agent_state":"running","last_tool":null,"last_event_at":null,"session_id":null}
{"recorded_at":"2026-05-20T00:05:00Z","wait_class":"non_idle","wait_reason":"timeout","elapsed_seconds":300,"bound_seconds":300,"unchanged_seconds":180,"target":"feature","branch":"feature","worktree":null,"task_run_id":null,"agent_kind":"claude_code","agent_state":"running","last_tool":null,"last_event_at":null,"session_id":null}
"#,
    );

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "agent", "wait-stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent wait stats"))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/agent.state/wait-observations.jsonl",
        ))
        .stdout(predicate::str::contains("Count: 3"))
        .stdout(predicate::str::contains("Sum seconds: 480"))
        .stdout(predicate::str::contains("Average seconds: 160"))
        .stdout(predicate::str::contains("Min seconds: 60"))
        .stdout(predicate::str::contains("Max seconds: 300"))
        .stdout(predicate::str::contains("1-4m: 2"))
        .stdout(predicate::str::contains("5-14m: 1"))
        .stdout(predicate::str::contains("wait_reason:"))
        .stdout(predicate::str::contains("heartbeat: count 2, avg 90s"))
        .stdout(predicate::str::contains("timeout: count 1, avg 300s"))
        .stdout(predicate::str::contains("bound_seconds:"))
        .stdout(predicate::str::contains("60: count 2, avg 90s"))
        .stdout(predicate::str::contains("agent_kind:"))
        .stdout(predicate::str::contains("codex: count 2, avg 90s"))
        .stdout(predicate::str::contains("claude_code: count 1, avg 300s"))
        .stdout(predicate::str::contains("agent_state:"))
        .stdout(predicate::str::contains("running: count 3, avg 160s"));
}

#[test]
fn agent_wait_stats_json_reports_average_and_group_summaries() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_wait_observations(
        temp.path(),
        r#"{"recorded_at":"2026-05-20T00:00:00Z","wait_class":"non_idle","wait_reason":"heartbeat","elapsed_seconds":60,"bound_seconds":60,"unchanged_seconds":60,"target":"feature","branch":"feature","worktree":null,"task_run_id":null,"agent_kind":"codex","agent_state":"running","last_tool":null,"last_event_at":null,"session_id":null}
{"recorded_at":"2026-05-20T00:01:00Z","wait_class":"non_idle","wait_reason":"heartbeat","elapsed_seconds":120,"bound_seconds":60,"unchanged_seconds":60,"target":"feature","branch":"feature","worktree":null,"task_run_id":null,"agent_kind":"codex","agent_state":"running","last_tool":null,"last_event_at":null,"session_id":null}
{"recorded_at":"2026-05-20T00:05:00Z","wait_class":"non_idle","wait_reason":"timeout","elapsed_seconds":300,"bound_seconds":300,"unchanged_seconds":180,"target":"feature","branch":"feature","worktree":null,"task_run_id":null,"agent_kind":"claude_code","agent_state":"running","last_tool":null,"last_event_at":null,"session_id":null}
"#,
    );

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "agent",
            "wait-stats",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["count"], 3);
    assert_eq!(value["sum_seconds"], 480);
    assert_eq!(value["average_seconds"].as_f64().unwrap(), 160.0);
    assert_eq!(value["min_seconds"], 60);
    assert_eq!(value["max_seconds"], 300);
    assert_eq!(value["buckets"]["1-4m"], 2);
    assert_eq!(value["buckets"]["5-14m"], 1);
    assert_eq!(value["groups"]["wait_reason"]["heartbeat"]["count"], 2);
    assert_eq!(
        value["groups"]["wait_reason"]["heartbeat"]["average_seconds"]
            .as_f64()
            .unwrap(),
        90.0
    );
    assert_eq!(value["groups"]["bound_seconds"]["60"]["count"], 2);
    assert_eq!(value["groups"]["agent_kind"]["codex"]["sum_seconds"], 180);
    assert_eq!(value["groups"]["agent_state"]["running"]["count"], 3);
}

#[test]
fn agent_wait_stats_missing_storage_reports_empty_state() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "agent", "wait-stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Count: 0"))
        .stdout(predicate::str::contains(
            "Empty state: no non-idle wait observations recorded",
        ))
        .stdout(predicate::str::contains("Buckets: none"));
}

#[cfg(unix)]
#[test]
fn setup_installs_detected_claude_and_codex_hooks() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let fake_bin = write_fake_agent(temp.path(), "claude");
    write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claude hook installed"))
        .stdout(predicate::str::contains("Codex hook installed"))
        .stdout(predicate::str::contains("Next: run `wt init`"));

    let settings = json_file(&home.join(".claude/settings.json"));
    for &(event_name, _) in MANAGED_INBOX_HOOK_EVENTS {
        assert!(
            claude_event_commands(&settings, event_name)
                .iter()
                .any(|command| command == CLAUDE_INBOX_HOOK_COMMAND)
        );
    }
    assert!(
        claude_event_commands(&settings, "SessionEnd")
            .iter()
            .any(|command| command == CLAUDE_SUPERVISOR_SESSION_END_HOOK_COMMAND)
    );
    assert!(!temp.path().join(".claude/settings.local.json").exists());

    let hooks = json_file(&codex_home.join("hooks.json"));
    for &(event_name, _) in MANAGED_INBOX_HOOK_EVENTS {
        assert!(codex_event_commands(&hooks, event_name).contains(&codex_dispatcher_command()));
    }
}

#[cfg(unix)]
#[test]
fn setup_preserves_user_hooks_cmux_hooks_and_unrelated_trust() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let claude_home = home.join(".claude");
    let codex_home = temp.path().join("codex-home");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::write(
        claude_home.join("settings.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "UserPromptSubmit": [{
                    "hooks": [{
                        "type": "command",
                        "command": "echo user-claude-hook"
                    }]
                }]
            },
            "theme": "dark"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        codex_home.join("hooks.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "UserPromptSubmit": [{
                    "hooks": [{
                        "type": "command",
                        "command": "cmux hooks codex prompt-submit",
                        "timeout": 5
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": "cmux hooks codex stop"
                    }]
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let cmux_key = format!(
        "{}:user_prompt_submit:0:0",
        codex_home.join("hooks.json").display()
    );
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"[features]
hooks = true

[hooks.state."{cmux_key}"]
trusted_hash = "sha256:cmux"
"#
        ),
    )
    .unwrap();
    let fake_bin = write_fake_agent(temp.path(), "claude");
    write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success();

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "setup",
            "--remove",
            "--yes",
        ])
        .assert()
        .success();

    let settings = json_file(&claude_home.join("settings.json"));
    assert_eq!(settings["theme"].as_str(), Some("dark"));
    assert!(
        claude_event_commands(&settings, "UserPromptSubmit")
            .iter()
            .any(|command| command == "echo user-claude-hook")
    );
    assert!(claude_managed_inbox_commands(&settings).is_empty());
    assert!(claude_event_commands(&settings, "SessionEnd").is_empty());

    let hooks = json_file(&codex_home.join("hooks.json"));
    assert!(
        codex_event_commands(&hooks, "UserPromptSubmit")
            .iter()
            .any(|command| command == "cmux hooks codex prompt-submit")
    );
    assert_eq!(
        hooks["hooks"]["Stop"][0]["hooks"][0]["command"].as_str(),
        Some("cmux hooks codex stop")
    );
    assert!(!codex_managed_inbox_commands(&hooks).contains(&codex_dispatcher_command()));

    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(codex_home.join("config.toml")).unwrap()).unwrap();
    assert_eq!(
        config["hooks"]["state"][&cmux_key]["trusted_hash"].as_str(),
        Some("sha256:cmux")
    );
}

#[cfg(unix)]
#[test]
fn setup_and_remove_are_idempotent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let fake_bin = write_fake_agent(temp.path(), "claude");
    write_fake_agent(temp.path(), "codex");

    for _ in 0..2 {
        wt_command()
            .env("HOME", &home)
            .env("CODEX_HOME", &codex_home)
            .env("SHELL", "/bin/fish")
            .env("PATH", path_with_fake_bin(&fake_bin))
            .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
            .assert()
            .success();
    }

    let settings = json_file(&home.join(".claude/settings.json"));
    for &(event_name, _) in MANAGED_INBOX_HOOK_EVENTS {
        let commands = claude_event_commands(&settings, event_name);
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == CLAUDE_INBOX_HOOK_COMMAND)
                .count(),
            1
        );
    }
    assert_eq!(
        claude_event_commands(&settings, "SessionEnd")
            .iter()
            .filter(|command| command.as_str() == CLAUDE_SUPERVISOR_SESSION_END_HOOK_COMMAND)
            .count(),
        1
    );
    let hooks = json_file(&codex_home.join("hooks.json"));
    for &(event_name, _) in MANAGED_INBOX_HOOK_EVENTS {
        let commands = codex_event_commands(&hooks, event_name);
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == codex_dispatcher_command())
                .count(),
            1
        );
    }

    for _ in 0..2 {
        wt_command()
            .env("HOME", &home)
            .env("CODEX_HOME", &codex_home)
            .env("SHELL", "/bin/fish")
            .args([
                "-C",
                temp.path().to_str().unwrap(),
                "setup",
                "--remove",
                "--yes",
            ])
            .assert()
            .success();
    }

    assert!(!home.join(".claude/settings.json").exists());
    let hooks = json_file(&codex_home.join("hooks.json"));
    assert!(!codex_managed_inbox_commands(&hooks).contains(&codex_dispatcher_command()));
}

#[cfg(unix)]
#[test]
fn setup_repairs_claude_when_session_end_hook_is_missing() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let settings_path = home.join(".claude/settings.json");
    let fake_bin = write_fake_agent(temp.path(), "claude");

    wt_command()
        .env("HOME", &home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success();

    let mut settings = json_file(&settings_path);
    settings["hooks"]
        .as_object_mut()
        .unwrap()
        .remove("SessionEnd");
    std::fs::write(
        &settings_path,
        format!("{}\n", serde_json::to_string_pretty(&settings).unwrap()),
    )
    .unwrap();

    wt_command()
        .env("HOME", &home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claude hook installed"));

    let settings = json_file(&settings_path);
    assert!(
        claude_event_commands(&settings, "SessionEnd")
            .iter()
            .any(|command| command == CLAUDE_SUPERVISOR_SESSION_END_HOOK_COMMAND)
    );
}

#[cfg(unix)]
#[test]
fn setup_repairs_codex_when_hooks_feature_is_disabled() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let config_path = codex_home.join("config.toml");
    let fake_bin = write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success();

    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        config.replacen("hooks = true", "hooks = false", 1),
    )
    .unwrap();

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Codex hook installed"));

    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    assert_eq!(config["features"]["hooks"].as_bool(), Some(true));

    let hooks = json_file(&codex_home.join("hooks.json"));
    for &(event_name, _) in MANAGED_INBOX_HOOK_EVENTS {
        let commands = codex_event_commands(&hooks, event_name);
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == codex_dispatcher_command())
                .count(),
            1
        );
    }
}

#[cfg(unix)]
#[test]
fn setup_remove_cleans_stale_codex_trust_without_hooks_and_preserves_other_trust() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let hooks_path = codex_home.join("hooks.json");
    let config_path = codex_home.join("config.toml");
    let fake_bin = write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success();

    let stale_prompt_key = format!("{}:user_prompt_submit:0:0", hooks_path.display());
    let stale_post_tool_key = format!("{}:post_tool_use:0:0", hooks_path.display());
    let user_key = format!("{}:user_prompt_submit:5:0", hooks_path.display());
    let cmux_key = format!("{}:post_tool_use:5:0", hooks_path.display());
    let unrelated_key = format!(
        "{}:user_prompt_submit:0:0",
        temp.path().join("other-hooks.json").display()
    );
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(
        &config_path,
        format!(
            r#"{config}
[hooks.state."{user_key}"]
trusted_hash = "sha256:user"

[hooks.state."{cmux_key}"]
trusted_hash = "sha256:cmux"

[hooks.state."{unrelated_key}"]
trusted_hash = "sha256:unrelated"
"#
        ),
    )
    .unwrap();
    std::fs::remove_file(&hooks_path).unwrap();

    wt_command()
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .args(["setup", "--remove", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Codex hook uninstalled"));

    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    let state = config["hooks"]["state"].as_table().unwrap();
    assert!(!state.contains_key(&stale_prompt_key));
    assert!(!state.contains_key(&stale_post_tool_key));
    assert_eq!(
        state[&user_key]["trusted_hash"].as_str(),
        Some("sha256:user")
    );
    assert_eq!(
        state[&cmux_key]["trusted_hash"].as_str(),
        Some("sha256:cmux")
    );
    assert_eq!(
        state[&unrelated_key]["trusted_hash"].as_str(),
        Some("sha256:unrelated")
    );
}

#[cfg(unix)]
#[test]
fn setup_remove_cleans_partial_wt_managed_hooks() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let claude_home = home.join(".claude");
    std::fs::create_dir_all(&claude_home).unwrap();
    std::fs::create_dir_all(&codex_home).unwrap();

    std::fs::write(
        claude_home.join("settings.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "SessionEnd": [{
                    "hooks": [{
                        "type": "command",
                        "command": CLAUDE_SUPERVISOR_SESSION_END_HOOK_COMMAND
                    }]
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        codex_home.join("hooks.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "UserPromptSubmit": [{
                    "hooks": [{
                        "type": "command",
                        "command": codex_dispatcher_command()
                    }]
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    wt_command()
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .args(["setup", "--remove", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claude hook uninstalled"))
        .stdout(predicate::str::contains("Codex hook uninstalled"));

    assert!(!claude_home.join("settings.json").exists());
    let hooks = json_file(&codex_home.join("hooks.json"));
    assert!(codex_managed_inbox_commands(&hooks).is_empty());
}

#[cfg(unix)]
#[test]
fn setup_remove_preserves_codex_user_trust_after_index_shift() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    std::fs::create_dir_all(&codex_home).unwrap();
    let hooks_path = codex_home.join("hooks.json");

    std::fs::write(
        &hooks_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "UserPromptSubmit": [{
                    "hooks": [
                        {
                            "type": "command",
                            "command": codex_dispatcher_command()
                        },
                        {
                            "type": "command",
                            "command": "echo user-prompt"
                        }
                    ]
                }],
                "PostToolUse": [
                    {
                        "hooks": [{
                            "type": "command",
                            "command": codex_dispatcher_command()
                        }]
                    },
                    {
                        "hooks": [{
                            "type": "command",
                            "command": "echo post-tool"
                        }]
                    }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let user_prompt_old = format!("{}:user_prompt_submit:0:1", hooks_path.display());
    let user_prompt_new = format!("{}:user_prompt_submit:0:0", hooks_path.display());
    let post_tool_old = format!("{}:post_tool_use:1:0", hooks_path.display());
    let post_tool_new = format!("{}:post_tool_use:0:0", hooks_path.display());
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"[hooks.state."{}:user_prompt_submit:0:0"]
trusted_hash = "sha256:wt-prompt"

[hooks.state."{user_prompt_old}"]
trusted_hash = "sha256:user-prompt"

[hooks.state."{}:post_tool_use:0:0"]
trusted_hash = "sha256:wt-post"

[hooks.state."{post_tool_old}"]
trusted_hash = "sha256:user-post"
"#,
            hooks_path.display(),
            hooks_path.display(),
        ),
    )
    .unwrap();

    wt_command()
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .args(["setup", "--remove", "--yes"])
        .assert()
        .success();

    let hooks = json_file(&hooks_path);
    assert_eq!(
        codex_event_commands(&hooks, "UserPromptSubmit"),
        vec!["echo user-prompt".to_string()]
    );
    assert_eq!(
        codex_event_commands(&hooks, "PostToolUse"),
        vec!["echo post-tool".to_string()]
    );

    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(codex_home.join("config.toml")).unwrap()).unwrap();
    let state = config["hooks"]["state"].as_table().unwrap();
    assert_eq!(
        state[&user_prompt_new]["trusted_hash"].as_str(),
        Some("sha256:user-prompt")
    );
    assert_eq!(
        state[&post_tool_new]["trusted_hash"].as_str(),
        Some("sha256:user-post")
    );
    assert!(!state.contains_key(&user_prompt_old));
    assert!(!state.contains_key(&post_tool_old));
}

#[cfg(unix)]
#[test]
fn setup_dry_run_writes_nothing() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let zdotdir = temp.path().join("zdot");
    let fake_bin = write_fake_agent(temp.path(), "claude");
    write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dry run complete"));

    assert!(!home.join(".claude/settings.json").exists());
    assert!(!codex_home.exists());
    assert!(!zdotdir.join(".zshrc").exists());
}

#[cfg(unix)]
#[test]
fn setup_shell_lines_honor_zdotdir() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let zdotdir = temp.path().join("custom-zdot");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success();

    let zshrc = std::fs::read_to_string(zdotdir.join(".zshrc")).unwrap();
    assert!(zshrc.contains("eval \"$(wt shell-init zsh)\""));
    assert!(zshrc.contains("eval \"$(wt completion zsh)\""));
    assert!(!home.join(".zshrc").exists());
}

#[cfg(unix)]
#[test]
fn setup_skips_completion_for_homebrew_install_source() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let zdotdir = temp.path().join("zdot");
    let homebrew = temp.path().join("homebrew");
    let wt = copy_wt_binary(&homebrew);

    wt_command_at(&wt)
        .env("HOME", &home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("HOMEBREW_PREFIX", &homebrew)
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Homebrew-managed wt detected; completion provided by formula. Skipping.",
        ));

    let zshrc = std::fs::read_to_string(zdotdir.join(".zshrc")).unwrap();
    assert!(zshrc.contains("eval \"$(wt shell-init zsh)\""));
    assert!(!zshrc.contains("eval \"$(wt completion zsh)\""));
}

#[cfg(unix)]
#[test]
fn setup_remove_removes_completion_for_homebrew_install_source() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let zdotdir = temp.path().join("zdot");
    let homebrew = temp.path().join("homebrew");
    let wt = copy_wt_binary(&homebrew);
    std::fs::create_dir_all(&zdotdir).unwrap();
    std::fs::write(
        zdotdir.join(".zshrc"),
        "eval \"$(wt shell-init zsh)\"\neval \"$(wt completion zsh)\"\n",
    )
    .unwrap();

    wt_command_at(&wt)
        .env("HOME", &home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("HOMEBREW_PREFIX", &homebrew)
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "setup",
            "--remove",
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Shell integration removed"))
        .stdout(predicate::str::contains("Shell completion removed"));

    let zshrc = std::fs::read_to_string(zdotdir.join(".zshrc")).unwrap();
    assert!(!zshrc.contains("eval \"$(wt shell-init zsh)\""));
    assert!(!zshrc.contains("eval \"$(wt completion zsh)\""));
}

#[cfg(unix)]
#[test]
fn setup_installs_completion_for_explicit_non_homebrew_wt_path() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let home = temp.path().join("home");
    let zdotdir = temp.path().join("zdot");
    let cargo_home = home.join(".cargo");
    let wt = copy_wt_binary(&cargo_home);

    wt_command_at(&wt)
        .env("HOME", &home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .args(["-C", temp.path().to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Shell completion added"));

    let zshrc = std::fs::read_to_string(zdotdir.join(".zshrc")).unwrap();
    assert!(zshrc.contains("eval \"$(wt shell-init zsh)\""));
    assert!(zshrc.contains("eval \"$(wt completion zsh)\""));
}

#[cfg(unix)]
#[test]
fn setup_dry_run_succeeds_outside_git_repo() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let zdotdir = temp.path().join("zdot");

    wt_command()
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", "")
        .args(["setup", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Setup dry run complete: no files changed",
        ));

    assert!(!home.join(".claude/settings.json").exists());
    assert!(!codex_home.exists());
    assert!(!zdotdir.join(".zshrc").exists());
}

#[cfg(unix)]
#[test]
fn setup_yes_succeeds_outside_git_repo() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let zdotdir = temp.path().join("zdot");

    wt_command()
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", "")
        .args(["setup", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Shell integration added"))
        .stdout(predicate::str::contains("Shell completion added"))
        .stdout(predicate::str::contains(
            "Next: run `wt init` inside a git repo.",
        ));

    let zshrc = std::fs::read_to_string(zdotdir.join(".zshrc")).unwrap();
    assert!(zshrc.contains("eval \"$(wt shell-init zsh)\""));
    assert!(zshrc.contains("eval \"$(wt completion zsh)\""));
    assert!(!home.join(".claude/settings.json").exists());
    assert!(!codex_home.exists());
}

#[cfg(unix)]
#[test]
fn setup_remove_yes_succeeds_outside_git_repo() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let codex_home = temp.path().join("codex-home");
    let zdotdir = temp.path().join("zdot");

    wt_command()
        .current_dir(temp.path())
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", "")
        .args(["setup", "--remove", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claude hooks: already absent"))
        .stdout(predicate::str::contains("Codex hooks: already absent"))
        .stdout(predicate::str::contains(
            "Shell integration: already absent",
        ));

    assert!(!home.join(".claude/settings.json").exists());
    assert!(!codex_home.exists());
    assert!(!zdotdir.join(".zshrc").exists());
}

#[test]
fn removed_setup_surfaces_are_unrecognized() {
    assert_removed_surface_unrecognized(&["install"]);
    assert_removed_surface_unrecognized(&["uninstall"]);
    assert_removed_surface_unrecognized(&["hooks", "setup"]);
    assert_removed_surface_unrecognized(&["hooks", "uninstall"]);
    assert_removed_surface_unrecognized(&["hooks", "codex"]);
    assert_removed_surface_unrecognized(&["hooks", "claude"]);
    assert_removed_surface_unrecognized(&["agent", "hook", "install"]);
    assert_removed_surface_unrecognized(&["agent", "hook", "uninstall"]);
}

fn assert_removed_surface_unrecognized(args: &[&str]) {
    wt_command()
        .args(args)
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[cfg(unix)]
#[test]
fn doctor_missing_claude_hooks_names_wt_setup() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_repo_agent_config(temp.path(), "claude");
    write_wt_core_dirs(temp.path());
    let home = temp.path().join("home");
    let zdotdir = temp.path().join("zdot");
    write_zsh_shell_integration(&zdotdir);
    let fake_bin = write_fake_agent(temp.path(), "claude");

    wt_command()
        .env("HOME", &home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Claude wt inbox hook: Run wt setup",
        ));
}

#[cfg(unix)]
#[test]
fn doctor_missing_codex_hooks_names_wt_setup() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_repo_agent_config(temp.path(), "codex");
    write_wt_core_dirs(temp.path());
    let home = temp.path().join("home");
    let zdotdir = temp.path().join("zdot");
    let codex_home = temp.path().join("codex-home");
    write_zsh_shell_integration(&zdotdir);
    let fake_bin = write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Codex wt inbox hook: missing hooks.json. Run wt setup",
        ));
}

#[cfg(unix)]
#[test]
fn doctor_missing_shell_integration_names_wt_setup() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_repo_agent_config(temp.path(), "codex");
    write_wt_core_dirs(temp.path());
    let home = temp.path().join("home");
    let zdotdir = temp.path().join("zdot");
    let codex_home = temp.path().join("codex-home");
    let fake_bin = write_fake_agent(temp.path(), "codex");

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", temp.path().to_str().unwrap(), "doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Shell integration: missing"))
        .stderr(predicate::str::contains("Run wt setup"));
}

#[cfg(unix)]
#[test]
fn doctor_missing_repo_config_names_wt_init() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_wt_core_dirs(temp.path());

    wt_command()
        .env("HOME", temp.path().join("home"))
        .env("SHELL", "/bin/fish")
        .args(["-C", temp.path().to_str().unwrap(), "doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Repo config: missing"))
        .stderr(predicate::str::contains("Run wt init"));
}

#[cfg(unix)]
#[test]
fn doctor_missing_core_dirs_names_wt_init() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    std::fs::write(temp.path().join(".wt.toml"), "").unwrap();

    wt_command()
        .env("HOME", temp.path().join("home"))
        .env("SHELL", "/bin/fish")
        .args(["-C", temp.path().to_str().unwrap(), "doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Core dirs: missing"))
        .stderr(predicate::str::contains("Run wt init"));
}

#[cfg(unix)]
fn write_repo_agent_config(root: &Path, cli: &str) {
    std::fs::write(root.join(".wt.toml"), format!("[agent]\ncli = \"{cli}\"\n")).unwrap();
}

#[cfg(unix)]
fn write_wt_core_dirs(root: &Path) {
    for path in [
        root.join(".git/wt/execution/tasks"),
        root.join(".git/wt/messages"),
        root.join(".git/wt/execution/task-runs"),
        root.join(".git/wt/agent.state"),
        root.join(".git/wt/worktrees"),
    ] {
        std::fs::create_dir_all(path).unwrap();
    }
}

#[cfg(unix)]
fn write_zsh_shell_integration(zdotdir: &Path) {
    std::fs::create_dir_all(zdotdir).unwrap();
    std::fs::write(zdotdir.join(".zshrc"), "eval \"$(wt shell-init zsh)\"\n").unwrap();
}

#[cfg(unix)]
#[test]
fn codex_wrapper_sets_default_agent_id_from_current_worktree_branch() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let status = git_command()
        .args(["checkout", "-b", "alice/feat-add-schema"])
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    let fake_bin = write_fake_agent(temp.path(), "codex");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "codex"])
        .env("PATH", path_with_fake_bin(&fake_bin))
        .env("WT_COORDINATOR_AGENT_ID", "agents/stale")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "WT_AGENT_ID=agents/feat-add-schema\n",
        ))
        .stdout(predicate::str::contains("WT_COORDINATOR_AGENT_ID=\n"));
}

#[cfg(unix)]
#[test]
fn codex_wrapper_role_uses_distinct_same_worktree_agent_id() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let status = git_command()
        .args(["checkout", "-b", "alice/feat-add-schema"])
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    let fake_bin = write_fake_agent(temp.path(), "codex");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "codex", "@planner"])
        .env("PATH", path_with_fake_bin(&fake_bin))
        .env("WT_COORDINATOR_AGENT_ID", "agents/stale")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "WT_AGENT_ID=agents/feat-add-schema-planner\n",
        ))
        .stdout(predicate::str::contains("WT_COORDINATOR_AGENT_ID=\n"))
        .stdout(predicate::str::contains("WT_AGENT_ID=agents/feat-add-schema\n").not());
}

#[cfg(unix)]
#[test]
fn claude_wrapper_role_uses_distinct_same_worktree_agent_id() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let status = git_command()
        .args(["checkout", "-b", "alice/feat-add-schema"])
        .current_dir(temp.path())
        .status()
        .unwrap();
    assert!(status.success());
    let fake_bin = write_fake_agent(temp.path(), "claude");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "claude", "@reviewer"])
        .env("PATH", path_with_fake_bin(&fake_bin))
        .env("WT_COORDINATOR_AGENT_ID", "agents/stale")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "WT_AGENT_ID=agents/feat-add-schema-reviewer\n",
        ))
        .stdout(predicate::str::contains("WT_COORDINATOR_AGENT_ID=\n"));
}

#[cfg(unix)]
#[test]
fn as_wrapper_uses_explicit_agent_id_for_arbitrary_command() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let fake_bin = write_fake_agent(temp.path(), "probe-agent");

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "as",
            "agents/manual-reviewer",
            "--",
            "probe-agent",
            "hello",
            "there",
        ])
        .env("PATH", path_with_fake_bin(&fake_bin))
        .env("WT_COORDINATOR_AGENT_ID", "agents/stale")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "WT_AGENT_ID=agents/manual-reviewer\n",
        ))
        .stdout(predicate::str::contains("WT_COORDINATOR_AGENT_ID=\n"))
        .stdout(predicate::str::contains("ARGS=hello there\n"));
}

#[test]
fn agent_runtime_wrapper_help_explains_role_separation() {
    wt_command()
        .args(["codex", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WT_AGENT_ID"))
        .stdout(predicate::str::contains("wt codex @planner"))
        .stdout(predicate::str::contains(
            "multiple agents do not consume each other's messages",
        ));

    wt_command()
        .args(["claude", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WT_AGENT_ID"))
        .stdout(predicate::str::contains("wt claude @coordinator"))
        .stdout(predicate::str::contains(
            "multiple agents do not consume each other's messages",
        ));

    wt_command()
        .args(["as", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("explicit WT_AGENT_ID"))
        .stdout(predicate::str::contains("wt as <AGENT> -- <COMMAND>"));
}

#[cfg(unix)]
#[test]
fn cross_agent_hook_roundtrip_uses_file_inbox_without_cmux() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_init(&repo);
    git_commit(&repo);

    let claude_wt = temp.path().join("repo-claude-smoke");
    let codex_wt = temp.path().join("repo-codex-smoke");
    let status = git_command()
        .args([
            "worktree",
            "add",
            "-b",
            "claude-smoke",
            claude_wt.to_str().unwrap(),
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(status.success());
    let status = git_command()
        .args([
            "worktree",
            "add",
            "-b",
            "codex-smoke",
            codex_wt.to_str().unwrap(),
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(status.success());

    let codex_home = temp.path().join("codex-home");
    let home = temp.path().join("home");
    let fake_bin = write_fake_agent(temp.path(), "claude");
    write_fake_agent(temp.path(), "codex");
    let wt_bin = assert_cmd::cargo::cargo_bin("wt");
    let wt_bin_dir = wt_bin.parent().unwrap().to_path_buf();
    let path = path_with_bins(&[wt_bin_dir, fake_bin]);

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .env("PATH", &path)
        .args(["-C", claude_wt.to_str().unwrap(), "setup", "--yes"])
        .assert()
        .success();

    let claude_settings = json_file(&home.join(".claude/settings.json"));
    let claude_hook = claude_managed_inbox_commands(&claude_settings)
        .into_iter()
        .find(|command| command == CLAUDE_INBOX_HOOK_COMMAND)
        .unwrap();
    let codex_hooks = json_file(&codex_home.join("hooks.json"));
    let codex_hook = codex_managed_inbox_commands(&codex_hooks)
        .into_iter()
        .find(|command| command == &codex_dispatcher_command())
        .unwrap();

    wt_command()
        .args([
            "-C",
            claude_wt.to_str().unwrap(),
            "as",
            "agents/claude-smoke",
            "--",
            wt_bin.to_str().unwrap(),
            "-C",
            claude_wt.to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/codex-smoke",
            "CLAUDE_SENT",
        ])
        .assert()
        .success();

    let codex_delivery = StdCommand::new("sh")
        .arg("-c")
        .arg(&codex_hook)
        .current_dir(&codex_wt)
        .env("CODEX_HOME", &codex_home)
        .env("PATH", &path)
        .env("WT_AGENT_ID", "agents/codex-smoke")
        .output()
        .unwrap();
    assert!(
        codex_delivery.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&codex_delivery.stdout),
        String::from_utf8_lossy(&codex_delivery.stderr)
    );
    let codex_json: serde_json::Value = serde_json::from_slice(&codex_delivery.stdout).unwrap();
    let codex_context = codex_json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(codex_context.contains("WT INBOX for agents/codex-smoke"));
    assert!(codex_context.contains("from: agents/claude-smoke"));
    assert!(codex_context.contains("CLAUDE_SENT"));

    wt_command()
        .args([
            "-C",
            codex_wt.to_str().unwrap(),
            "as",
            "agents/codex-smoke",
            "--",
            wt_bin.to_str().unwrap(),
            "-C",
            codex_wt.to_str().unwrap(),
            "msg",
            "send",
            "--to",
            "agents/claude-smoke",
            "CODEX_SENT",
            "REALWT_PONG_SEEN",
        ])
        .assert()
        .success();

    let claude_delivery = StdCommand::new("sh")
        .arg("-c")
        .arg(&claude_hook)
        .current_dir(&claude_wt)
        .env("PATH", &path)
        .env("HOME", &home)
        .env("WT_AGENT_ID", "agents/claude-smoke")
        .output()
        .unwrap();
    assert!(
        claude_delivery.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&claude_delivery.stdout),
        String::from_utf8_lossy(&claude_delivery.stderr)
    );
    let claude_json: serde_json::Value = serde_json::from_slice(&claude_delivery.stdout).unwrap();
    let claude_context = claude_json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(claude_context.contains("WT INBOX for agents/claude-smoke"));
    assert!(claude_context.contains("from: agents/codex-smoke"));
    assert!(claude_context.contains("CODEX_SENT REALWT_PONG_SEEN"));

    wt_command()
        .env("HOME", &home)
        .env("CODEX_HOME", &codex_home)
        .env("SHELL", "/bin/fish")
        .args([
            "-C",
            claude_wt.to_str().unwrap(),
            "setup",
            "--remove",
            "--yes",
        ])
        .assert()
        .success();
    assert!(!home.join(".claude/settings.json").exists());
    let codex_hooks = json_file(&codex_home.join("hooks.json"));
    assert!(!codex_managed_inbox_commands(&codex_hooks).contains(&codex_dispatcher_command()));
}

#[test]
fn ui_help_explains_read_only_local_server_contract() {
    wt_command()
        .args(["ui", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "read-only personal wt state web UI",
        ))
        .stdout(predicate::str::contains("127.0.0.1"))
        .stdout(predicate::str::contains("--port <PORT>"))
        .stdout(predicate::str::contains("0 selects an available port"))
        .stdout(predicate::str::contains("GET /api/snapshot"))
        .stdout(predicate::str::contains("embedded no-build assets"));
}

#[test]
fn inspect_help_explains_optional_target_selection() {
    wt_command()
        .args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("read-only work dossier"))
        .stdout(predicate::str::contains("[TARGET]"))
        .stdout(predicate::str::contains(
            "Branch, worktree path/name, or TaskRun id",
        ))
        .stdout(predicate::str::contains(
            "Omit TARGET in an interactive terminal",
        ))
        .stdout(predicate::str::contains("--pr"))
        .stdout(predicate::str::contains("pull request review evidence"));
}

#[test]
fn done_help_explains_cleanup_target_contract() {
    wt_command()
        .args(["done", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("worktree path/name"))
        .stdout(predicate::str::contains("issue-like branch-name shorthand"))
        .stdout(predicate::str::contains("direct TaskRun id"))
        .stdout(predicate::str::contains("wt workflow complete"));
}

#[test]
fn inspect_without_target_noninteractive_requires_explicit_target() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "inspect"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("wt inspect requires TARGET"))
        .stderr(predicate::str::contains(
            "branch, worktree path/name, or TaskRun id",
        ));
}

#[test]
fn inspect_explicit_branch_prints_dossier_without_cmux_contact() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let branch = current_branch(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "inspect", &branch])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("Inspect: {branch}")))
        .stdout(predicate::str::contains("Work"))
        .stdout(predicate::str::contains("Git"))
        .stdout(predicate::str::contains("Agent"))
        .stdout(predicate::str::contains("Cmux"))
        .stdout(predicate::str::contains("Expected report"))
        .stdout(predicate::str::contains("PR=<pr>"))
        .stderr(predicate::str::contains("Cmux:"));
}

#[test]
fn inspect_prints_task_run_route_report_and_review_state() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let branch = current_branch(temp.path());
    let tasks_dir = temp.path().join(".git/wt/execution/tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("inspect.toml"),
        format!("title = \"Inspect\"\nbranch = \"{branch}\"\n"),
    )
    .unwrap();
    write_task_run_file_with_routes(
        temp.path(),
        "run-inspect",
        "inspect",
        &branch,
        "running",
        "",
        (Some("agents/run-1-inspect"), Some("agents/coord-a")),
    );
    let task_run_path = temp
        .path()
        .join(".git/wt/execution/task-runs/run-inspect.toml");
    let mut task_run = std::fs::read_to_string(&task_run_path).unwrap();
    task_run.push_str(
        "coordinator_label = \"Coordinator\"\nlast_report_message_id = \"msg_report\"\nlast_reported_at = \"2026-05-18T00:01:00.000000000Z\"\nlast_review_status = \"accepted\"\nlast_review_message_id = \"msg_review\"\nlast_reviewed_at = \"2026-05-18T00:02:00.000000000Z\"\n",
    );
    std::fs::write(&task_run_path, task_run).unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "inspect",
            "run-inspect",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "TaskRun route: task_agent=agents/run-1-inspect, coordinator=agents/coord-a, coordinator_label=Coordinator",
        ))
        .stdout(predicate::str::contains(
            "TaskRun report: message=msg_report, reported_at=2026-05-18T00:01:00.000000000Z",
        ))
        .stdout(predicate::str::contains(
            "TaskRun review: status=accepted, message=msg_review, reviewed_at=2026-05-18T00:02:00.000000000Z",
        ));
}

#[test]
fn inspect_pr_renders_pull_request_review_section() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let branch = current_branch(temp.path());
    let fake_bin = write_fake_gh(temp.path());

    wt_command()
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "inspect",
            &branch,
            "--pr",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull Request Review"))
        .stdout(predicate::str::contains("PR: #42 Add PR evidence"))
        .stdout(predicate::str::contains("Verdict: passed"));
}

#[test]
fn inspect_dot_pr_works_from_task_worktree() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let worktree = temp.path().join("repo-feature");
    std::fs::create_dir_all(&repo).unwrap();
    git_init(&repo);
    git_commit(&repo);
    let status = git_command()
        .args([
            "worktree",
            "add",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
            "HEAD",
        ])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(status.success());
    let fake_bin = write_fake_gh(temp.path());

    wt_command()
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args(["-C", worktree.to_str().unwrap(), "inspect", ".", "--pr"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Inspect: ."))
        .stdout(predicate::str::contains("Branch: feature"))
        .stdout(predicate::str::contains("Pull Request Review"))
        .stdout(predicate::str::contains("Verdict: passed"));
}

#[test]
fn inspect_without_pr_does_not_fetch_pull_request_review() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let branch = current_branch(temp.path());
    let fake_bin = write_fake_gh(temp.path());
    let marker = temp.path().join("gh-called");

    wt_command()
        .env("PATH", path_with_fake_bin(&fake_bin))
        .env("WT_FAKE_GH_MARK", &marker)
        .args(["-C", temp.path().to_str().unwrap(), "inspect", &branch])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull Request Review").not());

    assert!(!marker.exists(), "plain inspect should not call gh");
}

#[test]
fn inspect_pr_json_nests_pull_request_review_without_top_level_status() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    git_commit(temp.path());
    let branch = current_branch(temp.path());
    let fake_bin = write_fake_gh(temp.path());

    let output = wt_command()
        .env("PATH", path_with_fake_bin(&fake_bin))
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "inspect",
            &branch,
            "--pr",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("==>").not())
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value.get("status").is_none());
    assert_eq!(value["pull_request_review"]["pr"]["number"], 42);
    assert_eq!(value["pull_request_review"]["verdict"], "passed");
}

#[test]
fn init_yes_uses_project_recommendation_without_agent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "init", "--yes"])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config/local.toml")).unwrap();
    assert!(content.contains("[workspace]"));
    assert!(!content.contains("[profile.agent]"));
    assert!(!content.contains("[issues]"));
}

#[test]
fn init_yes_bootstraps_only_core_state_dirs() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "init", "--yes"])
        .assert()
        .success();

    assert_eq!(
        wt_state_dir_names(temp.path()),
        vec![
            "agent.state".to_string(),
            "config".to_string(),
            "execution".to_string(),
            "messages".to_string(),
            "planning".to_string(),
            "worktrees".to_string(),
        ]
    );
    for legacy_flat in [
        "workflows",
        "archive",
        "ideas",
        "retrospectives",
        "profiles",
    ] {
        assert!(!temp.path().join(".git/wt").join(legacy_flat).exists());
    }
    for canonical in [
        "config/local.toml",
        "config/profiles",
        "planning/ideas",
        "planning/specs",
        "planning/retrospectives",
        "execution/tasks",
        "execution/workflows",
        "execution/task-runs",
        "execution/archive",
    ] {
        assert!(temp.path().join(".git/wt").join(canonical).exists());
    }
    assert!(!temp.path().join(".claude/settings.local.json").exists());
}

#[test]
fn init_rerun_preserves_user_created_workflow_file() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "init", "--yes"])
        .assert()
        .success();

    let workflow_path = temp
        .path()
        .join(".git/wt/execution/workflows/2026-05-21-999.toml");
    std::fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
    std::fs::write(&workflow_path, "mode = \"batch\"\n").unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--yes",
            "--force",
        ])
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(&workflow_path).unwrap(),
        "mode = \"batch\"\n"
    );
}

#[test]
fn init_explicit_agent_yes_writes_agent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--agent",
            "codex",
            "--yes",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config/local.toml")).unwrap();
    assert!(content.contains("[profile.agent]"));
    assert!(content.contains("cli = \"codex\""));
}

#[test]
fn init_rejects_legacy_minimal_shortcut() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--minimal",
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn init_dry_run_previews_plan_without_writing_config() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    std::fs::write(
        temp.path().join("package.json"),
        r#"{"scripts":{"test":"vitest","lint":"eslint ."}}"#,
    )
    .unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--yes",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("init 계획"))
        .stdout(predicate::str::contains("==>").not())
        .stdout(predicate::str::contains("저장할 파일:"))
        .stdout(predicate::str::contains("저장 범위: 개인 설정"))
        .stdout(predicate::str::contains("작업: 새 설정 생성"))
        .stdout(predicate::str::contains(
            "저장될 설정: workflow, worktree, setup, test, workspace",
        ))
        .stdout(predicate::str::contains("감지된 신호:").not())
        .stdout(predicate::str::contains("[ok] 감지됨:").not())
        .stdout(predicate::str::contains("[workflow]"))
        .stdout(predicate::str::contains("pull_request = \"none\""))
        .stdout(predicate::str::contains("landing = \"manual\""))
        .stdout(predicate::str::contains("[worktree]"))
        .stdout(predicate::str::contains("inject_local_context = \"\"\""))
        .stdout(predicate::str::contains(
            "colors = { task = \"blue\", issue = \"blue\", branch = \"green\", pr = \"magenta\" }",
        ))
        .stdout(predicate::str::contains("run = \"npm install\""))
        .stdout(predicate::str::contains("run = \"npm test\""))
        .stdout(predicate::str::contains("[setup]"))
        .stdout(predicate::str::contains("[test]"));

    assert!(!temp.path().join(".wt.toml").exists());
    assert!(!temp.path().join(".git/wt/config/local.toml").exists());
    assert!(!temp.path().join(".git/wt").exists());
    assert!(!temp.path().join(".claude/settings.local.json").exists());
}

#[test]
fn init_no_color_uses_plain_plan_output() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--no-color",
            "init",
            "--yes",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("init 계획\n"))
        .stdout(predicate::str::contains("==>").not())
        .stdout(predicate::str::contains(
            "저장될 설정: workflow, worktree, workspace",
        ));
}

#[test]
fn init_quiet_suppresses_status_output_but_still_writes_config() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--quiet",
            "init",
            "--yes",
        ])
        .assert()
        .success()
        .stdout("");

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config/local.toml")).unwrap();
    assert!(content.contains("[workspace]"));
}

#[test]
fn init_json_flag_rejects_init_without_status_decoration() {
    wt_command()
        .args(["--json", "init", "--yes", "--dry-run"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("JSON output is supported"))
        .stderr(predicate::str::contains("==>").not());
}

#[test]
fn json_output_uses_machine_readable_surface_without_status_decoration() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    let output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("==>").not())
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value["checks"].as_array().is_some());
}

#[test]
fn agent_status_supports_json_global_flag() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "agent",
            "status",
            "missing",
        ])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("Work target not found: missing"))
        .stderr(predicate::str::contains("JSON output is supported").not());
}

#[test]
fn agent_status_without_target_noninteractive_requires_guidance() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "agent", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("wt agent status requires TARGET"))
        .stderr(predicate::str::contains("wt agent status <target>"))
        .stderr(predicate::str::contains("wt agent watch <target>"))
        .stderr(predicate::str::contains("wt inspect [<target>]"));
}

#[test]
fn init_existing_config_requires_force_for_yes_and_force_overwrites() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(temp.path(), "[workspace]\ntabs = []\n");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "init", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("설정 파일이 이미 있습니다:"))
        .stderr(predicate::str::contains("--force로 덮어쓸 수 있습니다"));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--agent",
            "codex",
            "--yes",
            "--force",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("WARNING:"))
        .stdout(predicate::str::contains("설정 업데이트됨:"));

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config/local.toml")).unwrap();
    assert!(content.contains("[profile.agent]"));
}

#[test]
fn init_rejects_legacy_preset_flag() {
    wt_command()
        .args(["init", "--preset", "minimal"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn completion_generates_script() {
    let output = wt_command()
        .args(["completion", "bash"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("_wt"));
    assert_canonical_completion_surface("bash", &output);
    assert_bash_completion_syntax(&output);
}

#[test]
fn all_completion_shells_hide_removed_start_commands() {
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let output = wt_command()
            .args(["completion", shell])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let output = String::from_utf8(output).unwrap();
        assert_canonical_completion_surface(shell, &output);
    }
}

fn assert_canonical_completion_surface(shell: &str, output: &str) {
    for required in ["run", "issue", "pr", "branch", "task", "workflow"] {
        assert!(
            output.contains(required),
            "{shell} completion should contain {required}"
        );
    }

    for stale in [
        "__wt_removed_",
        "wt issue",
        "wt pr ",
        "wt new",
        "wt task run",
        "wt workflow run",
        "wt,issue)",
        "wt,pr)",
        "wt,new)",
        "wt__subcmd__issue",
        "wt__subcmd__pr)",
        "wt__subcmd__pr,",
        "wt__subcmd__new",
        "wt__subcmd__task,run",
        "wt__subcmd__workflow,run",
        "_wt__subcmd__issue",
        "_wt__subcmd__pr_commands",
        "_wt__subcmd__new",
        "_wt__subcmd__task__subcmd__run",
        "_wt__subcmd__workflow__subcmd__run",
        "__fish_wt_using_subcommand issue\"",
        "__fish_wt_using_subcommand pr\"",
        "__fish_wt_using_subcommand new\"",
        "__fish_seen_subcommand_from new",
        "__fish_seen_subcommand_from task run",
        "__fish_seen_subcommand_from workflow run",
        "cand ''",
        "[CompletionResult]::new('',",
        "'wt;issue'",
        "'wt;pr'",
        "'wt;new'",
        "'wt;task;run'",
        "'wt;workflow;run'",
        "&'wt;issue'",
        "&'wt;pr'",
        "&'wt;new'",
        "&'wt;task;run'",
        "&'wt;workflow;run'",
    ] {
        assert!(
            !output.contains(stale),
            "{shell} completion should not contain stale surface {stale:?}"
        );
    }
}

fn assert_bash_completion_syntax(output: &str) {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("wt-completion.bash");
    std::fs::write(&path, output).unwrap();
    let Ok(status) = StdCommand::new("bash").arg("-n").arg(&path).status() else {
        return;
    };
    assert!(status.success());
}

#[test]
fn completions_alias_is_rejected() {
    wt_command()
        .args(["completions", "bash"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn doctor_supports_json_and_directory_override() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    let output = wt_command()
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
    assert!(
        value["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| { check["name"] == "cmux_cli" })
    );
}

#[test]
fn doctor_uses_config_override() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    std::fs::write(
        temp.path().join("override.toml"),
        "[issues]\nprovider = \"github\"\n",
    )
    .unwrap();

    let output = wt_command()
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
fn config_renders_effective_profile_layers_and_conventions() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    std::fs::write(
        temp.path().join(".wt.toml"),
        r#"
[issues]
provider = "github"

[worktree]
copy = [".env.example"]
link = ["storage"]

[setup.env]
APP_ENV = "local"
LOG_LEVEL = "info"

[workflow]
pull_request = "draft"
landing = "auto"

[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
"#,
    )
    .unwrap();

    std::fs::create_dir_all(temp.path().join(".git/wt/config/profiles/codex/prompts")).unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join(".git/wt/config/profiles/codex/scaffold/.codex/skills"),
    )
    .unwrap();
    write_personal_config(
        temp.path(),
        r#"
[worktree]
copy = [".env"]

[setup.env]
LOG_LEVEL = "debug"
PRIVATE_TOKEN = "secret"

[profile]
name = "codex"
"#,
    );
    std::fs::write(
        temp.path()
            .join(".git/wt/config/profiles/codex/profile.toml"),
        r#"
[agent]
cli = "codex"
args = ["--yolo"]

[agent.prompt]
common = ["profile common\n"]
issue = ["from profile.toml\n"]
branch = ["branch prompt\n"]

[agent.prompt.append]
common = ["profile common append\n"]
branch = ["branch append\n"]

[workspace]
tabs = ["pnpm dev"]

[setup.env]
CODEX_MODE = "1"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/config/profiles/codex/prompts/common.md"),
        "from common prompt file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/config/profiles/codex/prompts/common.append.md"),
        "from common append file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/config/profiles/codex/prompts/issue.md"),
        "from prompt file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/config/profiles/codex/prompts/issue.append.md"),
        "from prompt append file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/config/profiles/codex/scaffold/AGENTS.override.md"),
        "codex override\n",
    )
    .unwrap();

    let explicit = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "config",
            "--profile",
            "codex",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let implicit = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(explicit, implicit);
    let rendered = String::from_utf8(explicit).unwrap();
    assert!(!rendered.contains("[agent.prompt.append]"));
    assert!(!rendered.contains("common ="));
    let config: wt::config::Config = toml::from_str(&rendered).unwrap();

    assert!(config.profile.is_none());
    assert_eq!(config.worktree.copy, vec![".env.example", ".env"]);
    assert_eq!(config.worktree.link, vec!["storage"]);
    assert_eq!(config.setup.env.get("APP_ENV").unwrap(), "local");
    assert_eq!(config.setup.env.get("LOG_LEVEL").unwrap(), "debug");
    assert_eq!(config.setup.env.get("PRIVATE_TOKEN").unwrap(), "secret");
    assert_eq!(config.setup.env.get("CODEX_MODE").unwrap(), "1");
    let policy = config.workflow_default_policy();
    assert_eq!(
        policy.pull_request,
        wt::config::WorkflowDefaultPullRequestMode::Draft
    );
    assert_eq!(
        policy.landing,
        wt::config::WorkflowDefaultLandingPolicy::Auto
    );

    let agent = config.agent.unwrap();
    assert_eq!(agent.cli, wt::config::AgentCli::Codex);
    assert_eq!(agent.args, vec!["--yolo"]);
    assert_eq!(
        agent.prompt.get("issue").unwrap(),
        &vec![
            "from common prompt file\n\nfrom common append file\n".to_string(),
            "from prompt file\n\nfrom prompt append file\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("branch").unwrap(),
        &vec![
            "from common prompt file\n\nfrom common append file\n".to_string(),
            "branch prompt\n\nbranch append\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["from common prompt file\n\nfrom common append file\n".to_string()]
    );

    let workspace = config.workspace.unwrap();
    assert_eq!(workspace.tabs, vec!["pnpm dev"]);
    assert_eq!(workspace.colors.get("task").unwrap(), "blue");
    assert_eq!(workspace.colors.get("issue").unwrap(), "blue");
    assert_eq!(workspace.colors.get("branch").unwrap(), "green");
    assert_eq!(workspace.colors.get("pr").unwrap(), "magenta");

    let copy_as = config.worktree.copy_as;
    assert!(copy_as.iter().any(|entry| {
        Path::new(&entry.from).ends_with(".git/wt/config/profiles/codex/scaffold")
            && entry.to == "."
    }));
}

#[test]
fn config_renders_builtin_workflow_defaults() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[workflow]"))
        .stdout(predicate::str::contains("pull_request = \"none\""))
        .stdout(predicate::str::contains("landing = \"manual\""));
}

#[test]
fn config_renders_builtin_workspace_color_defaults() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(temp.path(), "[workspace]\ntabs = [\"lazygit\"]\n");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[workspace]"))
        .stdout(predicate::str::contains(
            "colors = { task = \"blue\", issue = \"blue\", branch = \"green\", pr = \"magenta\" }",
        ));
}

#[test]
fn config_rejects_legacy_new_workspace_color_key() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(temp.path(), "[workspace]\ncolors = { new = \"green\" }\n");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("[workspace].colors.new"))
        .stderr(predicate::str::contains("[workspace].colors.branch"));
}

#[test]
fn config_renders_workspace_chrome_devtools_browser_policy_defaults() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(
        temp.path(),
        "[workspace.browser]\nmode = \"chrome_devtools\"\nurl = \"{{site_url}}/dashboard\"\n",
    );

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[workspace.browser]"))
        .stdout(predicate::str::contains("mode = \"chrome_devtools\""))
        .stdout(predicate::str::contains("url = \"{{site_url}}/dashboard\""))
        .stdout(predicate::str::contains("[workspace.chrome_devtools]"))
        .stdout(predicate::str::contains(
            "user_data_dir = \"{{worktree_parent}}/.chrome-devtools/{{worktree_name}}\"",
        ))
        .stdout(predicate::str::contains("enabled =").not())
        .stdout(predicate::str::contains("port =").not());
}

#[test]
fn config_preserves_empty_workspace_color_overrides() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(
        temp.path(),
        "[workspace]\ncolors = { task = \"\", issue = \"\", branch = \"\", pr = \"\" }\n",
    );

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "colors = { task = \"\", issue = \"\", branch = \"\", pr = \"\" }",
        ));
}

#[test]
fn config_renders_active_site_runtime_defaults() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(
        temp.path(),
        "[site]\nprovider = \"traefik\"\nsecure = false\n",
    );

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[site]"))
        .stdout(predicate::str::contains(
            "name = \"{{repo}}-{{branch_slug}}\"",
        ))
        .stdout(predicate::str::contains("root = \".\""))
        .stdout(predicate::str::contains("open_browser =").not())
        .stdout(predicate::str::contains(
            "url = \"http://{{site_name}}.test\"",
        ))
        .stdout(predicate::str::contains(
            "target = \"http://127.0.0.1:{{vite_port}}\"",
        ));
}

#[test]
fn config_omits_inactive_site_section() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(temp.path(), "[site]\nprovider = \"none\"\n");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[site]").not());
}

#[test]
fn config_renders_editor_placement_default_when_editor_is_active() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_personal_config(temp.path(), "[editor]\ncommand = \"code {{path}}\"\n");

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[editor]"))
        .stdout(predicate::str::contains("command = \"code {{path}}\""))
        .stdout(predicate::str::contains("placement = \"cmux_surface\""));
}

#[test]
fn list_supports_json_and_directory_override() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    let output = wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "--json", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value.as_array().is_some());
}

#[cfg(unix)]
#[test]
fn supervisor_start_records_threshold_and_poll_interval() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "agent",
            "supervisor",
            "start",
            "agents/codex",
            "--stale-threshold",
            "5m",
            "--poll-interval",
            "30s",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Supervisor started"));

    let content =
        std::fs::read_to_string(temp.path().join(".git/wt/supervisors/agents%2Fcodex.toml"))
            .unwrap();
    assert!(content.contains("agent_id = \"agents/codex\""));
    assert!(content.contains("stale_threshold_secs = 300"));
    assert!(content.contains("poll_interval_secs = 30"));
}

#[cfg(unix)]
#[test]
fn supervisor_start_refuses_live_duplicate_and_replace_succeeds() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let mut child = spawn_sleeping_child();
    write_supervisor_registration(temp.path(), "agents/codex", child.id(), "user", 900, 60);

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "agent",
            "supervisor",
            "start",
            "agents/codex",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Supervisor already running"));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "agent",
            "supervisor",
            "start",
            "agents/codex",
            "--replace",
        ])
        .assert()
        .success();

    let _ = child.wait();
    assert!(
        !std::fs::read_to_string(temp.path().join(".git/wt/supervisors/agents%2Fcodex.toml"))
            .unwrap()
            .contains(&format!("pid = {}", child.id()))
    );
}

#[cfg(unix)]
#[test]
fn supervisor_status_json_includes_timing_fields() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let mut child = spawn_sleeping_child();
    write_supervisor_registration(temp.path(), "agents/codex", child.id(), "user", 300, 30);

    let output = wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "--json",
            "agent",
            "supervisor",
            "status",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value[0]["agent_id"], "agents/codex");
    assert_eq!(value[0]["state"], "running");
    assert_eq!(value[0]["stale_threshold_secs"], 300);
    assert_eq!(value[0]["poll_interval_secs"], 30);

    child.kill().unwrap();
    let _ = child.wait();
}

#[cfg(unix)]
#[test]
fn supervisor_logs_reads_preserved_log_after_registration_cleanup() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let mut child = spawn_sleeping_child();
    let registration_path =
        write_supervisor_registration(temp.path(), "agents/codex", child.id(), "user", 300, 30);
    let log_path = registration_path.with_extension("log");
    std::fs::write(&log_path, "post-mortem log\n").unwrap();
    std::fs::remove_file(&registration_path).unwrap();

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "agent",
            "supervisor",
            "logs",
            "agents/codex",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("post-mortem log"));

    child.kill().unwrap();
    let _ = child.wait();
}

#[cfg(unix)]
#[test]
fn supervisor_stop_owned_by_filters_started_by() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let mut owned = spawn_sleeping_child();
    let mut cleanup_disabled = spawn_sleeping_child();
    let mut other = spawn_sleeping_child();
    write_supervisor_registration(
        temp.path(),
        "agents/owned",
        owned.id(),
        "agents/foo",
        900,
        60,
    );
    write_supervisor_registration_with_cleanup(
        temp.path(),
        "agents/cleanup-disabled",
        cleanup_disabled.id(),
        "agents/foo",
        false,
        900,
        60,
    );
    write_supervisor_registration(
        temp.path(),
        "agents/other",
        other.id(),
        "agents/bar",
        900,
        60,
    );

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "agent",
            "supervisor",
            "stop",
            "--owned-by",
            "agents/foo",
        ])
        .assert()
        .success();

    let _ = owned.wait();
    assert!(
        !temp
            .path()
            .join(".git/wt/supervisors/agents%2Fowned.toml")
            .exists()
    );
    assert!(
        temp.path()
            .join(".git/wt/supervisors/agents%2Fcleanup-disabled.toml")
            .exists()
    );
    assert!(
        temp.path()
            .join(".git/wt/supervisors/agents%2Fother.toml")
            .exists()
    );
    cleanup_disabled.kill().unwrap();
    let _ = cleanup_disabled.wait();
    other.kill().unwrap();
    let _ = other.wait();
}

#[cfg(unix)]
#[test]
fn supervisor_stop_escalates_sigkill_after_timeout() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    let mut child = spawn_term_ignoring_child();
    write_supervisor_registration(temp.path(), "agents/stubborn", child.id(), "user", 900, 60);

    let started = Instant::now();
    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "agent",
            "supervisor",
            "stop",
            "agents/stubborn",
        ])
        .assert()
        .success();
    assert!(started.elapsed() >= Duration::from_secs(5));

    let _ = child.wait();
    assert!(
        !temp
            .path()
            .join(".git/wt/supervisors/agents%2Fstubborn.toml")
            .exists()
    );
}
