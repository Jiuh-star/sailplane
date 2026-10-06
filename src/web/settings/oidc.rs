//! Headscale's own OpenID Connect settings.
//!
//! These decide how devices authenticate to the control server and live in
//! Headscale's config file, separate from the `oidc` block in Sailplane's own
//! config, which decides how people sign in to this UI. The client secret is
//! write-only: it is stored, never read back.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;
use crate::hsconfig::yaml_edit::parse_path;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};
use super::reload_after_change;

#[derive(Deserialize)]
pub struct OidcRequest {
    #[serde(default)]
    issuer: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    /// A value replaces the secret; the secret is never returned.
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    client_secret_path: Option<String>,
    #[serde(default)]
    scope: Option<Vec<String>>,
    #[serde(default)]
    use_expiry_from_token: Option<bool>,
    #[serde(default)]
    pkce_enabled: Option<bool>,
    #[serde(default)]
    only_start_if_available: Option<bool>,
    /// Headscale 0.29 removed `oidc.expiry` and refuses to start while it is
    /// still in the file. Removing it is the only way out of that state.
    #[serde(default)]
    remove_legacy_expiry: bool,
}

/// Returns the Headscale OIDC settings. `GET /api/oidc`
pub async fn get(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ReadNetwork])?;

    let document = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;
    let get = |path: &str| document.get_str(&parse_path(path)).unwrap_or_default();

    let secret = get("oidc.client_secret");
    let secret_path = get("oidc.client_secret_path");

    Ok(Json(json!({
        "oidc": {
            "configured": !get("oidc.issuer").is_empty(),
            "issuer": get("oidc.issuer"),
            "clientId": get("oidc.client_id"),
            // Presence only: the value never leaves the config file.
            "clientSecretSet": !secret.is_empty(),
            "clientSecretPath": secret_path,
            "scope": document.get_string_list(&parse_path("oidc.scope")),
            // Headscale removed this key; while it is present the server will
            // not start, so the UI offers to delete it.
            "legacyExpiry": !get("oidc.expiry").is_empty(),
            "useExpiryFromToken": document
                .get_bool(&parse_path("oidc.use_expiry_from_token"))
                .unwrap_or(false),
            "pkceEnabled": document
                .get_bool(&parse_path("oidc.pkce.enabled"))
                .unwrap_or(true),
            "onlyStartIfAvailable": document
                .get_bool(&parse_path("oidc.only_start_if_oidc_is_available"))
                .unwrap_or(true),
        },
        "access": {
            "read": true,
            "write": principal.has(Capability::WriteNetwork),
            "writable": state.hsconfig.writable(),
            "available": state.hsconfig.readable(),
        },
        "integration": state.integration.name(),
    })))
}

