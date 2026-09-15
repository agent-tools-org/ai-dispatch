// Prompt detection for PTY-backed agent output.
// Tracks partial lines and exposes immediate and idle-based input heuristics.

use std::time::{Duration, Instant};

const PROMPT_IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PARTIAL_LEN: usize = 240;

#[derive(Debug, Default)]
pub struct PromptDetector {
    partial_line: String,
    pending_since: Option<Instant>,
    signaled: bool,
}

impl PromptDetector {
    pub fn push_chunk(&mut self, chunk: &str, now: Instant) -> Option<String> {
        if let Some((_, tail)) = split_last_line(chunk) {
            self.partial_line = tail.to_string();
            self.signaled = false;
        } else {
            self.partial_line.push_str(chunk);
        }
        trim_partial_line(&mut self.partial_line);
        self.pending_since = (!self.partial_line.is_empty()).then_some(now);
        if is_prompt_candidate(&self.partial_line) && !self.signaled {
            self.signaled = true;
            return Some(self.partial_line.trim().to_string());
        }
        None
    }

    pub fn poll_idle(&mut self, now: Instant) -> Option<String> {
        if self.partial_line.is_empty() || self.signaled {
            return None;
        }
        let since = self.pending_since?;
        if now.duration_since(since) < PROMPT_IDLE_TIMEOUT {
            return None;
        }
        self.signaled = true;
        Some(self.partial_line.trim().to_string())
    }

    pub fn reset_after_input(&mut self) {
        self.partial_line.clear();
        self.pending_since = None;
        self.signaled = false;
    }
}

fn split_last_line(chunk: &str) -> Option<(&str, &str)> {
    chunk.rsplit_once('\n').or_else(|| chunk.rsplit_once('\r'))
}

fn is_prompt_candidate(line: &str) -> bool {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    trimmed.ends_with("? ") || trimmed.ends_with(": ") || trimmed.trim_end().ends_with("(y/n)")
}

fn trim_partial_line(line: &mut String) {
    if line.len() > MAX_PARTIAL_LEN {
        let start = line.len() - MAX_PARTIAL_LEN;
        *line = line[start..].to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::PromptDetector;
    use std::time::{Duration, Instant};

    #[test]
    fn detects_inline_question_prompts() {
        let mut detector = PromptDetector::default();
        let prompt = detector.push_chunk("Continue? ", Instant::now());

        assert_eq!(prompt.as_deref(), Some("Continue?"));
    }

    #[test]
    fn detects_idle_partial_lines_as_prompts() {
        let mut detector = PromptDetector::default();
        let start = Instant::now();

        assert_eq!(detector.push_chunk("Select an option", start), None);
        let prompt = detector.poll_idle(start + Duration::from_secs(2));

        assert_eq!(prompt.as_deref(), Some("Select an option"));
    }

    #[test]
    fn clears_pending_prompt_after_input() {
        let mut detector = PromptDetector::default();
        let start = Instant::now();
        let _ = detector.push_chunk("Name: ", start);
        detector.reset_after_input();

        let prompt = detector.poll_idle(start + Duration::from_secs(3));
        assert_eq!(prompt, None);
    }
}
