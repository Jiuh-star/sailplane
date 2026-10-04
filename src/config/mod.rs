//! Configuration loading for Sailplane.
//!
//! Mirrors the upstream YAML schema: a config file (default `/etc/sailplane/config.yaml`,
//! overridable with `SAILPLANE_CONFIG_PATH`), `SAILPLANE_<SECTION>__<KEY>` environment
//! overrides, and secrets given inline or read from a file with a `*_path` key.

pub mod integration;
pub mod oidc;
pub mod server;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub use integration::{
    AgentBackend, AgentConfig, DockerConfig, IntegrationConfig, KubernetesConfig,
    SshConfig,
};
pub use oidc::{OidcConfig, ProfilePictureSource, TokenEndpointAuthMethod};
pub use server::{ProxyAuthConfig, ServerConfig};

/// The fully resolved Sailplane configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub debug: bool,

    pub server: ServerConfig,

    pub headscale: HeadscaleConfig,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<IntegrationConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc: Option<OidcConfig>,
}

/// How Sailplane reaches the Headscale HTTP API and, optionally, the Headscale
/// configuration file on disk.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadscaleConfig {
    /// Internal API base URL, e.g. `http://headscale:8080`.
    pub url: String,

    /// Public URL shown in the UI and used in registration commands. Defaults
    /// to `url` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,

    /// Headscale API key. Required for OIDC, proxy auth and the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_path: Option<PathBuf>,

    /// Path to the Headscale config file. When absent, config-backed features
    /// (DNS, authentication restrictions) are disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<PathBuf>,

    /// Deprecated upstream; accepted and ignored.
    #[serde(default = "default_true")]
    pub config_strict: bool,

    /// Path of the JSON file backing `dns.extra_records_path`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_records_path: Option<PathBuf>,

    /// Pinned TLS certificate for the Headscale API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_cert_path: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl HeadscaleConfig {
    /// Returns the effective public URL, falling back to the internal URL, with any trailing slash
    /// removed.
    pub fn resolved_public_url(&self) -> String {
        self.public_url
            .clone()
            .unwrap_or_else(|| self.url.clone())
            .trim_end_matches('/')
            .to_string()
    }

    pub fn resolved_api_key(&self) -> Result<Option<String>> {
        resolve_secret(self.api_key.as_deref(), self.api_key_path.as_deref(), "headscale.api_key")
    }
}
/// Loads configuration from an explicit path. Used by tests and `--config`.
pub fn load_from(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;

    let mut value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("failed to parse config file at {}", path.display()))?;

    apply_env_overrides(&mut value)?;

    let mut config: Config = serde_yaml_ng::from_value(value)
        .context("config file does not match the sailplane schema")?;

    config.resolve_secrets()?;
    config.validate()?;

    Ok(config)
}

/// Applies `SAILPLANE_SECTION__KEY=value` overrides to the parsed YAML tree.
///
/// Nested keys use a double underscore; single underscores stay intact so `cookie_secret`
/// remains addressable. Values parse as YAML scalars, so `true` and `3000` keep their types.
pub fn apply_env_overrides(value: &mut serde_yaml_ng::Value) -> Result<()> {
    for (key, raw) in std::env::vars() {
        let Some(rest) = key.strip_prefix("SAILPLANE_") else {
            continue;
        };
        // Only vars with `__` are config overrides; bare SAILPLANE_* vars are
        // handled elsewhere.
        if !rest.contains("__") {
            continue;
        }

        let parts: Vec<String> = rest
            .split("__")
            .map(|p| p.to_ascii_lowercase())
            .collect();

        if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
            continue;
        }

        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&raw)
            .unwrap_or_else(|_| serde_yaml_ng::Value::String(raw.clone()));

        set_nested(value, &parts, parsed)?;
    }

    Ok(())
}

fn set_nested(
    value: &mut serde_yaml_ng::Value,
    path: &[String],
    new_value: serde_yaml_ng::Value,
) -> Result<()> {
    if path.len() == 1 {
        let map = value
            .as_mapping_mut()
            .context("config root must be a mapping")?;
        map.insert(
            serde_yaml_ng::Value::String(path[0].clone()),
            new_value,
        );
        return Ok(());
    }

    let map = value
        .as_mapping_mut()
        .context("config root must be a mapping")?;
    let key = serde_yaml_ng::Value::String(path[0].clone());
    if !map.contains_key(&key) {
        map.insert(key.clone(), serde_yaml_ng::Value::Mapping(Default::default()));
    }
    let child = map.get_mut(&key).expect("just inserted");
    if child.is_null() {
        *child = serde_yaml_ng::Value::Mapping(Default::default());
    }
    set_nested(child, &path[1..], new_value)
}

