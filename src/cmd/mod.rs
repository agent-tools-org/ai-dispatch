// Subcommand handlers for aid CLI.
// Each module implements one subcommand.

pub mod agent;
pub mod ask;
pub mod benchmark;
pub mod batch;
pub mod broadcast;
pub mod board;
pub mod board_stream;
pub mod clean;
pub mod config;
pub mod explain;
pub mod group;
pub mod memory;
pub mod finding;
pub mod init;
pub mod mcp;
pub mod merge;
mod mcp_schema;
mod mcp_tools;
pub mod query;
pub mod respond;
pub mod setup;
pub mod retry;
pub mod retry_logic;
pub mod run;
pub mod show;
pub mod export;
pub mod store;
pub mod store_lock;
pub mod team;
pub mod usage;
pub mod summary;
pub mod upgrade;
pub mod wait;
pub mod watch;
pub mod worktree;
pub mod tree;
