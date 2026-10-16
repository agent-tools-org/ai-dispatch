// Container API keys travel as child environment, never command arguments.
// Uses a synthetic value and inspects the wrapper without spawning a container.
use super::*;

#[test]
fn container_forwards_anthropic_key_by_name_only() {
    let mut inner = std::process::Command::new("claude");
    inner.env("ANTHROPIC_API_KEY", "synthetic-container-value");
    let command = exec_in_container(&inner, "test-container");
    assert!(command.get_args().any(|arg| arg == "ANTHROPIC_API_KEY"));
    assert!(
        !command
            .get_args()
            .any(|arg| arg.to_string_lossy().contains("synthetic-container-value"))
    );
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_some())
    );
}

#[test]
fn container_wrapper_preserves_route_api_key_removal() {
    let mut inner = std::process::Command::new("claude");
    inner.env_remove("ANTHROPIC_API_KEY");
    let command = exec_in_container(&inner, "test-container");
    assert!(
        command
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_none())
    );
    assert!(!command.get_args().any(|arg| arg == "ANTHROPIC_API_KEY"));
}
