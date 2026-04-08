// File-backed input signaling for background tasks.
// Exports helpers to enqueue and consume `aid respond` payloads under ~/.aid/jobs.

use anyhow::Result;

use crate::paths;
use crate::sanitize;

pub fn write_response(task_id: &str, input: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    if let Some(parent) = paths::job_input_path(task_id).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(paths::job_input_path(task_id), input)?;
    Ok(())
}

pub fn take_response(task_id: &str) -> Result<Option<String>> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_input_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let input = std::fs::read_to_string(&path)?;
    std::fs::remove_file(path)?;
    Ok(Some(input))
}

pub fn clear_response(task_id: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_input_path(task_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn write_steer(task_id: &str, message: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::steer_signal_path(task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, message)?;
    Ok(())
}

pub fn take_steer(task_id: &str) -> Result<Option<String>> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::steer_signal_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let message = std::fs::read_to_string(&path)?;
    std::fs::remove_file(path)?;
    Ok(Some(message))
}

pub fn clear_steer(task_id: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
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

        write_response("t-abcd", "yes").unwrap();
        assert!(paths::job_input_path("t-abcd").exists());

        let input = take_response("t-abcd").unwrap();
        assert_eq!(input.as_deref(), Some("yes"));
        assert!(!paths::job_input_path("t-abcd").exists());
    }

    #[test]
    fn writes_and_consumes_steer_file() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());

        write_steer("t-bcde", "go left").unwrap();
        assert!(paths::steer_signal_path("t-bcde").exists());

        let message = take_steer("t-bcde").unwrap();
        assert_eq!(message.as_deref(), Some("go left"));
        assert!(!paths::steer_signal_path("t-bcde").exists());
    }

    #[test]
    fn rejects_invalid_task_id() {
        let err = write_response("bad.id", "yes").unwrap_err();
        assert!(err.to_string().contains("Invalid task ID"));
    }
}
