// Real Claude version spellings resolve to feed prices, never legacy family prices.
// Uses an isolated synthetic feed with the measured canonical model identity.
use super::*;

#[test]
fn opus_55_numeric_id_and_alias_use_the_feed_not_static_opus() {
    let _home = crate::support::test_env::TestHome::new();
    super::super::clear_feed_for_tests();
    super::super::set_feed_for_tests(price_feed::Feed {
        built_at: chrono::Utc::now().to_rfc3339(),
        age_seconds: Some(0),
        stale: Some(false),
        count: Some(1),
        models: vec![price_feed::FeedModel {
            id: "anthropic/claude-opus-5.5".into(),
            aliases: vec!["opus".into()],
            input_per_mtok: 4.0,
            output_per_mtok: 20.0,
            cached_input_per_mtok: Some(0.2),
            context_length: None,
            source: None,
        }],
    });
    for model in [
        "claude-opus-5-5",
        "claude-opus-5.5",
        "anthropic/claude-opus-5.5",
        "opus",
    ] {
        assert_eq!(estimate(Some(1_000_000), Some(model)), Some(8.8));
    }
    assert_eq!(estimate(Some(1_000_000), Some("claude-opus-99-9")), None);
    assert_eq!(estimate(None, Some("opus")), None);
    assert_eq!(estimate(Some(1_000_000), None), None);
}
