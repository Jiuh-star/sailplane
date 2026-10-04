//! Whether one machine may reach another, according to a policy.
//!
//! Headscale compiles the policy into a packet filter and does not answer
//! reachability questions. This module re-implements the decision for the UI:
//! the first rule whose `src` and `dst` both match decides, and anything
//! unmatched is denied.

use std::net::IpAddr;

use serde::Serialize;

use crate::headscale::Machine;

use super::{AclRule, Policy, SshRule};

/// A machine reduced to the fields access control depends on.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MachineRef {
    pub id: String,
    pub name: String,
    pub addresses: Vec<String>,
    /// Owner's Headscale user name, absent for tag-owned nodes.
    pub user: Option<String>,
    pub tags: Vec<String>,
}

impl MachineRef {
    pub fn from_machine(machine: &Machine) -> Self {
        Self {
            id: machine.id.clone(),
            name: if machine.given_name.is_empty() {
                machine.name.clone()
            } else {
                machine.given_name.clone()
            },
            addresses: machine.ip_addresses.clone(),
            user: machine.user.as_ref().map(|user| user.name.clone()),
            tags: machine.effective_tags(),
        }
    }
}

/// One rule's verdict, for the per-rule breakdown the UI shows.
#[derive(Debug, Clone, Serialize)]
pub struct RuleOutcome {
    pub index: usize,
    pub action: String,
    pub src: Vec<String>,
    pub dst: Vec<String>,
    /// Which selector in `src` matched, when one did.
    pub matched_src: Option<String>,
    /// Which selector in `dst` matched, when one did.
    pub matched_dst: Option<String>,
    pub matched: bool,
}

impl RuleOutcome {
    fn from_acl(index: usize, rule: &AclRule) -> Self {
        Self {
            index,
            action: rule.action.clone(),
            src: rule.src.clone(),
            dst: rule.dst.clone(),
            matched_src: None,
            matched_dst: None,
            matched: false,
        }
    }

    fn from_ssh(index: usize, rule: &SshRule) -> Self {
        Self {
            index,
            action: rule.action.clone(),
            src: rule.src.clone(),
            dst: rule.dst.clone(),
            matched_src: None,
            matched_dst: None,
            matched: false,
        }
    }
}

/// The rule that decided the request.
#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub index: usize,
    pub action: String,
    pub src: Vec<String>,
    pub dst: Vec<String>,
    /// The selector pair that matched.
    pub matched_src: String,
    pub matched_dst: String,
}

/// The answer for one source/destination pair.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub allowed: bool,
    pub src: String,
    pub dst: String,
    pub port: u16,
    pub protocol: String,
    /// Machines the source selector expands to.
    pub source_machines: Vec<MachineRef>,
    /// Machines the destination selector (host part) expands to.
    pub destination_machines: Vec<MachineRef>,
    pub decision: Option<Decision>,
    pub rules: Vec<RuleOutcome>,
    /// SSH rules, evaluated only for port 22. They govern Tailscale SSH, not
    /// ordinary TCP.
    pub ssh_rules: Vec<RuleOutcome>,
    /// Selectors that could not be evaluated, plus other caveats for the verdict.
    pub notes: Vec<String>,
}

/// A request to evaluate.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Query {
    pub src: String,
    pub dst: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_port() -> u16 {
    22
}

fn default_protocol() -> String {
    "tcp".into()
}

