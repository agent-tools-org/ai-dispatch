// An execution route: which CLI, billed by which provider, running which model.
// Exports: Route.
// Deps: crate::types::{AgentKind, MeteringShape, ProviderId}, serde.

use serde::Serialize;

use super::{provider_for_cli, AgentKind, MeteringShape, ProviderId};

/// The three independent things aid used to collapse into one opaque agent id:
///
/// ```text
/// opencode / byok / deepseek-v4-flash
/// └ CLI      └ provider  └ model
/// ```
///
/// | Dimension | Owns |
/// |---|---|
/// | CLI | invocation: flags, output shape, session resume, sandboxing |
/// | provider | metering and billing: the quota pool and its reset semantics |
/// | model | capability per category, context window, per-token price |
///
/// This is additive. `AgentKind` is not deleted or renamed — it always was the
/// CLI dimension, carrying two extra jobs. It appears in 203 files and 1389
/// places, and a replacing rewrite at that coupling could not be reviewed
/// honestly, so the two extra jobs are taken away from it instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Route {
    pub cli: AgentKind,
    pub provider: ProviderId,
    /// `None` means the route does not pin a model and the CLI picks its own.
    /// It must never be filled in with a default to make the type look
    /// complete — unknown is the honest value and the one the attribution work
    /// exists to preserve.
    pub model: Option<String>,
}

impl Route {
    /// The route a bare agent name resolves to. `aid run codex` keeps working
    /// and now means a triple rather than an opaque id.
    pub fn for_cli(cli: AgentKind) -> Self {
        let (provider, _) = provider_for_cli(cli);
        Self { cli, provider, model: None }
    }

    pub fn with_model(mut self, model: Option<impl Into<String>>) -> Self {
        self.model = model.map(Into::into);
        self
    }

    /// Point the route at a provider other than the CLI's default. `opencode`
    /// reaches both Zen and any BYOK endpoint, and those bill differently
    /// enough that averaging them was one of the original defects.
    pub fn via(mut self, provider: ProviderId) -> Self {
        self.provider = provider;
        self
    }

    pub fn metering(&self) -> MeteringShape {
        let (default_provider, shape) = provider_for_cli(self.cli);
        // A route pointed somewhere other than its CLI's default provider
        // cannot borrow that provider's metering shape: BYOK through opencode
        // has no pool at all, while opencode Zen has a spend budget.
        if default_provider == self.provider {
            shape
        } else {
            MeteringShape::Unknown
        }
    }

    /// `<cli>/<provider>/<model>`, with `-` where a model is not pinned. This is
    /// the identity, and it is deliberately readable: the failure it replaces
    /// was a name like `glm5` that said nothing about which of its three
    /// dimensions had broken.
    pub fn id(&self) -> String {
        format!(
            "{}/{}/{}",
            self.cli.as_str(),
            self.provider.as_str(),
            self.model.as_deref().unwrap_or("-")
        )
    }

    /// Whether two routes draw on the same quota pool. The question that
    /// matters when one of them has just been refused, and the one
    /// `rate-limit-<agent>` could not answer: an exhausted codex says nothing
    /// about cursor, but it says everything about another codex route.
    pub fn shares_pool_with(&self, other: &Route) -> bool {
        if self.provider.is_unknown() || other.provider.is_unknown() {
            return false;
        }
        if self.provider != other.provider {
            return false;
        }
        match self.metering() {
            // Families are metered apart, so two routes share a pool only when
            // their models belong to the same one.
            MeteringShape::PerModelFamily => model_family(self.model.as_deref())
                .zip(model_family(other.model.as_deref()))
                .is_some_and(|(a, b)| a == b),
            // No pool means nothing to share, however equal the providers look.
            MeteringShape::None | MeteringShape::Unknown => false,
            _ => true,
        }
    }
}

/// `None` when the model is not pinned, so two unpinned routes on a
/// per-family provider are not assumed to share a pool.
fn model_family(model: Option<&str>) -> Option<&'static str> {
    model.map(super::provider::model_family)
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
