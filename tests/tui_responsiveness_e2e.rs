// Real-terminal coverage for asynchronous TUI scope changes and navigation.
// Runs the compiled aid binary against isolated historical tasks, then quits during refresh.
// Deps: portable-pty, rusqlite, tempfile and common command isolation.

mod common;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct TerminalSession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: mpsc::Receiver<Vec<u8>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Drop for TerminalSession {
    fn drop(&mut self) { let _ = self.child.kill(); }
}

impl TerminalSession {
    fn start(home: &std::path::Path) -> Self {
        let pair = native_pty_system().openpty(PtySize {
            rows: 40, cols: 180, pixel_width: 0, pixel_height: 0,
        }).unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_aid"));
        command.args(["watch", "--tui"]);
        command.env("AID_HOME", home);
        command.env("TERM", "xterm-256color");
        command.cwd(home);
        for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("AID_")) {
            if name != "AID_HOME" { command.env_remove(name); }
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = [0; 8192];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 || sender.send(bytes[..count].to_vec()).is_err() { break; }
            }
        });
        Self { child, writer, output, _master: pair.master }
    }

    fn send(&mut self, keys: &[u8]) { self.writer.write_all(keys).unwrap(); }

    fn wait_for(&mut self, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut received = Vec::new();
        while Instant::now() < deadline {
            if let Ok(bytes) = self.output.recv_timeout(Duration::from_millis(50)) {
                received.extend(bytes);
                if received.windows(4).any(|part| part == b"\x1b[6n") {
                    self.send(b"\x1b[1;1R");
                }
                if received.windows(expected.len()).any(|part| part == expected.as_bytes()) {
                    return;
                }
            }
            assert!(self.child.try_wait().unwrap().is_none(), "TUI exited before {expected}");
        }
        panic!("TUI did not show {expected}: {}", String::from_utf8_lossy(&received));
    }
}

fn fixture(home: &std::path::Path) {
    std::fs::write(home.join("config.toml"), "[updates]\ncheck = false\n").unwrap();
    let initialized = common::aid_cmd_in(home).arg("board").output().unwrap();
    assert!(initialized.status.success(), "{}", String::from_utf8_lossy(&initialized.stderr));
    let database = rusqlite::Connection::open(home.join("aid.db")).unwrap();
    database.execute_batch(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<1200)
         INSERT INTO tasks(id, agent, prompt, status, created_at, project_id)
         SELECT printf('t-old%04d', i), 'codex', hex(zeroblob(8192)), 'done',
                '2020-01-01T00:00:00Z', 'history' FROM n;",
    ).unwrap();
    database.execute(
        "INSERT INTO tasks(id, agent, prompt, status, created_at, project_id)
         VALUES('t-recent', 'codex', 'Recent task', 'done', ?1, 'recent')",
        [chrono::Local::now().to_rfc3339()],
    ).unwrap();
}

#[test]
fn tui_loads_history_navigates_and_quits_during_refresh() {
    if !cfg!(unix) { return; }
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    let mut terminal = TerminalSession::start(home.path());
    terminal.wait_for("t-recent");
    terminal.send(b"a");
    terminal.wait_for("history");
    terminal.send(b"tG");
    terminal.wait_for("aid tree");
    terminal.send(b"t");
    terminal.wait_for("Route");
    terminal.send(b"r");
    std::thread::sleep(Duration::from_millis(25));
    let start = Instant::now();
    terminal.send(b"q");
    loop {
        if let Some(status) = terminal.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(2), "quit blocked by refresh");
        std::thread::sleep(Duration::from_millis(10));
    }
}