/// Evaluates `query` against `policy` and the machines in the tailnet.
pub fn evaluate(policy: &Policy, machines: &[MachineRef], query: &Query) -> Report {
    let mut notes = Vec::new();
    let protocol = query.protocol.to_ascii_lowercase();

    let source_machines = expand(policy, machines, query.src.trim(), &mut notes);
    let (host, ports) = split_destination(query.dst.trim());
    let destination_machines = expand(policy, machines, &host, &mut notes);

    let port_matches = ports.matches(query.port);
    if !port_matches {
        notes.push(format!(
            "the destination `{}` does not cover port {}",
            query.dst.trim(),
            query.port
        ));
    }

    if source_machines.is_empty() {
        notes.push(format!(
            "`{}` does not select any machine in this tailnet",
            query.src.trim()
        ));
    }
    if destination_machines.is_empty() {
        notes.push(format!(
            "`{}` does not select any machine in this tailnet",
            host
        ));
    }

    let mut rules = Vec::with_capacity(policy.acls.len());
    let mut decision = None;

    for (index, rule) in policy.acls.iter().enumerate() {
        let mut outcome = RuleOutcome::from_acl(index, rule);

        if !protocol_matches(rule.proto.as_deref(), &protocol) {
            rules.push(outcome);
            continue;
        }

        let Some(src_selector) = matching_selector(policy, &source_machines, &rule.src, &mut notes)
        else {
            rules.push(outcome);
            continue;
        };

        let Some((dst_selector, rule_ports)) =
            matching_destination(policy, &destination_machines, &rule.dst, &mut notes)
        else {
            rules.push(outcome);
            continue;
        };

        // Both ends match; the port decides whether the rule applies at all.
        if !rule_ports.matches(query.port) {
            rules.push(outcome);
            continue;
        }

        outcome.matched = true;
        outcome.matched_src = Some(src_selector.clone());
        outcome.matched_dst = Some(dst_selector.clone());
        rules.push(outcome.clone());

        if decision.is_none() {
            decision = Some(Decision {
                index,
                action: rule.action.clone(),
                src: rule.src.clone(),
                dst: rule.dst.clone(),
                matched_src: src_selector,
                matched_dst: dst_selector,
            });
        }
    }

    // SSH rules are a separate list and only speak for Tailscale SSH.
    let mut ssh_rules = Vec::new();
    if query.port == 22 {
        for (index, rule) in policy.ssh.iter().enumerate() {
            let mut outcome = RuleOutcome::from_ssh(index, rule);
            if let (Some(src), Some((dst, _))) = (
                matching_selector(policy, &source_machines, &rule.src, &mut notes),
                matching_destination(policy, &destination_machines, &rule.dst, &mut notes),
            ) {
                outcome.matched = true;
                outcome.matched_src = Some(src);
                outcome.matched_dst = Some(dst);
            }
            ssh_rules.push(outcome);
        }
    }

    let allowed = decision
        .as_ref()
        .is_some_and(|decision| decision.action == "accept");

    if decision.is_none() {
        notes.push(
            "no ACL rule matched, which is the default: traffic is denied".to_string(),
        );
    }

    Report {
        allowed,
        src: query.src.trim().to_string(),
        dst: query.dst.trim().to_string(),
        port: query.port,
        protocol,
        source_machines,
        destination_machines,
        decision,
        rules,
        ssh_rules,
        notes,
    }
}

/// Expands a selector to the machines it names.
pub fn expand(policy: &Policy, machines: &[MachineRef], selector: &str, notes: &mut Vec<String>) -> Vec<MachineRef> {
    if selector.is_empty() {
        return Vec::new();
    }
    if selector.starts_with("autogroup:") {
        notes.push(format!(
            "`{selector}` depends on who is asking, which this check cannot know"
        ));
        return Vec::new();
    }

    machines
        .iter()
        .filter(|machine| matches_selector(policy, machine, selector))
        .cloned()
        .collect()
}

/// Whether a machine is named by a selector.
fn matches_selector(policy: &Policy, machine: &MachineRef, selector: &str) -> bool {
    let selector = selector.trim();

    if selector == "*" {
        return true;
    }
    if let Some(tag) = selector.strip_prefix("tag:") {
        let tag = format!("tag:{tag}");
        return machine.tags.iter().any(|owned| owned == &tag);
    }
    if let Some(group) = selector.strip_prefix("group:") {
        let name = format!("group:{group}");
        return policy
            .groups
            .get(&name)
            .is_some_and(|members| members.iter().any(|member| matches_selector(policy, machine, member)));
    }
    if let Some(alias) = policy.hosts.get(selector) {
        return machine.addresses.iter().any(|address| address_in(alias, address));
    }
    if is_address_or_cidr(selector) {
        return machine
            .addresses
            .iter()
            .any(|address| address_in(selector, address));
    }

    // A user name, with or without the trailing `@`.
    let user = selector.strip_suffix('@').unwrap_or(selector);
    if machine.user.as_deref() == Some(user) {
        return true;
    }

    // A node name, or the MagicDNS name it is registered under.
    let short = selector.split('.').next().unwrap_or(selector);
    machine.name == selector || machine.name == short
}

/// The first selector in `selectors` that names one of `machines`.
fn matching_selector(
    policy: &Policy,
    machines: &[MachineRef],
    selectors: &[String],
    notes: &mut Vec<String>,
) -> Option<String> {
    for selector in selectors {
        if selector.trim().is_empty() {
            continue;
        }
        note_autogroup(notes, selector);
        if machines
            .iter()
            .any(|machine| matches_selector(policy, machine, selector))
        {
            return Some(selector.clone());
        }
    }
    None
}

