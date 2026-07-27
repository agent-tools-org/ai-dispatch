// Persistent acceptance and durability evidence for task artifacts.
// Exports append-only custody records; depends on rusqlite and chrono.

use anyhow::Result;
use chrono::Local;
use rusqlite::{params, OptionalExtension};

use super::Store;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcceptanceDecision {
    Accepted,
    Rejected,
}

impl AcceptanceDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "accepted" => Self::Accepted,
            _ => Self::Rejected,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceRecord {
    pub decision: AcceptanceDecision,
    pub principal_id: String,
    pub accepted_head_sha: Option<String>,
    pub accepted_branch: Option<String>,
    pub manifest_digest: Option<String>,
}

impl Store {
    pub fn record_acceptance(
        &self,
        task_id: &str,
        record: &AcceptanceRecord,
        source: &str,
    ) -> Result<()> {
        self.db().execute(
            "INSERT INTO task_acceptance
             (task_id, decision, decided_at, principal_id, source,
              accepted_head_sha, accepted_branch, artifact_manifest_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task_id,
                record.decision.as_str(),
                Local::now().to_rfc3339(),
                record.principal_id,
                source,
                record.accepted_head_sha,
                record.accepted_branch,
                record.manifest_digest,
            ],
        )?;
        Ok(())
    }

    pub fn latest_acceptance(&self, task_id: &str) -> Result<Option<AcceptanceRecord>> {
        self.db()
            .query_row(
                "SELECT decision, principal_id, accepted_head_sha,
                        accepted_branch, artifact_manifest_digest
                 FROM task_acceptance WHERE task_id = ?1
                 ORDER BY id DESC LIMIT 1",
                params![task_id],
                |row| {
                    Ok(AcceptanceRecord {
                        decision: AcceptanceDecision::parse(&row.get::<_, String>(0)?),
                        principal_id: row.get(1)?,
                        accepted_head_sha: row.get(2)?,
                        accepted_branch: row.get(3)?,
                        manifest_digest: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_durability(
        &self,
        task_id: &str,
        head_sha: &str,
        manifest_digest: &str,
        certificate_json: &str,
    ) -> Result<()> {
        self.db().execute(
            "INSERT INTO artifact_durability
             (task_id, checked_at, accepted_head_sha, manifest_digest, certificate_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task_id,
                Local::now().to_rfc3339(),
                head_sha,
                manifest_digest,
                certificate_json
            ],
        )?;
        Ok(())
    }
}
