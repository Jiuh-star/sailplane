//! Headscale API keys: the credentials this deployment authenticates with.
//!
//! An API key is root-equivalent on the control plane, so the owner role guards
//! this page on its own. Creating a key stays a CLI action.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};

/// Lists Headscale API keys. `GET /api/api-keys`
pub async fn list(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::Owner])?;

    let client = state
        .admin_client()
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    let keys = client.list_api_keys().await.map_err(ApiError::from)?;

    Ok(Json(json!({
        "keys": keys,
        "canRevoke": true,
    })))
}

#[derive(Deserialize)]
pub struct RevokeRequest {
    id: String,
}

/// Revokes a Headscale API key. `POST /api/api-keys/revoke`
pub async fn revoke(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<RevokeRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::Owner])?;

    let id: u64 = request
        .id
        .trim()
        .parse()
        .map_err(|_| ApiError::bad_request("That API key id is not a number"))?;

    let client = state
        .admin_client()
        .ok_or_else(|| ApiError::internal("no Headscale API key is configured"))?;

    client.expire_api_key(id).await.map_err(ApiError::from)?;

    tracing::info!(api_key_id = id, "revoked a Headscale API key");
    Ok(Json(json!({ "ok": true })))
}
