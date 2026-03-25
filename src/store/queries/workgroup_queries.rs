// Workgroup-related store query methods.
// Exports: Store workgroup and finding lookup methods.
// Deps: super::super::Store, rusqlite, crate::types.

use anyhow::{Result, anyhow, bail};
use chrono::Local;
use rusqlite::params;

use super::super::schema::parse_dt;
use super::super::Store;
use crate::types::{Finding, Workgroup, WorkgroupId};

const FINDING_SELECT: &str = "SELECT id, workgroup_id, content, source_task_id, severity, title, file, lines, category, confidence, verdict, score, note, created_at, updated_at FROM findings";

fn row_to_workgroup(row: &rusqlite::Row) -> rusqlite::Result<Result<Workgroup>> {
    Ok(Ok(Workgroup {
        id: WorkgroupId(row.get::<_, String>(0)?),
        name: row.get(1)?,
        shared_context: row.get(2)?,
        created_by: row.get(5).ok().flatten(),
        created_at: parse_dt(&row.get::<_, String>(3)?),
        updated_at: parse_dt(&row.get::<_, String>(4)?),
    }))
}

fn row_to_finding(row: &rusqlite::Row) -> rusqlite::Result<Finding> {
    Ok(Finding {
        id: row.get(0)?,
        workgroup_id: row.get(1)?,
        content: row.get(2)?,
        source_task_id: row.get(3)?,
        severity: row.get(4)?,
        title: row.get(5)?,
        file: row.get(6)?,
        lines: row.get(7)?,
        category: row.get(8)?,
        confidence: row.get(9)?,
        verdict: row.get(10)?,
        score: row.get(11)?,
        note: row.get(12)?,
        created_at: parse_dt(&row.get::<_, String>(13)?),
        updated_at: row.get::<_, Option<String>>(14)?.map(|value| parse_dt(&value)),
    })
}

impl Store {
    pub fn get_workgroup(&self, id: &str) -> Result<Option<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at, created_by
             FROM workgroups WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_workgroup)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn list_workgroups(&self) -> Result<Vec<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at, created_by
             FROM workgroups ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_workgroup)?;
        rows.map(|row| row?).collect()
    }

    pub fn list_findings(&self, workgroup_id: &str) -> Result<Vec<Finding>> {
        self.list_findings_filtered(workgroup_id, None, None)
    }

    pub fn list_findings_filtered(
        &self,
        workgroup_id: &str,
        severity: Option<&str>,
        verdict: Option<&str>,
    ) -> Result<Vec<Finding>> {
        let conn = self.db();
        let mut stmt = conn.prepare(&format!(
            "{FINDING_SELECT} WHERE workgroup_id = ?1 AND (?2 IS NULL OR severity = ?2) AND (?3 IS NULL OR verdict = ?3) ORDER BY created_at ASC"
        ))?;
        let rows = stmt.query_map(params![workgroup_id, severity, verdict], row_to_finding)?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn get_finding(&self, workgroup_id: &str, finding_id: i64) -> Result<Option<Finding>> {
        let conn = self.db();
        let mut stmt = conn.prepare(&format!(
            "{FINDING_SELECT} WHERE workgroup_id = ?1 AND id = ?2"
        ))?;
        let mut rows = stmt.query_map(params![workgroup_id, finding_id], row_to_finding)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn update_finding(
        &self,
        workgroup_id: &str,
        finding_id: i64,
        verdict: Option<&str>,
        score: Option<&str>,
        note: Option<&str>,
    ) -> Result<()> {
        if verdict.is_none() && score.is_none() && note.is_none() {
            bail!("Provide --verdict, --score, and/or --note");
        }

        let now = Local::now().to_rfc3339();
        let rows = self.db().execute(
            "UPDATE findings
             SET verdict = COALESCE(?1, verdict),
                 score = COALESCE(?2, score),
                 note = COALESCE(?3, note),
                 updated_at = ?4
             WHERE workgroup_id = ?5 AND id = ?6",
            params![verdict, score, note, now, workgroup_id, finding_id],
        )?;
        if rows == 0 {
            return Err(anyhow!(
                "Finding '{finding_id}' not found in workgroup '{workgroup_id}'"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Store;
    use serde_json::Value;

    fn test_store() -> Store {
        let store = Store::open_memory().unwrap();
        store
            .create_workgroup("dispatch", "Shared repo rules.", None, Some("wg-abc"))
            .unwrap();
        store
    }

    #[test]
    fn get_finding_returns_matching_row() {
        let store = test_store();
        store
            .insert_finding(
                "wg-abc",
                "first finding",
                Some("t-1234"),
                Some("HIGH"),
                Some("Missing slippage protection"),
                Some("src/lib.rs"),
                Some("10-20"),
                Some("security"),
                Some("high"),
            )
            .unwrap();

        let finding = store.get_finding("wg-abc", 1).unwrap().unwrap();

        assert_eq!(finding.id, 1);
        assert_eq!(finding.content, "first finding");
        assert_eq!(finding.source_task_id.as_deref(), Some("t-1234"));
        assert_eq!(finding.severity.as_deref(), Some("HIGH"));
    }

    #[test]
    fn update_finding_roundtrip_persists_review_fields() {
        let store = test_store();
        store
            .insert_finding("wg-abc", "first finding", None, None, None, None, None, None, None)
            .unwrap();

        store
            .update_finding(
                "wg-abc",
                1,
                Some("CONFIRMED"),
                Some(r#"{"confidence":"high","impact":9}"#),
                Some("validated against current main branch"),
            )
            .unwrap();

        let finding = store.get_finding("wg-abc", 1).unwrap().unwrap();
        let score: Value = serde_json::from_str(finding.score.as_deref().unwrap()).unwrap();

        assert_eq!(finding.verdict.as_deref(), Some("CONFIRMED"));
        assert_eq!(score["confidence"], "high");
        assert_eq!(score["impact"], 9);
        assert_eq!(
            finding.note.as_deref(),
            Some("validated against current main branch")
        );
        assert!(finding.updated_at.is_some());
    }

    #[test]
    fn list_findings_filtered_by_severity() {
        let store = test_store();
        store
            .insert_finding(
                "wg-abc",
                "first finding",
                None,
                Some("LOW"),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        store
            .insert_finding(
                "wg-abc",
                "second finding",
                None,
                Some("HIGH"),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let findings = store
            .list_findings_filtered("wg-abc", Some("HIGH"), None)
            .unwrap();

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].content, "second finding");
    }
}
