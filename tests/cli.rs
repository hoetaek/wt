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
        .stdout(predicate::str::contains("new"))
        .stdout(predicate::str::contains("agent"));
}

#[test]
fn new_without_args_requires_branch_text() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "new"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "wt new starts one ad hoc worktree from branch-name text",
        ));
}

#[test]
fn new_task_option_is_unknown() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "new", "--task"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument '--task'"));
}

#[test]
fn new_help_explains_branch_text_only() {
    wt_command()
        .args(["new", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("branch-name text"))
        .stdout(predicate::str::contains("--task").not());
}

#[test]
fn task_run_help_explains_task_execution() {
    wt_command()
        .args(["task", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("one worktree per selected"))
        .stdout(predicate::str::contains("source = \"new\" TaskRun"))
        .stdout(predicate::str::contains("Task Run Coordinator Handoff"))
        .stdout(predicate::str::contains("Task-run agents report PR=none"))
        .stdout(predicate::str::contains("wt workflow task --mode batch"))
        .stdout(predicate::str::contains("wt workflow task --mode single"));
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
        .stdout(predicate::str::contains("wt task run and wt workflow run"))
        .stdout(predicate::str::contains("Omit task keys"))
        .stdout(predicate::str::contains(
            "already have [origin] are excluded",
        ))
        .stdout(predicate::str::contains("already has origin"));
}

#[test]
fn workflow_run_help_explains_omitted_target_selection() {
    wt_command()
        .args(["workflow", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Omit WORKFLOW"))
        .stdout(predicate::str::contains("choose from runnable workflows"))
        .stdout(predicate::str::contains(
            "omit to select a runnable workflow",
        ));
}

#[test]
fn workflow_prepare_help_uses_pr_mode() {
    wt_command()
        .args(["workflow", "task", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--pr <none|draft|ready>"))
        .stdout(predicate::str::contains("--pull-request").not());

    wt_command()
        .args(["workflow", "issue", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--pr <none|draft|ready>"))
        .stdout(predicate::str::contains("--pull-request").not());
}

#[test]
fn workflow_prepare_rejects_pr_none_on_non_stack_modes() {
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
            "none",
            "workflow-docs",
            "--base",
            "main",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--pr is only valid with --mode stack",
        ));

    wt_command()
        .args([
            "-C",
            temp.path().to_str().unwrap(),
            "workflow",
            "issue",
            "--mode",
            "batch",
            "--pr",
            "none",
            "PROJ-123",
            "--base",
            "main",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--pr is only valid with --mode stack",
        ));
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
        .stdout(predicate::str::contains(
            "Omit TARGET in an interactive terminal",
        ));
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
        ));
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
fn init_yes_uses_minimal_preset_without_agent() {
    let temp = TempDir::new().unwrap();
    git_init(temp.path());

    wt_command()
        .args(["-C", temp.path().to_str().unwrap(), "init", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created config:"));

    let content = std::fs::read_to_string(temp.path().join(".local/.wt.toml")).unwrap();
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

    let content = std::fs::read_to_string(temp.path().join(".local/.wt.toml")).unwrap();
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

    let content = std::fs::read_to_string(temp.path().join(".local/.wt.toml")).unwrap();
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
        .stdout(predicate::str::contains("[test]"));

    assert!(!temp.path().join(".wt.toml").exists());
    assert!(!temp.path().join(".local/.wt.toml").exists());
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

    let content = std::fs::read_to_string(temp.path().join(".local/.wt.toml")).unwrap();
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
    std::fs::create_dir_all(temp.path().join(".local")).unwrap();
    std::fs::write(
        temp.path().join(".local/.wt.toml"),
        "[workspace]\ntabs = []\n",
    )
    .unwrap();

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

    let content = std::fs::read_to_string(temp.path().join(".local/.wt.toml")).unwrap();
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

[workflow.defaults]
pull_request = "draft"
landing = "after_review"
landing_requires_approval = false

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
    let policy = config.workflow_default_policy();
    assert_eq!(
        policy.pull_request,
        wt::config::WorkflowDefaultPullRequestMode::Draft
    );
    assert_eq!(
        policy.landing,
        wt::config::WorkflowDefaultLandingPolicy::AfterReview
    );
    assert!(!policy.landing_requires_approval);

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
