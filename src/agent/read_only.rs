// Shared read-only prompt prefix for adapters without hard sandbox enforcement.
// Exports read_only_prompt and allow_result_file_write for command builders.
// Depends on RunOpts for result-file exception semantics.

use super::RunOpts;

/// Hard CLI plan/read-only modes block every write, including the task result
/// file. When a result file is declared, prefer prompt-level read-only so the
/// agent can deliver the report without modifying the repo under test.
pub(crate) fn allow_result_file_write(opts: &RunOpts) -> bool {
    opts.read_only && opts.result_file.is_some()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(read_only: bool, result_file: Option<&str>) -> RunOpts {
        RunOpts {
            dir: None,
            output: None,
            result_file: result_file.map(str::to_string),
            model: None,
            budget: false,
            read_only,
            sandbox: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        }
    }

    #[test]
    fn allow_result_file_write_only_when_read_only_and_result_file() {
        assert!(!allow_result_file_write(&opts(false, Some("result.md"))));
        assert!(!allow_result_file_write(&opts(true, None)));
        assert!(allow_result_file_write(&opts(true, Some("result.md"))));
    }

    #[test]
    fn read_only_prompt_names_result_file_exception() {
        let text = read_only_prompt("audit findings", &opts(true, Some("result.md")));
        assert!(text.contains("EXCEPT the result file"));
        assert!(text.contains("audit findings"));
    }
}
