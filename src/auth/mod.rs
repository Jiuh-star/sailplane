//! Authentication: session cookies, OIDC, proxy auth and the principal model.

pub mod oidc;
pub mod proxy;
pub mod roles;
pub mod session;

use anyhow::{Context, Result};

use crate::config::store::Settings;
use crate::db::{Db, SailplaneUser, Session, SessionKind};
use crate::headscale::Machine;

pub use roles::{Capability, CapabilitySet, Role};
pub use session::{CookieOptions, CookiePayload, OidcTransaction, SameSite};

/// The principal that makes the current request.
#[derive(Debug, Clone)]
pub enum Principal {
    /// Logged in with a Headscale API key. Sailplane uses the key for API calls,
    /// and every capability check passes.
    ApiKey {
        display_name: String,
        api_key: String,
    },

    /// Logged in through the OIDC provider.
    User {
        user: SailplaneUser,
        id_token: Option<String>,
    },

    /// Authenticated by a trusted reverse proxy. The request has no session row.
    Proxy { user: SailplaneUser },
}

impl Principal {
    /// Label for the account menu: the account name, or the API key prefix.
    pub fn display_name(&self) -> String {
        match self {
            Self::ApiKey { display_name, .. } => display_name.clone(),
            Self::User { user, .. } | Self::Proxy { user } => user.label().to_string(),
        }
    }

    pub fn sailplane_user(&self) -> Option<&SailplaneUser> {
        match self {
            Self::ApiKey { .. } => None,
            Self::User { user, .. } | Self::Proxy { user } => Some(user),
        }
    }

    pub fn id_token(&self) -> Option<&str> {
        match self {
            Self::User { id_token, .. } => id_token.as_deref(),
            _ => None,
        }
    }
    pub fn is_api_key(&self) -> bool {
        matches!(self, Self::ApiKey { .. })
    }

    /// Every capability held by this principal. API keys hold all of them.
    pub fn capabilities(&self) -> CapabilitySet {
        match self {
            Self::ApiKey { .. } => CapabilitySet::from_bits(Capability::ALL.0),
            Self::User { user, .. } | Self::Proxy { user } => user.role.capabilities(),
        }
    }
    pub fn has(&self, capability: Capability) -> bool {
        self.capabilities().contains(capability)
    }

    pub fn has_all(&self, capabilities: &[Capability]) -> bool {
        self.capabilities().contains_all(capabilities)
    }

    /// Reports whether this principal can modify the given node.
    ///
    /// API-key principals and `write_machines` holders can edit any node. Others
    /// can edit only nodes owned by their linked Headscale user.
    pub fn can_manage_node(&self, node: &Machine) -> bool {
        if self.has(Capability::WriteMachines) {
            return true;
        }
        let Some(user) = self.sailplane_user() else {
            return false;
        };
        let Some(linked) = user.headscale_user_id.as_deref() else {
            return false;
        };
        node.user.as_ref().is_some_and(|owner| owner.id == linked)
    }
    /// The Headscale user ID this principal can create self-service keys for.
    pub fn linked_headscale_user(&self) -> Option<&str> {
        self.sailplane_user()
            .and_then(|user| user.headscale_user_id.as_deref())
    }
}

/// Shared authentication service.
#[derive(Clone)]
pub struct AuthService {
    db: Db,
    settings: Settings,
}

impl AuthService {
    pub fn new(db: Db, settings: Settings) -> Self {
        Self { db, settings }
    }
    /// Cookie options for the session cookie.
    pub fn cookie_options(&self) -> CookieOptions {
        let config = self.settings.snapshot();
        CookieOptions {
            secure: config.server.cookie_secure,
            http_only: true,
            max_age_seconds: config.server.cookie_max_age,
            domain: config.server.cookie_domain.clone(),
            path: if config.server.base_path.is_empty() {
                "/".into()
            } else {
                config.server.base_path.clone()
            },
            same_site: SameSite::Lax,
        }
    }

