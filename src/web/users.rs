//! Headscale users, Sailplane accounts and the links between them.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::acl::Policy as AclPolicy;
use crate::auth::{Capability, Role};

use super::error::{ApiError, ApiResult};
use super::presentation::AccountView;
use super::state::{Auth, PrincipalExt, SharedState};

/// Usernames must be lowercase and URL-safe; Headscale rejects anything else.
fn validate_username(name: &str) -> Result<(), ApiError> {
    let valid = name.len() >= 2
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.' || c == '_')
        && !name.starts_with(['-', '.', '_']);

    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Usernames must be 2-63 characters of lowercase letters, digits, dots, dashes or \
             underscores",
        ))
    }
}

/// Resolves a route parameter against the live user list.
///
/// The id is interpolated into Headscale request paths, so it must come from
/// data already held rather than from the URL: a raw segment can carry `..`
/// (and `%2F`, which axum decodes) and escape into another endpoint.
async fn live_user(state: &SharedState, id_or_name: &str) -> ApiResult<crate::headscale::User> {
    state
        .live
        .users()
        .await
        .data
        .iter()
        .find(|user| user.id == id_or_name || user.name == id_or_name)
        .cloned()
        .ok_or_else(|| ApiError::not_found("That user does not exist"))
}

async fn load_policy(state: &SharedState) -> Option<AclPolicy> {
    let client = state.admin_client()?;
    let policy = client.get_policy().await.ok()?;
    AclPolicy::parse(&policy.policy).ok()
}

/// Lists accounts, Headscale users and group membership. `GET /api/users`
pub async fn list(State(state): State<SharedState>, Auth(principal): Auth) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadUsers) {
        return Err(ApiError::forbidden(
            "Your account does not have access to users",
        ));
    }

    let accounts = state.db.run(crate::db::list_users).await?;

    let nodes = state.live.nodes().await;
    let headscale_users = state.live.users().await;
    let policy = load_policy(&state).await;

    let groups: BTreeMap<String, Vec<String>> = headscale_users
        .data
        .iter()
        .map(|user| {
            let groups = policy
                .as_ref()
                .map(|policy| policy.groups_for_user(&user.name))
                .unwrap_or_default();
            (user.name.clone(), groups)
        })
        .collect();

    let views: Vec<AccountView> = accounts
        .iter()
        .map(|account| {
            let account_groups = account
                .headscale_user_id
                .as_deref()
                .and_then(|id| headscale_users.data.iter().find(|user| user.id == id))
                .and_then(|user| groups.get(&user.name))
                .cloned()
                .unwrap_or_default();
            AccountView::build(account, &headscale_users.data, &nodes.data, account_groups)
        })
        .collect();

    // Headscale users nobody has claimed yet.
    let claimed: Vec<&str> = accounts
        .iter()
        .filter_map(|account| account.headscale_user_id.as_deref())
        .collect();
    let unlinked: Vec<&crate::headscale::User> = headscale_users
        .data
        .iter()
        .filter(|user| !claimed.contains(&user.id.as_str()))
        .collect();

    Ok(Json(json!({
        "accounts": views,
        "headscaleUsers": headscale_users.data,
        "unlinkedUsers": unlinked,
        "groups": groups,
        "policy": {
            "available": policy.is_some(),
            "groups": policy.as_ref().map(|policy| policy.groups.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        },
        "access": {
            "read": true,
            "write": principal.has(Capability::WriteUsers),
            "policy_write": principal.has(Capability::WritePolicy),
            "owner": principal.has(Capability::Owner),
            "editable_groups": principal.has(Capability::WriteUsers)
                && principal.has(Capability::WritePolicy),
        },
        "currentAccountId": principal.sailplane_user().map(|user| user.id.clone()),
        "roles": Role::assignable()
            .iter()
            .map(|role| json!({
                "value": role.as_str(),
                "label": role.label(),
                "description": role.description(),
            }))
            .collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    username: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// Creates a Headscale user. `POST /api/users`
pub async fn create(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<CreateUserRequest>,
) -> ApiResult<Json<Value>> {
    principal
        .require(&[Capability::WriteUsers])?;

    let username = request.username.trim().to_lowercase();
    validate_username(&username)?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    let user = client
        .create_user(
            &username,
            request.email.as_deref().filter(|e| !e.trim().is_empty()),
            request.display_name.as_deref().filter(|n| !n.trim().is_empty()),
            None,
        )
        .await
        .map_err(ApiError::from)?;

    refresh_users(&state).await;
    Ok(Json(json!({ "user": user })))
}

/// Deletes a Headscale user, not the Sailplane account. `DELETE /api/users/{id}`
pub async fn remove(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteUsers])?;

    let user = live_user(&state, &id).await?;
    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;
    client.delete_user(&user.id).await.map_err(ApiError::from)?;

    refresh_users(&state).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct RenameUserRequest {
    new_name: String,
}

/// Renames a Headscale user. `POST /api/users/{id}/rename`
pub async fn rename(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<RenameUserRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteUsers])?;

    let new_name = request.new_name.trim().to_lowercase();
    validate_username(&new_name)?;

    let user = live_user(&state, &id).await?;
    if user.is_oidc() {
        return Err(ApiError::forbidden(
            "This user is managed by the identity provider and cannot be renamed here",
        ));
    }

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    client
        .rename_user(&user.id, &new_name)
        .await
        .map_err(ApiError::from)?;

    refresh_users(&state).await;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct GroupsRequest {
    groups: Vec<String>,
}

/// Rewrites a user's group membership in the ACL policy. `PUT /api/users/{name}/groups`
pub async fn update_groups(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(name): Path<String>,
    Json(request): Json<GroupsRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteUsers, Capability::WritePolicy])?;

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    let current = client.get_policy().await.map_err(ApiError::from)?;
    if current.updated_at.is_none() {
        return Err(ApiError::bad_request(
            "The ACL policy is read-only because Headscale is using file mode. Set \
             `policy.mode: database` to edit groups.",
        ));
    }

    let mut policy = AclPolicy::parse(&current.policy)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    policy
        .set_user_groups(&name, &request.groups)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    let text = policy
        .to_text()
        .map_err(|err| ApiError::internal(err.to_string()))?;
    client.set_policy(&text).await.map_err(ApiError::from)?;

    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct RoleRequest {
    role: String,
}

/// Changes an account's role. `POST /api/accounts/{id}/role`
pub async fn reassign_role(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<RoleRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteUsers])?;

    let role = Role::parse(&request.role);
    if role == Role::Owner {
        return Err(ApiError::bad_request(
            "Use the transfer ownership action to hand over ownership",
        ));
    }

    let target = state
        .db
        .get_user(&id)?
        .ok_or_else(|| ApiError::not_found("That account does not exist"))?;

    if target.is_owner() {
        return Err(ApiError::forbidden(
            "The owner's role cannot be changed; transfer ownership first",
        ));
    }

    state.db.set_user_role(&id, role)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct LinkRequest {
    headscale_user_id: String,
}

/// Links an account to a Headscale user. `POST /api/accounts/{id}/link`
pub async fn link(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Json(request): Json<LinkRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteUsers])?;

    state
        .db
        .link_headscale_user(&id, request.headscale_user_id.trim())
        .map_err(|err| ApiError::bad_request(err.to_string()))?;

    Ok(Json(json!({ "ok": true })))
}

