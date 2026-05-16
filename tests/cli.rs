use assert_cmd::Command;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::path::Path;
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
        .stdout(predicate::str::contains("issue"))
        .stdout(predicate::str::contains("pr"))
        .stdout(predicate::str::contains("new"));
}

#[test]
fn new_without_args_requires_branch_text_or_task_option() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "wt new requires branch-name text or --task",
        ));
}

#[test]
fn new_task_option_without_value_enters_task_selection() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "new", "--task"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No task files found in .local/tasks",
        ));
}

#[test]
fn new_help_explains_branch_text_and_task_selection() {
    wt_command()
        .args(["new", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("branch-name text"))
        .stdout(predicate::str::contains("--task [<TASK>]"))
        .stdout(predicate::str::contains(
            "repeat with branch-name text for multiple; omit value to select",
        ));
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
        .stdout(predicate::str::contains("wt new --task"))
        .stdout(predicate::str::contains("Omit task keys"))
        .stdout(predicate::str::contains(
            "already have [origin] are excluded",
        ))
        .stdout(predicate::str::contains("already has origin"));
}

#[test]
fn stack_run_help_explains_runnable_stack_selection() {
    wt_command()
        .args(["stack", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("omit to select a runnable stack"))
        .stdout(predicate::str::contains("next prepared or failed task"))
        .stdout(predicate::str::contains("no currently running task"))
        .stdout(predicate::str::contains("shorthand id"))
        .stdout(predicate::str::contains("latest").not());
}

#[test]
fn completion_generates_script() {
    wt_command()
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("_wt"));
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

[agent]
cli = "codex"
args = ["--model", "gpt-5.5"]
"#,
    )
    .unwrap();

    std::fs::create_dir_all(temp.path().join(".local/profiles/codex/prompts")).unwrap();
    std::fs::create_dir_all(
        temp.path()
            .join(".local/profiles/codex/scaffold/.codex/skills"),
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".local/.wt.toml"),
        r#"
[worktree]
copy = [".env"]

[setup.env]
LOG_LEVEL = "debug"
PRIVATE_TOKEN = "secret"

[profile]
name = "codex"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".local/profiles/codex/profile.toml"),
        r#"
[agent]
cli = "codex"
args = ["--yolo"]

[agent.prompt]
common = ["profile common\n"]
issue = ["from profile.toml\n"]
new = ["new branch prompt\n"]

[agent.prompt.append]
common = ["profile common append\n"]
new = ["new branch append\n"]

[workspace]
tabs = ["pnpm dev"]

[setup.env]
CODEX_MODE = "1"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".local/profiles/codex/prompts/common.md"),
        "from common prompt file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".local/profiles/codex/prompts/common.append.md"),
        "from common append file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".local/profiles/codex/prompts/issue.md"),
        "from prompt file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".local/profiles/codex/prompts/issue.append.md"),
        "from prompt append file\n",
    )
    .unwrap();
    std::fs::write(
        temp.path()
            .join(".local/profiles/codex/scaffold/AGENTS.override.md"),
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
        agent.prompt.get("new").unwrap(),
        &vec![
            "from common prompt file\n\nfrom common append file\n".to_string(),
            "new branch prompt\n\nnew branch append\n".to_string(),
        ]
    );
    assert_eq!(
        agent.prompt.get("pr").unwrap(),
        &vec!["from common prompt file\n\nfrom common append file\n".to_string()]
    );

    let copy_as = config.worktree.copy_as;
    assert!(
        copy_as
            .iter()
            .any(|entry| { entry.from == ".local/profiles/codex/scaffold" && entry.to == "." })
    );
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
