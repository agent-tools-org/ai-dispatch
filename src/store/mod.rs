// SQLite persistence for tasks, workgroups, and events.
// Exports: Store.
// Deps: rusqlite, anyhow.

mod mutations;
pub use mutations::TaskCompletionUpdate;
mod queries;
mod schema;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct DbStats {
    pub size_bytes: u64,
    pub wal_size_bytes: u64,
    pub page_count: u64,
    pub free_pages: u64,
}

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
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        optimize_for_concurrency(&conn)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.create_tables()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        optimize_for_concurrency(&conn)?;
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
        schema::create_tables(self)?;
        self.migrate()?;
        Ok(())
    }

    /// Idempotent schema migrations for v0.2 columns
    fn migrate(&self) -> Result<()> {
        schema::migrate(self)
    }

    pub fn db_stats(&self) -> Result<DbStats> {
        let conn = self.db();
        let db_path: String = conn.query_row("PRAGMA database_list", [], |row| row.get(2))?;
        let size_bytes = std::fs::metadata(&db_path).map(|meta| meta.len()).unwrap_or(0);
        let wal_size_bytes = std::fs::metadata(format!("{db_path}-wal"))
            .map(|meta| meta.len())
            .unwrap_or(0);
        let page_count = conn.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let free_pages = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
        Ok(DbStats {
            size_bytes,
            wal_size_bytes,
            page_count,
            free_pages,
        })
    }
}
