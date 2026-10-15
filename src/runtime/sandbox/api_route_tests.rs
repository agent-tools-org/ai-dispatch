// Shared sandbox environment forwarding keeps API-key values out of argv.
// Inspects an unlaunched wrapper with synthetic child credentials.
use super::*;

#[test]
fn sandbox_forwards_anthropic_key_by_name_only() {
    let mut inner = std::process::Command::new("claude");
    inner.env("ANTHROPIC_API_KEY", "synthetic-sandbox-value");
    let mut outer = std::process::Command::new("container");
    forward_command_envs(&mut outer, &inner);
    assert!(outer.get_args().any(|arg| arg == "ANTHROPIC_API_KEY"));
    assert!(
        !outer
            .get_args()
            .any(|arg| arg.to_string_lossy().contains("synthetic-sandbox-value"))
    );
    assert!(
        outer
            .get_envs()
            .any(|(key, value)| key == "ANTHROPIC_API_KEY" && value.is_some())
    );
}
