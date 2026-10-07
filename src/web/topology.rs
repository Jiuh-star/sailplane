//! The shape of the tailnet: policy edges, route hubs, exit nodes and relays.
//!
//! One graph with five kinds of node. Policy selectors sit on the left and
//! right. Machines that carry infrastructure (subnet routes, an exit route, a
//! relay) sit next to them. The CIDRs, the internet exit and the relay regions
//! they give sit farthest right. Every edge names its two node ids, so the
//! client lays the graph out without re-deriving links. The access-check
//! evaluator decides which selector reaches what, so the graph shows what the
//! data plane will do, not just what the policy text says.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::acl::eval::{self, MachineRef};
use crate::acl::{Policy as AclPolicy, Section};
use crate::auth::Capability;
use crate::headscale::Machine;

use super::error::{ApiError, ApiResult};
use super::state::{Auth, PrincipalExt, SharedState};

/// The single node that stands for the public internet behind an exit node.
const EXIT_ID: &str = "hub:exit";

/// A selector an edge names, with what it resolves to.
struct Identity {
    kind: &'static str,
    machines: usize,
    /// A few names, so the node is recognizable without expanding it.
    sample: Vec<String>,
}

/// One node in the graph.
///
/// `kind` is `selector`, `machine`, `cidr`, `exit` or `region`. `role` splits
/// the two selector columns. Only the fields that fit a kind are serialized.
#[derive(serde::Serialize, Default)]
struct Node {
    id: String,
    kind: &'static str,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<&'static str>,
    /// For selectors: what the selector names (`tag`, `group`, `user`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    selector_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    machines: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sample: Vec<String>,
    /// `nodeAttrs` attributes applied to the selector.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attrs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    online: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approved: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_node: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sole: Option<bool>,
}

/// One edge between two nodes.
#[derive(serde::Serialize, Debug)]
struct Edge {
    id: String,
    source: String,
    target: String,
    /// `acl`, `grant`, `cap`, `ssh`, `route`, `exit`, `relay` or `approval`.
    kind: &'static str,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    users: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    src_machines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dst_machines: Option<usize>,
}

impl Edge {
    fn new(source: String, target: String, kind: &'static str, label: String) -> Self {
        Self {
            id: String::new(),
            source,
            target,
            kind,
            label,
            rule: None,
            action: None,
            users: Vec::new(),
            src_machines: None,
            dst_machines: None,
        }
    }

    fn with_rule(mut self, rule: usize) -> Self {
        self.rule = Some(rule);
        self
    }

    fn with_action(mut self, action: &str) -> Self {
        self.action = Some(action.to_string());
        self
    }

    fn with_users(mut self, users: Vec<String>) -> Self {
        self.users = users;
        self
    }

    fn with_counts(mut self, src: usize, dst: usize) -> Self {
        self.src_machines = Some(src);
        self.dst_machines = Some(dst);
        self
    }
}

/// Accumulates nodes and edges, deduplicating both.
#[derive(Default)]
struct Graph {
    nodes: BTreeMap<String, Node>,
    edges: Vec<Edge>,
    seen: BTreeSet<String>,
}

impl Graph {
    fn add_node(&mut self, node: Node) {
        self.nodes.entry(node.id.clone()).or_insert(node);
    }

    /// Adds an edge unless an identical one exists. A dual-stack exit node and
    /// overlapping rules both reach here with the same endpoints.
    fn add_edge(&mut self, mut edge: Edge) {
        let key = format!(
            "{}|{}|{}|{}",
            edge.source, edge.target, edge.kind, edge.label
        );
        if !self.seen.insert(key) {
            return;
        }
        edge.id = format!("e{}", self.edges.len());
        self.edges.push(edge);
    }
}

/// A CIDR some machine advertises, and which machines advertise it.
#[derive(serde::Serialize)]
struct Route {
    cidr: String,
    approved: bool,
    advertisers: Vec<RouteAdvertiser>,
    /// True when one machine is the only advertiser of that CIDR.
    sole: bool,
    /// True for `0.0.0.0/0` and `::/0`.
    exit_node: bool,
}

#[derive(serde::Serialize)]
struct RouteAdvertiser {
    id: String,
    name: String,
    online: bool,
}

