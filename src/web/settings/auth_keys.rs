//! Pre-auth key listing and lifecycle.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;
use crate::headscale::PreAuthKey;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, SharedState};

/// Reports whether the caller may manage keys for the given Headscale user id.
///
/// `generate_authkeys` covers every user; `generate_own_authkeys` only the
/// account's linked Headscale user.
fn may_manage(principal: &crate::auth::Principal, user_id: Option<&str>) -> bool {
    if principal.has(Capability::GenerateAuthKeys) {
        return true;
    }
    if !principal.has(Capability::GenerateOwnAuthKeys) {
        return false;
    }
    match (principal.linked_headscale_user(), user_id) {
        // Tag-only keys belong to nobody, so self-service cannot create them.
        (_, None) => false,
        (Some(linked), Some(user_id)) => linked == user_id,
        (None, Some(_)) => false,
    }
}

/// Keeps only the keys owned by `user_id`. Tag-only keys have no owner and are
/// dropped.
fn owned_keys(keys: Vec<PreAuthKey>, user_id: Option<&str>) -> Vec<PreAuthKey> {
    keys.into_iter()
        .filter(|key| {
            key.user
                .as_ref()
                .is_some_and(|user| Some(user.id.as_str()) == user_id)
        })
        .collect()
}

/// Lists pre-auth keys. `GET /api/auth-keys`
pub async fn list(State(state): State<SharedState>, Auth(principal): Auth) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::GenerateAuthKeys)
        && !principal.has(Capability::GenerateOwnAuthKeys)
    {
        return Err(ApiError::forbidden(
            "Your account cannot view pre-authentication keys",
        ));
    }

    let client = state
        .admin_client()
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    let users = state.live.users().await;
    let capabilities = state.headscale.capabilities();

    // 0.28+ lists every key in one call; older versions need one request per
    // user. A user whose keys fail to load is returned in `missing`.
    let (keys, missing): (Vec<PreAuthKey>, Vec<String>) = if capabilities
        .pre_auth_keys_have_stable_ids
    {
        match client.list_pre_auth_keys().await {
            Ok(keys) => (keys, Vec::new()),
            Err(err) => return Err(ApiError::from(err)),
        }
    } else {
        let mut all = Vec::new();
        let mut missing = Vec::new();
        for user in users.data.iter() {
            match client.list_pre_auth_keys_for_user(&user.id).await {
                Ok(mut keys) => all.append(&mut keys),
                Err(_) => missing.push(user.name.clone()),
            }
        }
        (all, missing)
    };

    // A self-service principal reaches the list through the admin client,
    // which returns every user's keys. Filter here: a pre-auth key is a
    // credential for its owner's machines.
    let self_service_only = !principal.has(Capability::GenerateAuthKeys);
    let keys = if self_service_only {
        owned_keys(keys, principal.linked_headscale_user())
    } else {
        keys
    };

    Ok(Json(json!({
        "keys": keys,
        "missing": missing,
        "users": users.data,
        "selfServiceOnly": self_service_only,
        "access": {
            "any": principal.has(Capability::GenerateAuthKeys),
            "own": principal.has(Capability::GenerateOwnAuthKeys),
            "linkedHeadscaleUserId": principal.linked_headscale_user(),
        },
        "server": state.headscale_public_base(),
    })))
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    /// Headscale user id; omitted for tag-only keys.
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    acl_tags: Vec<String>,
    /// Whole days.
    #[serde(default = "default_expiry_days")]
    expiry_days: i64,
    #[serde(default)]
    reusable: bool,
    #[serde(default)]
    ephemeral: bool,
}

fn default_expiry_days() -> i64 {
    90
}

