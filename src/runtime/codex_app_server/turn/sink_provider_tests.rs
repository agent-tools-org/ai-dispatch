// Provider-scoped stream, app-server and stderr holds plus success and cost ceilings.
// Exercises the production sink with synthetic events and an in-memory store.
use super::*;
use crate::runtime::codex_app_server::fixture;
const ROUTE: Option<&str> = Some("codex-provider:X");

fn run<'a>(fixture: &'a fixture::Fixture, agent: &'a dyn crate::agent::Agent) -> Run<'a> {
    Run {
        caller_owned: false,
        start: std::time::Instant::now(),
        agent,
        command: &fixture.command,
        task: &fixture.task,
        store: &fixture.store,
        log: &fixture.log,
        output: None,
        group: None,
        policy: crate::config::timeout_policy::TimeoutPolicy::default(),
        max_cost: None,
        control: None,
    }
}

fn marker(name: Option<&str>) -> std::path::PathBuf {
    crate::routing::rate_limit::marker_path(&AgentKind::Codex, name)
}

#[test]
fn provider_refusals_and_success_touch_only_the_persisted_route() {
    let fixture = fixture::Fixture::new("complete");
    let adapter = crate::agent::codex::routed(ROUTE.map(str::to_string));
    let run = run(&fixture, adapter.as_ref());
    assert_stream_markers(&fixture, adapter.as_ref());
    let message = "You have hit your usage limit";
    let mut sink = Sink::new(&run).unwrap();
    sink.event(serde_json::json!({"type":"error","error":{
        "message":"Allowance unavailable","codexErrorInfo":"usageLimitExceeded"}}))
        .unwrap();
    assert!(sink.quota);
    assert!(marker(ROUTE).exists());
    assert!(!marker(None).exists());
    std::fs::remove_file(marker(ROUTE)).unwrap();
    sink.info.status = TaskStatus::Failed;
    std::fs::write(
        crate::support::paths::stderr_path(fixture.task.as_str()),
        message,
    )
    .unwrap();
    sink.failure_stderr().unwrap();
    assert!(marker(ROUTE).exists());
    assert!(!marker(None).exists());
    drop(sink);
    assert_success_clears_only_x(&run);
}

fn assert_success_clears_only_x(run: &Run<'_>) {
    std::fs::write(marker(None), "hold: manual\n").unwrap();
    std::fs::write(marker(Some("codex-provider:Y")), "hold: manual\n").unwrap();
    let mut sink = Sink::new(run).unwrap();
    sink.event(
        serde_json::json!({"type":"codex.rate_limits","rate_limits":{
        "primary":{"usedPercent":100,"resetsAt":4102444800_i64}}}),
    )
    .unwrap();
    assert!(!sink.quota, "ChatGPT snapshot does not describe X");
    sink.event(serde_json::json!({"type":"item.completed","item":{
        "type":"agent_message","text":"Completed."}}))
        .unwrap();
    assert_eq!(sink.finish().unwrap().status, TaskStatus::Done);
    assert!(!marker(ROUTE).exists());
    assert!(marker(None).exists());
    assert!(marker(Some("codex-provider:Y")).exists());
    let events = run.store.get_events(run.task.as_str()).unwrap();
    assert!(!events.iter().any(|event| {
        event
            .metadata
            .as_ref()
            .is_some_and(|meta| meta.get("codex_rate_limits").is_some())
    }));
}

#[test]
fn provider_usage_still_enforces_the_task_cost_ceiling() {
    let fixture = fixture::Fixture::new("complete");
    let adapter = crate::agent::codex::routed(ROUTE.map(str::to_string));
    let mut run = run(&fixture, adapter.as_ref());
    run.max_cost = Some(0.000001);
    fixture.store.db().execute(
        "UPDATE tasks SET custom_agent_name=?1 WHERE id=?2",
        [ROUTE.unwrap(), fixture.task.as_str()],
    ).unwrap();
    let task = fixture.store.get_task(fixture.task.as_str()).unwrap().unwrap();
    assert_eq!(task.route().provider.as_str(), ROUTE.unwrap());
    let mut sink = Sink::new(&run).unwrap();
    let model = crate::routing::model_catalog::AGENT_MODELS
        .iter()
        .find(|row| row.agent == AgentKind::Codex && row.input_per_m > 0.0)
        .unwrap()
        .model;
    sink.info.model = Some(model.into());
    let error = sink
        .event(serde_json::json!({"type":"turn.usage","model":model,
        "usage":{"input_tokens":1000000,"output_tokens":1000000}}))
        .unwrap_err();
    assert!(error.to_string().contains("cost ceiling"));
    assert!(sink.info.cost_usd.unwrap() > run.max_cost.unwrap());
}

fn assert_stream_markers(fixture: &fixture::Fixture, adapter: &dyn crate::agent::Agent) {
    let message = "You have hit your usage limit";
    for event in [
        serde_json::json!({"type":"error","message":message}),
        serde_json::json!({"type":"item.completed","item":{"type":"error","message":message}}),
    ] {
        adapter
            .parse_event(&fixture.task, &event.to_string())
            .unwrap();
        assert!(marker(ROUTE).exists());
        assert!(!marker(None).exists());
        std::fs::remove_file(marker(ROUTE)).unwrap();
    }
}
