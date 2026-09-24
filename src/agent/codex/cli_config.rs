// Reads the codex CLI's own configured default model from its config.toml.
// Exports: configured_model (live lookup) and parse_configured_model (pure).
// Deps: toml; honours `$CODEX_HOME`, else `~/.codex`.

use std::path::PathBuf;

#[cfg(test)]
thread_local! {
    static TEST_CODEX_HOME: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Points `configured_model` at a test directory on the current thread.
#[cfg(test)]
pub(crate) fn set_test_codex_home(home: Option<PathBuf>) {
    TEST_CODEX_HOME.with(|cell| *cell.borrow_mut() = home);
}

fn codex_home() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(home) = TEST_CODEX_HOME.with(|cell| cell.borrow().clone()) {
        return Some(home);
    }
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex"))
}

/// Top-level `model` from the codex config; `None` when unset or unreadable.
pub(crate) fn configured_model() -> Option<String> {
    let path = codex_home()?.join("config.toml");
    parse_configured_model(&std::fs::read_to_string(path).ok()?)
}

pub(crate) fn parse_configured_model(text: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let model = table.get("model")?.as_str()?.trim();
    (!model.is_empty()).then(|| model.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_top_level_model_only() {
        let text = "model = \"gpt-6-sol\"\n[profiles.fast]\nmodel = \"gpt-6-luna\"\n";
        assert_eq!(parse_configured_model(text).as_deref(), Some("gpt-6-sol"));
    }

    #[test]
    fn missing_blank_or_invalid_model_is_none() {
        assert_eq!(
            parse_configured_model("[profiles.fast]\nmodel = \"x\"\n"),
            None
        );
        assert_eq!(parse_configured_model("model = \"  \"\n"), None);
        assert_eq!(parse_configured_model("model = 5\n"), None);
        assert_eq!(parse_configured_model("not toml ["), None);
    }

    #[test]
    fn configured_model_reads_config_toml_under_codex_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("config.toml"), "model = \"gpt-6-sol\"\n").expect("config");
        set_test_codex_home(Some(temp.path().to_path_buf()));
        let model = configured_model();
        set_test_codex_home(None);
        assert_eq!(model.as_deref(), Some("gpt-6-sol"));
    }
}