/// Finds the first destination selector that names one of `machines`.
///
/// The ports stay attached to the host until the host has been matched.
fn matching_destination(
    policy: &Policy,
    machines: &[MachineRef],
    selectors: &[String],
    notes: &mut Vec<String>,
) -> Option<(String, Ports)> {
    for selector in selectors {
        if selector.trim().is_empty() {
            continue;
        }
        note_autogroup(notes, selector);
        let (host, ports) = split_destination(selector);
        if machines
            .iter()
            .any(|machine| matches_selector(policy, machine, &host))
        {
            return Some((selector.clone(), ports));
        }
    }
    None
}

fn note_autogroup(notes: &mut Vec<String>, selector: &str) {
    if selector.contains("autogroup:") && !notes.iter().any(|note| note.contains("autogroup:")) {
        notes.push(
            "`autogroup:` selectors depend on who is asking and are not evaluated here".to_string(),
        );
    }
}

fn protocol_matches(rule: Option<&str>, protocol: &str) -> bool {
    match rule {
        None => true,
        Some(rule) => {
            let rule = rule.to_ascii_lowercase();
            rule == protocol || rule == "*"
        }
    }
}

/// Splits `host:ports`, tolerating the colons inside IPv6 addresses.
///
/// A bare address, including IPv6, is always a host: `fd7a::1` cannot be told
/// apart from `fd7a::` plus port 1, so an IPv6 literal with a port must be
/// bracketed, as Headscale requires.
pub fn split_destination(destination: &str) -> (String, Ports) {
    let destination = destination.trim();
    if destination.is_empty() {
        return (String::new(), Ports::ANY);
    }

    if let Some(rest) = destination.strip_prefix('[')
        && let Some((host, tail)) = rest.split_once(']')
    {
        let ports = tail
            .strip_prefix(':')
            .map(Ports::parse)
            .unwrap_or(Ports::ANY);
        return (host.to_string(), ports);
    }

    if destination.parse::<IpAddr>().is_ok() {
        return (destination.to_string(), Ports::ANY);
    }

    if let Some((head, tail)) = destination.rsplit_once(':')
        && !tail.is_empty()
        && tail
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '-' | '*'))
    {
        return (head.to_string(), Ports::parse(tail));
    }

    (destination.to_string(), Ports::ANY)
}

/// A port specification: `*`, `22`, `80,443`, `1000-2000`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ports(Vec<(u16, u16)>);

impl Ports {
    pub const ANY: Self = Self(Vec::new());

    pub fn parse(spec: &str) -> Self {
        let spec = spec.trim();
        if spec.is_empty() || spec == "*" {
            return Self::ANY;
        }

        let ranges = spec
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part == "*" {
                    return None;
                }
                match part.split_once('-') {
                    Some((start, end)) => {
                        Some((start.trim().parse().ok()?, end.trim().parse().ok()?))
                    }
                    None => {
                        let port: u16 = part.parse().ok()?;
                        Some((port, port))
                    }
                }
            })
            .collect();

        Self(ranges)
    }

    pub fn matches(&self, port: u16) -> bool {
        self.0.is_empty() || self.0.iter().any(|(start, end)| (*start..=*end).contains(&port))
    }
}

/// What a selector names, so the UI can label and group it.
pub fn selector_kind(selector: &str) -> &'static str {
    let selector = selector.trim();
    if selector == "*" {
        "any"
    } else if selector.starts_with("tag:") {
        "tag"
    } else if selector.starts_with("group:") {
        "group"
    } else if selector.starts_with("autogroup:") {
        "autogroup"
    } else if is_address_or_cidr(selector) {
        "address"
    } else if selector.ends_with('@') {
        "user"
    } else {
        "host"
    }
}

/// The port list of a `host:ports` selector, exactly as written, for display.
pub fn destination_ports(selector: &str) -> String {
    let selector = selector.trim();
    if selector.parse::<IpAddr>().is_ok() {
        return "*".into();
    }
    match selector.rsplit_once(':') {
        Some((_, tail))
            if !tail.is_empty()
                && tail
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ',' | '-' | '*')) =>
        {
            tail.to_string()
        }
        _ => "*".into(),
    }
}

fn is_address_or_cidr(value: &str) -> bool {
    value.contains('/') || value.parse::<IpAddr>().is_ok()
}

