// API-route fallback estimates from exact price-feed identities.
// Claude numeric version separators are normalized; aliases never guess a model version.
use super::{feed_index, price_feed};

pub(crate) fn estimate(tokens: Option<i64>, model: Option<&str>) -> Option<f64> {
    let tokens = tokens.filter(|count| *count >= 0)?;
    let model = model?;
    let (feed, index) = feed_index()?;
    let normalized = normalize_model(model);
    let canonical = format!("anthropic/{normalized}");
    let price = price_feed::feed_lookup(&feed, &index, &canonical)
        .or_else(|| price_feed::feed_lookup(&feed, &index, &normalized))?;
    Some(tokens as f64 * (price.input_per_mtok * 0.7 + price.output_per_mtok * 0.3) / 1_000_000.0)
}

fn normalize_model(model: &str) -> String {
    let parts: Vec<_> = model.split('-').collect();
    if parts.len() == 4
        && parts[0] == "claude"
        && matches!(parts[1], "opus" | "sonnet" | "haiku")
        && parts[2..]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        format!("claude-{}-{}.{}", parts[1], parts[2], parts[3])
    } else {
        model.to_owned()
    }
}

#[cfg(test)]
#[path = "api_route_tests.rs"]
mod tests;
