//! Proxy authentication.
//!
//! A trusted reverse proxy (nginx basic auth, Authelia, Authentik) performs the
//! login and passes the identity in a header. Two gates apply: the direct peer
//! must fall inside `allowed_cidrs`, and the `ip_header` is honoured only when
//! the peer is also inside `trusted_proxy_cidrs`.

use std::net::IpAddr;

use crate::config::ProxyAuthConfig;

/// A parsed CIDR block.
#[derive(Debug, Clone, Copy)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }

        match input.split_once('/') {
            Some((addr, prefix)) => {
                let addr: IpAddr = addr.parse().ok()?;
                let prefix: u8 = prefix.parse().ok()?;
                let max = if addr.is_ipv4() { 32 } else { 128 };
                if prefix > max {
                    return None;
                }
                Some(Self { addr, prefix })
            }
            None => {
                // A bare address is a host route.
                let addr: IpAddr = input.parse().ok()?;
                let prefix = if addr.is_ipv4() { 32 } else { 128 };
                Some(Self { addr, prefix })
            }
        }
    }

    pub fn contains(&self, candidate: IpAddr) -> bool {
        match (self.addr, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let network = u32::from(network);
                let candidate = u32::from(candidate);
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - self.prefix)
                };
                network & mask == candidate & mask
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let network = u128::from(network);
                let candidate = u128::from(candidate);
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - self.prefix)
                };
                network & mask == candidate & mask
            }
            // Map IPv4-mapped IPv6 candidates onto their IPv4 network when the
            // configured block is IPv4 (and vice versa).
            (IpAddr::V4(_), IpAddr::V6(candidate)) => candidate
                .to_ipv4_mapped()
                .is_some_and(|v4| self.contains(IpAddr::V4(v4))),
            (IpAddr::V6(network), IpAddr::V4(candidate)) => {
                network.to_ipv4_mapped().is_some_and(|v4| {
                    Cidr {
                        addr: IpAddr::V4(v4),
                        prefix: self.prefix.saturating_sub(96),
                    }
                    .contains(IpAddr::V4(candidate))
                })
            }
        }
    }
}

/// Parses a list of CIDR strings, ignoring unparseable entries.
pub fn parse_cidrs(inputs: &[String]) -> Vec<Cidr> {
    inputs
        .iter()
        .filter_map(|entry| Cidr::parse(entry))
        .collect()
}

fn in_any(cidrs: &[Cidr], addr: IpAddr) -> bool {
    cidrs.iter().any(|cidr| cidr.contains(addr))
}

/// Resolved proxy identity for one request.
#[derive(Debug, Clone)]
pub struct ProxyIdentity {
    pub subject: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
}

/// Extracts and validates the proxy identity for a request.
///
/// `direct_peer` is the socket address the request arrived on; it is never
/// taken from a header. Returns `None` when proxy auth does not apply or the
/// request is not permitted to authenticate.
pub fn resolve(
    config: &ProxyAuthConfig,
    direct_peer: IpAddr,
    get_header: impl Fn(&str) -> Option<String>,
) -> Option<ProxyIdentity> {
    let allowed = parse_cidrs(&config.allowed_cidrs());
    let trusted = parse_cidrs(&config.trusted_proxy_cidrs());

    // The peer used for the allowlist check: either the direct socket peer or,
    // when the caller is a trusted proxy, the forwarded client address.
    let mut effective_peer = direct_peer;
    if let Some(header_name) = config.ip_header.as_deref() {
        if in_any(&trusted, direct_peer) {
            if let Some(raw) = get_header(header_name)
                && let Some(first) = raw.split(',').next()
                && let Ok(parsed) = first.trim().parse::<IpAddr>()
            {
                effective_peer = parsed;
            }
        } else {
            // Untrusted peer tried to supply a forwarding header: refuse.
            tracing::warn!(
                "proxy auth: rejecting {direct_peer} because it is not in trusted_proxy_cidrs"
            );
            return None;
        }
    }

    if !in_any(&allowed, effective_peer) {
        return None;
    }

    let username = get_header(&config.user_header)?;
    let username = username.trim();
    if username.is_empty() {
        return None;
    }

    let opt = |header: &Option<String>| -> Option<String> {
        header
            .as_deref()
            .and_then(&get_header)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    Some(ProxyIdentity {
        subject: format!("proxy:{username}"),
        name: Some(username.to_string()),
        email: opt(&config.email_header),
        picture: opt(&config.picture_header),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ProxyAuthConfig {
        ProxyAuthConfig {
            enabled: true,
            allowed_cidrs: None,
            trusted_proxy_cidrs: None,
            ip_header: None,
            user_header: "Remote-User".into(),
            email_header: Some("Remote-Email".into()),
            name_header: None,
            picture_header: None,
        }
    }

    fn peer(input: &str) -> IpAddr {
        input.parse().unwrap()
    }

    #[test]
    fn cidr_matching() {
        let cidr = Cidr::parse("10.0.0.0/8").unwrap();
        assert!(cidr.contains(peer("10.1.2.3")));
        assert!(!cidr.contains(peer("11.1.2.3")));

        let host = Cidr::parse("127.0.0.1").unwrap();
        assert!(host.contains(peer("127.0.0.1")));
        assert!(!host.contains(peer("127.0.0.2")));

        let v6 = Cidr::parse("::1/128").unwrap();
        assert!(v6.contains(peer("::1")));
    }

    #[test]
    fn loopback_default_allows_local_proxy() {
        let config = config();
        let identity = resolve(&config, peer("127.0.0.1"), |name| {
            (name == "Remote-User").then(|| "alice".to_string())
        });
        let identity = identity.expect("loopback peer should be allowed");
        assert_eq!(identity.subject, "proxy:alice");
    }

    #[test]
    fn remote_peer_is_rejected_by_default() {
        let config = config();
        assert!(
            resolve(&config, peer("203.0.113.5"), |_| Some("alice".into())).is_none(),
            "non-loopback peers must not authenticate without configuration"
        );
    }

    #[test]
    fn missing_identity_header_is_rejected() {
        let config = config();
        assert!(resolve(&config, peer("127.0.0.1"), |_| None).is_none());
        assert!(resolve(&config, peer("127.0.0.1"), |_| Some("  ".into())).is_none());
    }

    #[test]
    fn ip_header_only_honoured_from_trusted_peers() {
        let mut config = config();
        config.ip_header = Some("X-Forwarded-For".into());

        // Trusted peer forwarding a client outside allowed_cidrs -> refused.
        let identity = resolve(&config, peer("127.0.0.1"), |name| {
            (name == "X-Forwarded-For").then(|| "203.0.113.9".to_string())
        });
        // 203.0.113.9 is not in allowed_cidrs (default loopback).
        assert!(identity.is_none());

        // Untrusted direct peer that tries to set the header is refused.
        assert!(
            resolve(&config, peer("203.0.113.5"), |name| {
                (name == "X-Forwarded-For").then(|| "127.0.0.1".to_string())
            })
            .is_none()
        );
    }
}
