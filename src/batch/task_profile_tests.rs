// Batch TOML coverage for declared task-profile defaults and overrides.
// Verifies all four dimensions plus task-kind propagation.
// Deps: parent batch parser, tempfile, task-profile and classifier enums.

use std::io::Write;

use tempfile::NamedTempFile;

use super::parse_batch_file;
use crate::agent::classifier::TaskCategory;
use crate::types::{TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};

#[test]
fn declared_profile_defaults_propagate_and_tasks_override() {
    let mut file = NamedTempFile::new().expect("batch file");
    file.write_all(br#"
[defaults]
agent = "codex"
difficulty = "moderate"
budget = "standard"
urgency = "normal"
rigor = "standard"
kind = "testing"

[[task]]
name = "defaulted"
prompt = "Run tests"

[[task]]
name = "critical"
prompt = "Refactor"
difficulty = "complex"
budget = "premium"
urgency = "urgent"
rigor = "critical"
kind = "refactoring"
"#).expect("write batch");
    file.flush().expect("flush batch");

    let config = parse_batch_file(file.path()).expect("parse batch");
    let defaulted = &config.tasks[0];
    assert_eq!(defaulted.difficulty, Some(TaskDifficulty::Moderate));
    assert_eq!(defaulted.budget, Some(TaskBudget::Standard));
    assert_eq!(defaulted.urgency, Some(TaskUrgency::Normal));
    assert_eq!(defaulted.rigor, Some(TaskRigor::Standard));
    assert_eq!(defaulted.kind, Some(TaskCategory::Testing));
    let critical = &config.tasks[1];
    assert_eq!(critical.difficulty, Some(TaskDifficulty::Complex));
    assert_eq!(critical.budget, Some(TaskBudget::Premium));
    assert_eq!(critical.urgency, Some(TaskUrgency::Urgent));
    assert_eq!(critical.rigor, Some(TaskRigor::Critical));
    assert_eq!(critical.kind, Some(TaskCategory::Refactoring));
}
