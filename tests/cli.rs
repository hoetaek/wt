use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

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

fn write_task_document(root: &Path, key: &str, branch: &str) {
    let dir = root.join(".git/wt/tasks");
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
    let dir = root.join(".git/wt/task-runs");
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

fn write_workflow_file(root: &Path, id: &str, mode: &str, extra: &str, tasks: &str) {
    let dir = root.join(".git/wt/workflows");
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
    let dir = root.join(".git/wt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), content).unwrap();
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
        .stdout(predicate::str::contains("ui"));
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
            "Issue number or provider-specific key",
        ))
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
        .stdout(predicate::str::contains("select multiple open PRs"));
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
        .stdout(predicate::str::contains("Task Run Coordinator Handoff"))
        .stdout(predicate::str::contains("Task-run agents report PR=none"))
        .stdout(predicate::str::contains("wt workflow task --mode batch"))
        .stdout(predicate::str::contains("wt workflow task --mode single"));
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
fn task_list_help_explains_canonical_inventory() {
    wt_command()
        .args(["task", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("canonical read-only inventory"))
        .stdout(predicate::str::contains(
            "whether or not they are selectable by wt run task",
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
    write_task_document(temp.path(), "completed", "feature/completed");
    write_task_run_file(
        temp.path(),
        "run-completed",
        "completed",
        "feature/completed",
        "done",
        "",
    );
    write_task_document(temp.path(), "local", "feature/local");
    std::fs::write(
        temp.path().join(".git/wt/tasks/provider.toml"),
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
        temp.path().join(".git/wt/tasks/bad.toml"),
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
    assert_eq!(tasks.len(), 3);
    assert_eq!(invalid.len(), 1);

    let completed = tasks
        .iter()
        .find(|row| row["key"] == "completed")
        .expect("completed task should be listed even after a done TaskRun");
    assert_eq!(
        completed["path"],
        "<git-common-dir>/wt/tasks/completed.toml"
    );
    assert_eq!(completed["branch"], "feature/completed");
    assert_eq!(completed["publish_state"], "local");
    assert_eq!(completed["source"], "local");
    assert!(completed["origin"].is_null());

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
    assert_eq!(invalid[0]["path"], "<git-common-dir>/wt/tasks/bad.toml");
    assert!(
        invalid[0]["error"]
            .as_str()
            .unwrap()
            .contains("Failed to parse task")
    );
}

#[test]
fn task_list_text_includes_stable_task_fields_and_invalid_warning() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());
    write_task_document(temp.path(), "local", "feature/local");
    std::fs::write(
        temp.path().join(".git/wt/tasks/provider.toml"),
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
        temp.path().join(".git/wt/tasks/bad.toml"),
        "unknown = true\n",
    )
    .unwrap();

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "task", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "local  not published | task local | branch feature/local | source local",
        ))
        .stdout(predicate::str::contains(
            "Provider task  Linear PROJ-123 | task provider | branch alice/provider-task | source provider-origin",
        ))
        .stdout(predicate::str::contains("Path: <git-common-dir>/wt/tasks/local.toml"))
        .stdout(predicate::str::contains("Origin: none"))
        .stdout(predicate::str::contains("Origin: linear:PROJ-123"))
        .stdout(predicate::str::contains("Summary: Task body"))
        .stdout(predicate::str::contains("Summary: Provider task body"))
        .stderr(predicate::str::contains(
            "Invalid task <git-common-dir>/wt/tasks/bad.toml",
        ));
}

#[test]
fn task_import_help_explains_behavior() {
    wt_command()
        .args(["task", "import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Import existing provider issues"))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/tasks/<safe-issue-id>.toml",
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
        .stdout(predicate::str::contains("write [origin]"))
        .stdout(predicate::str::contains("does not start workspaces"))
        .stdout(predicate::str::contains("wt run task and wt run workflow"))
        .stdout(predicate::str::contains("Omit task keys"))
        .stdout(predicate::str::contains(
            "already have [origin] are excluded",
        ))
        .stdout(predicate::str::contains("already has origin"));
}

#[test]
fn msg_help_explains_agent_inbox_contract() {
    wt_command()
        .args(["msg", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("file-based agent inbox"))
        .stdout(predicate::str::contains(
            "<git-common-dir>/wt/messages/agents/<agent>/inbox",
        ))
        .stdout(predicate::str::contains("wt msg send --to <agent>"))
        .stdout(predicate::str::contains(
            "wt msg check-inbox --agent <agent>",
        ))
        .stdout(predicate::str::contains("inbox/read"));

    wt_command()
        .args(["msg", "send", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Target agent id as NAME or agents/NAME",
        ))
        .stdout(predicate::str::contains("Message text"));

    wt_command()
        .args(["msg", "check-inbox", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hook JSON"))
        .stdout(predicate::str::contains("Agent id as NAME or agents/NAME"));
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
            "<git-common-dir>/wt/messages/agents/codex/inbox/",
        ));

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    let files = toml_files(&inbox);
    assert_eq!(files.len(), 1);

    let content = std::fs::read_to_string(&files[0]).unwrap();
    let message: toml::Value = toml::from_str(&content).unwrap();
    assert_eq!(message["meta"]["to"].as_str(), Some("agents/codex"));
    assert_eq!(message["meta"]["from"].as_str(), Some("agents/user"));
    assert_eq!(message["envelope"]["kind"].as_str(), Some("request"));
    assert_eq!(
        message["envelope"]["expects_response"].as_bool(),
        Some(true)
    );
    assert_eq!(message["body"]["summary"].as_str(), Some("hello"));
    assert_eq!(
        message["body"]["parts"][0]["content"].as_str(),
        Some("hello")
    );
}

#[test]
fn msg_check_inbox_emits_hook_json_and_moves_messages_to_read() {
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
    assert!(context.contains("WT INBOX for agents/codex: 1 unread message"));
    assert!(context.contains("hello from claude"));
    assert!(context.contains("wt msg send --to <agent> <message>"));

    let inbox = temp.path().join(".git/wt/messages/agents/codex/inbox");
    assert!(toml_files(&inbox).is_empty());
    assert_eq!(toml_files(&inbox.join("read")).len(), 1);
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
    assert_eq!(toml_files(&common_inbox).len(), 1);
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
    assert!(toml_files(&common_inbox).is_empty());
    assert_eq!(toml_files(&common_inbox.join("read")).len(), 1);
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
        .stdout(predicate::str::contains("<git-common-dir>/wt/workflows"))
        .stdout(predicate::str::contains(legacy_local_path("workflows")).not());
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
        temp.path().join(".git/wt/workflows/bad.toml"),
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
        "<git-common-dir>/wt/workflows/2026-05-18-001.toml"
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
    assert_eq!(invalid[0]["path"], "<git-common-dir>/wt/workflows/bad.toml");
    assert!(
        invalid[0]["error"]
            .as_str()
            .unwrap()
            .contains("Failed to parse workflow")
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

    let workflows = std::fs::read_dir(temp.path().join(".git/wt/workflows"))
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
            "Prepared workflow: <git-common-dir>/wt/workflows/",
        ));

    assert!(!repo.join(legacy_local_path("workflows")).exists());
    let workflows = std::fs::read_dir(repo.join(".git/wt/workflows"))
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
    assert!(path.starts_with("<git-common-dir>/wt/workflows/"));
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
        .stdout(predicate::str::contains("unchanged running observations"))
        .stdout(predicate::str::contains(
            "Omit TARGET in an interactive terminal",
        ));
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
fn init_yes_uses_minimal_preset_without_agent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "init", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created config:"));

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config.toml")).unwrap();
    assert!(content.contains("[workspace]"));
    assert!(!content.contains("[profile.agent]"));
    assert!(!content.contains("[issues]"));
}

#[test]
fn init_preset_agent_yes_writes_agent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--preset",
            "agent",
            "--yes",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config.toml")).unwrap();
    assert!(content.contains("[profile.agent]"));
    assert!(content.contains("cli = \"codex\""));
}