    /// Resolves the principal for a session cookie value and prunes expired rows.
    pub async fn resolve_session(&self, cookie: Option<&str>) -> Result<Option<Principal>> {
        let Some(cookie) = cookie else {
            return Ok(None);
        };
        let config = self.settings.snapshot();
        let Some(payload) = session::decode_cookie(cookie, config.cookie_secret()) else {
            return Ok(None);
        };

        let db = self.db.clone();
        let sid = payload.sid.clone();
        let session = db
            .run(move |conn| {
                let session = conn
                    .query_row(
                        "SELECT * FROM auth_sessions WHERE id = ?1",
                        rusqlite::params![sid],
                        Session::from_row,
                    )
                    .ok();
                Ok(session)
            })
            .await?;

        let Some(session) = session else {
            return Ok(None);
        };

        if session.is_expired() {
            let db = self.db.clone();
            let sid = session.id.clone();
            db.run(move |conn| {
                conn.execute(
                    "DELETE FROM auth_sessions WHERE id = ?1",
                    rusqlite::params![sid],
                )?;
                Ok(())
            })
            .await?;
            return Ok(None);
        }

        match session.kind {
            SessionKind::ApiKey => {
                let Some(api_key) = payload.api_key else {
                    // The cookie carried no key: the session is unusable.
                    return Ok(None);
                };
                Ok(Some(Principal::ApiKey {
                    display_name: session
                        .api_key_display
                        .clone()
                        .unwrap_or_else(|| "API Key".into()),
                    api_key,
                }))
            }
            SessionKind::Oidc => {
                let Some(user_id) = session.user_id.clone() else {
                    return Ok(None);
                };
                let db = self.db.clone();
                let user = db
                    .run(move |conn| {
                        Ok(conn
                            .query_row(
                                "SELECT * FROM users WHERE id = ?1",
                                rusqlite::params![user_id],
                                SailplaneUser::from_row,
                            )
                            .ok())
                    })
                    .await?;

                let Some(user) = user else {
                    return Ok(None);
                };

                Ok(Some(Principal::User {
                    user,
                    id_token: session.oidc_id_token.clone(),
                }))
            }
        }
    }

    /// Finds or creates the Sailplane account behind an OIDC subject.
    pub async fn find_or_create_user(
        &self,
        subject: &str,
        name: Option<&str>,
        email: Option<&str>,
        picture: Option<&str>,
        initial_role: Role,
        sync_role: Option<Role>,
    ) -> Result<SailplaneUser> {
        let db = self.db.clone();
        let subject_owned = subject.to_string();
        let existing = db
            .run(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT * FROM users WHERE sub = ?1",
                        rusqlite::params![subject_owned],
                        SailplaneUser::from_row,
                    )
                    .ok())
            })
            .await?;

        match existing {
            Some(user) => {
                self.db
                    .update_user_profile(&user.id, name, email, picture)?;
                // A role claim always wins, except for the owner.
                if let Some(role) = sync_role {
                    self.db.set_user_role(&user.id, role)?;
                }
                self.db
                    .get_user(&user.id)?
                    .context("user disappeared after profile update")
            }
            None => self
                .db
                .create_user(subject, name, email, picture, initial_role),
        }
    }

    /// Links an account to a Headscale user when the subject or email matches.
    pub async fn auto_link_headscale_user(
        &self,
        user: &SailplaneUser,
        headscale_users: &[crate::headscale::User],
    ) -> Result<Option<String>> {
        if user.headscale_user_id.is_some() {
            return Ok(user.headscale_user_id.clone());
        }

        let subject = user.sub.strip_prefix("proxy:").unwrap_or(&user.sub);
        let matched = headscale_users
            .iter()
            .find(|candidate| candidate.provider_subject().as_deref() == Some(subject))
            .or_else(|| {
                let email = user.email.as_deref()?;
                headscale_users
                    .iter()
                    .find(|candidate| candidate.email.as_deref() == Some(email))
            });

        let Some(matched) = matched else {
            return Ok(None);
        };

        // Another account can already hold the link.
        if self.db.link_headscale_user(&user.id, &matched.id).is_err() {
            return Ok(None);
        }
        Ok(Some(matched.id.clone()))
    }

    /// Destroys the session behind a cookie value, if any.
    pub async fn destroy_session(&self, cookie: Option<&str>) -> Result<()> {
        let Some(cookie) = cookie else {
            return Ok(());
        };
        let config = self.settings.snapshot();
        let Some(payload) = session::decode_cookie(cookie, config.cookie_secret()) else {
            return Ok(());
        };
        self.db.delete_session(&payload.sid)
    }

    /// Verifies a Headscale API key against the server and returns its expiry.
    pub async fn validate_api_key(
        &self,
        headscale: &crate::headscale::Headscale,
        candidate: &str,
    ) -> Result<Result<ApiKeyValidation, String>> {
        let client = headscale.client(candidate);
        match client.list_api_keys().await {
            Ok(keys) => {
                let matched = keys
                    .iter()
                    .find(|key| candidate.starts_with(&key.matchable_prefix()));

                match matched {
                    Some(key) if key.is_expired() => {
                        Ok(Err("That API key already expired".into()))
                    }
                    Some(key) => {
                        let expires_at = key
                            .expiration
                            .as_deref()
                            .and_then(crate::util::parse_rfc3339)
                            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::days(365));
                        Ok(Ok(ApiKeyValidation {
                            display_name: format!("{}…", key.matchable_prefix()),
                            expires_at,
                        }))
                    }
                    None => Ok(Err(
                        "That API key was not found on this Headscale server".into()
                    )),
                }
            }
            Err(err) if err.is_unauthorized() || err.status() == Some(403) => {
                Ok(Err("The API key is invalid or expired".into()))
            }
            // Older Headscale builds answer with a generic 500 for bad keys.
            Err(err) if err.status() == Some(500) && err.raw_body().contains("Unauthorized") => {
                Ok(Err("The API key is invalid or expired".into()))
            }
            Err(err) => Ok(Err(format!("Could not validate the API key: {err}"))),
        }
    }
}

