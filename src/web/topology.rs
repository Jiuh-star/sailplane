//! The shape of the tailnet: policy edges, route hubs and relays.
//!
//! Shows how machines are wired: which identities an ACL rule connects, which
//! CIDRs a machine serves and whether anyone else could serve them, and where
//! each machine's relay is. Relays come from the netmap, the only source of a
//! relay region.

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::acl::Policy as AclPolicy;
use crate::acl::eval::{self, MachineRef};
use crate::auth::Capability;

use super::error::{ApiError, ApiResult};
use super::state::{Auth, PrincipalExt, SharedState};

/// One identity a rule names, with what it resolves to.
#[derive(serde::Serialize)]
struct Identity {
    selector: String,
    kind: &'static str,
    machines: usize,
    /// A few names, so the card is recognisable without expanding it.
    sample: Vec<String>,
}

/// One rule, as an edge from a source identity to a destination identity.
#[derive(serde::Serialize)]
struct Edge {
    rule: usize,
    /// `acl`, `grant` or `ssh`.
    kind: &'static str,
    action: String,
    src: String,
    dst: String,
    /// Ports as written in the rule.
    ports: String,
    src_machines: usize,
    dst_machines: usize,
    /// The SSH login names a rule allows. Always present: a field that is
    /// sometimes absent is a field the client has to defend against, and the
    /// empty list says the same thing.
    users: Vec<String>,
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
pub async fn get(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ReadMachines])?;

    let client = state
        .admin_client()
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    let (text, _, _) = super::acl::stored_policy(&client).await?;
    let policy =
        AclPolicy::parse(&text).map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    let nodes = state.live.nodes().await;
    let machines: Vec<MachineRef> = nodes.data.iter().map(MachineRef::from_machine).collect();

    let mut identities: BTreeMap<String, Identity> = BTreeMap::new();
    let mut edges = Vec::new();
    let ctx = state.eval_context().await;

    for (index, rule) in policy.acls.iter().enumerate() {
        for src in &rule.src {
            for dst in &rule.dst {
                let (host, _) = eval::split_destination(dst);
                let source = describe(&policy, &machines, src, &ctx, &mut identities);
                let target = describe(&policy, &machines, &host, &ctx, &mut identities);
                edges.push(Edge {
                    rule: index,
                    kind: "acl",
                    action: rule.action.clone(),
                    src: src.clone(),
                    dst: host.clone(),
                    ports: eval::destination_ports(dst),
                    src_machines: source,
                    dst_machines: target,
                    users: Vec::new(),
                });
            }
        }
    }

    for (index, rule) in policy.grant_rules().iter().enumerate() {
        for src in &rule.src {
            for dst in &rule.dst {
                let (host, _) = eval::split_destination(dst);
                let source = describe(&policy, &machines, src, &ctx, &mut identities);
                let target = describe(&policy, &machines, &host, &ctx, &mut identities);
                edges.push(Edge {
                    rule: index,
                    kind: "grant",
                    action: "accept".into(),
                    src: src.clone(),
                    dst: host.clone(),
                    ports: if rule.ip.is_empty() {
                        "*".into()
                    } else {
                        rule.ip.join(", ")
                    },
                    src_machines: source,
                    dst_machines: target,
                    users: Vec::new(),
                });
            }
        }
    }

    for (index, rule) in policy.ssh.iter().enumerate() {
        for src in &rule.src {
            for dst in &rule.dst {
                let (host, _) = eval::split_destination(dst);
                let source = describe(&policy, &machines, src, &ctx, &mut identities);
                let target = describe(&policy, &machines, &host, &ctx, &mut identities);
                edges.push(Edge {
                    rule: index,
                    kind: "ssh",
                    action: rule.action.clone(),
                    src: src.clone(),
                    dst: host.clone(),
                    ports: "22".into(),
                    src_machines: source,
                    dst_machines: target,
                    users: rule.users.clone(),
                });
            }
        }
    }

    Ok(Json(json!({
        "identities": identities.into_values().collect::<Vec<_>>(),
        "edges": edges,
        "routes": routes(&nodes.data),
        "relays": relays(&state, &nodes.data).await,
        "policy": policy_extras(&policy),
        "totals": {
            "machines": machines.len(),
            "online": nodes.data.iter().filter(|node| node.online).count(),
            "rules": policy.acls.len() + policy.grant_rules().len() + policy.ssh.len(),
        },
    })))
}

/// The policy keys that do not form edges: automatic approval, node
/// attributes, postures and the policy's own tests. Passed through as parsed
/// values so the UI can render them without a second model.
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
            selector: selector.to_string(),
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
fn routes(nodes: &[crate::headscale::Machine]) -> Vec<Route> {
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
            exit_node: crate::headscale::Machine::is_exit_route(&cidr),
            cidr,
            approved,
            advertisers,
        })
        .collect()
}

/// Machines grouped by the relay region they home to.
async fn relays(state: &SharedState, nodes: &[crate::headscale::Machine]) -> Vec<Value> {
    let host_info = state.agent.host_info().await.unwrap_or_default();

    let mut by_region: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for node in nodes {
        let region = host_info
            .get(&node.node_key)
            .and_then(|info| info.get("HomeDERP"))
            .and_then(Value::as_i64);

        by_region
            .entry(
                region
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "unknown".into()),
            )
            .or_default()
            .push(json!({
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
