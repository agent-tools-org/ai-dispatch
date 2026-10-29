// Verification outcome status, separate from task delivery assessment.
// Exports: VerifyStatus.
// Deps: serde.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum VerifyStatus {
    Pending,
    Passed,
    Failed,
    Skipped,
    /// Verify started but did not finish within the wall-clock cap.
    /// Distinct from Failed: a timeout is inconclusive about the change under test.
    TimedOut,
    /// Verify tooling failed before producing a compiler or test diagnostic.
    InfrastructureFailure,
    /// The agent exited after a deliberate foreground detach and no watcher
    /// was alive to observe its completion. No exit code, no completion event,
    /// no parse_completion output — the result is genuinely unknown. Maps to
    /// `Unverified(NoResult)` regardless of whether the operator asked for
    /// verification, because a kill and a success are indistinguishable here.
    Unobserved,
}

impl VerifyStatus {
    #[cfg(test)]
    pub const ALL: [Self; 7] = [
        Self::Pending,
        Self::Passed,
        Self::Failed,
        Self::Skipped,
        Self::TimedOut,
        Self::InfrastructureFailure,
        Self::Unobserved,
    ];

}

super::string_values::string_values!(
    VerifyStatus, as_str(&self), pub parse_str {
        Pending => "pending",
        Passed => "passed",
        Failed => "failed",
        Skipped => "skipped",
        TimedOut => "timed_out",
        InfrastructureFailure => "infrastructure_failure",
        Unobserved => "unobserved",
    }
);

#[cfg(test)]
mod tests {
    use super::VerifyStatus;

    #[test]
    fn timed_out_round_trips() {
        assert_eq!(VerifyStatus::TimedOut.as_str(), "timed_out");
        assert_eq!(VerifyStatus::parse_str("timed_out"), Some(VerifyStatus::TimedOut));
    }

    #[test]
    fn infrastructure_failure_round_trips() {
        assert_eq!(
            VerifyStatus::InfrastructureFailure.as_str(),
            "infrastructure_failure"
        );
        assert_eq!(
            VerifyStatus::parse_str("infrastructure_failure"),
            Some(VerifyStatus::InfrastructureFailure)
        );
    }

    #[test]
    fn unobserved_round_trips() {
        assert_eq!(VerifyStatus::Unobserved.as_str(), "unobserved");
        assert_eq!(VerifyStatus::parse_str("unobserved"), Some(VerifyStatus::Unobserved));
    }

}
