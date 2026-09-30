// Fixture table for the porcelain v1 parser and every reading built on it.
// Rows are `git status --porcelain --untracked-files=all` lines, escapes kept literal.
// Deps: status parser, snapshot's aid-ownership check.

use super::super::snapshot::line_is_only_aid_owned;
use super::{
    extract_baseline_path, parse_porcelain_line, parse_status_entry, status_line_path,
    status_line_paths, summarize_status,
};
use crate::worktree::WorktreeStatusKind::{self, Modified, Untracked};

struct Row {
    line: &'static str,
    entry: Option<(WorktreeStatusKind, &'static str)>,
    paths: &'static [&'static str],
    aid_only: bool,
    baseline: Option<&'static str>,
    /// (staged, modified, untracked)
    counts: (usize, usize, usize),
    prune: &'static str,
}

const fn row(
    line: &'static str,
    entry: Option<(WorktreeStatusKind, &'static str)>,
    paths: &'static [&'static str],
    aid_only: bool,
    baseline: Option<&'static str>,
    counts: (usize, usize, usize),
    prune: &'static str,
) -> Row {
    Row { line, entry, paths, aid_only, baseline, counts, prune }
}

const ROWS: &[Row] = &[
    row(" D .aid-lock", None, &[".aid-lock"], true, Some(".aid-lock"), (0, 1, 0), ".aid-lock"),
    row(r#" M "a -> b.txt""#, Some((Modified, r#""a -> b.txt""#)), &[r#""a"#, r#"b.txt""#], false, Some(r#"b.txt""#), (0, 1, 0), r#""a -> b.txt""#),
    row("A  a_", None, &["a_"], false, Some("a_"), (1, 0, 0), "a_"),
    row("AM am", Some((Modified, "am")), &["am"], false, Some("am"), (1, 1, 0), "am"),
    row("R  d_ -> c_dst", None, &["d_", "c_dst"], false, Some("c_dst"), (1, 0, 0), "d_ -> c_dst"),
    row(r#" M "caf\303\251.md""#, Some((Modified, r#""caf\303\251.md""#)), &[r"caf\303\251.md"], false, Some(r#""caf\303\251.md""#), (0, 1, 0), r#""caf\303\251.md""#),
    row("M  m_", Some((Modified, "m_")), &["m_"], false, Some("m_"), (1, 0, 0), "m_"),
    row("MM mm", Some((Modified, "mm")), &["mm"], false, Some("mm"), (1, 1, 0), "mm"),
    row(r#"R  arrow_src.txt -> "p -> q.txt""#, None, &["arrow_src.txt", "p -> q.txt"], false, Some(r#""p -> q.txt""#), (1, 0, 0), r#"arrow_src.txt -> "p -> q.txt""#),
    row(r#" M "q\"uote.txt""#, Some((Modified, r#""q\"uote.txt""#)), &[r#"q\"uote.txt"#], false, Some(r#""q\"uote.txt""#), (0, 1, 0), r#""q\"uote.txt""#),
    row("R  r_ -> r_new", None, &["r_", "r_new"], false, Some("r_new"), (1, 0, 0), "r_ -> r_new"),
    row(r#"R  rs -> "renamed space.txt""#, None, &["rs", "renamed space.txt"], false, Some(r#""renamed space.txt""#), (1, 0, 0), r#"rs -> "renamed space.txt""#),
    row("RM rm -> rm_new", Some((Modified, "rm -> rm_new")), &["rm", "rm_new"], false, Some("rm_new"), (1, 1, 0), "rm -> rm_new"),
    row(" M src/nested/deep/x.rs", Some((Modified, "src/nested/deep/x.rs")), &["src/nested/deep/x.rs"], false, Some("src/nested/deep/x.rs"), (0, 1, 0), "src/nested/deep/x.rs"),
    row(" M subm", Some((Modified, "subm")), &["subm"], false, Some("subm"), (0, 1, 0), "subm"),
    row(" D wd", None, &["wd"], false, Some("wd"), (0, 1, 0), "wd"),
    row(r#" M "with space.txt""#, Some((Modified, r#""with space.txt""#)), &["with space.txt"], false, Some(r#""with space.txt""#), (0, 1, 0), r#""with space.txt""#),
    row(" M wm", Some((Modified, "wm")), &["wm"], false, Some("wm"), (0, 1, 0), "wm"),
    row("?? .aid-lock-new", Some((Untracked, ".aid-lock-new")), &[".aid-lock-new"], true, Some(".aid-lock-new"), (0, 0, 1), ".aid-lock-new"),
    row("?? result-t-abc.md", Some((Untracked, "result-t-abc.md")), &["result-t-abc.md"], true, Some("result-t-abc.md"), (0, 0, 1), "result-t-abc.md"),
    row("?? src/nested/new.rs", Some((Untracked, "src/nested/new.rs")), &["src/nested/new.rs"], false, Some("src/nested/new.rs"), (0, 0, 1), "src/nested/new.rs"),
    row("?? target/debug/app", Some((Untracked, "target/debug/app")), &["target/debug/app"], false, Some("target/debug/app"), (0, 0, 1), "target/debug/app"),
    row(r#"?? "untracked space.txt""#, Some((Untracked, r#""untracked space.txt""#)), &["untracked space.txt"], false, Some(r#""untracked space.txt""#), (0, 0, 1), r#""untracked space.txt""#),
    row(r#"?? "x -> y.txt""#, Some((Untracked, r#""x -> y.txt""#)), &[r#""x"#, r#"y.txt""#], false, Some(r#""x -> y.txt""#), (0, 0, 1), r#""x -> y.txt""#),
    row(r#"?? "\303\274n\303\257.txt""#, Some((Untracked, r#""\303\274n\303\257.txt""#)), &[r"\303\274n\303\257.txt"], false, Some(r#""\303\274n\303\257.txt""#), (0, 0, 1), r#""\303\274n\303\257.txt""#),
    row("C  orig.txt -> copy.txt", None, &["orig.txt", "copy.txt"], false, Some("copy.txt"), (1, 0, 0), "orig.txt -> copy.txt"),
    row("D  gone.bin", None, &["gone.bin"], false, Some("gone.bin"), (1, 0, 0), "gone.bin"),
    row("M  orig.txt", Some((Modified, "orig.txt")), &["orig.txt"], false, Some("orig.txt"), (1, 0, 0), "orig.txt"),
    row("UU conflict.txt", None, &["conflict.txt"], false, Some("conflict.txt"), (1, 1, 0), "conflict.txt"),
    row(r#"?? " lead""#, Some((Untracked, r#"" lead""#)), &[" lead"], false, Some(r#"" lead""#), (0, 0, 1), r#"" lead""#),
    row(r#"?? "tab\tname""#, Some((Untracked, r#""tab\tname""#)), &[r"tab\tname"], false, Some(r#""tab\tname""#), (0, 0, 1), r#""tab\tname""#),
    row(r#"?? "trail ""#, Some((Untracked, r#""trail ""#)), &["trail "], false, Some(r#""trail ""#), (0, 0, 1), r#""trail ""#),
    row("M  .aid-lock", Some((Modified, ".aid-lock")), &[".aid-lock"], true, Some(".aid-lock"), (1, 0, 0), ".aid-lock"),
    row("R  src/lib.rs -> result-t-abcd.md", None, &["src/lib.rs", "result-t-abcd.md"], false, Some("result-t-abcd.md"), (1, 0, 0), "src/lib.rs -> result-t-abcd.md"),
    row("D  x", None, &["x"], false, Some("x"), (1, 0, 0), "x"),
    row("R  x -> .aid-lock", None, &["x", ".aid-lock"], false, Some(".aid-lock"), (1, 0, 0), "x -> .aid-lock"),
    row("?? .aid-lock", Some((Untracked, ".aid-lock")), &[".aid-lock"], true, Some(".aid-lock"), (0, 0, 1), ".aid-lock"),
    row("?? .aid-lock2", Some((Untracked, ".aid-lock2")), &[".aid-lock2"], true, Some(".aid-lock2"), (0, 0, 1), ".aid-lock2"),
    row("", None, &[], false, None, (0, 0, 0), ""),
    row(" M", None, &[], false, None, (0, 1, 0), "M"),
    row("??", None, &[], false, None, (1, 1, 0), "??"),
    row("?? ", Some((Untracked, "")), &[""], false, None, (0, 0, 1), "??"),
    row(" M ", None, &[""], false, None, (0, 1, 0), "M"),
    row("??x", None, &[""], false, None, (1, 1, 0), "??x"),
    row("??xa -> b", None, &["a", "b"], false, Some("b"), (1, 1, 0), "a -> b"),
    row("\u{e9}M x", None, &[" x"], false, Some(" x"), (1, 1, 0), "x"),
];

#[test]
fn every_reading_matches_the_fixture_table() {
    for (index, row) in ROWS.iter().enumerate() {
        let n = index + 1;
        let line = row.line;
        let entry = parse_status_entry(line).map(|entry| (entry.kind, entry.path));
        let expected = row.entry.map(|(kind, path)| (kind, path.to_string()));
        assert_eq!(entry, expected, "row {n} entry: {line:?}");
        assert_eq!(status_line_paths(line), row.paths, "row {n} paths: {line:?}");
        assert_eq!(line_is_only_aid_owned(line), row.aid_only, "row {n} aid-only: {line:?}");
        assert_eq!(extract_baseline_path(line).as_deref(), row.baseline, "row {n} baseline: {line:?}");
        let summary = summarize_status(&[line.to_string()]);
        let counts = (summary.staged, summary.modified, summary.untracked);
        assert_eq!(counts, row.counts, "row {n} counts: {line:?}");
        assert_eq!(status_line_path(line), row.prune, "row {n} prune path: {line:?}");
    }
    assert_eq!(ROWS.len(), 46);
}

#[test]
fn summary_adds_every_line() {
    let lines: Vec<String> = ROWS.iter().map(|row| row.line.to_string()).collect();
    let summary = summarize_status(&lines);
    let staged: usize = ROWS.iter().map(|row| row.counts.0).sum();
    let modified: usize = ROWS.iter().map(|row| row.counts.1).sum();
    let untracked: usize = ROWS.iter().map(|row| row.counts.2).sum();
    assert_eq!((summary.staged, summary.modified, summary.untracked), (staged, modified, untracked));
}

#[test]
fn parser_splits_tracked_renames_and_never_untracked_lines() {
    let rename = parse_porcelain_line("RM rm -> rm_new").expect("rename");
    assert_eq!((rename.x, rename.y), ('R', 'M'));
    assert!(!rename.untracked);
    assert_eq!((rename.path, rename.rest), ("rm_new", "rm -> rm_new"));

    let untracked = parse_porcelain_line(r#"?? "x -> y.txt""#).expect("untracked");
    assert!(untracked.untracked);
    assert_eq!(untracked.path, r#""x -> y.txt""#);

    let not_untracked = parse_porcelain_line("??xa -> b").expect("no separator");
    assert_eq!((not_untracked.x, not_untracked.y, not_untracked.untracked), ('?', '?', false));
    assert_eq!(not_untracked.path, "b");

    assert_eq!(parse_porcelain_line(" M"), None);
    assert_eq!(parse_porcelain_line(" M ").map(|entry| entry.rest), Some(""));
}
