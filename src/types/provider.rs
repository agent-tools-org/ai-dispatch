// The billing and metering entity behind a route: who meters quota and bills,
// and whether task data stays on-machine or leaves for a third party.
// Exports: ProviderId, MeteringShape, EgressTier, provider_for_cli, egress_*.
// Deps: crate::types::AgentKind, serde.

use serde::Serialize;

use super::AgentKind;

/// Who meters and bills the work — the dimension aid collapsed into the CLI
/// name and then could not reason about.
///
/// It is a string rather than an enum because BYOK routes point at arbitrary
/// endpoints: `examples/byok/mimo.toml` carries a `base_url` and a `key_env`,
/// and `glm5` is really `bash-wrapper / NVIDIA NIM / z-ai/glm5`. A closed enum
/// would have to call all of those the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The metering entity has not been established. A first-class value, not a
    /// placeholder to be filled in with a guess: naming a provider aid has not
    /// observed would put invented billing structure into the routing table,
    /// which is the failure this whole track exists to remove.
    pub fn unknown() -> Self {
        Self("unknown".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_unknown(&self) -> bool {
        self.0 == "unknown"
    }
}

/// Where task data goes when a provider runs. Independent of the CLI that
/// invokes it: codex is not "local" because its binary runs on the laptop; its
/// provider is openai-chatgpt-plan, and the prompts leave the machine.
///
/// This is the dimension `--egress` reads. It lived as a per-CLI constant on
/// `AgentKind::profile()` and mislabelled codex/oz as local while refusing
/// claude for being "api" — same third-party class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EgressTier {
    /// Endpoint is loopback only (`localhost` / `127.0.0.1` / `::1`). The only
    /// tier that satisfies `--egress local`.
    Local,
    /// Data leaves the machine for a third party.
    ThirdParty,
    /// Not established. Must not be admitted by `--egress local`.
    Unknown,
}

impl EgressTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::ThirdParty => "third-party",
            Self::Unknown => "unknown",
        }
    }

    pub fn admits_local(self) -> bool {
        matches!(self, Self::Local)
    }
}

/// How a provider meters what it sells. Quota reasoning needs this because the
/// shapes are not interchangeable — the whole reason `rate-limit-<agent>` could
/// not express reality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MeteringShape {
    /// One pool for the whole account, shared by every model it serves.
    /// qwen's ModelStudio token plan is one 5-hour pool across all 17 models.
    AccountPool,
    /// Separate pools per model family. Observed twice in one day on agy, in
    /// both directions: gemini exhausted while claude served, then claude
    /// exhausted while gemini served.
    PerModelFamily,
    /// A spend budget in currency rather than a time-windowed allowance. It
    /// does not refill on its own — opencode Zen's refusal is an HTTP 401
    /// "Insufficient balance", and only a top-up clears it.
    SpendBudget,
    /// A subscription that is not metered per task, though model tiers cost the
    /// plan differently.
    Subscription,
    /// No pool at all: billed per token against the user's own key.
    None,
    /// Not established. Never guess a shape — the wrong one produces a
    /// confident, wrong availability answer.
    Unknown,
}

