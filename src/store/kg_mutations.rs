// Knowledge graph store write methods.
// Exports: Store knowledge-graph mutation methods.
// Deps: super::Store, super::kg_types, rusqlite.

use anyhow::Result;
use chrono::Utc;
use rusqlite::params;

use super::Store;
use super::kg_types::parse_kg_timestamp;

impl Store {
    pub fn add_kg_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        valid_from: Option<&str>,
        source: Option<&str>,
    ) -> Result<i64> {
        let valid_from = valid_from.map(parse_kg_timestamp).transpose()?;
        let created_at = Utc::now().to_rfc3339();
        let conn = self.db();
        conn.execute(
            "INSERT INTO kg_triples (subject, predicate, object, valid_from, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![subject, predicate, object, valid_from, source, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn invalidate_kg_triple(&self, id: i64) -> Result<bool> {
        let rows = self.db().execute(
            "UPDATE kg_triples SET valid_to = ?1 WHERE id = ?2 AND valid_to IS NULL",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(rows > 0)
    }
}
