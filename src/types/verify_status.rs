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
    pub const ALL: [Self; 7] = [
        Self::Pending,
        Self::Passed,
        Self::Failed,
        Self::Skipped,
        Self::TimedOut,
        Self::InfrastructureFailure,
        Self::Unobserved,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::TimedOut => "timed_out",
            Self::InfrastructureFailure => "infrastructure_failure",
            Self::Unobserved => "unobserved",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "timed_out" => Some(Self::TimedOut),
            "infrastructure_failure" => Some(Self::InfrastructureFailure),
            "unobserved" => Some(Self::Unobserved),
            _ => None,
        }
    }

    /// True when a verify command was started (pass, fail, or timeout).
    /// Distinct from delivery: an empty-diff task can still have been verified.
    pub fn was_attempted(self) -> bool {
        matches!(
            self,
            Self::Passed | Self::Failed | Self::TimedOut | Self::InfrastructureFailure
        )
    }
}

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

    #[test]
    fn was_attempted_excludes_skipped_pending_and_unobserved() {
        assert!(VerifyStatus::Passed.was_attempted());
        assert!(VerifyStatus::Failed.was_attempted());
        assert!(VerifyStatus::TimedOut.was_attempted());
        assert!(VerifyStatus::InfrastructureFailure.was_attempted());
        assert!(!VerifyStatus::Skipped.was_attempted());
        assert!(!VerifyStatus::Pending.was_attempted());
        assert!(!VerifyStatus::Unobserved.was_attempted());
    }
}
