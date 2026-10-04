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
    /// `accept` for plain rules; unknown actions are preserved verbatim.
    #[serde(default = "default_accept")]
    pub action: String,

    #[serde(default)]
    pub src: Vec<String>,

    #[serde(default)]
    pub dst: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proto: Option<String>,

    /// Any additional keys on the rule object.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    #[serde(default, rename = "checkPeriod", skip_serializing_if = "Option::is_none")]
    pub check_period: Option<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_accept() -> String {
    "accept".into()
}

/// The typed view of a policy document.
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
        serde_json::to_string_pretty(self).context("failed to serialise the policy")
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
    /// `selected` is the complete set of groups the user should belong to;
    /// membership is removed from every other group.
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
/// Headscale requires `host:port`; the UI accepts bare hosts and normalises
/// them here.
pub fn with_default_port(destination: &str) -> String {
    let trimmed = destination.trim();
    if trimmed.is_empty() || trimmed.contains(':') {
        return trimmed.to_string();
    }
    format!("{trimmed}:*")
}

/// Whether normalising the destinations changed any of them.
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

        // Unknown top-level keys survive a round trip.
        let round_tripped = Policy::parse(&policy.to_text().unwrap()).unwrap();
        assert!(round_tripped.extra.contains_key("autoApprovers"));
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
        let policy =
            Policy::parse(r#"{"acls":[{"action":"accept","src":["*"],"dst":["*:*"],"proto":"tcp"}]}"#)
                .unwrap();
        let text = policy.to_text().unwrap();
        assert!(text.contains("\"proto\": \"tcp\""));
    }
}
