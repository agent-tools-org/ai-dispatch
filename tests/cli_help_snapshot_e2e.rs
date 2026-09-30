// E2E snapshot of `--help` for subcommand groups whose clap enums are shared with handlers,
// plus proof that `aid finding` and `aid group finding` reach the same handler.
// Deps: compiled aid binary, tempfile, tests/fixtures/cli_help_snapshot.txt.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

const COMMANDS: &[&[&str]] = &[
    &["container"],
    &["container", "build"],
    &["container", "list"],
    &["container", "stop"],
    &["store"],
    &["store", "browse"],
    &["store", "install"],
    &["store", "show"],
    &["store", "update"],
    &["team"],
    &["team", "list"],
    &["team", "show"],
    &["team", "create"],
    &["team", "delete"],
    &["byok"],
    &["byok", "apply"],
    &["byok", "remove"],
    &["byok", "probe"],
    &["byok", "example"],
    &["byok", "doc"],
    &["credential"],
    &["credential", "list"],
    &["credential", "add"],
    &["credential", "remove"],
    &["project"],
    &["project", "init"],
    &["project", "show"],
    &["project", "state"],
    &["project", "sync"],
    &["finding"],
    &["finding", "add"],
    &["finding", "list"],
    &["finding", "get"],
    &["finding", "update"],
    &["group"],
    &["group", "finding"],
    &["group", "finding", "add"],
    &["group", "finding", "list"],
    &["group", "finding", "get"],
    &["group", "finding", "update"],
];

fn render_help(aid_home: &TempDir) -> String {
    let mut rendered = String::new();
    for path in COMMANDS {
        let output = aid_cmd_in(aid_home.path()).args(*path).arg("--help").output().unwrap();
        assert!(output.status.success(), "aid {} --help failed", path.join(" "));
        rendered.push_str(&format!("$ aid {} --help\n", path.join(" ")));
        rendered.push_str(&String::from_utf8_lossy(&output.stdout));
        rendered.push('\n');
    }
    rendered
}

#[test]
fn shared_subcommand_help_matches_snapshot() {
    let aid_home = TempDir::new().unwrap();
    let rendered = render_help(&aid_home);
    let expected = include_str!("fixtures/cli_help_snapshot.txt");
    for (line_no, (actual, wanted)) in rendered.lines().zip(expected.lines()).enumerate() {
        assert_eq!(actual, wanted, "help snapshot differs at line {}", line_no + 1);
    }
    assert_eq!(rendered.lines().count(), expected.lines().count(), "help snapshot length differs");
}

#[test]
fn group_finding_and_finding_share_one_handler() {
    let aid_home = TempDir::new().unwrap();
    let run = |args: &[&str]| {
        let output = aid_cmd_in(aid_home.path()).args(args).output().unwrap();
        assert!(output.status.success(), "aid {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    run(&["group", "create", "snap", "--id", "wg-snap"]);
    run(&["group", "finding", "add", "wg-snap", "posted via group", "--severity", "high"]);
    run(&["finding", "add", "wg-snap", "posted via finding"]);
    for listing in [run(&["finding", "list", "wg-snap"]), run(&["group", "finding", "list", "wg-snap"])] {
        assert!(listing.contains("posted via group"), "{listing}");
        assert!(listing.contains("posted via finding"), "{listing}");
    }
    let count = run(&["group", "finding", "list", "wg-snap", "--count", "--severity", "high"]);
    assert_eq!(count.trim(), "1");
}
