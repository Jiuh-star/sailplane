//! `oidc.*` configuration: OpenID Connect login provider settings.

use std::path::PathBuf;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use super::resolve_secret;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OidcConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Issuer used for discovery and `iss` validation.
    pub issuer: String,

    pub client_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_path: Option<PathBuf>,

    /// Deprecated upstream fallback for `headscale.api_key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headscale_api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_endpoint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userinfo_endpoint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks_endpoint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<String>,

    #[serde(default)]
    pub use_end_session: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_logout_redirect_uri: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<TokenEndpointAuthMethod>,

    #[serde(default)]
    pub use_pkce: bool,

    #[serde(default)]
    pub disable_api_key_login: bool,

    #[serde(default = "default_scope")]
    pub scope: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject_claims: Vec<String>,

    #[serde(default = "default_role")]
    pub default_role: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_claim: Option<String>,

    #[serde(default)]
    pub allow_weak_rsa_keys: bool,

    #[serde(default)]
    pub profile_picture_source: ProfilePictureSource,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_params: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TokenEndpointAuthMethod {
    ClientSecretBasic,
    ClientSecretPost,
    ClientSecretJwt,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePictureSource {
    #[default]
    Oidc,
    Gravatar,
}

fn default_true() -> bool {
    true
}

fn default_scope() -> String {
    "openid email profile".into()
}

fn default_role() -> String {
    "member".into()
}

impl OidcConfig {
    pub(super) fn resolve_secrets(&mut self) -> Result<()> {
        self.client_secret = resolve_secret(
            self.client_secret.as_deref(),
            self.client_secret_path.as_deref(),
            "oidc.client_secret",
        )?;
        self.client_secret_path = None;

        let mut seen = Vec::new();
        for claim in std::mem::take(&mut self.subject_claims) {
            let claim = claim.trim().to_string();
            if !claim.is_empty() && !seen.contains(&claim) {
                seen.push(claim);
            }
        }
        self.subject_claims = seen;

        Ok(())
    }

    pub(super) fn validate(&mut self, base_url: Option<&str>) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        if self.issuer.is_empty() {
            bail!("oidc.issuer is required");
        }
        self.issuer = self.issuer.trim_end_matches('/').to_string();

        if self.client_id.is_empty() {
            bail!("oidc.client_id is required");
        }
        if self.client_secret.as_deref().unwrap_or("").is_empty() {
            bail!("oidc.client_secret is required (or client_secret_path)");
        }

        // The authorization code flow requires the redirect URI; it comes from
        // `server.base_url` when not pinned explicitly.
        if base_url.is_none() && self.post_logout_redirect_uri.is_none() {
            tracing::warn!(
                "oidc is enabled but server.base_url is not set; \
                 the redirect URI will fall back to the request host"
            );
        }

        Ok(())
    }
    /// Returns the post-logout redirect, defaulting to `<base_url><prefix>/login?s=logout`.
    pub fn post_logout_redirect(&self, base_url: &str, base_path: &str) -> String {
        self.post_logout_redirect_uri.clone().unwrap_or_else(|| {
            format!(
                "{}{}/login?s=logout",
                base_url.trim_end_matches('/'),
                base_path
            )
        })
    }
}