/// Whether `address` falls inside `pattern`, which may be an address or a CIDR.
fn address_in(pattern: &str, address: &str) -> bool {
    let Ok(address) = address.parse::<IpAddr>() else {
        return false;
    };

    match pattern.split_once('/') {
        Some((network, prefix)) => {
            let (Ok(network), Ok(prefix)) = (network.parse::<IpAddr>(), prefix.parse::<u8>()) else {
                return false;
            };
            in_prefix(network, address, prefix)
        }
        None => pattern.parse::<IpAddr>().is_ok_and(|pattern| pattern == address),
    }
}

fn in_prefix(network: IpAddr, address: IpAddr, prefix: u8) -> bool {
    match (network, address) {
        (IpAddr::V4(network), IpAddr::V4(address)) if prefix <= 32 => {
            let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
            u32::from(network) & mask == u32::from(address) & mask
        }
        (IpAddr::V6(network), IpAddr::V6(address)) if prefix <= 128 => {
            let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
            u128::from(network) & mask == u128::from(address) & mask
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(id: &str, name: &str, address: &str, user: Option<&str>, tags: &[&str]) -> MachineRef {
        MachineRef {
            id: id.into(),
            name: name.into(),
            addresses: vec![address.into()],
            user: user.map(str::to_string),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        }
    }

    /// Two users, a tagged server, and a host alias pointing at the server.
    fn tailnet() -> Vec<MachineRef> {
        vec![
            machine("1", "alice-laptop", "100.64.0.1", Some("alice"), &[]),
            machine("2", "bob-desktop", "100.64.0.2", Some("bob"), &[]),
            machine("3", "server", "100.64.0.3", None, &["tag:server"]),
        ]
    }

    fn policy() -> Policy {
        Policy::parse(
            r#"{
                "acls": [
                    { "action": "accept", "src": ["group:admins"], "dst": ["tag:server:22,80"] },
                    { "action": "accept", "src": ["alice@"], "dst": ["*:*"] },
                    { "action": "accept", "src": ["100.64.0.2"], "dst": ["git:443"] }
                ],
                "ssh": [
                    { "action": "accept", "src": ["group:admins"], "dst": ["tag:server"], "users": ["root"] }
                ],
                "hosts": { "git": "100.64.0.3" },
                "groups": { "group:admins": ["alice@"] },
                "tagOwners": { "tag:server": ["group:admins"] }
            }"#,
        )
        .unwrap()
    }

    fn query(src: &str, dst: &str, port: u16) -> Query {
        Query {
            src: src.into(),
            dst: dst.into(),
            port,
            protocol: "tcp".into(),
        }
    }

    #[test]
    fn a_group_member_reaches_the_tagged_server_on_an_allowed_port() {
        let report = evaluate(&policy(), &tailnet(), &query("alice@", "tag:server:22", 22));

        assert!(report.allowed);
        let decision = report.decision.unwrap();
        assert_eq!(decision.index, 0);
        assert_eq!(decision.matched_src, "group:admins");
        assert_eq!(decision.matched_dst, "tag:server:22,80");
        assert_eq!(report.source_machines.len(), 1);
        assert_eq!(report.destination_machines[0].name, "server");
    }

    /// A later rule must not rescue traffic the first matching rule would allow
    /// on a different port.
    #[test]
    fn ports_outside_the_rule_fall_through_to_the_next_match() {
        let report = evaluate(&policy(), &tailnet(), &query("alice@", "tag:server:9999", 9999));

        // Rule 0 allows only 22 and 80; rule 1 is `alice@ -> *:*`.
        assert!(report.allowed);
        assert_eq!(report.decision.unwrap().index, 1);
        assert!(!report.rules[0].matched);
    }

    #[test]
    fn an_unmatched_source_is_denied_with_an_explanation() {
        let report = evaluate(&policy(), &tailnet(), &query("bob@", "tag:server:22", 22));

        // Bob is not in group:admins, so only `alice@` and the address rule
        // could apply, and neither names him.
        assert!(!report.allowed);
        assert!(report.decision.is_none());
        assert!(report.notes.iter().any(|note| note.contains("denied")));
    }

    #[test]
    fn a_host_alias_resolves_to_the_machines_it_covers() {
        let report = evaluate(&policy(), &tailnet(), &query("100.64.0.2", "git:443", 443));

        assert!(report.allowed);
        assert_eq!(report.decision.unwrap().matched_dst, "git:443");
        assert_eq!(report.destination_machines[0].name, "server");
    }

    #[test]
    fn a_cidr_source_selects_every_machine_inside_it() {
        // .0–.3 covers all three machines in the fixture.
        let report = evaluate(&policy(), &tailnet(), &query("100.64.0.0/30", "tag:server:80", 80));

        assert!(report.allowed);
        assert_eq!(report.source_machines.len(), 3);
    }

    #[test]
    fn ssh_rules_are_reported_separately_on_port_22() {
        let report = evaluate(&policy(), &tailnet(), &query("alice@", "tag:server:22", 22));

        assert_eq!(report.ssh_rules.len(), 1);
        assert!(report.ssh_rules[0].matched);
        assert_eq!(report.ssh_rules[0].action, "accept");
    }

    #[test]
    fn ssh_rules_are_not_evaluated_on_other_ports() {
        let report = evaluate(&policy(), &tailnet(), &query("alice@", "tag:server:80", 80));
        assert!(report.ssh_rules.is_empty());
    }

    #[test]
    fn an_unknown_selector_says_so_rather_than_denying_silently() {
        let report = evaluate(&policy(), &tailnet(), &query("nobody@", "tag:server:22", 22));

        assert!(!report.allowed);
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("does not select any machine")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn the_protocol_filter_narrows_a_rule() {
        let policy = Policy::parse(
            r#"{"acls":[{"action":"accept","src":["*"],"dst":["*:*"],"proto":"udp"}]}"#,
        )
        .unwrap();

        let tcp = evaluate(&policy, &tailnet(), &query("*", "*:*", 53));
        assert!(!tcp.allowed);

        let mut udp = query("*", "*:*", 53);
        udp.protocol = "udp".into();
        assert!(evaluate(&policy, &tailnet(), &udp).allowed);
    }

    #[test]
    fn destinations_split_around_ipv6_colons() {
        assert_eq!(split_destination("100.64.0.1:*").0, "100.64.0.1");
        assert_eq!(split_destination("100.64.0.1").0, "100.64.0.1");
        assert_eq!(split_destination("tag:server:22,80").0, "tag:server");

        // An address is never split, so an IPv6 target needs brackets.
        assert_eq!(split_destination("fd7a:115c:a1e0::1").0, "fd7a:115c:a1e0::1");
        assert_eq!(split_destination("fd7a:115c:a1e0::1").1, Ports::ANY);
        assert_eq!(split_destination("[fd7a:115c:a1e0::1]:22").0, "fd7a:115c:a1e0::1");
        assert!(split_destination("[fd7a:115c:a1e0::1]:22").1.matches(22));
    }

    #[test]
    fn port_specifications_cover_ranges_lists_and_wildcards() {
        assert!(Ports::parse("*").matches(1));
        assert!(Ports::parse("22").matches(22));
        assert!(!Ports::parse("22").matches(23));
        assert!(Ports::parse("80,443").matches(443));
        assert!(Ports::parse("1000-2000").matches(1500));
        assert!(!Ports::parse("1000-2000").matches(2001));
        assert_eq!(Ports::parse("*"), Ports::ANY);
    }

    #[test]
    fn selector_kinds_are_labelled_for_the_ui() {
        assert_eq!(selector_kind("*"), "any");
        assert_eq!(selector_kind("tag:server"), "tag");
        assert_eq!(selector_kind("group:admins"), "group");
        assert_eq!(selector_kind("alice@"), "user");
        assert_eq!(selector_kind("100.64.0.1/24"), "address");
        assert_eq!(selector_kind("git"), "host");
    }

    #[test]
    fn ports_are_reported_as_written() {
        assert_eq!(destination_ports("tag:server:22,80,443"), "22,80,443");
        assert_eq!(destination_ports("*:*"), "*");
        assert_eq!(destination_ports("tag:server"), "*");
        assert_eq!(destination_ports("100.64.0.1"), "*");
        assert_eq!(destination_ports("[fd7a::1]:22"), "22");
    }

    #[test]
    fn address_matching_handles_cidrs() {
        assert!(address_in("100.64.0.0/10", "100.64.0.3"));
        assert!(!address_in("100.64.0.0/10", "10.0.0.1"));
        assert!(address_in("100.64.0.3", "100.64.0.3"));
        assert!(address_in("fd7a::/16", "fd7a:115c::1"));
        assert!(!address_in("fd7a::/16", "100.64.0.1"));
    }
}
