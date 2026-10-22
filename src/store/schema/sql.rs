// Store table definitions used by creation and migrations.
// Exports SQL constants to the schema module.

pub(super) const CREATE_TABLES_SQL: &str = "CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    agent TEXT NOT NULL,
    prompt TEXT NOT NULL,
    resolved_prompt TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    parent_task_id TEXT,
    workgroup_id TEXT,
    caller_kind TEXT,
    caller_session_id TEXT,
    repo_path TEXT,
    project_id TEXT,
    worktree_path TEXT,
    effective_dir TEXT,
    backup_url TEXT,
    worktree_branch TEXT, final_head_sha TEXT, final_branch TEXT,
    start_sha TEXT,
    log_path TEXT,
    output_path TEXT,
    tokens INTEGER,
    duration_ms INTEGER,
    model TEXT,
    cost_usd REAL,
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    completion_summary TEXT,
    peer_review TEXT,
    category TEXT,
    pending_reason TEXT,
    audit_verdict TEXT, audit_report_path TEXT, delivery_assessment TEXT, dispatch_args TEXT,
    declared_difficulty TEXT, declared_budget TEXT, declared_urgency TEXT, declared_rigor TEXT,
    observed_model TEXT,
    attribution_source TEXT,
    principal_merge_override TEXT
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
);
CREATE TABLE IF NOT EXISTS findings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workgroup_id TEXT NOT NULL,
    content TEXT NOT NULL,
    source_task_id TEXT,
    severity TEXT,
    title TEXT,
    file TEXT,
    lines TEXT,
    category TEXT,
    confidence TEXT,
    verdict TEXT,
    score TEXT,
    note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_findings_workgroup ON findings(workgroup_id);
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    memory_type TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'on_demand',
    content TEXT NOT NULL,
    source_task_id TEXT,
    agent TEXT,
    project_path TEXT,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_path);
CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_hash ON memories(content_hash);
CREATE TABLE IF NOT EXISTS task_acceptance (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    decision TEXT NOT NULL CHECK(decision IN ('accepted', 'rejected')),
    decided_at TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    source TEXT NOT NULL,
    accepted_head_sha TEXT,
    accepted_branch TEXT,
    artifact_manifest_digest TEXT
);
CREATE INDEX IF NOT EXISTS idx_task_acceptance_task
    ON task_acceptance(task_id, id);
CREATE TABLE IF NOT EXISTS artifact_durability (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    checked_at TEXT NOT NULL,
    accepted_head_sha TEXT NOT NULL,
    manifest_digest TEXT NOT NULL,
    certificate_json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_artifact_durability_task
    ON artifact_durability(task_id, id);
";
pub(super) const CREATE_WORKGROUPS_SQL: &str = "CREATE TABLE IF NOT EXISTS workgroups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    shared_context TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);";
pub(super) const CREATE_MEMORIES_SQL: &str = "CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    memory_type TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'on_demand',
    content TEXT NOT NULL,
    source_task_id TEXT,
    agent TEXT,
    project_path TEXT,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project_path);
CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_hash ON memories(content_hash);";
pub(super) const CREATE_FINDINGS_SQL: &str = "CREATE TABLE IF NOT EXISTS findings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workgroup_id TEXT NOT NULL,
    content TEXT NOT NULL,
    source_task_id TEXT,
    severity TEXT,
    title TEXT,
    file TEXT,
    lines TEXT,
    category TEXT,
    confidence TEXT,
    verdict TEXT,
    score TEXT,
    note TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_findings_workgroup ON findings(workgroup_id);";
