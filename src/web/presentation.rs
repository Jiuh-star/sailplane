//! Shaping Headscale data for the UI.
//!
//! The SPA needs views the raw API does not return: tags resolved across
//! Headscale versions, host info merged in, and capability flags flattened
//! for template use.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::auth::{Capability, Principal};
use crate::db::SailplaneUser;
use crate::headscale::{Machine, ServerVersion};

/// Role and capability flags for the signed-in principal.
#[derive(Debug, Clone, Serialize)]
pub struct AccessView {
    pub ui: bool,
    pub machines: bool,
    pub machines_write: bool,
    pub users: bool,
    pub users_write: bool,
    pub policy: bool,
    pub policy_write: bool,
    pub network: bool,
    pub network_write: bool,
    pub feature: bool,
    pub feature_write: bool,
    pub iam: bool,
    pub auth_keys: bool,
    pub auth_keys_own: bool,
    pub owner: bool,
}

impl AccessView {
    pub fn from_principal(principal: &Principal) -> Self {
        let has = |capability: Capability| principal.has(capability);
        Self {
            ui: has(Capability::UiAccess),
            machines: has(Capability::ReadMachines),
            machines_write: has(Capability::WriteMachines),
            users: has(Capability::ReadUsers),
            users_write: has(Capability::WriteUsers),
            policy: has(Capability::ReadPolicy),
            policy_write: has(Capability::WritePolicy),
            network: has(Capability::ReadNetwork),
            network_write: has(Capability::WriteNetwork),
            feature: has(Capability::ReadFeature),
            feature_write: has(Capability::WriteFeature),
            iam: has(Capability::ConfigureIam),
            auth_keys: has(Capability::GenerateAuthKeys),
            auth_keys_own: has(Capability::GenerateOwnAuthKeys),
            owner: has(Capability::Owner),
        }
    }
}

/// The signed-in user, as the header and account menu render them.
#[derive(Debug, Clone, Serialize)]
pub struct UserView {
    pub name: String,
    pub email: Option<String>,
    pub username: Option<String>,
    pub picture: Option<String>,
    pub role: String,
    pub role_label: String,
    pub is_owner: bool,
    pub headscale_user_id: Option<String>,
}

impl UserView {
    /// The stand-in shown for an API-key session.
    pub fn api_key(display_name: &str) -> Self {
        Self {
            name: display_name.to_string(),
            email: None,
            username: None,
            picture: None,
            role: "api_key".into(),
            role_label: "API Key".into(),
            is_owner: true,
            headscale_user_id: None,
        }
    }

    pub fn from_principal(principal: &Principal) -> Option<Self> {
        let user = principal.sailplane_user()?;
        Some(Self {
            name: user.label().to_string(),
            email: user.email.clone(),
            username: user.name.clone(),
            picture: user.picture.clone(),
            role: user.role.as_str().to_string(),
            role_label: user.role.label().to_string(),
            is_owner: user.is_owner(),
            headscale_user_id: user.headscale_user_id.clone(),
        })
    }
}

/// A Sailplane account joined with its Headscale user and machines.
#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub picture: Option<String>,
    pub role: String,
    pub role_label: String,
    pub is_owner: bool,
    pub headscale_user_id: Option<String>,
    /// Display name of the linked Headscale user, when resolvable.
    pub headscale_user_name: Option<String>,
    pub created_at: String,
    pub last_login_at: Option<String>,
    pub online: bool,
    pub last_seen: Option<String>,
    pub machine_count: usize,
    pub groups: Vec<String>,
}

