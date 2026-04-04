// Subcommand handlers for aid CLI.
// Each module implements one subcommand.

pub mod agent;
pub mod ask;
pub mod benchmark;
pub mod batch;
pub mod broadcast;
pub mod board;
pub mod board_stream;
pub mod checklist;
pub(crate) mod checklist_scan;
pub mod clean;
pub mod changelog;
pub mod container;
pub mod config;
pub mod cost;
pub mod explain;
pub mod eta;
pub mod group;
pub mod hook;
pub mod memory;
pub mod finding;
pub mod init;
pub mod mcp;
pub mod merge;
pub(crate) mod noninteractive_stdio;
mod mcp_schema;
mod mcp_tools;
pub mod query;
pub mod respond;
pub(crate) mod report_mode;
pub mod steer;
pub mod stop;
pub mod setup;
pub mod stats;
pub mod retry;
pub mod judge;
pub mod retry_logic;
pub mod run;
pub(crate) mod run_hung_recovery;
pub mod show;
pub mod show_checklist;
pub mod export;
pub mod store;
pub mod store_lock;
pub mod team;
pub mod tool;
pub mod project;
pub mod usage;
pub mod summary;
pub mod summary_cli;
pub mod upgrade;
pub mod wait;
pub mod watch;
#[cfg(feature = "web")]
pub mod web;
pub mod worktree;
pub mod tree;
pub mod experiment_types;
pub mod experiment;
pub mod experiment_persist;
pub mod experiment_status;
#[cfg(test)]
mod batch_auto_fallback_tests;
#[cfg(test)]
mod cost_tests;
#[cfg(test)]
mod show_result_tests;
