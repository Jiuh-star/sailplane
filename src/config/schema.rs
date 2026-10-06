//! Descriptors for the settings UI.
//!
//! Each leaf Sailplane stores is described once: how to render it, whether it
//! is a secret, and whether it needs a restart. The settings endpoint serves
//! this list so the form and the validation stay in step with the config
//! model.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Number,
    Bool,
    Path,
    List,
    Url,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SettingDescriptor {
    pub key: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    pub secret: bool,
    /// True when the value only takes effect after a restart.
    pub restart_required: bool,
}

const fn setting(
    key: &'static str,
    group: &'static str,
    kind: Kind,
    secret: bool,
    restart_required: bool,
) -> SettingDescriptor {
    SettingDescriptor {
        key,
        group,
        kind,
        secret,
        restart_required,
    }
}

/// Every setting the UI edits directly. Settings absent here are still stored
/// and served; the UI shows them in a raw JSON editor.
pub const SETTINGS_SCHEMA: &[SettingDescriptor] = &[
    // --- Server ---
    setting("server.host", "server", Kind::Text, false, true),
    setting("server.port", "server", Kind::Number, false, true),
    setting("server.base_url", "server", Kind::Url, false, false),
    setting("server.base_path", "server", Kind::Text, false, true),
    setting("server.data_path", "server", Kind::Path, false, true),
    setting("server.info_secret", "server", Kind::Text, true, false),
    setting("server.cookie_secret", "server", Kind::Text, true, false),
    setting(
        "server.cookie_secret_path",
        "server",
        Kind::Path,
        false,
        false,
    ),
    setting("server.cookie_secure", "server", Kind::Bool, false, false),
    setting("server.cookie_domain", "server", Kind::Text, false, false),
    setting(
        "server.cookie_max_age",
        "server",
        Kind::Number,
        false,
        false,
    ),
    setting("server.tls_cert_path", "server", Kind::Path, false, true),
    setting("server.tls_key_path", "server", Kind::Path, false, true),
    // --- Headscale ---
    setting("headscale.url", "headscale", Kind::Url, false, true),
    setting("headscale.public_url", "headscale", Kind::Url, false, false),
    setting("headscale.api_key", "headscale", Kind::Text, true, false),
    setting(
        "headscale.api_key_path",
        "headscale",
        Kind::Path,
        false,
        false,
    ),
    setting(
        "headscale.config_path",
        "headscale",
        Kind::Path,
        false,
        true,
    ),
    setting(
        "headscale.dns_records_path",
        "headscale",
        Kind::Path,
        false,
        true,
    ),
    setting(
        "headscale.tls_cert_path",
        "headscale",
        Kind::Path,
        false,
        true,
    ),
    // --- OIDC (Sailplane's own login provider) ---
    setting("oidc.enabled", "oidc", Kind::Bool, false, true),
    setting("oidc.issuer", "oidc", Kind::Url, false, true),
    setting("oidc.client_id", "oidc", Kind::Text, false, true),
    setting("oidc.client_secret", "oidc", Kind::Text, true, true),
    setting("oidc.client_secret_path", "oidc", Kind::Path, false, true),
    setting("oidc.headscale_api_key", "oidc", Kind::Text, true, false),
    setting("oidc.scope", "oidc", Kind::Text, false, true),
    setting("oidc.use_pkce", "oidc", Kind::Bool, false, true),
    setting(
        "oidc.disable_api_key_login",
        "oidc",
        Kind::Bool,
        false,
        false,
    ),
    setting("oidc.default_role", "oidc", Kind::Text, false, false),
    setting("oidc.role_claim", "oidc", Kind::Text, false, true),
    setting("oidc.subject_claims", "oidc", Kind::List, false, true),
    setting("oidc.use_end_session", "oidc", Kind::Bool, false, true),
    setting(
        "oidc.post_logout_redirect_uri",
        "oidc",
        Kind::Text,
        false,
        true,
    ),
    setting(
        "oidc.token_endpoint_auth_method",
        "oidc",
        Kind::Text,
        false,
        true,
    ),
    setting("oidc.allow_weak_rsa_keys", "oidc", Kind::Bool, false, true),
    setting(
        "oidc.profile_picture_source",
        "oidc",
        Kind::Text,
        false,
        true,
    ),
    // --- Integrations ---
    setting(
        "integration.docker.enabled",
        "integration",
        Kind::Bool,
        false,
        true,
    ),
    setting(
        "integration.docker.container_name",
        "integration",
        Kind::Text,
        false,
        true,
    ),
    setting(
        "integration.docker.container_label",
        "integration",
        Kind::Text,
        false,
        true,
    ),
    setting(
        "integration.docker.socket",
        "integration",
        Kind::Text,
        false,
        true,
    ),
    setting(
        "integration.kubernetes.enabled",
        "integration",
        Kind::Bool,
        false,
        true,
    ),
    setting(
        "integration.kubernetes.pod_name",
        "integration",
        Kind::Text,
        false,
        true,
    ),
    setting(
        "integration.kubernetes.validate_manifest",
        "integration",
        Kind::Bool,
        false,
        true,
    ),
    setting(
        "integration.proc.enabled",
        "integration",
        Kind::Bool,
        false,
        true,
    ),
    setting(
        "integration.agent.enabled",
        "agent",
        Kind::Bool,
        false,
        true,
    ),
    setting(
        "integration.agent.host_name",
        "agent",
        Kind::Text,
        false,
        true,
    ),
    setting(
        "integration.agent.cache_ttl",
        "agent",
        Kind::Number,
        false,
        true,
    ),
    setting(
        "integration.agent.backend",
        "agent",
        Kind::Text,
        false,
        true,
    ),
    setting("integration.agent.socket", "agent", Kind::Path, false, true),
    setting(
        "integration.agent.executable_path",
        "agent",
        Kind::Path,
        false,
        true,
    ),
    setting(
        "integration.agent.work_dir",
        "agent",
        Kind::Path,
        false,
        true,
    ),
    setting(
        "integration.agent.tailscale_netns",
        "agent",
        Kind::Bool,
        false,
        true,
    ),
    setting("integration.ssh.enabled", "ssh", Kind::Bool, false, true),
    setting("integration.ssh.port", "ssh", Kind::Number, false, true),
    setting("integration.ssh.username", "ssh", Kind::Text, false, true),
    setting("integration.ssh.proxy", "ssh", Kind::Text, false, true),
    setting(
        "integration.ssh.private_key_path",
        "ssh",
        Kind::Path,
        false,
        true,
    ),
    setting("integration.ssh.password", "ssh", Kind::Text, true, true),
    // --- Advanced ---
    setting("debug", "advanced", Kind::Bool, false, false),
];

/// The descriptor for a key, when it is listed.
pub fn descriptor(key: &str) -> Option<&'static SettingDescriptor> {
    SETTINGS_SCHEMA.iter().find(|entry| entry.key == key)
}
