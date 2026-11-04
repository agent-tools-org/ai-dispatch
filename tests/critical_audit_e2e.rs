// Exercises critical audit verdicts through the real foreground settlement path.
// Shell agents and aic processes isolate external services while preserving lifecycle code.

#[path = "fixtures/aic_shim.rs"]
mod aic_shim;
mod common;
#[path = "common/show_patch.rs"]
mod fixture;
use fixture::Fixture;
use std::fs;

fn setup(code: i32) -> (Fixture, std::ffi::OsString) {
    let f = Fixture::new();
    let bin = f.home.path().join("bin");
    common::write_executable(
        &bin.join("audited-agent"),
        "#!/bin/sh\nprintf '%s\n' '{\"type\":\"completion\",\"finalText\":\"done\"}'\n",
    );
    common::write_executable(
        &bin.join("aic"),
        &aic_shim::script(&format!("printf 'report: /tmp/audit.md\\n'\nexit {code}")),
    );
    fs::create_dir(f.home.path().join("agents")).unwrap();
    fs::write(
        f.home.path().join("agents/audited.toml"),
        format!(
            "[agent]\nid = 'audited'\ndisplay_name = 'audited'\ncommand = '{}'\n\
         trust_tier = 'local'\nstreaming = true\noutput_format = 'jsonl'\n",
            bin.join("audited-agent").display(),
        ),
    )
    .unwrap();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    (f, path)
}

fn run_case(code: i32, verdict: &str, critical: bool) {
    let (f, path) = setup(code);
    let mut command = common::aid_cmd_in(f.home.path());
    command
        .env("PATH", path)
        .args([
            "run",
            "audited",
            "Inspect the code",
            "--id",
            "t-audited",
            "--audit",
            "--verify",
            "true",
            "--no-hint",
            "--dir",
        ])
        .arg(&f.repo);
    if critical {
        command.args(["--rigor", "critical"]);
    }
    let output = command.output().unwrap();
    let failed = critical && verdict == "fail";
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), !failed, "{stderr}");
    let (status, saved, report): (String, String, String) =
        f.db.query_row(
            "SELECT status, audit_verdict, audit_report_path FROM tasks WHERE id = 't-audited'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, if failed { "failed" } else { "done" });
    assert_eq!(saved, verdict);
    assert_eq!(report, "/tmp/audit.md");
    if failed || (critical && matches!(verdict, "inconclusive" | "error")) {
        assert_event(&f, verdict, failed);
    }
}

fn assert_event(f: &Fixture, verdict: &str, failed: bool) {
    let expected = if failed {
        "critical cross-audit verdict: fail"
    } else {
        "No audit verdict for code"
    };
    let detail: String =
        f.db.query_row(
            "SELECT detail FROM events WHERE task_id = 't-audited' AND event_type = 'error'
             AND detail LIKE ?1",
            [format!("{expected}%")],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        detail.contains(verdict) && detail.contains("/tmp/audit.md"),
        "{detail}"
    );
    let shown = common::aid_cmd_in(f.home.path())
        .args(["show", "t-audited", "--events"])
        .output()
        .unwrap();
    assert!(shown.status.success());
    assert!(String::from_utf8_lossy(&shown.stdout).contains(expected));
    if !failed {
        let shown = common::aid_cmd_in(f.home.path())
            .args(["show", "t-audited"])
            .output()
            .unwrap();
        assert!(shown.status.success());
        let text = String::from_utf8_lossy(&shown.stdout);
        assert!(text.contains("DONE"), "{text}");
        assert!(text.contains(&format!("Audit: {verdict}")), "{text}");
        assert!(text.contains(&format!("No audit verdict for code: {verdict}")), "{text}");
    }
}

#[test]
fn critical_fail_fails_through_real_settlement() {
    run_case(1, "fail", true);
}

#[test]
fn critical_inconclusive_records_warning_and_stays_done() {
    run_case(3, "inconclusive", true);
}

#[test]
fn critical_error_records_warning_and_stays_done() {
    run_case(4, "error", true);
}

#[test]
fn critical_pass_and_advisory_and_noncritical_fail_stay_done() {
    run_case(0, "pass", true);
    run_case(2, "advisory", true);
    run_case(1, "fail", false);
}
