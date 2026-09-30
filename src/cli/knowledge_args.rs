// aid CLI arguments for knowledge commands: ask, query, memory, kg.
// Exports the matching clap Args structs and MemoryCommands, KgCommands; depends on clap derive.

use clap::{Args, Subcommand};

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid ask "What is the latest Rust edition?"
  aid ask "Explain this error" --files src/main.rs -o explanation.md"#)]
pub struct AskArgs {
    pub prompt: String,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(short, long)]
    pub model: Option<String>,
    #[arg(long)]
    pub files: Vec<String>,
    #[arg(short, long)]
    pub output: Option<String>,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid query "What does gamma=0 mean in CryptoSwap?"
  aid query "Explain this" --auto
  aid query "Key insight" -g wg-abc1 --finding"#)]
pub struct QueryArgs {
    pub prompt: String,
    #[arg(short, long)]
    pub auto: bool,
    #[arg(short, long)]
    pub model: Option<String>,
    #[arg(short, long)]
    pub group: Option<String>,
    #[arg(long)]
    pub finding: bool,
}

#[derive(Args)]
#[command(after_help = r#"Examples:
  aid memory add discovery "The auth module uses bcrypt not argon2"
  aid memory add convention "Use anyhow::Result in CLI handlers" --tier critical
  aid memory list --type convention
  aid memory search "auth"
  aid memory forget m-a3f1"#)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub action: MemoryCommands,
}

#[derive(Subcommand)]
pub enum MemoryCommands {
    /// Add a memory entry
    Add {
        /// Memory type: discovery, convention, lesson, fact
        #[arg(name = "TYPE")]
        memory_type: String,
        /// Content to remember
        content: String,
        /// Memory tier: identity, critical, on_demand, deep
        #[arg(long)]
        tier: Option<String>,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// List memories (project-scoped by default)
    List {
        /// Filter by type
        #[arg(long = "type")]
        memory_type: Option<String>,
        /// Show all memories across all projects
        #[arg(long)]
        all: bool,
        /// Include usage stats in the output
        #[arg(long)]
        stats: bool,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// Search memories by keyword
    Search {
        /// Search query
        query: String,
        /// Project path (defaults to current git root)
        #[arg(long)]
        project: Option<String>,
    },
    /// Update a memory's content
    Update { id: String, content: String },
    /// Delete a memory entry
    Forget { id: String },
    /// Show the version history for a memory chain
    History { id: String },
}

#[derive(Args)]
pub struct KgArgs {
    #[command(subcommand)]
    pub action: KgCommands,
}

#[derive(Subcommand)]
pub enum KgCommands {
    /// Add a knowledge-graph triple
    Add {
        subject: String,
        predicate: String,
        object: String,
        #[arg(long)]
        valid_from: Option<String>,
        #[arg(long)]
        source: Option<String>,
    },
    /// Query relationships for an entity
    Query {
        entity: String,
        #[arg(long)]
        as_of: Option<String>,
    },
    /// Invalidate a fact (mark as no longer true)
    Invalidate {
        /// Triple ID to invalidate
        id: i64,
    },
    /// Show chronological timeline for an entity
    Timeline { entity: String },
    /// Search the knowledge graph
    Search { query: String },
    /// Show knowledge graph statistics
    Stats,
}
