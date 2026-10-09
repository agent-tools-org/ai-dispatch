// Hermetic leaderboard setup from captured relay output only; tests only.
// Exports: sample, seed, seed_feed, live, live_prices, seed_live.
// Deps: typed score feed and the existing price-feed test seam. No hand-written ids or scores.

use super::feed::Feed;

/// The relay sample captured 2026-10-08 (pre id-namespace unification).
pub(crate) fn sample() -> Feed {
    serde_json::from_str(include_str!("../../tests/fixtures/scores-sample.json")).expect("sample")
}

pub(crate) fn seed() -> Feed {
    let feed = sample();
    seed_feed(&feed);
    feed
}

/// Writes `feed` as the scores cache and a price feed holding exactly its ids.
pub(crate) fn seed_feed(feed: &Feed) {
    write_scores(feed);
    let prices = crate::cost::price_feed::Feed {
        built_at: feed.built_at.clone(),
        age_seconds: Some(0),
        stale: Some(false),
        count: None,
        models: feed
            .models
            .iter()
            .map(|model| crate::cost::price_feed::FeedModel {
                id: model.id.clone(),
                aliases: vec![],
                input_per_mtok: 1.0,
                output_per_mtok: 1.0,
                cached_input_per_mtok: None,
                context_length: None,
                source: None,
            })
            .collect(),
    };
    crate::cost::set_feed_for_tests(prices);
}

/// Rows of the live /v1/scores.json captured 2026-10-09 02:34Z, trimmed to nine
/// models; ids and values are unchanged from the capture.
pub(crate) fn live() -> Feed {
    serde_json::from_str(include_str!("../../tests/fixtures/leaderboard/scores-trimmed-20261009.json"))
        .expect("live scores")
}

/// The matching rows of the live /v1/prices.json (captured 04:50Z, after bare
/// maker aliases were added), with their real aliases.
pub(crate) fn live_prices() -> crate::cost::price_feed::Feed {
    serde_json::from_str(include_str!("../../tests/fixtures/leaderboard/prices-trimmed-20261009.json"))
        .expect("live prices")
}

/// Seeds both caches from the live captures, so model names resolve through the
/// real price-feed aliases (e.g. `opus` -> anthropic/claude-opus-5.5).
pub(crate) fn seed_live() -> Feed {
    let feed = live();
    write_scores(&feed);
    crate::cost::set_feed_for_tests(live_prices());
    feed
}

fn write_scores(feed: &Feed) {
    crate::paths::ensure_dirs().expect("dirs");
    std::fs::write(
        crate::paths::aid_dir().join("scores.json"),
        serde_json::to_vec(feed).expect("scores JSON"),
    )
    .expect("scores");
    crate::cost::clear_feed_for_tests();
}
