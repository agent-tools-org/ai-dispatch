// Output control: quiet mode and leveled messaging for AI-friendly output.
// When AID_QUIET=1 or --quiet is set, only errors and essential output are shown.
// Exports: is_quiet(), info(), hint(), warn(), error()

use std::sync::atomic::{AtomicBool, Ordering};

static QUIET: AtomicBool = AtomicBool::new(false);

/// Initialize quiet mode from environment.
pub fn init() {
    if std::env::var("AID_QUIET").is_ok_and(|v| v == "1" || v == "true") {
        QUIET.store(true, Ordering::Relaxed);
    }
}

/// Set quiet mode programmatically (e.g. from --quiet flag).
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

#[allow(dead_code)] // Used by aid_info!/aid_hint! macros via $crate::output::is_quiet()
pub fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// Informational message — suppressed in quiet mode.
/// Use for: project detection, auto-applied settings, internal bookkeeping.
#[macro_export]
macro_rules! aid_info {
    ($($arg:tt)*) => {
        if !$crate::output::is_quiet() {
            eprintln!($($arg)*);
        }
    };
}

/// Hint/tip for the user — suppressed in quiet mode.
/// Use for: "aid watch --quiet ...", "aid merge ...", TUI suggestions.
#[macro_export]
macro_rules! aid_hint {
    ($($arg:tt)*) => {
        if !$crate::output::is_quiet() {
            eprintln!($($arg)*);
        }
    };
}

/// Warning — always shown, even in quiet mode.
/// Use for: rate limits, disk space, scope violations, audit safety.
#[macro_export]
macro_rules! aid_warn {
    ($($arg:tt)*) => {{
        eprintln!($($arg)*);
    }};
}

/// Error — always shown, even in quiet mode.
#[macro_export]
macro_rules! aid_error {
    ($($arg:tt)*) => {{
        eprintln!($($arg)*);
    }};
}

/// Progress update — always shown on stdout, even in quiet mode.
#[macro_export]
macro_rules! aid_progress {
    ($($arg:tt)*) => {{
        println!($($arg)*);
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn quiet_mode_defaults_off() {
        assert!(!is_quiet());
    }

    #[test]
    fn set_quiet_toggles_mode() {
        set_quiet(true);
        assert!(is_quiet());
        set_quiet(false);
        assert!(!is_quiet());
    }

    #[test]
    fn aid_progress_prints_to_stdout_even_in_quiet_mode() {
        const CHILD_ENV: &str = "AID_PROGRESS_TEST_CHILD";
        if std::env::var_os(CHILD_ENV).is_some() {
            set_quiet(true);
            aid_progress!("[batch] t-123 dispatched (codex: task)");
            set_quiet(false);
            return;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "output::tests::aid_progress_prints_to_stdout_even_in_quiet_mode",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("[batch] t-123 dispatched (codex: task)\n"));
    }
}
