use super::*;

/// The two shapes that broke routing on 2026-08-05 must stay distinguishable.
/// agy metering per family is why marking the whole CLI stranded a working
/// claude allowance; qwen's single pool is why marking one of its 17 models
/// would have been wrong in the opposite direction.
#[test]
fn the_shapes_that_broke_routing_are_distinct() {
    let (agy, agy_shape) = provider_for_cli(AgentKind::Antigravity);
    let (qwen, qwen_shape) = provider_for_cli(AgentKind::Qwen);
    assert_eq!(agy_shape, MeteringShape::PerModelFamily);
    assert_eq!(qwen_shape, MeteringShape::AccountPool);
    assert_ne!(agy, qwen);
}

/// A spend budget is not a time window. opencode Zen refuses with
/// "Insufficient balance" and only a top-up clears it, so treating it as an
/// account pool would have aid waiting for a reset that never comes.
#[test]
fn a_spend_budget_is_not_a_time_windowed_pool() {
    let (_, shape) = provider_for_cli(AgentKind::OpenCode);
    assert_eq!(shape, MeteringShape::SpendBudget);
}

/// Providers nobody has observed refusing stay unknown. The point of the
/// dimension is to stop recording plausible values that were never established.
#[test]
fn unobserved_providers_are_unknown_not_invented() {
    for cli in [AgentKind::Kilo, AgentKind::MiMoCode, AgentKind::Custom] {
        let (provider, shape) = provider_for_cli(cli);
        assert!(provider.is_unknown(), "{} must not be given an invented provider", cli.as_str());
        assert_eq!(shape, MeteringShape::Unknown);
    }
}

/// A provider whose id is known while its metering is not is a legitimate
/// state: grok's bearer token proves the vendor, and the CLI exposes no billing
/// surface at all. Forcing a shape here would be a guess.
#[test]
fn a_known_vendor_may_still_have_unknown_metering() {
    let (provider, shape) = provider_for_cli(AgentKind::Grok);
    assert_eq!(provider.as_str(), "xai");
    assert!(!provider.is_unknown());
    assert_eq!(shape, MeteringShape::Unknown);
}

#[test]
fn commandcode_is_its_own_provider() {
    let (provider, shape) = provider_for_cli(AgentKind::CommandCode);
    assert_eq!(provider.as_str(), "commandcode.ai");
    assert!(!provider.is_unknown());
    assert_eq!(shape, MeteringShape::Unknown);
}

/// Two CLIs from the same vendor must not collapse into one provider: agy and
/// gemini both reach Google models, but an exhausted Antigravity tier says
/// nothing about a Gemini API key.
#[test]
fn one_vendor_two_cli_routes_are_two_providers() {
    let (agy, _) = provider_for_cli(AgentKind::Antigravity);
    let (gemini, _) = provider_for_cli(AgentKind::Gemini);
    assert_ne!(agy, gemini);
}

/// Every built-in CLI is third-party or unknown. None of them are local: the
/// CLI binary running on the laptop does not make the provider local.
#[test]
fn no_builtin_cli_is_local_egress() {
    for cli in AgentKind::ALL_BUILTIN {
        let tier = egress_for_cli(*cli);
        assert!(
            !tier.admits_local(),
            "{} must not admit --egress local (got {})",
            cli.as_str(),
            tier.label()
        );
    }
}

/// Local is established only by a loopback base_url, never by a provider name.
#[test]
fn only_loopback_base_url_is_local() {
    assert_eq!(egress_for_base_url("http://127.0.0.1:11434/v1"), EgressTier::Local);
    assert_eq!(egress_for_base_url("http://localhost:8080/v1"), EgressTier::Local);
    assert_eq!(egress_for_base_url("http://[::1]:8080/v1"), EgressTier::Local);
    assert_eq!(
        egress_for_base_url("https://token-plan-sgp.xiaomimimo.com/v1"),
        EgressTier::ThirdParty
    );
    assert_eq!(egress_for_base_url(""), EgressTier::Unknown);
}

/// An unknown provider is not admitted by --egress local even when a custom
/// agent config claims trust_tier = "local" by hand.
#[test]
fn unknown_provider_does_not_admit_local() {
    assert!(!egress_for_provider(&ProviderId::unknown()).admits_local());
    assert!(!egress_for_provider(&ProviderId::new("openai-chatgpt-plan")).admits_local());
}