/// Returns the policy graph, routes and relays. `GET /api/topology`
pub async fn get(State(state): State<SharedState>, Auth(principal): Auth) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ReadMachines])?;

    let client = state
        .admin_client()
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    let (text, _, _) = super::acl::stored_policy(&client).await?;
    let policy = AclPolicy::parse(&text).map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    let nodes = state.live.nodes().await;
    let machines: Vec<MachineRef> = nodes.data.iter().map(MachineRef::from_machine).collect();
    let ctx = state.eval_context().await;
    let host_info = state.agent.host_info().await.unwrap_or_default();

    let (graph_nodes, graph_edges) = build_graph(&policy, &machines, &nodes.data, &host_info, &ctx);

    Ok(Json(json!({
        "nodes": graph_nodes,
        "edges": graph_edges,
        "routes": routes(&nodes.data),
        "relays": relays(&host_info, &nodes.data),
        "policy": policy_extras(&policy),
        "totals": {
            "machines": machines.len(),
            "online": nodes.data.iter().filter(|node| node.online).count(),
            "rules": policy.acls.len() + policy.grant_rules().len() + policy.ssh.len(),
        },
    })))
}

/// Builds the graph from the stored policy, the live machines and the agent's
/// host info. Pure, so it can be tested against a policy on its own.
fn build_graph(
    policy: &AclPolicy,
    machines: &[MachineRef],
    hosts: &[Machine],
    host_info: &HashMap<String, Value>,
    ctx: &eval::EvalContext,
) -> (Vec<Node>, Vec<Edge>) {
    let mut identities: BTreeMap<String, Identity> = BTreeMap::new();
    let mut source_selectors: BTreeSet<String> = BTreeSet::new();
    let mut destination_selectors: BTreeSet<String> = BTreeSet::new();
    let mut graph = Graph::default();

    for (index, rule) in policy.acls.iter().enumerate() {
        for src in &rule.src {
            for dst in &rule.dst {
                let (host, _) = eval::split_destination(dst);
                let source = describe(policy, machines, src, ctx, &mut identities);
                let target = describe(policy, machines, &host, ctx, &mut identities);
                source_selectors.insert(src.trim().to_string());
                destination_selectors.insert(host.trim().to_string());
                graph.add_edge(
                    Edge::new(
                        selector_id("source", src),
                        selector_id("destination", &host),
                        "acl",
                        eval::destination_ports(dst),
                    )
                    .with_rule(index)
                    .with_action(&rule.action)
                    .with_counts(source, target),
                );
            }
        }
    }

    // `ip` is a network edge, `app` a capability edge. A grant can carry either
    // or both, so the two are emitted independently.
    for (index, rule) in policy.grant_rules().iter().enumerate() {
        for src in &rule.src {
            for dst in &rule.dst {
                let (host, _) = eval::split_destination(dst);
                let source = describe(policy, machines, src, ctx, &mut identities);
                let target = describe(policy, machines, &host, ctx, &mut identities);
                source_selectors.insert(src.trim().to_string());
                destination_selectors.insert(host.trim().to_string());
                let from = selector_id("source", src);
                let to = selector_id("destination", &host);

                if !rule.ip.is_empty() {
                    graph.add_edge(
                        Edge::new(from.clone(), to.clone(), "grant", rule.ip.join(", "))
                            .with_rule(index)
                            .with_counts(source, target),
                    );
                }
                if rule.has_app() {
                    let capabilities = rule.capabilities();
                    let label = if capabilities.is_empty() {
                        "app".to_string()
                    } else {
                        capabilities.join(", ")
                    };
                    graph.add_edge(
                        Edge::new(from.clone(), to.clone(), "cap", label)
                            .with_rule(index)
                            .with_counts(source, target),
                    );
                }
                if rule.ip.is_empty() && !rule.has_app() {
                    graph.add_edge(
                        Edge::new(from, to, "grant", "*".into())
                            .with_rule(index)
                            .with_counts(source, target),
                    );
                }
            }
        }
    }

    for (index, rule) in policy.ssh.iter().enumerate() {
        for src in &rule.src {
            for dst in &rule.dst {
                let (host, _) = eval::split_destination(dst);
                let source = describe(policy, machines, src, ctx, &mut identities);
                let target = describe(policy, machines, &host, ctx, &mut identities);
                source_selectors.insert(src.trim().to_string());
                destination_selectors.insert(host.trim().to_string());
                graph.add_edge(
                    Edge::new(
                        selector_id("source", src),
                        selector_id("destination", &host),
                        "ssh",
                        "22".into(),
                    )
                    .with_rule(index)
                    .with_action(&rule.action)
                    .with_users(rule.users.clone())
                    .with_counts(source, target),
                );
            }
        }
    }

    let by_id: BTreeMap<&str, &Machine> =
        hosts.iter().map(|node| (node.id.as_str(), node)).collect();

    // Subnet routes: one hub per CIDR, an edge from each machine that serves it.
    for route in routes(hosts) {
        if route.exit_node {
            continue;
        }
        graph.add_node(cidr_node(&route.cidr, route.approved, route.sole));
        for advertiser in &route.advertisers {
            if let Some(machine) = by_id.get(advertiser.id.as_str()) {
                graph.add_node(machine_node(machine));
            }
            graph.add_edge(Edge::new(
                format!("machine:{}", advertiser.id),
                cidr_id(&route.cidr),
                "route",
                if route.approved { "approved" } else { "pending" }.into(),
            ));
        }
    }

    // Exit nodes collapse to one internet hub. Only a machine that still
    // advertises an exit route is an exit node (see `exit_route_state`).
    for machine in hosts {
        if !machine
            .available_routes
            .iter()
            .any(|route| Machine::is_exit_route(route))
        {
            continue;
        }
        let approved = machine
            .approved_routes
            .iter()
            .any(|route| Machine::is_exit_route(route));
        graph.add_node(machine_node(machine));
        graph.add_node(exit_node());
        graph.add_edge(Edge::new(
            format!("machine:{}", machine.id),
            EXIT_ID.into(),
            "exit",
            if approved { "approved" } else { "pending" }.into(),
        ));
    }

    // Relays: a region hub with an edge to each machine that homes there.
    for machine in hosts {
        let Some(region) = region_of(host_info.get(&machine.node_key)) else {
            continue;
        };
        graph.add_node(machine_node(machine));
        graph.add_node(region_node(&region));
        graph.add_edge(Edge::new(
            region_id(&region),
            format!("machine:{}", machine.id),
            "relay",
            String::new(),
        ));
    }

    // `autoApprovers` becomes an approval edge from the approver selector to the
    // route or exit hub it can approve.
    if let Some(approvers) = policy.auto_approvers.as_ref().and_then(Section::typed) {
        for (cidr, selectors) in &approvers.routes {
            graph.add_node(cidr_node(cidr, true, false));
            for selector in selectors {
                describe(policy, machines, selector, ctx, &mut identities);
                source_selectors.insert(selector.trim().to_string());
                graph.add_edge(Edge::new(
                    selector_id("source", selector),
                    cidr_id(cidr),
                    "approval",
                    String::new(),
                ));
            }
        }
        for selector in &approvers.exit_node {
            describe(policy, machines, selector, ctx, &mut identities);
            source_selectors.insert(selector.trim().to_string());
            graph.add_node(exit_node());
            graph.add_edge(Edge::new(
                selector_id("source", selector),
                EXIT_ID.into(),
                "approval",
                String::new(),
            ));
        }
    }

    // `nodeAttrs` are attributes on the nodes a selector names, so they ride on
    // the selector node.
    let mut attrs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(list) = policy.node_attrs.as_ref().and_then(Section::typed) {
        for entry in list {
            for target in &entry.target {
                describe(policy, machines, target, ctx, &mut identities);
                source_selectors.insert(target.trim().to_string());
                let slot = attrs.entry(target.trim().to_string()).or_default();
                for attr in &entry.attr {
                    if !slot.contains(attr) {
                        slot.push(attr.clone());
                    }
                }
            }
        }
    }

    for (selector, identity) in &identities {
        let attrs = attrs.get(selector).map(Vec::as_slice).unwrap_or(&[]);
        let mut placed = false;
        if source_selectors.contains(selector) {
            graph.add_node(selector_node("source", selector, identity, attrs));
            placed = true;
        }
        if destination_selectors.contains(selector) {
            graph.add_node(selector_node("destination", selector, identity, attrs));
            placed = true;
        }
        if !placed {
            graph.add_node(selector_node("source", selector, identity, attrs));
        }
    }

    (graph.nodes.into_values().collect(), graph.edges)
}