/// Transfers ownership of the instance. `POST /api/accounts/{id}/transfer-ownership`
pub async fn transfer_ownership(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::Owner])?;

    if principal.sailplane_user().map(|user| user.id.as_str()) == Some(id.as_str()) {
        return Err(ApiError::bad_request("You already own this account"));
    }

    state
        .db
        .get_user(&id)?
        .ok_or_else(|| ApiError::not_found("That account does not exist"))?;

    state.db.transfer_ownership(&id)?;
    Ok(Json(json!({ "ok": true })))
}

/// Deletes a Sailplane account. `DELETE /api/accounts/{id}`
pub async fn delete_account(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteUsers])?;

    if principal.sailplane_user().map(|user| user.id.as_str()) == Some(id.as_str()) {
        return Err(ApiError::bad_request("You cannot delete your own account"));
    }

    let target = state
        .db
        .get_user(&id)?
        .ok_or_else(|| ApiError::not_found("That account does not exist"))?;

    if target.is_owner() {
        return Err(ApiError::forbidden(
            "The owner account cannot be deleted; transfer ownership first",
        ));
    }

    state.db.delete_user(&id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn refresh_users(state: &SharedState) {
    if let Some(client) = state.admin_client()
        && let Ok(users) = client.list_users().await {
            state.live.set_users(users).await;
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_validation() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("alice-2").is_ok());
        assert!(validate_username("a.b_c").is_ok());

        assert!(validate_username("A").is_err());
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("Alice").is_err());
        assert!(validate_username("-alice").is_err());
        assert!(validate_username("a").is_err());
    }
}
