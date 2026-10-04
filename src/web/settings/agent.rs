//! Agent status and manual sync.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::auth::Capability;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};

/// Returns the agent status. `GET /api/agent`
pub async fn status(State(state): State<SharedState>, Auth(principal): Auth) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadFeature) {
        return Err(ApiError::forbidden(
            "Your account does not have access to the agent settings",
        ));
    }

    Ok(Json(json!({ "agent": state.agent.status().await })))
}

/// Forces an immediate host-info refresh. `POST /api/agent/sync`
pub async fn sync(State(state): State<SharedState>, Auth(principal): Auth) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteFeature])?;

    if !state.agent.is_enabled() {
        return Err(ApiError::bad_request(
            "The Sailplane agent is not enabled",
        ));
    }

    let node_keys: Vec<String> = state
        .live
        .nodes()
        .await
        .data
        .iter()
        .map(|node| node.node_key.clone())
        .collect();

    match state.agent.sync(&node_keys).await {
        Ok(count) => Ok(Json(json!({
            "ok": true,
            "nodeCount": count,
            "agent": state.agent.status().await,
        }))),
        Err(err) => Ok(Json(json!({
            "ok": false,
            "error": format!("{err:#}"),
            "agent": state.agent.status().await,
        }))),
    }
}