/// The node id of a selector in one of the two selector columns.
fn selector_id(role: &str, selector: &str) -> String {
    format!("{role}:{}", selector.trim())
}

fn cidr_id(cidr: &str) -> String {
    format!("hub:cidr:{cidr}")
}

fn region_id(region: &str) -> String {
    format!("hub:region:{region}")
}

fn selector_node(role: &'static str, selector: &str, identity: &Identity, attrs: &[String]) -> Node {
    Node {
        id: selector_id(role, selector),
        kind: "selector",
        label: selector.trim().to_string(),
        role: Some(role),
        selector_kind: Some(identity.kind),
        machines: Some(identity.machines),
        sample: identity.sample.clone(),
        attrs: attrs.to_vec(),
        ..Default::default()
    }
}

fn machine_node(machine: &Machine) -> Node {
    Node {
        id: format!("machine:{}", machine.id),
        kind: "machine",
        label: if machine.given_name.is_empty() {
            machine.name.clone()
        } else {
            machine.given_name.clone()
        },
        online: Some(machine.online),
        ..Default::default()
    }
}

fn cidr_node(cidr: &str, approved: bool, sole: bool) -> Node {
    Node {
        id: cidr_id(cidr),
        kind: "cidr",
        label: cidr.to_string(),
        approved: Some(approved),
        exit_node: Some(Machine::is_exit_route(cidr)),
        sole: Some(sole),
        ..Default::default()
    }
}

