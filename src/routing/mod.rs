// Provider routing, quota evidence, and model selection metadata.
// Groups rate limits, live quota, availability, credentials, and score feeds.

pub(crate) mod rate_limit;
pub(crate) mod capacity;
pub(crate) mod availability;
pub(crate) mod model_catalog;
pub(crate) mod model_health;
pub mod credentials;
pub(crate) mod api_budget;
pub(crate) mod auth_marker;
pub(crate) mod scores;
pub(crate) mod feed_refresh;
pub(crate) mod quota;
