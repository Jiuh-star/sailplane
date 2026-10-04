//! The audit log: what changed, by whom.
//!
//! Headscale keeps no history, so these rows, written by the `audit` middleware,
//! are the only record of a rename, a revocation or a policy rewrite.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};

/// Rows returned when the caller does not ask for a size.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 500;

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    limit: Option<i64>,
    /// Cursor: return entries older than this id.
    #[serde(default)]
    before: Option<i64>,
}

/// Lists audit log entries. `GET /api/audit`
pub async fn list(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    // The log names users and the actions taken against them.
    principal.require(&[Capability::ConfigureIam])?;

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let entries = state
        .db
        .list_audit(limit, query.before)
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let next = entries
        .last()
        .filter(|_| entries.len() as i64 == limit)
        .map(|entry| entry.id);

    Ok(Json(json!({ "entries": entries, "next": next })))
}
