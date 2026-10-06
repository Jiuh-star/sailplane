//! DERP configuration: where the tailnet's relays come from.
//!
//! Headscale's API has no DERP surface; the map is built from the config file.
//! These handlers patch `derp.*` through the same comment-preserving editor the
//! DNS page uses.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::Capability;
use crate::hsconfig::yaml_edit::parse_path;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};
use super::reload_after_change;

/// A partial update: every field is optional, so the UI sends only what changed.
/// A request never clears a field it did not mention.
#[derive(Deserialize)]
pub struct DerpRequest {
    #[serde(default)]
    urls: Option<Vec<String>>,
    #[serde(default)]
    paths: Option<Vec<String>>,
    #[serde(default)]
    auto_update_enabled: Option<bool>,
    #[serde(default)]
    update_frequency: Option<String>,
    #[serde(default)]
    server_enabled: Option<bool>,
}

/// Returns the DERP configuration. `GET /api/derp`
pub async fn get(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ReadNetwork])?;

    let document = state
        .hsconfig
        .document()
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;

    Ok(Json(json!({
        "derp": {
            "urls": document.get_string_list(&parse_path("derp.urls")),
            "paths": document.get_string_list(&parse_path("derp.paths")),
            "autoUpdateEnabled": document
                .get_bool(&parse_path("derp.auto_update_enabled"))
                .unwrap_or(false),
            "updateFrequency": document
                .get_str(&parse_path("derp.update_frequency"))
                .unwrap_or_else(|| "24h".into()),
            "serverEnabled": document
                .get_bool(&parse_path("derp.server.enabled"))
                .unwrap_or(false),
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

/// Updates the DERP configuration. `POST /api/derp`
pub async fn update(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<DerpRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::WriteNetwork])?;
    if !state.hsconfig.writable() {
        return Err(ApiError::forbidden(
            "The Headscale configuration file is not writable by Sailplane",
        ));
    }

    let mut changes = Vec::new();

    if let Some(urls) = request.urls {
        for url in &urls {
            validate_url(url)?;
        }
        changes.push((parse_path("derp.urls"), Some(json!(dedupe(urls)))));
    }

    if let Some(paths) = request.paths {
        for path in &paths {
            validate_path(path)?;
        }
        changes.push((parse_path("derp.paths"), Some(json!(dedupe(paths)))));
    }

    if let Some(enabled) = request.auto_update_enabled {
        changes.push((parse_path("derp.auto_update_enabled"), Some(json!(enabled))));
    }

    if let Some(frequency) = request.update_frequency {
        validate_frequency(&frequency)?;
        changes.push((
            parse_path("derp.update_frequency"),
            Some(json!(frequency.trim())),
        ));
    }

    if let Some(enabled) = request.server_enabled {
        // Headscale refuses to start when the embedded server is enabled without
        // its listener. Check before writing a config that cannot boot.
        if enabled {
            let document = state
                .hsconfig
                .document()
                .map_err(|err| ApiError::internal(format!("{err:#}")))?;
            let mut missing = Vec::new();
            if document
                .get_str(&parse_path("derp.server.stun_listen_addr"))
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("derp.server.stun_listen_addr");
            }
            if document
                .get_str(&parse_path("derp.server.private_key_path"))
                .unwrap_or_default()
                .trim()
                .is_empty()
            {
                missing.push("derp.server.private_key_path");
            }
            if !missing.is_empty() {
                return Err(ApiError::bad_request(format!(
                    "Headscale will not start with the embedded DERP server enabled until {} \
                     {} set. Add them in the config file first.",
                    missing.join(" and "),
                    if missing.len() == 1 { "is" } else { "are" }
                )));
            }
        }
        changes.push((parse_path("derp.server.enabled"), Some(json!(enabled))));
    }

    if changes.is_empty() {
        return Err(ApiError::bad_request("Nothing to change"));
    }

    state
        .hsconfig
        .patch(&changes)
        .await
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    // Refuse to leave the tailnet with no relay map: every client would lose its
    // fallback path.
    let warning = match relay_sources(&state).await {
        Ok(sources) if sources > 0 => reload_after_change(&state).await,
        Ok(_) => Some(
            "Headscale now has no DERP map: set at least one URL, a local path, or enable \
             the embedded server."
                .to_string(),
        ),
        Err(err) => Some(format!("{err:#}")),
    };

    Ok(Json(json!({ "ok": true, "warning": warning })))
}

/// Counts the relay sources in the configuration.
async fn relay_sources(state: &SharedState) -> anyhow::Result<usize> {
    let document = state.hsconfig.document()?;
    let urls = document.get_string_list(&parse_path("derp.urls")).len();
    let paths = document.get_string_list(&parse_path("derp.paths")).len();
    let embedded = document
        .get_bool(&parse_path("derp.server.enabled"))
        .unwrap_or(false) as usize;
    Ok(urls + paths + embedded)
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

/// Validates a relay map URL. The URL must be fetchable and must not smuggle a
/// line break into the config document.
///
/// The check is explicit rather than left to the parser: the URL standard
/// *removes* tabs and newlines while parsing, so `https://a/b\nc` parses cleanly
/// and would be written into the config verbatim.
fn validate_url(url: &str) -> ApiResult<()> {
    let url = url.trim();
    if url.chars().any(char::is_control) || url.chars().any(char::is_whitespace) {
        return Err(ApiError::bad_request(
            "A DERP map URL cannot contain spaces",
        ));
    }

    let parsed = url::Url::parse(url)
        .map_err(|_| ApiError::bad_request(format!("`{url}` is not a valid URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ApiError::bad_request(
            "A DERP map URL must use http or https",
        ));
    }
    Ok(())
}

fn validate_path(path: &str) -> ApiResult<()> {
    let path = path.trim();
    if path.is_empty() || path.chars().any(char::is_control) {
        return Err(ApiError::bad_request("Enter a path to a DERP map file"));
    }
    Ok(())
}

/// Validates a Go duration: one or more `<number><unit>` groups, as Headscale
/// parses with `time.ParseDuration`.
pub(super) fn validate_frequency(frequency: &str) -> ApiResult<()> {
    validate_duration(frequency, false)
}

/// The same, optionally allowing a `d` unit. Headscale writes `oidc.expiry` in
/// days (`180d`), while `derp.update_frequency` is a plain Go duration; the
/// wrong shape is a config Headscale rejects.
pub(super) fn validate_duration(value: &str, allow_days: bool) -> ApiResult<()> {
    let mut rest = value.trim();
    if rest.is_empty() {
        return Err(ApiError::bad_request("Enter an update interval"));
    }
    while !rest.is_empty() {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            return Err(ApiError::bad_request("Use a duration such as 24h or 1h30m"));
        }
        rest = &rest[digits..];
        let units: &[&str] = if allow_days {
            &["ns", "us", "ms", "s", "m", "h", "d"]
        } else {
            &["ns", "us", "ms", "s", "m", "h"]
        };
        let unit = units.iter().find(|unit| rest.starts_with(**unit));
        let Some(unit) = unit else {
            return Err(ApiError::bad_request("Use a duration such as 24h or 1h30m"));
        };
        rest = &rest[unit.len()..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_follow_go_syntax() {
        for valid in ["24h", "1h30m", "30s", "500ms", "1ns"] {
            assert!(validate_frequency(valid).is_ok(), "{valid}");
        }
        for invalid in ["", "24", "h", "24 h", "24x", "-1h"] {
            assert!(validate_frequency(invalid).is_err(), "{invalid}");
        }
        // Days are only valid where Headscale expects them.
        assert!(validate_duration("180d", true).is_ok());
        assert!(validate_frequency("180d").is_err());
    }

    #[test]
    fn urls_must_be_http() {
        assert!(validate_url("https://controlplane.tailscale.com/derpmap/default").is_ok());
        assert!(validate_url("http://derp.internal/map.json").is_ok());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("not a url").is_err());
        assert!(validate_url("https://evil.example/a\nb").is_err());
    }

    #[test]
    fn paths_reject_control_characters() {
        assert!(validate_path("/etc/headscale/derp.yaml").is_ok());
        assert!(validate_path("").is_err());
        assert!(validate_path("/etc/derp.yaml\nurls: []").is_err());
    }

    #[test]
    fn duplicates_are_collapsed() {
        let values = vec![" a ".into(), "b".into(), "a".into(), "".into()];
        assert_eq!(dedupe(values), vec!["a", "b"]);
    }
}
