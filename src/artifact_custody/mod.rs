// Principal-controlled lifecycle for task-owned Git artifacts.
// Exports acceptance commands and the sole authorized deletion gate.

mod acceptance;
mod deletion_gate;
mod durability;

pub(crate) use acceptance::{accept, reject};
pub(crate) use deletion_gate::delete_accepted_worktree;
