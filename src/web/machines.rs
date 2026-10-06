//! Machine listing and mutation endpoints.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::acl::Policy as AclPolicy;
use crate::auth::Capability;
use crate::headscale::Machine;

use super::error::{ApiError, ApiResult};
use super::presentation::MachineView;
use super::state::{Auth, SharedState};

/// Lists machines with their host info and the versions the UI needs. `GET /api/machines`
pub async fn list(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadMachines) {
        return Err(ApiError::forbidden(
            "Your account does not have access to machines",
        ));
    }

    let nodes = state.live.nodes().await;
    let users = state.live.users().await;
    let host_info = state.agent.host_info().await.unwrap_or_default();

    let views: Vec<MachineView> = nodes
        .data
        .iter()
        .map(|node| MachineView::build(node, host_info.get(&node.node_key)))
        .collect();

    // Declared tags come from the ACL policy. A missing or unreadable policy
    // is not fatal; the UI then offers no tag suggestions.
    let policy = state
        .admin_client()
        .map(|client| async move { client.get_policy().await.ok() });
    let policy_tags = match policy {
        Some(future) => future
            .await
            .and_then(|policy| AclPolicy::parse(&policy.policy).ok())
            .map(|policy| policy.declared_tags())
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let magic = state
        .hsconfig
        .dns_config()
        .ok()
        .and_then(|dns| dns.base_domain);

    Ok(Json(json!({
        "machines": views,
        "users": users.data,
        "nodesVersion": nodes.version,
        "usersVersion": users.version,
        "policyTags": policy_tags,
        "magic": magic,
        "access": {
            "read": true,
            "write": principal.has(Capability::WriteMachines),
        },
        "supports": {
            "nodeOwnerChange": !state.headscale.capabilities().node_owner_is_immutable,
            "disablingKeyExpiry": state.headscale.capabilities().key_expiry_can_be_disabled,
        },
        "agent": state.agent.status().await,
        "server": state.headscale_public_base(),
    })))
}

/// Returns one machine in full. `GET /api/machines/{id}`
pub async fn detail(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadMachines) {
        return Err(ApiError::forbidden(
            "Your account does not have access to machines",
        ));
    }

    let nodes = state.live.nodes().await;
    let node = nodes
        .data
        .iter()
        .find(|node| node.id == id || node.given_name == id)
        .ok_or_else(|| ApiError::not_found("That machine does not exist"))?;

    let host_info = state.agent.host_info().await.unwrap_or_default();
    let view = MachineView::build(node, host_info.get(&node.node_key));

    Ok(Json(json!({
        "machine": view,
        "magic": state.hsconfig.dns_config().ok().and_then(|dns| dns.base_domain),
        "access": {
            "read": true,
            "write": principal.can_manage_node(node),
        },
        "supports": {
            "nodeOwnerChange": !state.headscale.capabilities().node_owner_is_immutable,
            "disablingKeyExpiry": state.headscale.capabilities().key_expiry_can_be_disabled,
        },
    })))
}

/// Loads a machine and checks the caller may modify it.
async fn writable_node(
    state: &SharedState,
    principal: &crate::auth::Principal,
    id: &str,
) -> ApiResult<Machine> {
    let nodes = state.live.nodes().await;
    let node = nodes
        .data
        .iter()
        .find(|node| node.id == id || node.given_name == id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("That machine does not exist"))?;

    if !principal.can_manage_node(&node) {
        return Err(ApiError::forbidden(
            "You can only manage machines that belong to your account",
        ));
    }
    Ok(node)
}

/// Refreshes the live snapshot so connected browsers see the change.
async fn refresh(state: &SharedState) {
    if let Some(client) = state.admin_client()
        && let Ok(nodes) = client.list_nodes().await
    {
        state.live.set_nodes(nodes).await;
    }
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    register_key: String,
    user: String,
}

/// Claims a machine from a registration key. `POST /api/machines/register`
pub async fn register(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<RegisterRequest>,
) -> ApiResult<Json<Value>> {
    principal
        .has(Capability::WriteMachines)
        .then_some(())
        .ok_or_else(|| ApiError::forbidden("You cannot register new machines"))?;

    let key = normalise_registration_key(&request.register_key, state.headscale.capabilities());
    if key.is_empty() {
        return Err(ApiError::bad_request("Enter a registration key"));
    }
    if request.user.trim().is_empty() {
        return Err(ApiError::bad_request("Select an owner for the machine"));
    }

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    let node = client
        .register_node(request.user.trim(), &key)
        .await
        .map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "machine": node })))
}

/// Headscale 0.29+ expects the full `hskey-authreq-…` key; earlier versions
/// want the bare id.
pub fn normalise_registration_key(
    raw: &str,
    capabilities: crate::headscale::Capabilities,
) -> String {
    let trimmed = raw.trim();
    if capabilities.register_key_includes_auth_req_prefix {
        return trimmed.to_string();
    }
    trimmed
        .strip_prefix("hskey-authreq-")
        .unwrap_or(trimmed)
        .to_string()
}

