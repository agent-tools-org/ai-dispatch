use super::*;

#[test]
fn rejects_trivial_length() {
    assert!(!is_surprising("too short", "discovery"));
}

#[test]
fn rejects_common_boilerplate() {
    assert!(!is_surprising(
        "The code uses the anyhow crate for error handling",
        "convention"
    ));
    assert!(!is_surprising(
        "this project uses tailwind for styling",
        "convention"
    ));
}

#[test]
fn rejects_discovery_signatures() {
    assert!(!is_surprising("pub fn build_prompt_bundle()", "discovery"));
    assert!(!is_surprising("struct PromptBundle", "discovery"));
    assert!(!is_surprising("crate::types::Memory", "discovery"));
}

#[test]
fn accepts_real_discoveries() {
    assert!(is_surprising(
        "The auth module uses bcrypt, but it should be using argon2 to avoid timing attacks",
        "discovery"
    ));
    assert!(is_surprising(
        "Found a bug in the retry logic where it doesn't wait long enough between attempts",
        "lesson"
    ));
    assert!(is_surprising(
        "The external API endpoint has a rate limit of 100 requests per minute",
        "fact"
    ));
}

#[test]
fn accepts_non_obvious_behavior() {
    assert!(is_surprising(
        "Non-obvious behavior in the cache where it invalidates on any write to the database",
        "discovery"
    ));
}

#[test]
fn accepts_performance_notes() {
    assert!(is_surprising(
        "Performance bottleneck in the JSON parser when handling large arrays",
        "discovery"
    ));
}

#[test]
fn parse_memory_tier_defaults_to_on_demand() {
    assert_eq!(parse_memory_tier(None).unwrap(), MemoryTier::OnDemand);
}

#[test]
fn parse_memory_tier_accepts_explicit_values() {
    assert_eq!(
        parse_memory_tier(Some("critical")).unwrap(),
        MemoryTier::Critical
    );
}
