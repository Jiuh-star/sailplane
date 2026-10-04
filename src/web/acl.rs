//! Access control policy endpoints.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::acl::eval::{self, MachineRef};
use crate::acl::{Policy as AclPolicy, hujson};
use crate::auth::Capability;
use crate::headscale::ApiClient;

use super::error::{ApiError, ApiResult};
use super::presentation::tag_usage;
use super::state::{Auth, PrincipalExt, SharedState};

/// Reads the stored policy.
///
/// A missing policy is not an error: Headscale reports one thing in file mode
/// (a 500 with "acl policy not found") and another in database mode (an empty
/// policy). Both mean "nothing configured yet". `writable` is false when
/// Headscale reads the policy from a file, so edits would be discarded.
pub(super) async fn stored_policy(client: &ApiClient) -> Result<(String, Option<String>, bool), ApiError> {
    match client.get_policy().await {
        Ok(policy) => {
            let writable = policy.updated_at.is_some();
            Ok((policy.policy, policy.updated_at, writable))
        }
        Err(err) if err.is_policy_missing() || err.status() == Some(500) => {
            Ok((String::new(), None, true))
        }
        Err(err) => Err(ApiError::from(err)),
    }
}

/// Returns the stored policy and everything the editor needs. `GET /api/acl`
pub async fn get_policy(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadPolicy) {
        return Err(ApiError::forbidden(
            "Your account does not have access to the ACL policy",
        ));
    }

    let client = state
        .admin_client()
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    let (text, updated_at, writable) = stored_policy(&client).await?;

    let parsed = AclPolicy::parse(&text);
    let parse_error = parsed.as_ref().err().map(|err| err.to_string());
    let policy = parsed.ok();

    let nodes = state.live.nodes().await;
    let users = state.live.users().await;

    Ok(Json(json!({
        "policy": text,
        "updatedAt": updated_at,
        "writable": writable,
        "hasComments": hujson::has_comments(&text),
        "parsed": policy.as_ref().map(parsed_policy),
        "parseError": parse_error,
        "users": users.data.iter().map(|user| user.name.clone()).collect::<Vec<_>>(),
        "tagUsage": tag_usage(&nodes.data),
        "declaredTags": policy.as_ref().map(|policy| policy.declared_tags()).unwrap_or_default(),
        "selectors": selectors(&nodes.data, policy.as_ref()),
        "access": {
            "read": true,
            "write": principal.has(Capability::WritePolicy),
        },
    })))
}

/// Everything a policy selector can name, for the simulator's suggestions.
fn selectors(nodes: &[crate::headscale::Machine], policy: Option<&AclPolicy>) -> Value {
    let mut machines: Vec<String> = Vec::new();
    for node in nodes {
        machines.push(node.given_name.clone());
        machines.extend(node.ip_addresses.iter().cloned());
    }
    machines.retain(|value| !value.is_empty());
    machines.sort();

    let (groups, hosts) = match policy {
        Some(policy) => (
            policy.groups.keys().cloned().collect::<Vec<_>>(),
            policy.hosts.keys().cloned().collect::<Vec<_>>(),
        ),
        None => (Vec::new(), Vec::new()),
    };

    json!({
        "users": nodes
            .iter()
            .filter_map(|node| node.user.as_ref().map(|user| format!("{}@", user.name)))
            .collect::<std::collections::BTreeSet<_>>(),
        "groups": groups,
        "hosts": hosts,
        "tags": nodes
            .iter()
            .flat_map(crate::headscale::Machine::effective_tags)
            .collect::<std::collections::BTreeSet<_>>(),
        "machines": machines,
    })
}

#[derive(Deserialize)]
pub struct PolicyRequest {
    policy: String,
}

#[derive(Deserialize)]
pub struct SimulateRequest {
    src: String,
    /// One or more destinations. Checking a source against several hosts is
    /// the common case, so the endpoint takes a list.
    dsts: Vec<String>,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_protocol")]
    protocol: String,
    /// The editor's working copy, so a policy can be tested before saving.
    #[serde(default)]
    policy: Option<String>,
}

/// Bound on one request: each destination is a full policy evaluation.
const MAX_DESTINATIONS: usize = 50;

fn default_port() -> u16 {
    22
}

fn default_protocol() -> String {
    "tcp".into()
}

