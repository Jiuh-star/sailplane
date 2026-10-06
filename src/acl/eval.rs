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
    /// True when the machine is an approved exit node. `autogroup:internet`
    /// resolves to these.
    pub exit_node: bool,
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
            exit_node: machine
                .approved_routes
                .iter()
                .any(|route| Machine::is_exit_route(route)),
        }
    }
}

/// Facts a selector cannot derive from the machine list alone.
///
/// `autogroup:admin` and `autogroup:self` depend on who is asking. Headscale has
/// no admin user concept of its own, so `admins` is the set of Sailplane
/// owner/admin accounts that are linked to a Headscale user.
#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    pub admins: Vec<String>,
}

/// One rule's verdict, for the per-rule breakdown the UI shows.
#[derive(Debug, Clone, Serialize)]
pub struct RuleOutcome {
    pub index: usize,
    /// `acl`, `grant` or `ssh`. Index alone is not unique across sections.
    pub kind: &'static str,
    pub action: String,
    pub src: Vec<String>,
    pub dst: Vec<String>,
    /// The `ip` list of a grant rule, empty for other kinds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ip: Vec<String>,
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
            kind: "acl",
            action: rule.action.clone(),
            src: rule.src.clone(),
            dst: rule.dst.clone(),
            ip: Vec::new(),
            matched_src: None,
            matched_dst: None,
            matched: false,
        }
    }

    fn from_ssh(index: usize, rule: &SshRule) -> Self {
        Self {
            index,
            kind: "ssh",
            action: rule.action.clone(),
            src: rule.src.clone(),
            dst: rule.dst.clone(),
            ip: Vec::new(),
            matched_src: None,
            matched_dst: None,
            matched: false,
        }
    }

    fn from_grant(index: usize, rule: &crate::acl::GrantRule) -> Self {
        Self {
            index,
            kind: "grant",
            action: "accept".into(),
            src: rule.src.clone(),
            dst: rule.dst.clone(),
            ip: rule.ip.clone(),
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
    /// `acl` or `grant`.
    pub kind: &'static str,
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

/// Evaluates `query` with an [`EvalContext`] for the dynamic autogroups.
pub fn evaluate_with(
    policy: &Policy,
    machines: &[MachineRef],
    query: &Query,
    ctx: &EvalContext,
) -> Report {
    let mut notes = Vec::new();
    let protocol = query.protocol.to_ascii_lowercase();

    let source_machines = expand(policy, machines, query.src.trim(), &mut notes, ctx, &[]);
    let (host, ports) = split_destination(query.dst.trim());
    // `autogroup:self` on the destination means "the requester's own machines",
    // which are the source machines.
    let destination_machines = expand(policy, machines, &host, &mut notes, ctx, &source_machines);

    if !ports.matches(query.port) {
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

        let Some(src_selector) = matching_selector(policy, &source_machines, &rule.src, ctx) else {
            rules.push(outcome);
            continue;
        };

        let Some((dst_selector, rule_ports)) = matching_destination(
            policy,
            &destination_machines,
            &rule.dst,
            ctx,
            &source_machines,
        ) else {
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
                kind: "acl",
                action: rule.action.clone(),
                src: rule.src.clone(),
                dst: rule.dst.clone(),
                matched_src: src_selector,
                matched_dst: dst_selector,
            });
        }
    }

    // Grants are evaluated after acls; the first match across both sections
    // decides, matching Headscale's order.
    for (index, rule) in policy.grant_rules().iter().enumerate() {
        let mut outcome = RuleOutcome::from_grant(index, rule);

        let Some(src_selector) = matching_selector(policy, &source_machines, &rule.src, ctx) else {
            rules.push(outcome);
            continue;
        };

        let Some((dst_selector, _)) = matching_destination(
            policy,
            &destination_machines,
            &rule.dst,
            ctx,
            &source_machines,
        ) else {
            rules.push(outcome);
            continue;
        };

        if rule.ip.is_empty() {
            if rule.app.is_empty() {
                note_once(
                    &mut notes,
                    "a grant has neither `ip` nor `app`, so it is not evaluated",
                );
                rules.push(outcome);
                continue;
            }
            note_once(
                &mut notes,
                "grant `app` capabilities are not evaluated by this check",
            );
        } else if !rule
            .ip
            .iter()
            .any(|spec| ip_spec_matches(spec, &protocol, query.port))
        {
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
                kind: "grant",
                action: "accept".into(),
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
                matching_selector(policy, &source_machines, &rule.src, ctx),
                matching_destination(
                    policy,
                    &destination_machines,
                    &rule.dst,
                    ctx,
                    &source_machines,
                ),
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
        notes.push("no ACL rule matched, which is the default: traffic is denied".to_string());
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
///
/// `self_ref` is the source machine set, used to resolve `autogroup:self`.
pub fn expand(
    policy: &Policy,
    machines: &[MachineRef],
    selector: &str,
    notes: &mut Vec<String>,
    ctx: &EvalContext,
    self_ref: &[MachineRef],
) -> Vec<MachineRef> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Vec::new();
    }

    match selector {
        "autogroup:internet" => {
            let nodes = by_selector(machines, policy, selector, ctx, self_ref);
            if nodes.is_empty() {
                note_once(
                    notes,
                    "`autogroup:internet` names exit nodes; none is approved in this tailnet",
                );
            }
            return nodes;
        }
        "autogroup:member" => return by_selector(machines, policy, selector, ctx, self_ref),
        "autogroup:admin" => {
            let nodes = by_selector(machines, policy, selector, ctx, self_ref);
            if nodes.is_empty() {
                note_once(
                    notes,
                    "`autogroup:admin` matches Sailplane owner/admin accounts; none is linked \
                     to a Headscale user here",
                );
            }
            return nodes;
        }
        "autogroup:self" => {
            if self_ref.is_empty() {
                note_once(
                    notes,
                    "`autogroup:self` depends on the requesting user and cannot be resolved \
                     for this source",
                );
                return Vec::new();
            }
            return by_selector(machines, policy, selector, ctx, self_ref);
        }
        _ if selector.starts_with("autogroup:") => {
            note_once(
                notes,
                &format!("`{selector}` is not evaluated by this check"),
            );
            return Vec::new();
        }
        _ => {}
    }

    by_selector(machines, policy, selector, ctx, self_ref)
}

/// The machines named by `selector`, including the dynamic autogroups.
fn by_selector(
    machines: &[MachineRef],
    policy: &Policy,
    selector: &str,
    ctx: &EvalContext,
    self_ref: &[MachineRef],
) -> Vec<MachineRef> {
    machines
        .iter()
        .filter(|machine| matches_selector(policy, machine, selector, ctx, self_ref))
        .cloned()
        .collect()
}

/// Whether a machine is named by a selector, including dynamic autogroups.
fn matches_selector(
    policy: &Policy,
    machine: &MachineRef,
    selector: &str,
    ctx: &EvalContext,
    self_ref: &[MachineRef],
) -> bool {
    let selector = selector.trim();

    match selector {
        "*" => return true,
        "autogroup:internet" => return machine.exit_node,
        "autogroup:member" => return machine.user.is_some(),
        "autogroup:admin" => return is_admin_machine(machine, ctx),
        "autogroup:self" => return self_matches(machine, self_ref),
        _ if selector.starts_with("autogroup:") => return false,
        _ => {}
    }

    if let Some(tag) = selector.strip_prefix("tag:") {
        let tag = format!("tag:{tag}");
        return machine.tags.iter().any(|owned| owned == &tag);
    }
    if let Some(group) = selector.strip_prefix("group:") {
        let name = format!("group:{group}");
        return policy.groups.get(&name).is_some_and(|members| {
            members
                .iter()
                .any(|member| matches_selector(policy, machine, member, ctx, self_ref))
        });
    }
    if let Some(alias) = policy.hosts.get(selector) {
        return machine
            .addresses
            .iter()
            .any(|address| address_in(alias, address));
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

fn is_admin_machine(machine: &MachineRef, ctx: &EvalContext) -> bool {
    machine
        .user
        .as_deref()
        .is_some_and(|user| ctx.admins.iter().any(|admin| admin == user))
}

/// `autogroup:self`: the machine itself, or another owned by the same user.
fn self_matches(machine: &MachineRef, self_ref: &[MachineRef]) -> bool {
    self_ref.iter().any(|source| {
        source.id == machine.id || (source.user.is_some() && source.user == machine.user)
    })
}

/// The first selector in `selectors` that names one of `machines`.
fn matching_selector(
    policy: &Policy,
    machines: &[MachineRef],
    selectors: &[String],
    ctx: &EvalContext,
) -> Option<String> {
    for selector in selectors {
        if selector.trim().is_empty() {
            continue;
        }
        if machines
            .iter()
            .any(|machine| matches_selector(policy, machine, selector, ctx, &[]))
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
    ctx: &EvalContext,
    self_ref: &[MachineRef],
) -> Option<(String, Ports)> {
    for selector in selectors {
        if selector.trim().is_empty() {
            continue;
        }
        let (host, ports) = split_destination(selector);
        if machines
            .iter()
            .any(|machine| matches_selector(policy, machine, &host, ctx, self_ref))
        {
            return Some((selector.clone(), ports));
        }
    }
    None
}

fn note_once(notes: &mut Vec<String>, note: &str) {
    if !notes.iter().any(|existing| existing == note) {
        notes.push(note.to_string());
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

/// Whether a grant `ip` entry (`*`, `*:*`, `tcp:443`, `udp:1000-2000`) covers a
/// protocol and port.
fn ip_spec_matches(spec: &str, protocol: &str, port: u16) -> bool {
    let spec = spec.trim();
    if spec.is_empty() || spec == "*" || spec == "*:*" {
        return true;
    }
    match spec.split_once(':') {
        Some((proto, ports)) => {
            let proto = proto.trim();
            (proto == "*" || proto.eq_ignore_ascii_case(protocol))
                && Ports::parse(ports).matches(port)
        }
        None => spec.eq_ignore_ascii_case(protocol),
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
        self.0.is_empty()
            || self
                .0
                .iter()
                .any(|(start, end)| (*start..=*end).contains(&port))
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
            let (Ok(network), Ok(prefix)) = (network.parse::<IpAddr>(), prefix.parse::<u8>())
            else {
                return false;
            };
            in_prefix(network, address, prefix)
        }
        None => pattern
            .parse::<IpAddr>()
            .is_ok_and(|pattern| pattern == address),
    }
}

fn in_prefix(network: IpAddr, address: IpAddr, prefix: u8) -> bool {
    match (network, address) {
        (IpAddr::V4(network), IpAddr::V4(address)) if prefix <= 32 => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            u32::from(network) & mask == u32::from(address) & mask
        }
        (IpAddr::V6(network), IpAddr::V6(address)) if prefix <= 128 => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            u128::from(network) & mask == u128::from(address) & mask
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(
        id: &str,
        name: &str,
        address: &str,
        user: Option<&str>,
        tags: &[&str],
    ) -> MachineRef {
        MachineRef {
            id: id.into(),
            name: name.into(),
            addresses: vec![address.into()],
            user: user.map(str::to_string),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
            exit_node: false,
        }
    }

    /// An approved exit node, for `autogroup:internet` tests.
    fn exit_node(id: &str, name: &str, address: &str, user: Option<&str>) -> MachineRef {
        MachineRef {
            exit_node: true,
            ..machine(id, name, address, user, &[])
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

    /// Evaluates without dynamic-autogroup knowledge, for the common cases.
    fn eval(policy: &Policy, machines: &[MachineRef], query: &Query) -> Report {
        evaluate_with(policy, machines, query, &EvalContext::default())
    }

    #[test]
    fn a_group_member_reaches_the_tagged_server_on_an_allowed_port() {
        let report = eval(&policy(), &tailnet(), &query("alice@", "tag:server:22", 22));

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
        let report = eval(
            &policy(),
            &tailnet(),
            &query("alice@", "tag:server:9999", 9999),
        );

        // Rule 0 allows only 22 and 80; rule 1 is `alice@ -> *:*`.
        assert!(report.allowed);
        assert_eq!(report.decision.unwrap().index, 1);
        assert!(!report.rules[0].matched);
    }

    #[test]
    fn an_unmatched_source_is_denied_with_an_explanation() {
        let report = eval(&policy(), &tailnet(), &query("bob@", "tag:server:22", 22));

        // Bob is not in group:admins, so only `alice@` and the address rule
        // could apply, and neither names him.
        assert!(!report.allowed);
        assert!(report.decision.is_none());
        assert!(report.notes.iter().any(|note| note.contains("denied")));
    }

    #[test]
    fn a_host_alias_resolves_to_the_machines_it_covers() {
        let report = eval(&policy(), &tailnet(), &query("100.64.0.2", "git:443", 443));

        assert!(report.allowed);
        assert_eq!(report.decision.unwrap().matched_dst, "git:443");
        assert_eq!(report.destination_machines[0].name, "server");
    }

    #[test]
    fn a_cidr_source_selects_every_machine_inside_it() {
        // .0–.3 covers all three machines in the fixture.
        let report = eval(
            &policy(),
            &tailnet(),
            &query("100.64.0.0/30", "tag:server:80", 80),
        );

        assert!(report.allowed);
        assert_eq!(report.source_machines.len(), 3);
    }

    #[test]
    fn ssh_rules_are_reported_separately_on_port_22() {
        let report = eval(&policy(), &tailnet(), &query("alice@", "tag:server:22", 22));

        assert_eq!(report.ssh_rules.len(), 1);
        assert!(report.ssh_rules[0].matched);
        assert_eq!(report.ssh_rules[0].action, "accept");
    }

    #[test]
    fn ssh_rules_are_not_evaluated_on_other_ports() {
        let report = eval(&policy(), &tailnet(), &query("alice@", "tag:server:80", 80));
        assert!(report.ssh_rules.is_empty());
    }

    #[test]
    fn an_unknown_selector_says_so_rather_than_denying_silently() {
        let report = eval(
            &policy(),
            &tailnet(),
            &query("nobody@", "tag:server:22", 22),
        );

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

        let tcp = eval(&policy, &tailnet(), &query("*", "*:*", 53));
        assert!(!tcp.allowed);

        let mut udp = query("*", "*:*", 53);
        udp.protocol = "udp".into();
        assert!(eval(&policy, &tailnet(), &udp).allowed);
    }

    #[test]
    fn destinations_split_around_ipv6_colons() {
        assert_eq!(split_destination("100.64.0.1:*").0, "100.64.0.1");
        assert_eq!(split_destination("100.64.0.1").0, "100.64.0.1");
        assert_eq!(split_destination("tag:server:22,80").0, "tag:server");

        // An address is never split, so an IPv6 target needs brackets.
        assert_eq!(
            split_destination("fd7a:115c:a1e0::1").0,
            "fd7a:115c:a1e0::1"
        );
        assert_eq!(split_destination("fd7a:115c:a1e0::1").1, Ports::ANY);
        assert_eq!(
            split_destination("[fd7a:115c:a1e0::1]:22").0,
            "fd7a:115c:a1e0::1"
        );
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

    #[test]
    fn autogroup_internet_resolves_to_exit_nodes() {
        let mut machines = tailnet();
        machines.push(exit_node("9", "gateway", "100.64.0.9", Some("carol")));
        let policy = Policy::parse(
            r#"{"acls":[{"action":"accept","src":["alice@"],"dst":["autogroup:internet:*"]}]}"#,
        )
        .unwrap();

        let report = evaluate_with(
            &policy,
            &machines,
            &query("alice@", "autogroup:internet:443", 443),
            &EvalContext::default(),
        );

        assert!(report.allowed);
        assert_eq!(report.destination_machines.len(), 1);
        assert_eq!(report.destination_machines[0].name, "gateway");
    }

    #[test]
    fn autogroup_member_and_admin_filter_by_owner() {
        let policy = Policy::parse(
            r#"{"acls":[{"action":"accept","src":["autogroup:member"],"dst":["*:*"]}]}"#,
        )
        .unwrap();
        let member = evaluate_with(
            &policy,
            &tailnet(),
            &query("autogroup:member", "*:*", 22),
            &EvalContext::default(),
        );
        // Two of the three fixture machines are owned by a user; the tagged
        // server is not.
        assert_eq!(member.source_machines.len(), 2);

        let admin_policy = Policy::parse(
            r#"{"acls":[{"action":"accept","src":["autogroup:admin"],"dst":["*:*"]}]}"#,
        )
        .unwrap();
        let ctx = EvalContext {
            admins: vec!["bob".into()],
        };
        let admin = evaluate_with(
            &admin_policy,
            &tailnet(),
            &query("autogroup:admin", "*:*", 22),
            &ctx,
        );
        assert_eq!(admin.source_machines.len(), 1);
        assert_eq!(admin.source_machines[0].name, "bob-desktop");
    }

    #[test]
    fn autogroup_self_matches_the_sources_own_machines() {
        let mut machines = tailnet();
        // A second machine owned by alice.
        machines.push(machine(
            "4",
            "alice-phone",
            "100.64.0.4",
            Some("alice"),
            &[],
        ));
        let policy = Policy::parse(
            r#"{"acls":[{"action":"accept","src":["alice-laptop"],"dst":["autogroup:self"]}]}"#,
        )
        .unwrap();

        let report = evaluate_with(
            &policy,
            &machines,
            &query("alice-laptop", "autogroup:self", 22),
            &EvalContext::default(),
        );

        assert!(report.allowed, "{:?}", report.notes);
        assert_eq!(report.destination_machines.len(), 2);
    }

    #[test]
    fn grants_are_evaluated_with_their_ip_ports() {
        let policy = Policy::parse(
            r#"{"grants":[{"src":["alice@"],"dst":["tag:server"],"ip":["tcp:443"]}]}"#,
        )
        .unwrap();

        let allowed = eval(&policy, &tailnet(), &query("alice@", "tag:server", 443));
        assert!(allowed.allowed);
        assert_eq!(allowed.decision.as_ref().unwrap().kind, "grant");

        let denied = eval(&policy, &tailnet(), &query("alice@", "tag:server", 80));
        assert!(!denied.allowed);
    }

    #[test]
    fn grant_wildcard_ip_covers_any_port() {
        let policy = Policy::parse(r#"{"grants":[{"src":["*"],"dst":["*"],"ip":["*"]}]}"#).unwrap();
        let report = eval(&policy, &tailnet(), &query("*", "*:*", 12345));
        assert!(report.allowed);
    }
}
