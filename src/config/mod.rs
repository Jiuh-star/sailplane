//! Configuration loading for Sailplane.
//!
//! Mirrors the upstream YAML schema: a config file (default `/etc/sailplane/config.yaml`,
//! overridable with `SAILPLANE_CONFIG_PATH`), `SAILPLANE_<SECTION>__<KEY>` environment
//! overrides, and secrets given inline or read from a file with a `*_path` key.

pub mod import;
pub mod integration;
pub mod oidc;
pub mod schema;
pub mod server;
pub mod store;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub use integration::{
    AgentBackend, AgentConfig, DockerConfig, IntegrationConfig, KubernetesConfig, SshConfig,
};
pub use oidc::{OidcConfig, ProfilePictureSource, TokenEndpointAuthMethod};
pub use server::{ProxyAuthConfig, ServerConfig};

/// The fully resolved Sailplane configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

impl Config {
    /// The agent configuration with deployment defaults filled in.
    ///
    /// Derived from a snapshot rather than stored, so a saved setting rebuilds
    /// it without a restart.
    pub fn agent_config(&self) -> AgentConfig {
        let mut config = self
            .integration
            .as_ref()
            .and_then(|integration| integration.agent.clone())
            .unwrap_or_else(|| AgentConfig {
                enabled: false,
                host_name: "sailplane-agent".into(),
                cache_ttl: 180_000,
                backend: AgentBackend::System,
                socket: None,
                executable_path: PathBuf::from("/usr/libexec/sailplane/agent"),
                work_dir: self.server.data_path.join("agent"),
                tailscale_netns: true,
            });
        if config.socket.is_none() {
            config.socket = crate::agent::tailscale::default_socket();
        }
        config
    }

    /// The browser-SSH configuration. It defaults to disabled.
    pub fn ssh_config(&self) -> SshConfig {
        self.integration
            .as_ref()
            .and_then(|integration| integration.ssh.clone())
            .unwrap_or_default()
    }
}

/// How Sailplane reaches the Headscale HTTP API and, optionally, the Headscale
/// configuration file on disk.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadscaleConfig {
    /// Internal API base URL, for example `http://headscale:8080`.
    pub url: String,

    /// Public URL shown in the UI and used in registration commands. Defaults
    /// to `url` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,

    /// Headscale API key. Necessary for OIDC, proxy auth and the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_path: Option<PathBuf>,

    /// Path to the Headscale config file. When absent, config-backed features
    /// (DNS, authentication restrictions) are disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<PathBuf>,

    /// Deprecated upstream. Accepted and ignored.
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

impl Default for HeadscaleConfig {
    fn default() -> Self {
        Self {
            // A placeholder that validation accepts before onboarding fills in
            // the real address. The UI treats an unreachable Headscale as
            // "not configured".
            url: "http://127.0.0.1:8080".into(),
            public_url: None,
            api_key: None,
            api_key_path: None,
            config_path: None,
            config_strict: true,
            dns_records_path: None,
            tls_cert_path: None,
        }
    }
}

impl HeadscaleConfig {
    /// Returns the effective public URL with any trailing slash removed. It
    /// falls back to the internal URL when unset.
    pub fn resolved_public_url(&self) -> String {
        self.public_url
            .clone()
            .unwrap_or_else(|| self.url.clone())
            .trim_end_matches('/')
            .to_string()
    }

    pub fn resolved_api_key(&self) -> Result<Option<String>> {
        resolve_secret(
            self.api_key.as_deref(),
            self.api_key_path.as_deref(),
            "headscale.api_key",
        )
    }
}

/// Reads a secret from an inline value or a `*_path` file. Setting both is an
/// error.
pub fn resolve_secret(
    inline: Option<&str>,
    path: Option<&Path>,
    field: &str,
) -> Result<Option<String>> {
    match (inline, path) {
        (Some(_), Some(_)) => bail!("cannot set both `{field}` and `{field}_path`. Choose one"),
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
    pub(crate) fn resolve_secrets(&mut self) -> Result<()> {
        self.server.resolve_secrets()?;
        self.headscale.api_key = self.headscale.resolved_api_key()?;
        self.headscale.api_key_path = None;
        if let Some(oidc) = self.oidc.as_mut() {
            oidc.resolve_secrets()?;
        }
        Ok(())
    }

    pub(crate) fn validate(&mut self) -> Result<()> {
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

    /// Returns the resolved cookie secret, which `ServerConfig` validates to 32
    /// characters.
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

    #[test]
    fn secret_conflict_is_rejected() {
        let err = resolve_secret(
            Some("abc"),
            Some(Path::new("/tmp/x")),
            "server.cookie_secret",
        )
        .unwrap_err();
        assert!(err.to_string().contains("cannot set both"));
    }
}
