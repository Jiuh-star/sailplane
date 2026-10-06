//! Session establishment, API-key login, OIDC and logout.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::session::{
    CookieOptions, OIDC_STATE_COOKIE, SESSION_COOKIE, encode_cookie, read_cookie,
};
use crate::auth::{CookiePayload, OidcTransaction, Role};

use super::error::{ApiError, ApiResult};
use super::presentation::{AccessView, HeadscaleVersionView, UserView};
use super::state::{AppState, MaybeAuth, SharedState};

/// Returns everything the SPA shell needs on boot. `GET /api/session`
///
/// Unauthenticated callers still get a 200 with `authenticated: false` so the
/// login page can render configuration warnings without a second request.
pub async fn session(
    State(state): State<SharedState>,
    MaybeAuth(principal): MaybeAuth,
) -> Json<Value> {
    let healthy = state.headscale.health().await;
    let version = HeadscaleVersionView::build(&state.headscale.version());

    let Some(principal) = principal else {
        return Json(json!({
            "authenticated": false,
            "user": Value::Null,
            "principal": Value::Null,
            "access": Value::Null,
            "config": config_view(&state),
            "headscale": { "healthy": healthy, "version": version },
        }));
    };

    let access = AccessView::from_principal(&principal);
    Json(json!({
        "authenticated": true,
        // API-key sessions have no account row, so the key prefix stands in as
        // the display name.
        "user": UserView::from_principal(&principal)
            .or_else(|| principal.is_api_key().then(|| UserView::api_key(&principal.display_name()))),
        "principal": match &principal {
            crate::auth::Principal::ApiKey { .. } => "api_key",
            crate::auth::Principal::Proxy { .. } => "proxy",
            crate::auth::Principal::User { .. } => "user",
        },
        "access": access,
        "config": config_view(&state),
        "headscale": { "healthy": healthy, "version": version },
    }))
}

/// Configuration flags the UI branches on.
fn config_view(state: &AppState) -> Value {
    let oidc_enabled = state.oidc.is_some();
    let config = state.config();
    json!({
        "prefix": state.prefix(),
        // The address machines register with, not Sailplane's own URL.
        "baseUrl": state.headscale_public_base(),
        "headscaleUrl": config.headscale.url,
        "grantsSupported": state.headscale.capabilities().grants_supported,
        "configAvailable": state.hsconfig.readable(),
        "configWritable": state.hsconfig.writable(),
        "oidcEnabled": oidc_enabled,
        "apiKeyLoginDisabled": config
            .oidc
            .as_ref()
            .map(|oidc| oidc.disable_api_key_login)
            .unwrap_or(false),
        "cookieSecure": config.server.cookie_secure,
        "integration": state.integration.name(),
        "agentEnabled": state.agent.is_enabled(),
        "agentBackend": state.agent.name(),
        "debug": config.debug_logging(),
        "version": env!("CARGO_PKG_VERSION"),
        // The first-run wizard shows until onboarding is completed and an
        // account exists. A deployment that logs in through OIDC skips it.
        "setupRequired": setup_required(state),
    })
}

/// True while the first-run wizard should be shown: a fresh deployment with no
/// account, no single sign-on, and no Headscale API key yet.
pub(super) fn setup_required(state: &AppState) -> bool {
    let onboarded = state
        .db
        .get_setting("meta.onboarding_completed_at")
        .ok()
        .flatten()
        .is_some();
    if onboarded || state.oidc.is_some() {
        return false;
    }
    // A deployment that already has a Headscale API key (imported from a legacy
    // config file or set in the environment) is configured; the wizard would
    // only re-ask for values it already has.
    if state.config().headscale.api_key.is_some() {
        return false;
    }
    state
        .db
        .count_users()
        .map(|count| count == 0)
        .unwrap_or(false)
}

#[derive(Deserialize)]
pub struct LoginRequest {
    api_key: String,
}

