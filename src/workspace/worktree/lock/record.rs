// Record helpers.
// Moved from the parent module; shares its integration types and dependencies.

use super::*;

impl LockRecord {
    pub(super) fn new(task_id: &str) -> Self {
        Self {
            task_id: task_id.to_string(),
            owner_pid: Some(std::process::id()),
            worker_pid: None,
        }
    }

    pub(super) fn parse(content: &str) -> Option<Self> {
        let mut task_id = None;
        let mut owner_pid = None;
        let mut worker_pid = None;
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key {
                "task_id" if !value.is_empty() => task_id = Some(value.to_string()),
                "owner_pid" => owner_pid = value.parse::<u32>().ok(),
                "worker_pid" => worker_pid = value.parse::<u32>().ok(),
                _ => {}
            }
        }
        Some(Self {
            task_id: task_id?,
            owner_pid,
            worker_pid,
        })
    }

    pub(super) fn to_content(&self) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        format!(
            "version=1\ntask_id={}\nowner_pid={}\nworker_pid={}\nupdated_at={}\n",
            self.task_id,
            self.owner_pid.map_or(String::new(), |pid| pid.to_string()),
            self.worker_pid.map_or(String::new(), |pid| pid.to_string()),
            now
        )
    }
}
