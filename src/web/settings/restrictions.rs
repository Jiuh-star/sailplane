//! Authentication restrictions: which OIDC identities may sign in.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;
use crate::hsconfig::yaml_edit::parse_path;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};
use super::reload_after_change;

/// The restriction list an action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestrictionKind {
    Domains,
    Groups,
    Users,
}

impl RestrictionKind {
    fn config_path(self) -> &'static str {
        match self {
            Self::Domains => "oidc.allowed_domains",
            Self::Groups => "oidc.allowed_groups",
            Self::Users => "oidc.allowed_users",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Domains => "domain",
            Self::Groups => "group",
            Self::Users => "user",
        }
    }
}

/// Returns the OIDC restrictions. `GET /api/restrictions`
pub async fn get(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    if !principal.has(Capability::ReadUsers) {
        return Err(ApiError::forbidden(
            "Your account does not have access to authentication restrictions",
        ));
    }

    if state.oidc.is_none() {
        return Err(ApiError::bad_request(
            "Single sign-on is not configured, so there are no restrictions to manage",
        ));
    }

    let restrictions = state
        .hsconfig
        .oidc_restrictions()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    Ok(Json(json!({
        "restrictions": restrictions,
        "access": {
            "read": true,
            "write": principal.has(Capability::ConfigureIam),
            "writable": state.hsconfig.writable(),
        },
    })))
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    action: RestrictionAction,
    kind: RestrictionKind,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RestrictionAction {
    Add,
    Remove,
}

/// Adds or removes a restriction. `POST /api/restrictions`
pub async fn update(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<UpdateRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ConfigureIam])?;

    if !state.hsconfig.writable() {
        return Err(ApiError::forbidden(
            "The Headscale configuration file is not writable",
        ));
    }

    let value = request
        .value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("A value is required"))?;

    if request.kind == RestrictionKind::Domains {
        // A bare domain is accepted; a URL is rejected.
        let probe = format!("http://{value}");
        let host = url::Url::parse(&probe)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string));
        if host.as_deref() != Some(value) {
            return Err(ApiError::bad_request(
                "Enter a bare domain such as `example.com`, without a scheme or path",
            ));
        }
    }

    let path = parse_path(request.kind.config_path());

    let mut current = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?
        .get_string_list(&path);

    match request.action {
        RestrictionAction::Add => {
            if current.iter().any(|existing| existing == value) {
                return Err(ApiError::bad_request(format!(
                    "That {} is already permitted",
                    request.kind.label()
                )));
            }
            current.push(value.to_string());
        }
        RestrictionAction::Remove => {
            let before = current.len();
            current.retain(|existing| existing != value);
            if current.len() == before {
                return Err(ApiError::not_found(format!(
                    "That {} is not in the list",
                    request.kind.label()
                )));
            }
        }
    }

    state
        .hsconfig
        .patch(&[(path, Some(json!(current)))])
        .await
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    let warning = reload_after_change(&state).await;
    Ok(Json(json!({ "ok": true, "warning": warning })))
}