/// Result of validating a submitted API key.
#[derive(Debug, Clone)]
pub struct ApiKeyValidation {
    pub display_name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headscale::types::{Machine, User as HeadscaleUser};

    fn principal(role: Role, linked: Option<&str>) -> Principal {
        Principal::User {
            user: SailplaneUser {
                id: "u".into(),
                sub: "sub".into(),
                name: Some("Test".into()),
                email: None,
                picture: None,
                role,
                headscale_user_id: linked.map(str::to_string),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_login_at: None,
            },
            id_token: None,
        }
    }

    fn node_owned_by(id: &str) -> Machine {
        serde_json::from_str(&format!(
            r#"{{"id":"1","user":{{"id":"{id}","name":"owner"}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn viewer_cannot_manage_others_nodes() {
        let viewer = principal(Role::Viewer, Some("hs-1"));
        assert!(!viewer.can_manage_node(&node_owned_by("hs-2")));
        assert!(viewer.can_manage_node(&node_owned_by("hs-1")));
    }

    #[test]
    fn write_machines_manages_every_node() {
        let it_admin = principal(Role::ItAdmin, None);
        assert!(it_admin.can_manage_node(&node_owned_by("hs-9")));
    }

    #[test]
    fn unlinked_users_manage_nothing() {
        let viewer = principal(Role::Viewer, None);
        assert!(!viewer.can_manage_node(&node_owned_by("hs-1")));
    }

    #[test]
    fn api_key_principals_hold_every_capability() {
        let api = Principal::ApiKey {
            display_name: "key".into(),
            api_key: "secret".into(),
        };
        assert!(api.has(Capability::Owner));
        assert!(api.can_manage_node(&node_owned_by("anything")));
    }

    #[test]
    fn proxy_subject_is_stripped_for_matching() {
        let proxy = Principal::Proxy {
            user: SailplaneUser {
                id: "u".into(),
                sub: "proxy:alice".into(),
                name: None,
                email: None,
                picture: None,
                role: Role::Viewer,
                headscale_user_id: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                last_login_at: None,
            },
        };
        assert_eq!(proxy.capabilities(), Role::Viewer.capabilities());
        assert_eq!(proxy.display_name(), "proxy:alice");
    }

    #[test]
    fn headscale_user_matching_uses_provider_subject() {
        let user: HeadscaleUser = serde_json::from_str(
            r#"{"id":"7","name":"alice","providerId":"https://idp.example/alice"}"#,
        )
        .unwrap();
        assert_eq!(user.provider_subject().as_deref(), Some("alice"));
    }
}
