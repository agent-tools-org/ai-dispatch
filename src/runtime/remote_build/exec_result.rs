// Typed rbox execution receipts for remote verification.
// Reads the last valid version-one JSONL receipt and classifies safe failover.
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(crate) enum ExecResult {
    NotStarted { reason: String, detail: String },
    Completed {},
    TimedOut {},
    Unknown {},
}

#[derive(Deserialize)]
struct Receipt {
    version: u32,
    #[serde(flatten)]
    result: ExecResult,
}

impl ExecResult {
    pub(crate) fn read(path: &Path) -> Option<Self> {
        std::fs::read_to_string(path)
            .ok()?
            .lines()
            .rev()
            .find_map(|line| {
                let receipt: Receipt = serde_json::from_str(line).ok()?;
                (receipt.version == 1).then_some(receipt.result)
            })
    }

    pub(crate) fn not_started(&self) -> bool {
        matches!(self, Self::NotStarted { .. })
    }

    pub(crate) fn retryable(&self) -> bool {
        matches!(self, Self::NotStarted { reason, .. } if matches!(reason.as_str(),
            "lock_timeout" | "disk_refused" | "offline" | "stamp_mismatch"
            | "ensure_failed" | "launch_error"))
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        match self {
            Self::NotStarted { detail, .. } => Some(detail),
            _ => None,
        }
    }
}

pub(super) fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        std::process::Command::new("rbox")
            .args(["exec", "--help"])
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("--result-file")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_valid_version_one_receipt_wins() {
        let file = tempfile::NamedTempFile::new().expect("file");
        std::fs::write(file.path(), concat!(
            "{\"version\":1,\"phase\":\"not_started\",\"reason\":\"offline\",\"detail\":\"offline\"}\n",
            "{\"version\":1,\"phase\":\"completed\",\"command_exit\":75}\n",
            "{\"version\":2,\"phase\":\"unknown\"}\ninvalid\n"
        )).expect("write");
        assert!(matches!(
            ExecResult::read(file.path()),
            Some(ExecResult::Completed {})
        ));
    }

    #[test]
    fn only_known_safe_precommand_reasons_retry() {
        for reason in [
            "lock_timeout",
            "disk_refused",
            "offline",
            "stamp_mismatch",
            "ensure_failed",
            "launch_error",
            "sync_failed",
            "future_reason",
        ] {
            let result = ExecResult::NotStarted {
                reason: reason.into(),
                detail: "detail".into(),
            };
            assert!(result.not_started());
            assert_eq!(result.detail(), Some("detail"));
            assert_eq!(
                result.retryable(),
                !matches!(reason, "sync_failed" | "future_reason")
            );
        }
        for result in [
            ExecResult::Completed {},
            ExecResult::TimedOut {},
            ExecResult::Unknown {},
        ] {
            assert!(!result.not_started());
            assert!(!result.retryable());
            assert_eq!(result.detail(), None);
        }
    }
}
