// Database query modules for store reads and lookups.
// Exports: re-exports for task, event, memory, and workgroup query methods.
// Deps: submodules under src/store/queries/.

mod event_queries;
mod message_queries;
mod memory_queries;
mod state_queries;
mod task_metrics_queries;
mod task_queries;
mod task_similarity_queries;
mod worktree_task_queries;
mod workgroup_queries;

#[allow(unused_imports)]
pub use event_queries::*;
#[allow(unused_imports)]
pub use message_queries::*;
#[allow(unused_imports)]
pub use memory_queries::*;
#[allow(unused_imports)]
pub use state_queries::*;
#[allow(unused_imports)]
pub use task_metrics_queries::*;
#[allow(unused_imports)]
pub use task_queries::*;
#[allow(unused_imports)]
pub use task_similarity_queries::*;
#[allow(unused_imports)]
pub use worktree_task_queries::*;
#[allow(unused_imports)]
pub use workgroup_queries::*;

#[cfg(test)]
mod tests;
