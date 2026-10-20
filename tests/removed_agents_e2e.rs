// Removed builtins fail before launch; persisted task names remain queryable.
// Exercises CLI validation, SQLite task/event history, board, show, and stats.
// Deps: compiled aid, isolated AID_HOME, rusqlite, serde_json.
mod common;
use common::aid_cmd_in;
use rusqlite::{Connection, params};

#[test]
fn removed_agents_fail_with_migration_guidance() {
    let home = tempfile::tempdir().expect("home");
    for agent in ["gemini", "qwen"] {
        for args in [
            vec!["run", agent, "Review this module", "--no-hint"],
            vec![
                "run",
                "codex",
                "Review this module",
                "--cascade",
                agent,
                "--no-hint",
            ],
            vec!["agent", "config", agent, "--model", "test-model"],
            vec!["agent", "show", agent],
            vec!["config", "clear-limit", agent],
            vec!["ask", "Explain this module", "--agent", agent],
        ] {
            let output = aid_cmd_in(home.path()).args(&args).output().expect("aid");
            assert!(!output.status.success(), "{args:?}");
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("removed in this release; use `agy`"),
                "{args:?}: {error}"
            );
            assert!(
                error.contains("`opencode`/`kilo` or a custom agent"),
                "{error}"
            );
        }
    }
}

#[test]
fn removed_agent_history_loads_in_board_show_and_stats() {
    let home = tempfile::tempdir().expect("home");
    assert!(
        aid_cmd_in(home.path())
            .args(["board", "--json"])
            .output()
            .expect("init store")
            .status
            .success()
    );
    let db = Connection::open(home.path().join("aid.db")).expect("db");
    for agent in ["gemini", "qwen"] {
        let id = format!("t-old-{agent}");
        seed_task(&db, home.path(), agent, &id);
        assert_show(home.path(), agent, &id);
    }
    for args in [vec!["board", "--json"], vec!["stats", "--window", "all"]] {
        let output = aid_cmd_in(home.path())
            .args(args)
            .output()
            .expect("history");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8_lossy(&output.stdout);
        for agent in ["gemini", "qwen"] {
            assert!(text.contains(agent), "{text}");
        }
    }
}

fn seed_task(db: &Connection, home: &std::path::Path, agent: &str, id: &str) {
    db.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at, cost_usd)
        VALUES (?1, ?2, 'Old task', 'done', ?3, 0.25)",
        params![id, agent, chrono::Local::now().to_rfc3339()],
    )
    .expect("task");
    db.execute(
        "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
        VALUES (?1, ?2, 'completion', 'Old completion', ?3)",
        params![
            id,
            chrono::Local::now().to_rfc3339(),
            serde_json::json!({"agent": agent, "tokens": 12}).to_string()
        ],
    )
    .expect("event");
    std::fs::write(
        home.join(format!("rate-limit-{agent}")),
        "recovery_at: Jan 01, 2099 12:00 PM\nmessage: Historical quota\n",
    )
    .expect("marker");
}

fn assert_show(home: &std::path::Path, agent: &str, id: &str) {
    let output = aid_cmd_in(home)
        .args(["show", id, "--json"])
        .output()
        .expect("show");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let shown = String::from_utf8_lossy(&output.stdout);
    assert!(shown.contains(agent), "{shown}");
    let events = aid_cmd_in(home)
        .args(["show", id, "--events"])
        .output()
        .expect("events");
    assert!(events.status.success());
    assert!(String::from_utf8_lossy(&events.stdout).contains("Old completion"));
}
