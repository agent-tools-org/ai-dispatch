// Streaming watcher run state and ordered phases.
// Depends on watcher parsing, process monitoring, and persistent events.
use super::*;
use std::ops::ControlFlow;
mod finish;
mod streaming;

pub(super) struct WatchRun<'a> {
    lines: tokio::io::Lines<
        BufReader<crate::runtime::pty::watch::piped::Reader<tokio::process::ChildStdout>>,
    >,
    watchdog: Option<crate::runtime::pty::watch::piped::Watchdog>,
    transcript: Option<Arc<std::sync::Mutex<std::fs::File>>>,
    stderr_handle: Option<stderr::StderrCapture>,
    input: Option<crate::runtime::claude_input::Session>,

    agent: &'a dyn Agent,
    child: &'a mut Child,
    task_id: &'a TaskId,
    store: &'a Arc<Store>,
    log_file: tokio::fs::File,
    info: CompletionInfo,
    event_count: u32,
    saw_completion_event: bool,
    api_route_failed: bool,
    delivery_evidence: DeliveryEvidence,
    full_output: String,
    session_saved: bool,
    synthetic_tracker: SyntheticMilestoneTracker,
    last_event_detail: Option<String>,
}

impl<'a> WatchRun<'a> {
    pub(super) async fn setup(
        agent: &'a dyn Agent,
        child: &'a mut Child,
        task_id: &'a TaskId,
        store: &'a Arc<Store>,
        log_path: &std::path::Path,
        prompt: Option<&str>,
        policy: Option<crate::config::timeout_policy::TimeoutPolicy>,
    ) -> Result<Self> {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("No stdout on child process"))?;
        let watchdog = policy
            .filter(|_| prompt.is_some())
            .map(crate::runtime::pty::watch::piped::Watchdog::new);
        let transcript = if watchdog.is_some() {
            std::fs::create_dir_all(crate::support::paths::task_dir(task_id.as_str()))?;
            Some(Arc::new(std::sync::Mutex::new(std::fs::File::create(
                crate::support::paths::transcript_path(task_id.as_str()),
            )?)))
        } else {
            None
        };
        let reader = BufReader::new(crate::runtime::pty::watch::piped::Reader::new(
            stdout,
            watchdog.as_ref(),
            transcript.clone(),
            false,
        ));
        let lines = reader.lines();
        let log_file = tokio::fs::File::create(log_path).await?;
        Ok(Self::new(
            agent, child, task_id, store, log_file, lines, watchdog, transcript,
        ))
    }

    fn new(
        agent: &'a dyn Agent,
        child: &'a mut Child,
        task_id: &'a TaskId,
        store: &'a Arc<Store>,
        log_file: tokio::fs::File,
        lines: tokio::io::Lines<
            BufReader<crate::runtime::pty::watch::piped::Reader<tokio::process::ChildStdout>>,
        >,
        watchdog: Option<crate::runtime::pty::watch::piped::Watchdog>,
        transcript: Option<Arc<std::sync::Mutex<std::fs::File>>>,
    ) -> Self {
        Self {
            lines,
            watchdog,
            transcript,
            stderr_handle: None,
            input: None,
            agent,
            child,
            task_id,
            store,
            log_file,
            info: CompletionInfo {
                tokens: None,
                status: TaskStatus::Done,
                model: None,
                cost_usd: None,
                exit_code: None,
            },
            event_count: 0u32,
            session_saved: false,
            saw_completion_event: false,
            api_route_failed: false,
            synthetic_tracker: SyntheticMilestoneTracker::new(),
            delivery_evidence: DeliveryEvidence::default(),
            last_event_detail: None,
            full_output: String::new(),
        }
    }
}
