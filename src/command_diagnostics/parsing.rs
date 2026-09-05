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
            history::record("parse", error.exit_code(), vec![Issue::new(
                &code, &parser_message(&error), &hint,
            )]);
            let _ = error.print();
            eprintln!("\n[aid] {hint}");
            std::process::exit(error.exit_code());
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
