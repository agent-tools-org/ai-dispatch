// Exercise provider clear-limit names through the public parsed config action.
// Checks independent marker scopes, clear-all discovery and path rejection.
use super::*;
use clap::Parser;

#[test]
fn provider_clear_limit_scopes_one_route_and_all_discovers_every_provider() {
    let (_home, store) = crate::support::test_env::TestHome::new().shared_store();
    let first = "codex-provider:X";
    let second = "codex-provider:Y";
    for name in [Some(first), Some(second), None] {
        fs::write(
            rate_limit::marker_path(&AgentKind::Codex, name),
            "hold: manual\n",
        )
        .unwrap();
    }
    let cli = crate::cli::Cli::try_parse_from(["aid", "config", "clear-limit", first]).unwrap();
    let Some(crate::cli::Commands::Config(args)) = cli.command else {
        panic!("config action")
    };
    run(&store, args.action).unwrap();
    assert!(!rate_limit::marker_path(&AgentKind::Codex, Some(first)).exists());
    assert!(rate_limit::marker_path(&AgentKind::Codex, Some(second)).exists());
    assert!(rate_limit::marker_path(&AgentKind::Codex, None).exists());
    for name in [
        "codex-provider:",
        "codex-provider:../X",
        "codex-provider:X/Y",
        "codex-provider:X.Y",
    ] {
        assert!(clear_limit(name).is_err(), "{name}");
    }
    let orphan =
        crate::support::paths::aid_dir().join("rate-limit-codex-provider:orphan--capacity-rpm");
    fs::write(&orphan, "hold: manual\n").unwrap();
    clear_limit("all").unwrap();
    assert!(!rate_limit::marker_path(&AgentKind::Codex, Some(second)).exists());
    assert!(!rate_limit::marker_path(&AgentKind::Codex, None).exists());
    assert!(!orphan.exists());
}
