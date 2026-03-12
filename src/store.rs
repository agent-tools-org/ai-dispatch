// SQLite persistence for tasks and events.
// Uses WAL mode for concurrent read/write. Single file at ~/.aid/aid.db.

use anyhow::{Result, Context};
use chrono::{DateTime, Local};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

use crate::types::*;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let store = Self { conn: Mutex::new(conn) };
        store.create_tables()?;
        Ok(store)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let store = Self { conn: Mutex::new(conn) };
        store.create_tables()?;
        Ok(store)
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    fn create_tables(&self) -> Result<()> {
        self.db().execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                agent TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                worktree_path TEXT,
                worktree_branch TEXT,
                log_path TEXT,
                output_path TEXT,
                tokens INTEGER,
                duration_ms INTEGER,
                created_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES tasks(id),
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                detail TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    pub fn insert_task(&self, task: &Task) -> Result<()> {
        self.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, worktree_path, worktree_branch,
             log_path, output_path, tokens, duration_ms, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                task.id.as_str(),
                task.agent.as_str(),
                task.prompt,
                task.status.as_str(),
                task.worktree_path,
                task.worktree_branch,
                task.log_path,
                task.output_path,
                task.tokens,
                task.duration_ms,
                task.created_at.to_rfc3339(),
                task.completed_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn update_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        self.db().execute(
            "UPDATE tasks SET status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn update_task_completion(
        &self,
        id: &str,
        status: TaskStatus,
        tokens: Option<i64>,
        duration_ms: i64,
    ) -> Result<()> {
        let now = Local::now().to_rfc3339();
        self.db().execute(
            "UPDATE tasks SET status = ?1, tokens = ?2, duration_ms = ?3, completed_at = ?4
             WHERE id = ?5",
            params![status.as_str(), tokens, duration_ms, now, id],
        )?;
        Ok(())
    }

    pub fn insert_event(&self, event: &TaskEvent) -> Result<()> {
        self.db().execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                event.task_id.as_str(),
                event.timestamp.to_rfc3339(),
                event.event_kind.as_str(),
                event.detail,
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, status, worktree_path, worktree_branch,
             log_path, output_path, tokens, duration_ms, created_at, completed_at
             FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_task)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        let conn = self.db();
        let (sql, filter_params): (&str, Vec<String>) = match filter {
            TaskFilter::All => (
                "SELECT id, agent, prompt, status, worktree_path, worktree_branch,
                 log_path, output_path, tokens, duration_ms, created_at, completed_at
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
            TaskFilter::Running => (
                "SELECT id, agent, prompt, status, worktree_path, worktree_branch,
                 log_path, output_path, tokens, duration_ms, created_at, completed_at
                 FROM tasks WHERE status = ?1 ORDER BY created_at DESC",
                vec!["running".to_string()],
            ),
            TaskFilter::Today => (
                "SELECT id, agent, prompt, status, worktree_path, worktree_branch,
                 log_path, output_path, tokens, duration_ms, created_at, completed_at
                 FROM tasks WHERE date(created_at) = date('now', 'localtime')
                 ORDER BY created_at DESC",
                vec![],
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            filter_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), row_to_task)?;
        rows.map(|r| r?.map_err(Into::into)).collect::<Result<Vec<_>>>()
    }

    pub fn get_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT task_id, timestamp, event_type, detail
             FROM events WHERE task_id = ?1 ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| {
            Ok(TaskEvent {
                task_id: TaskId(row.get::<_, String>(0)?),
                timestamp: parse_dt(&row.get::<_, String>(1)?),
                event_kind: EventKind::from_str(&row.get::<_, String>(2)?)
                    .unwrap_or(EventKind::Reasoning),
                detail: row.get(3)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Result<Task>> {
    Ok(Ok(Task {
        id: TaskId(row.get::<_, String>(0)?),
        agent: AgentKind::from_str(&row.get::<_, String>(1)?)
            .unwrap_or(AgentKind::Codex),
        prompt: row.get(2)?,
        status: TaskStatus::from_str(&row.get::<_, String>(3)?)
            .unwrap_or(TaskStatus::Pending),
        worktree_path: row.get(4)?,
        worktree_branch: row.get(5)?,
        log_path: row.get(6)?,
        output_path: row.get(7)?,
        tokens: row.get(8)?,
        duration_ms: row.get(9)?,
        created_at: parse_dt(&row.get::<_, String>(10)?),
        completed_at: row.get::<_, Option<String>>(11)?
            .map(|s| parse_dt(&s)),
    }))
}

fn parse_dt(s: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent,
            prompt: "test prompt".to_string(),
            status,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            duration_ms: None,
            created_at: Local::now(),
            completed_at: None,
        }
    }

    #[test]
    fn insert_and_get_task() {
        let store = Store::open_memory().unwrap();
        let task = make_task("t-0001", AgentKind::Codex, TaskStatus::Running);
        store.insert_task(&task).unwrap();

        let loaded = store.get_task("t-0001").unwrap().unwrap();
        assert_eq!(loaded.id, task.id);
        assert_eq!(loaded.agent, AgentKind::Codex);
        assert_eq!(loaded.status, TaskStatus::Running);
    }

    #[test]
    fn update_completion() {
        let store = Store::open_memory().unwrap();
        let task = make_task("t-0002", AgentKind::Gemini, TaskStatus::Running);
        store.insert_task(&task).unwrap();
        store.update_task_completion("t-0002", TaskStatus::Done, Some(3000), 47000).unwrap();

        let loaded = store.get_task("t-0002").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Done);
        assert_eq!(loaded.tokens, Some(3000));
        assert_eq!(loaded.duration_ms, Some(47000));
        assert!(loaded.completed_at.is_some());
    }

    #[test]
    fn list_running_filter() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-0010", AgentKind::Codex, TaskStatus::Running)).unwrap();
        store.insert_task(&make_task("t-0011", AgentKind::Gemini, TaskStatus::Done)).unwrap();

        let running = store.list_tasks(TaskFilter::Running).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id.as_str(), "t-0010");
    }

    #[test]
    fn insert_and_get_events() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-0020", AgentKind::Codex, TaskStatus::Running)).unwrap();

        let event = TaskEvent {
            task_id: TaskId("t-0020".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::ToolCall,
            detail: "exec: cargo test".to_string(),
        };
        store.insert_event(&event).unwrap();

        let events = store.get_events("t-0020").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_kind, EventKind::ToolCall);
    }
}
