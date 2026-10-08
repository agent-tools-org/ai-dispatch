// Shared hermetic leaderboard setup using the unmodified captured relay sample.
// Exports: sample, seed, add_terminal; tests only.
// Deps: typed score feed and the existing price-feed test seam.

use super::feed::{Feed, Score};

pub(crate) fn sample() -> Feed {
    serde_json::from_str(include_str!("../../tests/fixtures/scores-sample.json")).expect("sample")
}

pub(crate) fn seed() -> Feed {
    let feed = sample();
    seed_feed(&feed);
    feed
}

pub(crate) fn seed_feed(feed: &Feed) {
    crate::paths::ensure_dirs().expect("dirs");
    std::fs::write(
        crate::paths::aid_dir().join("scores.json"),
        serde_json::to_vec(feed).expect("scores JSON"),
    )
    .expect("scores");
    crate::cost::clear_feed_for_tests();
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

pub(crate) fn add_terminal(feed: &mut Feed, cli: &str, effort: Option<&str>) {
    feed.models[2].scores.push(Score {
        source: "terminal_bench".into(),
        board: "terminal-bench-4.0".into(),
        cli: Some(cli.into()),
        effort: effort.map(str::to_string),
        value: 72.0,
        unit: "percent".into(),
        rank: Some(4),
        of: Some(35),
        ci_low: Some(70.0),
        ci_high: Some(74.0),
        date: "2026-10-06".into(),
    });
}

pub(crate) fn seed_catalog_aliases() {
    let feed = seed();
    let mut prices = crate::cost::price_feed::Feed {
        built_at: feed.built_at,
        age_seconds: Some(0),
        stale: Some(false),
        count: None,
        models: vec![],
    };
    for (id, aliases) in [
        (
            "302ai/kimi-k2-thinking",
            vec!["opus", "gpt-5.6-sol", "gpt-5.3-codex"],
        ),
        ("302ai/qwen3-30b-a3b", vec!["sonnet"]),
    ] {
        prices.models.push(crate::cost::price_feed::FeedModel {
            id: id.into(),
            aliases: aliases.into_iter().map(str::to_string).collect(),
            input_per_mtok: 1.0,
            output_per_mtok: 1.0,
            cached_input_per_mtok: None,
            context_length: None,
            source: None,
        });
    }
    crate::cost::set_feed_for_tests(prices);
}
