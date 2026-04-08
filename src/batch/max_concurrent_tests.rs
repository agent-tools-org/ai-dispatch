// Tests for batch-level max_concurrent TOML parsing.
// Exports: module-local parser coverage only.
// Deps: crate::batch::BatchConfig, toml

use crate::batch::BatchConfig;

#[test]
fn defaults_max_concurrent_deserializes_from_toml() {
    let config: BatchConfig = toml::from_str(
        r#"
[defaults]
max_concurrent = 6

[[tasks]]
agent = "codex"
prompt = "test"
"#,
    )
    .unwrap();

    assert_eq!(config.defaults.max_concurrent, Some(6));
}
