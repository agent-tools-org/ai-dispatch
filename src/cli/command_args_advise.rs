// CLI arguments for read-only agent routing advice.
// Exports: AdviseArgs.
// Deps: classifier task kind, declared task-profile types, clap.

use clap::Args;

use crate::agent::classifier::TaskCategory;
use crate::types::{TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};

#[derive(Args)]
pub struct AdviseArgs {
    pub prompt: String,
    #[arg(long)]
    pub difficulty: TaskDifficulty,
    #[arg(long)]
    pub budget: TaskBudget,
    #[arg(long)]
    pub urgency: TaskUrgency,
    #[arg(long)]
    pub rigor: TaskRigor,
    #[arg(long)]
    pub kind: Option<TaskCategory>,
    #[arg(long)]
    pub team: Option<String>,
    #[arg(long, default_value = "5")]
    pub top: usize,
    #[arg(long)]
    pub json: bool,
    #[arg(short, long)]
    pub dir: Option<String>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};
    use crate::types::{TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};

    #[test]
    fn advise_requires_and_parses_all_declared_dimensions() {
        let cli = Cli::try_parse_from([
            "aid", "advise", "Refactor the router",
            "--difficulty", "complex", "--budget", "premium",
            "--urgency", "urgent", "--rigor", "critical",
        ]).expect("parse advise");
        let Some(Commands::Advise(args)) = cli.command else {
            panic!("expected advise command");
        };
        assert_eq!(args.difficulty, TaskDifficulty::Complex);
        assert_eq!(args.budget, TaskBudget::Premium);
        assert_eq!(args.urgency, TaskUrgency::Urgent);
        assert_eq!(args.rigor, TaskRigor::Critical);
    }

    #[test]
    fn advise_rejects_missing_declared_dimension() {
        let result = Cli::try_parse_from([
            "aid", "advise", "Refactor the router",
            "--difficulty", "complex", "--budget", "premium",
            "--urgency", "urgent",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn run_accepts_nullable_declared_dimensions_and_kind() {
        let cli = Cli::try_parse_from([
            "aid", "run", "codex", "Refactor the router",
            "--difficulty", "complex", "--budget", "premium",
            "--urgency", "urgent", "--rigor", "critical",
            "--kind", "refactoring",
        ]).expect("parse run profile");
        let Some(Commands::Run(args)) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.difficulty, Some(TaskDifficulty::Complex));
        assert_eq!(args.budget, Some(TaskBudget::Premium));
        assert_eq!(args.urgency, Some(TaskUrgency::Urgent));
        assert_eq!(args.rigor, Some(TaskRigor::Critical));
        assert_eq!(args.kind, Some(crate::agent::classifier::TaskCategory::Refactoring));
    }
}
