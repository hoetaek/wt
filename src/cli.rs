use clap::{ArgAction, ArgGroup, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const ROOT_HELP_TEMPLATE: &str = "\
{about}

{usage-heading} {usage}

Start Work:
  run issue [ISSUE]       Start from provider issue
  run pr [PR]             Start from pull request
  run branch <TEXT>       Start ad hoc branch work
  run task [TASK]...      Start local TaskDocuments
  run workflow [WORKFLOW] Start saved Workflow tasks

Manage Work:
  open     Open an existing worktree or branch
  inspect  Read a work dossier
  done     Clean completed work
  list     Show current wt state

Prepare:
  scaffold  Create idea/spec/task/workflow skeletons
  task      Manage local TaskDocuments
  workflow  Prepare and coordinate saved workflow tasks

Coordinate Agents:
  agent    Observe task-agent runtime state
  msg      Send and inspect agent inbox messages
  send     Send a live cmux prompt message
  session  Manage current agent identity

Run Agents:
  codex   Launch Codex with wt agent identity
  claude  Launch Claude with wt agent identity
  as      Run any command with explicit WT_AGENT_ID

Setup:
  init        Start the config recommendation wizard
  config      Print, edit, or refactor config
  profile     List or manage named profile configs
  setup       Install or remove per-machine integration
  doctor      Check config and local tools

Tools:
  ui          Start the read-only personal state web UI
  studio      Start the write-capable authoring surface
  site        Inspect and manage local site helpers
  shell-init  Print shell integration source
  completion  Generate shell completions
  version     Print wt version

Options:
{options}{after-help}";

#[derive(Parser, Debug)]
#[command(
    name = "wt",
    version,
    about = "Worktree-based agent orchestration harness",
    help_template = ROOT_HELP_TEMPLATE,
    after_help = "Examples:\n  $ wt init\n  $ wt run issue 123\n  $ wt run pr 42\n  $ wt run branch \"fix login\"\n  $ wt run task\n  $ wt run workflow release-stack\n  $ wt inspect <target> --pr\n  $ wt agent watch <target> --heartbeat 300\n  $ wt run -h\n  $ wt help <cmd>"
)]
pub struct Cli {
    /// Run wt from DIR
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// Read wt config from PATH
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Show more diagnostics (-v, -vv)
    #[arg(short, long, action = ArgAction::Count, global = true, conflicts_with = "quiet")]
    pub verbose: u8,
    /// Hide routine status output
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// When to use terminal colors
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true)]
    pub color: ColorMode,
    /// Disable terminal colors
    #[arg(long = "no-color", global = true, conflicts_with = "color")]
    pub no_color: bool,
    /// Output JSON for supported commands
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Commands {
    /// Print wt version
    Version,
    /// Generate shell completion script
    Completion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print shell integration source for ambient worker identity binding
    #[command(
        long_about = "Print shell integration source for ambient worker identity binding.\n\nAdd `eval \"$(wt shell-init zsh)\"` or `eval \"$(wt shell-init bash)\"` to your shell rc. The generated hook runs `wt env` when the current directory changes so worker worktree shells inherit WT_AGENT_ID. See docs/architecture.md#shell-integration."
    )]
    ShellInit {
        /// Shell to initialize: zsh or bash
        #[arg(value_enum)]
        shell: ShellInitShell,
    },
    /// Print shell statements for the current worker worktree identity
    #[command(
        name = "env",
        hide = true,
        long_about = "Internal shell-hook command. Print export/unset statements for WT_AGENT_ID based on the current git worktree branch and matching <repo-root>/.wt/execution/task-runs records, while clearing removed legacy coordinator routing env.\n\nThis command is intended to be called by source generated from `wt shell-init <shell>`."
    )]
    Env,
    /// Declare, clear, or inspect the current session agent identity
    #[command(
        long_about = "Declare, clear, or inspect the current session agent identity using the current terminal or agent-session anchor.\n\nUse `eval \"$(wt session set <id>)\"` to bind this shell or agent session to WT_AGENT_ID while also writing a marker that later wt invocations from the same anchor can resolve."
    )]
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Start workspace execution from issue, PR, branch text, task, or workflow
    #[command(
        long_about = "Start workspace execution from issues, pull requests, branch-name text, local TaskDocuments, or saved Workflows.\n\nCanonical start surfaces are `wt run issue`, `wt run pr`, `wt run branch`, `wt run task`, and `wt run workflow`.\n\n`wt run` only starts workspace execution. Cleanup stays under `wt done`, inspection under `wt inspect`, agent observation under `wt agent`, and saved workflow lifecycle actions under `wt workflow`."
    )]
    Run {
        #[command(subcommand)]
        command: RunCommand,
    },
    #[command(name = "issue", hide = true, disable_help_flag = true)]
    DeprecatedIssue {
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    #[command(name = "pr", hide = true, disable_help_flag = true)]
    DeprecatedPr {
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    #[command(name = "new", hide = true, disable_help_flag = true)]
    DeprecatedNew {
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    /// Manage local TaskDocuments
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Prepare, inspect, edit, repair, archive, or pass workflow tasks
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
    },
    /// Create blank skeleton documents for a feature
    #[command(
        long_about = "Create blank skeleton documents for a feature under <repo-root>/.wt/planning/ideas, numbered specs, tasks, workflows, and spec-local retrospects. Pass one or more document-kind flags, use --all for every kind, or omit flags to choose interactively."
    )]
    Scaffold {
        /// Feature slug to use for every generated path
        #[arg(value_name = "FEATURE")]
        feature: String,
        /// Create <repo-root>/.wt/planning/ideas/<feature>.md
        #[arg(long)]
        idea: bool,
        /// Create phase-folder prep files under <repo-root>/.wt/planning/specs/<feature>/
        #[arg(long)]
        spec: bool,
        /// Create <repo-root>/.wt/execution/tasks/<feature>.toml
        #[arg(long)]
        task: bool,
        /// Create <repo-root>/.wt/execution/workflows/<feature>.toml
        #[arg(long)]
        workflow: bool,
        /// Create <repo-root>/.wt/planning/specs/<feature>/04-Feedback/09-retrospect.md
        #[arg(long)]
        retrospect: bool,
        /// Create all scaffold document kinds
        #[arg(
            long,
            conflicts_with_all = ["idea", "spec", "task", "workflow", "retrospect"]
        )]
        all: bool,
        /// Overwrite existing scaffold files
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Show worktree, branch, site, and setup state
    List {
        /// Do not truncate table columns to fit the terminal
        #[arg(long)]
        wide: bool,
    },
    /// Open a workspace from an existing worktree or branch
    Open {
        /// Branch or worktree directory name to open directly
        target: Option<String>,
    },
    /// Remove worktrees, clean integrations, and delete local branches
    #[command(
        long_about = "Remove checked-out worktrees, clean integrations, mark matching direct running TaskRuns passed, and delete local branches.\n\nPass branch, worktree path/name, issue-like branch-name shorthand, or direct TaskRun id. Workflow-linked TaskRun ids are passed with `wt workflow pass`, not `wt done`. Omit TARGETS to choose worktrees interactively."
    )]
    Done {
        /// Branch, worktree path/name, issue-like branch-name shorthand, or direct TaskRun id to remove
        targets: Vec<String>,
    },
    /// Read a work dossier for a branch, worktree, or TaskRun
    #[command(
        long_about = "Read a concise, read-only work dossier for a branch, worktree path/name, or TaskRun id. Omit TARGET in an interactive terminal to choose an inspectable work target; pass TARGET explicitly for scripts and non-interactive use. Pass --pr to fetch and render read-only pull request review evidence for the inspected branch."
    )]
    Inspect {
        /// Branch, worktree path/name, or TaskRun id to inspect
        target: Option<String>,
        /// Fetch and render read-only pull request review evidence for the inspected branch
        #[arg(long)]
        pr: bool,
    },
    /// Observe and watch task agent runtime state
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Set up or remove per-machine wt integration
    #[command(
        long_about = "Set up or remove per-machine wt integration.\n\n`wt setup` detects supported local agent CLIs, renders a structured plan of target files and planned actions, prompts before installing wt-managed Claude and Codex inbox hooks, and can add shell integration and completion eval lines to the resolved shell rc file. Repo-local `.wt` storage is prepared by `wt init`, not `wt setup`. Use --yes to apply detected steps without prompting, --dry-run to preview the plan without writing files, and --remove to remove wt-managed per-machine entries."
    )]
    Setup {
        /// Accept every detected setup step without prompting
        #[arg(short = 'y', long = "yes")]
        yes: bool,
        /// Preview setup or removal without writing files
        #[arg(long)]
        dry_run: bool,
        /// Remove wt-managed per-machine setup entries
        #[arg(long)]
        remove: bool,
    },
    /// Launch Codex with the current worktree's wt agent identity
    #[command(
        long_about = "Launch Codex with WT_AGENT_ID derived from the current git branch, and clear removed legacy coordinator routing env before the child process starts.\n\nUse `wt codex` for the default agent inbox `agents/<branch_slug>`. In the same worktree, use a leading role such as `wt codex @planner` or `wt codex @reviewer` to launch a separate inbox like `agents/<branch_slug>-planner`, so multiple agents do not consume each other's messages. Extra Codex arguments are passed through after the optional role."
    )]
    Codex {
        /// Optional @role followed by arguments passed to codex
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    /// Launch Claude with the current worktree's wt agent identity
    #[command(
        long_about = "Launch Claude with WT_AGENT_ID derived from the current git branch, and clear removed legacy coordinator routing env before the child process starts.\n\nUse `wt claude` for the default agent inbox `agents/<branch_slug>`. In the same worktree, use a leading role such as `wt claude @coordinator` or `wt claude @reviewer` to launch a separate inbox like `agents/<branch_slug>-coordinator`, so multiple agents do not consume each other's messages. Extra Claude arguments are passed through after the optional role."
    )]
    Claude {
        /// Optional @role followed by arguments passed to claude
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    /// Run any command with an explicit wt agent identity
    #[command(
        long_about = "Run any command with an explicit WT_AGENT_ID, and clear removed legacy coordinator routing env before the child process starts.\n\nUse `wt as <AGENT> -- <COMMAND>` as the low-level escape hatch for scripts, unusual agent CLIs, or identities that should not be derived from the current branch. For daily Codex and Claude launches, prefer `wt codex`, `wt codex @planner`, `wt claude`, or `wt claude @reviewer`."
    )]
    As {
        /// Agent id as NAME or agents/NAME
        agent: String,
        /// Command to run with WT_AGENT_ID set
        #[arg(
            value_name = "COMMAND",
            required = true,
            num_args = 1..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        command: Vec<String>,
    },
    /// Start a read-only personal state web UI
    #[command(
        long_about = "Start a read-only personal wt state web UI. The server binds to 127.0.0.1, prints the local URL, and opens it in the default browser unless --quiet is set. It serves embedded no-build assets and exposes only allowlisted routes including GET /api/snapshot for <repo-root>/.wt ideas, spec-local and cross-work retrospectives, TaskDocuments, Workflows, TaskRuns, profiles, and effective config summaries."
    )]
    Ui {
        /// Port to bind on 127.0.0.1; 0 selects an available port
        #[arg(long, default_value_t = 0, value_name = "PORT")]
        port: u16,
    },
    /// Start the write-capable authoring web surface
    #[command(
        long_about = "Start the write-capable wt studio authoring surface. The server binds only to 127.0.0.1, prints a one-time /auth URL, and opens it in the default browser unless --quiet is set. By default Studio serves the embedded Vite production bundle from the wt binary. Pass --dev while running the Vite dev server from src/studio/web to use HMR assets instead; /auth and /api requests should be proxied back to this Studio server. API routes require the session cookie and a matching browser Origin header."
    )]
    Studio {
        /// Port to bind on 127.0.0.1; 0 selects an available port
        #[arg(long, default_value_t = 0, value_name = "PORT")]
        port: u16,
        /// Use Vite dev-server assets instead of the embedded production bundle
        #[arg(long)]
        dev: bool,
        /// Browser origin for --dev; defaults to Vite's loopback dev origin
        #[arg(long, value_name = "ORIGIN", requires = "dev")]
        dev_origin: Option<String>,
    },
    /// Send, deliver, and inspect file-based agent inbox messages
    #[command(
        long_about = "Send, deliver, observe, and inspect file-based agent inbox messages stored under <repo-root>/.wt/runtime/agents/<agent>/inbox/<state>.\n\nUse `wt msg send --to agents/<agent> <message>` as a low-level explicit inbox write. Task completion should use `wt task report <message>`, which derives direct or workflow scope from the current TaskRun; coordinator feedback should use `wt task review <task-run-id> --accept|--reject|--block <message>`, which sends task_run:<id> scope to the recorded task agent. Use `wt msg list --agent <agent>` and `wt msg read --agent <agent> <message-id>` for read-only lifecycle inspection. Use `wt msg watch --agent <agent> --timeout 300` to observe one agent's inbox/new without claiming messages; omitted --agent falls back to WT_AGENT_ID, then the current live identity anchor. `wt msg check-inbox --silent` is an internal hook consumer for the implicit inbox resolved from WT_AGENT_ID, then the current live identity anchor; missing both exits successfully with no output. `--silent` makes the command exit 0 quietly when wt context cannot load (non-git CWD, legacy `.local/.wt.toml`, missing setup), so a globally installed hook never blocks the agent. Pass `--agent <agent>` only as an explicit single-inbox override. Deliverable direct-scope messages and authorized workflow/task_run scoped messages from inbox/new or eligible inbox/retry are claimed, emitted as hook-compatible JSON, then acknowledged into inbox/delivered after stdout is written."
    )]
    Msg {
        #[command(subcommand)]
        command: MsgCommand,
    },
    /// Send a message to a task agent's cmux surface
    Send {
        /// Branch, worktree path/name, or TaskRun id to contact
        target: String,
        /// Message to send
        #[arg(
            value_name = "MESSAGE",
            required = true,
            num_args = 1..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        message: Vec<String>,
        /// Insert the message without pressing enter
        #[arg(long)]
        no_enter: bool,
    },
    /// Check configured providers and required local tools
    Doctor {
        /// Run checks against the effective config for <repo-root>/.wt/config/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Delete one env-keyed identity anchor by display key, for example surface:A22D...
        #[arg(long = "prune-env-anchors", value_name = "KEY")]
        prune_env_anchors: Option<String>,
    },
    /// Print, edit, or refactor wt config files
    #[command(
        long_about = "Print, edit, or refactor wt-managed config files. Shared repo config is .wt.toml and SOURCE name `shared`; private repo config is <repo-root>/.wt/config/local.toml and SOURCE name `local`; named profile config is <repo-root>/.wt/config/profiles/<name>/profile.toml and SOURCE name `profiles/<name>`. Config edit, extract, and inline reject files outside that managed namespace."
    )]
    Config {
        /// Show effective config using <repo-root>/.wt/config/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    /// List or manage named profile configs
    #[command(
        long_about = "List or manage named profile configs stored under <repo-root>/.wt/config/profiles/<name>/profile.toml. Bare `wt profile` is an omission-default that runs `wt profile list`; the canonical inventory surface is the explicit `wt profile list` subcommand. Use `wt profile create <name>` to scaffold a new profile."
    )]
    Profile {
        #[command(subcommand)]
        command: Option<ProfileCommand>,
    },
    /// 이 저장소에 맞는 config 추천 wizard 시작
    #[command(
        long_about = "Start a project-specific config recommendation wizard and bootstrap repo-local wt storage.\n\n`wt init` prepares the current repository: it writes one selected config file, prepares the canonical <repo-root>/.wt/ personal state directory path when applying changes, and records the clone-local `/.wt` ignore line in git info/exclude. An existing `.wt` symlink to a directory is accepted. Use --dry-run to preview without writing files."
    )]
    Init {
        /// 개인 설정 파일에 쓰기
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        /// 팀 공유 설정을 .wt.toml에 쓰기
        #[arg(long)]
        shared: bool,
        /// [profile.agent]에 저장할 agent runtime
        #[arg(long, value_enum)]
        agent: Option<InitAgent>,
        /// [profile.agent]에 추가할 실행 인자
        #[arg(long = "agent-arg", allow_hyphen_values = true)]
        agent_args: Vec<String>,
        /// agent 실행 command override
        #[arg(long)]
        agent_command: Option<String>,
        /// 설정할 issue provider
        #[arg(long, value_enum)]
        issue_provider: Option<InitIssueProvider>,
        /// 설정할 local site provider
        #[arg(long, value_enum)]
        site_provider: Option<InitSiteProvider>,
        /// issue 목록 필터링에 사용할 GitHub 사용자
        #[arg(long)]
        gh_user: Option<String>,
        /// interactive 질문을 건너뛰고 기본 추천값으로 쓰기
        #[arg(long)]
        yes: bool,
        /// 파일을 쓰지 않고 대상, 추천 모드, 감지된 신호, TOML 미리보기
        #[arg(long)]
        dry_run: bool,
        /// non-interactive 쓰기에서 기존 설정 파일 덮어쓰기
        #[arg(long)]
        force: bool,
    },
    /// Inspect and manage local site provider helpers
    Site {
        #[command(subcommand)]
        command: SiteCommand,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum SessionCommand {
    /// Write a session identity anchor and print shell exports
    #[command(
        long_about = "Write a session identity anchor for the current terminal or agent-session anchor and print shell exports.\n\nUse `eval \"$(wt session set <id>)\"`, for example `eval \"$(wt session set coord-review-routing)\"`, so the current shell gets WT_AGENT_ID immediately while later wt invocations from the same anchor can resolve the identity anchor."
    )]
    Set {
        /// Agent id as NAME or agents/NAME
        id: String,
    },
    /// Remove the current session identity anchor and print shell unsets
    Unset,
    /// Print the current session identity resolution
    Show {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellInitShell {
    Zsh,
    Bash,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum ConfigCommand {
    /// Open a config file in the configured editor
    #[command(
        long_about = "Open a wt-managed config source in the configured editor. SOURCE may be `shared`, `local`, `profiles/<name>`, or a canonical path to one of those managed files. Missing managed files are created by the editor path after parent directories are prepared. Omit SOURCE to select from existing managed config files; non-managed paths are rejected before the editor opens."
    )]
    Edit {
        /// Managed config source to edit: shared, local, profiles/<name>, or a canonical path
        #[arg(value_name = "SOURCE")]
        source: Option<PathBuf>,
    },
    /// Move selected config sections into the next structured config file
    #[command(
        long_about = "Move selected config sections out of a wt-managed config source. SOURCE may be `shared`, `local`, `profiles/<name>`, or a canonical path to one of those managed files. Omit SOURCE to select from managed config files; non-managed paths are rejected."
    )]
    Extract {
        /// Managed config source to refactor: shared, local, profiles/<name>, or a canonical path
        #[arg(value_name = "SOURCE")]
        source: Option<PathBuf>,
    },
    /// Move selected structured config back inline
    #[command(
        long_about = "Move selected structured config back inline from a wt-managed config source. SOURCE may be `shared`, `local`, `profiles/<name>`, or a canonical path to one of those managed files. Prompt convention files are inlined through their owning profile source; direct prompt-file SOURCE paths are rejected."
    )]
    Inline {
        /// Managed config source to refactor: shared, local, profiles/<name>, or a canonical path
        #[arg(value_name = "SOURCE")]
        source: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum ProfileCommand {
    /// List named profile configs
    #[command(
        long_about = "List named profile configs discovered under <repo-root>/.wt/config/profiles/<name>/profile.toml. Profiles are listed in deterministic name order with their copy, link, and agent summary. Invalid profile records are surfaced as warnings in text output and as `invalid_profiles` entries in JSON output rather than being silently hidden. The reserved `default` name is never shown as a valid named profile."
    )]
    List,
    /// Create a named profile scaffold
    Create {
        /// New profile name
        name: String,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum AgentCommand {
    /// Observe a task agent's current runtime state once
    #[command(
        long_about = "Observe a task agent's current runtime state from the matching cmux surface. This is read-only: it observes surface process evidence, cmux screen fallback, status, and hook signals without updating TaskRuns or provider issues. Omit TARGET in an interactive terminal to choose an observable work target; pass TARGET explicitly for scripts, --json, --quiet, and non-interactive use. Codex status is weaker until cmux Codex hooks are installed with `cmux hooks codex install --yes`."
    )]
    Status {
        /// Branch, worktree path/name, or TaskRun id to observe
        target: Option<String>,
    },
    /// Poll a task agent's runtime state until it is no longer running, becomes blocked, or reaches a bound
    #[command(
        long_about = "Poll a task agent's runtime state from the matching cmux surface. Prints compact state transitions and exits with the agent observation exit-code contract. Use --timeout to stop waiting after a bounded number of seconds, and --heartbeat to print unchanged running observations at an explicit interval. When --timeout or --heartbeat emits a non-idle sample and the runtime AgentId is known, wt agent watch appends it to <repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl. Omit TARGET in an interactive terminal to choose an observable work target; pass TARGET explicitly for scripts, --json, --quiet, and non-interactive use."
    )]
    Watch {
        /// Branch, worktree path/name, or TaskRun id to watch
        target: Option<String>,
        /// Seconds between observations
        #[arg(
            long,
            default_value_t = 2,
            value_name = "SECONDS",
            value_parser = parse_positive_u64
        )]
        interval: u64,
        /// Stop waiting after this many positive seconds
        #[arg(long, value_name = "SECONDS", value_parser = parse_positive_u64)]
        timeout: Option<u64>,
        /// Print unchanged running observations at this positive-second interval
        #[arg(long, value_name = "SECONDS", value_parser = parse_positive_u64)]
        heartbeat: Option<u64>,
    },
    /// Summarize recorded non-idle wait observations
    #[command(
        long_about = "Read a local summary of non-idle wait observations recorded by `wt agent watch` when heartbeat or timeout samples are emitted. This is read-only: it summarizes <repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl files with count, sum, average, min, max, bucket, and low-cardinality group data; it does not observe agents, contact cmux, mutate TaskRuns, or infer new watch defaults."
    )]
    WaitStats,
    /// Manage opt-in supervisors for agent inbox stale-rescue
    #[command(
        long_about = "Manage opt-in supervisors for agent inbox stale-rescue.\n\nA supervisor is default-off Layer 3 insurance for one agent identity. It records local state under <repo-root>/.wt/runtime/agents/<agent>/supervisor.toml and supervisor.log, and only intervenes after an inbox/new message has aged past --stale-threshold. Supervisors started with --surface run inside an unfocused cmux surface in the target pane so cmux push delivery stays attached to cmux without creating another workspace; supervisors without --surface use the detached process path. No wt verb starts a supervisor implicitly."
    )]
    Supervisor {
        #[command(subcommand)]
        command: AgentSupervisorCommand,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum AgentSupervisorCommand {
    /// Start a supervisor for one agent identity
    Start {
        /// Agent id as NAME or agents/NAME
        agent_id: String,
        /// Stop any existing live supervisor for this identity before starting
        #[arg(long)]
        replace: bool,
        /// Pre-bound cmux surface id for later delivery slices
        #[arg(long, value_name = "ID")]
        surface: Option<String>,
        /// Target agent kind fallback when cmux cannot detect it
        #[arg(long, value_name = "claude|codex|unknown")]
        kind: Option<String>,
        /// Whether session-end cleanup should stop this supervisor
        #[arg(long, value_name = "BOOL")]
        cleanup_on_session_end: Option<bool>,
        /// Message age before later delivery slices may rescue it
        #[arg(long, default_value = "15m", value_name = "DURATION")]
        stale_threshold: String,
        /// Poll cadence for later delivery slices
        #[arg(long, default_value = "60s", value_name = "DURATION")]
        poll_interval: String,
    },
    /// Stop registered supervisors by target identity or owner
    Stop {
        /// Agent id as NAME or agents/NAME
        agent_id: Option<String>,
        /// Stop only supervisors started by this agent id
        #[arg(long, value_name = "AGENT")]
        owned_by: Option<String>,
    },
    /// List registered supervisors
    Status {
        /// Agent id as NAME or agents/NAME
        agent_id: Option<String>,
    },
    /// Print or follow a supervisor log
    Logs {
        /// Agent id as NAME or agents/NAME
        agent_id: String,
        /// Continue printing appended log lines
        #[arg(long)]
        follow: bool,
    },
    /// Run the supervisor loop process
    #[command(hide = true)]
    Run {
        /// Agent id as NAME or agents/NAME
        agent_id: String,
        /// Run in the foreground for debugging
        #[arg(long)]
        foreground: bool,
        /// Pre-bound cmux surface id for later delivery slices
        #[arg(long, value_name = "ID")]
        surface: Option<String>,
        /// Target agent kind fallback when cmux cannot detect it
        #[arg(long, value_name = "claude|codex|unknown")]
        kind: Option<String>,
        /// Whether session-end cleanup should stop this supervisor
        #[arg(long, value_name = "BOOL")]
        cleanup_on_session_end: Option<bool>,
        /// Parsed stale threshold from start
        #[arg(
            long,
            default_value_t = 900,
            value_name = "SECONDS",
            value_parser = parse_positive_u64
        )]
        stale_threshold_secs: u64,
        /// Parsed poll interval from start
        #[arg(
            long,
            default_value_t = 60,
            value_name = "SECONDS",
            value_parser = parse_positive_u64
        )]
        poll_interval_secs: u64,
        /// Maximum stale messages processed per poll cycle
        #[arg(long, default_value_t = 64, value_name = "COUNT", value_parser = parse_positive_usize)]
        cycle_cap: usize,
        /// Maximum rendered payload size in bytes
        #[arg(long, default_value_t = 1024, value_name = "BYTES", value_parser = parse_positive_usize)]
        payload_cap: usize,
        /// Log path for the supervisor process
        #[arg(long, value_name = "PATH")]
        log_path: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum MsgCommand {
    /// Write one message to an agent inbox
    #[command(
        long_about = "Write one message to an explicit agent inbox.\n\nUnscoped sends use the direct/default scope. Prefer `wt task report <message>` for TaskRun completion reports and `wt task review <task-run-id> --accept|--reject|--block <message>` for coordinator review feedback; use explicit `--scope workflow:<id>`, `--scope task_run:<id>`, or `--scope repo` only as low-level escape hatches."
    )]
    Send {
        /// Target agent id as NAME or agents/NAME
        #[arg(long)]
        to: String,
        /// Message ownership scope: direct, repo, workflow:<id>, or task_run:<id>
        #[arg(long)]
        scope: Option<String>,
        /// Message text
        #[arg(
            value_name = "MESSAGE",
            required = true,
            num_args = 1..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        message: Vec<String>,
    },
    /// List lifecycle messages for one agent inbox without claiming them
    List {
        /// Agent id as NAME or agents/NAME
        #[arg(long)]
        agent: String,
    },
    /// Read one lifecycle message by id without changing delivery state
    Read {
        /// Agent id as NAME or agents/NAME
        #[arg(long)]
        agent: String,
        /// Message id without the .toml extension
        message_id: String,
    },
    /// Claim deliverable inbox messages, emit hook JSON, and acknowledge delivery
    #[command(hide = true)]
    CheckInbox {
        /// Explicit single agent id as NAME or agents/NAME; omitted uses WT_AGENT_ID, then the current live identity anchor
        #[arg(long)]
        agent: Option<String>,
        /// Internal hook event name supplied by wt-managed hook templates; omitted preserves the compatible UserPromptSubmit default
        #[arg(long, hide = true)]
        hook_event_name: Option<String>,
        /// Hook mode: exit 0 silently when wt context cannot load (non-git CWD, legacy `.local/.wt.toml`, missing setup). Intended for agent hooks installed globally; direct CLI use should omit this flag.
        #[arg(long)]
        silent: bool,
    },
    /// Observe pending or newly-arriving inbox/new messages without claiming them
    #[command(
        long_about = "Observe pending or newly-arriving inbox/new messages for one agent without claiming, moving, or acknowledging them.\n\n`wt msg watch` arms a filesystem watcher, drains existing .toml messages in mtime order, and exits after emitting pending messages, one new arrival, or a timeout. Omitted --agent falls back to WT_AGENT_ID, then the current live identity anchor. Use --json for newline-delimited JSON rows with the same fields as `wt msg list --json` message records. Use `wt msg list` for a snapshot instead of --timeout 0."
    )]
    Watch {
        /// Explicit single agent id as NAME or agents/NAME; omitted uses WT_AGENT_ID, then the current live identity anchor
        #[arg(long)]
        agent: Option<String>,
        /// Maximum seconds to wait for a new inbox/new message; must be greater than 0
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        /// Emit newline-delimited JSON message rows
        #[arg(long)]
        json: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum RunCommand {
    /// Start a workspace from an issue
    Issue {
        /// Issue numbers or provider-specific keys (omit to select multiple provider issues)
        #[arg(value_name = "ISSUE")]
        targets: Vec<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled issue worktree from <repo-root>/.wt/config/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each named profile
        #[arg(long, conflicts_with = "profile")]
        matrix: bool,
        /// Maximum number of provider issues to execute concurrently
        #[arg(long, default_value_t = 3, value_parser = parse_positive_usize)]
        jobs: usize,
    },
    /// Start workspaces from pull requests
    Pr {
        /// Pull request numbers (omit to select multiple open PRs)
        #[arg(value_name = "PR")]
        numbers: Vec<u32>,
        /// Apply config from <repo-root>/.wt/config/profiles/<name> to the PR worktree
        #[arg(long)]
        profile: Option<String>,
        /// Maximum number of pull requests to execute concurrently
        #[arg(long, default_value_t = 3, value_parser = parse_positive_usize)]
        jobs: usize,
    },
    /// Start a workspace from branch-name text
    #[command(
        long_about = "Start one ad hoc workspace from branch-name text by creating a new local branch and worktree.\n\nThis does not open an existing branch or worktree. Use `wt open <branch|worktree>` for existing work."
    )]
    Branch {
        /// Branch name words
        #[arg(num_args = 0..)]
        name: Vec<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled branch worktree from <repo-root>/.wt/config/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each named profile
        #[arg(long, conflicts_with = "profile")]
        matrix: bool,
    },
    /// Start one worktree per selected local TaskDocument
    #[command(
        long_about = "Start one worktree per selected <repo-root>/.wt/execution/tasks/<task>.toml TaskDocument and record each attempt as a direct TaskRun in <repo-root>/.wt/execution/task-runs.\n\nPass explicit task keys for scripts. Omit task keys to choose local TaskDocuments interactively.\n\nEvery started task prompt leads with `wt task report \"Agent Completion Report: ...\"` and includes fallback cmux send coordinates. Task-run agents report PR=none and wait for the coordinator to review, land, and clean up explicitly.\n\nUse `wt workflow task --mode batch` and `wt run workflow` when multiple independent TaskDocuments need saved batch coordination. Use `wt workflow task --mode single` and `wt run workflow` when multiple TaskDocuments should share one workspace."
    )]
    Task {
        /// Local task keys from <repo-root>/.wt/execution/tasks/<task>.toml
        #[arg(value_name = "TASK")]
        tasks: Vec<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled task worktree from <repo-root>/.wt/config/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Maximum number of local tasks to execute concurrently
        #[arg(long, default_value_t = 3, value_parser = parse_positive_usize)]
        jobs: usize,
    },
    /// Start runnable tasks from a saved workflow
    #[command(
        long_about = "Start runnable tasks from a saved workflow.\n\nOmit WORKFLOW to choose from runnable workflows. A runnable workflow has prepared or failed TaskRuns that can still be started: single mode requires all linked TaskRuns to be prepared or failed, batch mode requires at least one prepared or failed task, and stack mode requires a next prepared or failed task with no running task. Passing WORKFLOW accepts a TOML path or shorthand id for scripts. In non-interactive shells, pass WORKFLOW explicitly.\n\nThis does not list, edit, repair, or pass workflow tasks; those lifecycle actions stay under `wt workflow`.\n\nEvery started task prompt includes a Workflow Coordinator Handoff using `wt task report \"Agent Completion Report: ...\"` with workflow scope derived from the TaskRun, plus fallback cmux send coordinates. All workflow modes use the prepared [policy].pull_request value for PR reporting and pull-request creation, the prepared [policy.review].codex_base value for Codex base-diff review evidence, and include their `wt workflow pass ...` command. Stack prompts include `--run-next`."
    )]
    Workflow {
        /// Workflow TOML path or shorthand id (omit to select a runnable workflow)
        workflow: Option<String>,
        /// Maximum number of runnable batch-mode tasks to execute concurrently
        #[arg(long, default_value_t = 1, value_parser = parse_positive_usize)]
        jobs: usize,
    },
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum InitAgent {
    Codex,
    Claude,
    Gemini,
    None,
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum InitIssueProvider {
    Github,
    Linear,
    None,
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq)]
pub enum InitSiteProvider {
    None,
    Herd,
    Valet,
    #[value(name = "docker_proxy")]
    DockerProxy,
    Traefik,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum TaskCommand {
    /// List actionable local TaskDocument files
    #[command(
        long_about = "List actionable <repo-root>/.wt/execution/tasks/<task>.toml TaskDocument files by default.\n\nThe default working set uses the same selectability rules as wt run task: tasks with no TaskRun, or whose latest TaskRun status is prepared, failed, or skipped. Tasks whose latest TaskRun status is passed or running are hidden with a count hint. Use --all to show the full read-only TaskDocument inventory.\n\nEach mode reports invalid TaskDocument TOML files instead of hiding them, and does not start workspaces, create local branches, create TaskRuns, prepare workflows, publish provider issues, open pull requests, or run agent setup."
    )]
    List {
        /// Show the full TaskDocument inventory, including passed and running tasks
        #[arg(long)]
        all: bool,
    },
    /// Import provider issues as local TaskDocuments
    #[command(
        long_about = "Import existing provider issues into <repo-root>/.wt/execution/tasks/<safe-issue-id>.toml TaskDocuments, materialize the provider issue branch when needed, and write title, branch, body, and [origin] with the configured provider and issue id. This command does not start workspaces, create local branches, create TaskRuns, prepare workflows, open pull requests, or run agent setup.\n\nFor GitHub, materializing a missing provider issue branch may call gh issue develop. Import fails instead of writing a TaskDocument with an empty branch.\n\nPass explicit issue ids for scripts. Omit issue ids to choose provider issues interactively.\n\nFails before writing when no issue provider is configured, duplicate issue ids are passed, or an imported issue would overwrite an existing local TaskDocument."
    )]
    Import {
        /// Provider issue ids to import
        #[arg(value_name = "ISSUE")]
        issues: Vec<String>,
    },
    #[command(name = "run", hide = true, disable_help_flag = true)]
    DeprecatedRun {
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    /// Publish local TaskDocuments as provider issues
    #[command(
        long_about = "Create provider issues from selected <repo-root>/.wt/execution/tasks/<task>.toml files, then rewrite branch to a provider-keyed branch and write [origin] with the configured provider and created issue id. This command does not start workspaces, create local branches, create TaskRuns, or run workflow work.\n\nAfter branch and [origin] are written, later wt run task and wt run workflow treat that TaskDocument as provider-origin issue work.\n\nPass explicit task keys for scripts. Omit task keys to choose unprocessed local TaskDocuments interactively; tasks that already have [origin] are excluded from that selector.\n\nFails before creating an issue for an explicit task when no issue provider is configured, the task is missing or invalid, the task already has origin, the task has an empty title, or rewriting the old branch would be unsafe because it already has a TaskRun, checked-out worktree, local branch, or remote branch."
    )]
    Publish {
        /// Local task keys from <repo-root>/.wt/execution/tasks/<task>.toml
        #[arg(value_name = "TASK")]
        tasks: Vec<String>,
    },
    /// Send the current TaskRun completion report to its recorded coordinator
    #[command(
        long_about = "Send a report from the current TaskRun to the coordinator recorded on that TaskRun.\n\nThe normal task-agent path is `wt task report \"Agent Completion Report: Summary=<summary>; Changed files=<files>; Checks run=<checks>; PR=none; Risks or follow-ups=<risks>\"`. When WT_TASK_RUN_ID is set, wt uses that exact TaskRun when it is running or passed. Without WT_TASK_RUN_ID, wt may fall back to the current branch only when exactly one running or passed TaskRun matches. Workflow-linked TaskRuns use their workflow scope automatically. Reports are sent through the file inbox and update TaskRun report metadata."
    )]
    Report {
        /// Report message to send
        #[arg(value_name = "MESSAGE", num_args = 1..)]
        message: Vec<String>,
    },
    /// Send coordinator review feedback to a TaskRun agent
    #[command(
        long_about = "Send coordinator review feedback to the task agent recorded on a TaskRun.\n\nUse `wt task review <task-run-id> --accept <message>` to accept a report, `--reject` to request changes, or `--block` when the task cannot proceed. Feedback is sent through the file inbox to TaskRun.agent_id with task_run:<id> scope and updates TaskRun review metadata. Rejecting or blocking a passed TaskRun reopens it to running; accepting records metadata only and does not pass a running TaskRun.",
        group(ArgGroup::new("review_status").required(true).args(["accept", "reject", "block"]))
    )]
    Review {
        /// TaskRun id to review
        #[arg(value_name = "TASK_RUN_ID")]
        task_run_id: String,
        /// Accept the TaskRun report
        #[arg(long)]
        accept: bool,
        /// Reject the TaskRun report and ask for changes
        #[arg(long)]
        reject: bool,
        /// Block the TaskRun on missing input or external state
        #[arg(long)]
        block: bool,
        /// Review feedback message to send
        #[arg(value_name = "MESSAGE", num_args = 1..)]
        message: Vec<String>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum WorkflowCommand {
    /// List saved workflow files
    #[command(
        long_about = "List all saved <repo-root>/.wt/execution/workflows/<id>.toml Workflow files.\n\nThis is the canonical read-only inventory for saved workflows. It lists valid Workflow files whether or not they are currently runnable, reports invalid workflow TOML files instead of hiding them, and exposes runnable as derived metadata from linked TaskRuns. Human text output groups workflows under derived action labels such as runnable, waiting, and passed, with indented rows and secondary detail lines."
    )]
    List,
    /// Move passed workflow state into the frozen archive
    #[command(
        long_about = "Move a passed Workflow out of the active surface into <repo-root>/.wt/execution/archive/workflows/<workflow-id>/.\n\nArchive is a visibility and retention action: wt workflow list, wt task list, and wt ui stop showing the archived workflow because active inventory reads only typed active directories. It is not a substitute for landing, merge checks, wt workflow pass, or wt done. Only workflows whose linked TaskRuns are passed or skipped can be archived."
    )]
    Archive {
        /// Workflow key under <repo-root>/.wt/execution/workflows/<workflow>.toml
        workflow: String,
    },
    /// Prepare local tasks as a workflow file without starting workspaces
    #[command(
        long_about = "Prepare local TaskDocuments as a saved Workflow without starting workspaces.\n\nUse --title, --body/--body-file, and --origin-provider with --origin-id for Workflow-level context when one larger issue-like unit is split into runnable child TaskDocuments. Workflow-level [origin] is stored only on the Workflow; it is not copied into child TaskDocuments and does not add issue-closing keywords to child PR bodies.\n\nTaskDocument [origin] still belongs only to a runnable slice that is itself a provider issue."
    )]
    Task {
        /// Task titles or existing task keys to prepare (omit to select multiple existing tasks)
        tasks: Vec<String>,
        /// Workflow execution shape
        #[arg(long, value_enum)]
        mode: WorkflowModeArg,
        /// Named profile from <repo-root>/.wt/config/profiles/<name> for all tasks
        #[arg(long, conflicts_with = "profiles")]
        profile: Option<String>,
        /// With --mode matrix, selected named profiles to run in order
        #[arg(long, value_name = "PROFILE", value_delimiter = ',')]
        profiles: Vec<String>,
        /// Short workflow title for list, select, and show surfaces
        #[arg(long)]
        title: Option<String>,
        /// Long workflow body with larger context and requirements
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read the workflow body from a file
        #[arg(long = "body-file", value_name = "PATH", conflicts_with = "body")]
        body_file: Option<PathBuf>,
        /// Provider for the workflow-level origin link
        #[arg(long = "origin-provider", requires = "origin_id")]
        origin_provider: Option<String>,
        /// Provider issue id for the workflow-level origin link
        #[arg(long = "origin-id", requires = "origin_provider")]
        origin_id: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Override [workflow].pull_request for this prepared workflow
        #[arg(long = "pr", value_enum, value_name = "none|draft|ready")]
        pr: Option<WorkflowPrModeArg>,
    },
    /// Prepare issues as a workflow file without starting workspaces
    #[command(
        long_about = "Prepare provider issues as a saved Workflow without starting workspaces.\n\nEach selected provider issue becomes an executable child TaskDocument, and that TaskDocument records [origin] for the selected issue. Selected issue ids are not automatically lifted into Workflow [origin]. Use --origin-provider with --origin-id only when the Workflow itself has a separate larger provider source.\n\nWorkflow-level [origin] is stored only on the Workflow; it is not copied into child TaskDocuments and does not add issue-closing keywords to child PR bodies."
    )]
    Issue {
        /// Issue identifiers to import as tasks (omit to select interactively)
        issues: Vec<String>,
        /// Workflow execution shape
        #[arg(long, value_enum)]
        mode: WorkflowModeArg,
        /// Named profile from <repo-root>/.wt/config/profiles/<name> for all tasks
        #[arg(long)]
        profile: Option<String>,
        /// Short workflow title for list, select, and show surfaces
        #[arg(long)]
        title: Option<String>,
        /// Long workflow body with larger context and requirements
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Read the workflow body from a file
        #[arg(long = "body-file", value_name = "PATH", conflicts_with = "body")]
        body_file: Option<PathBuf>,
        /// Provider for the workflow-level origin link
        #[arg(long = "origin-provider", requires = "origin_id")]
        origin_provider: Option<String>,
        /// Provider issue id for the workflow-level origin link
        #[arg(long = "origin-id", requires = "origin_provider")]
        origin_id: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Override [workflow].pull_request for this prepared workflow
        #[arg(long = "pr", value_enum, value_name = "none|draft|ready")]
        pr: Option<WorkflowPrModeArg>,
    },
    #[command(name = "run", hide = true, disable_help_flag = true)]
    DeprecatedRun {
        #[arg(
            value_name = "ARGS",
            num_args = 0..,
            trailing_var_arg = true,
            allow_hyphen_values = true
        )]
        args: Vec<String>,
    },
    /// Show workflow metadata and task statuses
    #[command(
        long_about = "Show one saved <repo-root>/.wt/execution/workflows/<id>.toml Workflow file with its prepared policy snapshot and linked TaskRun statuses.\n\nHuman output preserves the compact meta section plus numbered task rows. Use global --json for the one-shot machine-readable observation surface: path, mode, base, title, pull_request, landing, review.codex_base, and tasks with order, task, status, branch, parent, and title. This command is read-only and its exit code means command success or failure only."
    )]
    Show {
        /// Workflow TOML path, shorthand id, or "latest" (default)
        workflow: Option<String>,
    },
    /// Block until every workflow task reaches a terminal state
    #[command(
        long_about = "Poll one saved Workflow until every linked TaskRun is terminal: passed, failed, or skipped.\n\nThis is a workflow-level durable terminal block over <repo-root>/.wt/execution/workflows/<id>.toml and linked TaskRuns. It is separate from `wt agent watch`, which observes one task agent's Layer 2 runtime state from cmux. `wt workflow watch` reuses the agent watch exit-code contract for workflow status: 0 for all passed/skipped or timeout while still non-terminal, 1 for unavailable workflow state, and 3 when any terminal task failed. It does not write <repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl. Human output is transition-only by default; use --heartbeat for unchanged waiting output. Use global --json to print the final workflow show JSON snapshot on exit. Omit WORKFLOW in an interactive terminal to choose an observable workflow; pass WORKFLOW explicitly for scripts, --json, --quiet, and non-interactive use."
    )]
    Watch {
        /// Workflow TOML path or shorthand id to watch
        workflow: Option<String>,
        /// Seconds between workflow observations
        #[arg(
            long,
            default_value_t = 2,
            value_name = "SECONDS",
            value_parser = parse_positive_u64
        )]
        interval: u64,
        /// Stop waiting after this many positive seconds
        #[arg(long, value_name = "SECONDS", value_parser = parse_positive_u64)]
        timeout: Option<u64>,
        /// Print unchanged workflow observations at this positive-second interval
        #[arg(long, value_name = "SECONDS", value_parser = parse_positive_u64)]
        heartbeat: Option<u64>,
    },
    /// Open workflow TOML in the configured editor
    Edit {
        /// Workflow TOML path, shorthand id, or "latest" (default)
        workflow: Option<String>,
    },
    /// Preview or apply workflow runtime repairs
    #[command(
        long_about = "Preview or apply workflow runtime repairs.\n\nBy default this command is a dry-run: it observes linked TaskRuns, local worktrees, and live cmux agent surfaces, then prints recommended repairs without changing state. Pass --apply to mark repairable inconsistent TaskRuns failed through the existing TaskRun failure model. Repair never closes cmux workspaces or removes worktrees."
    )]
    Repair {
        /// Workflow TOML path or shorthand id
        workflow: String,
        /// Apply repairable TaskRun status changes
        #[arg(long)]
        apply: bool,
    },
    /// Mark running workflow TaskRuns passed
    Pass {
        /// Workflow TOML path or shorthand id
        workflow: String,
        /// Running workflow task identifier to pass
        task: Option<String>,
        /// Start the next stack-mode workflow task after marking this one passed
        #[arg(long)]
        run_next: bool,
    },
    /// Legacy migration surface for wt workflow pass
    #[command(hide = true)]
    Complete {
        /// Workflow TOML path or shorthand id
        workflow: String,
        /// Running workflow task identifier to pass
        task: Option<String>,
        /// Start the next stack-mode workflow task after marking this one passed
        #[arg(long)]
        run_next: bool,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowModeArg {
    Single,
    Batch,
    Stack,
    Matrix,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowPrModeArg {
    None,
    Draft,
    Ready,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum SiteCommand {
    /// Check the configured local site provider setup
    Doctor,
    /// Print provider-managed site paths and defaults
    Paths,
    /// Print an example macOS LaunchDaemon plist for provider = "traefik"
    ExampleLaunchd {
        /// LaunchDaemon label to render
        #[arg(long, default_value = "wt.traefik")]
        label: String,
        /// IP address Traefik should bind for wt sites
        #[arg(long, default_value = "127.0.0.2")]
        bind_ip: String,
    },
}

/// Parsed base branch mode, derived from the raw --base flag.
#[derive(Debug, PartialEq)]
pub enum BaseMode {
    /// No --base flag: prompt user to confirm current branch
    Default,
    /// --base with no value: interactive branch selector
    Interactive,
    /// --base .: use current branch without prompting
    Current,
    /// --base <branch>: use the explicit branch
    Explicit(String),
}

impl BaseMode {
    pub fn from_raw(raw: &Option<String>) -> Self {
        match raw {
            None => BaseMode::Default,
            Some(s) if s.is_empty() => BaseMode::Interactive,
            Some(s) if s == "." => BaseMode::Current,
            Some(s) => BaseMode::Explicit(s.clone()),
        }
    }
}

fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if parsed == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(parsed)
}

fn parse_positive_u64(value: &str) -> std::result::Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| "must be a positive integer".to_string())?;
    if parsed == 0 {
        return Err("must be a positive integer".into());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }

    fn root_help_section<'a>(help: &'a str, heading: &str, next_heading: &str) -> &'a str {
        let start_marker = format!("{heading}:\n");
        let start = help
            .find(&start_marker)
            .unwrap_or_else(|| panic!("missing heading {heading}"))
            + start_marker.len();
        if next_heading.is_empty() {
            return &help[start..];
        }
        let end_marker = format!("\n\n{next_heading}:\n");
        let end = help[start..]
            .find(&end_marker)
            .unwrap_or_else(|| panic!("missing next heading {next_heading}"))
            + start;
        &help[start..end]
    }

    fn assert_section_contains_command(section: &str, command: &str) {
        let prefix = format!("{command} ");
        assert!(
            section
                .lines()
                .any(|line| line.trim_start().starts_with(&prefix)),
            "expected section to contain command {command:?}; section was:\n{section}"
        );
    }

    #[test]
    fn no_subcommand_is_allowed_for_manual_help_handling() {
        let cli = parse(&["wt"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn version_subcommand() {
        let cli = parse(&["wt", "version"]);
        assert!(matches!(cli.command, Some(Commands::Version)));
    }

    #[test]
    fn completion_subcommand() {
        let cli = parse(&["wt", "completion", "bash"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Completion {
                shell: clap_complete::Shell::Bash
            })
        ));
    }

    #[test]
    fn global_options_parse_before_subcommand() {
        let cli = parse(&[
            "wt",
            "-C",
            "/tmp/repo",
            "--config",
            "/tmp/wt.toml",
            "--color",
            "always",
            "-vv",
            "doctor",
        ]);
        assert_eq!(
            cli.directory.as_deref(),
            Some(std::path::Path::new("/tmp/repo"))
        );
        assert_eq!(
            cli.config.as_deref(),
            Some(std::path::Path::new("/tmp/wt.toml"))
        );
        assert_eq!(cli.color, ColorMode::Always);
        assert_eq!(cli.verbose, 2);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor {
                profile: None,
                prune_env_anchors: None
            })
        ));
    }

    #[test]
    fn no_color_flag() {
        let cli = parse(&["wt", "--no-color", "doctor"]);
        assert!(cli.no_color);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor {
                profile: None,
                prune_env_anchors: None
            })
        ));
    }

    #[test]
    fn run_issue_no_args_starts_interactive_issue_flow() {
        let cli = parse(&["wt", "run", "issue"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Issue {
                    ref targets,
                    base: None,
                    profile: None,
                    matrix: false,
                    jobs: 3,
                }
            }) if targets.is_empty()
        ));
    }

    #[test]
    fn run_issue_with_target() {
        let cli = parse(&["wt", "run", "issue", "PROJ-680"]);
        if let Some(Commands::Run {
            command:
                RunCommand::Issue {
                    targets,
                    base,
                    profile,
                    matrix,
                    jobs,
                },
        }) = cli.command
        {
            assert_eq!(targets, vec!["PROJ-680".to_string()]);
            assert_eq!(base, None);
            assert_eq!(profile, None);
            assert!(!matrix);
            assert_eq!(jobs, 3);
        } else {
            panic!("expected Issue");
        }
    }

    #[test]
    fn run_issue_with_multiple_targets() {
        let cli = parse(&["wt", "run", "issue", "PROJ-680", "PROJ-681"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Issue {
                    ref targets,
                    base: None,
                    profile: None,
                    matrix: false,
                    jobs: 3,
                }
            }) if targets == &vec!["PROJ-680".to_string(), "PROJ-681".to_string()]
        ));
    }

    #[test]
    fn run_issue_accepts_jobs() {
        let cli = parse(&["wt", "run", "issue", "PROJ-680", "PROJ-681", "--jobs", "1"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Issue {
                    ref targets,
                    jobs: 1,
                    ..
                }
            }) if targets == &vec!["PROJ-680".to_string(), "PROJ-681".to_string()]
        ));
    }

    #[test]
    fn run_issue_with_base_interactive() {
        let cli = parse(&["wt", "run", "issue", "--base"]);
        if let Some(Commands::Run {
            command: RunCommand::Issue { base, .. },
        }) = &cli.command
        {
            assert_eq!(BaseMode::from_raw(base), BaseMode::Interactive);
        } else {
            panic!("expected Issue");
        }
    }

    #[test]
    fn run_issue_with_matrix_flag() {
        let cli = parse(&["wt", "run", "issue", "680", "--matrix"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Issue { matrix: true, .. }
            })
        ));
    }

    #[test]
    fn run_issue_with_profile_flag() {
        let cli = parse(&["wt", "run", "issue", "680", "--profile", "codex-yolo"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Issue {
                    profile: Some(ref profile),
                    ..
                }
            }) if profile == "codex-yolo"
        ));
    }

    #[test]
    fn run_issue_rejects_matrix_with_profile() {
        let result = Cli::try_parse_from([
            "wt",
            "run",
            "issue",
            "680",
            "--matrix",
            "--profile",
            "codex",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_issue_rejects_profiles_flag() {
        let result = Cli::try_parse_from(["wt", "run", "issue", "680", "--profiles", "alpha,beta"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_issue_rejects_profiles_with_profile() {
        let result = Cli::try_parse_from([
            "wt",
            "run",
            "issue",
            "680",
            "--matrix",
            "--profiles",
            "alpha",
            "--profile",
            "codex",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_issue_rejects_removed_parallel_flag() {
        let result = Cli::try_parse_from(["wt", "run", "issue", "680", "--parallel"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_pr_no_args_starts_interactive_pr_flow() {
        let cli = parse(&["wt", "run", "pr"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Pr {
                    ref numbers,
                    profile: None,
                    jobs: 3,
                }
            }) if numbers.is_empty()
        ));
    }

    #[test]
    fn run_pr_with_number_and_profile() {
        let cli = parse(&["wt", "run", "pr", "42", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Pr {
                    ref numbers,
                    profile: Some(ref profile),
                    jobs: 3,
                }
            }) if numbers == &vec![42] && profile == "codex"
        ));
    }

    #[test]
    fn run_pr_with_multiple_numbers_and_profile() {
        let cli = parse(&["wt", "run", "pr", "42", "43", "44", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Pr {
                    ref numbers,
                    profile: Some(ref profile),
                    jobs: 3,
                }
            }) if numbers == &vec![42, 43, 44] && profile == "codex"
        ));
    }

    #[test]
    fn run_pr_accepts_jobs() {
        let cli = parse(&["wt", "run", "pr", "42", "43", "--jobs", "1"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Pr {
                    ref numbers,
                    jobs: 1,
                    ..
                }
            }) if numbers == &vec![42, 43]
        ));
    }

    #[test]
    fn run_pr_help_describes_multiple_targets() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("run")
            .unwrap()
            .find_subcommand_mut("pr")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("[PR]..."));
        assert!(help.contains("Pull request numbers"));
        assert!(help.contains("select multiple open PRs"));
        assert!(help.contains("--jobs"));
    }

    #[test]
    fn run_issue_help_describes_multiple_targets() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("run")
            .unwrap()
            .find_subcommand_mut("issue")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("[ISSUE]..."));
        assert!(help.contains("Issue numbers or provider-specific keys"));
        assert!(help.contains("select multiple provider issues"));
        assert!(help.contains("--jobs"));
    }

    #[test]
    fn inspect_accepts_optional_target() {
        let cli = parse(&["wt", "inspect"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Inspect {
                target: None,
                pr: false
            })
        ));

        let cli = parse(&["wt", "inspect", "feature"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Inspect {
                ref target,
                pr: false
            }) if target.as_deref() == Some("feature")
        ));

        let cli = parse(&["wt", "inspect", "feature", "--pr"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Inspect {
                ref target,
                pr: true
            }) if target.as_deref() == Some("feature")
        ));
    }

    #[test]
    fn inspect_help_describes_optional_target_and_selector() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("inspect")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("[TARGET]"));
        assert!(help.contains("read-only work dossier"));
        assert!(help.contains("Branch, worktree path/name, or TaskRun id"));
        assert!(help.contains("Omit TARGET in an interactive terminal"));
        assert!(help.contains("--pr"));
        assert!(help.contains("pull request review evidence"));
    }

    #[test]
    fn agent_status_accepts_optional_target() {
        let cli = parse(&["wt", "agent", "status", "feature"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent {
                command: AgentCommand::Status { ref target }
            }) if target.as_deref() == Some("feature")
        ));

        let cli = parse(&["wt", "agent", "status"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent {
                command: AgentCommand::Status { target: None }
            })
        ));
    }

    #[test]
    fn agent_status_help_describes_target_types_and_selector() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("agent")
            .unwrap()
            .find_subcommand_mut("status")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("task agent"));
        assert!(help.contains("[TARGET]"));
        assert!(help.contains("Branch, worktree path/name, or TaskRun id"));
        assert!(help.contains("Omit TARGET in an interactive terminal"));
    }

    #[test]
    fn agent_watch_help_describes_bounds_and_heartbeat() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("agent")
            .unwrap()
            .find_subcommand_mut("watch")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("task agent"));
        assert!(help.contains("[TARGET]"));
        assert!(help.contains("--timeout"));
        assert!(help.contains("--heartbeat"));
        assert!(!help.contains("--record-wait-observations"));
        assert!(help.contains(
            "<repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl"
        ));
        assert!(help.contains("When --timeout or --heartbeat emits a non-idle sample"));
        assert!(help.contains("unchanged running observations"));
        assert!(help.contains("Omit TARGET in an interactive terminal"));
    }

    #[test]
    fn agent_wait_stats_help_describes_read_only_summary() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("agent")
            .unwrap()
            .find_subcommand_mut("wait-stats")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("non-idle wait observations"));
        assert!(help.contains("read-only"));
        assert!(help.contains(
            "<repo-root>/.wt/runtime/agents/<agent>/observations/wait-observations.jsonl"
        ));
        assert!(help.contains("does not observe agents"));
        assert!(help.contains("mutate TaskRuns"));
    }

    #[test]
    fn agent_watch_accepts_optional_target_and_interval() {
        let cli = parse(&[
            "wt",
            "agent",
            "watch",
            "feature",
            "--interval",
            "5",
            "--timeout",
            "60",
            "--heartbeat",
            "10",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent {
                command: AgentCommand::Watch {
                    ref target,
                    interval: 5,
                    timeout: Some(60),
                    heartbeat: Some(10),
                }
            }) if target.as_deref() == Some("feature")
        ));

        let cli = parse(&["wt", "agent", "watch", "feature", "--heartbeat", "10"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent {
                command: AgentCommand::Watch {
                    ref target,
                    interval: 2,
                    timeout: None,
                    heartbeat: Some(10),
                }
            }) if target.as_deref() == Some("feature")
        ));

        let cli = parse(&["wt", "agent", "watch"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent {
                command: AgentCommand::Watch {
                    target: None,
                    interval: 2,
                    timeout: None,
                    heartbeat: None,
                }
            })
        ));
    }

    #[test]
    fn agent_watch_rejects_zero_timeout_and_heartbeat() {
        let interval = Cli::try_parse_from(["wt", "agent", "watch", "feature", "--interval", "0"]);
        assert!(interval.is_err());

        let timeout = Cli::try_parse_from(["wt", "agent", "watch", "feature", "--timeout", "0"]);
        assert!(timeout.is_err());

        let heartbeat =
            Cli::try_parse_from(["wt", "agent", "watch", "feature", "--heartbeat", "0"]);
        assert!(heartbeat.is_err());
    }

    #[test]
    fn agent_wait_stats_parses_read_only_subcommand() {
        let cli = parse(&["wt", "agent", "wait-stats"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Agent {
                command: AgentCommand::WaitStats
            })
        ));
    }

    #[test]
    fn send_accepts_target_and_message() {
        let cli = parse(&["wt", "send", "feature", "hello", "agent"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Send {
                ref target,
                ref message,
                no_enter: false,
            }) if target == "feature" && message == &vec!["hello".to_string(), "agent".to_string()]
        ));
    }

    #[test]
    fn send_accepts_no_enter() {
        let cli = parse(&["wt", "send", "feature", "--no-enter", "draft"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Send {
                ref target,
                ref message,
                no_enter: true,
            }) if target == "feature" && message == &vec!["draft".to_string()]
        ));
    }

    #[test]
    fn send_help_describes_target_and_message() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("send")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("task agent"));
        assert!(help.contains("<TARGET>"));
        assert!(help.contains("<MESSAGE>"));
        assert!(help.contains("--no-enter"));
    }

    #[test]
    fn task_import_accepts_issue_id() {
        let cli = parse(&["wt", "task", "import", "PROJ-123"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::Import { ref issues }
            }) if issues == &vec!["PROJ-123".to_string()]
        ));
    }

    #[test]
    fn task_import_accepts_multiple_issue_ids() {
        let cli = parse(&["wt", "task", "import", "PROJ-123", "#42"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::Import { ref issues }
            }) if issues == &vec!["PROJ-123".to_string(), "#42".to_string()]
        ));
    }

    #[test]
    fn task_import_accepts_no_issue_ids_for_interactive_selection() {
        let cli = parse(&["wt", "task", "import"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::Import { ref issues }
            }) if issues.is_empty()
        ));
    }

    #[test]
    fn task_import_rejects_stack_selector() {
        let err = Cli::try_parse_from(["wt", "task", "import", "--stack", "latest"])
            .unwrap_err()
            .to_string();
        assert!(err.contains("unexpected argument '--stack'"));
    }

    #[test]
    fn task_import_help_explains_non_executing_import_and_failures() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("task")
            .unwrap()
            .find_subcommand_mut("import")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("Import existing provider issues"));
        assert!(help.contains("<repo-root>/.wt/execution/tasks/<safe-issue-id>.toml"));
        assert!(help.contains("write title, branch, body, and [origin]"));
        assert!(help.contains("does not start workspaces"));
        assert!(help.contains("create local branches"));
        assert!(help.contains("create TaskRuns"));
        assert!(help.contains("gh issue develop"));
        assert!(help.contains("empty branch"));
        assert!(help.contains("Omit issue ids to choose provider issues interactively"));
        assert!(help.contains("duplicate issue ids"));
        assert!(help.contains("overwrite an existing local TaskDocument"));
        assert!(!help.contains("--stack <STACK>"));
        assert!(!help.contains("--batch <BATCH>"));
    }

    #[test]
    fn task_publish_accepts_task_key() {
        let cli = parse(&["wt", "task", "publish", "add-profile-docs"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::Publish { ref tasks }
            }) if tasks == &vec!["add-profile-docs".to_string()]
        ));
    }

    #[test]
    fn task_publish_accepts_multiple_task_keys() {
        let cli = parse(&["wt", "task", "publish", "task-a", "task-b"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::Publish { ref tasks }
            }) if tasks == &vec!["task-a".to_string(), "task-b".to_string()]
        ));
    }

    #[test]
    fn task_publish_accepts_no_task_keys_for_interactive_selection() {
        let cli = parse(&["wt", "task", "publish"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::Publish { ref tasks }
            }) if tasks.is_empty()
        ));
    }

    #[test]
    fn task_publish_rejects_stack_selector() {
        let err = Cli::try_parse_from(["wt", "task", "publish", "--stack", "latest"])
            .unwrap_err()
            .to_string();
        assert!(err.contains("unexpected argument '--stack'"));
    }

    #[test]
    fn task_publish_help_explains_side_effects_and_failures() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("task")
            .unwrap()
            .find_subcommand_mut("publish")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("provider issue"));
        assert!(help.contains("write [origin]"));
        assert!(help.contains("rewrite branch to a provider-keyed branch"));
        assert!(help.contains("does not start workspaces"));
        assert!(help.contains("create local branches"));
        assert!(help.contains("later wt run task and wt run workflow"));
        assert!(help.contains("Omit task keys to choose unprocessed local TaskDocuments"));
        assert!(help.contains("tasks that already have [origin] are excluded"));
        assert!(!help.contains("--stack <STACK>"));
        assert!(!help.contains("--batch <BATCH>"));
        assert!(help.contains("no issue provider"));
        assert!(help.contains("already has origin"));
        assert!(help.contains("checked-out worktree"));
    }

    #[test]
    fn scaffold_accepts_feature_and_kind_flags() {
        let cli = parse(&["wt", "scaffold", "foo", "--idea", "--task"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Scaffold {
                ref feature,
                idea: true,
                spec: false,
                task: true,
                workflow: false,
                retrospect: false,
                all: false,
                force: false,
            }) if feature == "foo"
        ));
    }

    #[test]
    fn scaffold_rejects_all_with_kind_flags() {
        let result = Cli::try_parse_from(["wt", "scaffold", "foo", "--all", "--idea"]);
        assert!(result.is_err());
    }

    #[test]
    fn scaffold_rejects_missing_feature() {
        let result = Cli::try_parse_from(["wt", "scaffold", "--idea"]);
        assert!(result.is_err());
    }

    #[test]
    fn scaffold_help_exposes_document_kind_flags() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("scaffold")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("Create blank skeleton documents for a feature"));
        assert!(help.contains("<FEATURE>"));
        assert!(help.contains("--idea"));
        assert!(help.contains("--spec"));
        assert!(help.contains("--task"));
        assert!(help.contains("--workflow"));
        assert!(help.contains("--retrospect"));
        assert!(help.contains("--all"));
        assert!(help.contains("--force"));
    }

    #[test]
    fn run_task_accepts_task_keys_base_and_profile() {
        let cli = parse(&["wt", "run", "task", "task-a", "task-b", "--base", "main"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Task {
                    ref tasks,
                    base: Some(ref base),
                    profile: None,
                    jobs: 3,
                }
            }) if tasks == &vec!["task-a".to_string(), "task-b".to_string()]
                && base == "main"
        ));

        let cli = parse(&["wt", "run", "task", "task-a", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Task {
                    ref tasks,
                    profile: Some(ref profile),
                    jobs: 3,
                    ..
                }
            }) if tasks == &vec!["task-a".to_string()] && profile == "codex"
        ));
    }

    #[test]
    fn run_task_accepts_no_task_keys_for_interactive_selection() {
        let cli = parse(&["wt", "run", "task"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Task {
                    ref tasks,
                    base: None,
                    profile: None,
                    jobs: 3,
                }
            }) if tasks.is_empty()
        ));
    }

    #[test]
    fn run_task_accepts_jobs() {
        let cli = parse(&["wt", "run", "task", "task-a", "task-b", "--jobs", "1"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Task {
                    ref tasks,
                    jobs: 1,
                    ..
                }
            }) if tasks == &vec!["task-a".to_string(), "task-b".to_string()]
        ));
    }

    #[test]
    fn run_task_rejects_matrix() {
        let result = Cli::try_parse_from(["wt", "run", "task", "task-a", "--matrix"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_task_rejects_profiles_without_matrix() {
        let result =
            Cli::try_parse_from(["wt", "run", "task", "task-a", "--profiles", "alpha,beta"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_task_rejects_profiles_with_profile() {
        let result = Cli::try_parse_from([
            "wt",
            "run",
            "task",
            "task-a",
            "--profiles",
            "alpha",
            "--profile",
            "codex",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_task_help_explains_task_execution_surface() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("run")
            .unwrap()
            .find_subcommand_mut("task")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("one worktree per selected"));
        assert!(help.contains("direct TaskRun"));
        assert!(help.contains("Omit task keys"));
        assert!(help.contains("wt task report"));
        assert!(help.contains("Agent Completion Report"));
        assert!(help.contains("fallback cmux send coordinates"));
        assert!(help.contains("Task-run agents report PR=none"));
        assert!(help.contains("wt workflow task --mode batch"));
        assert!(help.contains("wt workflow task --mode single"));
        assert!(help.contains("wt run workflow"));
        assert!(help.contains("--jobs"));
        assert!(!help.contains("--matrix"));
        assert!(!help.contains("--profiles"));
    }

    #[test]
    fn workflow_task_requires_mode() {
        let err = Cli::try_parse_from(["wt", "workflow", "task", "add-schema"]).unwrap_err();
        assert!(err.to_string().contains("--mode <MODE>"));
    }

    #[test]
    fn workflow_task_accepts_mode_profile_and_base() {
        let cli = parse(&[
            "wt",
            "workflow",
            "task",
            "add-schema",
            "wire-api",
            "--mode",
            "stack",
            "--profile",
            "codex",
            "--title",
            "Split workflow",
            "--body",
            "Ship the split workflow",
            "--origin-provider",
            "linear",
            "--origin-id",
            "WT-123",
            "--base",
            "main",
            "--pr",
            "ready",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Task {
                    ref tasks,
                    mode: WorkflowModeArg::Stack,
                    profile: Some(ref profile),
                    ref profiles,
                    title: Some(ref title),
                    body: Some(ref body),
                    body_file: None,
                    origin_provider: Some(ref origin_provider),
                    origin_id: Some(ref origin_id),
                    base: Some(ref base),
                    pr: Some(WorkflowPrModeArg::Ready),
                }
            }) if tasks == &vec!["add-schema".to_string(), "wire-api".to_string()]
                && profile == "codex"
                && profiles.is_empty()
                && title == "Split workflow"
                && body == "Ship the split workflow"
                && origin_provider == "linear"
                && origin_id == "WT-123"
                && base == "main"
        ));
    }

    #[test]
    fn workflow_task_accepts_no_task_args_for_interactive_selection() {
        let cli = parse(&["wt", "workflow", "task", "--mode", "batch"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Task {
                    ref tasks,
                    mode: WorkflowModeArg::Batch,
                    profile: None,
                    ref profiles,
                    title: None,
                    body: None,
                    body_file: None,
                    origin_provider: None,
                    origin_id: None,
                    base: None,
                    pr: None,
                }
            }) if tasks.is_empty() && profiles.is_empty()
        ));
    }

    #[test]
    fn workflow_task_rejects_removed_objective_flag() {
        let err = Cli::try_parse_from([
            "wt",
            "workflow",
            "task",
            "--mode",
            "batch",
            "--objective",
            "Ship workflow",
            "add-schema",
        ])
        .unwrap_err();
        assert!(err.to_string().contains("--objective"));
    }

    #[test]
    fn workflow_task_rejects_body_and_body_file_together() {
        let err = Cli::try_parse_from([
            "wt",
            "workflow",
            "task",
            "--mode",
            "batch",
            "--body",
            "inline",
            "--body-file",
            "workflow.md",
            "add-schema",
        ])
        .unwrap_err();
        assert!(err.to_string().contains("--body"));
        assert!(err.to_string().contains("--body-file"));
    }

    #[test]
    fn workflow_task_accepts_matrix_profiles() {
        let cli = parse(&[
            "wt",
            "workflow",
            "task",
            "--mode",
            "matrix",
            "--profiles",
            "alpha,beta",
            "--profiles",
            "gamma",
            "add-schema",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Task {
                    ref tasks,
                    mode: WorkflowModeArg::Matrix,
                    profile: None,
                    ref profiles,
                    ..
                }
            }) if tasks == &vec!["add-schema".to_string()]
                && profiles == &vec![
                    "alpha".to_string(),
                    "beta".to_string(),
                    "gamma".to_string(),
                ]
        ));
    }

    #[test]
    fn workflow_issue_accepts_no_issue_args_for_interactive_selection() {
        let cli = parse(&["wt", "workflow", "issue", "--mode", "batch"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Issue {
                    ref issues,
                    mode: WorkflowModeArg::Batch,
                    profile: None,
                    title: None,
                    body: None,
                    body_file: None,
                    origin_provider: None,
                    origin_id: None,
                    base: None,
                    pr: None,
                }
            }) if issues.is_empty()
        ));
    }

    #[test]
    fn workflow_issue_accepts_pr_draft() {
        let cli = parse(&[
            "wt", "workflow", "issue", "--mode", "stack", "PROJ-1", "--pr", "draft",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Issue {
                    ref issues,
                    mode: WorkflowModeArg::Stack,
                    pr: Some(WorkflowPrModeArg::Draft),
                    ..
                }
            }) if issues == &vec!["PROJ-1".to_string()]
        ));
    }

    #[test]
    fn run_workflow_accepts_jobs() {
        let cli = parse(&["wt", "run", "workflow", "2026-05-16-001", "--jobs", "3"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Workflow {
                    ref workflow,
                    jobs: 3,
                }
            }) if workflow.as_deref() == Some("2026-05-16-001")
        ));
    }

    #[test]
    fn workflow_repair_accepts_apply_flag() {
        let cli = parse(&["wt", "workflow", "repair", "2026-05-17-002", "--apply"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Repair {
                    ref workflow,
                    apply: true,
                }
            }) if workflow == "2026-05-17-002"
        ));
    }

    #[test]
    fn workflow_pass_accepts_task_and_run_next() {
        let cli = parse(&[
            "wt",
            "workflow",
            "pass",
            "2026-05-17-002",
            "add-schema",
            "--run-next",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::Pass {
                    ref workflow,
                    ref task,
                    run_next: true,
                }
            }) if workflow == "2026-05-17-002" && task.as_deref() == Some("add-schema")
        ));
    }

    #[test]
    fn workflow_repair_help_describes_preview_first_contract() {
        let mut command = Cli::command();
        let workflow = command.find_subcommand_mut("workflow").unwrap();
        let repair = workflow.find_subcommand_mut("repair").unwrap();
        let help = repair.render_long_help().to_string();
        assert!(help.contains("dry-run"));
        assert!(help.contains("--apply"));
        assert!(help.contains("Repair never closes cmux workspaces or removes worktrees"));
    }

    #[test]
    fn run_workflow_help_describes_coordinator_handoff() {
        let mut command = Cli::command();
        let run = command.find_subcommand_mut("run").unwrap();
        let workflow = run.find_subcommand_mut("workflow").unwrap();
        let help = workflow.render_long_help().to_string();
        assert!(help.contains("saved workflow"));
        assert!(help.contains("does not list, edit, repair, or pass workflow tasks"));
        assert!(help.contains("Workflow Coordinator Handoff"));
        assert!(help.contains("wt task report"));
        assert!(help.contains("workflow scope derived from the TaskRun"));
        assert!(help.contains("fallback cmux send coordinates"));
        assert!(help.contains("prepared [policy].pull_request"));
        assert!(help.contains("prepared [policy.review].codex_base"));
        assert!(help.contains("wt workflow pass"));
    }

    #[test]
    fn workflow_prepare_help_describes_title_body_origin_options() {
        let mut command = Cli::command();
        let workflow = command.find_subcommand_mut("workflow").unwrap();

        let task = workflow.find_subcommand_mut("task").unwrap();
        let task_help = task.render_long_help().to_string();
        assert!(task_help.contains("--title"));
        assert!(task_help.contains("--body"));
        assert!(task_help.contains("--body-file"));
        assert!(task_help.contains("--origin-provider"));
        assert!(task_help.contains("--origin-id"));
        assert!(task_help.contains("Workflow-level [origin] is stored only on the Workflow"));
        assert!(task_help.contains("does not add issue-closing keywords"));
        assert!(!task_help.contains("--objective"));

        let issue = workflow.find_subcommand_mut("issue").unwrap();
        let issue_help = issue.render_long_help().to_string();
        assert!(issue_help.contains("--title"));
        assert!(issue_help.contains("--body"));
        assert!(issue_help.contains("--body-file"));
        assert!(issue_help.contains("--origin-provider"));
        assert!(issue_help.contains("--origin-id"));
        assert!(
            issue_help
                .contains("Each selected provider issue becomes an executable child TaskDocument")
        );
        assert!(issue_help.contains("Selected issue ids are not automatically lifted"));
        assert!(issue_help.contains("does not add issue-closing keywords"));
        assert!(!issue_help.contains("--objective"));
    }

    #[test]
    fn workflow_help_uses_canonical_description() {
        let mut command = Cli::command();
        let workflow = command.find_subcommand_mut("workflow").unwrap();
        let help = workflow.render_help().to_string();
        assert!(help.contains("Prepare, inspect, edit, repair, archive, or pass workflow tasks"));
        assert!(!help.contains("Start runnable tasks from a workflow"));
        assert!(help.contains("archive"));
        assert!(help.contains("repair"));
        assert!(help.contains("task"));
        assert!(help.contains("issue"));
        assert!(help.contains("pass"));
        assert!(!help.contains("complete"));
    }

    #[test]
    fn workflow_task_help_explains_interactive_task_selection() {
        let mut command = Cli::command();
        let workflow = command.find_subcommand_mut("workflow").unwrap();
        let task = workflow.find_subcommand_mut("task").unwrap();
        let help = task.render_help().to_string();
        assert!(help.contains("[TASKS]..."));
        assert!(help.contains("omit to select multiple existing tasks"));
        assert!(help.contains("--pr <none|draft|ready>"));
        assert!(help.contains("[workflow].pull_request"));
        assert!(!help.contains(&format!("workflow.{}", "defaults")));
        assert!(!help.contains("--pull-request"));
    }

    #[test]
    fn workflow_issue_help_uses_pr_mode() {
        let mut command = Cli::command();
        let workflow = command.find_subcommand_mut("workflow").unwrap();
        let issue = workflow.find_subcommand_mut("issue").unwrap();
        let help = issue.render_help().to_string();
        assert!(help.contains("--pr <none|draft|ready>"));
        assert!(help.contains("[workflow].pull_request"));
        assert!(!help.contains(&format!("workflow.{}", "defaults")));
        assert!(!help.contains("--pull-request"));
    }

    #[test]
    fn run_branch_with_matrix_flag() {
        let cli = parse(&["wt", "run", "branch", "some", "feature", "--matrix"]);
        if let Some(Commands::Run {
            command: RunCommand::Branch { name, matrix, .. },
        }) = &cli.command
        {
            assert_eq!(name, &vec!["some".to_string(), "feature".to_string()]);
            assert!(*matrix);
        } else {
            panic!("expected Branch");
        }
    }

    #[test]
    fn run_branch_rejects_task_option() {
        let err = Cli::try_parse_from(["wt", "run", "branch", "--task", "add-schema"])
            .unwrap_err()
            .to_string();
        assert!(err.contains("unexpected argument '--task'"));
    }

    #[test]
    fn run_branch_with_base_and_profile() {
        let cli = parse(&[
            "wt",
            "run",
            "branch",
            "some",
            "feature",
            "--base",
            "main",
            "--profile",
            "codex",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Branch {
                    ref name,
                    base: Some(ref base),
                    profile: Some(ref profile),
                    matrix: false,
                }
            }) if name == &vec!["some".to_string(), "feature".to_string()]
                && base == "main"
                && profile == "codex"
        ));
    }

    #[test]
    fn run_branch_rejects_matrix_with_profile() {
        let result = Cli::try_parse_from([
            "wt",
            "run",
            "branch",
            "some",
            "feature",
            "--matrix",
            "--profile",
            "codex",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_branch_rejects_profiles_flag() {
        let result = Cli::try_parse_from([
            "wt",
            "run",
            "branch",
            "some",
            "feature",
            "--profiles",
            "alpha,beta",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_branch_rejects_profiles_with_profile() {
        let result = Cli::try_parse_from([
            "wt",
            "run",
            "branch",
            "some",
            "feature",
            "--matrix",
            "--profiles",
            "alpha",
            "--profile",
            "codex",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_branch_rejects_removed_parallel_flag() {
        let result = Cli::try_parse_from(["wt", "run", "branch", "some", "feature", "--parallel"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_branch_help_explains_branch_text_and_task_selection() {
        let mut command = Cli::command();
        let branch = command
            .find_subcommand_mut("run")
            .unwrap()
            .find_subcommand_mut("branch")
            .unwrap();
        let help = branch.render_long_help().to_string();
        assert!(help.contains("branch-name text"));
        assert!(help.contains("new local branch and worktree"));
        assert!(help.contains("wt open"));
        assert!(!help.contains("--task"));
        assert!(!help.contains("prepared local task"));
    }

    #[test]
    fn root_help_lists_canonical_start_surfaces() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("wt run issue"));
        assert!(help.contains("wt run pr"));
        assert!(help.contains("wt run branch"));
        assert!(help.contains("wt run task"));
        assert!(help.contains("wt run workflow"));
        assert!(help.contains("wt run -h"));
        assert!(help.contains("wt help <cmd>"));
        assert!(!help.contains("wt new"));
    }

    #[test]
    fn root_help_prioritizes_common_commands() {
        let help = Cli::command().render_long_help().to_string();

        for heading in [
            "Start Work:",
            "Manage Work:",
            "Prepare:",
            "Coordinate Agents:",
            "Run Agents:",
            "Setup:",
            "Tools:",
            "Examples:",
        ] {
            assert!(help.contains(heading), "missing heading {heading}");
        }
        assert!(!help.contains("\nCommands:\n"));

        let start_work = root_help_section(&help, "Start Work", "Manage Work");
        for command in [
            "run issue",
            "run pr",
            "run branch",
            "run task",
            "run workflow",
        ] {
            assert_section_contains_command(start_work, command);
        }

        let manage_work = root_help_section(&help, "Manage Work", "Prepare");
        for command in ["open", "list", "inspect", "done"] {
            assert_section_contains_command(manage_work, command);
        }
        assert!(!manage_work.contains("\n  ui"));

        let prepare = root_help_section(&help, "Prepare", "Coordinate Agents");
        for command in ["scaffold", "task", "workflow"] {
            assert_section_contains_command(prepare, command);
        }

        let coordinate_agents = root_help_section(&help, "Coordinate Agents", "Run Agents");
        for command in ["agent", "msg", "send", "session"] {
            assert_section_contains_command(coordinate_agents, command);
        }

        let run_agents = root_help_section(&help, "Run Agents", "Setup");
        for command in ["codex", "claude", "as"] {
            assert_section_contains_command(run_agents, command);
        }

        let setup = root_help_section(&help, "Setup", "Tools");
        for command in ["init", "config", "profile", "setup", "doctor"] {
            assert_section_contains_command(setup, command);
        }
        assert!(!setup.contains("\n  shell-init"));

        let tools = root_help_section(&help, "Tools", "Options");
        for command in [
            "ui",
            "studio",
            "site",
            "shell-init",
            "completion",
            "version",
        ] {
            assert_section_contains_command(tools, command);
        }

        let examples = root_help_section(&help, "Examples", "");
        for command in [
            "wt run issue",
            "wt run pr",
            "wt run workflow",
            "wt help <cmd>",
        ] {
            assert!(examples.contains(command), "missing example {command}");
        }
    }

    #[test]
    fn run_help_lists_execution_start_surfaces() {
        let mut command = Cli::command();
        let run = command.find_subcommand_mut("run").unwrap();
        let help = run.render_long_help().to_string();
        assert!(help.contains("wt run issue"));
        assert!(help.contains("wt run pr"));
        assert!(help.contains("wt run branch"));
        assert!(help.contains("wt run task"));
        assert!(help.contains("wt run workflow"));
        assert!(help.contains("issue"));
        assert!(help.contains("pr"));
        assert!(help.contains("branch"));
        assert!(help.contains("task"));
        assert!(help.contains("workflow"));
        assert!(help.contains("only starts workspace execution"));
        assert!(help.contains("Cleanup stays under `wt done`"));
    }

    #[test]
    fn start_subcommand_is_removed() {
        let result = Cli::try_parse_from(["wt", "start"]);
        assert!(result.is_err());
    }

    #[test]
    fn old_execution_start_subcommands_parse_as_hidden_deprecated_traps() {
        let cli = parse(&["wt", "issue"]);
        assert!(matches!(
            cli.command,
            Some(Commands::DeprecatedIssue { .. })
        ));

        let cli = parse(&["wt", "pr"]);
        assert!(matches!(cli.command, Some(Commands::DeprecatedPr { .. })));

        let cli = parse(&["wt", "new"]);
        assert!(matches!(cli.command, Some(Commands::DeprecatedNew { .. })));

        let cli = parse(&["wt", "task", "run"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Task {
                command: TaskCommand::DeprecatedRun { .. }
            })
        ));

        let cli = parse(&["wt", "workflow", "run"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Workflow {
                command: WorkflowCommand::DeprecatedRun { .. }
            })
        ));
    }

    #[test]
    fn list_subcommand() {
        let cli = parse(&["wt", "list"]);
        assert!(matches!(cli.command, Some(Commands::List { wide: false })));
    }

    #[test]
    fn list_accepts_wide_flag() {
        let cli = parse(&["wt", "list", "--wide"]);
        assert!(matches!(cli.command, Some(Commands::List { wide: true })));
    }

    #[test]
    fn done_subcommand() {
        let cli = parse(&["wt", "done", "feature"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Done { ref targets }) if targets == &vec!["feature".to_string()]
        ));
    }

    #[test]
    fn doctor_subcommand() {
        let cli = parse(&["wt", "doctor"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor {
                profile: None,
                prune_env_anchors: None
            })
        ));
    }

    #[test]
    fn doctor_accepts_profile_flag() {
        let cli = parse(&["wt", "doctor", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor {
                profile: Some(ref profile),
                prune_env_anchors: None,
            }) if profile == "codex"
        ));
    }

    #[test]
    fn doctor_accepts_prune_env_anchors_flag() {
        let cli = parse(&["wt", "doctor", "--prune-env-anchors", "surface:A22D"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor {
                profile: None,
                prune_env_anchors: Some(ref key),
            }) if key == "surface:A22D"
        ));
    }

    #[test]
    fn config_subcommand() {
        let cli = parse(&["wt", "config"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: None
            })
        ));
    }

    #[test]
    fn config_accepts_profile_flag() {
        let cli = parse(&["wt", "config", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: Some(ref profile),
                command: None
            }) if profile == "codex"
        ));
    }

    #[test]
    fn config_edit_accepts_optional_source() {
        let cli = parse(&["wt", "config", "edit", ".wt/config/local.toml"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: Some(ConfigCommand::Edit { ref source }),
            }) if source.as_deref() == Some(std::path::Path::new(".wt/config/local.toml"))
        ));
    }

    #[test]
    fn config_extract_accepts_optional_source() {
        let cli = parse(&["wt", "config", "extract", ".wt/config/local.toml"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: Some(ConfigCommand::Extract { ref source }),
            }) if source.as_deref() == Some(std::path::Path::new(".wt/config/local.toml"))
        ));
    }

    #[test]
    fn config_inline_accepts_optional_source() {
        let cli = parse(&[
            "wt",
            "config",
            "inline",
            ".wt/config/profiles/codex/profile.toml",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: Some(ConfigCommand::Inline { ref source }),
            }) if source.as_deref() == Some(std::path::Path::new(".wt/config/profiles/codex/profile.toml"))
        ));
    }

    #[test]
    fn profile_promote_subcommand_is_removed() {
        let result = Cli::try_parse_from(["wt", "profile", "promote", "codex"]);
        assert!(result.is_err());
    }

    #[test]
    fn open_subcommand() {
        let cli = parse(&["wt", "open", "feature"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Open {
                target: Some(ref target)
            }) if target == "feature"
        ));
    }

    #[test]
    fn init_local_codex_yes() {
        let cli = parse(&["wt", "init", "--local", "--agent", "codex", "--yes"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                local: true,
                shared: false,
                agent: Some(InitAgent::Codex),
                yes: true,
                ..
            })
        ));
    }

    #[test]
    fn init_parses_agent_args() {
        let cli = parse(&[
            "wt",
            "init",
            "--local",
            "--agent",
            "gemini",
            "--agent-arg",
            "--model=gemini-pro",
            "--issue-provider",
            "github",
            "--site-provider",
            "valet",
            "--gh-user",
            "alice",
            "--force",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                local: true,
                agent: Some(InitAgent::Gemini),
                issue_provider: Some(InitIssueProvider::Github),
                site_provider: Some(InitSiteProvider::Valet),
                force: true,
                ..
            })
        ));
        if let Some(Commands::Init {
            agent_args,
            site_provider,
            gh_user,
            ..
        }) = cli.command
        {
            assert_eq!(agent_args, vec!["--model=gemini-pro"]);
            assert_eq!(site_provider, Some(InitSiteProvider::Valet));
            assert_eq!(gh_user.as_deref(), Some("alice"));
        }
    }

    #[test]
    fn init_parses_recommendation_dry_run() {
        let cli = parse(&["wt", "init", "--dry-run"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init { dry_run: true, .. })
        ));
    }

    #[test]
    fn init_rejects_legacy_preset_flag() {
        let result = Cli::try_parse_from(["wt", "init", "--preset", "agent"]);
        assert!(result.is_err());
    }

    #[test]
    fn init_rejects_legacy_minimal_flag() {
        let result = Cli::try_parse_from(["wt", "init", "--minimal"]);
        assert!(result.is_err());
    }

    #[test]
    fn init_rejects_prompts_flag() {
        let result = Cli::try_parse_from(["wt", "init", "--prompts"]);
        assert!(result.is_err());

        let result = Cli::try_parse_from(["wt", "init", "--no-prompts"]);
        assert!(result.is_err());
    }

    #[test]
    fn init_accepts_docker_proxy_site_provider() {
        let cli = parse(&["wt", "init", "--site-provider", "docker_proxy"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                site_provider: Some(InitSiteProvider::DockerProxy),
                ..
            })
        ));
    }

    #[test]
    fn init_rejects_docker_proxy_kebab_alias() {
        let result = Cli::try_parse_from(["wt", "init", "--site-provider", "docker-proxy"]);
        assert!(result.is_err());
    }

    #[test]
    fn init_accepts_traefik_site_provider() {
        let cli = parse(&["wt", "init", "--site-provider", "traefik"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                site_provider: Some(InitSiteProvider::Traefik),
                ..
            })
        ));
    }

    #[test]
    fn site_doctor_subcommand() {
        let cli = parse(&["wt", "site", "doctor"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Site {
                command: SiteCommand::Doctor
            })
        ));
    }

    #[test]
    fn site_example_launchd_accepts_overrides() {
        let cli = parse(&[
            "wt",
            "site",
            "example-launchd",
            "--label",
            "dev.wt-traefik",
            "--bind-ip",
            "127.0.0.3",
        ]);

        assert!(matches!(
            cli.command,
            Some(Commands::Site {
                command: SiteCommand::ExampleLaunchd { label, bind_ip }
            }) if label == "dev.wt-traefik" && bind_ip == "127.0.0.3"
        ));
    }

    #[test]
    fn traefik_subcommand_is_removed() {
        let result = Cli::try_parse_from(["wt", "traefik", "doctor"]);
        assert!(result.is_err());
    }

    #[test]
    fn base_mode_from_none() {
        assert_eq!(BaseMode::from_raw(&None), BaseMode::Default);
    }

    #[test]
    fn base_mode_from_empty() {
        assert_eq!(BaseMode::from_raw(&Some("".into())), BaseMode::Interactive);
    }

    #[test]
    fn base_mode_from_dot() {
        assert_eq!(BaseMode::from_raw(&Some(".".into())), BaseMode::Current);
    }

    #[test]
    fn base_mode_from_value() {
        assert_eq!(
            BaseMode::from_raw(&Some("develop".into())),
            BaseMode::Explicit("develop".into())
        );
    }

    #[test]
    fn profile_list_subcommand_parses() {
        let cli = parse(&["wt", "profile", "list"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Profile {
                command: Some(ProfileCommand::List)
            })
        ));
    }

    #[test]
    fn profile_without_subcommand_parses_as_omission_default() {
        let cli = parse(&["wt", "profile"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Profile { command: None })
        ));
    }

    #[test]
    fn profile_help_describes_omission_default_and_list_subcommand() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("profile")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("omission-default"));
        assert!(help.contains("wt profile list"));
        assert!(help.contains("list"));
        assert!(help.contains("create"));
    }

    #[test]
    fn config_help_describes_canonical_config_locations() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("config")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains(".wt.toml"));
        assert!(help.contains("<repo-root>/.wt/config/local.toml"));
        assert!(help.contains("<repo-root>/.wt/config/profiles/<name>/profile.toml"));
        assert!(help.contains("SOURCE name `shared`"));
        assert!(help.contains("SOURCE name `local`"));
        assert!(help.contains("SOURCE name `profiles/<name>`"));
        assert!(help.contains("reject files outside that managed namespace"));
    }

    #[test]
    fn config_source_subcommand_help_describes_closed_namespace() {
        let mut command = Cli::command();
        let config = command.find_subcommand_mut("config").unwrap();

        for subcommand in ["edit", "extract", "inline"] {
            let help = config
                .find_subcommand_mut(subcommand)
                .unwrap()
                .render_long_help()
                .to_string();
            assert!(help.contains("shared"), "{subcommand} help was:\n{help}");
            assert!(help.contains("local"), "{subcommand} help was:\n{help}");
            assert!(
                help.contains("profiles/<name>"),
                "{subcommand} help was:\n{help}"
            );
            assert!(
                help.contains("canonical path"),
                "{subcommand} help was:\n{help}"
            );
        }

        let inline_help = config
            .find_subcommand_mut("inline")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(inline_help.contains("direct prompt-file SOURCE paths are rejected"));
    }

    #[test]
    fn profile_list_help_describes_invalid_profile_reporting() {
        let mut command = Cli::command();
        let help = command
            .find_subcommand_mut("profile")
            .unwrap()
            .find_subcommand_mut("list")
            .unwrap()
            .render_long_help()
            .to_string();

        assert!(help.contains("List named profile configs"));
        assert!(help.contains("invalid_profiles"));
        assert!(help.contains("deterministic"));
    }
}
