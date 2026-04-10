// GitButler integration: mode parsing, `but` CLI detection, helpers.
// Used by: project config, dispatch flow, merge workflows.
// Deps: anyhow, std::{env, process, sync::OnceLock}.

use anyhow::{Result, bail};
use std::process::Command;
use std::sync::OnceLock;

static BUT_AVAILABLE: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Off,
    Auto,
    Always,
}

impl Mode {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            other => bail!("unknown gitbutler mode '{other}'"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Always => "always",
        }
    }
}

/// Returns true iff the `but` binary is on PATH and `but --version` succeeds.
/// Result is cached for the process lifetime via OnceLock.
pub fn but_available() -> bool {
    *BUT_AVAILABLE.get_or_init(detect_but_available)
}

/// Resolves whether GitButler features should run for this dispatch.
/// `Always` => true. `Auto` => true iff `but_available()`. `Off` => false.
pub fn is_active(mode: Mode) -> bool {
    match mode {
        Mode::Off => false,
        Mode::Auto => but_available(),
        Mode::Always => true,
    }
}

fn detect_but_available() -> bool {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AID_GITBUTLER_TEST_PRESENT") {
        return matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    }

    Command::new("but")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{Mode, but_available, is_active};

    #[test]
    fn gitbutler_mode_parse_round_trip() {
        for expected in [Mode::Off, Mode::Auto, Mode::Always] {
            let parsed = Mode::from_str(expected.as_str()).unwrap();
            assert_eq!(parsed, expected);
        }
    }

    #[test]
    fn gitbutler_mode_rejects_unknown_value() {
        assert!(Mode::from_str("sometimes").is_err());
    }

    #[test]
    fn gitbutler_is_active_is_false_for_off() {
        assert!(!is_active(Mode::Off));
    }

    #[test]
    fn gitbutler_is_active_is_true_for_always() {
        assert!(is_active(Mode::Always));
    }

    #[test]
    #[ignore = "process-wide cache; enable when explicitly validating detection"]
    fn gitbutler_but_available_respects_test_override() {
        unsafe {
            std::env::set_var("AID_GITBUTLER_TEST_PRESENT", "1");
        }
        assert!(but_available());
    }
}
