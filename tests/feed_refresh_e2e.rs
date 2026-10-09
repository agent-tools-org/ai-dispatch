// Feed downloads survive short CLI commands and are adopted on the next invocation.
// Uses delayed fake curl and committed relay fixtures, with isolated AID_HOME.
// Deps: compiled aid, Unix FIFOs, libc, serde_json, tempfile.

#![cfg(unix)]

mod common;

use std::{ffi::CString, fs, io::Read, os::unix::fs::PermissionsExt, path::Path, sync::mpsc};

const CURL: &str = r#"#!/bin/sh
target=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) target="$2"; shift 2 ;;
        --max-time) shift 2 ;;
        *) url="$1"; shift ;;
    esac
done
file=${url##*/}
case "$file" in scores.json|prices.json) ;; *) exit 1 ;; esac
ps -o sid= -p $$ > "$AID_HOME/$file.session"
sleep 1
if [ -n "$target" ]; then
    cp "$AID_HOME/fixtures/$file" "$target" || exit 1
else
    cat "$AID_HOME/fixtures/$file" || :
    printf done > "$AID_HOME/$file.done"
fi
"#;

const MV: &str = r#"#!/bin/sh
/bin/mv "$@" || exit 1
file=${2##*/}
file=${file%.ready}
printf done > "$AID_HOME/$file.done"
"#;

fn setup(home: &Path) {
    fs::create_dir(home.join("bin")).expect("bin");
    fs::create_dir(home.join("fixtures")).expect("fixtures");
    for (name, script) in [("curl", CURL), ("mv", MV)] {
        fs::write(home.join("bin").join(name), script).expect("script");
        fs::set_permissions(
            home.join("bin").join(name),
            fs::Permissions::from_mode(0o755),
        )
        .expect("mode");
    }
    fs::write(home.join("config.toml"), "[updates]\ncheck = false\n").expect("config");
    for (file, body) in [
        (
            "scores.json",
            include_str!("fixtures/leaderboard/e2e-codex-scores.json"),
        ),
        (
            "prices.json",
            include_str!("fixtures/leaderboard/e2e-codex-prices.json"),
        ),
    ] {
        let mut feed: serde_json::Value = serde_json::from_str(body).expect("fixture");
        feed["built_at"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
        fs::write(home.join("fixtures").join(file), feed.to_string()).expect("fixture");
    }
}

fn completions(home: &Path) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    for file in ["scores.json", "prices.json"] {
        let fifo = home.join(format!("{file}.done"));
        let name = CString::new(fifo.as_os_str().as_encoded_bytes()).expect("fifo path");
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let sender = sender.clone();
        std::thread::spawn(move || {
            let mut body = String::new();
            fs::File::open(fifo)
                .expect("fifo")
                .read_to_string(&mut body)
                .expect("done");
            assert_eq!(body, "done");
            sender.send(file.to_string()).expect("completion");
        });
    }
    receiver
}

fn board(home: &Path) {
    let mut paths = vec![home.join("bin")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH"),
    ));
    let output = common::aid_cmd_in(home)
        .env("PATH", std::env::join_paths(paths).expect("PATH"))
        .args(["board", "--json"])
        .output()
        .expect("board");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn short_command_refreshes_both_feeds_after_parent_exit() {
    let home = tempfile::Builder::new()
        .prefix("aid feeds 'quoted'; ")
        .tempdir()
        .expect("home");
    setup(home.path());
    let done = completions(home.path());
    board(home.path());
    assert!(
        done.try_recv().is_err(),
        "board must exit before delayed curl finishes"
    );
    for _ in 0..2 {
        done.recv_timeout(std::time::Duration::from_secs(15))
            .expect("fake curl finished");
    }
    board(home.path());
    for file in ["scores.json", "prices.json"] {
        let bytes = fs::read(home.path().join(file))
            .unwrap_or_else(|error| panic!("{file} missing after short command refresh: {error}"));
        let feed: serde_json::Value = serde_json::from_slice(&bytes).expect("valid cache JSON");
        assert!(!feed["models"].as_array().expect("models").is_empty());
        assert_eq!(feed["stale"], false);
        let age = feed["age_seconds"].as_i64().expect("server age");
        assert!((0..=24 * 60 * 60).contains(&age));
        assert_eq!(
            bytes,
            fs::read(home.path().join("fixtures").join(file)).expect("fixture")
        );
        assert!(!home.path().join(format!("{file}.ready")).exists());
        assert!(!home.path().join(format!("{file}.part")).exists());
        let session: i32 = fs::read_to_string(home.path().join(format!("{file}.session")))
            .expect("curl session")
            .trim()
            .parse()
            .expect("session id");
        assert_ne!(
            session,
            unsafe { libc::getsid(0) },
            "download must own its session"
        );
    }
}
