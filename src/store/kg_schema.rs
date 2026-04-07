// Knowledge graph schema helpers and row mapping.
// Exports: CREATE_KG_SQL, row_to_kg_triple.
// Deps: rusqlite, super::schema::parse_dt, super::kg_types.

use rusqlite::Row;

use super::kg_types::KgTriple;
use super::schema::parse_dt;

pub(super) const CREATE_KG_SQL: &str = "CREATE TABLE IF NOT EXISTS kg_triples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    object TEXT NOT NULL,
    valid_from TEXT,
    valid_to TEXT,
    source TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_kg_subject ON kg_triples(subject);
CREATE INDEX IF NOT EXISTS idx_kg_object ON kg_triples(object);
CREATE INDEX IF NOT EXISTS idx_kg_predicate ON kg_triples(predicate);
CREATE INDEX IF NOT EXISTS idx_kg_validity ON kg_triples(valid_from, valid_to);";

pub(super) fn row_to_kg_triple(row: &Row<'_>) -> rusqlite::Result<KgTriple> {
    Ok(KgTriple {
        id: row.get(0)?,
        subject: row.get(1)?,
        predicate: row.get(2)?,
        object: row.get(3)?,
        valid_from: row.get::<_, Option<String>>(4)?.map(|value| parse_dt(&value)),
        valid_to: row.get::<_, Option<String>>(5)?.map(|value| parse_dt(&value)),
        source: row.get(6)?,
        created_at: parse_dt(&row.get::<_, String>(7)?),
    })
}