fn exit_node() -> Node {
    Node {
        id: EXIT_ID.into(),
        kind: "exit",
        label: "Internet".into(),
        exit_node: Some(true),
        ..Default::default()
    }
}

fn region_node(region: &str) -> Node {
    Node {
        id: region_id(region),
        kind: "region",
        label: region.to_string(),
        ..Default::default()
    }
}

/// The relay region a machine homes to, accepting both the numeric id the
/// netmap reports and the string code the status projection reports.
fn region_of(info: Option<&Value>) -> Option<String> {
    match info?.get("HomeDERP")? {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

/// The policy keys that do not form edges, passed through for the UI.
fn policy_extras(policy: &AclPolicy) -> Value {
    json!({
        "autoApprovers": policy.auto_approvers,
        "nodeAttrs": policy.node_attrs,
        "postures": policy.postures,
        "tests": policy.tests,
        "sshTests": policy.ssh_tests,
        "randomizeClientPort": policy.randomize_client_port,
    })
}

/// Records an identity and returns how many machines it covers.
fn describe(
    policy: &AclPolicy,
    machines: &[MachineRef],
    selector: &str,
    ctx: &eval::EvalContext,
    identities: &mut BTreeMap<String, Identity>,
) -> usize {
    let selector = selector.trim();
    let mut notes = Vec::new();
    let matched = eval::expand(policy, machines, selector, &mut notes, ctx, &[]);

    identities
        .entry(selector.to_string())
        .or_insert_with(|| Identity {
            kind: eval::selector_kind(selector),
            machines: matched.len(),
            sample: matched
                .iter()
                .take(3)
                .map(|machine| machine.name.clone())
                .collect(),
        });

    matched.len()
}

/// Every route a machine advertises, grouped by CIDR.
fn routes(nodes: &[Machine]) -> Vec<Route> {
    let mut by_cidr: BTreeMap<String, (bool, Vec<RouteAdvertiser>)> = BTreeMap::new();

    for node in nodes {
        let approved: BTreeSet<&String> = node.approved_routes.iter().collect();
        for cidr in node
            .available_routes
            .iter()
            .chain(node.approved_routes.iter())
            .collect::<BTreeSet<_>>()
        {
            let entry = by_cidr
                .entry(cidr.clone())
                .or_insert_with(|| (approved.contains(cidr), Vec::new()));
            entry.1.push(RouteAdvertiser {
                id: node.id.clone(),
                name: node.given_name.clone(),
                online: node.online,
            });
        }
    }

    by_cidr
        .into_iter()
        .map(|(cidr, (approved, advertisers))| Route {
            sole: advertisers.len() == 1,
            exit_node: Machine::is_exit_route(&cidr),
            cidr,
            approved,
            advertisers,
        })
        .collect()
}

/// Machines grouped by the relay region they home to.
fn relays(host_info: &HashMap<String, Value>, nodes: &[Machine]) -> Vec<Value> {
    let mut by_region: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for node in nodes {
        let region = region_of(host_info.get(&node.node_key)).unwrap_or_else(|| "unknown".into());
        by_region.entry(region).or_default().push(json!({
            "id": node.id,
            "name": node.given_name,
            "online": node.online,
        }));
    }

    by_region
        .into_iter()
        .map(|(region, machines)| json!({ "region": region, "machines": machines }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_context() -> eval::EvalContext {
        eval::EvalContext::default()
    }

    fn machine(json: &str) -> Machine {
        serde_json::from_str(json).expect("valid machine")
    }

    /// The reported policy: everything expressed through `grants`, with an
    /// app-capability grant and `nodeAttrs`. It used to draw an empty graph
    /// because the map-shaped `app` made the whole `grants` section fall back
    /// to `Raw`.
    #[test]
    fn a_grant_policy_draws_edges() {
        let policy = AclPolicy::parse(
            r#"{
                "acls": [], "ssh": [], "hosts": {}, "groups": {}, "tagOwners": {},
                "grants": [
                    { "src": ["*"], "dst": ["*"], "ip": ["*"] },
                    { "src": ["*"], "dst": ["*"], "app": { "tailscale.com/cap/drive": [ { "shares": ["*"], "access": "rw" } ] } }
                ],
                "nodeAttrs": [
                    { "target": ["autogroup:member"], "attr": ["drive:share", "drive:access"] }
                ]
            }"#,
        )
        .unwrap();

        let (nodes, edges) = build_graph(
            &policy,
            &[],
            &[],
            &HashMap::new(),
            &empty_context(),
        );

        assert!(
            edges.iter().any(|edge| edge.kind == "grant"),
            "an `ip` grant becomes a network edge: {edges:?}"
        );
        let cap = edges
            .iter()
            .find(|edge| edge.kind == "cap")
            .expect("an `app` grant becomes a capability edge");
        assert!(cap.label.contains("tailscale.com/cap/drive"), "{cap:?}");

        let member = nodes
            .iter()
            .find(|node| node.id == "source:autogroup:member")
            .expect("the nodeAttrs target is a node");
        assert_eq!(member.attrs, vec!["drive:share", "drive:access"]);
    }

    /// A subnet router and an exit node become hubs with edges from the machine.
    #[test]
    fn routes_and_exit_nodes_become_hubs() {
        let policy = AclPolicy::parse("{}").unwrap();
        let hosts = vec![
            machine(
                r#"{
                    "id": "1", "givenName": "router",
                    "availableRoutes": ["10.20.0.0/16"], "approvedRoutes": ["10.20.0.0/16"]
                }"#,
            ),
            machine(
                r#"{
                    "id": "2", "givenName": "gateway",
                    "availableRoutes": ["0.0.0.0/0"], "approvedRoutes": ["0.0.0.0/0"]
                }"#,
            ),
        ];

        let (nodes, edges) = build_graph(
            &policy,
            &[],
            &hosts,
            &HashMap::new(),
            &empty_context(),
        );

        assert!(nodes.iter().any(|node| node.id == "hub:cidr:10.20.0.0/16"));
        assert!(nodes.iter().any(|node| node.id == EXIT_ID));
        assert!(edges
            .iter()
            .any(|edge| edge.kind == "route" && edge.source == "machine:1"));
        assert!(edges
            .iter()
            .any(|edge| edge.kind == "exit" && edge.source == "machine:2"));
    }

    /// A relay region comes from the agent's host info, whether the region is
    /// the numeric netmap id or the string status code.
    #[test]
    fn relays_use_numeric_and_string_regions() {
        let policy = AclPolicy::parse("{}").unwrap();
        let hosts = vec![
            machine(r#"{"id": "1", "givenName": "a", "nodeKey": "k1"}"#),
            machine(r#"{"id": "2", "givenName": "b", "nodeKey": "k2"}"#),
        ];
        let mut host_info = HashMap::new();
        host_info.insert("k1".to_string(), json!({ "HomeDERP": 7 }));
        host_info.insert("k2".to_string(), json!({ "HomeDERP": "sin" }));

        let (nodes, edges) = build_graph(&policy, &[], &hosts, &host_info, &empty_context());

        assert!(nodes.iter().any(|node| node.id == "hub:region:7"));
        assert!(nodes.iter().any(|node| node.id == "hub:region:sin"));
        assert_eq!(edges.iter().filter(|edge| edge.kind == "relay").count(), 2);
    }
}