/// Updates the Headscale OIDC settings. `POST /api/oidc`
pub async fn update(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<OidcRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteNetwork])?;
    if !state.hsconfig.writable() {
        return Err(ApiError::forbidden(
            "The Headscale configuration file is not writable by Sailplane",
        ));
    }

    let mut changes: Vec<(Vec<String>, Option<Value>)> = Vec::new();

    if let Some(issuer) = request.issuer {
        validate_url("Issuer", &issuer)?;
        changes.push((parse_path("oidc.issuer"), Some(json!(issuer.trim()))));
    }

    if let Some(client_id) = request.client_id {
        validate_token("Client ID", &client_id)?;
        changes.push((parse_path("oidc.client_id"), Some(json!(client_id.trim()))));
    }

    if let Some(secret) = request.client_secret {
        if secret.is_empty() {
            // Clearing the secret is legitimate: a deployment may switch to
            // `client_secret_path` or a public client.
            changes.push((parse_path("oidc.client_secret"), None));
        } else {
            validate_secret(&secret)?;
            changes.push((parse_path("oidc.client_secret"), Some(json!(secret.trim()))));
        }
    }

    if let Some(path) = request.client_secret_path {
        if path.trim().is_empty() {
            changes.push((parse_path("oidc.client_secret_path"), None));
        } else {
            validate_path(&path)?;
            changes.push((
                parse_path("oidc.client_secret_path"),
                Some(json!(path.trim())),
            ));
        }
    }

    if let Some(scope) = request.scope {
        for entry in &scope {
            validate_token("Scope", entry)?;
        }
        changes.push((parse_path("oidc.scope"), Some(json!(dedupe(scope)))));
    }

    if request.remove_legacy_expiry {
        changes.push((parse_path("oidc.expiry"), None));
    }

    for (value, path) in [
        (request.use_expiry_from_token, "oidc.use_expiry_from_token"),
        (request.pkce_enabled, "oidc.pkce.enabled"),
        (
            request.only_start_if_available,
            "oidc.only_start_if_oidc_is_available",
        ),
    ] {
        if let Some(value) = value {
            changes.push((parse_path(path), Some(json!(value))));
        }
    }

    if changes.is_empty() {
        return Err(ApiError::bad_request("Nothing to change"));
    }

    // Headscale refuses to start with an issuer and no client id, or the
    // reverse, so refuse the partial edit. Clearing both fields together turns
    // OIDC off again.
    let touches_issuer = changes
        .iter()
        .any(|(path, _)| path.last().is_some_and(|key| key == "issuer"));
    let touches_client_id = changes
        .iter()
        .any(|(path, _)| path.last().is_some_and(|key| key == "client_id"));
    if touches_issuer != touches_client_id {
        let document = state
            .hsconfig
            .document()
            .map_err(|err| ApiError::internal(format!("{err:#}")))?;
        let current = |path: &str| document.get_str(&parse_path(path)).unwrap_or_default();
        let pending = |path: &str, key: &str| {
            changes
                .iter()
                .find(|(segment, _)| segment.last().is_some_and(|last| last == key))
                .map(|(_, value)| {
                    value
                        .as_ref()
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| current(path))
        };

        let issuer = pending("oidc.issuer", "issuer");
        let client_id = pending("oidc.client_id", "client_id");
        if issuer.is_empty() != client_id.is_empty() {
            let missing = if issuer.is_empty() {
                "oidc.issuer"
            } else {
                "oidc.client_id"
            };
            return Err(ApiError::bad_request(format!(
                "Headscale will not start with an issuer and no client id, or with a client id \
                 and no issuer. Set {missing} in the same request, or clear both to turn single \
                 sign-on off."
            )));
        }
    }

    // Headscale refuses to start when the provider is unreachable and the
    // "require it at startup" switch is on, which is its default. Probe first
    // rather than writing a configuration that will not boot.
    if let Some(target) = unroutable_provider(&state, &changes).await {
        return Err(ApiError::bad_request(format!(
            "Headscale cannot reach {target} and refuses to start while \
             `oidc.only_start_if_oidc_is_available` is on. Turn that switch off to configure \
             ahead of the provider."
        )));
    }

    state
        .hsconfig
        .patch(&changes)
        .await
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    // Half-configured OIDC is worse than none: Headscale refuses to start when
    // an issuer is set without a client id, or vice versa.
    let warning = match oidc_state(&state) {
        Ok(None) => reload_after_change(&state).await,
        Ok(Some(missing)) => Some(format!(
            "Headscale will not start until {missing} is filled in."
        )),
        Err(err) => Some(format!("{err:#}")),
    };

    Ok(Json(json!({ "ok": true, "warning": warning })))
}

