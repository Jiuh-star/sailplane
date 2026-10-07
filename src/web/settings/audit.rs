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

/// Rows per page when the caller does not ask for a size.
const DEFAULT_PER_PAGE: i64 = 50;
const MAX_PER_PAGE: i64 = 200;

#[derive(Deserialize)]
pub struct AuditQuery {
    /// 1-based page number.
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    per_page: Option<i64>,
}

/// Lists one page of audit log entries. `GET /api/audit`
pub async fn list(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    // The log names users and the actions taken against them.
    principal.require(&[Capability::ConfigureIam])?;

    let per_page = query.per_page.unwrap_or(DEFAULT_PER_PAGE).clamp(1, MAX_PER_PAGE);
    let page = query.page.unwrap_or(1).max(1);

    let total = state
        .db
        .count_audit()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    // An entry-per-page of zero would divide by zero; `per_page` is clamped to
    // at least one above, so `pages` is also at least one.
    let pages = ((total + per_page - 1) / per_page).max(1);
    let page = page.min(pages);
    let offset = (page - 1) * per_page;

    let entries = state
        .db
        .list_audit(per_page, offset)
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    Ok(Json(json!({
        "entries": entries,
        "total": total,
        "page": page,
        "per_page": per_page,
        "pages": pages,
    })))
}