/// Creates a pre-auth key. `POST /api/auth-keys`
pub async fn create(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<CreateKeyRequest>,
) -> ApiResult<Json<Value>> {
    if !may_manage(&principal, request.user_id.as_deref()) {
        return Err(ApiError::forbidden(
            "You can only create keys for your own account",
        ));
    }

    let user_id = request
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());

    if user_id.is_none() && request.acl_tags.is_empty() {
        return Err(ApiError::bad_request(
            "Choose a user or provide at least one ACL tag",
        ));
    }

    for tag in &request.acl_tags {
        if !crate::acl::is_valid_tag_name(tag) {
            return Err(ApiError::bad_request(format!(
                "`{tag}` is not a valid tag; tags must start with `tag:`"
            )));
        }
    }

    if request.expiry_days < 1 || request.expiry_days > 365_000 {
        return Err(ApiError::bad_request(
            "Expiry must be between 1 and 365000 days",
        ));
    }

    let expiration = chrono::Utc::now() + chrono::Duration::days(request.expiry_days);

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    let key = client
        .create_pre_auth_key(
            user_id,
            &request.acl_tags,
            request.ephemeral,
            request.reusable,
            Some(&crate::util::format_rfc3339(expiration)),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(json!({
        "key": key,
        // Shown to the user exactly once.
        "command": format!(
            "tailscale up --login-server={} --authkey={}",
            state.headscale_public_base(),
            key.key
        ),
    })))
}

#[derive(Deserialize)]
pub struct ExpireKeyRequest {
    #[serde(default)]
    key_id: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    user_id: Option<String>,
}

/// Expires a pre-auth key. `POST /api/auth-keys/expire`
pub async fn expire(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<ExpireKeyRequest>,
) -> ApiResult<Json<Value>> {
    if !may_manage(&principal, request.user_id.as_deref()) {
        return Err(ApiError::forbidden(
            "You can only expire keys for your own account",
        ));
    }

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    client
        .expire_pre_auth_key(&request.key_id, request.user_id.as_deref(), &request.key)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(json!({ "ok": true })))
}

/// Deletes a pre-auth key. `POST /api/auth-keys/delete`
///
/// Distinct from expiry: the key is removed, not marked expired.
pub async fn delete(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<ExpireKeyRequest>,
) -> ApiResult<Json<Value>> {
    if !may_manage(&principal, request.user_id.as_deref()) {
        return Err(ApiError::forbidden(
            "You can only delete keys for your own account",
        ));
    }
    if !state.headscale.capabilities().pre_auth_keys_have_stable_ids {
        return Err(ApiError::bad_request(
            "Deleting a pre-auth key needs Headscale 0.28 or newer; expire it instead",
        ));
    }

    let client = state
        .client_for(&principal)
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    client
        .delete_pre_auth_key(&request.key_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Principal, Role};
    use crate::db::SailplaneUser;

    fn principal(role: Role, linked: Option<&str>) -> Principal {
        Principal::User {
            user: SailplaneUser {
                id: "u".into(),
                sub: "sub".into(),
                name: None,
                email: None,
                picture: None,
                role,
                headscale_user_id: linked.map(str::to_string),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_login_at: None,
            },
            id_token: None,
        }
    }

    #[test]
    fn admins_manage_every_key() {
        let network_admin = principal(Role::NetworkAdmin, None);
        assert!(may_manage(&network_admin, Some("42")));
        assert!(may_manage(&network_admin, None));
    }

    #[test]
    fn viewers_only_manage_their_own_keys() {
        let viewer = principal(Role::Viewer, Some("7"));
        assert!(may_manage(&viewer, Some("7")));
        assert!(!may_manage(&viewer, Some("8")));
        // Tag-only keys have no owner and are therefore off limits.
        assert!(!may_manage(&viewer, None));
    }

    #[test]
    fn unlinked_viewers_cannot_manage_anything() {
        let viewer = principal(Role::Viewer, None);
        assert!(!may_manage(&viewer, Some("7")));
    }

    #[test]
    fn members_have_no_key_access() {
        let member = principal(Role::Member, Some("7"));
        assert!(!may_manage(&member, Some("7")));
    }

    /// A self-service principal must not receive other users' key secrets.
    #[test]
    fn self_service_listing_keeps_only_the_linked_users_keys() {
        let key = |id: &str, owner: Option<&str>| {
            let mut value = json!({ "id": id, "key": format!("secret-{id}") });
            if let Some(owner) = owner {
                value["user"] = json!({ "id": owner, "name": format!("user{owner}") });
            }
            serde_json::from_value::<PreAuthKey>(value).unwrap()
        };

        let all = vec![key("1", Some("7")), key("2", Some("8")), key("3", None)];

        let ids = |keys: Vec<PreAuthKey>| keys.into_iter().map(|key| key.id).collect::<Vec<_>>();

        assert_eq!(ids(owned_keys(all.clone(), Some("7"))), ["1"]);
        // No linked user: nothing is theirs, tag-only keys included.
        assert!(ids(owned_keys(all, None)).is_empty());
    }
}