#[derive(Deserialize)]
pub struct RenameRequest {
    name: String,
}

/// Renames a machine. `POST /api/machines/{id}/rename`
pub async fn rename(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<RenameRequest>,
) -> ApiResult<Json<Value>> {
    let node = writable_node(&state, &principal, &id).await?;

    let name = request.name.trim();
    if !crate::util::is_valid_dns_label(name) {
        return Err(ApiError::bad_request(
            "Machine names may only contain lowercase letters, digits and dashes",
        ));
    }

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client
        .rename_node(&node.id, name)
        .await
        .map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true })))
}

/// Deletes a machine. `DELETE /api/machines/{id}`
pub async fn remove(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let node = writable_node(&state, &principal, &id).await?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client.delete_node(&node.id).await.map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true })))
}

/// Expires a machine's key now. `POST /api/machines/{id}/expire`
pub async fn expire(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let node = writable_node(&state, &principal, &id).await?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client.expire_node(&node.id).await.map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ExpiryRequest {
    #[serde(rename = "disableExpiry")]
    disable_expiry: bool,
}

/// Enables or disables key expiry; requires Headscale 0.29+. `POST /api/machines/{id}/expiry`
pub async fn toggle_expiry(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<ExpiryRequest>,
) -> ApiResult<Json<Value>> {
    if !state.headscale.capabilities().key_expiry_can_be_disabled {
        return Err(ApiError::bad_request(
            "This Headscale version cannot disable key expiry; upgrade to 0.29.0 or newer",
        ));
    }

    let node = writable_node(&state, &principal, &id).await?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client
        .toggle_node_expiry(&node.id, request.disable_expiry)
        .await
        .map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct TagsRequest {
    tags: Vec<String>,
}

/// Replaces a machine's tags. `POST /api/machines/{id}/tags`
pub async fn update_tags(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<TagsRequest>,
) -> ApiResult<Json<Value>> {
    for tag in &request.tags {
        if !crate::acl::is_valid_tag_name(tag) {
            return Err(ApiError::bad_request(format!(
                "`{tag}` is not a valid tag; tags must start with `tag:`"
            )));
        }
    }

    let node = writable_node(&state, &principal, &id).await?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    client
        .set_node_tags(&node.id, &request.tags)
        .await
        .map_err(|err| {
            if err.status() == Some(400) {
                ApiError::bad_request(
                    "Headscale rejected the tags. Every tag must be declared under `tagOwners` \
                     in the ACL policy.",
                )
            } else {
                ApiError::from(err)
            }
        })?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct RoutesRequest {
    /// A single route to enable or disable.
    route: String,
    enabled: bool,
}

/// Approves or revokes one advertised route. `POST /api/machines/{id}/routes`
pub async fn update_routes(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<RoutesRequest>,
) -> ApiResult<Json<Value>> {
    let node = writable_node(&state, &principal, &id).await?;

    if !node.available_routes.contains(&request.route) {
        return Err(ApiError::bad_request(
            "That route is not advertised by this machine",
        ));
    }

    let mut approved = node.approved_routes.clone();
    if request.enabled {
        if !approved.contains(&request.route) {
            approved.push(request.route.clone());
        }
    } else {
        approved.retain(|route| route != &request.route);
    }

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client
        .approve_routes(&node.id, &approved)
        .await
        .map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true, "approvedRoutes": approved })))
}

#[derive(Deserialize)]
pub struct OwnerRequest {
    user: String,
}

/// Reassigns a machine's owner; available before Headscale 0.28. `POST /api/machines/{id}/owner`
pub async fn reassign(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<OwnerRequest>,
) -> ApiResult<Json<Value>> {
    if state.headscale.capabilities().node_owner_is_immutable {
        return Err(ApiError::bad_request(
            "This Headscale version no longer allows changing a machine's owner",
        ));
    }

    let node = writable_node(&state, &principal, &id).await?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client
        .reassign_node(&node.id, request.user.trim())
        .await
        .map_err(ApiError::from)?;

    refresh(&state).await;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headscale::ServerVersion;

    #[test]
    fn registration_key_prefix_is_stripped_below_029() {
        let old = ServerVersion::parse("v0.28.0").capabilities();
        assert_eq!(
            normalise_registration_key("hskey-authreq-abc123", old),
            "abc123"
        );
        assert_eq!(normalise_registration_key("abc123", old), "abc123");

        let new = ServerVersion::parse("v0.29.0").capabilities();
        assert_eq!(
            normalise_registration_key("hskey-authreq-abc123", new),
            "hskey-authreq-abc123"
        );
    }

    #[test]
    fn registration_key_is_trimmed() {
        let caps = ServerVersion::parse("v0.28.0").capabilities();
        assert_eq!(normalise_registration_key("  abc  ", caps), "abc");
    }
}
