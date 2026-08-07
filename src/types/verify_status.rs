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
}

impl VerifyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::TimedOut => "timed_out",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "timed_out" => Some(Self::TimedOut),
            _ => None,
        }
    }

    /// True when a verify command was started (pass, fail, or timeout).
    /// Distinct from delivery: an empty-diff task can still have been verified.
    pub fn was_attempted(self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::TimedOut)
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
    fn was_attempted_excludes_skipped_and_pending() {
        assert!(VerifyStatus::Passed.was_attempted());
        assert!(VerifyStatus::Failed.was_attempted());
        assert!(VerifyStatus::TimedOut.was_attempted());
        assert!(!VerifyStatus::Skipped.was_attempted());
        assert!(!VerifyStatus::Pending.was_attempted());
    }
}
