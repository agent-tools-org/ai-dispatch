// Full CLI cascades retain advice models, profiles and retry parent linkage.
// Also provides a test-only baseline reproducer via actual known-default argv.
// Deps: controlled CLI harness, compiled aid and SQLite.
#![cfg(unix)]
mod common;
#[path = "common/advised_cascade.rs"]
mod harness;
use harness::{Harness, PROFILE, PROMPT, success};

#[test]
fn quota_continuation_launches_first_advised_peer_with_exact_model_profile_and_parent() {
    for sticky in [false, true] {
        let h = Harness::new(Some("gpt-6-sol"), sticky);
        let candidate = h.first_peer().expect("launchable peer");
        assert_eq!(candidate["agent"], "codex");
        success(&h.run().output().expect("run"));
        let rows = h.rows();
        assert_eq!(rows.len(), 2);
        let child = rows
            .iter()
            .find(|r| r.parent.as_deref() == Some("t-primary"))
            .expect("continuation");
        h.assert_route(child, &candidate);
        let parent = rows.iter().find(|r| r.id == "t-primary").expect("parent");
        assert_eq!(parent.agent, "oz");
        assert_eq!(parent.model.as_deref(), Some("parent-model"));
        assert!(h.home.path().join("oz.args").exists());
    }
}

#[test]
fn prelaunch_held_substitution_pins_known_cli_default_and_protects_unknown_under_budget_pressure() {
    for model in [Some("gpt-6-sol"), None] {
        let h = Harness::new(model, false);
        std::fs::write(h.home.path().join("agent_config.toml"), format!("{}\n[claude]\ndisabled = true\n", std::fs::read_to_string(h.home.path().join("agent_config.toml")).expect("agents"))).expect("fixed peer inventory");
        h.hold("oz");
        std::fs::write(h.home.path().join("config.toml"),
            "[[usage.budget]]\nname = 'synthetic'\nagent = 'codex'\ncost_limit_usd = 10.0\nexternal_cost_usd = 9.0\n").expect("near budget limit");
        let candidate = h.first_peer().expect("peer");
        assert_eq!(candidate["model"].as_str(), model);
        success(&h.run().output().expect("held run"));
        let rows = h.rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].parent.is_none());
        h.assert_route(&rows[0], &candidate);
        assert!(!h.home.path().join("oz.args").exists());
    }
}

#[test]
fn stored_batch_retry_launches_candidate_model_from_saved_profile_and_links_parent() {
    let h = Harness::new(Some("gpt-6-sol"), false);
    h.mode("failure");
    let _failed = h
        .run()
        .args(["--group", "wg-route"])
        .output()
        .expect("failed parent");
    assert_eq!(h.rows().len(), 1);
    h.hold("oz");
    let candidate = h.first_peer().expect("peer");
    success(
        &h.aid()
            .args(["batch", "retry", "wg-route"])
            .output()
            .expect("batch retry"),
    );
    let rows = h.rows();
    assert_eq!(rows.len(), 2);
    let child = rows
        .iter()
        .find(|r| r.parent.as_deref() == Some("t-primary"))
        .expect("retry");
    success(
        &h.aid()
            .args(["wait", &child.id, "--timeout", "30"])
            .output()
            .expect("wait"),
    );
    h.assert_route(child, &candidate);
}

#[test]
fn failed_batch_auto_fallback_launches_advice_model_and_saved_profile() {
    let h = Harness::new(Some("gpt-6-sol"), false);
    h.mode("failure");
    let candidate = h.first_peer().expect("peer");
    let batch = h.home.path().join("batch.toml");
    std::fs::write(&batch, format!(
        "[defaults]\nauto_fallback = true\n[[task]]\nid = 't-batch-parent'\nagent = 'oz'\nprompt = '{PROMPT}'\nkind = 'refactoring'\nteam = 'routes'\ndifficulty = 'complex'\nbudget = 'premium'\nurgency = 'normal'\nrigor = 'standard'\nno_skill = true\nmodel = 'parent-model'\ndir = '{}'\noutput = '{}'\n",
        h.home.path().display(), h.home.path().join("answer.md").display(),
    )).expect("batch");
    success(
        &h.aid()
            .arg("batch")
            .arg(batch)
            .args(["--wait", "--yes"])
            .output()
            .expect("batch run"),
    );
    let rows = h.rows();
    assert_eq!(rows.len(), 2);
    let child = rows
        .iter()
        .find(|r| r.parent.as_deref() == Some("t-batch-parent"))
        .expect("fallback");
    h.assert_route(child, &candidate);
}

#[test]
fn no_automatic_launch_when_only_exhausted_held_or_team_preferred_claude_remain() {
    let h = Harness::new(Some("gpt-6-sol"), true);
    h.hold("oz");
    h.hold("codex");
    assert!(h.first_peer().is_none());
    assert!(
        h.advice()["recommended"].is_object(),
        "advisory rescue must not launch"
    );
    let output = h.run().output().expect("run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("is held"));
    assert!(h.rows().is_empty());
    assert!(!h.home.path().join("codex.args").exists());
    assert!(!h.home.path().join("claude.args").exists());
}

#[test]
fn known_unservable_candidate_fails_before_launch_instead_of_defaulting() {
    let h = Harness::new(Some("gpt-6-sol"), true);
    h.hold("oz");
    h.served("another-model");
    let output = h.run().output().expect("run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing a different default"));
    assert!(h.rows().is_empty());
    assert!(!h.home.path().join("codex.args").exists());
}

