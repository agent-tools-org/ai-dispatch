// Atomic schema upgrades and the read-only current-version admission path.
// Exports migration coordination and checked column additions; depends on rusqlite.
use anyhow::{Result, bail};
use rusqlite::{Connection, Transaction, TransactionBehavior};

pub(in crate::store) const CURRENT_VERSION: i64 = 8;

pub(in crate::store) fn ensure_current(conn: &Connection) -> Result<()> {
    let version = schema_version(conn)?;
    if version == CURRENT_VERSION && has_private_launch_table(conn)? {
        return Ok(());
    }
    if version > CURRENT_VERSION {
        bail!("Database schema version {version} exceeds supported version {CURRENT_VERSION}");
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let version = schema_version(&tx)?;
    // Append future `if version < N` steps after this block and bump CURRENT_VERSION.
    if version < 2 {
        super::create_tables(&tx)?;
        super::migrate(&tx)?;
        crate::store::usage_checkpoint::create_table(&tx)?;
        crate::store::migrations::migrate_billing_index(&tx)?;
        if version < 1 {
            crate::store::migrations::migrate_backup_url(&tx)?;
        }
    }
    if version < 3 {
        tx.execute_batch("DROP INDEX IF EXISTS idx_events_task_id_id;")?;
    }
    if version < 4 {
        super::execution::migrate(&tx)?;
    }
    if version < 5 {
        super::execution::sequence(&tx)?;
    }
    if version < 6 {
        super::execution::agent_evidence(&tx)?;
    }
    if version < 7 {
        super::execution::sequence(&tx)?;
        tx.execute_batch("DROP TRIGGER IF EXISTS execution_evidence_revision;")?;
        super::execution::agent_evidence(&tx)?;
    }
    if version < 8 {
        super::execution::sequence(&tx)?;
    }
    if !has_private_launch_table(&tx)? {
        crate::store::read_only_launch::create_table(&tx)?;
    }
    if version < CURRENT_VERSION {
        tx.pragma_update(None, "user_version", CURRENT_VERSION)?;
    }
    tx.commit()?;
    Ok(())
}

fn has_private_launch_table(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema
         WHERE type = 'table' AND name = 'task_read_only_launch')",
        [],
        |row| row.get(0),
    )?)
}

fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

pub(in crate::store) fn add_column(conn: &Connection, table: &str, definition: &str) -> Result<()> {
    let name = definition
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing column"))?;
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == name {
            return Ok(());
        }
    }
    conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {definition};"))?;
    Ok(())
}