impl AccountView {
    pub fn build(
        user: &SailplaneUser,
        headscale_users: &[crate::headscale::User],
        nodes: &[Machine],
        groups: Vec<String>,
    ) -> Self {
        let machines: Vec<&Machine> = nodes
            .iter()
            .filter(|node| {
                node.user.as_ref().map(|owner| owner.id.as_str())
                    == user.headscale_user_id.as_deref()
            })
            .collect();

        let online = machines.iter().any(|node| node.online);
        let last_seen = machines
            .iter()
            .filter_map(|node| crate::util::parse_rfc3339(&node.last_seen))
            .max()
            .map(crate::util::format_rfc3339);

        let headscale_user_name = user.headscale_user_id.as_deref().and_then(|id| {
            headscale_users
                .iter()
                .find(|candidate| candidate.id == id)
                .map(|candidate| candidate.label().to_string())
        });

        Self {
            id: user.id.clone(),
            name: user.label().to_string(),
            email: user.email.clone(),
            picture: user.picture.clone(),
            role: user.role.as_str().to_string(),
            role_label: user.role.label().to_string(),
            is_owner: user.is_owner(),
            headscale_user_id: user.headscale_user_id.clone(),
            headscale_user_name,
            created_at: crate::util::format_rfc3339(user.created_at),
            last_login_at: user.last_login_at.map(crate::util::format_rfc3339),
            online,
            last_seen,
            machine_count: machines.len(),
            groups,
        }
    }
}

/// A machine enriched with agent-reported host info.
#[derive(Debug, Clone, Serialize)]
pub struct MachineView {
    #[serde(flatten)]
    pub machine: Machine,

    /// Effective tags, normalised across Headscale versions.
    pub tags: Vec<String>,

    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub expired: bool,
    pub expiry_disabled: bool,

    /// UI status tags (exit node, subnet routes, Tailscale SSH, …).
    pub status_tags: Vec<StatusTag>,

    /// Raw host info from the agent, when available.
    pub host_info: Option<Value>,

    /// Convenience fields extracted from host info.
    pub version: Option<String>,
    pub os: Option<String>,
    pub endpoints: Vec<String>,
    pub ssh_host_keys: Vec<String>,
}

