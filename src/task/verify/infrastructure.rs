// Legacy fallback for older rbox: recognize pre-command refusals without exit numbers.
// Exports diagnostic selection and safe failover eligibility for remote verification.
// Deps: anchored rbox diagnostics and completion/job-state markers.

pub(crate) fn diagnostic(output: &str) -> Option<&str> {
    if output.lines().any(|line| {
        line.starts_with("rbox: job ")
            || line.contains("the job continues on the box")
            || line.starts_with("rbox: warning: job ") && line.contains(" is running;")
            || line.starts_with("rbox: infrastructure: box lost mid-job;")
    }) {
        return None;
    }
    output.lines().find(|line| {
        let Some(message) = line.strip_prefix("rbox: ") else {
            return false;
        };
        message.starts_with("tailnet-offline (probe age ")
            || message.contains(": offline (tailnet-offline)")
            || message.contains(": expected stamp ")
                && message.contains(", actual ")
                && message.contains("explicitly")
            || message.starts_with("ensure failed: exit code ")
            || message.starts_with("Environment stamp does not match after ensure;")
            || message.starts_with("sync to ") && message.contains(" (bundle ")
            || message.starts_with("Box lock not acquired within ")
            || message.contains(" GiB free; requires ") && message.ends_with(" GiB")
    })
}

pub(crate) fn retryable(output: &str) -> bool {
    diagnostic(output).is_some_and(|line| !line.starts_with("rbox: sync to "))
}

#[cfg(test)]
#[path = "infrastructure_tests.rs"]
mod tests;
