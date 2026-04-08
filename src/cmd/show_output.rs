// Output and diff rendering hub for `aid show`.
// Exports: diff/output/log helpers re-exported from focused modules.
// Deps: show_output_diff, show_output_messages, show_output_tests.
#[path = "show_output_diff.rs"]
mod show_output_diff;
#[path = "show_output_artifacts.rs"]
mod show_output_artifacts;
#[path = "show_output_extract.rs"]
mod show_output_extract;
#[path = "show_output_messages.rs"]
mod show_output_messages;

pub use show_output_diff::{diff_text, diff_text_file};
pub use show_output_messages::{
    log_text, output_text, output_text_brief, output_text_for_task, output_text_full,
    read_task_output,
};
pub(crate) use show_output_diff::{diff_stat, parse_diff_stat, worktree_diff};
pub(crate) use show_output_messages::{extract_messages_from_log, read_tail};

#[cfg(test)]
#[path = "show_output_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "show_output_brief_tests.rs"]
mod brief_tests;

#[cfg(test)]
#[path = "show_output_diff_tests.rs"]
mod diff_tests;

#[cfg(test)]
#[path = "show_output_format_tests.rs"]
mod format_tests;
