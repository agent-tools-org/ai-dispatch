// Codex command-output classification helpers.
// Exports classify_output for mapping aggregated shell output to event kinds.
// Depends on EventKind and manual line matching only.

use crate::types::EventKind;

pub(crate) fn classify_output(output: &str) -> Option<EventKind> {
    if output_contains_error(output) {
        Some(EventKind::Error)
    } else if output.contains("test result:") {
        Some(EventKind::Test)
    } else if output.contains("Finished") || output.contains("Compiling") {
        Some(EventKind::Build)
    } else {
        None
    }
}

fn output_contains_error(output: &str) -> bool {
    output.lines().any(|line| {
        is_rust_compiler_error(line)
            || line.contains("test result: FAILED")
            || line == "FAILED"
            || line.starts_with("FAILED ")
    })
}

fn is_rust_compiler_error(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("error[E") else {
        return false;
    };
    let Some((digits, _)) = rest.split_once(']') else {
        return false;
    };
    !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::classify_output;
    use crate::types::EventKind;

    #[test]
    fn classifies_rust_compiler_error_lines() {
        assert_eq!(classify_output("error[E0308]: mismatched types"), Some(EventKind::Error));
        assert_eq!(classify_output("src/lib.rs:1:error[E0308]"), None);
        assert_eq!(classify_output("error[Eabc]: nope"), None);
    }

    #[test]
    fn classifies_test_failures_without_substring_false_positive() {
        assert_eq!(classify_output("test result: FAILED. 1 failed"), Some(EventKind::Error));
        assert_eq!(classify_output("FAILED"), Some(EventKind::Error));
        assert_eq!(classify_output("FAILED run failed"), Some(EventKind::Error));
        assert_eq!(classify_output("path.rs:10: TEST FAILED here"), None);
    }

    #[test]
    fn classifies_build_output() {
        assert_eq!(classify_output("   Compiling aid v0.1.0"), Some(EventKind::Build));
        assert_eq!(classify_output("Finished dev profile"), Some(EventKind::Build));
    }
}
