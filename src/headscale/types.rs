//! Wire types for the Headscale v1 HTTP API.
//!
//! Field names mirror the `protojson` contract exactly: 64-bit integers arrive
//! as decimal strings and timestamps as RFC 3339.

use serde::{Deserialize, Serialize};

/// A device registered with the control server.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub id: String,

    #[serde(default)]
    pub machine_key: String,
    #[serde(default)]
    pub node_key: String,
    #[serde(default)]
    pub disco_key: String,

    #[serde(default)]
    pub ip_addresses: Vec<String>,

    #[serde(default)]
    pub name: String,

    /// Absent for tag-owned nodes on Headscale 0.28+.
    #[serde(default)]
    pub user: Option<User>,

    #[serde(default)]
    pub last_seen: String,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub expiry: Option<String>,

    #[serde(default)]
    pub pre_auth_key: Option<PreAuthKey>,

    #[serde(default)]
    pub created_at: String,

    #[serde(default)]
    pub register_method: String,

    /// Flat tag list on 0.28+, empty on older versions (see `forced_tags`).
    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(default)]
    pub given_name: String,

    #[serde(default)]
    pub online: bool,

    #[serde(default)]
    pub approved_routes: Vec<String>,

    #[serde(default)]
    pub available_routes: Vec<String>,

    #[serde(default)]
    pub subnet_routes: Vec<String>,

    // --- Pre-0.28 tag representation ---
    #[serde(default)]
    pub forced_tags: Vec<String>,
    #[serde(default)]
    pub valid_tags: Vec<String>,
    #[serde(default)]
    pub invalid_tags: Vec<String>,
}

impl Machine {
    /// Returns the effective tag list, normalized across Headscale versions.
    ///
    /// 0.28+ exposes a flat `tags` array. Earlier versions split the tags into
    /// `forcedTags` and `validTags`.
    pub fn effective_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.tags.clone();
        if tags.is_empty() {
            for tag in self.forced_tags.iter().chain(self.valid_tags.iter()) {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
        }
        tags
    }

    /// Whether a route CIDR is a full-tunnel exit route (`0.0.0.0/0` or `::/0`).
    ///
    /// A dual-stack exit node advertises both, but that is one exit node, so
    /// callers must collapse them rather than counting two.
    pub fn is_exit_route(route: &str) -> bool {
        route == "0.0.0.0/0" || route == "::/0"
    }

    /// Returns the first CGNAT IPv4 address, falling back to any IPv4.
    pub fn ipv4(&self) -> Option<&str> {
        self.ip_addresses
            .iter()
            .find(|ip| ip.starts_with("100."))
            .or_else(|| self.ip_addresses.iter().find(|ip| !ip.contains(':')))
            .map(|s| s.as_str())
    }

    /// Returns the first IPv6 address.
    pub fn ipv6(&self) -> Option<&str> {
        self.ip_addresses
            .iter()
            .find(|ip| {
                // Unique local addresses use fc00::/7, so both `fc…` and `fd…`.
                let prefix = ip.get(..2).unwrap_or("");
                prefix.eq_ignore_ascii_case("fc") || prefix.eq_ignore_ascii_case("fd")
            })
            .or_else(|| self.ip_addresses.iter().find(|ip| ip.contains(':')))
            .map(|s| s.as_str())
    }
    pub fn is_expired(&self) -> bool {
        let Some(expiry) = self.expiry.as_deref() else {
            return false;
        };
        match crate::util::parse_rfc3339(expiry) {
            Some(expiry) => expiry < chrono::Utc::now(),
            None => false,
        }
    }

    /// True when the key carries Headscale's never-expire sentinel.
    pub fn expiry_disabled(&self) -> bool {
        matches!(self.expiry.as_deref(), None | Some(""))
            || self
                .expiry
                .as_deref()
                .is_some_and(|e| e.starts_with("0001-01-01"))
    }
}

/// Headscale serializes absent optional strings as `""`, not by omitting them.
/// `Option<String>` alone would then yield `Some("")` and defeat UI fallbacks,
/// so empty strings become `None`.
fn empty_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|text| !text.is_empty()))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,

    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub created_at: String,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub display_name: Option<String>,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub email: Option<String>,

    /// For OIDC users this is a URL whose last path segment is the subject.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub provider_id: Option<String>,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub provider: Option<String>,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub profile_pic_url: Option<String>,
}

impl User {
    /// Returns the display name, falling back to the username.
    pub fn label(&self) -> &str {
        match self.display_name.as_deref() {
            Some(name) if !name.trim().is_empty() => name,
            _ => &self.name,
        }
    }

    pub fn is_oidc(&self) -> bool {
        self.provider.as_deref() == Some("oidc")
    }

