// Profile inference shared by leaderboard selection and caller evidence.
// Exports: inferred_advice, complexity_for; no scoring or state changes.
// Deps: classifier, task difficulty, advice payload.

use super::InferredAdvice;
use crate::agent::classifier::{self, Complexity, TaskCategory};
use crate::types::TaskDifficulty;

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

pub(super) fn complexity_for(difficulty: TaskDifficulty) -> Complexity {
    match difficulty {
        TaskDifficulty::Trivial | TaskDifficulty::Simple => Complexity::Low,
        TaskDifficulty::Moderate => Complexity::Medium,
        TaskDifficulty::Complex => Complexity::High,
    }
}
