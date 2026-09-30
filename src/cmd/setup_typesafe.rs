// TypeSafe setup: probe key presence and let security own terminal key entry.
// Exports: run; dependencies: std I/O/process and the shared TypeSafe classifier.

use crate::typesafe::classify::{self, ClassifyError, ClassifyRequest, ClassifyState, KEY_SETUP};
use anyhow::Result;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};

pub(super) fn run() -> Result<()> {
    let account = std::env::var("USER").ok();
    run_step(
        cfg!(target_os = "macos"),
        io::stdin().is_terminal(),
        &mut io::stdin().lock(),
        &mut io::stdout(),
        || probe(account.as_deref()),
        || store(account.as_deref()),
        || classify::classify(&verification_request()).map(|_| ()),
    )
}

fn run_step(
    macos: bool,
    tty: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
    probe: impl FnOnce() -> bool,
    store: impl FnOnce() -> Result<bool>,
    verify: impl FnOnce() -> std::result::Result<(), ClassifyError>,
) -> Result<()> {
    if !macos {
        writeln!(
            output,
            "  TypeSafe keys are keychain-only on macOS; skipped."
        )?;
        return Ok(());
    }
    let status = if probe() { "present" } else { "absent" };
    writeln!(
        output,
        "  Login keychain: {status} (typesafe-api-key, account $USER)"
    )?;
    write!(output, "  Set/replace TypeSafe key? [y/N] ")?;
    output.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        writeln!(output, "  Skipped")?;
    } else if !tty {
        writeln!(output, "  Run in a real terminal: {KEY_SETUP}")?;
    } else {
        match store() {
            Ok(true) => match verify() {
                Ok(()) => writeln!(output, "  OK")?,
                Err(error) => writeln!(output, "  FAILED: {}", error.message)?,
            },
            Ok(false) => writeln!(output, "  Keychain store failed")?,
            Err(_) => writeln!(output, "  Could not run /usr/bin/security")?,
        }
    }
    Ok(())
}

fn security_command(action: &str, account: &str) -> Command {
    let mut command = Command::new("/usr/bin/security");
    command.args([action, "-a", account, "-s", "typesafe-api-key"]);
    command
}

fn probe(account: Option<&str>) -> bool {
    let Some(account) = account.filter(|account| !account.is_empty()) else {
        return false;
    };
    security_command("find-generic-password", account)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn store(account: Option<&str>) -> Result<bool> {
    let account = account
        .filter(|account| !account.is_empty())
        .ok_or_else(|| anyhow::anyhow!("USER is not set"))?;
    Ok(security_command("add-generic-password", account)
        .args(["-U", "-w"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?
        .success())
}

fn verification_request() -> ClassifyRequest {
    ClassifyRequest {
        state: ClassifyState::Text("Hello.".into()),
        questions: serde_json::Map::from_iter([(
            "greeting".into(),
            serde_json::json!({"type": "noul", "instructions": "Is this a greeting?"}),
        )]),
        model: classify::DEFAULT_MODEL.into(),
        timeout_secs: classify::DEFAULT_TIMEOUT_SECS,
        allow_secret_like: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesafe::classify::ErrorKind;
    use std::cell::Cell;

    #[test]
    fn presence_status_and_default_skip() {
        for present in [false, true] {
            for answer in ["\n", "n\n", ""] {
                let mut output = Vec::new();
                run_step(
                    true,
                    true,
                    &mut answer.as_bytes(),
                    &mut output,
                    || present,
                    || panic!("must not store"),
                    || panic!("must not verify"),
                )
                .unwrap();
                let text = String::from_utf8(output).unwrap();
                assert!(text.contains(if present {
                    "Login keychain: present"
                } else {
                    "Login keychain: absent"
                }));
                assert!(text.contains("Set/replace TypeSafe key? [y/N]"));
                assert!(text.contains("Skipped"));
            }
        }
    }

    #[test]
    fn non_tty_yes_prints_manual_command_without_spawning() {
        let mut output = Vec::new();
        run_step(
            true,
            false,
            &mut &b"y\n"[..],
            &mut output,
            || false,
            || panic!("must not store"),
            || panic!("must not verify"),
        )
        .unwrap();
        assert!(String::from_utf8(output).unwrap().contains(KEY_SETUP));
    }

    #[test]
    fn non_macos_skips_all_keychain_and_network_operations() {
        let mut input = &b"y\n"[..];
        let mut output = Vec::new();
        run_step(
            false,
            true,
            &mut input,
            &mut output,
            || panic!("must not probe"),
            || panic!("must not store"),
            || panic!("must not verify"),
        )
        .unwrap();
        assert_eq!(input, b"y\n");
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("keychain-only on macOS; skipped")
        );
    }

    #[test]
    fn tty_yes_stores_then_verifies_once() {
        let stored = Cell::new(false);
        let mut output = Vec::new();
        run_step(
            true,
            true,
            &mut &b"YES\n"[..],
            &mut output,
            || false,
            || {
                stored.set(true);
                Ok(true)
            },
            || {
                assert!(stored.get());
                Ok(())
            },
        )
        .unwrap();
        assert!(String::from_utf8(output).unwrap().ends_with("  OK\n"));
    }

    #[test]
    fn failed_store_never_verifies() {
        for result in [Ok(false), Err(anyhow::anyhow!("private diagnostic"))] {
            let mut output = Vec::new();
            run_step(
                true,
                true,
                &mut &b"y\n"[..],
                &mut output,
                || false,
                || result,
                || panic!("must not verify"),
            )
            .unwrap();
            let text = String::from_utf8(output).unwrap();
            assert!(!text.contains("OK"));
            assert!(!text.contains("private diagnostic"));
        }
    }

    #[test]
    fn verification_reports_failure_message() {
        for (kind, message) in [
            (ErrorKind::NoKey, "no TypeSafe key in the login keychain"),
            (ErrorKind::Api, "TypeSafe API error: HTTP 401"),
            (ErrorKind::Invalid, "timeout must be 1-300 seconds"),
            (ErrorKind::Refused, "state contains secret-like text"),
        ] {
            let mut output = Vec::new();
            run_step(
                true,
                true,
                &mut &b"y\n"[..],
                &mut output,
                || true,
                || Ok(true),
                || Err(ClassifyError::new(kind, message)),
            )
            .unwrap();
            let text = String::from_utf8(output).unwrap();
            assert!(text.ends_with(&format!("  FAILED: {message}\n")));
        }
    }

    #[test]
    fn probe_command_never_requests_password() {
        let command = security_command("find-generic-password", "test-account");
        assert_eq!(command.get_program(), "/usr/bin/security");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "find-generic-password",
                "-a",
                "test-account",
                "-s",
                "typesafe-api-key"
            ]
        );
        assert!(!probe(None));
        assert!(store(None).is_err());
    }

    #[test]
    fn verification_uses_one_valid_tiny_question() {
        let request = verification_request();
        assert_eq!(request.questions.len(), 1);
        assert!(crate::typesafe::question::validate_questions(&request.questions).is_ok());
        assert_eq!(request.model, classify::DEFAULT_MODEL);
        assert!(!request.allow_secret_like);
    }
}
