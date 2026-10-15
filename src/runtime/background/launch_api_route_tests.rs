// Worker launch and PTY regressions for helper-backed API routes.
// A fake Claude validates settings and key absence without invoking the keychain.
use super::*;
use crate::support::paths::{self, AidHomeGuard};

fn fixture() -> (tempfile::TempDir, AidHomeGuard, Arc<Store>) {
    let home = tempfile::tempdir().unwrap();
    let guard = AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    std::fs::write(
        paths::config_path(),
        concat!(
            "[[api_route]]\nid = 'claude-api-ming'\nagent = 'claude'\n",
            "keychain_service = 'test'\nkeychain_account = 'account-ming'\nmonthly_budget_usd = 200\n",
        ),
    )
    .unwrap();
    crate::support::test_env::write_executable(&home.path().join("bin/claude"), FAKE_CLAUDE);
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_waiting_task(
            "t-api-launch",
            "claude-api-ming",
            "test",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET status = 'running', agent = 'custom',
        custom_agent_name = 'claude-api-ming' WHERE id = 't-api-launch'",
            [],
        )
        .unwrap();
    (home, guard, store)
}

fn spec(interactive: bool) -> BackgroundRunSpec {
    serde_json::from_value(serde_json::json!({
        "task_id": "t-api-launch", "agent_name": "claude-api-ming", "prompt": "test",
        "dir": null, "output": null, "verify": null, "retry": 0, "group": null,
        "interactive": interactive,
    }))
    .unwrap()
}

fn args(home: &std::path::Path) -> RunArgs {
    RunArgs {
        agent_name: "claude-api-ming".into(),
        dir: Some(home.display().to_string()),
        sandbox: true,
        env: Some(std::collections::HashMap::from([
            ("PATH".into(), home.join("bin").display().to_string()),
            ("ANTHROPIC_API_KEY".into(), "synthetic-operator-key".into()),
            ("ANTHROPIC_BASE_URL".into(), "https://example.invalid".into()),
            ("ANTHROPIC_AUTH_TOKEN".into(), "synthetic".into()),
            ("CLAUDE_CODE_USE_BEDROCK".into(), "1".into()),
            ("CLAUDE_CODE_USE_VERTEX".into(), "1".into()),
        ])),
        ..Default::default()
    }
}

fn prepared(
    home: &std::path::Path,
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
) -> (
    RunArgs,
    Box<dyn agent::Agent>,
    std::process::Command,
    agent::home_isolation::IsolatedHomeGuard,
) {
    let args = args(home);
    let agent = agent::registry::resolve_custom_agent(&args.agent_name).unwrap();
    let isolated =
        agent::home_isolation::IsolatedHomeGuard::create_from_home(Some(home), Some("t-api-launch")).unwrap();
    let (command, _) = prepare_launch(store, spec, &args, agent.as_ref(), &isolated).unwrap();
    assert!(command.get_args().any(|arg| arg == "--settings"));
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_none())
    );
    (args, agent, command, isolated)
}

#[test]
fn launch_removes_explicit_env_key_after_shared_environment_setup() {
    let (home, _guard, store) = fixture();
    let (_, _, command, _isolated) = prepared(home.path(), &store, &spec(false));
    assert!(
        !command
            .get_args()
            .any(|arg| arg.to_string_lossy().contains("synthetic-operator-key"))
    );
}

#[tokio::test]
async fn piped_worker_launch_uses_route_helper_and_no_env_key() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store) = fixture();
    let spec = spec(false);
    let (args, agent, command, _isolated) = prepared(home.path(), &store, &spec);
    super::super::run_process(&store, &spec, &args, agent.as_ref(), command)
        .await
        .unwrap();
    assert_eq!(
        store.get_task("t-api-launch").unwrap().unwrap().status,
        crate::types::TaskStatus::Done
    );
}

#[tokio::test]
async fn foreground_pty_launch_uses_route_helper_and_no_env_key() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store) = fixture();
    let spec = spec(true);
    let (args, agent, command, _isolated) = prepared(home.path(), &store, &spec);
    super::super::run_process(&store, &spec, &args, agent.as_ref(), command)
        .await
        .unwrap();
    assert_eq!(
        store.get_task("t-api-launch").unwrap().unwrap().status,
        crate::types::TaskStatus::Done
    );
}

const FAKE_CLAUDE: &str = r#"#!/bin/sh
settings=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --settings) settings=$2; shift 2 ;;
        *) shift ;;
    esac
done
expected='{"apiKeyHelper":"/usr/bin/security find-generic-password -s test -a account-ming -w"}'
[ "$settings" = "$expected" ] || exit 91
[ "${ANTHROPIC_API_KEY+x}" != x ] || exit 92
[ "${ANTHROPIC_BASE_URL+x}" != x ] || exit 93
[ "${ANTHROPIC_AUTH_TOKEN+x}" != x ] || exit 94
[ "${CLAUDE_CODE_USE_BEDROCK+x}" != x ] || exit 95
[ "${CLAUDE_CODE_USE_VERTEX+x}" != x ] || exit 96
printf '%s\n' '{"type":"system","subtype":"init","apiKeySource":"apiKeyHelper"}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0}'
"#;

#[tokio::test]
async fn route_cost_cap_stops_assistant_usage_without_a_result() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store) = fixture();
    crate::support::test_env::write_executable(
        &home.path().join("bin/claude"),
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"id\":\"cap\",",
            "\"model\":\"sonnet\",\"usage\":{\"input_tokens\":1000000}}}'\n",
            "/bin/sleep 30\n",
        ),
    );
    let spec = spec(false);
    let (mut args, agent, command, _isolated) = prepared(home.path(), &store, &spec);
    args.max_task_cost = Some(1.0);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        super::super::run_process(&store, &spec, &args, agent.as_ref(), command),
    )
    .await
    .unwrap()
    .unwrap();
    let task = store.get_task("t-api-launch").unwrap().unwrap();
    assert_eq!(task.status, crate::types::TaskStatus::Failed);
    assert!(task.cost_usd.is_some_and(|cost| cost > 1.0));
}
