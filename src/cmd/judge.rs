use anyhow::{anyhow, Context, Result};
use std::{env, fs, path::{Path, PathBuf}, process::{Command as StdCommand, Stdio}};
use tokio::process::Command;
use crate::store::Store;
use crate::types::Task;
pub struct JudgeResult {
    pub passed: bool,
    pub feedback: String,
}
pub async fn judge_task(store: &Store, task: &Task, judge_agent: &str, original_prompt: &str) -> Result<JudgeResult> {
    let _ = store;
    let diff = gather_diff(task)
        .or_else(|| read_output(task))
        .unwrap_or_else(|| "(no diff or output)".to_string());
    let prompt = format!("You are a code review judge. Original task: {original_prompt}. Output diff: {diff}. Respond with PASS or RETRY: <feedback>");
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
pub(crate) fn gather_diff(task: &Task) -> Option<String> {
    let dir = task.worktree_path.as_deref().or(task.repo_path.as_deref()).unwrap_or(".");
    if !Path::new(dir).exists() {
        return None;
    }
    let output = StdCommand::new("git")
        .current_dir(dir)
        .args(["diff", "--no-color"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let diff = String::from_utf8_lossy(&output.stdout).into_owned();
    (!diff.trim().is_empty()).then_some(diff)
}
pub(crate) fn read_output(task: &Task) -> Option<String> {
    let Some(output_path) = task.output_path.as_deref() else {
        return None;
    };
    let mut candidates = vec![PathBuf::from(output_path)];
    if let Some(worktree) = task.worktree_path.as_deref() {
        candidates.push(Path::new(worktree).join(output_path));
    }
    for candidate in candidates {
        if candidate.exists() {
            if let Ok(text) = fs::read_to_string(&candidate) {
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}
fn parse_judge_response(text: &str) -> Result<JudgeResult> {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow!("Judge response was empty"))?;
    let trimmed = line.trim();
    let up = trimmed.to_uppercase();
    let (word, passed) = if up.starts_with("PASS") {
        ("PASS", true)
    } else if up.starts_with("RETRY") {
        ("RETRY", false)
    } else {
        return Err(anyhow!("Judge response must start with PASS or RETRY"));
    };
    let feedback = trimmed[word.len()..]
        .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == ':' || c == '-')
        .trim()
        .to_string();
    Ok(JudgeResult { passed, feedback })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_variants() {
        let pass = parse_judge_response("PASS ready").unwrap();
        assert!(pass.passed);
        assert_eq!(pass.feedback, "ready");
        let retry = parse_judge_response("RETRY missing work").unwrap();
        assert!(!retry.passed);
        assert_eq!(retry.feedback, "missing work");
    }
}
