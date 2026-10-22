// Profile inference shared by leaderboard selection and caller evidence.
// Exports: inferred_advice; no scoring or state changes.
// Deps: classifier, task difficulty, advice payload.

use super::InferredAdvice;
use crate::agent::classifier::{self, TaskCategory};

pub(super) fn inferred_advice(prompt: &str, kind_override: Option<TaskCategory>) -> InferredAdvice {
    let normalized = prompt.trim().to_lowercase();
    let chars = prompt.chars().count();
    let file_mentions = classifier::count_file_mentions(&normalized);
    let kind = kind_override
        .unwrap_or_else(|| classifier::classify(prompt, file_mentions, chars).category);
    InferredAdvice {
        kind,
        file_mentions,
        chars,
    }
}
