// SQLite persistence for tasks, workgroups, and events.
// Exports: Store.
// Deps: rusqlite, anyhow.

mod artifact_custody;
mod completion_mutations;
mod memory_mutations;
#[cfg(test)]
mod mutation_tests;
mod mutations;
mod status_mutations;
mod task_insert_mutations;
mod task_metadata_mutations;
mod usage_checkpoint;
pub use artifact_custody::{AcceptanceDecision, AcceptanceRecord};
pub use mutations::TaskCompletionUpdate;
mod execution_mutations;
pub(crate) mod execution_protocol;
mod kg_mutations;
mod kg_queries;
mod kg_schema;
mod kg_types;
mod migrations;
mod queries;
mod read_only_launch;
mod route_launch;
mod schema;
mod schema_rows;
mod task_profile;
pub use kg_types::{KgStats, KgTriple};
pub use queries::TaskStatsRow;
pub(crate) use queries::TaskReadFilter;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Mutex;

pub struct Store {
    conn: Mutex<Connection>,
}

pub fn optimize_for_concurrency(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA busy_timeout=10000;
         PRAGMA journal_size_limit=67108864;
         PRAGMA mmap_size=268435456;
         PRAGMA cache_size=-8000;",
    )?;
    Ok(())
}

impl Store {
    pub fn open_read_only(path: &Path) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("Failed to open database read-only at {}", path.display()))?;
        Ok(Some(Self {
            conn: Mutex::new(conn),
        }))
    }

    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        optimize_for_concurrency(&conn)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_tables()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        optimize_for_concurrency(&conn)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_tables()?;
        Ok(store)
    }

    pub(crate) fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    fn create_tables(&self) -> Result<()> {
        self.migrate()
    }

    fn migrate(&self) -> Result<()> {
        schema::ensure_current(&self.db())
    }

}

pub(crate) mod state;
#[cfg(test)]
mod state_tests;
pub(crate) mod workgroups;
