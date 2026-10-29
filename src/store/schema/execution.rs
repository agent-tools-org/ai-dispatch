// Persist execution policy, settlement and a monotonic task revision.
// SQLite triggers cover all task writers, including direct SQL mutations.
use anyhow::Result;
use rusqlite::Connection;

pub(super) fn migrate(conn: &Connection) -> Result<()> {
    for definition in [
        "revision INTEGER NOT NULL DEFAULT 1",
        "execution_policy TEXT",
        "worker_settlement TEXT",
    ] {
        super::version::add_column(conn, "tasks", definition)?;
    }
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS execution_status_revision
         AFTER UPDATE OF status ON tasks WHEN OLD.status IS NOT NEW.status
         BEGIN
           UPDATE tasks SET revision = revision + 1 WHERE id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS execution_policy_revision
         AFTER UPDATE OF execution_policy ON tasks
         BEGIN
           UPDATE tasks SET revision = revision + 1 WHERE id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS execution_worker_revision
         AFTER UPDATE OF worker_settlement ON tasks
         BEGIN
           UPDATE tasks SET revision = revision + 1 WHERE id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS execution_attribution_revision
         AFTER UPDATE OF model, observed_model, attribution_source, exit_code, completed_at ON tasks
         WHEN OLD.model IS NOT NEW.model OR OLD.observed_model IS NOT NEW.observed_model
           OR OLD.attribution_source IS NOT NEW.attribution_source
           OR OLD.exit_code IS NOT NEW.exit_code OR OLD.completed_at IS NOT NEW.completed_at
         BEGIN
           UPDATE tasks SET revision = revision + 1 WHERE id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS caller_owned_parent_insert
         BEFORE INSERT ON tasks
         WHEN EXISTS (SELECT 1 FROM tasks WHERE id = NEW.parent_task_id
                      AND execution_policy IS NOT NULL)
         BEGIN
           SELECT RAISE(ABORT, 'caller-owned parent refuses child execution');
         END;
         CREATE TRIGGER IF NOT EXISTS caller_owned_parent_update
         BEFORE UPDATE OF parent_task_id ON tasks
         WHEN EXISTS (SELECT 1 FROM tasks WHERE id = NEW.parent_task_id
                      AND execution_policy IS NOT NULL)
         BEGIN
           SELECT RAISE(ABORT, 'caller-owned parent refuses child execution');
         END;",
    )?;
    Ok(())
}


pub(super) fn sequence(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS execution_revisions (id INTEGER PRIMARY KEY AUTOINCREMENT);
         INSERT INTO execution_revisions(id) SELECT MAX(revision) FROM tasks
         HAVING MAX(revision) > COALESCE((SELECT seq FROM sqlite_sequence
                                        WHERE name = 'execution_revisions'), 0);
         DROP TRIGGER IF EXISTS execution_status_revision;
         DROP TRIGGER IF EXISTS execution_policy_revision;
         DROP TRIGGER IF EXISTS execution_worker_revision;
         DROP TRIGGER IF EXISTS execution_attribution_revision;
         DROP TRIGGER IF EXISTS execution_insert_revision;
         DROP TRIGGER IF EXISTS execution_change_revision;
         CREATE TRIGGER execution_insert_revision AFTER INSERT ON tasks BEGIN
           INSERT INTO execution_revisions(id) VALUES(NULL);
           UPDATE tasks SET revision = last_insert_rowid() WHERE id = NEW.id;
           DELETE FROM execution_revisions;
         END;
         CREATE TRIGGER execution_change_revision
         AFTER UPDATE OF status, agent, custom_agent_name, execution_policy, worker_settlement,
                         model, observed_model, attribution_source, exit_code, completed_at ON tasks
         WHEN OLD.status IS NOT NEW.status OR OLD.agent IS NOT NEW.agent
           OR OLD.custom_agent_name IS NOT NEW.custom_agent_name
           OR OLD.execution_policy IS NOT NEW.execution_policy
           OR OLD.worker_settlement IS NOT NEW.worker_settlement
           OR OLD.model IS NOT NEW.model OR OLD.observed_model IS NOT NEW.observed_model
           OR OLD.attribution_source IS NOT NEW.attribution_source
           OR OLD.exit_code IS NOT NEW.exit_code OR OLD.completed_at IS NOT NEW.completed_at
         BEGIN
           INSERT INTO execution_revisions(id) VALUES(NULL);
           UPDATE tasks SET revision = last_insert_rowid() WHERE id = NEW.id;
           DELETE FROM execution_revisions;
         END;",
    )?;
    Ok(())
}


pub(super) fn agent_evidence(conn: &Connection) -> Result<()> {
    for definition in ["agent_process TEXT", "worker_spawned INTEGER NOT NULL DEFAULT 0",
        "execution_stop INTEGER NOT NULL DEFAULT 0"] {
        super::version::add_column(conn, "tasks", definition)?;
    }
    conn.execute_batch("CREATE TRIGGER IF NOT EXISTS execution_evidence_revision
        AFTER UPDATE OF agent_process, worker_spawned, execution_stop ON tasks
        WHEN OLD.agent_process IS NOT NEW.agent_process
          OR OLD.worker_spawned IS NOT NEW.worker_spawned
          OR OLD.execution_stop IS NOT NEW.execution_stop
        BEGIN
          INSERT INTO execution_revisions(id) VALUES(NULL);
          UPDATE tasks SET revision = last_insert_rowid() WHERE id = NEW.id;
          DELETE FROM execution_revisions;
        END;")?;
    Ok(())
}
