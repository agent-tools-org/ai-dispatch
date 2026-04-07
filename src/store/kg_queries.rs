// Knowledge graph store query methods.
// Exports: Store knowledge-graph read methods.
// Deps: super::Store, super::kg_schema, super::kg_types, rusqlite.

use anyhow::Result;
use rusqlite::params;

use super::Store;
use super::kg_schema::row_to_kg_triple;
use super::kg_types::{KgStats, KgTriple, parse_kg_timestamp};

fn escape_like(value: &str) -> String {
    value.replace('%', r"\%").replace('_', r"\_")
}

impl Store {
    pub fn query_kg_entity(&self, entity: &str, as_of: Option<&str>) -> Result<Vec<KgTriple>> {
        let as_of = as_of.map(parse_kg_timestamp).transpose()?;
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, subject, predicate, object, valid_from, valid_to, source, created_at
             FROM kg_triples
             WHERE (subject = ?1 OR object = ?1)
               AND (?2 IS NULL OR (
                    COALESCE(valid_from, '') <= ?2
                    AND (valid_to IS NULL OR valid_to > ?2)
               ))
             ORDER BY COALESCE(valid_from, created_at), created_at, id",
        )?;
        let rows = stmt.query_map(params![entity, as_of.as_deref()], row_to_kg_triple)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn kg_timeline(&self, entity: &str) -> Result<Vec<KgTriple>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, subject, predicate, object, valid_from, valid_to, source, created_at
             FROM kg_triples
             WHERE subject = ?1 OR object = ?1
             ORDER BY COALESCE(valid_from, created_at), created_at, id",
        )?;
        let rows = stmt.query_map(params![entity], row_to_kg_triple)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn search_kg(&self, query: &str) -> Result<Vec<KgTriple>> {
        let pattern = format!("%{}%", escape_like(query));
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, subject, predicate, object, valid_from, valid_to, source, created_at
             FROM kg_triples
             WHERE subject LIKE ?1 ESCAPE '\\'
                OR predicate LIKE ?1 ESCAPE '\\'
                OR object LIKE ?1 ESCAPE '\\'
                OR COALESCE(source, '') LIKE ?1 ESCAPE '\\'
             ORDER BY created_at DESC, id DESC
             LIMIT 50",
        )?;
        let rows = stmt.query_map(params![pattern], row_to_kg_triple)?;
        rows.map(|row| row.map_err(Into::into)).collect()
    }

    pub fn kg_stats(&self) -> Result<KgStats> {
        let conn = self.db();
        let stats = conn.query_row(
            "SELECT
                (SELECT COUNT(*) FROM kg_triples),
                (SELECT COUNT(*) FROM kg_triples WHERE valid_to IS NULL),
                (SELECT COUNT(DISTINCT entity) FROM (
                    SELECT subject AS entity FROM kg_triples
                    UNION
                    SELECT object AS entity FROM kg_triples
                )),
                (SELECT COUNT(DISTINCT predicate) FROM kg_triples)",
            [],
            |row| {
                Ok(KgStats {
                    triple_count: row.get(0)?,
                    active_triple_count: row.get(1)?,
                    entity_count: row.get(2)?,
                    predicate_count: row.get(3)?,
                })
            },
        )?;
        Ok(stats)
    }
}