/// Validates a Headscale API key and opens a session. `POST /api/auth/login`
pub async fn login(
    State(state): State<SharedState>,
    Json(request): Json<LoginRequest>,
) -> ApiResult<Response> {
    // The SPA hides the form when the deployment is SSO-only; the API has to
    // refuse as well, or the policy is only a UI preference.
    if state
        .config()
        .oidc
        .as_ref()
        .is_some_and(|oidc| oidc.disable_api_key_login)
    {
        return Err(ApiError::forbidden(
            "API key sign-in is disabled for this deployment; sign in with single sign-on",
        ));
    }

    let candidate = request.api_key.trim();
    if candidate.is_empty() {
        return Err(ApiError::bad_request(
            "Enter a Headscale API key to sign in",
        ));
    }

    let validation = state
        .auth
        .validate_api_key(&state.headscale, candidate)
        .await
        .map_err(ApiError::Internal)?
        .map_err(ApiError::unauthorized)?;

    let ttl = validation.expires_at - chrono::Utc::now();
    if ttl <= chrono::Duration::zero() {
        return Err(ApiError::unauthorized("That API key has already expired"));
    }

    let session = state
        .db
        .create_api_key_session(candidate, &validation.display_name, ttl)
        .map_err(ApiError::Internal)?;

    let cookie = encode_cookie(
        &CookiePayload {
            sid: session.id.clone(),
            api_key: Some(candidate.to_string()),
            profile: None,
        },
        &state.cookie_secret(),
    )
    .map_err(ApiError::Internal)?;

    let options = state.auth.cookie_options();
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, options.render(SESSION_COOKIE, &cookie))],
        Json(json!({
            "ok": true,
            "user": { "name": validation.display_name, "role": "api_key" },
        })),
    )
        .into_response())
}

/// Clears the session and returns the provider's end-session URL for OIDC
/// RP-initiated logout. `POST /api/auth/logout`
pub async fn logout(
    State(state): State<SharedState>,
    MaybeAuth(principal): MaybeAuth,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let cookie = read_cookie(
        headers.get(header::COOKIE).and_then(|v| v.to_str().ok()),
        SESSION_COOKIE,
    );

    state
        .auth
        .destroy_session(cookie.as_deref())
        .await
        .map_err(ApiError::Internal)?;

    let mut body = json!({ "ok": true, "redirect": Value::Null });

    // RP-initiated logout: hand the client the provider URL so the browser can
    // complete the round trip.
    if let (Some(oidc), Some(principal)) = (state.oidc.as_ref(), principal.as_ref())
        && let Some(id_token) = principal.id_token()
    {
        let post_logout = state
            .config()
            .oidc
            .as_ref()
            .map(|config| config.post_logout_redirect(&state.public_base(), state.prefix()))
            .unwrap_or_else(|| format!("{}{}/login?s=logout", state.public_base(), state.prefix()));

        if let Some(url) = oidc.end_session_url(Some(id_token), &post_logout).await {
            body["redirect"] = Value::String(url);
        }
    }

    let options = state.auth.cookie_options();
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, options.render_cleared(SESSION_COOKIE))],
        Json(body),
    )
        .into_response())
}

/// Begins the OIDC authorization code flow. `GET /oidc/start`
pub async fn oidc_start(
    State(state): State<SharedState>,
    MaybeAuth(principal): MaybeAuth,
) -> Result<Response, ApiError> {
    if principal.is_some() {
        return Ok(
            Redirect::to(&format!("{}{}/", state.public_base(), state.prefix())).into_response(),
        );
    }

    let Some(oidc) = state.oidc.clone() else {
        return Err(ApiError::bad_request(
            "Single sign-on is not configured on this Sailplane instance",
        ));
    };

    let redirect_uri = oidc_redirect_uri(&state);
    let (url, transaction) = oidc
        .begin_flow(&redirect_uri)
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let options = oidc_state_options(&state);
    let value = transaction
        .encode()
        .map_err(|err| ApiError::internal(err.to_string()))?;

    Ok((
        StatusCode::FOUND,
        [
            (header::LOCATION, url),
            (
                header::SET_COOKIE,
                options.render(OIDC_STATE_COOKIE, &value),
            ),
        ],
        (),
    )
        .into_response())
}

