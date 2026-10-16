// Command-path coverage for quota JSON and credential identifier isolation.
// Uses the real aid binary with temporary route configuration and cache directories.
use serde_json::Value;
use tempfile::TempDir;

mod common;

fn quota(home: &TempDir, json: bool) -> String {
    let mut cmd = common::aid_cmd_in(home.path());
    cmd.env("AID_QUOTA_REFRESH", "0")
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .args(["agent", "quota"]);
    if json {
        cmd.arg("--json");
    }
    let output = cmd.output().expect("run quota");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 quota")
}

fn configured_home() -> TempDir {
    let home = TempDir::new().expect("home");
    std::fs::write(
        home.path().join("config.toml"),
        concat!(
            "[[api_route]]\nid = 'claude-api-test'\nagent = 'claude'\n",
            "keychain_service = 'quota-private-service-fixture'\n",
            "keychain_account = 'quota-private-account-fixture'\n",
            "monthly_budget_usd = 200\n",
        ),
    )
    .expect("config");
    home
}

#[test]
fn quota_json_command_includes_configured_route_and_matches_text_rows() {
    use chrono::{Datelike, Timelike};
    let home = configured_home();
    let output = quota(&home, true);
    let rows: Vec<Value> = serde_json::from_str(&output).expect("JSON array");
    let route = rows
        .iter()
        .find(|row| row["id"] == "claude-api-test")
        .expect("route");
    assert_eq!(route["kind"], "api_route");
    assert_eq!(route["spend_usd"], 0.0);
    assert_eq!(route["budget_usd"], 200.0);
    assert_eq!(route["unpriced_tasks"], 0);
    assert_eq!(route["state"], "in_plan");
    assert_eq!(route["pace_penalty"], false);
    assert_eq!(route["hold"], Value::Null);
    assert_eq!(route.as_object().expect("route object").len(), 10);
    let month = chrono::NaiveDate::parse_from_str(
        &format!("{}-01", route["month"].as_str().expect("month")),
        "%Y-%m-%d",
    )
    .expect("UTC month");
    let reset = chrono::DateTime::parse_from_rfc3339(route["resets_at"].as_str().expect("reset"))
        .expect("RFC 3339");
    assert_eq!(reset.date_naive(), month + chrono::Months::new(1));
    assert_eq!(
        (reset.day(), reset.hour(), reset.minute(), reset.second()),
        (1, 0, 0, 0)
    );
    assert_eq!(reset.offset().local_minus_utc(), 0);
    let text = quota(&home, false);
    assert_eq!(text.lines().skip(1).count(), rows.len());
    assert!(text.contains("IN PLAN"));
    assert!(text.contains("$0.00 / $200 this month"));
    for row in rows.iter().filter(|row| row["kind"] == "agent") {
        let id = row["id"].as_str().expect("id");
        let line = text
            .lines()
            .find(|line| line.split_whitespace().next() == Some(id))
            .expect("row");
        assert!(line.contains(row["status"].as_str().expect("status")));
        assert!(line.contains(row["detail"].as_str().expect("detail")));
    }
}

#[test]
fn quota_json_never_exposes_keychain_identifiers() {
    let home = configured_home();
    let output = quota(&home, true);
    let _: Vec<Value> = serde_json::from_str(&output).expect("JSON array");
    for forbidden in [
        "quota-private-service-fixture",
        "quota-private-account-fixture",
        "keychain",
        "api_key",
    ] {
        assert!(
            !output.contains(forbidden),
            "credential field or identifier leaked"
        );
    }
}
