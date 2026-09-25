// `aid classify`: builds a classify request from flags and state input, prints JSON or a one-line error.
// Exports: run (returns the process exit code), request_from_args.
// Deps: cli::command_args_classify, typesafe::{classify, question}.

use serde_json::{Map, Value};
use std::io::Read;

use crate::cli::command_args_classify::ClassifyArgs;
use crate::typesafe::classify::{ClassifyError, ClassifyRequest, ClassifyState, ErrorKind, classify};
use crate::typesafe::question::{merge_questions, questions_from_flags};

/// Prints the answer JSON on stdout (exit 0) or one line on stderr, returning the exit code.
pub fn run(args: ClassifyArgs) -> i32 {
    if args.flags.batch.is_some() {
        return super::classify_batch::run(&args);
    }
    let outcome = request_from_args(&args, &mut std::io::stdin()).and_then(|request| classify(&request));
    match outcome {
        Ok(answer) => {
            println!("{answer}");
            0
        }
        Err(err) => {
            eprintln!("aid classify: {err}");
            err.exit_code()
        }
    }
}

pub(crate) fn request_from_args(args: &ClassifyArgs, stdin: &mut dyn Read) -> Result<ClassifyRequest, ClassifyError> {
    let mut request = request_template(args)?;
    let raw = read_state(args.flags.state.as_deref(), stdin)?;
    request.state = if args.flags.state_json { ClassifyState::Json(parse_json_state(&raw)?) } else { ClassifyState::Text(raw) };
    Ok(request)
}

pub(super) fn request_template(args: &ClassifyArgs) -> Result<ClassifyRequest, ClassifyError> {
    let invalid = |message: String| ClassifyError::new(ErrorKind::Invalid, message);
    let flags = questions_from_flags(&args.events).map_err(invalid)?;
    let file = args.flags.questions.as_deref().map(read_questions_file).transpose()?;
    let questions = merge_questions(file, flags).map_err(invalid)?;
    Ok(ClassifyRequest {
        state: ClassifyState::Text("batch template".into()),
        questions,
        model: args.flags.model.clone(),
        timeout_secs: args.flags.timeout,
        allow_secret_like: args.flags.allow_secret_like,
    })
}

fn read_questions_file(path: &str) -> Result<Map<String, Value>, ClassifyError> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| ClassifyError::new(ErrorKind::Invalid, format!("cannot read --questions {path}: {err}")))?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(ClassifyError::new(ErrorKind::Invalid, format!("--questions {path} is not a JSON object"))),
    }
}

fn read_state(path: Option<&str>, stdin: &mut dyn Read) -> Result<String, ClassifyError> {
    let mut text = String::new();
    let read = match path {
        None | Some("-") => stdin.read_to_string(&mut text).map(|_| ()),
        Some(path) => std::fs::read_to_string(path).map(|content| text = content),
    };
    read.map_err(|err| {
        let source = path.filter(|path| *path != "-").unwrap_or("stdin");
        ClassifyError::new(ErrorKind::Invalid, format!("cannot read state from {source}: {err}"))
    })?;
    Ok(text)
}

fn parse_json_state(raw: &str) -> Result<Value, ClassifyError> {
    match serde_json::from_str::<Value>(raw) {
        Ok(value @ (Value::Object(_) | Value::Array(_))) => Ok(value),
        _ => Err(ClassifyError::new(ErrorKind::Invalid, "--state-json expects a JSON object or array")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use crate::cli::{Cli, Commands};

    fn args(argv: &[&str]) -> ClassifyArgs {
        let full = ["aid", "classify"].iter().chain(argv).copied();
        match Cli::try_parse_from(full).expect("parse").command {
            Some(Commands::Classify(args)) => args,
            _ => panic!("expected classify"),
        }
    }

    fn build(argv: &[&str], stdin: &str) -> Result<ClassifyRequest, ClassifyError> {
        request_from_args(&args(argv), &mut stdin.as_bytes())
    }

    #[test]
    fn stdin_is_the_default_state_and_dash_means_stdin() {
        let request = build(&["--noul", "a=A?"], "hello").expect("request");
        assert_eq!(request.state, ClassifyState::Text("hello".into()));
        let dashed = build(&["--state", "-", "--noul", "a=A?"], "piped").expect("request");
        assert_eq!(dashed.state, ClassifyState::Text("piped".into()));
    }

    #[test]
    fn questions_file_merges_with_flags() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("q.json");
        std::fs::write(&file, r#"{"f":{"type":"noul","instructions":"F?"}}"#).expect("write");
        let path = file.to_str().expect("utf8");
        let request = build(&["--questions", path, "--noul", "a=A?"], "x").expect("request");
        assert_eq!(request.questions.keys().collect::<Vec<_>>(), ["a", "f"]);
        let clash = build(&["--questions", path, "--noul", "f=Again?"], "x").unwrap_err();
        assert_eq!(clash.kind, ErrorKind::Invalid);
        std::fs::write(&file, "[1]").expect("write");
        assert_eq!(build(&["--questions", path], "x").unwrap_err().exit_code(), 4);
    }

    #[test]
    fn json_state_must_be_object_or_array() {
        let request = build(&["--state-json", "--noul", "a=A?"], r#"{"k":[1]}"#).expect("request");
        assert_eq!(request.state, ClassifyState::Json(serde_json::json!({ "k": [1] })));
        for bad in ["\"text\"", "not json", "3"] {
            assert_eq!(build(&["--state-json", "--noul", "a=A?"], bad).unwrap_err().exit_code(), 4, "{bad}");
        }
    }

    #[test]
    fn unpaired_flags_and_missing_files_exit_4() {
        assert_eq!(build(&["--choice", "a=A?"], "x").unwrap_err().exit_code(), 4);
        assert_eq!(build(&["--noul", "a=A?", "--state", "/nonexistent/state"], "").unwrap_err().exit_code(), 4);
    }
}
