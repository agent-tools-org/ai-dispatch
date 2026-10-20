// Execution route identity and metering regression tests.
// Covers provider pools and named API account isolation.
use super::*;

#[test]
fn a_bare_agent_name_resolves_to_a_triple() {
    let route = Route::for_cli(AgentKind::Codex);
    assert_eq!(route.cli, AgentKind::Codex);
    assert_eq!(route.provider.as_str(), "openai-chatgpt-plan");
    assert_eq!(route.id(), "codex/openai-chatgpt-plan/-");
}

#[test]
fn an_unpinned_model_reads_as_unpinned_not_as_a_default() {
    assert!(Route::for_cli(AgentKind::Codex).model.is_none());
    let pinned = Route::for_cli(AgentKind::Codex).with_model(Some("gpt-6-sol"));
    assert_eq!(pinned.id(), "codex/openai-chatgpt-plan/gpt-6-sol");
}

/// The concrete loss this refactor was started for: agy's gemini allowance was
/// exhausted while its claude allowance still served, aid marked the whole CLI,
/// and `claude-opus-4-6-thinking` sat available behind the CLI it abandoned.
#[test]
fn agy_families_do_not_share_a_pool() {
    let gemini = Route::for_cli(AgentKind::Antigravity).with_model(Some("gemini-3.6-flash-low"));
    let claude = Route::for_cli(AgentKind::Antigravity).with_model(Some("claude-opus-4-6-thinking"));
    assert!(!gemini.shares_pool_with(&claude));

    let other_gemini = Route::for_cli(AgentKind::Antigravity).with_model(Some("gemini-3.1-pro-high"));
    assert!(gemini.shares_pool_with(&other_gemini));
}

/// Codex meters an account pool, so
/// two different models there do share, and marking one must mark both.
#[test]
fn codex_models_share_one_account_pool() {
    let a = Route::for_cli(AgentKind::Codex).with_model(Some("gpt-6-sol"));
    let b = Route::for_cli(AgentKind::Codex).with_model(Some("gpt-6-luna"));
    assert!(a.shares_pool_with(&b));
}

/// Different CLIs never share a pool. An exhausted codex says nothing about
/// cursor — the assumption that sent work hunting for another CLI when the
/// useful question was "what else reaches a model of this class".
#[test]
fn different_clis_do_not_share_a_pool() {
    let codex = Route::for_cli(AgentKind::Codex);
    let cursor = Route::for_cli(AgentKind::Cursor);
    assert!(!codex.shares_pool_with(&cursor));
}

/// Two routes with an unestablished provider must not be assumed to share
/// anything. Equal-because-both-unknown is the trap: it would let one refusal
/// take out every unmapped CLI at once.
#[test]
fn unknown_providers_never_share() {
    let a = Route::for_cli(AgentKind::Codex);
    let b = Route::for_cli(AgentKind::Custom);
    assert!(a.provider.is_unknown() && b.provider.is_unknown());
    assert!(!a.shares_pool_with(&b));
    assert!(!a.shares_pool_with(&a.clone()));
}

/// The same CLI pointed at a different provider is a different route with
/// different billing. opencode reaching a BYOK endpoint must not inherit Zen's
/// spend budget.
#[test]
fn redirecting_the_provider_drops_the_default_metering() {
    let zen = Route::for_cli(AgentKind::OpenCode);
    assert_eq!(zen.metering(), MeteringShape::SpendBudget);

    let byok = Route::for_cli(AgentKind::OpenCode).via(ProviderId::new("byok-deepseek"));
    assert_eq!(byok.metering(), MeteringShape::Unknown);
    assert!(!zen.shares_pool_with(&byok));
}

/// A subscription is metered per account, so its routes share — cursor being
/// rate-limited applies to every model it serves.
#[test]
fn a_subscription_shares_across_models() {
    let a = Route::for_cli(AgentKind::Cursor).with_model(Some("composer-2"));
    let b = Route::for_cli(AgentKind::Cursor).with_model(Some("claude-opus-5-thinking-high"));
    assert!(a.shares_pool_with(&b));
}

#[test]
fn api_account_routes_have_independent_spend_pools() {
    let ming = Route::for_cli(AgentKind::Claude).via(ProviderId::new("anthropic-api-claude-api-ming"));
    let meet = Route::for_cli(AgentKind::Claude).via(ProviderId::new("anthropic-api-claude-api-meet"));
    assert_eq!(ming.metering(), MeteringShape::SpendBudget);
    assert!(ming.shares_pool_with(&ming));
    assert!(!ming.shares_pool_with(&meet));
    assert!(!ming.shares_pool_with(&Route::for_cli(AgentKind::Claude)));
}
