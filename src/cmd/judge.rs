// Judge module: auto-review task output and decide PASS/RETRY.
// Exports: judge_task(), gather_diff(), read_output().
// Deps: crate::store::Store, crate::types::Task.
use anyhow::{Context, Result};
use std::{env, path::Path, process::{Command as StdCommand, Stdio}};
use tokio::process::Command;
use crate::types::Task;

const MAX_DIFF_CHARS: usize = 8000;

pub struct JudgeResult {
    pub passed: bool,
    pub feedback: String,
}

pub struct PeerReview {
    pub score: u8,
    pub feedback: String,
}

pub async fn judge_task(task: &Task, judge_agent: &str, original_prompt: &str) -> Result<JudgeResult> {
    let diff = judge_review_material(task);
    let truncated = truncate_diff(&diff, MAX_DIFF_CHARS);
    let prompt = format!(
        concat!(
            "You are a code review judge.\n\n",
            "## Original task\n{}\n\n",
            "## Output\n```\n{}\n```\n\n",
            "## Instructions\n",
            "Review whether the output satisfies the original task.\n",
            "Your FIRST line of output MUST be exactly one of:\n",
            "  PASS: <brief reason>\n",
            "  RETRY: <what needs to be fixed>\n",
            "Do NOT output anything before PASS or RETRY.",
        ),
        original_prompt, truncated,
    );
    let exe = env::current_exe().context("Failed to locate aid binary")?;
    let output = Command::new(exe)
        .args(["run", judge_agent, &prompt, "--dir", "."])
        .current_dir(task.repo_path.as_deref().unwrap_or("."))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .context("Judge subprocess failed")?;
    if !output.status.success() {
        anyhow::bail!("Judge agent exited: {}", output.status);
    }
    parse_judge_response(&String::from_utf8_lossy(&output.stdout))
}

pub async fn peer_review_task(task: &Task, reviewer_agent: &str, original_prompt: &str) -> Result<PeerReview> {
    let diff = judge_review_material(task);
    let truncated = truncate_diff(&diff, MAX_DIFF_CHARS);
    let prompt = format!(
        concat!(
            "You are a code review peer.\n\n",
            "## Original task\n{}\n\n",
            "## Output\n```\n{}\n```\n\n",
            "## Instructions\n",
            "Score the output quality from 1-10 and provide brief feedback.\n",
            "Your FIRST line MUST be: SCORE: <number>/10\n",
            "Then provide 1-3 lines of feedback.\n",
        ),
        original_prompt, truncated,
    );
    let exe = std::env::current_exe().context("Failed to locate aid binary")?;
    let output = tokio::process::Command::new(exe)
        .args(["run", reviewer_agent, &prompt, "--dir", "."])
        .current_dir(task.repo_path.as_deref().unwrap_or("."))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .context("Peer review subprocess failed")?;
    if !output.status.success() {
        anyhow::bail!("Peer reviewer exited: {}", output.status);
    }
    parse_peer_review(&String::from_utf8_lossy(&output.stdout))
}

pub(crate) fn gather_diff(task: &Task) -> Option<String> {
    let dir = task.worktree_path.as_deref().or(task.repo_path.as_deref())?;
    if !Path::new(dir).exists() {
        return None;
    }
    let diff_args = match task.start_sha.as_deref() {
        Some(start_sha) => vec![vec!["diff", "--no-color", start_sha, "--"]],
        None => vec![
            vec!["diff", "--no-color", "HEAD", "--"],
            vec!["diff", "--no-color", "HEAD~1..HEAD", "--"],
        ],
    };
    for args in diff_args {
        let output = StdCommand::new("git").current_dir(dir).args(args).output().ok()?;
        if output.status.success() {
            let diff = String::from_utf8_lossy(&output.stdout).into_owned();
            if !diff.trim().is_empty() {
                return Some(diff);
            }
        }
    }
    None
}

pub(crate) fn judge_review_material(task: &Task) -> String {
    let notice = crate::cmd::show::missing_owned_output_absence(task);
    let body = gather_diff(task)
        .or_else(|| read_output(task))
        .unwrap_or_else(|| "(no diff or output)".to_string());
    match notice {
        Some(notice) => format!("{notice}\n{body}"),
        None => body,
    }
}

