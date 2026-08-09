// Classifies verification failures that come from tooling rather than the change.
// Exports: output and error infrastructure classifiers.
// Deps: anyhow and standard string processing.

pub(super) fn output_indicates_infrastructure_failure(output: &str) -> bool {
    contains_infrastructure_marker(output) && !has_compiler_or_test_diagnostic(output)
}

pub(super) fn error_indicates_infrastructure_failure(
    error: &anyhow::Error,
    containerized: bool,
) -> bool {
    let lower = error
        .chain()
        .map(|cause| cause.to_string().to_lowercase())
        .collect::<Vec<_>>()
        .join(" | ");
    lower.contains("resource temporarily unavailable")
        || lower.contains("cannot allocate memory")
        || lower.contains("verify output reader")
        || lower.contains("verify stdout pipe")
        || lower.contains("verify stderr pipe")
        || lower.contains("verify process wait")
        || (containerized
            && (lower.contains("connection refused")
                || lower.contains("cannot connect")
                || lower.contains("daemon is not running")
                || lower.contains("no such file or directory")))
}

fn contains_infrastructure_marker(output: &str) -> bool {
    let lower = output.to_lowercase();
    [
        "sccache: encountered fatal error",
        "failed to spawn command",
        "no space left on device",
        "disk quota exceeded",
        "disk space below",
        "free space below",
        "less than 10 gb free",
        "not enough free space",
        "cannot connect to the docker daemon",
        "container daemon is not running",
        "failed to connect to container",
        "container is not running",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn has_compiler_or_test_diagnostic(output: &str) -> bool {
    output.lines().any(|line| {
        let lower = line.trim().to_lowercase();
        if contains_infrastructure_marker(&lower) || lower.starts_with("sccache:") {
            return false;
        }
        lower.contains("error[")
            || lower.starts_with("error:")
            || lower.starts_with("error ")
            || lower.contains("could not compile")
            || lower.contains("test result: failed")
            || lower.contains("failures:")
            || lower.contains("tests failed")
            || (lower.contains("test suites:") && lower.contains("failed"))
            || lower.contains("assertion failed")
            || lower.contains(" ... failed")
            || lower.contains("--- fail:")
            || lower.starts_with("fail ")
            || lower.starts_with("fail:")
            || lower.starts_with("fail/")
            || lower.contains("build failed")
            || lower.contains("panic:")
            || lower.contains("traceback")
            || lower.contains("syntaxerror")
    })
}

#[cfg(test)]
mod tests {
    use super::{error_indicates_infrastructure_failure, output_indicates_infrastructure_failure};

    #[test]
    fn classifies_real_sccache_spawn_failure_without_diagnostics() {
        assert!(output_indicates_infrastructure_failure(
            "sccache: encountered fatal error\nsccache: error: failed to spawn"
        ));
    }

    #[test]
    fn keeps_go_and_jest_failures_as_change_failures() {
        assert!(!output_indicates_infrastructure_failure(
            "sccache: encountered fatal error\n--- FAIL: TestThing (0.01s)"
        ));
        assert!(!output_indicates_infrastructure_failure(
            "sccache: encountered fatal error\nFAIL src/example.test.ts"
        ));
    }

    #[test]
    fn loose_disk_prose_is_not_an_infrastructure_marker() {
        assert!(!output_indicates_infrastructure_failure(
            "a test that checks free space handling"
        ));
        assert!(!output_indicates_infrastructure_failure(
            "reported disk space usage for the cache"
        ));
    }

    #[test]
    fn classifies_started_verify_reader_failure_as_infrastructure() {
        let error = anyhow::anyhow!("verify output reader thread panicked");
        assert!(error_indicates_infrastructure_failure(&error, false));
    }

    #[test]
    fn classifies_resource_errors_but_not_missing_configured_commands() {
        let eagain = anyhow::anyhow!("failed to spawn subprocess: Resource temporarily unavailable");
        let enomem = anyhow::anyhow!("failed to spawn subprocess: Cannot allocate memory");
        let missing = anyhow::anyhow!("Failed to run verify command: No such file or directory");
        assert!(error_indicates_infrastructure_failure(&eagain, false));
        assert!(error_indicates_infrastructure_failure(&enomem, false));
        assert!(!error_indicates_infrastructure_failure(&missing, false));
    }

    #[test]
    fn keeps_container_markers_gated_to_containerized_verification() {
        let error = anyhow::anyhow!("container exec: No such file or directory");
        assert!(!error_indicates_infrastructure_failure(&error, false));
        assert!(error_indicates_infrastructure_failure(&error, true));
    }
}
