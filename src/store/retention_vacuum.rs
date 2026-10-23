// Explicit database compaction and idle/free-space admission checks.
// Separate from deadline-bounded payload retention; extends Store without schema changes.
use super::{Store, TERMINAL};
use anyhow::{Result, bail};
use rusqlite::Connection;

impl Store {
    pub(crate) fn vacuum_events(&self) -> Result<(u64, u64)> {
        let conn = self.db();
        let path: String = conn.query_row("PRAGMA database_list", [], |row| row.get(2))?;
        let before = std::fs::metadata(&path)?.len();
        let available = crate::support::system_resources::available_disk_mb(&path)
            .ok_or_else(|| anyhow::anyhow!("Cannot determine free disk space for VACUUM"))?;
        require_space(before, available)?;
        conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE;")?;
        let result = conn
            .execute_batch("BEGIN EXCLUSIVE;")
            .map_err(anyhow::Error::from)
            .and_then(|()| vacuum_idle(&conn));
        if !conn.is_autocommit() {
            conn.execute_batch("ROLLBACK;")?;
        }
        conn.execute_batch(
            "PRAGMA locking_mode=NORMAL; BEGIN; SELECT count(*) FROM sqlite_schema; COMMIT;",
        )?;
        result?;
        Ok((before, std::fs::metadata(path)?.len()))
    }
}

pub(super) fn require_space(size: u64, available_mb: u64) -> Result<()> {
    let required = size.saturating_mul(2).div_ceil(1024 * 1024);
    if available_mb < required {
        bail!(
            "VACUUM needs temporary free disk space up to twice the database size: \
               {required} MB required, {available_mb} MB available"
        );
    }
    Ok(())
}

fn vacuum_idle(conn: &Connection) -> Result<()> {
    let active: i64 = conn.query_row(
        &format!("SELECT count(*) FROM tasks WHERE status NOT IN ({TERMINAL})"),
        [],
        |row| row.get(0),
    )?;
    if active > 0 {
        bail!("VACUUM refused: {active} tasks are running, waiting, or otherwise active");
    }
    conn.execute_batch("COMMIT; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}
