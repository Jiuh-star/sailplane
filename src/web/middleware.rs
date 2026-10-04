//! Cross-cutting HTTP middleware.

use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::state::SharedState;

/// Whether the request's `Origin` matches its `Host`.
///
/// `None` means the header was absent, which is distinct from a mismatch:
/// non-browser clients omit it, and the WebSocket upgrade path requires it
/// outright.
pub fn origin_matches_host(headers: &axum::http::HeaderMap) -> Option<bool> {
    let origin = headers.get(header::ORIGIN)?.to_str().ok()?;
    // `Origin: null` and opaque origins have no host and never match.
    let origin_host = origin.split("://").nth(1)?.trim_end_matches('/');
    let host = headers.get(header::HOST)?.to_str().ok()?;
    Some(origin_host == host)
}

/// Rejects state-changing requests whose `Origin` does not match the host.
///
/// Upstream relies on `SameSite=Lax` alone; an explicit origin check costs
/// nothing and closes the gap for older browsers and non-browser clients.
pub async fn origin_check(request: Request, next: Next) -> Response {
    let is_state_changing = !matches!(
        request.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS | &Method::TRACE
    );

    if is_state_changing && origin_matches_host(request.headers()) == Some(false) {
        return (StatusCode::FORBIDDEN, "Cross-origin request rejected").into_response();
    }

    next.run(request).await
}

/// Records who changed what.
///
/// Headscale keeps no history of its own, so this log is the only record of a
/// rename, a revoke or a policy rewrite. Only state-changing methods are
/// recorded, and the query string is deliberately never stored, because it
/// can carry a key or a token.
///
/// The actor is resolved here rather than read from the handler: request
/// extensions set by an extractor do not survive into the response, and a
/// failed request (a 403, for example) is worth recording.
pub async fn audit(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let state_changing = !matches!(
        method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    );
    // The colour-scheme cookie is a display preference, not an action, and the
    // access check is a POST only because it carries a query object.
    let skippable = path.ends_with("/color-scheme") || path.ends_with("/simulate");
    if !state_changing || !path.contains("/api/") || skippable {
        return next.run(request).await;
    }

    let actor = actor_for(&state, request.headers()).await;

    let response = next.run(request).await;
    let status = response.status().as_u16();

    let entry = crate::db::AuditEntry {
        id: 0,
        at: chrono::Utc::now(),
        actor: actor.0,
        role: actor.1,
        method: method.to_string(),
        path,
        status,
        detail: None,
    };
    // A log that cannot be written must not fail the request it describes.
    if let Err(err) = state.db.record_audit(&entry) {
        tracing::warn!("could not write an audit entry: {err:#}");
    }

    response
}

/// The account behind a request, as `(label, role)`.
async fn actor_for(state: &SharedState, headers: &axum::http::HeaderMap) -> (String, String) {
    let cookie = crate::auth::session::read_cookie(
        headers.get(header::COOKIE).and_then(|value| value.to_str().ok()),
        crate::auth::session::SESSION_COOKIE,
    );

    match state.auth.resolve_session(cookie.as_deref()).await {
        Ok(Some(principal)) => {
            let role = match &principal {
                crate::auth::Principal::ApiKey { .. } => "api_key".to_string(),
                crate::auth::Principal::User { user, .. }
                | crate::auth::Principal::Proxy { user } => user.role.as_str().to_string(),
            };
            (principal.display_name(), role)
        }
        // A sign-in attempt has no session yet, and an unauthenticated request
        // is worth logging precisely because it was refused.
        _ => ("—".into(), "—".into()),
    }
}

/// Adds the security headers upstream omits.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; \
             script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; frame-ancestors 'none'; \
             base-uri 'self'; form-action 'self'",
        ),
    );

    response
}
