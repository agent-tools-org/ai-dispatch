// aid CLI arguments for agents and providers: agent, byok, credential, team, tool.
// Exports the matching clap Args structs and their subcommand enums; depends on clap derive.

use clap::{Args, Subcommand};

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid agent list
  aid agent show aider
  aid agent add my-agent
  aid agent remove my-agent"#)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub action: AgentCommands,
}

#[derive(Subcommand)]
pub enum AgentCommands {
    /// List all agents (built-in + custom)
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show agent details and configuration
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Set or clear per-agent defaults
    Config {
        name: String,
        #[arg(long)]
        model: Option<String>,
        /// Default idle timeout in seconds (0 to clear)
        #[arg(long)]
        idle_timeout: Option<u64>,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
    },
    /// Create a new custom agent definition
    Add { name: String },
    /// Remove a custom agent definition
    Remove { name: String },
    /// Fork a built-in or custom agent definition for local editing
    Fork {
        /// Name of the agent to fork (built-in or custom)
        name: String,
        /// Override the new agent name (defaults to `<name>-custom`)
        #[arg(long = "as")]
        new_name: Option<String>,
    },
    /// Show rate-limit / quota status for all agents
    Quota,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid byok apply examples/byok/mimo.toml --dry-run
  aid byok apply ./mimo.toml
  aid byok probe ./mimo.toml
  aid byok remove mimo
  aid byok example > my-provider.toml
  aid byok doc | less"#)]
pub struct ByokArgs {
    #[command(subcommand)]
    pub action: ByokCommands,
}

#[derive(Subcommand)]
pub enum ByokCommands {
    /// Apply a BYOK manifest: patch opencode config + auth and generate the matching aid agent
    Apply {
        /// Path to the BYOK manifest TOML
        manifest: std::path::PathBuf,
        /// Print the plan and exit without modifying any files
        #[arg(long)]
        dry_run: bool,
        /// API key override (else uses manifest api_key or key_env)
        #[arg(long)]
        key: Option<String>,
    },
    /// Remove a BYOK provider by manifest path or provider id
    Remove {
        /// Manifest path OR provider id (e.g. mimo)
        target: String,
    },
    /// Probe an OpenAI-compatible endpoint to confirm it emits tool_calls
    Probe {
        /// Path to the BYOK manifest TOML
        manifest: std::path::PathBuf,
        /// API key override (else uses manifest api_key or key_env)
        #[arg(long)]
        key: Option<String>,
    },
    /// Print a canonical example BYOK manifest (MiMo) to stdout
    Example,
    /// Print the BYOK pattern documentation to stdout
    Doc,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid credential list
  aid credential add codex personal OPENAI_API_KEY
  aid credential remove codex personal"#)]
pub struct CredentialArgs {
    #[command(subcommand)]
    pub action: CredentialAction,
}

#[derive(Subcommand)]
pub enum CredentialAction {
    /// List configured credential pools and key status
    List,
    /// Add a credential entry to the pool config
    Add {
        provider: String,
        name: String,
        env: String,
    },
    /// Remove a credential entry from the pool config
    Remove {
        provider: String,
        name: String,
    },
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid team list
  aid team show dev
  aid team create dev"#)]
pub struct TeamArgs {
    #[command(subcommand)]
    pub action: TeamAction,
}

#[derive(Subcommand)]
pub enum TeamAction {
    /// List all teams
    List,
    /// Show team details and members
    Show {
        /// Team name
        name: String,
    },
    /// Create a new team definition
    Create {
        /// Team name
        name: String,
    },
    /// Remove a team definition
    Delete {
        /// Team name
        name: String,
    },
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid tool list
  aid tool show lint-check
  aid tool add lint-check
  aid tool add scanner --team dev
  aid tool test lint-check file.ts"#)]
pub struct ToolArgs {
    #[command(subcommand)]
    pub action: ToolAction,
}

#[derive(Subcommand)]
pub enum ToolAction {
    /// List available tools
    List {
        /// Filter to a specific team
        #[arg(long)]
        team: Option<String>,
    },
    /// Show tool details
    Show {
        name: String,
        /// Search in team tools directory
        #[arg(long)]
        team: Option<String>,
    },
    /// Create a new tool definition
    Add {
        name: String,
        /// Create in team tools directory
        #[arg(long)]
        team: Option<String>,
    },
    /// Remove a tool definition
    Remove { name: String },
    /// Test-run a tool with arguments
    Test {
        name: String,
        /// Search in team tools directory
        #[arg(long)]
        team: Option<String>,
        /// Arguments to pass to the tool
        args: Vec<String>,
    },
}
