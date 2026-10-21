// Typed rbox execution receipts for remote verification.
// Reads every valid version-one JSONL receipt and classifies safe failover.
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub(crate) enum ExecResult {
    NotStarted {
        reason: String,
        detail: String,
        job: Option<String>,
    },
    Completed {},
    TimedOut {},
    Unknown {},
    #[serde(other)]
    Future,
}

#[derive(Deserialize)]
struct Receipt {
    version: u32,
    #[serde(flatten)]
    result: ExecResult,
}

impl ExecResult {
    pub(crate) fn not_started(&self) -> bool {
        matches!(self, Self::NotStarted { job: None, .. })
    }

    pub(crate) fn retryable(&self) -> bool {
        matches!(self, Self::NotStarted { reason, job: None, .. } if matches!(reason.as_str(),
            "lock_timeout" | "disk_refused" | "offline" | "stamp_mismatch"
            | "ensure_failed" | "launch_error"))
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        match self {
            Self::NotStarted { detail, .. } if !detail.trim().is_empty() => Some(detail),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExecResults(Vec<ExecResult>);

impl ExecResults {
    pub(crate) fn read(path: &Path) -> Option<Self> {
        let receipts = std::fs::read_to_string(path)
            .ok()?
            .lines()
            .filter_map(|line| {
                let receipt: Receipt = serde_json::from_str(line).ok()?;
                (receipt.version == 1).then_some(receipt.result)
            })
            .collect::<Vec<_>>();
        (!receipts.is_empty()).then_some(Self(receipts))
    }

    pub(crate) fn not_started(&self) -> bool {
        !self.0.is_empty() && self.0.iter().all(ExecResult::not_started)
    }

    pub(crate) fn retryable(&self) -> bool {
        !self.0.is_empty() && self.0.iter().all(ExecResult::retryable)
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        self.0.iter().rev().find_map(ExecResult::detail)
    }
}

pub(super) struct ResultFile(PathBuf);

impl ResultFile {
    pub(super) fn create(dir: &Path) -> Option<Self> {
        let path = dir.join(format!("verify-rbox-{:016x}.jsonl", rand::random::<u64>()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Some(Self(path)),
            Err(error) => {
                aid_warn!(
                    "[aid] Cannot create rbox receipt file {}: {error}; using legacy diagnostics",
                    path.display()
                );
                None
            }
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ResultFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_valid_version_one_receipts_are_retained() {
        let file = tempfile::NamedTempFile::new().expect("file");
        std::fs::write(file.path(), concat!(
            "{\"version\":1,\"phase\":\"not_started\",\"reason\":\"offline\",\"detail\":\"offline\"}\n",
            "{\"version\":1,\"phase\":\"completed\",\"command_exit\":75}\n",
            "{\"version\":2,\"phase\":\"unknown\"}\ninvalid\n"
        )).expect("write");
        let results = ExecResults::read(file.path()).expect("receipts");
        assert_eq!(results.0.len(), 2);
        assert!(!results.not_started());
        assert!(!results.retryable());
    }

    #[test]
    fn blank_detail_falls_back_to_text_diagnostics() {
        for detail in ["", " ", "\t\n", "\u{2003}"] {
            let result = ExecResult::NotStarted {
                job: None,
                reason: "lock_timeout".into(),
                detail: detail.into(),
            };
            assert_eq!(result.detail(), None);
        }
        let result = ExecResult::NotStarted {
            job: None,
            reason: "lock_timeout".into(),
            detail: " typed detail ".into(),
        };
        assert_eq!(result.detail(), Some(" typed detail "));
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
                job: None,
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
            ExecResult::Future,
        ] {
            assert!(!result.not_started());
            assert!(!result.retryable());
            assert_eq!(result.detail(), None);
        }
    }
    fn read_receipts(lines: &[serde_json::Value]) -> ExecResults {
        let file = tempfile::NamedTempFile::new().expect("file");
        let jsonl = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(file.path(), jsonl).expect("write");
        ExecResults::read(file.path()).expect("receipts")
    }

    #[test]
    fn any_started_receipt_blocks_infrastructure_and_repick() {
        let refusal = serde_json::json!({"version":1, "phase":"not_started",
            "job":null, "reason":"lock_timeout", "detail":"locked"});
        for phase in ["completed", "timed_out", "unknown", "future_phase"] {
            let started = serde_json::json!({"version":1, "phase":phase, "command_exit":101});
            for lines in [
                vec![started.clone(), refusal.clone()],
                vec![refusal.clone(), started],
            ] {
                let results = read_receipts(&lines);
                assert!(!results.not_started());
                assert!(!results.retryable());
            }
        }
        let mut with_job = refusal;
        with_job["job"] = serde_json::json!("started-job");
        let results = read_receipts(&[with_job]);
        assert!(!results.not_started());
        assert!(!results.retryable());
    }

    #[test]
    fn every_refusal_must_be_retryable() {
        let refusal = serde_json::json!({"version":1, "phase":"not_started",
            "job":null, "reason":"lock_timeout", "detail":"locked"});
        for reason in ["offline", "sync_failed", "future_reason"] {
            let mut other = refusal.clone();
            other["reason"] = serde_json::json!(reason);
            let results = read_receipts(&[refusal.clone(), other]);
            assert!(results.not_started());
            assert_eq!(results.retryable(), reason == "offline");
            assert_eq!(results.detail(), Some("locked"));
        }
        let empty = tempfile::NamedTempFile::new().expect("file");
        assert!(ExecResults::read(empty.path()).is_none());
        std::fs::write(
            empty.path(),
            "invalid\n{\"version\":2,\"phase\":\"completed\"}",
        )
        .expect("write");
        assert!(ExecResults::read(empty.path()).is_none());
    }

    #[test]
    fn receipt_files_are_unique_and_removed_on_drop() {
        let dir = tempfile::tempdir().expect("dir");
        let first = ResultFile::create(dir.path()).expect("first");
        let second = ResultFile::create(dir.path()).expect("second");
        assert_ne!(first.path(), second.path());
        let path = first.path().to_path_buf();
        assert!(
            path.file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("verify-rbox-")
        );
        drop(first);
        assert!(!path.exists());
        assert!(second.path().exists());
        assert!(ResultFile::create(&dir.path().join("missing")).is_none());
    }
}
