// Named Claude API accounts and validation for [[api_route]] configuration.
// Secrets remain in the keychain; this module contains item identifiers only.
use crate::routing::keychain::KeychainItem;
use crate::types::AgentKind;
use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, de::Error};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApiRoute {
    pub id: String,
    pub agent: String,
    pub keychain_service: String,
    pub keychain_account: Option<String>,
    pub monthly_budget_usd: f64,
    #[serde(default)]
    pub over_budget: OverBudget,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OverBudget {
    #[default]
    Stop,
    Bill,
}

impl ApiRoute {
    pub(crate) fn item(&self) -> KeychainItem {
        KeychainItem {
            service: self.keychain_service.clone(),
            account: self.keychain_account.clone().unwrap_or_else(|| self.id.clone()),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.agent != "claude" {
            bail!("API route {}: only agent = \"claude\" is supported", self.id);
        }
        if self.id.is_empty()
            || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || AgentKind::parse_str(&self.id).is_some()
        {
            bail!("API route id must be a unique non-builtin name using letters, digits and hyphens");
        }
        if !self.monthly_budget_usd.is_finite() || self.monthly_budget_usd <= 0.0 {
            bail!(
                "API route {}: monthly_budget_usd must be positive and finite",
                self.id
            );
        }
        if self.keychain_service.trim().is_empty() || self.item().account.trim().is_empty() {
            bail!(
                "API route {}: keychain service and account must not be empty",
                self.id
            );
        }
        Ok(())
    }
}

pub(crate) fn deserialize_routes<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<ApiRoute>, D::Error> {
    let routes = Vec::<ApiRoute>::deserialize(d)?;
    let mut ids = std::collections::HashSet::new();
    for route in &routes {
        route.validate().map_err(D::Error::custom)?;
        if !ids.insert(&route.id) {
            return Err(D::Error::custom(format!("duplicate API route {}", route.id)));
        }
    }
    Ok(routes)
}

pub(crate) fn find(id: &str) -> Result<Option<ApiRoute>> {
    Ok(super::load_config()?
        .api_route
        .into_iter()
        .find(|route| route.id == id))
}

#[cfg(test)]
#[path = "api_route_tests.rs"]
mod tests;
