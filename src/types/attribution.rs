// Evidence grade for a task's observed model.
// Exports: AttributionSource, is_router_alias.
// Deps: serde only.

use serde::Serialize;

/// How aid came to believe a particular model ran. Stored alongside
/// `observed_model` so a consumer can demand the strength of evidence its
/// decision deserves instead of treating every value as equally solid.
///
/// The split exists because several CLIs never report a model at all: codex
/// emits 593 KB of JSONL with no model string anywhere, and agy's plain-text
/// output has nothing to read. Without a second grade their tasks are
/// permanently `unknown`, which starves the model-level history that routing is
/// meant to be built on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AttributionSource {
    /// The CLI named the model in its own output. The strongest evidence
    /// available, and the only grade fit for scoring a model's capability.
    Echoed,
    /// aid passed an explicit model and the run succeeded, so that model ran —
    /// a CLI asked for a model it cannot serve fails instead, as `t-bd455a68`
    /// did when the `claude` CLI was handed `gemini-3.6-flash-low`.
    ///
    /// Weaker than an echo: it infers from the absence of a refusal rather than
    /// from a statement. A CLI that silently substitutes a model on success
    /// would defeat it, which is why capability scoring must not accept it.
    ConfirmedBySuccess,
}

impl AttributionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Echoed => "echoed",
            Self::ConfirmedBySuccess => "confirmed_by_success",
        }
    }

    pub fn parse_str(value: &str) -> Option<Self> {
        match value {
            "echoed" => Some(Self::Echoed),
            "confirmed_by_success" => Some(Self::ConfirmedBySuccess),
            _ => None,
        }
    }

    /// Whether this grade may be used to score a model's capability or to learn
    /// an agent's default model. Only a statement by the CLI qualifies.
    pub fn is_conclusive(self) -> bool {
        matches!(self, Self::Echoed)
    }
}

/// Names that select a model rather than being one. Confirming these by success
/// would put a router back in the model column — exactly the defect the
/// requested/observed split was introduced to remove, where `auto` was stored
/// as the model for codex and cursor tasks.
///
/// `cursor.model = "auto"` is a real, valid entry in `~/.aid/agent_config.toml`,
/// so this is not a hypothetical: a successful cursor run would otherwise
/// confirm `auto` as the model that did the work.
pub fn is_router_alias(model: &str) -> bool {
    matches!(
        model.trim().to_ascii_lowercase().as_str(),
        "auto" | "default" | "router"
    )
}

/// Decide what model to record and how strongly it is believed, from what the
/// CLI said and what aid dispatched.
///
/// This is one function rather than three because the value it replaces —
/// `info.model.as_deref().or(model)` — was copied into `pty_runner`,
/// `run_process` and `run_agent::timeout`, and all three drifted into recording
/// a request as an observation.
pub fn grade_observation(
    echoed: Option<&str>,
    requested: Option<&str>,
    succeeded: bool,
) -> (Option<String>, Option<AttributionSource>) {
    if let Some(model) = echoed {
        return (Some(model.to_string()), Some(AttributionSource::Echoed));
    }
    // A failed run proves nothing: the failure may be the CLI refusing the very
    // model aid asked for, which is how `t-bd455a68` came to claim the `claude`
    // CLI ran `gemini-3.6-flash-low`.
    if !succeeded {
        return (None, None);
    }
    match requested {
        Some(model) if !is_router_alias(model) => (
            Some(model.to_string()),
            Some(AttributionSource::ConfirmedBySuccess),
        ),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_echo_is_conclusive() {
        assert!(AttributionSource::Echoed.is_conclusive());
        assert!(!AttributionSource::ConfirmedBySuccess.is_conclusive());
    }

    #[test]
    fn grades_round_trip_through_storage() {
        for grade in [AttributionSource::Echoed, AttributionSource::ConfirmedBySuccess] {
            assert_eq!(AttributionSource::parse_str(grade.as_str()), Some(grade));
        }
        assert_eq!(AttributionSource::parse_str("guessed"), None);
    }

    #[test]
    fn router_names_are_not_models() {
        for name in ["auto", "AUTO", " auto ", "default", "router"] {
            assert!(is_router_alias(name), "{name} selects a model, it is not one");
        }
        for name in ["gpt-5.6", "claude-opus-5", "composer-2", "qwen3.8-max"] {
            assert!(!is_router_alias(name));
        }
    }
}

#[cfg(test)]
mod grade_observation_tests {
    use super::*;

    #[test]
    fn an_echo_outranks_the_request_and_is_conclusive() {
        let (model, source) = grade_observation(Some("composer-2"), Some("auto"), true);
        assert_eq!(model.as_deref(), Some("composer-2"));
        assert_eq!(source, Some(AttributionSource::Echoed));
    }

    /// codex and agy never name a model. Without this rule their every task is
    /// permanently unknown, which is what starves model-level history.
    #[test]
    fn a_silent_cli_that_succeeded_confirms_what_was_asked_for() {
        let (model, source) = grade_observation(None, Some("gpt-5.6-luna"), true);
        assert_eq!(model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(source, Some(AttributionSource::ConfirmedBySuccess));
    }

    /// The failure may be the CLI refusing that exact model — `t-bd455a68` is
    /// the `claude` CLI failing on `gemini-3.6-flash-low`. Confirming on a
    /// failed run would reinstate the defect this replaced.
    #[test]
    fn a_failed_run_confirms_nothing() {
        assert_eq!(
            grade_observation(None, Some("gemini-3.6-flash-low"), false),
            (None, None)
        );
    }

    /// `cursor.model = "auto"` is a real entry in agent_config.toml. Confirming
    /// it would put a router back in the model column.
    #[test]
    fn a_router_alias_is_never_confirmed() {
        assert_eq!(grade_observation(None, Some("auto"), true), (None, None));
    }

    #[test]
    fn nothing_asked_and_nothing_said_stays_unknown() {
        assert_eq!(grade_observation(None, None, true), (None, None));
    }
}
