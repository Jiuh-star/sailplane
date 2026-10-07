//! Operational endpoints: health, build info and the color-scheme cookie.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::error::{ApiError, ApiResult};
use super::state::SharedState;

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
}

/// Reports Headscale health. `GET /healthz`
pub async fn healthz(State(state): State<SharedState>) -> Response {
    let healthy = state.headscale.health().await;
    let status = if healthy { "OK" } else { "ERROR" };
    let code = if healthy {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (code, Json(HealthResponse { status })).into_response()
}

/// Returns build and version info, guarded by `server.info_secret`. `GET /api/info`
pub async fn info(State(state): State<SharedState>, headers: HeaderMap) -> ApiResult<Response> {
    let config = state.config();
    let Some(secret) = config.server.info_secret.as_deref() else {
        return Err(ApiError::forbidden(
            "The info endpoint is disabled because `server.info_secret` is not set",
        ));
    };

    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    match provided {
        None => return Err(ApiError::unauthorized("Missing bearer token")),
        Some(token)
            if !crate::auth::session::constant_time_eq(token.as_bytes(), secret.as_bytes()) =>
        {
            return Err(ApiError::forbidden("Invalid info secret"));
        }
        Some(_) => {}
    }

    let healthy = state.headscale.health().await;
    let version = state.headscale.version();

    Ok(Json(json!({
        "status": if healthy { "healthy" } else { "unhealthy" },
        "sailplane_version": env!("CARGO_PKG_VERSION"),
        "headscale_canonical_version": version.canonical(),
        "headscale_version_raw": version.raw,
        "internal_versions": {
            "runtime": "rust",
            "rustc": option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown"),
        },
    }))
    .into_response())
}

#[derive(Deserialize)]
pub struct ColorSchemeRequest {
    #[serde(rename = "colorScheme")]
    color_scheme: String,
    #[serde(rename = "returnTo")]
    return_to: Option<String>,
}

/// Stores the theme preference in a cookie. `POST /api/color-scheme`
pub async fn color_scheme(
    State(state): State<SharedState>,
    Json(request): Json<ColorSchemeRequest>,
) -> ApiResult<Response> {
    let scheme = request.color_scheme.as_str();
    if !matches!(scheme, "dark" | "light" | "system") {
        return Err(ApiError::bad_request(
            "colorScheme must be one of dark, light or system",
        ));
    }

    let fallback = format!("{}/", state.prefix());
    let target = crate::auth::session::safe_redirect(
        request.return_to.as_deref().unwrap_or(&fallback),
        &fallback,
    );

    let cookie = if scheme == "system" {
        format!(
            "{}=; Path=/; Max-Age=0; SameSite=Lax{}",
            crate::auth::session::COLOR_SCHEME_COOKIE,
            if state.config().server.cookie_secure {
                "; Secure"
            } else {
                ""
            }
        )
    } else {
        format!(
            "{}={}; Path=/; Max-Age=34560000; SameSite=Lax{}",
            crate::auth::session::COLOR_SCHEME_COOKIE,
            scheme,
            if state.config().server.cookie_secure {
                "; Secure"
            } else {
                ""
            }
        )
    };

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "ok": true, "redirect": target })),
    )
        .into_response())
}
