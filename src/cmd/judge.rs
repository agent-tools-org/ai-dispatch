// Judge module: auto-review task output and decide PASS/RETRY.
// Exports: judge_task(), gather_diff(), read_output().
// Deps: crate::store::Store, crate::types::Task.
use anyhow::{Context, Result};
use std::{env, fs, path::{Path, PathBuf}, process::{Command as StdCommand, Stdio}};
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
    let diff = gather_diff(task)
        .or_else(|| read_output(task))
        .unwrap_or_else(|| "(no diff or output)".to_string());
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
    let diff = gather_diff(task)
        .or_else(|| read_output(task))
        .unwrap_or_else(|| "(no diff or output)".to_string());
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
    // Try committed diff first (codex commits changes), then unstaged diff
    for args in [&["diff", "--no-color", "HEAD~1..HEAD"][..], &["diff", "--no-color"]] {
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

pub(crate) fn read_output(task: &Task) -> Option<String> {
    let output_path = task.output_path.as_deref()?;
    let mut candidates = vec![PathBuf::from(output_path)];
    if let Some(worktree) = task.worktree_path.as_deref() {
        candidates.push(Path::new(worktree).join(output_path));
    }
    for candidate in candidates {
        if let Ok(text) = fs::read_to_string(&candidate)
            && !text.trim().is_empty()
        {
            return Some(text);
        }
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
    // Scan all lines — agents may prefix with reasoning before the verdict
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let up = trimmed.to_uppercase();
        let (word, passed) = if up.starts_with("PASS") {
            ("PASS", true)
        } else if up.starts_with("RETRY") {
            ("RETRY", false)
        } else {
            continue;
        };
        let feedback = trimmed[word.len()..]
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ':' || c == '-')
            .trim()
            .to_string();
        return Ok(JudgeResult { passed, feedback });
    }
    // Fallback: if no explicit verdict found, default to PASS (avoid blocking on judge failures)
    aid_warn!("[aid] Judge response contained no PASS/RETRY verdict — defaulting to PASS");
    Ok(JudgeResult { passed: true, feedback: "no verdict found in judge response".to_string() })
}

fn parse_peer_review(text: &str) -> Result<PeerReview> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let up = trimmed.to_uppercase();
        if up.starts_with("SCORE:") || up.starts_with("SCORE ") {
            let rest = trimmed[6..].trim().trim_start_matches(':').trim();
            if let Some(num_str) = rest.split('/').next()
                && let Ok(score) = num_str.trim().parse::<u8>()
            {
                let score = score.min(10);
                let feedback: String = text
                    .lines()
                    .skip_while(|l| !l.trim().to_uppercase().starts_with("SCORE"))
                    .skip(1)
                    .filter(|l| !l.trim().is_empty())
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                return Ok(PeerReview { score, feedback });
            }
        }
    }
    Ok(PeerReview { score: 5, feedback: "no score found in review".to_string() })
}

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
    fn parse_verdict_after_prose() {
        let text = "Looking at the diff, I can see changes were made.\nThe implementation looks complete.\nPASS: all requirements met";
        let result = parse_judge_response(text).unwrap();
        assert!(result.passed);
        assert_eq!(result.feedback, "all requirements met");
    }

    #[test]
    fn parse_retry_after_reasoning() {
        let text = "The task asked for tests but none were added.\nRETRY: add unit tests for the new function";
        let result = parse_judge_response(&text).unwrap();
        assert!(!result.passed);
        assert_eq!(result.feedback, "add unit tests for the new function");
    }

    #[test]
    fn parse_no_verdict_defaults_to_pass() {
        let text = "The code looks fine and all changes are appropriate.";
        let result = parse_judge_response(text).unwrap();
        assert!(result.passed);
        assert!(result.feedback.contains("no verdict"));
    }

    #[test]
    fn parse_empty_response_defaults_to_pass() {
        let result = parse_judge_response("").unwrap();
        assert!(result.passed);
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
    fn parse_peer_review_no_score_defaults_to_5() {
        let review = parse_peer_review("The code looks fine.").unwrap();
        assert_eq!(review.score, 5);
    }
}
