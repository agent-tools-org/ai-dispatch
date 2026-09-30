// Worker retry flows through real dispatch and fake agent completion.
// Proves persisted config, live runtime, and single completion-hook execution.
use super::RunArgs;
use crate::cmd::{checklist_scan::*, run::*};
use crate::{paths, store::Store, types::*};
use chrono::Local;
use std::{collections::HashMap, sync::Arc};

#[tokio::test]
async fn verify_worker_retry_preserves_configuration() { worker_retry("verify").await; }
#[tokio::test]
async fn checklist_worker_retry_preserves_configuration() { worker_retry("checklist").await; }
#[tokio::test]
async fn iterate_worker_retry_preserves_configuration() { worker_retry("iterate").await; }
#[tokio::test]
async fn hung_worker_retry_preserves_configuration() { worker_retry("hung").await; }
#[tokio::test]
async fn selfheal_worker_retry_preserves_configuration() { worker_retry("selfheal").await; }

async fn worker_retry(site: &str) {
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    let dir = tempfile::tempdir().unwrap();
    install_agent();
    let store = Arc::new(Store::open_memory().unwrap());
    let mut row = task("t-worker-retry");
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("worker-retry".into());
    row.status = if matches!(site, "hung" | "selfheal") { TaskStatus::Failed } else { TaskStatus::Done };
    row.verify_status = VerifyStatus::Failed;
    row.verify = Some("false".into());
    row.output_path = Some("row-output".into());
    store.insert_task(&row).unwrap();
    let marker = home.path().join("parent-hook");
    let saved = RunArgs {
        agent_name: "worker-retry".into(), prompt: "saved prompt".into(),
        dir: Some("/missing/saved-dir".into()), output: Some("saved-output".into()),
        verify: Some("skip".into()), read_only: true, budget: true,
        retry: 2, on_done: Some(format!("touch '{}'", marker.display())),
        ..Default::default()
    };
    store.update_task_dispatch_args(row.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();
    let runtime = RunArgs {
        dir: Some(dir.path().display().to_string()),
        env: Some(HashMap::from([("RETRY_RUNTIME".into(), "runtime-kept".into())])),
        verify: Some("false".into()), checklist: vec!["missing item".into()],
        retry: 2, foreground: true, announce: false, ..Default::default()
    };
    let id = tokio::time::timeout(std::time::Duration::from_secs(15),
        dispatch_retry(site, &store, &row, &runtime)).await.expect("worker retry timed out");
    let replayed = RunArgs::saved_for_task(&store, id.as_str()).unwrap().unwrap();
    assert_worker_retry(site, &store, &id, &replayed, &runtime);
    assert!(!marker.exists(), "{site} fired the parent's hook");
    let log = std::fs::read_to_string(paths::log_path(id.as_str())).unwrap();
    assert!(log.contains("runtime-kept"), "{site} lost the runtime environment: {log}");
}

fn install_agent() {
    std::fs::create_dir_all(paths::aid_dir().join("agents")).unwrap();
    std::fs::write(paths::aid_dir().join("agents/worker-retry.toml"),
        "[agent]\nid = \"worker-retry\"\ndisplay_name = \"Worker Retry\"\ncommand = \"/bin/sh\"\nprompt_mode = \"arg\"\nfixed_args = [\"-c\", \"printf '%s' \\\"$RETRY_RUNTIME\\\"\"]\ninteractive_input = false\n").unwrap();
}

async fn dispatch_retry(site: &str, store: &Arc<Store>, row: &Task, args: &RunArgs) -> TaskId {
    let result = match site {
        "verify" => maybe_auto_retry_after_verify_failure(store, &row.id, args, TaskStatus::Done).await,
        "checklist" => {
            let missing = ChecklistResult { items: vec![ChecklistItemResult {
                item: "missing item".into(), status: ChecklistItemStatus::Missing,
            }] };
            maybe_auto_retry_after_checklist_miss(store, &row.id, args, Some(&missing)).await
        }
        "iterate" => maybe_iterate(store, &row.id, args, &IterateConfig {
            max_iterations: 2, eval_command: "echo failure; exit 1".into(), feedback_template: None,
        }).await,
        "hung" => {
            crate::process_monitor::insert_hung_detected_events(store, &row.id, 180, 1, None, false).unwrap();
            crate::cmd::run::run_post::maybe_auto_retry_after_hang(store, &row.id, args).await
        }
        "selfheal" => {
            std::fs::write(paths::stderr_path(row.id.as_str()), "Model not found: stale").unwrap();
            crate::cmd::run::run_model_selfheal::maybe_auto_retry_after_model_unavailable(store, &row.id, args).await
        }
        _ => panic!("unknown retry site"),
    };
    result.unwrap().unwrap()
}

fn assert_worker_retry(site: &str, store: &Store, id: &TaskId, args: &RunArgs, runtime: &RunArgs) {
    assert_eq!(store.get_task(id.as_str()).unwrap().unwrap().status, TaskStatus::Done, "{site}");
    assert_eq!(args.dir, runtime.dir, "{site}");
    assert_eq!(args.foreground, runtime.foreground, "{site}");
    assert_eq!(args.announce, runtime.announce, "{site}");
    assert!(args.on_done.is_none(), "{site}");
    assert_eq!(args.output.as_deref(), Some("saved-output"), "{site}");
    assert_eq!(args.verify.as_deref(), Some("skip"), "{site}");
    assert!(args.read_only, "{site}");
    assert_eq!(args.budget, site != "selfheal", "{site}");
    assert_eq!(args.force_default_model, site == "selfheal", "{site}");
    assert_eq!(args.retry, if site == "selfheal" { 2 } else { 1 }, "{site}");
    assert_eq!(args.parent_task_id.as_deref(), Some("t-worker-retry"), "{site}");
}

fn task(task_id: &str) -> Task {
    Task {
        id: TaskId(task_id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Running, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, project_id: None, worktree_path: None, effective_dir: None,
        worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None,
        log_path: None, output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}