/// A short status badge for a machine.
///
/// `key` is the stable identifier the UI translates; `label` is the English
/// fallback, used when a client does not know the key yet.
#[derive(Debug, Clone, Serialize)]
pub struct StatusTag {
    pub key: &'static str,
    pub label: String,
    pub kind: &'static str,
    /// Route or tag the badge refers to, interpolated into the translation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

impl MachineView {
    pub fn build(machine: &Machine, host_info: Option<&Value>) -> Self {
        let tags = machine.effective_tags();
        let ssh_host_keys = host_info
            .and_then(|info| info.get("sshHostKeys"))
            .and_then(Value::as_array)
            .map(|keys| {
                keys.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let endpoints = host_info
            .and_then(|info| info.get("Endpoints"))
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            machine: machine.clone(),
            tags,
            ipv4: machine.ipv4().map(str::to_string),
            ipv6: machine.ipv6().map(str::to_string),
            expired: machine.is_expired(),
            expiry_disabled: machine.expiry_disabled(),
            status_tags: status_tags(machine, host_info),
            host_info: host_info.cloned(),
            version: host_info
                .and_then(|info| info.get("IPNVersion"))
                .and_then(Value::as_str)
                .map(str::to_string),
            os: host_info
                .and_then(|info| info.get("OS"))
                .and_then(Value::as_str)
                .map(str::to_string),
            endpoints,
            ssh_host_keys,
        }
    }
}

fn status_tags(machine: &Machine, host_info: Option<&Value>) -> Vec<StatusTag> {
    let mut tags = Vec::new();

    if machine.is_expired() {
        tags.push(StatusTag {
            key: "expired",
            label: "Expired".into(),
            kind: "danger",
            subject: None,
        });
    } else if machine.expiry_disabled() {
        tags.push(StatusTag {
            key: "noExpiry",
            label: "No expiry".into(),
            kind: "muted",
            subject: None,
        });
    }

    let approved = &machine.approved_routes;
    let available = &machine.available_routes;

    for route in available {
        let is_exit = route == "0.0.0.0/0" || route == "::/0";
        let blessed = approved.contains(route);
        if is_exit {
            tags.push(StatusTag {
                key: if blessed { "exitNode" } else { "exitNodePending" },
                label: if blessed {
                    "Exit node".into()
                } else {
                    "Exit node (pending)".into()
                },
                kind: if blessed { "success" } else { "warning" },
                subject: None,
            });
        } else {
            tags.push(StatusTag {
                key: if blessed { "subnet" } else { "subnetPending" },
                label: if blessed {
                    format!("Subnet {route}")
                } else {
                    format!("Subnet {route} (pending)")
                },
                kind: if blessed { "success" } else { "warning" },
                subject: Some(route.clone()),
            });
        }
    }

    if host_info
        .and_then(|info| info.get("sshHostKeys"))
        .and_then(Value::as_array)
        .is_some_and(|keys| !keys.is_empty())
    {
        tags.push(StatusTag {
            key: "tailscaleSsh",
            label: "Tailscale SSH".into(),
            kind: "info",
            subject: None,
        });
    }

    if host_info
        .and_then(|info| info.get("SailplaneAgent"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        tags.push(StatusTag {
            key: "sailplaneAgent",
            label: "Sailplane agent".into(),
            kind: "info",
            subject: None,
        });
    }

    tags
}

/// Capability flags advertised for the connected Headscale version.
#[derive(Debug, Clone, Serialize)]
pub struct HeadscaleVersionView {
    pub raw: String,
    pub canonical: String,
    pub unknown: bool,
    pub below_minimum: bool,
    pub capabilities: crate::headscale::Capabilities,
    pub browser_ssh_supported: bool,
}

impl HeadscaleVersionView {
    pub fn build(version: &ServerVersion) -> Self {
        let capabilities = version.capabilities();
        Self {
            raw: version.raw.clone(),
            canonical: version.canonical(),
            unknown: version.unknown,
            below_minimum: version.below_minimum(),
            browser_ssh_supported: capabilities.browser_ssh_supported(version),
            capabilities,
        }
    }
}

/// Builds a JSON object that maps each tag to the machines that use it.
pub fn tag_usage(nodes: &[Machine]) -> Value {
    let mut usage: Map<String, Value> = Map::new();

    for node in nodes {
        for tag in node.effective_tags() {
            let entry = usage
                .entry(tag)
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(array) = entry.as_array_mut() {
                array.push(Value::String(
                    node.given_name.clone().max(node.name.clone()),
                ));
            }
        }
    }

    Value::Object(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_view_reflects_role() {
        let viewer = Principal::User {
            user: SailplaneUser {
                id: "1".into(),
                sub: "sub".into(),
                name: Some("v".into()),
                email: None,
                picture: None,
                role: crate::auth::Role::Viewer,
                headscale_user_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_login_at: None,
            },
            id_token: None,
        };
        let access = AccessView::from_principal(&viewer);
        assert!(access.machines);
        assert!(!access.machines_write);
        assert!(!access.owner);
    }

    #[test]
    fn machine_view_normalises_tags_and_addresses() {
        let machine: Machine = serde_json::from_str(
            r#"{
                "id": "1",
                "givenName": "web",
                "ipAddresses": ["100.64.0.2", "fd7a::2"],
                "forcedTags": ["tag:a"],
                "availableRoutes": ["10.0.0.0/8", "0.0.0.0/0"],
                "approvedRoutes": ["10.0.0.0/8"]
            }"#,
        )
        .unwrap();

        let view = MachineView::build(&machine, None);
        assert_eq!(view.tags, vec!["tag:a"]);
        assert_eq!(view.ipv4.as_deref(), Some("100.64.0.2"));
        assert_eq!(view.ipv6.as_deref(), Some("fd7a::2"));

        let labels: Vec<&str> = view.status_tags.iter().map(|t| t.label.as_str()).collect();
        assert!(labels.contains(&"Subnet 10.0.0.0/8"));
        assert!(labels.contains(&"Exit node (pending)"));
    }

    #[test]
    fn expired_machines_are_flagged() {
        let machine: Machine = serde_json::from_str(
            r#"{"id":"1","expiry":"2000-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        let view = MachineView::build(&machine, None);
        assert!(view.expired);
        assert!(view.status_tags.iter().any(|t| t.label == "Expired"));
    }
}
