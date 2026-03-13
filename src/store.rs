// SQLite persistence for tasks, workgroups, and events.
// Uses WAL mode for concurrent read/write. Single file at ~/.aid/aid.db.

use anyhow::{Result, Context};
use chrono::{DateTime, Local};
use rusqlite::{Connection, OptionalExtension, params};
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

    pub(crate) fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    fn create_tables(&self) -> Result<()> {
        self.db().execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                agent TEXT NOT NULL,
                prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                parent_task_id TEXT,
                workgroup_id TEXT,
                caller_kind TEXT,
                caller_session_id TEXT,
                repo_path TEXT,
                worktree_path TEXT,
                worktree_branch TEXT,
                log_path TEXT,
                output_path TEXT,
                tokens INTEGER,
                duration_ms INTEGER,
                model TEXT,
                cost_usd REAL,
                created_at TEXT NOT NULL,
                completed_at TEXT
            );
            CREATE TABLE IF NOT EXISTS workgroups (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                shared_context TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL REFERENCES tasks(id),
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                detail TEXT NOT NULL,
                metadata TEXT
            );",
        )?;
        self.migrate()?;
        Ok(())
    }

    /// Idempotent schema migrations for v0.2 columns
    fn migrate(&self) -> Result<()> {
        let conn = self.db();
        // Add columns if missing (ALTER TABLE ADD COLUMN is idempotent when wrapped in try)
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN model TEXT;");
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN cost_usd REAL;");
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN parent_task_id TEXT;");
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN workgroup_id TEXT;");
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN caller_kind TEXT;");
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN caller_session_id TEXT;");
        let _ = conn.execute_batch("ALTER TABLE tasks ADD COLUMN repo_path TEXT;");
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS workgroups (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                shared_context TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        );
        let _ = conn.execute_batch("ALTER TABLE events ADD COLUMN metadata TEXT;");
        Ok(())
    }

    pub fn insert_task(&self, task: &Task) -> Result<()> {
        self.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, repo_path, worktree_path, worktree_branch,
             log_path, output_path, tokens, duration_ms, model, cost_usd, created_at,
             completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
             ?16, ?17, ?18, ?19)",
            params![
                task.id.as_str(),
                task.agent.as_str(),
                task.prompt,
                task.status.as_str(),
                task.parent_task_id,
                task.workgroup_id,
                task.caller_kind,
                task.caller_session_id,
                task.repo_path,
                task.worktree_path,
                task.worktree_branch,
                task.log_path,
                task.output_path,
                task.tokens,
                task.duration_ms,
                task.model,
                task.cost_usd,
                task.created_at.to_rfc3339(),
                task.completed_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn create_workgroup(&self, name: &str, shared_context: &str) -> Result<Workgroup> {
        let now = Local::now();
        let workgroup = Workgroup {
            id: WorkgroupId::generate(),
            name: name.to_string(),
            shared_context: shared_context.to_string(),
            created_at: now,
            updated_at: now,
        };
        self.db().execute(
            "INSERT INTO workgroups (id, name, shared_context, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                workgroup.id.as_str(),
                workgroup.name,
                workgroup.shared_context,
                workgroup.created_at.to_rfc3339(),
                workgroup.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(workgroup)
    }

    pub fn get_workgroup(&self, id: &str) -> Result<Option<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at
             FROM workgroups WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_workgroup)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn list_workgroups(&self) -> Result<Vec<Workgroup>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, name, shared_context, created_at, updated_at
             FROM workgroups ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_workgroup)?;
        rows.map(|row| row?).collect()
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
        model: Option<&str>,
        cost_usd: Option<f64>,
    ) -> Result<()> {
        let now = Local::now().to_rfc3339();
        self.db().execute(
            "UPDATE tasks SET status = ?1, tokens = ?2, duration_ms = ?3, completed_at = ?4,
             model = ?5, cost_usd = ?6 WHERE id = ?7",
            params![status.as_str(), tokens, duration_ms, now, model, cost_usd, id],
        )?;
        Ok(())
    }

    pub fn insert_event(&self, event: &TaskEvent) -> Result<()> {
        let metadata_str = event.metadata.as_ref().map(|m| m.to_string());
        self.db().execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.task_id.as_str(),
                event.timestamp.to_rfc3339(),
                event.event_kind.as_str(),
                event.detail,
                metadata_str,
            ],
        )?;
        Ok(())
    }

    pub fn latest_milestone(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.db();
        let milestone = conn
            .query_row(
                "SELECT detail FROM events
                 WHERE task_id = ?1 AND event_type = 'milestone'
                 ORDER BY timestamp DESC
                 LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(milestone)
    }

    pub fn get_workgroup_milestones(&self, workgroup_id: &str) -> Result<Vec<(String, String)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT e.task_id, e.detail FROM events e
             JOIN tasks t ON e.task_id = t.id
             WHERE t.workgroup_id = ?1 AND e.event_type = 'milestone'
             ORDER BY e.timestamp ASC",
        )?;
        let rows = stmt.query_map(params![workgroup_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, status, parent_task_id, workgroup_id, caller_kind,
             caller_session_id, repo_path, worktree_path, worktree_branch, log_path,
             output_path, tokens, duration_ms, model, cost_usd, created_at, completed_at
             FROM tasks WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], row_to_task)?;
        match rows.next() {
            Some(row) => Ok(Some(row??)),
            None => Ok(None),
        }
    }

    pub fn get_retry_chain(&self, task_id: &str) -> Result<Vec<Task>> {
        let mut chain = Vec::new();
        let mut current = self.get_task(task_id)?;
        while let Some(task) = current {
            let parent_task_id = task.parent_task_id.clone();
            chain.push(task);
            current = match parent_task_id {
                Some(parent_id) => self.get_task(&parent_id)?,
                None => None,
            };
        }
        chain.reverse();
        Ok(chain)
    }

    pub fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        let conn = self.db();
        let (sql, filter_params): (&str, Vec<String>) = match filter {
            TaskFilter::All => (
                "SELECT id, agent, prompt, status, parent_task_id, workgroup_id, caller_kind,
                 caller_session_id, repo_path, worktree_path, worktree_branch, log_path,
                 output_path, tokens, duration_ms, model, cost_usd, created_at, completed_at
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
            TaskFilter::Running => (
                "SELECT id, agent, prompt, status, parent_task_id, workgroup_id, caller_kind,
                 caller_session_id, repo_path, worktree_path, worktree_branch, log_path,
                 output_path, tokens, duration_ms, model, cost_usd, created_at, completed_at
                 FROM tasks WHERE status IN (?1, ?2) ORDER BY created_at DESC",
                vec!["running".to_string(), "awaiting_input".to_string()],
            ),
            TaskFilter::Today => (
                "SELECT id, agent, prompt, status, parent_task_id, workgroup_id, caller_kind,
                 caller_session_id, repo_path, worktree_path, worktree_branch, log_path,
                 output_path, tokens, duration_ms, model, cost_usd, created_at, completed_at
                 FROM tasks ORDER BY created_at DESC",
                vec![],
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let params: Vec<&dyn rusqlite::ToSql> =
            filter_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), row_to_task)?;
        let mut tasks = rows.map(|r| r?).collect::<Result<Vec<_>>>()?;
        if matches!(filter, TaskFilter::Today) {
            let today = Local::now().date_naive();
            tasks.retain(|task| task.created_at.date_naive() == today);
        }
        Ok(tasks)
    }

    pub fn list_tasks_by_session(&self, session_id: &str) -> Result<Vec<Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, status, parent_task_id, workgroup_id, caller_kind,
             caller_session_id, repo_path, worktree_path, worktree_branch, log_path,
             output_path, tokens, duration_ms, model, cost_usd, created_at, completed_at
             FROM tasks WHERE caller_session_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id], row_to_task)?;
        rows.map(|row| row?).collect()
    }

    pub fn get_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT task_id, timestamp, event_type, detail, metadata
             FROM events WHERE task_id = ?1 ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |row| {
            let metadata_str: Option<String> = row.get(4)?;
            let metadata = metadata_str
                .and_then(|s| serde_json::from_str(&s).ok());
            Ok(TaskEvent {
                task_id: TaskId(row.get::<_, String>(0)?),
                timestamp: parse_dt(&row.get::<_, String>(1)?),
                event_kind: EventKind::parse_str(&row.get::<_, String>(2)?)
                    .unwrap_or(EventKind::Reasoning),
                detail: row.get(3)?,
                metadata,
            })
        })?;
        rows.map(|r| Ok(r?)).collect()
    }
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Result<Task>> {
    Ok(Ok(Task {
        id: TaskId(row.get::<_, String>(0)?),
        agent: AgentKind::parse_str(&row.get::<_, String>(1)?)
            .unwrap_or(AgentKind::Codex),
        prompt: row.get(2)?,
        status: TaskStatus::parse_str(&row.get::<_, String>(3)?)
            .unwrap_or(TaskStatus::Pending),
        parent_task_id: row.get(4)?,
        workgroup_id: row.get(5)?,
        caller_kind: row.get(6)?,
        caller_session_id: row.get(7)?,
        repo_path: row.get(8)?,
        worktree_path: row.get(9)?,
        worktree_branch: row.get(10)?,
        log_path: row.get(11)?,
        output_path: row.get(12)?,
        tokens: row.get(13)?,
        duration_ms: row.get(14)?,
        model: row.get(15)?,
        cost_usd: row.get(16)?,
        created_at: parse_dt(&row.get::<_, String>(17)?),
        completed_at: row.get::<_, Option<String>>(18)?
            .map(|s| parse_dt(&s)),
    }))
}

