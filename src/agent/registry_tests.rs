// Custom registry loading and delegated adapter regression tests.
// Uses temporary agent definitions, never account keys.
use super::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;

fn write_agent(dir: &Path, file: &str, contents: &str) {
    fs::write(dir.join(file), contents).unwrap();
}

/// A fork of a built-in adapter is not a route and must not load. The
/// negative half matters as much: a wrapper around an unrelated binary is
/// exactly what custom agents are for and must still load.
#[test]
fn a_custom_agent_naming_a_builtin_binary_is_refused() {
    let dir = TempDir::new().unwrap();
    write_agent(
        dir.path(),
        "fork.toml",
        r#"[agent]
id = "l2-researcher"
display_name = "L2 Researcher"
command = "droid"
prompt_mode = "arg"
"#,
    );
    write_agent(
        dir.path(),
        "wrapper.toml",
        r#"[agent]
id = "goose"
display_name = "Goose"
command = "goose"
prompt_mode = "arg"
"#,
    );
    let agents = load_from_dir(dir.path());
    assert!(
        !agents.contains_key("l2-researcher"),
        "a droid fork must not load"
    );
    assert!(
        agents.contains_key("goose"),
        "a genuinely different CLI must still load"
    );
}

#[test]
fn a_path_qualified_builtin_binary_is_refused_too() {
    let config = parse_config(
        r#"[agent]
id = "sneaky"
display_name = "Sneaky"
command = "/usr/local/bin/codex"
prompt_mode = "arg"
"#,
    )
    .unwrap();
    assert!(fork_of_builtin_reason(&config).is_some());
}

/// `delegate_to` is the supported way to reach a different model through a
/// built-in CLI, so it is a route and stays allowed.
#[test]
fn a_delegating_agent_is_still_allowed() {
    let config = parse_config(
        r#"[agent]
id = "mimo"
display_name = "MiMo"
command = "opencode"
prompt_mode = "arg"
delegate_to = "opencode"
forced_model = "mimo/mimo-v2.5-pro"
"#,
    )
    .unwrap();
    assert!(fork_of_builtin_reason(&config).is_none());
}

fn sample_agent_toml(id: &str) -> String {
    format!(
        r#"[agent]
id = "{id}"
display_name = "{id} agent"
command = "{id}"
"#,
        id = id
    )
}

#[test]
fn empty_dir_returns_empty_registry() {
    let dir = TempDir::new().unwrap();
    assert!(load_from_dir(dir.path()).is_empty());
}

#[test]
fn loads_valid_toml() {
    let dir = TempDir::new().unwrap();
    write_agent(dir.path(), "foo.toml", &sample_agent_toml("foo"));
    let map = load_from_dir(dir.path());
    assert!(map.contains_key("foo"));
}

#[test]
fn skips_invalid_toml() {
    let dir = TempDir::new().unwrap();
    write_agent(dir.path(), "bad.toml", "not = valid = toml");
    assert!(load_from_dir(dir.path()).is_empty());
}

#[test]
fn resolve_returns_none_for_unknown() {
    let map = HashMap::new();
    assert!(resolve_from_registry(&map, "missing").is_none());
}

#[test]
fn list_returns_sorted() {
    let dir = TempDir::new().unwrap();
    write_agent(dir.path(), "b.toml", &sample_agent_toml("b"));
    write_agent(dir.path(), "a.toml", &sample_agent_toml("a"));
    let map = load_from_dir(dir.path());
    let list = list_from_registry(&map);
    let ids: Vec<_> = list.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

#[test]
fn delegate_to_opencode_returns_overlay_agent() {
    let toml_data = r#"[agent]
id = "mimo"
display_name = "MiMo"
command = "bash"
delegate_to = "opencode"
forced_model = "mimo/mimo-v2.5-pro"
"#;
    let config = parse_config(toml_data).unwrap();
    let agent = build_agent(&config);
    // Overlay reports Custom kind; bash CustomAgent does too now, so we
    // additionally verify that build_command produces an `opencode` invocation
    // with -m flag rather than a `bash -lc ...` wrapper.
    let opts = super::super::RunOpts {
        ..Default::default()
    };
    let cmd = agent.build_command("hello", &opts).unwrap();
    let program = cmd.get_program().to_string_lossy().into_owned();
    assert_eq!(program, "opencode");
    let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(args.iter().any(|a| a == "-m"));
    assert!(args.iter().any(|a| a == "mimo/mimo-v2.5-pro"));
}

#[test]
fn delegate_to_opencode_uses_binary_extra_args_and_rate_limit_kind() {
    let toml_data = r#"[agent]
id = "mimocode"
display_name = "MiMo Code"
command = "bash"
delegate_to = "opencode"
forced_model = "mimo/mimo-auto"
binary = "mimo"
extra_args = ["--dangerously-skip-permissions"]
rate_limit_kind = "mimocode"
"#;
    let config = parse_config(toml_data).unwrap();
    let agent = build_agent(&config);
    let opts = super::super::RunOpts {
        ..Default::default()
    };
    let cmd = agent.build_command("hello", &opts).unwrap();
    assert_eq!(cmd.get_program().to_string_lossy(), "mimo");
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    assert!(args.windows(2).any(|pair| pair == ["-m", "mimo/mimo-auto"]));
}

#[test]
fn missing_forced_model_falls_back_to_bash_wrapper() {
    let toml_data = r#"[agent]
id = "broken"
display_name = "Broken"
command = "bash"
delegate_to = "opencode"
"#;
    let config = parse_config(toml_data).unwrap();
    let agent = build_agent(&config);
    let opts = super::super::RunOpts {
        ..Default::default()
    };
    let cmd = agent.build_command("hi", &opts).unwrap();
    // Falls back to plain CustomAgent → command is "bash"
    assert_eq!(cmd.get_program().to_string_lossy(), "bash");
}

#[test]
fn ordinary_claude_delegate_custom_agent_still_resolves() {
    let _home = crate::support::test_env::TestHome::new();
    let dir = agents_dir();
    fs::create_dir_all(&dir).unwrap();
    write_agent(
        &dir,
        "delegate.toml",
        concat!(
            "[agent]\nid = 'delegate'\ndisplay_name = 'Delegate'\ncommand = 'wrapper'\n",
            "delegate_to = 'claude'\nforced_model = 'opus'\n"
        ),
    );
    let agent = resolve_custom_agent("delegate").unwrap();
    assert!(agent.api_route().is_none());
    assert_eq!(agent.rate_limit_name(), Some("delegate"));
}
