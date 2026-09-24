// Capture clap failures before any task/store initialization.
// Exports parse(); keeps clap exit codes/help semantics and adds audit-kind guidance.
// Deps: clap, CLI parser, task-independent history.

use clap::Parser;
use super::{Issue, history};

pub(crate) fn parse() -> crate::cli::Cli {
    match crate::cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if !error.use_stderr() { error.exit(); }
            let code = format!("{:?}", error.kind());
            let hint = parser_hint(&error);
            history::record("parse", exit_code(&error, std::env::args()), vec![Issue::new(
                &code, &parser_message(&error), &hint,
            )]);
            let _ = error.print();
            eprintln!("\n[aid] {hint}");
            std::process::exit(exit_code(&error, std::env::args()));
        }
    }
}

fn parser_message(error: &clap::Error) -> String {
    use clap::error::{ContextKind, ContextValue};
    let mut message = format!("CLI argument parsing failed ({:?}).", error.kind());
    // Store argument identifiers only, never clap's raw invalid input values.
    if let Some(ContextValue::String(arg)) = error.get(ContextKind::InvalidArg) {
        let name = arg.split_whitespace().next().unwrap_or_default();
        if name.starts_with("--") && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            message.push_str(&format!(" Argument: {name}."));
        }
    }
    message
}

fn parser_hint(error: &clap::Error) -> String {
    let args = std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
    let audit_kind = args.windows(2).any(|pair| pair == ["--kind", "audit"])
        || args.iter().any(|arg| arg == "--kind=audit");
    if error.kind() == clap::error::ErrorKind::InvalidValue && audit_kind {
        return "For a bug audit, use --kind debugging --read-only --dir <checkout-path>. --audit schedules an additional post-task cross-audit; it is not a task kind.".into();
    }
    "Use aid <command> --help for valid values and combinations; inspect history with aid errors.".into()
}

/// `aid classify` reserves exit 2 for a missing key, so its usage errors exit 4.
fn exit_code(error: &clap::Error, args: impl Iterator<Item = String>) -> i32 {
    let mut words = args.skip(1).filter(|arg| !arg.starts_with('-'));
    if words.next().as_deref() == Some("classify") { 4 } else { error.exit_code() }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn classify_usage_errors_exit_4_and_others_keep_clap_codes() {
        let argv = |line: &str| line.split(' ').map(String::from).collect::<Vec<_>>();
        let error = crate::cli::Cli::try_parse_from(["aid", "classify", "--bogus"]).err().expect("error");
        assert_eq!(super::exit_code(&error, argv("aid -q classify --bogus").into_iter()), 4);
        let other = crate::cli::Cli::try_parse_from(["aid", "board", "--bogus"]).err().expect("error");
        assert_eq!(super::exit_code(&other, argv("aid board --bogus").into_iter()), 2);
    }
}
