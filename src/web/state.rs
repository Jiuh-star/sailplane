//! Shared application state and request-scoped extraction.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::agent::AgentService;
use crate::auth::{AuthService, Principal};
use crate::config::Config;
use crate::config::store::Settings;
use crate::db::Db;
use crate::headscale::{Headscale, LiveStore};
use crate::hsconfig::HeadscaleConfigFile;
use crate::integrations::Integration;

use super::error::ApiError;

/// Everything a handler needs.
pub struct AppState {
    /// Runtime configuration. Read a fresh snapshot per request so a saved
    /// setting takes effect without a restart.
    pub settings: Settings,
    /// The base path, fixed for the process lifetime.
    pub prefix: String,
    pub db: Db,
    pub auth: AuthService,
    pub headscale: Headscale,
    pub live: Arc<LiveStore>,
    pub hsconfig: HeadscaleConfigFile,
    pub integration: Integration,
    pub agent: AgentService,
    pub ssh: crate::ssh::SshService,
    pub oidc: Option<Arc<crate::auth::oidc::OidcProvider>>,
    /// Flips to `true` when the process is shutting down.
    ///
    /// The event feed, the log tail and the terminal hold a connection open
    /// indefinitely, and graceful shutdown waits for them, so they watch this
    /// to let the process exit.
    pub shutdown: tokio::sync::watch::Receiver<bool>,
}

impl AppState {
    /// The current configuration snapshot.
    pub fn config(&self) -> Arc<Config> {
        self.settings.snapshot()
    }

    /// Rebuilds the settings snapshot and repoints the Headscale client at the
    /// possibly-changed URL, so a saved setting takes effect immediately rather
    /// than on the next poll.
    pub fn reload_settings(&self) -> anyhow::Result<()> {
        self.settings.reload(&self.db)?;
        self.headscale.set_base_url(&self.config().headscale.url);
        Ok(())
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Public URL of Sailplane itself, without the base path. Builds the OIDC
    /// redirect URIs and the sign-in redirects.
    ///
    /// Falls back to the Headscale URL when `server.base_url` is unset, which
    /// matches single-domain deployments.
    pub fn public_base(&self) -> String {
        let config = self.config();
        config
            .server
            .base_url
            .clone()
            .unwrap_or_else(|| config.headscale.resolved_public_url())
            .trim_end_matches('/')
            .to_string()
    }

    /// Public URL of the Headscale instance. Registration commands point
    /// machines here.
    pub fn headscale_public_base(&self) -> String {
        self.config().headscale.resolved_public_url()
    }

    /// The API key every server-initiated Headscale call should use.
    pub fn admin_api_key(&self) -> Option<String> {
        self.config().headscale.api_key.clone()
    }

    /// The cookie signing secret, owned so it outlives a snapshot borrow.
    pub fn cookie_secret(&self) -> String {
        self.config().cookie_secret().to_string()
    }

    /// An API client authenticated as the given principal.
    ///
    /// API-key sessions use their own key; everyone else uses the configured
    /// admin key, exactly like upstream.
    pub fn client_for(&self, principal: &Principal) -> Option<crate::headscale::ApiClient> {
        match principal {
            Principal::ApiKey { api_key, .. } => Some(self.headscale.client(api_key.clone())),
            _ => self.admin_api_key().map(|key| self.headscale.client(key)),
        }
    }

    /// The client used for server-initiated work (live store, policy reads).
    pub fn admin_client(&self) -> Option<crate::headscale::ApiClient> {
        self.admin_api_key().map(|key| self.headscale.client(key))
    }

    /// Facts for evaluating dynamic autogroups.
    ///
    /// `autogroup:admin` maps to Sailplane owner/admin accounts linked to a
    /// Headscale user. Headscale has no admin user concept, so this is the
    /// closest available meaning.
    pub async fn eval_context(&self) -> crate::acl::eval::EvalContext {
        let users = match self.db.run(crate::db::list_users).await {
            Ok(users) => users,
            Err(err) => {
                tracing::warn!("could not read accounts for autogroup:admin: {err:#}");
                return crate::acl::eval::EvalContext::default();
            }
        };

        let admins = users
            .into_iter()
            .filter(|user| {
                matches!(
                    user.role,
                    crate::auth::Role::Owner | crate::auth::Role::Admin
                )
            })
            .filter(|user| user.headscale_user_id.is_some())
            .filter_map(|user| user.name)
            .collect();

        crate::acl::eval::EvalContext { admins }
    }
}

pub type SharedState = Arc<AppState>;

/// An authenticated principal.
pub struct Auth(pub Principal);

/// A principal, when one is present.
pub struct MaybeAuth(pub Option<Principal>);

/// Resolves the principal for a request: proxy auth first, then the session
/// cookie. This matches the upstream precedence.
pub async fn resolve_principal(
    state: &AppState,
    cookie: Option<&str>,
    peer: Option<IpAddr>,
    headers: &axum::http::HeaderMap,
) -> Option<Principal> {
    let config = state.config();
    if let Some(proxy_config) = config
        .server
        .proxy_auth
        .as_ref()
        .filter(|config| config.enabled)
        && let Some(peer) = peer
        && let Some(identity) = crate::auth::proxy::resolve(proxy_config, peer, |name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        })
    {
        match state
            .auth
            .find_or_create_user(
                &identity.subject,
                identity.name.as_deref(),
                identity.email.as_deref(),
                identity.picture.as_deref(),
                config
                    .oidc
                    .as_ref()
                    .map(|oidc| crate::auth::Role::parse(&oidc.default_role))
                    .unwrap_or_default(),
                None,
            )
            .await
        {
            Ok(user) => return Some(Principal::Proxy { user }),
            Err(err) => {
                tracing::error!("proxy auth could not resolve a user: {err:#}");
                return None;
            }
        }
    }

    state.auth.resolve_session(cookie).await.ok().flatten()
}

impl FromRequestParts<SharedState> for MaybeAuth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let cookie = crate::auth::session::read_cookie(
            parts
                .headers
                .get(axum::http::header::COOKIE)
                .and_then(|v| v.to_str().ok()),
            crate::auth::session::SESSION_COOKIE,
        );
        let peer = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|info| info.0.ip());

        Ok(Self(
            resolve_principal(state, cookie.as_deref(), peer, &parts.headers).await,
        ))
    }
}

impl FromRequestParts<SharedState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let MaybeAuth(principal) = MaybeAuth::from_request_parts(parts, state).await?;
        principal
            .map(Auth)
            .ok_or_else(|| ApiError::unauthorized("You must sign in to continue"))
    }
}

/// Capability checks that produce an [`ApiError`], so route handlers can use
/// `?` directly.
pub trait PrincipalExt {
    fn require(&self, capabilities: &[crate::auth::Capability]) -> Result<(), ApiError>;
}

impl PrincipalExt for Principal {
    fn require(&self, capabilities: &[crate::auth::Capability]) -> Result<(), ApiError> {
        if self.has_all(capabilities) {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "Your account does not have permission to do that",
            ))
        }
    }
}
