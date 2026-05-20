use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "wt",
    version,
    about = "Worktree-based agent orchestration harness",
    after_help = "Start workspace execution with: wt run issue, wt run pr, wt run branch, wt run task, wt run workflow.\nUse wt open for existing branches or worktrees; use wt workflow for saved workflow files and lifecycle actions."
)]
pub struct Cli {
    /// Run wt from DIR
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    pub directory: Option<PathBuf>,
    /// Config file to load for commands that read wt config
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Increase diagnostic output (-v, -vv)
    #[arg(short, long, action = ArgAction::Count, global = true, conflicts_with = "quiet")]
    pub verbose: u8,
    /// Suppress normal status output
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// When to use terminal colors
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true)]
    pub color: ColorMode,
    /// Disable terminal colors
    #[arg(long = "no-color", global = true, conflicts_with = "color")]
    pub no_color: bool,
    /// Emit machine-readable JSON for supported commands
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
    /// Prepare, inspect, edit, repair, or complete workflow tasks
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
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
        long_about = "Remove checked-out worktrees, clean integrations, mark matching direct running TaskRuns done, and delete local branches.\n\nPass branch, worktree path/name, issue-like branch-name shorthand, or direct TaskRun id. Workflow-linked TaskRun ids are completed with `wt workflow complete`, not `wt done`. Omit TARGETS to choose worktrees interactively."
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
    /// Install hooks for detected agent CLIs
    #[command(
        long_about = "Install wt-managed inbox hooks for detected agent CLIs.\n\n`wt install` scans PATH for supported agent commands such as `claude` and `codex`, then installs the matching WT_AGENT_ID dispatcher hooks. Hook installation is capability setup only; per-session identity still comes from `wt codex`, `wt claude`, `wt as`, or wt-launched run/workflow sessions."
    )]
    Install,
    /// Uninstall wt-managed agent hooks
    #[command(
        long_about = "Uninstall wt-managed inbox hooks for supported agent CLIs.\n\n`wt uninstall` removes wt-managed Claude and Codex hook entries while preserving user-managed hooks, cmux hooks, and unrelated trust state."
    )]
    Uninstall,
    /// Launch Codex with the current worktree's wt agent identity
    #[command(
        long_about = "Launch Codex with WT_AGENT_ID derived from the current git branch and WT_COORDINATOR_AGENT_ID=agents/coordinator.\n\nUse `wt codex` for the default agent inbox `agents/<branch_slug>`. In the same worktree, use a leading role such as `wt codex @planner` or `wt codex @reviewer` to launch a separate inbox like `agents/<branch_slug>-planner`, so multiple agents do not consume each other's messages. Extra Codex arguments are passed through after the optional role."
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
        long_about = "Launch Claude with WT_AGENT_ID derived from the current git branch and WT_COORDINATOR_AGENT_ID=agents/coordinator.\n\nUse `wt claude` for the default agent inbox `agents/<branch_slug>`. In the same worktree, use a leading role such as `wt claude @coordinator` or `wt claude @reviewer` to launch a separate inbox like `agents/<branch_slug>-coordinator`, so multiple agents do not consume each other's messages. Extra Claude arguments are passed through after the optional role."
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
        long_about = "Run any command with an explicit WT_AGENT_ID and WT_COORDINATOR_AGENT_ID=agents/coordinator.\n\nUse `wt as <AGENT> -- <COMMAND>` as the low-level escape hatch for scripts, unusual agent CLIs, or identities that should not be derived from the current branch. For daily Codex and Claude launches, prefer `wt codex`, `wt codex @planner`, `wt claude`, or `wt claude @reviewer`."
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
        long_about = "Start a read-only personal wt state web UI. The server binds to 127.0.0.1, serves embedded no-build assets, and exposes only allowlisted routes including GET /api/snapshot for <git-common-dir>/wt ideas, retrospectives, TaskDocuments, Workflows, TaskRuns, profiles, and effective config summaries."
    )]
    Ui {
        /// Port to bind on 127.0.0.1; 0 selects an available port
        #[arg(long, default_value_t = 0, value_name = "PORT")]
        port: u16,
    },
    /// Send, deliver, and inspect file-based agent inbox messages
    #[command(
        long_about = "Send, deliver, and inspect file-based agent inbox messages stored under <git-common-dir>/wt/messages/agents/<agent>/inbox/<state>.\n\nUse `wt msg send --to <agent> <message>` for scriptable direct/default-scope sends, or add `--scope workflow:<id>` for workflow-owned coordinator reports. The short target `coordinator` normalizes to the local coordinator inbox `agents/coordinator`. Use `wt msg list --agent <agent>` and `wt msg read --agent <agent> <message-id>` for read-only lifecycle inspection. Use `wt msg check-inbox --agent <agent>` from agent hooks; deliverable direct-scope messages from inbox/new or eligible inbox/retry are claimed, emitted as hook-compatible JSON, then acknowledged into inbox/delivered after stdout is written."
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
        /// Run checks against the effective config for <git-common-dir>/wt/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
    },
    /// Print, edit, or refactor wt config files
    #[command(
        long_about = "Print, edit, or refactor wt config files. Shared repo config is .wt.toml; private repo config is <git-common-dir>/wt/config.toml; named profile config is <git-common-dir>/wt/profiles/<name>/profile.toml."
    )]
    Config {
        /// Show effective config using <git-common-dir>/wt/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    /// List or manage named profile configs
    #[command(
        long_about = "List or manage named profile configs stored under <git-common-dir>/wt/profiles/<name>/profile.toml. Bare `wt profile` is an omission-default that runs `wt profile list`; the canonical inventory surface is the explicit `wt profile list` subcommand. Use `wt profile create <name>` to scaffold a new profile."
    )]
    Profile {
        #[command(subcommand)]
        command: Option<ProfileCommand>,
    },
    /// Start the workspace config wizard
    Init {
        /// Write private config to <git-common-dir>/wt/config.toml
        #[arg(long, conflicts_with = "shared")]
        local: bool,
        /// Write shared project config to .wt.toml
        #[arg(long)]
        shared: bool,
        /// Starter preset to generate
        #[arg(long, value_enum, value_name = "PRESET", conflicts_with = "minimal")]
        preset: Option<InitPreset>,
        /// Generate the minimal starter preset
        #[arg(long, conflicts_with = "preset")]
        minimal: bool,
        /// Agent runtime to write into [profile.agent]
        #[arg(long, value_enum)]
        agent: Option<InitAgent>,
        /// Extra argument for [profile.agent]
        #[arg(long = "agent-arg", allow_hyphen_values = true)]
        agent_args: Vec<String>,
        /// Override the agent launch command
        #[arg(long)]
        agent_command: Option<String>,
        /// Issue provider to configure
        #[arg(long, value_enum)]
        issue_provider: Option<InitIssueProvider>,
        /// Local site provider to configure
        #[arg(long, value_enum)]
        site_provider: Option<InitSiteProvider>,
        /// GitHub user for issue list filtering
        #[arg(long)]
        gh_user: Option<String>,
        /// Skip interactive prompts, use defaults, and write unless target exists
        #[arg(long)]
        yes: bool,
        /// Preview target, preset, detected signals, and TOML without writing files
        #[arg(long)]
        dry_run: bool,
        /// Overwrite an existing config file during non-interactive writes
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
pub enum ConfigCommand {
    /// Open a config file in the configured editor
    Edit {
        /// Config file to edit (omit to select from known config files)
        source: Option<PathBuf>,
    },
    /// Move selected config sections into the next structured config file
    Extract {
        /// Config source file to refactor
        source: Option<PathBuf>,
    },
    /// Move selected structured config back inline
    Inline {
        /// Config or prompt source file to refactor
        source: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum ProfileCommand {
    /// List named profile configs
    #[command(
        long_about = "List named profile configs discovered under <git-common-dir>/wt/profiles/<name>/profile.toml. Profiles are listed in deterministic name order with their copy, link, and agent summary. Invalid profile records are surfaced as warnings in text output and as `invalid_profiles` entries in JSON output rather than being silently hidden. The reserved `default` name is never shown as a valid named profile."
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
        long_about = "Poll a task agent's runtime state from the matching cmux surface. Prints compact state transitions and exits with the agent observation exit-code contract. Use --timeout to stop waiting after a bounded number of seconds, and --heartbeat to print unchanged running observations at an explicit interval. Omit TARGET in an interactive terminal to choose an observable work target; pass TARGET explicitly for scripts, --json, --quiet, and non-interactive use."
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
    /// Install or uninstall local agent hook adapters
    #[command(
        long_about = "Install or uninstall local agent hook adapters.\n\nThe Claude adapter is Claude-specific: it uses Claude Code worktree-local `.claude/settings.local.json` hooks to deliver the wt file inbox through a WT_AGENT_ID dispatcher, and adds that path to the per-worktree Git exclude file. The Codex adapter is Codex-specific: it uses user-level `$CODEX_HOME/hooks.json` or `~/.codex/hooks.json` hooks plus matching trusted hook state in `config.toml`. Adapter installation preserves non-wt hooks and trust state."
    )]
    Hook {
        #[command(subcommand)]
        command: AgentHookCommand,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum AgentHookCommand {
    /// Install a local hook adapter
    Install {
        #[command(subcommand)]
        command: AgentHookInstallCommand,
    },
    /// Uninstall a local hook adapter
    Uninstall {
        #[command(subcommand)]
        command: AgentHookUninstallCommand,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum AgentHookInstallCommand {
    /// Install Claude Code UserPromptSubmit inbox polling
    #[command(
        long_about = "Install the Claude-specific wt inbox hook dispatcher for Claude Code.\n\nThis writes a worktree-local `.claude/settings.local.json` UserPromptSubmit hook that reads WT_AGENT_ID at runtime and runs `wt msg check-inbox --agent \"$WT_AGENT_ID\"`; when WT_AGENT_ID is unset, the hook exits successfully without output. Install also adds that local settings path to the per-worktree Git exclude file, preserves existing local Claude settings, and fails instead of modifying tracked settings or source files.\n\nUse --agent only as a manual or test override. Normal `wt run issue`, `wt run task`, and `wt run workflow` sessions bind the per-run agent by setting WT_AGENT_ID=agents/<branch_slug> and WT_COORDINATOR_AGENT_ID=agents/coordinator when they launch Claude."
    )]
    Claude {
        /// Manual/test override: bind this hook to one agent instead of WT_AGENT_ID
        #[arg(long)]
        agent: Option<String>,
    },
    /// Install Codex UserPromptSubmit inbox polling
    #[command(
        long_about = "Install the Codex-specific wt inbox hook dispatcher for Codex.\n\nThis writes a user-level `$CODEX_HOME/hooks.json` or `~/.codex/hooks.json` UserPromptSubmit hook that reads WT_AGENT_ID at runtime and runs `wt msg check-inbox --agent \"$WT_AGENT_ID\"`; when WT_AGENT_ID is unset, the hook exits successfully without output. Install also writes the matching trusted hook state into Codex `config.toml` and preserves existing non-wt and cmux hooks and trust entries.\n\nUse --agent only as a manual or test override. Normal `wt run issue`, `wt run task`, and `wt run workflow` sessions bind the per-run agent by setting WT_AGENT_ID=agents/<branch_slug> and WT_COORDINATOR_AGENT_ID=agents/coordinator when they launch Codex."
    )]
    Codex {
        /// Manual/test override: bind this user-level hook to one agent instead of WT_AGENT_ID
        #[arg(long)]
        agent: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum AgentHookUninstallCommand {
    /// Uninstall Claude Code UserPromptSubmit inbox polling
    #[command(
        long_about = "Uninstall the Claude-specific wt inbox hook dispatcher for Claude Code.\n\nThis removes wt-managed Claude UserPromptSubmit hook entries from worktree-local `.claude/settings.local.json`. Other local Claude settings and user-managed hooks are preserved. Use --agent only to remove a manual/test override for that agent."
    )]
    Claude {
        /// Manual/test override: remove only the wt-managed hook bound to this agent
        #[arg(long)]
        agent: Option<String>,
    },
    /// Uninstall Codex UserPromptSubmit inbox polling
    #[command(
        long_about = "Uninstall the Codex-specific wt inbox hook dispatcher for Codex.\n\nThis removes wt-managed Codex UserPromptSubmit hook entries from user-level hooks.json and removes the matching wt-managed trust state from config.toml. Other Codex hooks and trust entries are preserved. Use --agent only to remove a manual/test override for that agent."
    )]
    Codex {
        /// Manual/test override: remove only the wt-managed hook bound to this agent
        #[arg(long)]
        agent: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum MsgCommand {
    /// Write one message to an agent inbox
    #[command(
        long_about = "Write one message to an agent inbox.\n\nUnscoped sends use the direct/default scope. Use `--scope workflow:<id>` for workflow-owned coordinator reports, `--scope task_run:<id>` for TaskRun-owned delivery, or `--scope repo` for repo-local singleton delivery."
    )]
    Send {
        /// Target agent id as NAME or agents/NAME; coordinator targets agents/coordinator
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
    CheckInbox {
        /// Agent id as NAME or agents/NAME
        #[arg(long)]
        agent: String,
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
        /// Issue number or provider-specific key
        target: Option<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled issue worktree from <git-common-dir>/wt/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each named profile
        #[arg(long, conflicts_with = "profile")]
        matrix: bool,
    },
    /// Start workspaces from pull requests
    Pr {
        /// Pull request numbers (omit to select multiple open PRs)
        #[arg(value_name = "PR")]
        numbers: Vec<u32>,
        /// Apply config from <git-common-dir>/wt/profiles/<name> to the PR worktree
        #[arg(long)]
        profile: Option<String>,
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
        /// Create a profiled branch worktree from <git-common-dir>/wt/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
        /// Start one workspace for each named profile
        #[arg(long, conflicts_with = "profile")]
        matrix: bool,
    },
    /// Start one worktree per selected local TaskDocument
    #[command(
        long_about = "Start one worktree per selected <git-common-dir>/wt/tasks/<task>.toml TaskDocument and record each attempt as a direct TaskRun in <git-common-dir>/wt/task-runs.\n\nPass explicit task keys for scripts. Omit task keys to choose local TaskDocuments interactively.\n\nEvery started task prompt includes a Task Run Coordinator Handoff with coordinator cmux send coordinates and the coordinator inbox target `coordinator`. Task-run agents report PR=none and wait for the coordinator to review, land, and clean up explicitly.\n\nUse `wt workflow task --mode batch` and `wt run workflow` when multiple independent TaskDocuments need saved batch coordination. Use `wt workflow task --mode single` and `wt run workflow` when multiple TaskDocuments should share one workspace."
    )]
    Task {
        /// Local task keys from <git-common-dir>/wt/tasks/<task>.toml
        #[arg(value_name = "TASK")]
        tasks: Vec<String>,
        /// Base branch: --base (interactive), --base . (current), --base main (explicit)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        base: Option<String>,
        /// Create a profiled task worktree from <git-common-dir>/wt/profiles/<name>
        #[arg(long)]
        profile: Option<String>,
    },
    /// Start runnable tasks from a saved workflow
    #[command(
        long_about = "Start runnable tasks from a saved workflow.\n\nOmit WORKFLOW to choose from runnable workflows. A runnable workflow has prepared or failed TaskRuns that can still be started: single mode requires all linked TaskRuns to be prepared or failed, batch mode requires at least one prepared or failed task, and stack mode requires a next prepared or failed task with no running task. Passing WORKFLOW accepts a TOML path or shorthand id for scripts.\n\nThis does not list, edit, repair, or complete workflow files; those lifecycle actions stay under `wt workflow`.\n\nEvery started task prompt includes a Workflow Coordinator Handoff with coordinator cmux send coordinates and a scoped coordinator inbox fallback using `wt msg send --scope workflow:<id> --to coordinator`. All workflow modes use the prepared [policy].pull_request value for PR reporting and pull-request creation and include their `wt workflow complete ...` command. Stack prompts include `--run-next`."
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

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitPreset {
    Minimal,
    Agent,
    Issue,
    App,
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
    /// List saved local TaskDocument files
    #[command(
        long_about = "List all saved <git-common-dir>/wt/tasks/<task>.toml TaskDocument files.\n\nThis is the canonical read-only inventory for local TaskDocuments. It lists valid TaskDocument files whether or not they are selectable by wt run task, reports invalid TaskDocument TOML files instead of hiding them, and does not start workspaces, create local branches, create TaskRuns, prepare workflows, publish provider issues, open pull requests, or run agent setup."
    )]
    List,
    /// Import provider issues as local TaskDocuments
    #[command(
        long_about = "Import existing provider issues into <git-common-dir>/wt/tasks/<safe-issue-id>.toml TaskDocuments, materialize the provider issue branch when needed, and write title, branch, body, and [origin] with the configured provider and issue id. This command does not start workspaces, create local branches, create TaskRuns, prepare workflows, open pull requests, or run agent setup.\n\nFor GitHub, materializing a missing provider issue branch may call gh issue develop. Import fails instead of writing a TaskDocument with an empty branch.\n\nPass explicit issue ids for scripts. Omit issue ids to choose provider issues interactively.\n\nFails before writing when no issue provider is configured, duplicate issue ids are passed, or an imported issue would overwrite an existing local TaskDocument."
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
        long_about = "Create provider issues from selected <git-common-dir>/wt/tasks/<task>.toml files, then write [origin] with the configured provider and created issue id. This command does not start workspaces, create TaskRuns, or run workflow work.\n\nAfter [origin] is written, later wt run task and wt run workflow treat that TaskDocument as provider-origin issue work.\n\nPass explicit task keys for scripts. Omit task keys to choose unprocessed local TaskDocuments interactively; tasks that already have [origin] are excluded from that selector.\n\nFails before creating an issue for an explicit task when no issue provider is configured, the task is missing or invalid, the task already has origin, or the task has an empty title."
    )]
    Publish {
        /// Local task keys from <git-common-dir>/wt/tasks/<task>.toml
        #[arg(value_name = "TASK")]
        tasks: Vec<String>,
    },
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum WorkflowCommand {
    /// List saved workflow files
    #[command(
        long_about = "List all saved <git-common-dir>/wt/workflows/<id>.toml Workflow files.\n\nThis is the canonical read-only inventory for saved workflows. It lists valid Workflow files whether or not they are currently runnable, reports invalid workflow TOML files instead of hiding them, and exposes runnable as derived metadata from linked TaskRuns. Human text output groups workflows under derived action labels such as runnable, waiting, and done, with indented rows and secondary detail lines."
    )]
    List,
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
        /// Named profile from <git-common-dir>/wt/profiles/<name> for all tasks
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
        /// Named profile from <git-common-dir>/wt/profiles/<name> for all tasks
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
    Show {
        /// Workflow TOML path, shorthand id, or "latest" (default)
        workflow: Option<String>,
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
    /// Mark running workflow task runs as complete
    Complete {
        /// Workflow TOML path or shorthand id
        workflow: String,
        /// Running workflow task identifier to complete
        task: Option<String>,
        /// Start the next stack-mode workflow task after marking this one complete
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
            Some(Commands::Doctor { profile: None })
        ));
    }

    #[test]
    fn no_color_flag() {
        let cli = parse(&["wt", "--no-color", "doctor"]);
        assert!(cli.no_color);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor { profile: None })
        ));
    }

    #[test]
    fn run_issue_no_args_starts_interactive_issue_flow() {
        let cli = parse(&["wt", "run", "issue"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Run {
                command: RunCommand::Issue {
                    target: None,
                    base: None,
                    profile: None,
                    matrix: false,
                }
            })
        ));
    }

    #[test]
    fn run_issue_with_target() {
        let cli = parse(&["wt", "run", "issue", "PROJ-680"]);
        if let Some(Commands::Run {
            command:
                RunCommand::Issue {
                    target,
                    base,
                    profile,
                    matrix,
                },
        }) = cli.command
        {
            assert_eq!(target.as_deref(), Some("PROJ-680"));
            assert_eq!(base, None);
            assert_eq!(profile, None);
            assert!(!matrix);
        } else {
            panic!("expected Issue");
        }
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
                    profile: None
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
                }
            }) if numbers == &vec![42, 43, 44] && profile == "codex"
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
        assert!(help.contains("unchanged running observations"));
        assert!(help.contains("Omit TARGET in an interactive terminal"));
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
        assert!(help.contains("<git-common-dir>/wt/tasks/<safe-issue-id>.toml"));
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
        assert!(help.contains("does not start workspaces"));
        assert!(help.contains("later wt run task and wt run workflow"));
        assert!(help.contains("Omit task keys to choose unprocessed local TaskDocuments"));
        assert!(help.contains("tasks that already have [origin] are excluded"));
        assert!(!help.contains("--stack <STACK>"));
        assert!(!help.contains("--batch <BATCH>"));
        assert!(help.contains("no issue provider"));
        assert!(help.contains("already has origin"));
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
                }
            }) if tasks.is_empty()
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
        assert!(help.contains("Task Run Coordinator Handoff"));
        assert!(help.contains("Task-run agents report PR=none"));
        assert!(help.contains("wt workflow task --mode batch"));
        assert!(help.contains("wt workflow task --mode single"));
        assert!(help.contains("wt run workflow"));
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
        assert!(help.contains("does not list, edit, repair, or complete workflow files"));
        assert!(help.contains("Workflow Coordinator Handoff"));
        assert!(help.contains("coordinator cmux send coordinates"));
        assert!(help.contains("scoped coordinator inbox fallback"));
        assert!(help.contains("wt msg send --scope workflow:<id> --to coordinator"));
        assert!(help.contains("prepared [policy].pull_request"));
        assert!(help.contains("wt workflow complete"));
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
        assert!(help.contains("Prepare, inspect, edit, repair, or complete workflow tasks"));
        assert!(!help.contains("Start runnable tasks from a workflow"));
        assert!(help.contains("repair"));
        assert!(help.contains("task"));
        assert!(help.contains("issue"));
        assert!(help.contains("complete"));
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
        assert!(help.contains("Use wt open"));
        assert!(!help.contains("wt new"));
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
            Some(Commands::Doctor { profile: None })
        ));
    }

    #[test]
    fn doctor_accepts_profile_flag() {
        let cli = parse(&["wt", "doctor", "--profile", "codex"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Doctor {
                profile: Some(ref profile),
            }) if profile == "codex"
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
        let cli = parse(&["wt", "config", "edit", ".git/wt/config.toml"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: Some(ConfigCommand::Edit { ref source }),
            }) if source.as_deref() == Some(std::path::Path::new(".git/wt/config.toml"))
        ));
    }

    #[test]
    fn config_extract_accepts_optional_source() {
        let cli = parse(&["wt", "config", "extract", ".git/wt/config.toml"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: Some(ConfigCommand::Extract { ref source }),
            }) if source.as_deref() == Some(std::path::Path::new(".git/wt/config.toml"))
        ));
    }

    #[test]
    fn config_inline_accepts_optional_source() {
        let cli = parse(&[
            "wt",
            "config",
            "inline",
            ".git/wt/profiles/codex/profile.toml",
        ]);
        assert!(matches!(
            cli.command,
            Some(Commands::Config {
                profile: None,
                command: Some(ConfigCommand::Inline { ref source }),
            }) if source.as_deref() == Some(std::path::Path::new(".git/wt/profiles/codex/profile.toml"))
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
    fn init_parses_preset() {
        let cli = parse(&["wt", "init", "--preset", "agent", "--dry-run"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                preset: Some(InitPreset::Agent),
                dry_run: true,
                ..
            })
        ));
    }

    #[test]
    fn init_parses_minimal_shortcut() {
        let cli = parse(&["wt", "init", "--minimal"]);
        assert!(matches!(
            cli.command,
            Some(Commands::Init {
                minimal: true,
                preset: None,
                ..
            })
        ));
    }

    #[test]
    fn init_rejects_conflicting_preset_and_minimal() {
        let result = Cli::try_parse_from(["wt", "init", "--preset", "minimal", "--minimal"]);
        assert!(result.is_err());
    }

    #[test]
    fn init_rejects_unknown_preset_alias() {
        let result = Cli::try_parse_from(["wt", "init", "--preset", "full"]);
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
        assert!(help.contains("<git-common-dir>/wt/config.toml"));
        assert!(help.contains("<git-common-dir>/wt/profiles/<name>/profile.toml"));
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