/// Completes the OIDC flow and opens a session. `GET /oidc/callback`
pub async fn oidc_callback(
    State(state): State<SharedState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let login_url =
        |code: &str| format!("{}{}/login?s={code}", state.public_base(), state.prefix());

    if !query.contains_key("code") && !query.contains_key("state") {
        return Ok(Redirect::to(&login_url("error_no_query")).into_response());
    }

    let Some(oidc) = state.oidc.clone() else {
        return Err(ApiError::bad_request(
            "Single sign-on is not configured on this Sailplane instance",
        ));
    };

    let Some(raw_state) = read_cookie(
        headers.get(header::COOKIE).and_then(|v| v.to_str().ok()),
        OIDC_STATE_COOKIE,
    ) else {
        return Ok(Redirect::to(&login_url("error_no_session")).into_response());
    };

    let Some(transaction) = OidcTransaction::decode(&raw_state).ok() else {
        return Ok(Redirect::to(&login_url("error_invalid_session")).into_response());
    };

    if query.get("state") != Some(&transaction.state) {
        return Ok(Redirect::to(&login_url("error_auth_failed")).into_response());
    }

    let Some(code) = query.get("code") else {
        return Ok(Redirect::to(&login_url("error_auth_failed")).into_response());
    };

    let profile = match oidc.handle_callback(code, &transaction).await {
        Ok(profile) => profile,
        Err(err) => {
            tracing::warn!("OIDC callback failed: {err:#}");
            let code = if err.to_string().contains("missing_sub") {
                "error_no_sub"
            } else {
                "error_auth_failed"
            };
            return Ok(Redirect::to(&login_url(code)).into_response());
        }
    };

    let initial_role = state
        .config()
        .oidc
        .as_ref()
        .map(|config| Role::parse(&config.default_role))
        .unwrap_or(Role::Member);

    let user = state
        .auth
        .find_or_create_user(
            &profile.subject,
            profile.name.as_deref(),
            profile.email.as_deref(),
            profile.picture.as_deref(),
            initial_role,
            profile.role,
        )
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    // Auto-link to the matching Headscale user so machines and keys resolve.
    if user.headscale_user_id.is_none()
        && let Some(client) = state.admin_client()
        && let Ok(headscale_users) = client.list_users().await
    {
        let _ = state
            .auth
            .auto_link_headscale_user(&user, &headscale_users)
            .await;
    }

    // Only persist the ID token when it is needed for RP-initiated logout.
    let store_id_token = state
        .config()
        .oidc
        .as_ref()
        .map(|config| config.use_end_session)
        .unwrap_or(false);

    let session = state
        .db
        .create_oidc_session(
            &user.id,
            store_id_token.then_some(profile.id_token.as_str()),
            chrono::Duration::seconds(state.config().server.cookie_max_age),
        )
        .map_err(ApiError::Internal)?;

    let cookie = encode_cookie(
        &CookiePayload {
            sid: session.id.clone(),
            api_key: None,
            profile: None,
        },
        &state.cookie_secret(),
    )
    .map_err(ApiError::Internal)?;

    Ok(oidc_redirect_response(
        format!("{}{}/", state.public_base(), state.prefix()),
        state.auth.cookie_options().render(SESSION_COOKIE, &cookie),
        oidc_state_options(&state).render_cleared(OIDC_STATE_COOKIE),
    ))
}

/// Redirects the browser into the session. The response carries two cookies:
/// the new session and the cleared OIDC transaction.
///
/// A plain array of headers cannot express this. `IntoResponseParts` for
/// arrays calls `HeaderMap::insert`, so the second `Set-Cookie` would replace
/// the first and the browser would never see the session. `AppendHeaders`
/// keeps both.
fn oidc_redirect_response(
    location: String,
    session_cookie: String,
    cleared_state_cookie: String,
) -> Response {
    (
        StatusCode::FOUND,
        [(header::LOCATION, location)],
        AppendHeaders([
            (header::SET_COOKIE, session_cookie),
            (header::SET_COOKIE, cleared_state_cookie),
        ]),
    )
        .into_response()
}

/// Cookie attributes of the short-lived OIDC transaction cookie. The callback
/// clears the cookie with the same attributes; a different path would leave
/// the original cookie in the browser.
fn oidc_state_options(state: &AppState) -> CookieOptions {
    CookieOptions {
        secure: state.config().server.cookie_secure,
        http_only: true,
        max_age_seconds: 1800,
        domain: state.config().server.cookie_domain.clone(),
        path: format!("{}/oidc/callback", state.prefix()),
        same_site: crate::auth::SameSite::Lax,
    }
}

fn oidc_redirect_uri(state: &AppState) -> String {
    format!("{}{}/oidc/callback", state.public_base(), state.prefix())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The session cookie must not shadow the cleared transaction cookie. A
    /// regression here breaks every OIDC sign-in while the API key login keeps
    /// working, which is hard to spot without this check.
    #[test]
    fn oidc_redirect_keeps_both_cookies() {
        let response = oidc_redirect_response(
            "https://sailplane.example.com/admin/".into(),
            "_sailplane_auth=session; Path=/admin; HttpOnly".into(),
            "__oidc_state=; Path=/admin/oidc/callback; Max-Age=0".into(),
        );

        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect();

        assert_eq!(cookies.len(), 2, "both cookies must survive: {cookies:?}");
        assert!(cookies.iter().any(|c| c.starts_with("_sailplane_auth=")));
        assert!(cookies.iter().any(|c| c.starts_with("__oidc_state=")));
        assert_eq!(
            response.headers()[header::LOCATION],
            "https://sailplane.example.com/admin/"
        );
    }
}