#[test]
fn detected_caller_excludes_weaker_same_pool_route_before_launch() {
    let h = Harness::new(Some("gpt-5.6-luna"), true);
    h.hold("oz");
    let caller = |cmd: &mut std::process::Command| {
        cmd.env("AID_CALLER_KIND", "codex")
            .env("AID_CALLER_SESSION", "synthetic-caller")
            .env("AID_CALLER_MODEL", "gpt-5.6-sol");
    };
    let mut advice = h.aid();
    caller(&mut advice);
    let output = advice
        .args([
            "advise",
            PROMPT,
            "--kind",
            "refactoring",
            "--team",
            "routes",
            "--json",
            "--top",
            "0",
        ])
        .args(PROFILE)
        .output()
        .expect("advice");
    success(&output);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("report");
    assert_eq!(report["caller"]["model"], "gpt-5.6-sol");
    let codex = report["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .find(|c| c["agent"] == "codex")
        .expect("codex");
    assert!(
        codex["exclusion_codes"]
            .as_array()
            .expect("codes")
            .iter()
            .any(|code| code == "weaker_on_caller_pool")
    );
    let mut run = h.run();
    caller(&mut run);
    assert!(!run.output().expect("run").status.success());
    assert!(h.rows().is_empty());
    assert!(!h.home.path().join("codex.args").exists());
}

#[test]
fn batch_fallback_child_receives_runtime_env_forward_and_shared_dir_without_persisting_values() {
    let h = Harness::new(Some("gpt-6-sol"), false);
    h.mode("failure");
    success(&h.aid().args(["group", "create", "Environment fixture", "--id", "wg-env-route"]).output().expect("group"));
    std::fs::create_dir_all(h.home.path().join("shared/wg-env-route")).expect("shared directory");
    let candidate = h.first_peer().expect("peer");
    let batch = h.home.path().join("env-batch.toml");
    std::fs::write(&batch, format!(
        "[defaults]\nauto_fallback = true\nshared_dir = true\n[[task]]\nid = 't-env-parent'\ngroup = 'wg-env-route'\nagent = 'oz'\nprompt = '{PROMPT}'\nkind = 'refactoring'\nteam = 'routes'\ndifficulty = 'complex'\nbudget = 'premium'\nurgency = 'normal'\nrigor = 'standard'\nno_skill = true\nmodel = 'parent-model'\ndir = '{}'\noutput = '{}'\nenv_forward = ['CASCADE_SYNTHETIC_FORWARDED']\n[task.env]\nCASCADE_SYNTHETIC_INLINE = 'synthetic-inline-value'\n",
        h.home.path().display(), h.home.path().join("answer.md").display(),
    )).expect("batch");
    success(&h.aid().arg("batch").arg(batch).args(["--wait", "--yes"])
        .env("CASCADE_SYNTHETIC_FORWARDED", "synthetic-forwarded-value").output().expect("batch run"));
    let rows = h.rows();
    assert_eq!(rows.len(), 2);
    let child = rows.iter().find(|r| r.parent.as_deref() == Some("t-env-parent")).expect("fallback");
    h.assert_route(child, &candidate);
    let captured = std::fs::read_to_string(h.home.path().join("codex.env")).expect("child environment");
    let values: Vec<_> = captured.lines().collect();
    assert_eq!(&values[..2], ["synthetic-inline-value", "synthetic-forwarded-value"]);
    assert!(!values[2].is_empty(), "shared directory missing: {captured}");
    assert!(std::path::Path::new(values[2]).ends_with(child.saved["group"].as_str().expect("group")));
    for row in rows {
        let saved = row.saved.to_string();
        assert!(!saved.contains("synthetic-inline-value") && !saved.contains("synthetic-forwarded-value"));
        assert!(row.saved["env"].is_null());
        assert_eq!(row.saved["env_forward"], serde_json::json!(["CASCADE_SYNTHETIC_FORWARDED"]));
    }
}

#[test]
fn failed_batch_with_consumed_explicit_fallback_does_not_launch_automatic_peer() {
    let h = Harness::new(Some("gpt-6-sol"), false);
    h.mode("failure");
    let batch = h.home.path().join("exhausted-batch.toml");
    std::fs::write(&batch, format!(
        "[defaults]\nauto_fallback = true\n[[task]]\nid = 't-explicit-parent'\nagent = 'claude'\nfallback = 'oz'\nprompt = '{PROMPT}'\nno_skill = true\ndir = '{}'\n",
        h.home.path().display(),
    )).expect("batch");
    h.hold("claude");
    let _output = h.aid().arg("batch").arg(batch).args(["--wait", "--yes"]).output().expect("batch run");
    let rows = h.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent, "oz");
    assert_eq!(rows[0].saved["cascade"], serde_json::json!([]));
    assert!(!h.home.path().join("codex.args").exists());
}

#[test]
fn prelaunch_default_profile_dimensions_are_materialized_in_saved_dispatch() {
    let h = Harness::new(Some("gpt-6-sol"), false);
    h.hold("oz");
    success(&h.aid().args(["run", "oz", PROMPT, "--no-hint", "--no-skill", "--no-audit",
        "--kind", "refactoring", "--team", "routes"])
        .arg("--dir").arg(h.home.path()).arg("--output").arg(h.home.path().join("answer.md"))
        .output().expect("default profile run"));
    let rows = h.rows();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.agent, "codex");
    assert_eq!(row.model.as_deref(), Some("gpt-6-sol"));
    assert!(row.parent.is_none());
    for (key, value) in [("declared_difficulty", "moderate"), ("declared_budget", "standard"),
        ("declared_urgency", "normal"), ("declared_rigor", "standard"), ("model_source", "Advised")] {
        assert_eq!(row.saved[key], value, "{key}");
    }
    let argv = std::fs::read_to_string(h.home.path().join("codex.args")).expect("actual argv");
    assert!(argv.lines().collect::<Vec<_>>().windows(2).any(|pair| pair == ["-m", "gpt-6-sol"]));
}
