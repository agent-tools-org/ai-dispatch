// SQLite persistence for tasks, workgroups, and events.
// Exports: Store.
// Deps: rusqlite, anyhow.

mod mutations;
mod queries;
mod schema;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")?;
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
}
