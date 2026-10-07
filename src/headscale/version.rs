//! Headscale version parsing and the capability set derived from it.
//!
//! `/version` only exists from Headscale 0.27.0 onward, so a 404 means the
//! server is older than that. Anything unparseable (`dev`, Go pseudo-versions)
//! is treated as *newest*: capabilities are permissive rather than absent.

use serde::{Deserialize, Serialize};

/// Lowest Headscale release Sailplane supports.
pub const MIN_SUPPORTED: Semver = Semver {
    major: 0,
    minor: 27,
    patch: 0,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Semver {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Semver {
    /// Compares versions, ignoring prerelease metadata. `0.28.0-beta.1` counts
    /// as `0.28.0`.
    pub fn gte(&self, other: &Semver) -> bool {
        (self.major, self.minor, self.patch) >= (other.major, other.minor, other.patch)
    }
}

impl std::fmt::Display for Semver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A parsed (or unparseable) Headscale version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerVersion {
    pub raw: String,
    /// `None` when the version string could not be parsed.
    pub parsed: Option<Semver>,
    /// True when the version is unknown, so Sailplane assumes every capability.
    pub unknown: bool,
}

impl ServerVersion {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        let without_v = trimmed.strip_prefix('v').unwrap_or(trimmed);

        // Go pseudo-versions (`0.0.0-20260703052708-048308511c72`) carry no
        // useful ordering information.
        let is_pseudo = without_v.starts_with("0.0.0-")
            && without_v
                .split('-')
                .nth(1)
                .is_some_and(|seg| seg.len() >= 14 && seg.chars().all(|c| c.is_ascii_digit()));

        if is_pseudo {
            return Self {
                raw: raw.to_string(),
                parsed: None,
                unknown: true,
            };
        }

        match parse_semver(without_v) {
            Some(parsed) => Self {
                raw: raw.to_string(),
                parsed: Some(parsed),
                unknown: false,
            },
            None => Self {
                raw: raw.to_string(),
                parsed: None,
                unknown: true,
            },
        }
    }

    fn at_least(&self, major: u64, minor: u64, patch: u64) -> bool {
        if self.unknown {
            return true;
        }
        match self.parsed {
            Some(v) => v.gte(&Semver {
                major,
                minor,
                patch,
            }),
            None => true,
        }
    }

    /// True when the server is older than [`MIN_SUPPORTED`].
    pub fn below_minimum(&self) -> bool {
        match self.parsed {
            Some(v) if !self.unknown => !v.gte(&MIN_SUPPORTED),
            _ => false,
        }
    }

    /// Returns the canonical form, or `unknown`.
    pub fn canonical(&self) -> String {
        match self.parsed {
            Some(v) => v.to_string(),
            None => "unknown".to_string(),
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            pre_auth_keys_have_stable_ids: self.at_least(0, 28, 0),
            node_tags_are_flat: self.at_least(0, 28, 0),
            node_owner_is_immutable: self.at_least(0, 28, 0),
            register_key_includes_auth_req_prefix: self.at_least(0, 29, 0),
            key_expiry_can_be_disabled: self.at_least(0, 29, 0),
            // Headscale's support for the `grants` syntax is version dependent
            // and newer than the parsing here. Parsing and round-tripping never
            // depend on this flag. It only decides whether the structured
            // Grants editor is offered.
            grants_supported: self.at_least(0, 27, 0),
        }
    }
}

impl Default for ServerVersion {
    fn default() -> Self {
        Self {
            raw: "unknown".into(),
            parsed: None,
            unknown: true,
        }
    }
}

/// Feature availability derived from the server version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Pre-auth keys have stable IDs, so Sailplane can list them unfiltered (0.28+).
    pub pre_auth_keys_have_stable_ids: bool,
    /// Node tags are a flat array rather than forced/valid split (0.28+).
    pub node_tags_are_flat: bool,
    /// `POST /node/{id}/user` no longer reassigns ownership (0.28+).
    pub node_owner_is_immutable: bool,
    /// A registration key is the full `hskey-authreq-…` string (0.29+).
    pub register_key_includes_auth_req_prefix: bool,
    /// Sailplane can toggle key expiry (0.29+).
    pub key_expiry_can_be_disabled: bool,
    /// The server accepts the `grants` access syntax in a policy.
    pub grants_supported: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        ServerVersion::default().capabilities()
    }
}

impl Capabilities {
    /// True when Browser SSH is supported (0.28+, excluding the 0.29.0/0.29.1
    /// regression where `/ts2021` returned 405).
    pub fn browser_ssh_supported(&self, version: &ServerVersion) -> bool {
        match version.parsed {
            Some(v) if !version.unknown => {
                if (v.major, v.minor, v.patch) < (0, 28, 0) {
                    return false;
                }
                // `/ts2021` returned 405 for WebSocket upgrades in 0.29.0
                // through 0.29.1, fixed in 0.29.2.
                if v.major == 0 && v.minor == 29 && v.patch < 2 {
                    return false;
                }
                true
            }
            _ => true,
        }
    }

    /// True when the agent can create tag-only pre-auth keys.
    pub fn agent_supported(&self) -> bool {
        self.pre_auth_keys_have_stable_ids
    }
}

fn parse_semver(input: &str) -> Option<Semver> {
    // Strip build/prerelease metadata.
    let core = input.split(['+', '-']).next().unwrap_or(input);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Semver {
        major,
        minor,
        patch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_versions() {
        let v = ServerVersion::parse("v0.28.3");
        assert_eq!(v.parsed.unwrap().to_string(), "0.28.3");
        assert!(!v.unknown);
    }

    #[test]
    fn prerelease_ignored_for_comparison() {
        let v = ServerVersion::parse("v0.28.0-beta.1");
        assert!(v.capabilities().pre_auth_keys_have_stable_ids);
    }

    #[test]
    fn dev_and_pseudo_versions_are_permissive() {
        for raw in ["dev", "0.0.0-20260703052708-048308511c72"] {
            let v = ServerVersion::parse(raw);
            assert!(v.unknown, "{raw} should be unknown");
            assert!(v.capabilities().key_expiry_can_be_disabled);
            assert!(!v.below_minimum());
        }
    }

    #[test]
    fn old_versions_lack_new_capabilities() {
        let v = ServerVersion::parse("v0.27.1");
        let caps = v.capabilities();
        assert!(!caps.pre_auth_keys_have_stable_ids);
        assert!(!caps.node_owner_is_immutable);
        assert!(!caps.key_expiry_can_be_disabled);
        assert!(!v.below_minimum());
    }

    #[test]
    fn detects_below_minimum() {
        assert!(ServerVersion::parse("v0.26.0").below_minimum());
        assert!(!ServerVersion::parse("v0.27.0").below_minimum());
    }

    #[test]
    fn ssh_support_excludes_029_regression() {
        let broken = ServerVersion::parse("v0.29.1");
        assert!(!broken.capabilities().browser_ssh_supported(&broken));
        let fixed = ServerVersion::parse("v0.29.2");
        assert!(fixed.capabilities().browser_ssh_supported(&fixed));
    }
}
