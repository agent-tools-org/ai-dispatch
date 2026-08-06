// Egress tier classification from provider identity and endpoint URLs.
// Exports: egress_for_base_url, egress_for_provider, egress_for_cli.
// Deps: super::EgressTier, super::AgentKind, super::ProviderId.

use super::{AgentKind, EgressTier, ProviderId};

/// Egress for a named provider without a separate endpoint observation.
///
/// A known id is still third-party: every provider aid has established so far
/// reaches a remote endpoint. Local is established only by a loopback
/// `base_url` (see [`egress_for_base_url`]), never by CLI identity.
pub fn egress_for_provider(provider: &ProviderId) -> EgressTier {
    if provider.is_unknown() {
        EgressTier::Unknown
    } else {
        EgressTier::ThirdParty
    }
}

/// Egress for a CLI's default provider. Every current built-in is third-party
/// or unknown; none qualify for `--egress local`.
pub fn egress_for_cli(cli: AgentKind) -> EgressTier {
    let (provider, _) = super::provider_for_cli(cli);
    egress_for_provider(&provider)
}

/// Establish egress from an OpenAI-compatible `base_url`. Loopback hosts are
/// Local; RFC1918/link-local hosts are PrivateNetwork; public hosts are
/// ThirdParty; an unparseable empty value is Unknown.
pub fn egress_for_base_url(base_url: &str) -> EgressTier {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return EgressTier::Unknown;
    }
    host_from_base_url(trimmed)
        .map(egress_for_host)
        .unwrap_or(EgressTier::Unknown)
}

fn egress_for_host(host: &str) -> EgressTier {
    let host = host.trim_matches(|c| c == '[' || c == ']');
    let lower = host.to_ascii_lowercase();
    if lower == "localhost" {
        return EgressTier::Local;
    }
    if let Ok(ip) = lower.parse::<std::net::IpAddr>() {
        return egress_for_ip(ip);
    }
    if has_private_dns_suffix(&lower) {
        return EgressTier::PrivateNetwork;
    }
    EgressTier::ThirdParty
}

fn egress_for_ip(ip: std::net::IpAddr) -> EgressTier {
    match ip {
        std::net::IpAddr::V4(v4) => {
            if v4.is_loopback() {
                return EgressTier::Local;
            }
            if is_private_ipv4(v4) {
                return EgressTier::PrivateNetwork;
            }
            EgressTier::ThirdParty
        }
        std::net::IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return EgressTier::Local;
            }
            if v6.is_unicast_link_local() || is_ipv6_ula(v6) {
                return EgressTier::PrivateNetwork;
            }
            EgressTier::ThirdParty
        }
    }
}

fn is_private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 169 && o[1] == 254)
}

/// IPv6 unique local address (fc00::/7).
fn is_ipv6_ula(ip: std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn has_private_dns_suffix(host: &str) -> bool {
    host.ends_with(".local") || host.ends_with(".home.arpa")
}

fn host_from_base_url(base_url: &str) -> Option<&str> {
    let rest = base_url
        .split_once("://")
        .map(|(_, after)| after)
        .unwrap_or(base_url);
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    // Strip userinfo and port; IPv6 stays bracketed until egress_for_host.
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    if hostport.starts_with('[') {
        return hostport.split(']').next().map(|h| h.trim_start_matches('['));
    }
    Some(hostport.split(':').next().unwrap_or(hostport)).filter(|h| !h.is_empty())
}
