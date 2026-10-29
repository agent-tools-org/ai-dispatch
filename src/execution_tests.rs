// Protocol discovery and refusal tests without store or process side effects.
// The CLI parser is exercised before admission checks.
use super::*;
use crate::cli::{Cli, Commands};
use clap::Parser;

fn args(flags: &[&str]) -> crate::cli::run_args::RunArgs {
    let mut argv = vec!["aid", "run", "codex", "prompt", "--policy", "caller-owned"];
    argv.extend_from_slice(flags);
    let Some(Commands::Run(args)) = Cli::try_parse_from(argv).expect("parse").command else {
        panic!("run");
    };
    args
}

#[test]
fn caller_owned_capability_contract() {
    let caps = capabilities();
    assert!(caps["policies"]["caller-owned"]["refused_flags"].as_array().expect("flags")
        .contains(&json!("dry_run")));
    assert_eq!(caps["protocol"], PROTOCOL);
    assert_eq!(caps["policies"]["caller-owned"]["version"], 1);
    assert_eq!(caps["policies"]["caller-owned"]["refused_flags"], json!(REFUSED));
    assert!(Cli::try_parse_from(["aid", "capabilities", "--json"]).is_ok());
}

#[test]
fn caller_owned_requires_id_model_and_known_policy() {
    for (flags, code) in [
        (vec![], "id_required"),
        (vec!["--id", "bad/id", "--model", "m"], "id_required"),
        (vec!["--id", "t-explicit"], "model_required"),
    ] {
        let err = validate_cli(&args(&flags)).expect_err("refuse");
        let refusal = err.downcast_ref::<Refusal>().expect("typed refusal");
        assert_eq!(refusal.code, code);
        assert_eq!(refusal.exit, 3);
        assert_eq!(refusal.body()["protocol"], PROTOCOL);
    }
    let mut cli = args(&["--id", "t-explicit", "--model", "m"]);
    cli.policy = Some("unknown".into());
    assert_eq!(
        validate_cli(&cli)
            .expect_err("unsupported")
            .downcast_ref::<Refusal>()
            .expect("refusal")
            .code,
        "policy_unsupported"
    );
}

#[test]
fn caller_owned_refuses_every_explicit_execution_expansion_flag() {
    for flags in [
        vec!["--retry", "1"],
        vec!["--cascade", "agy"],
        vec!["--best-of", "1"],
        vec!["--iterate", "1"],
        vec!["--judge"],
        vec!["--peer-review", "agy"],
        vec!["--audit"],
        vec!["--budget", "standard"],
        vec!["--dry-run"],
    ] {
        let mut base = vec!["--id", "t-explicit", "--model", "m"];
        base.extend(flags);
        let err = validate_cli(&args(&base)).expect_err("refused flag");
        let refusal = err.downcast_ref::<Refusal>().expect("typed refusal");
        assert_eq!(refusal.code, "flag_refused");
        assert!(REFUSED.contains(&refusal.detail.as_deref().expect("flag detail")));
    }
    validate_cli(&args(&["--id", "t-explicit", "--model", "m", "--retry", "0"]))
        .expect("zero retries allowed");
}
