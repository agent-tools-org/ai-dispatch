// Shared read-only prompt prefix for adapters without hard sandbox enforcement.
// Exports read_only_prompt for command builders.
// Depends on RunOpts for result-file exception semantics.

use super::RunOpts;

pub(crate) fn read_only_prompt(prompt: &str, opts: &RunOpts) -> String {
    if opts.result_file.is_some() {
        format!(
            "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files, EXCEPT the result file specified in this prompt. Only read, analyze, and write your findings to the designated result file.\n\n{}",
            prompt
        )
    } else {
        format!(
            "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files. Only read and analyze.\n\n{}",
            prompt
        )
    }
}
