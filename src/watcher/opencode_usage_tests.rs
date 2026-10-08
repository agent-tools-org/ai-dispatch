// Usage regressions for adapters sharing the OpenCode stream parser.
// Replays the real multi-step fixture through parsing and the streaming watcher.
// Depends on overlay adapters and the streaming completion test harness.

use crate::agent::opencode_overlay::OpenCodeOverlayAgent;
use crate::agent::{Agent, get_agent};
use crate::types::{AgentKind, CompletionInfo, EventKind, TaskId, TaskStatus};

use super::streaming_completion_tests::watch_usage;

const MULTI_STEP: &str =
    include_str!("../../tests/fixtures/streaming_completion/opencode-multi-step.jsonl");

fn custom_opencode_agent() -> OpenCodeOverlayAgent {
    OpenCodeOverlayAgent::new("custom".into(), "Custom".into(), "provider/model".into())
}

async fn assert_multi_step_sum(agent: &dyn Agent) {
    let (info, _) = watch_usage(agent, MULTI_STEP, None).await;
    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.exit_code, Some(0));
    assert_eq!(info.tokens, Some(1_498_073));
    assert!((info.cost_usd.unwrap() - 0.10654006).abs() < 1e-12);
}

#[tokio::test]
async fn kilo_multi_step_completion_records_sum() {
    assert_multi_step_sum(get_agent(AgentKind::Kilo).as_ref()).await;
}

#[tokio::test]
async fn mimocode_multi_step_completion_records_sum() {
    assert_multi_step_sum(get_agent(AgentKind::MiMoCode).as_ref()).await;
}

#[tokio::test]
async fn custom_opencode_multi_step_completion_records_sum() {
    assert_multi_step_sum(&custom_opencode_agent()).await;
}

#[test]
fn opencode_family_step_usage_accumulates_before_final_parsing() {
    let task_id = TaskId("t-opencode-family-steps".into());
    let agents: Vec<Box<dyn Agent>> = vec![
        get_agent(AgentKind::Kilo),
        get_agent(AgentKind::MiMoCode),
        Box::new(custom_opencode_agent()),
    ];
    for agent in agents {
        let mut info = CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        };
        for (line, (tokens, cost)) in MULTI_STEP
            .lines()
            .zip([(14_346, 0.00851796), (29_465, 0.009527012)])
        {
            let event = agent.parse_event(&task_id, line).unwrap();
            super::apply_completion_event(&mut info, &event);
            assert_eq!(info.tokens, Some(tokens), "{} running tokens", agent.kind());
            assert!((info.cost_usd.unwrap() - cost).abs() < 1e-12);
        }
    }
}

#[tokio::test]
async fn kilo_running_sum_enforces_cost_ceiling() {
    let agent = get_agent(AgentKind::Kilo);
    let (info, events) = watch_usage(agent.as_ref(), MULTI_STEP, Some(0.009)).await;
    assert_eq!(info.status, TaskStatus::Failed);
    assert_eq!(info.tokens, Some(29_465));
    assert!((info.cost_usd.unwrap() - 0.009527012).abs() < 1e-12);
    assert!(events.iter().any(|event| {
        event.event_kind == EventKind::Error && event.detail.contains("exceeded ceiling")
    }));
}
