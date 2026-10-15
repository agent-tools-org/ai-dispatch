// Named Claude API accounts and validation for [[api_route]] configuration.
// Secrets remain in the keychain; this module contains item identifiers only.
use crate::types::AgentKind;
use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiRoute {
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
pub enum OverBudget {
    #[default]
    Stop,
    Bill,
}

impl ApiRoute {
    pub(crate) fn account(&self) -> &str {
        self.keychain_account.as_deref().unwrap_or(&self.id)
    }

    pub(crate) fn helper_command(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "/usr/bin/security find-generic-password -s {} -a {} -w",
            self.keychain_service,
            self.account(),
        ))
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
        for (field, value) in [
            ("keychain_service", self.keychain_service.as_str()),
            ("keychain_account", self.account()),
        ] {
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            {
                bail!("API route {}: {field} must match [A-Za-z0-9._-]+", self.id);
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_routes(routes: &[ApiRoute]) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    for route in routes {
        route.validate()?;
        if !ids.insert(&route.id) {
            bail!("duplicate API route {}", route.id);
        }
    }
    Ok(())
}

pub(crate) fn find(id: &str) -> Result<Option<ApiRoute>> {
    Ok(super::load_config()?
        .api_route
        .into_iter()
        .find(|route| route.id == id))
}

pub(crate) fn find_for_dispatch(id: &str) -> Result<Option<ApiRoute>> {
    if AgentKind::parse_str(id).is_some() {
        return Ok(None);
    }
    let path = crate::support::paths::config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let value: toml::Value = toml::from_str(&content)?;
    let Some(routes) = value.get("api_route").and_then(toml::Value::as_array) else {
        return Ok(None);
    };
    let mut matching = routes
        .iter()
        .filter(|route| route.get("id").and_then(toml::Value::as_str) == Some(id));
    let Some(route) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        bail!("duplicate API route {id}");
    }
    let route: ApiRoute = route
        .clone()
        .try_into()
        .with_context(|| format!("Invalid API route {id}"))?;
    route.validate()?;
    Ok(Some(route))
}

#[cfg(test)]
#[path = "api_route_tests.rs"]
mod tests;
