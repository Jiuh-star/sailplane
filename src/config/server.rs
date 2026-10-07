//! `server.*` configuration: listener, cookies, TLS and proxy auth.

use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::resolve_secret;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,

    /// Public URL of the deployment, excluding the base path. Necessary for
    /// OIDC redirect URIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// URL path prefix the app is served under.
    #[serde(default = "default_base_path")]
    pub base_path: String,

    #[serde(default = "default_data_path")]
    pub data_path: PathBuf,

    /// Protects `GET /api/info`. The endpoint is disabled when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_secret: Option<String>,

    /// Exactly 32 characters. Signs session cookies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_secret: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_secret_path: Option<PathBuf>,

    #[serde(default = "default_true")]
    pub cookie_secure: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_domain: Option<String>,

    /// Session cookie lifetime in seconds.
    #[serde(default = "default_cookie_max_age")]
    pub cookie_max_age: i64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_cert_path: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_key_path: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_auth: Option<ProxyAuthConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyAuthConfig {
    pub enabled: bool,

    /// CIDRs that can authenticate. Defaults to loopback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_cidrs: Option<Vec<String>>,

    /// Peers whose `ip_header` can be trusted. Defaults to loopback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trusted_proxy_cidrs: Option<Vec<String>>,

    /// Header carrying the client IP when behind another proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_header: Option<String>,

    #[serde(default = "default_user_header")]
    pub user_header: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_header: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_header: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_header: Option<String>,
}

impl ProxyAuthConfig {
    /// Returns `allowed_cidrs`, or loopback when unset.
    pub fn allowed_cidrs(&self) -> Vec<String> {
        self.allowed_cidrs
            .clone()
            .unwrap_or_else(default_loopback_cidrs)
    }

    /// Returns `trusted_proxy_cidrs`, or loopback when unset.
    pub fn trusted_proxy_cidrs(&self) -> Vec<String> {
        self.trusted_proxy_cidrs
            .clone()
            .unwrap_or_else(default_loopback_cidrs)
    }
}

fn default_loopback_cidrs() -> Vec<String> {
    vec!["127.0.0.1/32".into(), "::1/128".into()]
}

fn default_host() -> String {
    "0.0.0.0".into()
}

fn default_port() -> u16 {
    3000
}

fn default_base_path() -> String {
    "/admin".into()
}

fn default_data_path() -> PathBuf {
    PathBuf::from("/var/lib/sailplane/")
}

fn default_cookie_max_age() -> i64 {
    86400
}

fn default_user_header() -> String {
    "Remote-User".into()
}

fn default_true() -> bool {
    true
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            base_url: None,
            base_path: default_base_path(),
            data_path: default_data_path(),
            info_secret: None,
            cookie_secret: None,
            cookie_secret_path: None,
            cookie_secure: true,
            cookie_domain: None,
            cookie_max_age: default_cookie_max_age(),
            tls_cert_path: None,
            tls_key_path: None,
            proxy_auth: None,
        }
    }
}

impl ServerConfig {
    pub(super) fn resolve_secrets(&mut self) -> Result<()> {
        self.cookie_secret = resolve_secret(
            self.cookie_secret.as_deref(),
            self.cookie_secret_path.as_deref(),
            "server.cookie_secret",
        )?;
        self.cookie_secret_path = None;
        Ok(())
    }

    pub(super) fn validate(&mut self) -> Result<()> {
        let Some(secret) = self.cookie_secret.as_deref() else {
            bail!("server.cookie_secret is necessary (32 characters)");
        };
        if secret.chars().count() != 32 {
            bail!(
                "server.cookie_secret must be exactly 32 characters, got {}",
                secret.chars().count()
            );
        }

        // Normalizes the base path: leading slash, no trailing slash.
        let mut base = self.base_path.trim().to_string();
        if base.is_empty() || base == "/" {
            base = String::new();
        } else {
            if !base.starts_with('/') {
                base.insert(0, '/');
            }
            base = base.trim_end_matches('/').to_string();
        }
        self.base_path = base;

        if self.tls_cert_path.is_some() != self.tls_key_path.is_some() {
            bail!("set server.tls_cert_path and server.tls_key_path together");
        }
        if self.tls_cert_path.is_some() {
            self.cookie_secure = true;
        }

        if let Some(domain) = self.cookie_domain.clone() {
            self.cookie_domain = Some(domain.to_ascii_lowercase());
        }

        if self.cookie_max_age <= 0 {
            bail!("server.cookie_max_age must be positive");
        }

        Ok(())
    }

    /// Returns the base path with no trailing slash. Empty means served at the root.
    pub fn base_path(&self) -> &str {
        &self.base_path
    }
    pub fn tls_enabled(&self) -> bool {
        self.tls_cert_path.is_some() && self.tls_key_path.is_some()
    }
}
