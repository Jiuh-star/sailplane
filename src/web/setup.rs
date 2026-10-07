//! First-run onboarding.
//!
//! These endpoints exist only while the deployment is unconfigured: no account,
//! no single sign-on, and no Headscale API key. They are reachable without
//! authentication, because there is nothing to authenticate against yet. A
//! loopback peer, or a one-time token written beside the database and printed
//! to the log, guards them.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use serde::Deserialize;
use serde_json::{Value, json};

use super::error::{ApiError, ApiResult};
use super::state::SharedState;

/// Failed token attempts since start. The token is 32 random characters, so
/// guessing it is already hopeless. This only bounds how long an attacker (or a
/// very persistent typo) can hammer the endpoint.
static FAILED_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
const MAX_FAILED_ATTEMPTS: u32 = 10;

/// The peer address, when the server inserted one. It never rejects, so an
/// onboarding call works even where connect info is unavailable.
pub struct Peer(pub Option<IpAddr>);

impl FromRequestParts<SharedState> for Peer {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Peer(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| address.ip()),
        ))
    }
}

#[derive(Deserialize)]
pub struct TestRequest {
    url: String,
    #[serde(default)]
    api_key: String,
}

#[derive(Deserialize)]
pub struct CompleteRequest {
    url: String,
    api_key: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    cookie_secure: Option<bool>,
}

/// Reports whether onboarding is still needed. `GET /api/setup/status`
pub async fn status(State(state): State<SharedState>) -> ApiResult<Json<Value>> {
    let config = state.config();
    Ok(Json(json!({
        "required": super::auth_routes::setup_required(&state),
        "headscaleUrl": config.headscale.url,
        "hasApiKey": config.headscale.api_key.is_some(),
    })))
}

/// Probes a Headscale URL and API key before they are saved.
/// `POST /api/setup/test-headscale`
pub async fn test_headscale(
    State(state): State<SharedState>,
    Peer(peer): Peer,
    headers: axum::http::HeaderMap,
    Json(request): Json<TestRequest>,
) -> ApiResult<Json<Value>> {
    authorize(&state, peer, &headers)?;

    let headscale = crate::headscale::Headscale::new(request.url.trim(), None)
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;
    let client = headscale.client(request.api_key.clone());

    // Probe the server and prove the key at the same time. A valid key can list
    // nodes, which catches a wrong key before it is saved.
    let (version, nodes) = tokio::join!(headscale.probe_version(), client.list_nodes());
    let version = version
        .map_err(|err| ApiError::bad_request(format!("could not reach Headscale: {err:#}")))?;
    nodes.map_err(|err| ApiError::bad_request(format!("the API key was rejected: {err}")))?;

    Ok(Json(json!({
        "ok": true,
        "version": version.map(|version| version.raw),
    })))
}

/// Saves the minimum configuration and marks onboarding complete.
/// `POST /api/setup/complete`
pub async fn complete(
    State(state): State<SharedState>,
    Peer(peer): Peer,
    headers: axum::http::HeaderMap,
    Json(request): Json<CompleteRequest>,
) -> ApiResult<Json<Value>> {
    authorize(&state, peer, &headers)?;

    if request.url.trim().is_empty() || request.api_key.trim().is_empty() {
        return Err(ApiError::bad_request(
            "Enter the Headscale URL and an API key",
        ));
    }

    save(&state, "headscale.url", json!(request.url.trim()))?;
    save(&state, "headscale.api_key", json!(request.api_key.trim()))?;
    if let Some(base_url) = request.base_url.filter(|value| !value.trim().is_empty()) {
        save(&state, "server.base_url", json!(base_url.trim()))?;
    }
    if let Some(secure) = request.cookie_secure {
        save(&state, "server.cookie_secure", json!(secure))?;
    }
    state
        .db
        .save_setting(
            "meta.onboarding_completed_at",
            &json!(chrono::Utc::now().to_rfc3339()).to_string(),
            false,
        )
        .map_err(ApiError::Internal)?;

    state
        .reload_settings()
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    // The token is spent. Do not leave it lying on disk. `data_path` is where
    // it was written at boot, which an environment variable can override.
    if let Some(directory) = state.settings.data_path() {
        let _ = std::fs::remove_file(directory.join(crate::config::store::SETUP_TOKEN_FILE));
    }

    Ok(Json(json!({ "ok": true })))
}

fn save(state: &SharedState, key: &str, value: Value) -> Result<(), ApiError> {
    let encoded = serde_json::to_string(&value)
        .map_err(|err| ApiError::internal(format!("failed to encode `{key}`: {err}")))?;
    let secret = crate::config::schema::descriptor(key).is_some_and(|entry| entry.secret);
    state
        .db
        .save_setting(key, &encoded, secret)
        .map_err(ApiError::Internal)
}

/// Lets the setup call continue while onboarding is pending, from a loopback
/// peer or with the one-time token.
fn authorize(
    state: &SharedState,
    peer: Option<IpAddr>,
    headers: &axum::http::HeaderMap,
) -> Result<(), ApiError> {
    if !super::auth_routes::setup_required(state) {
        return Err(ApiError::not_found("Setup is already complete"));
    }

    if peer.is_some_and(|address| address.is_loopback()) {
        return Ok(());
    }

    if FAILED_ATTEMPTS.load(Ordering::Relaxed) >= MAX_FAILED_ATTEMPTS {
        return Err(ApiError::forbidden(
            "Too many failed setup attempts. Continue from localhost or restart Sailplane",
        ));
    }

    let expected = std::env::var("SAILPLANE_SETUP_TOKEN").ok().or_else(|| {
        state
            .db
            .get_setting("meta.setup_token")
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<String>(&raw).ok())
    });

    let provided = headers
        .get("x-setup-token")
        .and_then(|value| value.to_str().ok());

    if let (Some(expected), Some(provided)) = (expected, provided)
        && crate::auth::session::constant_time_eq(expected.as_bytes(), provided.as_bytes())
    {
        FAILED_ATTEMPTS.store(0, Ordering::Relaxed);
        return Ok(());
    }

    FAILED_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    Err(ApiError::forbidden(
        "Enter the setup token from the server log, or continue from localhost",
    ))
}
