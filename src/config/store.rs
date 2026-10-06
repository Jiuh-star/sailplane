//! Runtime settings store.
//!
//! Sailplane's configuration lives in the database, with the environment and
//! built-in defaults layered around it. A snapshot is swapped in place, so a
//! request that reads a value sees the latest save. Settings that need a
//! restart to take effect are marked in [`super::schema`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

use crate::db::Db;

use super::Config;

/// Where an effective setting came from, for the settings UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Default,
    Database,
    Environment,
}

/// A swap-able configuration snapshot. Cheap to clone.
#[derive(Clone)]
pub struct Settings {
    inner: Arc<RwLock<Arc<Config>>>,
    sources: Arc<RwLock<BTreeMap<String, Source>>>,
    /// The data directory as resolved at boot. It can come from an environment
    /// variable, so it is not always equal to `config.server.data_path`.
    data_path: Option<PathBuf>,
}

impl Settings {
    /// Loads settings, importing a legacy YAML file when the store is empty.
    ///
    /// `data_path`, when known, receives a copy of the one-time setup token so
    /// a container operator can read it from a mounted volume instead of the
    /// logs.
    pub fn bootstrap(db: &Db, legacy: Option<&Path>, data_path: Option<&Path>) -> Result<Self> {
        if db.count_settings()? == 0 {
            if let Some(path) = legacy {
                match super::import::import_file(db, path) {
                    Ok(()) => tracing::warn!(
                        "imported the deprecated config file at {} into the database; \
                         it can now be removed",
                        path.display()
                    ),
                    Err(err) => tracing::warn!(
                        "could not import the config file at {}: {err:#}",
                        path.display()
                    ),
                }
            }
        } else if legacy.is_some_and(Path::exists) {
            tracing::warn!("ignoring the config file; settings are now stored in the database");
        }

        // A cookie secret is required before any session can be signed. Generate
        // one on first boot rather than refusing to start.
        if db
            .load_settings()?
            .iter()
            .all(|row| row.key != "server.cookie_secret")
        {
            let secret = crate::util::random_token(24);
            db.save_setting("server.cookie_secret", &json_string(&secret), true)?;
        }

        // A one-time token guards the onboarding endpoints when they are reached
        // from off-host. It is printed once, at first boot.
        if db.get_setting("meta.setup_token")?.is_none() {
            let token = crate::util::random_token(24);
            db.save_setting("meta.setup_token", &json_string(&token), true)?;
            write_setup_token(data_path, &token);
            tracing::info!(
                "first-run setup token: {token} (use it at the setup page when not on localhost)"
            );
        }

        let (config, sources) = build(db)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(Arc::new(config))),
            sources: Arc::new(RwLock::new(sources)),
            data_path: data_path.map(Path::to_path_buf),
        })
    }

    /// Rebuilds the snapshot from the database and environment.
    pub fn reload(&self, db: &Db) -> Result<()> {
        let (config, sources) = build(db)?;
        *self.inner.write().expect("settings lock poisoned") = Arc::new(config);
        *self.sources.write().expect("sources lock poisoned") = sources;
        Ok(())
    }

    /// The current configuration.
    pub fn snapshot(&self) -> Arc<Config> {
        self.inner.read().expect("settings lock poisoned").clone()
    }

    /// The source of each leaf setting.
    pub fn sources(&self) -> BTreeMap<String, Source> {
        self.sources.read().expect("sources lock poisoned").clone()
    }

    /// The data directory resolved at boot, when one was known.
    pub fn data_path(&self) -> Option<&Path> {
        self.data_path.as_deref()
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

/// Name of the file holding the one-time setup token, beside the database.
pub const SETUP_TOKEN_FILE: &str = "setup-token";

/// Writes the one-time setup token beside the database, readable only by the
/// owner. Best-effort: a read-only or missing data directory must not stop the
/// server from starting, since the token is also printed to the log.
fn write_setup_token(data_path: Option<&Path>, token: &str) {
    let Some(directory) = data_path else { return };
    let path = directory.join(SETUP_TOKEN_FILE);
    if let Err(err) = std::fs::write(&path, format!("{token}\n")) {
        tracing::warn!("could not write {}: {err}", path.display());
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!("could not restrict {}: {err}", path.display());
        }
    }
}

/// Builds the configuration from defaults, then the database, then the
/// environment.
pub fn build(db: &Db) -> Result<(Config, BTreeMap<String, Source>)> {
    let (mut tree, mut sources) = stored_tree(db)?;
    apply_env(&mut tree, &mut sources)?;

    // Anything not stored or overridden is a default, so the UI can explain
    // where each value comes from.
    for descriptor in super::schema::SETTINGS_SCHEMA {
        sources
            .entry(descriptor.key.to_string())
            .or_insert(Source::Default);
    }

    let mut config: Config =
        serde_json::from_value(tree).context("settings do not match the configuration schema")?;
    config.resolve_secrets()?;
    config.validate()?;

    Ok((config, sources))
}

/// The default configuration tree overlaid with the stored settings, plus the
/// source of each stored value.
///
/// `meta.*` rows track deployment state (onboarding, import time) and are not
/// part of the configuration schema.
pub fn stored_tree(db: &Db) -> Result<(Value, BTreeMap<String, Source>)> {
    let mut tree = serde_json::to_value(Config::default())
        .context("failed to serialise the default configuration")?;
    let mut sources = BTreeMap::new();

    for row in db.load_settings()? {
        if row.key.starts_with("meta.") {
            continue;
        }
        let value: Value = serde_json::from_str(&row.value)
            .with_context(|| format!("setting `{}` is not valid JSON", row.key))?;
        set_path(&mut tree, &row.key, value)
            .with_context(|| format!("setting `{}` has an invalid path", row.key))?;
        sources.insert(row.key, Source::Database);
    }

    Ok((tree, sources))
}