/// Reads a secret from an inline value or a `*_path` file. Setting both is an
/// error.
pub fn resolve_secret(
    inline: Option<&str>,
    path: Option<&Path>,
    field: &str,
) -> Result<Option<String>> {
    match (inline, path) {
        (Some(_), Some(_)) => bail!(
            "cannot set both `{field}` and `{field}_path`; choose one"
        ),
        (Some(value), None) => {
            if value.is_empty() {
                bail!("`{field}` is empty");
            }
            Ok(Some(value.to_string()))
        }
        (None, Some(path)) => {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read `{field}_path` at {}", path.display()))?;
            let trimmed = contents.trim();
            if trimmed.is_empty() {
                bail!("`{field}_path` at {} is empty", path.display());
            }
            Ok(Some(trimmed.to_string()))
        }
        (None, None) => Ok(None),
    }
}

impl Config {
    fn resolve_secrets(&mut self) -> Result<()> {
        self.server.resolve_secrets()?;
        self.headscale.api_key = self.headscale.resolved_api_key()?;
        self.headscale.api_key_path = None;
        if let Some(oidc) = self.oidc.as_mut() {
            oidc.resolve_secrets()?;
        }
        Ok(())
    }

    fn validate(&mut self) -> Result<()> {
        self.server.validate()?;

        if !self.headscale.url.starts_with("http://") && !self.headscale.url.starts_with("https://")
        {
            bail!("headscale.url must start with http:// or https://");
        }
        self.headscale.url = self.headscale.url.trim_end_matches('/').to_string();

        if let Some(public) = self.headscale.public_url.clone() {
            if !public.starts_with("http://") && !public.starts_with("https://") {
                bail!("headscale.public_url must start with http:// or https://");
            }
            self.headscale.public_url = Some(public.trim_end_matches('/').to_string());
        }

        if let Some(oidc) = self.oidc.as_mut() {
            oidc.validate(self.server.base_url.as_deref())?;
        }

        if let Some(integration) = self.integration.as_mut() {
            integration.validate()?;
        }

        Ok(())
    }
    pub fn debug_logging(&self) -> bool {
        self.debug || env_flag("SAILPLANE_DEBUG_LOG")
    }

    /// Returns the resolved cookie secret, validated to 32 characters by `ServerConfig`.
    pub fn cookie_secret(&self) -> &str {
        self.server
            .cookie_secret
            .as_deref()
            .expect("cookie_secret validated at load")
    }
}

pub fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "true" | "1" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_yaml() -> serde_yaml_ng::Value {
        serde_yaml_ng::from_str(
            r#"
server:
  host: 127.0.0.1
  port: 3000
  cookie_secret: "0123456789012345678901234567890a"
headscale:
  url: http://localhost:8080
"#,
        )
        .unwrap()
    }

    #[test]
    fn env_override_sets_nested_scalar_with_type() {
        // SAFETY: single-threaded test process section.
        unsafe {
            std::env::set_var("SAILPLANE_SERVER__PORT", "9999");
            std::env::set_var("SAILPLANE_SERVER__COOKIE_SECURE", "false");
        }
        let mut value = base_yaml();
        apply_env_overrides(&mut value).unwrap();
        let cfg: Config = serde_yaml_ng::from_value(value).unwrap();
        assert_eq!(cfg.server.port, 9999);
        assert!(!cfg.server.cookie_secure);
        unsafe {
            std::env::remove_var("SAILPLANE_SERVER__PORT");
            std::env::remove_var("SAILPLANE_SERVER__COOKIE_SECURE");
        }
    }

    #[test]
    fn secret_conflict_is_rejected() {
        let err = resolve_secret(Some("abc"), Some(Path::new("/tmp/x")), "server.cookie_secret")
            .unwrap_err();
        assert!(err.to_string().contains("cannot set both"));
    }
}
