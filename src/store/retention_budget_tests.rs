// Retention concurrency and deadline regressions, using isolated SQLite stores.
// Verifies lock-free parsing, bounded mutex/SQL work, and oversized-row restart.
use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Instant,
};

fn seed(store: &Store, bytes: usize) {
    store
        .db()
        .execute_batch(
            "INSERT INTO tasks (id,agent,prompt,status,created_at,completed_at)
         VALUES ('old','codex','test','done','2020-01-01','2020-01-01');",
        )
        .unwrap();
    store
        .db()
        .execute(
            "INSERT INTO events (task_id,timestamp,event_type,detail,metadata)
         VALUES ('old','2020-01-01','build','output',?1)",
            [serde_json::json!({"full": "x".repeat(bytes)}).to_string()],
        )
        .unwrap();
}

#[test]
fn busy_store_returns_within_budget_without_waiting_for_mutex() {
    let store = Arc::new(Store::open_memory().unwrap());
    seed(&store, 5000);
    let held = store.db();
    let (send, receive) = mpsc::channel();
    let worker_store = Arc::clone(&store);
    let worker = std::thread::spawn(move || {
        send.send(worker_store.trim_event_payloads(
            "2021-01-01",
            0,
            false,
            Duration::from_millis(10),
        ))
        .unwrap();
    });
    let report = receive.recv_timeout(Duration::from_secs(1));
    drop(held);
    worker.join().unwrap();
    let report = report
        .expect("retention waited for the held Store mutex")
        .unwrap();
    assert_eq!(report.cursor, 0);
    assert_eq!(report.rows, 0);
    assert!(!report.complete);
}

#[test]
fn json_parsing_releases_store_for_other_event_writes() {
    let store = Arc::new(Store::open_memory().unwrap());
    seed(&store, 5000);
    let mut deadline = Deadline::new(Duration::from_secs(10));
    let writer = Arc::clone(&store);
    let observed = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&observed);
    deadline.before_parse = Some(Box::new(move || {
        let conn = writer
            .conn
            .try_lock()
            .expect("JSON parsing holds the Store mutex");
        conn.execute("UPDATE tasks SET prompt='event writer' WHERE id='old'", [])
            .unwrap();
        count.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(
        trim(&store, "2021-01-01", 0, false, &deadline)
            .unwrap()
            .rows,
        1
    );
    assert!(observed.load(Ordering::SeqCst) > 0);
}

#[test]
fn oversized_row_obeys_deadline_and_resumes_without_skipping() {
    let store = Store::open_memory().unwrap();
    seed(&store, 8 * 1024 * 1024);
    let row = candidates(&store.db(), 0, "2021-01-01").unwrap().remove(0);
    assert!(
        row.metadata.is_none(),
        "oversized metadata was copied under the read mutex"
    );
    let start = Instant::now();
    let report = store
        .trim_event_payloads("2021-01-01", 0, false, Duration::from_millis(1))
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!((report.rows, report.cursor), (0, 0));
    let resumed = store
        .trim_event_payloads("2021-01-01", report.cursor, false, Duration::from_secs(30))
        .unwrap();
    assert_eq!(resumed.rows, 1);
    assert!(resumed.complete);
}

#[test]
fn sqlite_work_is_interrupted_and_connection_settings_are_restored() {
    let store = Store::open_memory().unwrap();
    let start = Instant::now();
    let result = with_conn(&store, &Deadline::new(Duration::from_millis(2)), |conn| {
        conn.query_row(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000)
             SELECT sum(x) FROM n",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(())
    })
    .unwrap();
    assert!(result.is_none());
    assert!(start.elapsed() < Duration::from_secs(1));
    let timeout: u64 = store
        .db()
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 10000);
    assert_eq!(
        store
            .db()
            .query_row("SELECT 1", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn expired_deadlines_cancel_json_scans_copies_and_utf8_validation() {
    let raw = format!("{{\"full\":\"{}\"}}", "x".repeat(5000));
    let expired = Deadline::new(Duration::ZERO);
    assert!(expired.parse(&raw).is_none());
    assert!(payload::replace_string(&raw, "full", "{}", &expired).is_none());
    assert!(expired.copy(&mut String::new(), &raw).is_none());
    assert!(expired.utf8(raw.into_bytes()).is_none());
    let live = Deadline::new(Duration::from_secs(10));
    let unicode = "xé😀".repeat(4096);
    assert_eq!(live.utf8(unicode.clone().into_bytes()), Some(unicode));
    assert!(live.utf8(vec![255]).is_none());
}
