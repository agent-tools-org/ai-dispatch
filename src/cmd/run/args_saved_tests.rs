// Worker configuration rebuilt from saved dispatch args across the job-file boundary.
// Every saved RunArgs field must reach the worker; env values are never persisted.
use super::args_retry_tests::{every_field_set, assert_every_field_differs_from_default};
use super::RunArgs;
use crate::agent::classifier::TaskCategory;
use crate::agent::model_validation::ModelSource;
use crate::runtime::background::{load_spec_if_exists, save_spec, BackgroundRunSpec};
use crate::store::Store;
use crate::config::timeout_policy::TimeoutPolicy;
use crate::types::*;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn worker_rebuild_keeps_every_saved_field() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    let store = Store::open(&crate::support::paths::db_path()).unwrap();
    let id = "t-every-field";
    let saved = every_field_set();
    assert_every_field_differs_from_default(&saved);
    let mut row = task(id);
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("resolved-agent".into());
    row.effective_dir = Some("/tmp/row-effective".into());
    row.resolved_prompt = Some("resolved prompt".into());
    store.insert_task(&row).unwrap();
    let json = saved.dispatch_args_json().unwrap();
    assert!(!json.contains("secret-value"), "env values must never be persisted");
    store.update_task_dispatch_args(id, &json).unwrap();
    // The detached worker receives only the task id: config comes from the
    // store, runtime facts from the job file.
    save_spec(&spec(id)).unwrap();
    let loaded = load_spec_if_exists(id).unwrap().unwrap();
    let reopened = Store::open(&crate::support::paths::db_path()).unwrap();
    let worker = RunArgs::for_worker(&reopened, &loaded).unwrap();

    let mut expected = serde_json::to_value(&saved).unwrap();
    for (key, value) in [
        ("agent_name", json!("resolved-agent")),
        ("dir", json!("/tmp/spec")),
        ("prompt", json!(row.prompt)),
        ("budget", json!(false)),
        ("on_done", Value::Null),
        ("origin", json!("automatic")),
        ("env", json!({"RUNTIME_ONLY": "from-spec"})),
        ("existing_task_id", Value::Null),
        ("foreground", json!(false)),
        ("announce", json!(false)),
        ("background", json!(true)),
    ] {
        expected[key] = value;
    }
    let actual = serde_json::to_value(&worker).unwrap();
    for (key, want) in expected.as_object().unwrap() {
        assert_eq!(&actual[key], want, "field `{key}` changed across the worker rebuild");
    }
}

#[test]
fn dispatch_saves_the_callers_budget_request_not_the_resolved_mode() {
    let _home = crate::support::test_env::TestHome::with_dirs();
    std::fs::write(crate::support::paths::config_path(), "[selection]\nbudget_mode = true\n").unwrap();
    std::fs::create_dir_all(crate::support::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(crate::support::paths::aid_dir().join("agents/budget-test.toml"),
        "[agent]\nid = \"budget-test\"\ndisplay_name = \"Budget Test\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\ninteractive_input = false\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "budget-test".into(), prompt: "Assess the project".into(),
        dir: Some(dir.path().display().to_string()), read_only: true, ..Default::default()
    };
    let prepared = super::super::run_dispatch_prepare::prepare_dispatch_with(&store, &mut args, |_| true).unwrap();
    let id = prepared.task_id.as_str();
    assert!(prepared.budget_active, "dispatch resolved budget mode for this run");
    // Retries read task.budget and the saved args: neither may carry a mode the caller never asked for.
    assert!(!store.get_task(id).unwrap().unwrap().budget);
    assert!(!RunArgs::saved_for_task(&store, id).unwrap().unwrap().budget);
}

#[tokio::test]
async fn worker_without_a_dispatched_dir_skips_verify() {
    let home = crate::support::test_env::TestHome::with_dirs();
    std::fs::create_dir_all(crate::support::paths::aid_dir().join("agents")).unwrap();
    std::fs::write(crate::support::paths::aid_dir().join("agents/noop-test.toml"),
        "[agent]\nid = \"noop-test\"\ndisplay_name = \"Noop Test\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\nfixed_args = [\"-c\", \"true\"]\ninteractive_input = false\n").unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let id = "t-no-dir";
    let marker = home.path().join("verify-ran");
    let verify = format!("touch '{}'", marker.display());
    let mut row = task(id);
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("noop-test".into());
    // Dispatch records the caller's cwd on the row even when no dir was given.
    row.effective_dir = Some(home.path().display().to_string());
    row.verify = Some(verify.clone());
    row.verify_status = VerifyStatus::Pending;
    store.insert_task(&row).unwrap();
    let saved = RunArgs { agent_name: "noop-test".into(), verify: Some(verify), ..Default::default() };
    store.update_task_dispatch_args(id, &saved.dispatch_args_json().unwrap()).unwrap();
    let mut job = spec(id);
    job.dir = None;
    save_spec(&job).unwrap();
    assert_eq!(RunArgs::for_worker(&store, &job).unwrap().dir, None);

    crate::runtime::background::run_task(store.clone(), id).await.unwrap();

    let done = store.get_task(id).unwrap().unwrap();
    assert_eq!(done.status, TaskStatus::Done);
    assert!(!marker.exists(), "verify must not run without a dispatched working directory");
}

fn spec(task_id: &str) -> BackgroundRunSpec {
    serde_json::from_value(json!({
        "task_id": task_id, "worker_pid": null, "agent_name": "spec-agent",
        "prompt": "spec prompt", "dir": "/tmp/spec", "output": null, "model": "spec-model",
        "budget": false, "verify": null, "retry": 0, "group": null, "interactive": true,
        "env": {"RUNTIME_ONLY": "from-spec"}, "foreground": false
    }))
    .unwrap()
}

fn task(task_id: &str) -> Task {
    crate::types::Task::test_fixture(TaskId(task_id.to_string()), TaskStatus::Running)
}