/// Returns the issuer to refuse when the change leaves Headscale pointed at a
/// provider it cannot reach and configured to insist on reaching it.
async fn unroutable_provider(
    state: &SharedState,
    changes: &[(Vec<String>, Option<Value>)],
) -> Option<String> {
    let document = state.hsconfig.document().ok()?;

    let changed = |key: &str| {
        changes
            .iter()
            .find(|(path, _)| path.last().is_some_and(|last| last == key))
            .map(|(_, value)| value.clone())
    };

    // Only a write that introduces an issuer is worth probing.
    let issuer = match changed("issuer") {
        Some(Some(Value::String(issuer))) => issuer.trim().to_string(),
        _ => return None,
    };
    if issuer.is_empty() {
        return None;
    }

    // The switch may be part of this same request.
    let required = match changed("only_start_if_oidc_is_available") {
        Some(Some(Value::Bool(value))) => value,
        _ => document
            .get_bool(&parse_path("oidc.only_start_if_oidc_is_available"))
            .unwrap_or(true),
    };
    if !required {
        return None;
    }

    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .no_proxy()
        .build()
        .ok()?;

    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => None,
        Ok(response) => Some(format!("{issuer} (HTTP {})", response.status())),
        Err(_) => Some(issuer),
    }
}

/// Returns which piece of a half-configured provider is missing, if any.
fn oidc_state(state: &SharedState) -> anyhow::Result<Option<String>> {
    let document = state.hsconfig.document()?;
    let issuer = document
        .get_str(&parse_path("oidc.issuer"))
        .unwrap_or_default();
    let client_id = document
        .get_str(&parse_path("oidc.client_id"))
        .unwrap_or_default();

    Ok(
        match (issuer.trim().is_empty(), client_id.trim().is_empty()) {
            (false, true) => Some("oidc.client_id".to_string()),
            (true, false) => Some("oidc.issuer".to_string()),
            _ => None,
        },
    )
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if !value.is_empty() && !seen.contains(&value) {
            seen.push(value);
        }
    }
    seen
}

fn has_control_characters(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn validate_url(label: &str, value: &str) -> ApiResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if has_control_characters(value) || value.chars().any(char::is_whitespace) {
        return Err(ApiError::bad_request(format!(
            "{label} cannot contain spaces"
        )));
    }
    let parsed = url::Url::parse(value)
        .map_err(|_| ApiError::bad_request(format!("`{value}` is not a valid URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ApiError::bad_request(format!(
            "{label} must use http or https"
        )));
    }
    Ok(())
}

/// Validates client ids, scopes and other opaque single tokens.
fn validate_token(label: &str, value: &str) -> ApiResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    if has_control_characters(value)
        || value.chars().any(char::is_whitespace)
        || value.contains('"')
    {
        return Err(ApiError::bad_request(format!(
            "{label} cannot contain spaces or quotes"
        )));
    }
    Ok(())
}

/// Validates a secret. A secret may contain almost anything but not a line
/// break: it lands in a YAML document, and the editor's quoting is the second
/// line of defence.
fn validate_secret(value: &str) -> ApiResult<()> {
    if has_control_characters(value) {
        return Err(ApiError::bad_request(
            "The client secret cannot contain line breaks",
        ));
    }
    Ok(())
}

fn validate_path(value: &str) -> ApiResult<()> {
    if has_control_characters(value) {
        return Err(ApiError::bad_request("A path cannot contain line breaks"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issuers_must_be_urls() {
        assert!(validate_url("Issuer", "https://idp.example.com").is_ok());
        assert!(validate_url("Issuer", "").is_ok());
        assert!(validate_url("Issuer", "idp.example.com").is_err());
        assert!(validate_url("Issuer", "https://idp.example.com\nx: y").is_err());
        assert!(validate_url("Issuer", "javascript:alert(1)").is_err());
    }

    #[test]
    fn tokens_reject_structure() {
        assert!(validate_token("Client ID", "headscale").is_ok());
        assert!(validate_token("Scope", "openid").is_ok());
        assert!(validate_token("Client ID", "a b").is_err());
        assert!(validate_token("Client ID", "a\"b").is_err());
    }

    #[test]
    fn secrets_may_be_anything_but_a_line_break() {
        assert!(validate_secret("s3cr3t-!@#$%^&*()").is_ok());
        assert!(validate_secret("a\nb").is_err());
        assert!(validate_secret("a\rb").is_err());
    }
}
