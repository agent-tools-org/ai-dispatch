// File-backed input signaling for background tasks.
// Exports helpers to enqueue and consume `aid respond` payloads under ~/.aid/jobs.

use anyhow::Result;

use crate::paths;

pub fn write_response(task_id: &str, input: &str) -> Result<()> {
    if let Some(parent) = paths::job_input_path(task_id).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(paths::job_input_path(task_id), input)?;
    Ok(())
}

pub fn take_response(task_id: &str) -> Result<Option<String>> {
    let path = paths::job_input_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let input = std::fs::read_to_string(&path)?;
    std::fs::remove_file(path)?;
    Ok(Some(input))
}

pub fn clear_response(task_id: &str) -> Result<()> {
    let path = paths::job_input_path(task_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn write_steer(task_id: &str, message: &str) -> Result<()> {
    let path = paths::steer_signal_path(task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, message)?;
    Ok(())
}

pub fn take_steer(task_id: &str) -> Result<Option<String>> {
    let path = paths::steer_signal_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let message = std::fs::read_to_string(&path)?;
    std::fs::remove_file(path)?;
    Ok(Some(message))
}

pub fn clear_steer(task_id: &str) -> Result<()> {
    let path = paths::steer_signal_path(task_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{take_response, write_response, take_steer, write_steer};
    use crate::paths;

    #[test]
    fn writes_and_consumes_response_file() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());

        write_response("t-respond", "yes").unwrap();
        assert!(paths::job_input_path("t-respond").exists());

        let input = take_response("t-respond").unwrap();
        assert_eq!(input.as_deref(), Some("yes"));
        assert!(!paths::job_input_path("t-respond").exists());
    }

    #[test]
    fn writes_and_consumes_steer_file() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());

        write_steer("t-steer", "go left").unwrap();
        assert!(paths::steer_signal_path("t-steer").exists());

        let message = take_steer("t-steer").unwrap();
        assert_eq!(message.as_deref(), Some("go left"));
        assert!(!paths::steer_signal_path("t-steer").exists());
    }
}
