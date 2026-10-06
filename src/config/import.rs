//! Imports a legacy YAML config file into the settings store.
//!
//! One-time migration aid: an existing deployment keeps working after upgrade
//! without hand-editing the database. The file is read, validated against the
//! same schema, then written as flattened setting rows.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::db::Db;

use super::Config;

/// Reads `path` and writes every leaf as a setting. Fails if the YAML does not
/// match the configuration schema, so a typo is reported rather than imported.
pub fn import_file(db: &Db, path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    import_str(db, &raw)
}

/// Imports YAML text. Used by the settings API for an uploaded file.
pub fn import_str(db: &Db, raw: &str) -> Result<()> {
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(raw).context("the config file is not valid YAML")?;

    // Validate against the schema before storing anything.
    serde_yaml_ng::from_value::<Config>(yaml.clone())
        .context("the config file does not match the sailplane schema")?;

    let json = serde_json::to_value(&yaml).context("failed to normalise the config file")?;
    let mut leaves = Vec::new();
    flatten("", &json, &mut leaves);

    for (key, value) in leaves {
        let encoded = serde_json::to_string(&value).context("failed to encode a setting")?;
        db.save_setting(&key, &encoded, is_secret(&key))?;
    }

    // Record the migration so the deprecation warning is not repeated blindly
    // and the UI can explain where the values came from.
    let imported_at = serde_json::to_string(&chrono::Utc::now().to_rfc3339())
        .context("failed to encode the import timestamp")?;
    db.save_setting("meta.config_imported_at", &imported_at, false)?;
    Ok(())
}

/// Collects every scalar or array under `prefix` as a dotted leaf.
fn flatten(prefix: &str, value: &Value, leaves: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&path, child, leaves);
            }
        }
        // `null` means "unset"; the default applies instead.
        Value::Null => {}
        other => leaves.push((prefix.to_string(), other.clone())),
    }
}

/// Keys whose value is a secret, so the UI masks them and never returns them.
/// Secret-ness is declared once, on the schema descriptor.
fn is_secret(key: &str) -> bool {
    crate::config::schema::descriptor(key).is_some_and(|entry| entry.secret)
}