pub(crate) fn read_output(task: &Task) -> Option<String> {
    // Prefer task-owned path resolution (never CWD-relative `-o` leakage).
    if let Ok(text) = crate::cmd::show::read_task_output(task)
        && !text.trim().is_empty()
    {
        return Some(text);
    }
    None
}

fn truncate_diff(diff: &str, max_chars: usize) -> &str {
    if diff.len() <= max_chars {
        return diff;
    }
    // Find a safe split point at a newline boundary
    match diff[..max_chars].rfind('\n') {
        Some(pos) => &diff[..pos],
        None => &diff[..max_chars],
    }
}

fn parse_judge_response(text: &str) -> Result<JudgeResult> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .context("judge response is empty")?;
    let upper = line.to_uppercase();
    let (passed, prefix_len) = if upper.starts_with("PASS:") {
        (true, "PASS:".len())
    } else if upper.starts_with("RETRY:") {
        (false, "RETRY:".len())
    } else {
        anyhow::bail!("judge response has no explicit first-line PASS:/RETRY: verdict");
    };
    Ok(JudgeResult {
        passed,
        feedback: line[prefix_len..].trim().to_string(),
    })
}

fn parse_peer_review(text: &str) -> Result<PeerReview> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .context("peer review response is empty")?;
    let rest = line
        .strip_prefix("SCORE:")
        .context("peer review has no explicit first-line SCORE: verdict")?
        .trim();
    let score = rest
        .split('/')
        .next()
        .context("peer review score is missing")?
        .trim()
        .parse::<u8>()
        .context("peer review score is not an integer")?;
    if !(1..=10).contains(&score) {
        anyhow::bail!("peer review score must be between 1 and 10");
    }
    let feedback = text
        .lines()
        .skip_while(|candidate| candidate.trim() != line)
        .skip(1)
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(PeerReview { score, feedback })
}

#[cfg(test)]
#[path = "judge_diff_tests.rs"]
mod diff_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_first_line_pass() {
        let result = parse_judge_response("PASS: looks good").unwrap();
        assert!(result.passed);
        assert_eq!(result.feedback, "looks good");
    }

    #[test]
    fn parse_first_line_retry() {
        let result = parse_judge_response("RETRY: missing tests").unwrap();
        assert!(!result.passed);
        assert_eq!(result.feedback, "missing tests");
    }

    #[test]
    fn parse_verdict_after_prose_is_inconclusive() {
        let text = "Looking at the diff, I can see changes were made.\nThe implementation looks complete.\nPASS: all requirements met";
        assert!(parse_judge_response(text).is_err());
    }

    #[test]
    fn parse_retry_after_reasoning_is_inconclusive() {
        let text = "The task asked for tests but none were added.\nRETRY: add unit tests for the new function";
        assert!(parse_judge_response(text).is_err());
    }

    #[test]
    fn parse_no_verdict_is_inconclusive() {
        let text = "The code looks fine and all changes are appropriate.";
        assert!(parse_judge_response(text).is_err());
    }

    #[test]
    fn parse_empty_response_is_inconclusive() {
        assert!(parse_judge_response("").is_err());
    }

    #[test]
    fn truncate_diff_within_limit() {
        let short = "abc\ndef";
        assert_eq!(truncate_diff(short, 100), short);
    }

    #[test]
    fn truncate_diff_at_newline_boundary() {
        let diff = "line1\nline2\nline3\nline4";
        let result = truncate_diff(diff, 13);
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn parse_peer_review_extracts_score() {
        let text = "SCORE: 8/10\nGood implementation, clean code.";
        let review = parse_peer_review(text).unwrap();
        assert_eq!(review.score, 8);
        assert!(review.feedback.contains("Good implementation"));
    }

    #[test]
    fn parse_peer_review_no_score_is_inconclusive() {
        assert!(parse_peer_review("The code looks fine.").is_err());
    }
}
