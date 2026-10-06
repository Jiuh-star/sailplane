//! Sailplane's own deployment settings.
//!
//! Unlike the rest of `web::settings`, which edits Headscale's config file,
//! this edits the values Sailplane stores in its own database. Secret values are
//! never returned; the UI only learns whether one is set.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::auth::Capability;
use crate::config::schema::SETTINGS_SCHEMA;
use crate::config::store::Source;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};

/// Returns every setting: its descriptor, effective value and source. `GET /api/settings`
pub async fn get(
    State(state): State<SharedState>,
    Auth(principal): Auth,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ConfigureSailplane])?;

    let config = serde_json::to_value(&*state.config())
        .map_err(|err| ApiError::internal(format!("failed to render the settings: {err}")))?;
    // `sources` also tells the form whether a secret is already set, so it can
    // distinguish "no value yet" from "a value is set, replace it".
    let sources = state.settings.sources();

    let entries: Vec<Value> = SETTINGS_SCHEMA
        .iter()
        .map(|descriptor| {
            let raw = get_path(&config, descriptor.key);
            let (value, set) = if descriptor.secret {
                let configured = sources
                    .get(descriptor.key)
                    .is_some_and(|source| *source != Source::Default);
                (Value::Null, configured)
            } else {
                (raw.cloned().unwrap_or(Value::Null), raw.is_some())
            };
            json!({
                "key": descriptor.key,
                "group": descriptor.group,
                "kind": descriptor.kind,
                "secret": descriptor.secret,
                "restartRequired": descriptor.restart_required,
                "value": value,
                "set": set,
                "source": sources.get(descriptor.key),
            })
        })
        .collect();

    Ok(Json(json!({ "settings": entries })))
}

#[derive(Deserialize)]
pub struct UpdateRequest {
    /// Dotted keys to values. A `null` value removes the setting.
    values: Map<String, Value>,
}

/// Stores settings and reloads the snapshot. `PUT /api/settings`
pub async fn update(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<UpdateRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ConfigureSailplane])?;

    let mut restart_required = Vec::new();
    for key in request.values.keys() {
        let Some(descriptor) = crate::config::schema::descriptor(key) else {
            return Err(ApiError::bad_request(format!(
                "`{key}` is not a known setting"
            )));
        };
        if descriptor.restart_required {
            restart_required.push(key.clone());
        }
    }

    apply(&state, &request.values)?;
    state
        .reload_settings()
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    Ok(Json(
        json!({ "ok": true, "restartRequired": restart_required }),
    ))
}

#[derive(Deserialize)]
pub struct ValidateRequest {
    values: Map<String, Value>,
}

/// Validates candidate settings without saving. `POST /api/settings/validate`
pub async fn validate(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<ValidateRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ConfigureSailplane])?;

    for key in request.values.keys() {
        if crate::config::schema::descriptor(key).is_none() {
            return Ok(Json(
                json!({ "valid": false, "error": format!("`{key}` is not a known setting") }),
            ));
        }
    }

    // Merge the candidates over the stored values and try to build a config.
    let (mut tree, mut sources) = crate::config::store::stored_tree(&state.db)
        .map_err(|err| ApiError::internal(format!("{err:#}")))?;
    for (key, value) in &request.values {
        crate::config::store::set_path(&mut tree, key, value.clone())
            .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;
    }
    // Environment overrides win over both stored and candidate values, exactly
    // as they do on a real save.
    crate::config::store::apply_env_from(std::env::vars(), &mut tree, &mut sources)
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;
    let result = serde_json::from_value::<crate::config::Config>(tree)
        .map_err(|err| err.to_string())
        .and_then(|mut config| {
            config
                .resolve_secrets()
                .and_then(|_| config.validate())
                .map_err(|err| format!("{err:#}"))
        });

    Ok(Json(match result {
        Ok(_) => json!({ "valid": true }),
        Err(error) => json!({ "valid": false, "error": error }),
    }))
}

/// Imports a legacy YAML config. `POST /api/settings/import`
pub async fn import(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Json(request): Json<ImportRequest>,
) -> ApiResult<Json<Value>> {
    principal.require(&[Capability::ConfigureSailplane])?;

    crate::config::import::import_str(&state.db, &request.yaml)
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;
    state
        .reload_settings()
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ImportRequest {
    yaml: String,
}

fn apply(state: &SharedState, values: &Map<String, Value>) -> Result<(), ApiError> {
    for (key, value) in values {
        let descriptor = crate::config::schema::descriptor(key)
            .ok_or_else(|| ApiError::bad_request(format!("`{key}` is not a known setting")))?;
        if value.is_null() {
            state.db.delete_setting(key).map_err(ApiError::Internal)?;
            continue;
        }
        let encoded = serde_json::to_string(value)
            .map_err(|err| ApiError::internal(format!("failed to encode `{key}`: {err}")))?;
        state
            .db
            .save_setting(key, &encoded, descriptor.secret)
            .map_err(ApiError::Internal)?;
    }
    Ok(())
}

fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cursor = value;
    for part in path.split('.') {
        cursor = cursor.get(part)?;
    }
    Some(cursor)
}