/// Checks whether the policy lets a source reach a destination, and reports
/// the rule that decided it. `POST /api/acl/simulate`
pub async fn simulate(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<SimulateRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ReadPolicy])?;

    let (text, source) = match request.policy {
        Some(draft) => (draft, "draft"),
        None => {
            let client = state
                .admin_client()
                .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
            let (text, _, _) = stored_policy(&client).await?;
            (text, "saved")
        }
    };

    let policy = AclPolicy::parse(&text)
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    let nodes = state.live.nodes().await;
    let machines: Vec<MachineRef> = nodes
        .data
        .iter()
        .map(MachineRef::from_machine)
        .collect();

    let dsts: Vec<String> = request
        .dsts
        .into_iter()
        .map(|dst| dst.trim().to_string())
        .filter(|dst| !dst.is_empty())
        .take(MAX_DESTINATIONS)
        .collect();

    if dsts.is_empty() {
        return Err(ApiError::bad_request("Enter a destination to check"));
    }

    let reports: Vec<eval::Report> = dsts
        .into_iter()
        .map(|dst| {
            eval::evaluate(
                &policy,
                &machines,
                &eval::Query {
                    src: request.src.clone(),
                    dst,
                    port: request.port,
                    protocol: request.protocol.clone(),
                },
            )
        })
        .collect();

    Ok(Json(json!({ "reports": reports, "policy": source })))
}

/// Renders the structured policy for the editor.
///
/// The editor round-trips this value straight back into the policy text, so it
/// has to match what `to_text` writes: field order is preserved (`preserve_order`
/// in Cargo.toml) and unknown top-level keys (`autoApprovers`, `nodeAttrs`, …)
/// are spread in place rather than nested under a wrapper that Headscale would
/// reject.
fn parsed_policy(policy: &AclPolicy) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("acls".into(), json!(policy.acls));
    map.insert("ssh".into(), json!(policy.ssh));
    map.insert("hosts".into(), json!(policy.hosts));
    map.insert("groups".into(), json!(policy.groups));
    map.insert("tagOwners".into(), json!(policy.tag_owners));

    for (key, value) in &policy.extra {
        map.insert(key.clone(), value.clone());
    }

    Value::Object(map)
}

/// Validates and stores the ACL policy. `PUT /api/acl`
pub async fn set_policy(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<PolicyRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WritePolicy])?;

    // Validate before sending so the user gets a precise syntax error.
    let mut parsed = AclPolicy::parse(&request.policy)
        .map_err(|err| ApiError::bad_request(format!("{err}")))?;

    // Headscale requires `host:port`; normalise bare hosts here as well as in
    // the editor so the API is safe without the UI.
    for rule in parsed.acls.iter_mut() {
        let (dst, _) = crate::acl::normalise_destinations(&rule.dst);
        rule.dst = dst;
    }

    let text = parsed
        .to_text()
        .map_err(|err| ApiError::internal(err.to_string()))?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    match client.set_policy(&text).await {
        Ok(policy) => Ok(Json(json!({
            "ok": true,
            "policy": policy.policy,
            "updatedAt": policy.updated_at,
        }))),
        Err(err) if err.is_policy_read_only() => Err(ApiError::forbidden(
            "The ACL policy is read-only because Headscale is using file mode. Set \
             `policy.mode: database` in the Headscale configuration to enable editing.",
        )),
        Err(err) => {
            // Headscale prefixes parse failures with `parsing HuJSON:` or
            // `parsing policy from bytes:`; strip that so the editor shows a
            // plain syntax error.
            let raw = err.raw_body();
            for prefix in ["parsing HuJSON:", "parsing policy from bytes:"] {
                if let Some(rest) = raw.split(prefix).nth(1) {
                    return Err(ApiError::bad_request(format!(
                        "Syntax error:{}",
                        rest.trim_end()
                    )));
                }
            }
            Err(ApiError::from(err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The preview diffs the JSON against the policy text, so the two must
    /// render identically. Without `preserve_order` every line looked changed.
    #[test]
    fn parsed_json_keeps_the_order_to_text_writes() {
        let policy = AclPolicy::parse(
            r#"{
                "acls": [{ "action": "accept", "src": ["a"], "dst": ["b:*"] }],
                "hosts": { "git": "100.64.0.3" },
                "autoApprovers": { "exitNode": ["tag:prod"] }
            }"#,
        )
        .unwrap();

        let text = policy.to_text().unwrap();
        let parsed = parsed_policy(&policy);
        let from_json = serde_json::to_string_pretty(&parsed).unwrap();

        assert_eq!(text, from_json, "the two views must render identically");

        // The rule fields also keep their declared order.
        let keys: Vec<&str> = parsed["acls"][0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["action", "src", "dst"]);
    }

    /// Unknown top-level keys stay at the top level, where the editor writes
    /// them back from.
    #[test]
    fn parsed_json_has_no_synthetic_wrapper_key() {
        let policy = AclPolicy::parse(r#"{"acls":[],"autoApprovers":{"exitNode":[]}}"#).unwrap();
        let parsed = parsed_policy(&policy);
        let object = parsed.as_object().unwrap();

        assert!(!object.contains_key("extra"));
        assert!(object.contains_key("autoApprovers"));
    }
}