fn row_to_workgroup(row: &rusqlite::Row) -> rusqlite::Result<Result<Workgroup>> {
    Ok(Ok(Workgroup {
        id: WorkgroupId(row.get::<_, String>(0)?),
        name: row.get(1)?,
        shared_context: row.get(2)?,
        created_at: parse_dt(&row.get::<_, String>(3)?),
        updated_at: parse_dt(&row.get::<_, String>(4)?),
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
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            log_path: None,
            output_path: None,
            tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
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
    fn migrate_adds_repo_path_column() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tasks (
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
            CREATE TABLE events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                detail TEXT NOT NULL
            );",
        )
        .unwrap();
        let store = Store {
            conn: std::sync::Mutex::new(conn),
        };

        store.migrate().unwrap();

        let conn = store.db();
        let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|row| row.unwrap())
            .collect::<Vec<_>>();
        assert!(columns.contains(&"repo_path".to_string()));
    }

    #[test]
    fn update_completion() {
        let store = Store::open_memory().unwrap();
        let task = make_task("t-0002", AgentKind::Gemini, TaskStatus::Running);
        store.insert_task(&task).unwrap();
        store.update_task_completion(
            "t-0002", TaskStatus::Done, Some(3000), 47000,
            Some("gemini-2.5-flash"), Some(0.0038),
        ).unwrap();

        let loaded = store.get_task("t-0002").unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Done);
        assert_eq!(loaded.tokens, Some(3000));
        assert_eq!(loaded.duration_ms, Some(47000));
        assert_eq!(loaded.model.as_deref(), Some("gemini-2.5-flash"));
        assert!((loaded.cost_usd.unwrap() - 0.0038).abs() < 0.0001);
        assert!(loaded.completed_at.is_some());
    }

    #[test]
    fn list_running_filter() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-0010", AgentKind::Codex, TaskStatus::Running)).unwrap();
        store.insert_task(&make_task("t-0012", AgentKind::Cursor, TaskStatus::AwaitingInput)).unwrap();
        store.insert_task(&make_task("t-0011", AgentKind::Gemini, TaskStatus::Done)).unwrap();

        let running = store.list_tasks(TaskFilter::Running).unwrap();
        assert_eq!(running.len(), 2);
        let ids = running.into_iter().map(|task| task.id.0).collect::<Vec<_>>();
        assert!(ids.contains(&"t-0010".to_string()));
        assert!(ids.contains(&"t-0012".to_string()));
    }

    #[test]
    fn gets_retry_chain_from_root_to_current() {
        let store = Store::open_memory().unwrap();
        let root = make_task("t-1001", AgentKind::Codex, TaskStatus::Done);
        let mut retry_1 = make_task("t-1002", AgentKind::Codex, TaskStatus::Failed);
        retry_1.parent_task_id = Some("t-1001".to_string());
        let mut retry_2 = make_task("t-1003", AgentKind::Codex, TaskStatus::Done);
        retry_2.parent_task_id = Some("t-1002".to_string());

        store.insert_task(&root).unwrap();
        store.insert_task(&retry_1).unwrap();
        store.insert_task(&retry_2).unwrap();

        let chain = store.get_retry_chain("t-1003").unwrap();
        let ids = chain.iter().map(|task| task.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, vec!["t-1001", "t-1002", "t-1003"]);
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
            metadata: Some(serde_json::json!({"tool": "exec_command"})),
        };
        store.insert_event(&event).unwrap();

        let events = store.get_events("t-0020").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_kind, EventKind::ToolCall);
        assert!(events[0].metadata.is_some());
    }

    #[test]
    fn create_and_get_workgroup() {
        let store = Store::open_memory().unwrap();
        let workgroup = store
            .create_workgroup("dispatch", "Shared API contract context.")
            .unwrap();

        let loaded = store
            .get_workgroup(workgroup.id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(loaded.id, workgroup.id);
        assert_eq!(loaded.name, "dispatch");
        assert!(loaded.shared_context.contains("API contract"));
    }

    #[test]
    fn gets_latest_milestone() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-0030", AgentKind::Codex, TaskStatus::Running)).unwrap();

        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0030".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(2),
            event_kind: EventKind::Milestone,
            detail: "types defined".to_string(),
            metadata: None,
        }).unwrap();
        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0030".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(1),
            event_kind: EventKind::ToolCall,
            detail: "cargo check".to_string(),
            metadata: None,
        }).unwrap();
        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0030".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "tests passing".to_string(),
            metadata: None,
        }).unwrap();

        let milestone = store.latest_milestone("t-0030").unwrap();
        assert_eq!(milestone.as_deref(), Some("tests passing"));
    }

    #[test]
    fn gets_workgroup_milestones() {
        let store = Store::open_memory().unwrap();
        let workgroup = store
            .create_workgroup("dispatch", "Shared API contract context.")
            .unwrap();

        let mut first = make_task("t-0040", AgentKind::Codex, TaskStatus::Running);
        first.workgroup_id = Some(workgroup.id.as_str().to_string());
        store.insert_task(&first).unwrap();

        let mut second = make_task("t-0041", AgentKind::Gemini, TaskStatus::Running);
        second.workgroup_id = Some(workgroup.id.as_str().to_string());
        store.insert_task(&second).unwrap();

        let mut other = make_task("t-0042", AgentKind::Cursor, TaskStatus::Running);
        other.workgroup_id = Some("wg-other".to_string());
        store.insert_task(&other).unwrap();

        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0040".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(3),
            event_kind: EventKind::Milestone,
            detail: "finding one".to_string(),
            metadata: None,
        }).unwrap();
        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0040".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(2),
            event_kind: EventKind::ToolCall,
            detail: "ignored".to_string(),
            metadata: None,
        }).unwrap();
        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0041".to_string()),
            timestamp: Local::now() - chrono::Duration::seconds(1),
            event_kind: EventKind::Milestone,
            detail: "finding two".to_string(),
            metadata: None,
        }).unwrap();
        store.insert_event(&TaskEvent {
            task_id: TaskId("t-0042".to_string()),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "other group".to_string(),
            metadata: None,
        }).unwrap();

        let milestones = store
            .get_workgroup_milestones(workgroup.id.as_str())
            .unwrap();
        assert_eq!(
            milestones,
            vec![
                ("t-0040".to_string(), "finding one".to_string()),
                ("t-0041".to_string(), "finding two".to_string()),
            ]
        );
    }
}
