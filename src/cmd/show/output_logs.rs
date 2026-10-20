// Raw agent and worker diagnostics for aid show --log.
// Reads per-task worker stderr even when no agent log was created.
use super::show_output_messages::read_tail;
use crate::support::paths;
use anyhow::{Context, Result};

pub fn log_text(task_id: &str) -> Result<String> {
    render(task_id, true)
}

pub fn log_text_brief(task_id: &str) -> Result<String> {
    render(task_id, false)
}

fn render(task_id: &str, full: bool) -> Result<String> {
    let agent = paths::log_path(task_id);
    let worker = paths::task_dir(task_id).join("worker.log");
    let mut text = if full && agent.exists() {
        std::fs::read_to_string(&agent)
            .with_context(|| format!("Failed to read log file {}", agent.display()))?
    } else if full && !worker.exists() {
        return std::fs::read_to_string(&agent)
            .with_context(|| format!("Failed to read log file {}", agent.display()));
    } else {
        read_tail(&agent, 200, "No log available")
    };
    if worker.exists() {
        let body = if full {
            std::fs::read_to_string(&worker)
                .with_context(|| format!("Failed to read worker log {}", worker.display()))?
        } else {
            read_tail(&worker, 200, "No worker diagnostics")
        };
        text.push_str(&format!("\nWorker log ({}):\n{body}", worker.display()));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_worker_diagnostics_without_agent_log() {
        let temp = tempfile::tempdir().expect("home");
        let _home = paths::AidHomeGuard::set(temp.path());
        let dir = paths::task_dir("t-worker-log");
        std::fs::create_dir_all(&dir).expect("task directory");
        std::fs::write(dir.join("worker.log"), "panic: worker failure\n").expect("worker log");
        for text in [log_text("t-worker-log"), log_text_brief("t-worker-log")] {
            let text = text.expect("render");
            assert!(text.contains("panic: worker failure"));
            assert!(text.contains("worker.log"));
        }
    }
}
