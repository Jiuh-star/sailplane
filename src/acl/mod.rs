//! Structured access to the Headscale ACL policy.
//!
//! The policy is one HuJSON document. Structured editors round-trip it through
//! the typed [`Policy`] model, which preserves unknown top-level keys but drops
//! comments. The UI warns about that using [`hujson::has_comments`].

pub mod eval;
pub mod hujson;

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// An access rule inside `acls`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclRule {
    /// `accept` for plain rules. Unknown actions are preserved verbatim.
    #[serde(default = "default_accept")]
    pub action: String,

    #[serde(default)]
    pub src: Vec<String>,

    #[serde(default)]
    pub dst: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proto: Option<String>,

    /// Device posture conditions the source must satisfy.
    #[serde(default, rename = "srcPosture", skip_serializing_if = "Vec::is_empty")]
    pub src_posture: Vec<String>,

    /// More keys on the rule object.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A rule inside `grants`, the newer Tailscale access syntax.
///
/// `ip` carries the protocol and ports (`tcp:443`, `udp:*`, `*`). `app` names
/// application capabilities. Fields whose exact shape is uncertain keep their
/// keys in `extra`, so a round trip never drops them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GrantRule {
    #[serde(default)]
    pub src: Vec<String>,

    #[serde(default)]
    pub dst: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ip: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,

    /// Application capabilities. Tailscale writes this as an object keyed by
    /// capability name (`{"tailscale.com/cap/drive": [{"access": "rw"}]}`). An
    /// older, array-shaped form also exists. Kept as a `Value` so any shape
    /// round-trips and an unrecognized one cannot push the whole `grants`
    /// section into the [`Section::Raw`] fallback, which would hide every grant
    /// from the topology and the access checker.
    #[serde(default = "null_value", skip_serializing_if = "Value::is_null")]
    pub app: Value,

    #[serde(default, rename = "srcPosture", skip_serializing_if = "Vec::is_empty")]
    pub src_posture: Vec<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl GrantRule {
    /// Whether the grant carries application capabilities.
    pub fn has_app(&self) -> bool {
        !self.app.is_null()
    }

    /// The capability names the grant names, best-effort across shapes. An
    /// object contributes its keys. An array contributes each entry's `cap`.
    pub fn capabilities(&self) -> Vec<String> {
        match &self.app {
            Value::Object(map) => map.keys().cloned().collect(),
            Value::Array(items) => items
                .iter()
                .filter_map(|item| item.get("cap"))
                .flat_map(|cap| match cap {
                    Value::String(name) => vec![name.clone()],
                    Value::Array(names) => names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    _ => Vec::new(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

fn null_value() -> Value {
    Value::Null
}

/// An SSH rule inside `ssh`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshRule {
    #[serde(default = "default_accept")]
    pub action: String,

    #[serde(default)]
    pub src: Vec<String>,

    #[serde(default)]
    pub dst: Vec<String>,

    #[serde(default)]
    pub users: Vec<String>,

    /// Only meaningful when `action` is `check`.
    #[serde(
        default,
        rename = "checkPeriod",
        skip_serializing_if = "Option::is_none"
    )]
    pub check_period: Option<String>,

    /// Device posture conditions the source must satisfy.
    #[serde(default, rename = "srcPosture", skip_serializing_if = "Vec::is_empty")]
    pub src_posture: Vec<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Automatic route and exit-node approval by selector.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoApprovers {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routes: BTreeMap<String, Vec<String>>,

    #[serde(default, rename = "exitNode", skip_serializing_if = "Vec::is_empty")]
    pub exit_node: Vec<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A `nodeAttrs` entry: attributes applied to the nodes a selector names.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeAttr {
    #[serde(default)]
    pub target: Vec<String>,

    #[serde(default)]
    pub attr: Vec<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A section that is structured when its shape is understood, and preserved
/// verbatim when it is not.
///
/// This keeps a policy Headscale accepts from failing to parse here: an
/// unexpected shape falls back to `Raw` and is re-emitted unchanged, so the
/// structured editor stays reachable instead of locking on a parse error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Section<T> {
    Typed(T),
    Raw(Value),
}

impl<T> Section<T> {
    /// The structured value, when the section parsed into the typed form.
    pub fn typed(&self) -> Option<&T> {
        match self {
            Section::Typed(value) => Some(value),
            Section::Raw(_) => None,
        }
    }
}

fn default_accept() -> String {
    "accept".into()
}

/// The typed view of a policy document.
///
/// Every rule type Tailscale defines has a slot here. Sections whose exact
/// shape is not modeled are held in a [`Section`], which preserves an
/// unrecognized shape instead of failing to parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub acls: Vec<AclRule>,

    #[serde(default)]
    pub ssh: Vec<SshRule>,

    #[serde(default)]
    pub hosts: BTreeMap<String, String>,

    #[serde(default)]
    pub groups: BTreeMap<String, Vec<String>>,

    #[serde(default, rename = "tagOwners")]
    pub tag_owners: BTreeMap<String, Vec<String>>,

    /// `grants`, the newer access syntax that can replace `acls`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grants: Option<Section<Vec<GrantRule>>>,

    #[serde(
        default,
        rename = "autoApprovers",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_approvers: Option<Section<AutoApprovers>>,

    #[serde(default, rename = "nodeAttrs", skip_serializing_if = "Option::is_none")]
    pub node_attrs: Option<Section<Vec<NodeAttr>>>,

    /// Named posture conditions referenced by `srcPosture`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postures: Option<Section<BTreeMap<String, Vec<String>>>>,

    /// Policy assertions, preserved and edited as JSON (shape is version
    /// dependent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<Section<Vec<Value>>>,

    #[serde(default, rename = "sshTests", skip_serializing_if = "Option::is_none")]
    pub ssh_tests: Option<Section<Vec<Value>>>,

    #[serde(
        default,
        rename = "randomizeClientPort",
        skip_serializing_if = "Option::is_none"
    )]
    pub randomize_client_port: Option<bool>,

    /// Unknown top-level keys, preserved so saving never destroys them.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Policy {
    /// Parses policy text, tolerating HuJSON syntax.
    pub fn parse(text: &str) -> Result<Self> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let value = hujson::parse(text)
            .map_err(|err| anyhow::anyhow!("parsing HuJSON: {err}"))
            .context("could not parse the ACL policy")?;

        // A policy of `null` or `{}` is valid and means "no rules".
        match value {
            Value::Null => Ok(Self::default()),
            other => serde_json::from_value(other)
                .context("policy does not match the expected structure"),
        }
    }

    /// Renders the policy back to text.
    pub fn to_text(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("failed to serialize the policy")
    }

    /// The `grants` rules, when the section parsed into the typed form.
    pub fn grant_rules(&self) -> &[GrantRule] {
        self.grants
            .as_ref()
            .and_then(Section::typed)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Names declared under `postures`.
    pub fn posture_names(&self) -> Vec<&str> {
        self.postures
            .as_ref()
            .and_then(Section::typed)
            .map(|postures| postures.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Tags declared under `tagOwners`.
    pub fn declared_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.tag_owners.keys().cloned().collect();
        tags.sort();
        tags
    }

    /// Groups that list `username@` as a member.
    pub fn groups_for_user(&self, username: &str) -> Vec<String> {
        let member = format!("{username}@");
        self.groups
            .iter()
            .filter(|(_, members)| members.iter().any(|m| m == &member))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Replaces `username@` group membership wholesale.
    ///
    /// `selected` is the complete set of groups the user must belong to.
    /// It removes membership from every other group.
    pub fn set_user_groups(&mut self, username: &str, selected: &[String]) -> Result<()> {
        for group in selected {
            if !is_valid_group_name(group) {
                bail!("`{group}` is not a valid group name");
            }
        }

        let member = format!("{username}@");
        let group_names: Vec<String> = self.groups.keys().cloned().collect();

        for group in group_names {
            let members = self.groups.get_mut(&group).expect("key from iteration");
            let should_contain = selected.iter().any(|g| g == &group);
            let contains = members.iter().any(|m| m == &member);

            match (should_contain, contains) {
                (true, false) => members.push(member.clone()),
                (false, true) => members.retain(|m| m != &member),
                _ => {}
            }
        }

        // Selected groups that do not exist were rejected above, so nothing
        // else to do.
        Ok(())
    }
}

/// `tag:` prefixed, non-empty, no whitespace.
pub fn is_valid_tag_name(name: &str) -> bool {
    name.starts_with("tag:")
        && name.len() > 4
        && !name.contains(char::is_whitespace)
        && !name.contains('"')
}

/// `group:` prefixed, non-empty, no whitespace.
pub fn is_valid_group_name(name: &str) -> bool {
    name.starts_with("group:")
        && name.len() > 6
        && !name.contains(char::is_whitespace)
        && !name.contains('"')
}
/// Appends the default port when a destination omits one.
///
/// `host:port` is necessary for Headscale. The UI accepts bare hosts and
/// normalizes them here.
pub fn with_default_port(destination: &str) -> String {
    let trimmed = destination.trim();
    if trimmed.is_empty() || trimmed.contains(':') {
        return trimmed.to_string();
    }
    format!("{trimmed}:*")
}

/// Whether normalizing the destinations changed any of them.
pub fn normalise_destinations(destinations: &[String]) -> (Vec<String>, bool) {
    let mut changed = false;
    let normalised = destinations
        .iter()
        .map(|dst| {
            let next = with_default_port(dst);
            if &next != dst {
                changed = true;
            }
            next
        })
        .collect();
    (normalised, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_realistic_policy() {
        let text = r#"{
            // comments are fine
            "acls": [
                { "action": "accept", "src": ["*"], "dst": ["*:*"] },
            ],
            "hosts": { "server": "100.64.0.1" },
            "groups": { "group:ops": ["alice@", "bob@"] },
            "tagOwners": { "tag:prod": ["group:ops"] },
            "autoApprovers": { "routes": { "10.0.0.0/8": ["tag:prod"] } },
        }"#;

        let policy = Policy::parse(text).unwrap();
        assert_eq!(policy.acls.len(), 1);
        assert_eq!(policy.acls[0].action, "accept");
        assert_eq!(policy.declared_tags(), vec!["tag:prod"]);
        assert_eq!(policy.groups_for_user("alice"), vec!["group:ops"]);
        assert!(policy.groups_for_user("carol").is_empty());

        // `autoApprovers` is now modeled, and the typed routes survive a
        // round trip.
        let approvers = policy
            .auto_approvers
            .as_ref()
            .and_then(Section::typed)
            .expect("autoApprovers is typed");
        assert_eq!(approvers.routes["10.0.0.0/8"], vec!["tag:prod"]);

        let round_tripped = Policy::parse(&policy.to_text().unwrap()).unwrap();
        let approvers = round_tripped
            .auto_approvers
            .as_ref()
            .and_then(Section::typed)
            .expect("autoApprovers survives a round trip");
        assert_eq!(approvers.routes["10.0.0.0/8"], vec!["tag:prod"]);
    }

    #[test]
    fn grants_and_src_posture_round_trip() {
        let text = r#"{
            "postures": { "posture:latest": ["node:os == 'macos'"] },
            "grants": [
                {
                    "src": ["group:ops"],
                    "dst": ["tag:server"],
                    "ip": ["tcp:22", "tcp:443"],
                    "srcPosture": ["posture:latest"],
                    "app": [{ "cap": ["funnel"] }]
                }
            ]
        }"#;

        let policy = Policy::parse(text).unwrap();
        assert_eq!(policy.grant_rules().len(), 1);
        let grant = &policy.grant_rules()[0];
        assert_eq!(grant.ip, vec!["tcp:22", "tcp:443"]);
        assert_eq!(grant.src_posture, vec!["posture:latest"]);
        assert_eq!(policy.posture_names(), vec!["posture:latest"]);

        let text = policy.to_text().unwrap();
        assert!(text.contains("srcPosture"), "{text}");
        assert!(text.contains("\"cap\""), "{text}");
    }

    /// The shape Tailscale actually writes: `app` is an object keyed by
    /// capability name. It used to be modeled as an array, so serde rejected
    /// the whole `grants` section and every grant silently vanished.
    #[test]
    fn a_map_shaped_app_grants_parse() {
        let text = r#"{
            "grants": [
                {
                    "src": ["*"],
                    "dst": ["*"],
                    "app": {
                        "tailscale.com/cap/drive": [
                            { "shares": ["*"], "access": "rw" }
                        ]
                    }
                }
            ],
            "nodeAttrs": [
                { "target": ["autogroup:member"], "attr": ["drive:share", "drive:access"] }
            ]
        }"#;

        let policy = Policy::parse(text).unwrap();
        let grants = policy.grant_rules();
        assert_eq!(grants.len(), 1, "the grants section must stay typed");
        assert!(grants[0].ip.is_empty());
        assert!(grants[0].has_app());
        assert_eq!(
            grants[0].capabilities(),
            vec!["tailscale.com/cap/drive".to_string()]
        );

        // The object shape survives a round trip.
        let round_tripped = Policy::parse(&policy.to_text().unwrap()).unwrap();
        assert_eq!(round_tripped.grant_rules().len(), 1);
        assert!(round_tripped.to_text().unwrap().contains("tailscale.com/cap/drive"));
    }

    /// A section with an unexpected shape must not fail the parse. The parser
    /// keeps it verbatim so the structured editor stays reachable.
    #[test]
    fn an_unexpected_section_shape_is_preserved() {
        let policy = Policy::parse(r#"{"grants":"not-an-array","acls":[]}"#).unwrap();
        assert!(policy.grant_rules().is_empty());
        let text = policy.to_text().unwrap();
        assert!(text.contains("\"grants\": \"not-an-array\""), "{text}");
    }

    #[test]
    fn empty_and_null_policies_parse() {
        assert_eq!(Policy::parse("").unwrap().acls.len(), 0);
        assert_eq!(Policy::parse("null").unwrap().acls.len(), 0);
        assert_eq!(Policy::parse("{}").unwrap().acls.len(), 0);
    }

    #[test]
    fn malformed_policy_is_rejected() {
        let err = Policy::parse("{ not json").unwrap_err();
        assert!(err.to_string().contains("could not parse"), "{err}");
    }

    #[test]
    fn setting_groups_adds_and_removes_membership() {
        let mut policy = Policy::parse(
            r#"{"groups":{"group:a":["alice@","bob@"],"group:b":["alice@"],"group:c":["carol@"]}}"#,
        )
        .unwrap();

        policy
            .set_user_groups("alice", &["group:b".into()])
            .unwrap();

        assert_eq!(policy.groups["group:a"], vec!["bob@"]);
        assert_eq!(policy.groups["group:b"], vec!["alice@"]);
        assert_eq!(policy.groups["group:c"], vec!["carol@"]);
    }

    #[test]
    fn setting_groups_rejects_unknown_group_names() {
        let mut policy = Policy::default();
        assert!(policy.set_user_groups("alice", &["ops".into()]).is_err());
    }

    #[test]
    fn destination_normalisation_appends_default_port() {
        assert_eq!(with_default_port("100.64.0.1"), "100.64.0.1:*");
        assert_eq!(with_default_port("100.64.0.1:443"), "100.64.0.1:443");
        assert_eq!(with_default_port("*"), "*:*");
        assert_eq!(with_default_port(""), "");
    }

    #[test]
    fn name_validation() {
        assert!(is_valid_tag_name("tag:prod"));
        assert!(!is_valid_tag_name("prod"));
        assert!(!is_valid_tag_name("tag:"));
        assert!(is_valid_group_name("group:ops"));
        assert!(!is_valid_group_name("group:"));
    }

    #[test]
    fn acl_rules_preserve_unknown_fields() {
        let policy = Policy::parse(
            r#"{"acls":[{"action":"accept","src":["*"],"dst":["*:*"],"proto":"tcp"}]}"#,
        )
        .unwrap();
        let text = policy.to_text().unwrap();
        assert!(text.contains("\"proto\": \"tcp\""));
    }
}