/// The provider each built-in CLI bills against by default, with the evidence
/// that established it. A CLI can reach more than one provider — `opencode`
/// serves both Zen and BYOK — so this is the default for a bare agent name, not
/// a claim that the mapping is one-to-one.
///
/// Every entry below was observed directly. Where nothing was observed the
/// answer is `unknown`, deliberately: a plausible-looking provider name is
/// exactly the kind of value this refactor was started to stop recording.
pub fn provider_for_cli(cli: AgentKind) -> (ProviderId, MeteringShape) {
    let (id, shape) = match cli {
        // "You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage
        //  ... try again at Aug 11th, 2026 2:23 PM." — run directly, 2026-08-05.
        AgentKind::Codex => ("openai-chatgpt-plan", MeteringShape::AccountPool),
        // settings.json baseUrl token-plan.ap-southeast-1.maas.aliyuncs.com, and
        // "Your token-plan 5-hour quota has been exhausted."
        AgentKind::Qwen => ("alibaba-modelstudio-token-plan", MeteringShape::AccountPool),
        // "Individual quota reached ... Resets in 59m21s." on one family while
        // another kept serving — captured in both directions on 2026-08-05/06.
        AgentKind::Antigravity => ("google-antigravity-individual", MeteringShape::PerModelFamily),
        // HTTP 401 "Insufficient balance. Manage your billing here:
        //  https://opencode.ai/workspace/.../billing" — t-76181278.
        AgentKind::OpenCode => ("opencode-zen", MeteringShape::SpendBudget),
        // "Error: Quota limit reached. ... check Warp logs at
        //  ~/Library/Logs/oz/warp.log" — t-2d3827e5.
        AgentKind::Oz => ("warp", MeteringShape::AccountPool),
        // "You've reached your weekly standard usage limit (resets in 1 day)"
        // as an HTTP 402 body.
        AgentKind::Droid => ("factory", MeteringShape::AccountPool),
        // Local bearer in ~/.grok/auth.json; the CLI exposes no billing surface.
        AgentKind::Grok => ("xai", MeteringShape::Unknown),
        // Rate-limited with a reset timestamp but no per-task metering.
        AgentKind::Cursor => ("cursor-subscription", MeteringShape::Subscription),
        AgentKind::Claude => ("anthropic", MeteringShape::Unknown),
        AgentKind::Gemini => ("google-genai", MeteringShape::Unknown),
        AgentKind::Copilot => ("github-copilot", MeteringShape::Subscription),
        // Never observed refusing, so nothing is known about how they meter.
        // Naming a provider here would be invention.
        AgentKind::Kilo | AgentKind::MiMoCode | AgentKind::Codebuff | AgentKind::Custom => {
            ("unknown", MeteringShape::Unknown)
        }
    };
    (ProviderId::new(id), shape)
}

/// Egress for a named provider without a separate endpoint observation.
///
/// A known id is still third-party: every provider aid has established so far
/// reaches a remote endpoint. Local is established only by a loopback
/// `base_url` (see [`egress_for_base_url`]), never by CLI identity.
pub fn egress_for_provider(provider: &ProviderId) -> EgressTier {
    if provider.is_unknown() {
        EgressTier::Unknown
    } else {
        EgressTier::ThirdParty
    }
}

/// Egress for a CLI's default provider. Every current built-in is third-party
/// or unknown; none qualify for `--egress local`.
pub fn egress_for_cli(cli: AgentKind) -> EgressTier {
    let (provider, _) = provider_for_cli(cli);
    egress_for_provider(&provider)
}

/// Establish egress from an OpenAI-compatible `base_url`. Loopback hosts are
/// Local; any other host is ThirdParty; an unparseable empty value is Unknown.
pub fn egress_for_base_url(base_url: &str) -> EgressTier {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return EgressTier::Unknown;
    }
    match loopback_host(trimmed) {
        Some(true) => EgressTier::Local,
        Some(false) => EgressTier::ThirdParty,
        None => EgressTier::Unknown,
    }
}

/// `Some(true)` when the URL/host is loopback, `Some(false)` when a non-loopback
/// host is visible, `None` when no host can be read.
fn loopback_host(base_url: &str) -> Option<bool> {
    let host = host_from_base_url(base_url)?;
    let host = host.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
    Some(host == "localhost" || host == "127.0.0.1" || host == "::1")
}

fn host_from_base_url(base_url: &str) -> Option<&str> {
    let rest = base_url
        .split_once("://")
        .map(|(_, after)| after)
        .unwrap_or(base_url);
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    // Strip userinfo and port; IPv6 stays bracketed until loopback_host.
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    if hostport.starts_with('[') {
        return hostport.split(']').next().map(|h| h.trim_start_matches('['));
    }
    Some(hostport.split(':').next().unwrap_or(hostport)).filter(|h| !h.is_empty())
}

/// The family a model is metered under, by vendor prefix — only meaningful for
/// a provider whose shape is `PerModelFamily`.
///
/// Lives here rather than beside the agy quota code because it is a fact about
/// how a provider partitions its allowance, not about any one CLI. It had been
/// written twice with two different answers for `gpt-*`, which is precisely the
/// kind of quiet divergence that decides whether a route is marked dead.
///
/// Names captured from `agy models`, 2026-08-05.
pub fn model_family(model: &str) -> &'static str {
    let model = model.to_ascii_lowercase();
    if model.starts_with("gemini") {
        "gemini"
    } else if model.starts_with("claude") {
        "claude"
    } else if model.starts_with("gpt") {
        "gpt-oss"
    } else {
        "other"
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