    /// Returns the OIDC subject embedded in `providerId`.
    pub fn provider_subject(&self) -> Option<String> {
        let provider_id = self.provider_id.as_deref()?;
        if provider_id.is_empty() {
            return None;
        }
        let last = provider_id.rsplit('/').next().unwrap_or(provider_id);
        Some(percent_decode(last))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreAuthKey {
    #[serde(default)]
    pub id: String,

    #[serde(default)]
    pub key: String,

    /// Absent for tag-only keys.
    #[serde(default)]
    pub user: Option<User>,

    #[serde(default)]
    pub reusable: bool,

    #[serde(default)]
    pub ephemeral: bool,

    #[serde(default)]
    pub used: bool,

    #[serde(default)]
    pub expiration: String,

    #[serde(default)]
    pub created_at: String,

    #[serde(default)]
    pub acl_tags: Vec<String>,
}

/// A Headscale API key. Only the prefix is retrievable after creation.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    #[serde(default)]
    pub id: String,

    #[serde(default)]
    pub prefix: String,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub expiration: Option<String>,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub created_at: Option<String>,

    #[serde(default, deserialize_with = "empty_as_none")]
    pub last_seen: Option<String>,
}

impl ApiKey {
    /// Returns the prefix with the masking `*` characters removed, so you can
    /// match a submitted key with `starts_with`.
    pub fn matchable_prefix(&self) -> String {
        self.prefix.replace('*', "")
    }

    pub fn is_expired(&self) -> bool {
        self.expiration
            .as_deref()
            .and_then(crate::util::parse_rfc3339)
            .map(|expiry| expiry < chrono::Utc::now())
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodesResponse {
    #[serde(default)]
    pub nodes: Vec<Machine>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeResponse {
    #[serde(default)]
    pub node: Option<Machine>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsersResponse {
    #[serde(default)]
    pub users: Vec<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserResponse {
    #[serde(default)]
    pub user: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreAuthKeysResponse {
    #[serde(default, rename = "preAuthKeys")]
    pub pre_auth_keys: Vec<PreAuthKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreAuthKeyResponse {
    #[serde(default, rename = "preAuthKey")]
    pub pre_auth_key: Option<PreAuthKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeysResponse {
    #[serde(default, rename = "apiKeys")]
    pub api_keys: Vec<ApiKey>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    #[serde(default)]
    pub policy: String,

    /// `None` means Headscale is running in file mode: the policy is read-only.
    #[serde(default, deserialize_with = "empty_as_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionResponse {
    #[serde(default)]
    pub version: String,
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_merge_legacy_representation() {
        let machine: Machine = serde_json::from_str(
            r#"{
                "id": "1",
                "forcedTags": ["tag:server"],
                "validTags": ["tag:prod"],
                "ipAddresses": ["100.64.0.1", "fd7a::1"]
            }"#,
        )
        .unwrap();
        assert_eq!(machine.effective_tags(), vec!["tag:server", "tag:prod"]);
        assert_eq!(machine.ipv4(), Some("100.64.0.1"));
        assert_eq!(machine.ipv6(), Some("fd7a::1"));
    }

    #[test]
    fn ipv6_prefers_unique_local_addresses() {
        let machine: Machine = serde_json::from_str(
            r#"{"id":"1","ipAddresses":["100.64.0.2","fd7a:115c::2","fc11:22::3"]}"#,
        )
        .unwrap();
        assert_eq!(machine.ipv6(), Some("fd7a:115c::2"));

        let machine: Machine =
            serde_json::from_str(r#"{"id":"1","ipAddresses":["100.64.0.2","fc11:22::3"]}"#)
                .unwrap();
        assert_eq!(machine.ipv6(), Some("fc11:22::3"));
    }

    #[test]
    fn flat_tags_win_over_legacy() {
        let machine: Machine = serde_json::from_str(
            r#"{"id":"1","tags":["tag:a"],"forcedTags":["tag:b"],"validTags":["tag:c"]}"#,
        )
        .unwrap();
        assert_eq!(machine.effective_tags(), vec!["tag:a"]);
    }

    #[test]
    fn provider_subject_decodes_last_segment() {
        let user: User = serde_json::from_str(
            r#"{"id":"1","name":"a","providerId":"https://idp.example/u%40corp.com"}"#,
        )
        .unwrap();
        assert_eq!(user.provider_subject().as_deref(), Some("u@corp.com"));
    }

    #[test]
    fn empty_strings_become_none() {
        let user: User = serde_json::from_str(
            r#"{"id":"1","name":"alice","displayName":"","email":"","providerId":""}"#,
        )
        .unwrap();
        assert_eq!(user.display_name, None);
        assert_eq!(user.email, None);
        assert_eq!(user.provider_id, None);
        // `label` then falls back to the username rather than an empty string.
        assert_eq!(user.label(), "alice");

        let machine: Machine = serde_json::from_str(r#"{"id":"1","expiry":""}"#).unwrap();
        assert_eq!(machine.expiry, None);
        assert!(machine.expiry_disabled());
    }

    #[test]
    fn api_key_prefix_strips_masking() {
        let key: ApiKey = serde_json::from_str(r#"{"id":"1","prefix":"abcd*ef*"}"#).unwrap();
        assert_eq!(key.matchable_prefix(), "abcdef");
    }
}