/// Sets a dotted path (`server.port`) in a JSON object tree.
pub fn set_path(tree: &mut Value, path: &str, value: Value) -> Result<()> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.iter().any(|part| part.is_empty()) {
        anyhow::bail!("empty path segment");
    }
    let mut cursor = tree;
    for (index, part) in parts.iter().enumerate() {
        if index + 1 == parts.len() {
            let object = cursor
                .as_object_mut()
                .context("the parent of a setting must be an object")?;
            object.insert((*part).to_string(), value);
            return Ok(());
        }
        let object = cursor
            .as_object_mut()
            .context("the parent of a setting must be an object")?;
        cursor = object
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Default::default()));
        if cursor.is_null() {
            *cursor = Value::Object(Default::default());
        }
    }
    Ok(())
}

/// Applies `SAILPLANE_<SECTION>__<KEY>` overrides to the JSON tree. A double
/// underscore separates nesting; single underscores stay in the key.
fn apply_env(tree: &mut Value, sources: &mut BTreeMap<String, Source>) -> Result<()> {
    apply_env_from(std::env::vars(), tree, sources)
}

/// Applies overrides from an arbitrary variable source. Production passes the
/// process environment; tests pass their own, so they do not have to mutate
/// global state.
pub fn apply_env_from<I>(
    vars: I,
    tree: &mut Value,
    sources: &mut BTreeMap<String, Source>,
) -> Result<()>
where
    I: IntoIterator<Item = (String, String)>,
{
    for (key, raw) in vars {
        let Some(rest) = key.strip_prefix("SAILPLANE_") else {
            continue;
        };
        if !rest.contains("__") {
            continue;
        }
        let path: Vec<String> = rest
            .split("__")
            .map(|part| part.to_ascii_lowercase())
            .collect();
        if path.len() < 2 || path.iter().any(String::is_empty) {
            continue;
        }

        let parsed: Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| Value::String(raw.clone()));
        let dotted = path.join(".");
        set_path(tree, &dotted, parsed)?;
        sources.insert(dotted, Source::Environment);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn bootstrap_generates_the_required_secrets() {
        let db = Db::open_in_memory().unwrap();
        let settings = Settings::bootstrap(&db, None, None).unwrap();

        let config = settings.snapshot();
        assert_eq!(config.cookie_secret().chars().count(), 32);
        assert!(db.get_setting("meta.setup_token").unwrap().is_some());
    }

    #[test]
    fn bootstrap_writes_the_setup_token_beside_the_database() {
        let db = Db::open_in_memory().unwrap();
        let directory = tempfile::tempdir().unwrap();
        Settings::bootstrap(&db, None, Some(directory.path())).unwrap();

        let stored = db.get_setting("meta.setup_token").unwrap().unwrap();
        let token: String = serde_json::from_str(&stored).unwrap();
        let path = directory.path().join(SETUP_TOKEN_FILE);
        assert_eq!(std::fs::read_to_string(path).unwrap().trim(), token);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.path().join(SETUP_TOKEN_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn environment_overrides_are_applied_with_types() {
        let mut tree = serde_json::to_value(Config::default()).unwrap();
        let mut sources = BTreeMap::new();
        apply_env_from(
            [
                ("SAILPLANE_SERVER__PORT".to_string(), "9999".to_string()),
                (
                    "SAILPLANE_SERVER__COOKIE_SECURE".to_string(),
                    "false".to_string(),
                ),
                // A bare `SAILPLANE_*` variable is handled elsewhere, not here.
                ("SAILPLANE_DEBUG".to_string(), "1".to_string()),
            ],
            &mut tree,
            &mut sources,
        )
        .unwrap();

        assert_eq!(tree["server"]["port"], 9999);
        assert_eq!(tree["server"]["cookie_secure"], false);
        assert_eq!(sources.get("server.port"), Some(&Source::Environment));
        assert!(!sources.contains_key("debug"));
    }

    #[test]
    fn database_values_override_defaults() {
        let db = Db::open_in_memory().unwrap();
        let settings = Settings::bootstrap(&db, None, None).unwrap();
        // Defaults, then a stored value.
        assert_eq!(settings.snapshot().server.port, 3000);
        db.save_setting("server.port", "1234", false).unwrap();
        settings.reload(&db).unwrap();

        assert_eq!(settings.snapshot().server.port, 1234);
        assert_eq!(
            settings.sources().get("server.port"),
            Some(&Source::Database)
        );
        assert_eq!(
            settings.sources().get("server.host"),
            Some(&Source::Default)
        );
    }

    #[test]
    fn imports_a_legacy_yaml_file() {
        let db = Db::open_in_memory().unwrap();
        let yaml = r#"
server:
  cookie_secret: "0123456789012345678901234567890a"
  port: 8080
headscale:
  url: http://headscale:8080
"#;
        crate::config::import::import_str(&db, yaml).unwrap();
        let (config, sources) = build(&db).unwrap();

        assert_eq!(config.server.port, 8080);
        assert_eq!(config.headscale.url, "http://headscale:8080");
        assert_eq!(sources.get("headscale.url"), Some(&Source::Database));
        assert!(db.get_setting("meta.config_imported_at").unwrap().is_some());
    }

    /// `meta.*` rows track deployment state and must not reach the config
    /// schema, which rejects unknown keys.
    #[test]
    fn meta_rows_are_ignored_by_the_config_build() {
        let db = Db::open_in_memory().unwrap();
        let settings = Settings::bootstrap(&db, None, None).unwrap();
        db.save_setting("meta.onboarding_completed_at", "\"now\"", false)
            .unwrap();
        settings.reload(&db).unwrap();
        assert_eq!(settings.snapshot().server.port, 3000);
    }
}