#[test]
fn init_minimal_shortcut_writes_minimal_config() {
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
        .success();

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config.toml")).unwrap();
    assert!(content.contains("[workspace]"));
    assert!(!content.contains("[profile.agent]"));
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
            "--preset",
            "app",
            "--yes",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Init plan"))
        .stdout(predicate::str::contains("==>").not())
        .stdout(predicate::str::contains("target:"))
        .stdout(predicate::str::contains("preset: app"))
        .stdout(predicate::str::contains(
            "selected sections: setup, test, workspace",
        ))
        .stdout(predicate::str::contains("detected signals:"))
        .stdout(predicate::str::contains("[ok] detected setup: npm install"))
        .stdout(predicate::str::contains("setup: npm install"))
        .stdout(predicate::str::contains("test: npm test"))
        .stdout(predicate::str::contains("[setup]"))
        .stdout(predicate::str::contains("[test]"))
        .stdout(predicate::str::contains("# [workflow]"))
        .stdout(predicate::str::contains("# pull_request = \"none\""))
        .stdout(predicate::str::contains("# landing = \"manual\""))
        .stdout(predicate::str::contains("\n    [workflow]\n").not());

    assert!(!temp.path().join(".wt.toml").exists());
    assert!(!temp.path().join(".git/wt/config.toml").exists());
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
            "--minimal",
            "--yes",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("Init plan\n"))
        .stdout(predicate::str::contains("==>").not())
        .stdout(predicate::str::contains("preset: minimal"));
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
            "--minimal",
            "--yes",
        ])
        .assert()
        .success()
        .stdout("");

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config.toml")).unwrap();
    assert!(content.contains("[workspace]"));
}

#[test]
fn init_json_flag_rejects_init_without_status_decoration() {
    wt_command()
        .args(["--json", "init", "--minimal", "--yes", "--dry-run"])
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
        .stderr(predicate::str::contains("Config already exists:"))
        .stderr(predicate::str::contains("use --force to overwrite"));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "init",
            "--preset",
            "agent",
            "--yes",
            "--force",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("WARNING:"))
        .stdout(predicate::str::contains("Updated config:"));

    let content = std::fs::read_to_string(temp.path().join(".git/wt/config.toml")).unwrap();
    assert!(content.contains("[profile.agent]"));
}

#[test]
fn init_rejects_conflicting_preset_and_minimal() {
    wt_command()
        .args(["init", "--preset", "minimal", "--minimal"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "'--preset <PRESET>' cannot be used with '--minimal'",
        ));
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

    std::fs::create_dir_all(temp.path().join(".git/wt/profiles/codex/prompts")).unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join(".git/wt/profiles/codex/scaffold/.codex/skills"),
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
        temp.path().join(".git/wt/profiles/codex/profile.toml"),
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
        temp.path().join(".git/wt/profiles/codex/prompts/common.md"),
        "from common prompt file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/profiles/codex/prompts/common.append.md"),
        "from common append file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".git/wt/profiles/codex/prompts/issue.md"),
        "from prompt file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/profiles/codex/prompts/issue.append.md"),
        "from prompt append file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".git/wt/profiles/codex/scaffold/AGENTS.override.md"),
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
        Path::new(&entry.from).ends_with(".git/wt/profiles/codex/scaffold") && entry.to == "."
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
