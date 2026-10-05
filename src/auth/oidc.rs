//! OpenID Connect login.
//!
//! The flow is hand-rolled to match upstream: manual endpoint overrides skip
//! discovery, the token request retries with the other client-auth method on
//! `invalid_client`, and the subject falls back through `subject_claims`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::config::{OidcConfig, ProfilePictureSource, TokenEndpointAuthMethod};

use super::session::OidcTransaction;

/// The discovery document, or manually configured endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub issuer: Option<String>,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
    /// Advertised client-auth methods, used to pick a default.
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
}

/// Why OIDC is unavailable, surfaced to the login page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OidcError {
    /// The discovery document could not be fetched.
    DiscoveryFailed,
    /// The provider does not advertise the required endpoints.
    MissingEndpoints,
}

impl OidcError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::DiscoveryFailed => {
                "Sailplane could not reach the OpenID Connect provider's discovery document."
            }
            Self::MissingEndpoints => {
                "The OpenID Connect provider is missing required endpoints (authorization, token or JWKS)."
            }
        }
    }
}

/// The identity extracted from a successful token exchange.
#[derive(Debug, Clone)]
pub struct OidcProfile {
    pub subject: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
    pub role: Option<super::Role>,
    pub id_token: String,
}

/// The OIDC provider, holding cached discovery and JWKS state.
pub struct OidcProvider {
    config: OidcConfig,
    http: reqwest::Client,
    metadata: RwLock<Option<ProviderMetadata>>,
    jwks: RwLock<Option<jsonwebtoken::jwk::JwkSet>>,
    /// Remembered once the provider reports which client-auth method it accepts.
    auth_method: RwLock<Option<TokenEndpointAuthMethod>>,
}

impl OidcProvider {
    pub fn new(config: OidcConfig) -> Result<Arc<Self>> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Arc::new(Self {
            config,
            http,
            metadata: RwLock::new(None),
            jwks: RwLock::new(None),
            auth_method: RwLock::new(None),
        }))
    }
    /// Resolves the provider endpoints, using manual overrides when the three
    /// required endpoints are configured.
    pub async fn discover(&self) -> Result<ProviderMetadata, OidcError> {
        if let Some(metadata) = self.metadata.read().await.clone() {
            return Ok(metadata);
        }

        let manual = match (
            self.config.authorization_endpoint.clone(),
            self.config.token_endpoint.clone(),
            self.config.jwks_endpoint.clone(),
        ) {
            (Some(authorization_endpoint), Some(token_endpoint), Some(jwks_uri)) => {
                Some(ProviderMetadata {
                    issuer: Some(self.config.issuer.clone()),
                    authorization_endpoint,
                    token_endpoint,
                    jwks_uri,
                    userinfo_endpoint: self.config.userinfo_endpoint.clone(),
                    end_session_endpoint: self.config.end_session_endpoint.clone(),
                    token_endpoint_auth_methods_supported: Vec::new(),
                })
            }
            _ => None,
        };

        let metadata = match manual {
            Some(metadata) => metadata,
            None => self.fetch_discovery().await?,
        };

        if metadata.authorization_endpoint.is_empty()
            || metadata.token_endpoint.is_empty()
            || metadata.jwks_uri.is_empty()
        {
            return Err(OidcError::MissingEndpoints);
        }

        *self.metadata.write().await = Some(metadata.clone());
        Ok(metadata)
    }

    async fn fetch_discovery(&self) -> Result<ProviderMetadata, OidcError> {
        let issuer = self.config.issuer.trim_end_matches('/');
        // The well-known path is inserted before any path component of the
        // issuer, per RFC 8414.
        let url = match issuer.split_once("://") {
            Some((scheme, rest)) => match rest.find('/') {
                Some(index) => format!(
                    "{scheme}://{}/.well-known/openid-configuration{}",
                    &rest[..index],
                    &rest[index..]
                ),
                None => format!("{scheme}://{rest}/.well-known/openid-configuration"),
            },
            None => format!("{issuer}/.well-known/openid-configuration"),
        };

        let response = self
            .http
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|err| {
                tracing::warn!("OIDC discovery request failed: {err}");
                OidcError::DiscoveryFailed
            })?;

        if !response.status().is_success() {
            tracing::warn!("OIDC discovery returned {}", response.status());
            return Err(OidcError::DiscoveryFailed);
        }

        let document: Value = response
            .json()
            .await
            .map_err(|_| OidcError::DiscoveryFailed)?;

        let string = |key: &str| -> Option<String> {
            document
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty())
        };

        let metadata = ProviderMetadata {
            issuer: string("issuer"),
            authorization_endpoint: string("authorization_endpoint")
                .unwrap_or_default(),
            token_endpoint: string("token_endpoint").unwrap_or_default(),
            jwks_uri: string("jwks_uri").unwrap_or_default(),
            userinfo_endpoint: string("userinfo_endpoint"),
            end_session_endpoint: string("end_session_endpoint"),
            token_endpoint_auth_methods_supported: document
                .get("token_endpoint_auth_methods_supported")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        };

        Ok(metadata)
    }

    /// Builds the authorization redirect URL and the transaction to stash in a cookie.
    pub async fn begin_flow(&self, redirect_uri: &str) -> Result<(String, OidcTransaction)> {
        let metadata = self.discover().await.map_err(|e| anyhow::anyhow!(e.message()))?;

        let state = crate::util::random_token(32);
        let nonce = crate::util::random_token(32);

        let verifier = self
            .config
            .use_pkce
            .then(|| crate::util::random_token(64));

        let mut url = url::Url::parse(&metadata.authorization_endpoint)
            .map_err(|err| anyhow::anyhow!("invalid authorization endpoint: {err}"))?;

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &self.config.client_id);
            query.append_pair("redirect_uri", redirect_uri);
            query.append_pair("scope", &self.config.scope);
            query.append_pair("state", &state);
            query.append_pair("nonce", &nonce);

            if let Some(verifier) = verifier.as_deref() {
                query.append_pair(
                    "code_challenge",
                    &crate::util::sha256_hex_b64url(verifier.as_bytes()),
                );
                query.append_pair("code_challenge_method", "S256");
            }

            if let Some(extra) = self.config.extra_params.as_ref() {
                for (key, value) in extra {
                    query.append_pair(key, value);
                }
            }
        }

        let transaction = OidcTransaction {
            state,
            nonce,
            verifier,
            redirect_uri: redirect_uri.to_string(),
        };

        Ok((url.to_string(), transaction))
    }

    /// Exchanges the authorization code and validates the resulting ID token.
    pub async fn handle_callback(
        &self,
        code: &str,
        transaction: &OidcTransaction,
    ) -> Result<OidcProfile> {
        let metadata = self
            .discover()
            .await
            .map_err(|e| anyhow::anyhow!(e.message()))?;

        let client_secret = self.config.client_secret.clone().unwrap_or_default();
        let (id_token, access_token) = self
            .exchange_code(&metadata, code, transaction, &client_secret)
            .await?;

        let claims = self.verify_id_token(&metadata, &id_token, &transaction.nonce).await?;

        // The userinfo response fills in anything the ID token omitted.
        let userinfo = match access_token.as_deref() {
            Some(access_token) => self.fetch_userinfo(&metadata, access_token).await,
            None => None,
        };

        let claim = |name: &str| -> Option<Value> {
            claims
                .get(name)
                .cloned()
                .or_else(|| userinfo.as_ref().and_then(|info| info.get(name).cloned()))
                .filter(|value| !value.is_null())
        };

        let string_claim = |name: &str| -> Option<String> {
            claim(name).and_then(|value| match value {
                Value::String(s) if !s.trim().is_empty() => Some(s),
                _ => None,
            })
        };

        // Subject: `sub` first, then each configured fallback claim.
        let mut subject = string_claim("sub");
        for fallback in &self.config.subject_claims {
            if subject.is_some() {
                break;
            }
            subject = string_claim(fallback);
        }
        let subject = subject.ok_or_else(|| anyhow::anyhow!("missing_sub"))?;

        let email = string_claim("email");
        let given = string_claim("given_name");
        let family = string_claim("family_name");
        let preferred = string_claim("preferred_username");

        let name = string_claim("name")
            .or_else(|| match (given, family) {
                (Some(given), Some(family)) => Some(format!("{given} {family}")),
                (Some(given), None) => Some(given),
                (None, Some(family)) => Some(family),
                (None, None) => None,
            })
            .or(preferred);

        let picture = match self.config.profile_picture_source {
            ProfilePictureSource::Gravatar => email.as_ref().map(|email| {
                format!(
                    "https://www.gravatar.com/avatar/{}?s=200&d=identicon&r=x",
                    crate::util::sha256_hex(email.trim().to_ascii_lowercase().as_bytes())
                )
            }),
            ProfilePictureSource::Oidc => string_claim("picture"),
        };

        let role = self
            .config
            .role_claim
            .as_deref()
            .and_then(claim)
            .and_then(|value| resolve_role(&value));

        Ok(OidcProfile {
            subject,
            name,
            email,
            picture,
            role,
            id_token,
        })
    }

    async fn exchange_code(
        &self,
        metadata: &ProviderMetadata,
        code: &str,
        transaction: &OidcTransaction,
        client_secret: &str,
    ) -> Result<(String, Option<String>)> {
        let method = self
            .auth_method
            .read()
            .await
            .unwrap_or(self.config.token_endpoint_auth_method.unwrap_or_default());

        let (token, access_token, used) = self
            .token_request(metadata, code, transaction, client_secret, method, false)
            .await?;

        if used != method {
            *self.auth_method.write().await = Some(used);
        }

        Ok((token, access_token))
    }

    /// `retried` bounds the client-auth fallback to one extra attempt: a
    /// provider that answers `invalid_client` to both methods would loop forever.
    async fn token_request(
        &self,
        metadata: &ProviderMetadata,
        code: &str,
        transaction: &OidcTransaction,
        client_secret: &str,
        method: TokenEndpointAuthMethod,
        retried: bool,
    ) -> Result<(String, Option<String>, TokenEndpointAuthMethod)> {
        let effective = match method {
            TokenEndpointAuthMethod::Auto => pick_auto_method(
                &metadata.token_endpoint_auth_methods_supported,
                self.config.token_endpoint_auth_method,
            ),
            other => other,
        };

        let mut form: Vec<(String, String)> = vec![
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), code.to_string()),
            ("redirect_uri".into(), transaction.redirect_uri.clone()),
        ];
        if let Some(verifier) = transaction.verifier.as_deref() {
            form.push(("code_verifier".into(), verifier.to_string()));
        }

        let mut request = self.http.post(&metadata.token_endpoint);
        request = request.header("Accept", "application/json");

        match effective {
            TokenEndpointAuthMethod::ClientSecretBasic => {
                request = request.basic_auth(&self.config.client_id, Some(client_secret));
            }
            TokenEndpointAuthMethod::ClientSecretPost | TokenEndpointAuthMethod::Auto => {
                form.push(("client_id".into(), self.config.client_id.clone()));
                form.push(("client_secret".into(), client_secret.to_string()));
            }
            TokenEndpointAuthMethod::ClientSecretJwt => {
                // `client_secret_jwt` needs an HS256 assertion. Providers that
                // require it are rare, so this falls back to basic auth and
                // lets the retry logic negotiate.
                request = request.basic_auth(&self.config.client_id, Some(client_secret));
            }
        }

        let response = request.form(&form).send().await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);

        if !status.is_success() {
            let error = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let description = body
                .get("error_description")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if error.contains("pkce") || description.to_ascii_lowercase().contains("pkce") {
                bail!(
                    "the identity provider rejected PKCE; disable oidc.use_pkce or enable PKCE \
                     on the client ({description})"
                );
            }

            // `invalid_client` usually means the client-auth method guess was
            // wrong: retry once with the other method before giving up. Record
            // the first failure at `warn`, because a provider that invalidates
            // the code before it checks the client answers the retry with a
            // misleading error, and the real cause would stay invisible.
            if error == "invalid_client" && !retried {
                let alternative = match effective {
                    TokenEndpointAuthMethod::ClientSecretBasic => {
                        TokenEndpointAuthMethod::ClientSecretPost
                    }
                    _ => TokenEndpointAuthMethod::ClientSecretBasic,
                };
                tracing::warn!(
                    "token endpoint rejected {effective:?} client authentication \
                     ({error} {description}); retrying as {alternative:?}"
                );
                return Box::pin(self.token_request(
                    metadata,
                    code,
                    transaction,
                    client_secret,
                    alternative,
                    true,
                ))
                .await;
            }

            bail!("token endpoint returned {status}: {error} {description}");
        }

        let id_token = body
            .get("id_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("the token response did not include an id_token"))?;

        let access_token = body
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::to_string);

        Ok((id_token, access_token, effective))
    }

    async fn fetch_jwks(&self, metadata: &ProviderMetadata) -> Result<jsonwebtoken::jwk::JwkSet> {
        if let Some(cached) = self.jwks.read().await.clone() {
            return Ok(cached);
        }
        let jwks: jsonwebtoken::jwk::JwkSet = self
            .http
            .get(&metadata.jwks_uri)
            .send()
            .await?
            .json()
            .await?;
        *self.jwks.write().await = Some(jwks.clone());
        Ok(jwks)
    }

    async fn verify_id_token(
        &self,
        metadata: &ProviderMetadata,
        token: &str,
        expected_nonce: &str,
    ) -> Result<Value> {
        let jwks = self.fetch_jwks(metadata).await?;
        let header = jsonwebtoken::decode_header(token)?;

        let jwk = match header.kid.as_deref() {
            Some(kid) => jwks.find(kid).cloned(),
            None => jwks.keys.first().cloned(),
        }
        .ok_or_else(|| anyhow::anyhow!("no matching JWKS key for the ID token"))?;

        let algorithm = jwk
            .common
            .key_algorithm
            .map(|alg| alg.to_string())
            .and_then(|alg| alg.parse::<jsonwebtoken::Algorithm>().ok())
            .unwrap_or(header.alg);

        let key = jsonwebtoken::DecodingKey::from_jwk(&jwk).map_err(|err| {
            if self.config.allow_weak_rsa_keys {
                tracing::warn!("the identity provider uses a weak signing key: {err}");
            }
            anyhow::anyhow!("could not build a verification key: {err}")
        })?;

        let mut validation = jsonwebtoken::Validation::new(algorithm);
        validation.leeway = 60;
        validation.set_issuer(&[if self.config.issuer.is_empty() {
            metadata.issuer.clone().unwrap_or_default()
        } else {
            self.config.issuer.clone()
        }]);
        validation.set_audience(std::slice::from_ref(&self.config.client_id));

        let data = jsonwebtoken::decode::<Value>(token, &key, &validation)
            .map_err(|err| anyhow::anyhow!("ID token validation failed: {err}"))?;

        // The nonce was sent in the authorization request, so a token for this
        // flow has to echo it. A token that omits the claim is not acceptable.
        if !expected_nonce.is_empty() {
            match data.claims.get("nonce").and_then(Value::as_str) {
                Some(nonce) if nonce == expected_nonce => {}
                Some(_) => bail!("ID token nonce mismatch"),
                None => bail!("the ID token is missing the nonce claim"),
            }
        }

        Ok(data.claims)
    }

    async fn fetch_userinfo(&self, metadata: &ProviderMetadata, access_token: &str) -> Option<Value> {
        let endpoint = self
            .config
            .userinfo_endpoint
            .clone()
            .or_else(|| metadata.userinfo_endpoint.clone())?;

        match self
            .http
            .get(&endpoint)
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => response.json().await.ok(),
            Ok(response) => {
                tracing::debug!("userinfo request returned {}", response.status());
                None
            }
            Err(err) => {
                // Enrichment is best-effort; the ID token already provided a
                // usable identity.
                tracing::debug!("userinfo request failed: {err}");
                None
            }
        }
    }

    /// Builds an RP-initiated logout URL when configured.
    pub async fn end_session_url(
        &self,
        id_token_hint: Option<&str>,
        post_logout_redirect_uri: &str,
    ) -> Option<String> {
        if !self.config.use_end_session {
            return None;
        }
        let metadata = self.discover().await.ok()?;
        let endpoint = self
            .config
            .end_session_endpoint
            .clone()
            .or(metadata.end_session_endpoint)?;

        let mut url = url::Url::parse(&endpoint).ok()?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("client_id", &self.config.client_id);
            query.append_pair("post_logout_redirect_uri", post_logout_redirect_uri);
            if let Some(hint) = id_token_hint {
                query.append_pair("id_token_hint", hint);
            }
        }
        Some(url.to_string())
    }
}
/// Maps a role claim value (string or array) onto a role, ignoring `owner`.
fn resolve_role(value: &Value) -> Option<super::Role> {
    let candidates: Vec<String> = match value {
        Value::String(single) => vec![single.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => return None,
    };

    // Highest-privilege recognised value wins, in the upstream order.
    for wanted in [
        "admin",
        "network_admin",
        "it_admin",
        "auditor",
        "viewer",
        "member",
    ] {
        if candidates.iter().any(|c| c == wanted) {
            return Some(super::Role::parse(wanted));
        }
    }
    None
}

fn pick_auto_method(
    supported: &[String],
    configured: Option<TokenEndpointAuthMethod>,
) -> TokenEndpointAuthMethod {
    if let Some(method) = configured
        && method != TokenEndpointAuthMethod::Auto {
            return method;
        }
    if supported
        .iter()
        .any(|m| m == "client_secret_basic")
        || supported.is_empty()
    {
        TokenEndpointAuthMethod::ClientSecretBasic
    } else if supported.iter().any(|m| m == "client_secret_post") {
        TokenEndpointAuthMethod::ClientSecretPost
    } else {
        TokenEndpointAuthMethod::ClientSecretBasic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> Arc<OidcProvider> {
        let config: OidcConfig = serde_yaml_ng::from_str(
            r#"
issuer: https://idp.example
client_id: sailplane
client_secret: secret
authorization_endpoint: https://idp.example/authorize
token_endpoint: https://idp.example/token
jwks_endpoint: https://idp.example/jwks
"#,
        )
        .unwrap();
        OidcProvider::new(config).unwrap()
    }

    #[tokio::test]
    async fn manual_endpoints_skip_discovery() {
        let provider = provider();
        let metadata = provider.discover().await.unwrap();
        assert_eq!(metadata.authorization_endpoint, "https://idp.example/authorize");
        assert_eq!(metadata.jwks_uri, "https://idp.example/jwks");
    }

    #[tokio::test]
    async fn flow_carries_state_nonce_and_redirect() {
        let provider = provider();
        let (url, transaction) = provider
            .begin_flow("https://app.example/admin/oidc/callback")
            .await
            .unwrap();

        let parsed = url::Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();

        assert_eq!(query.get("response_type").unwrap(), "code");
        assert_eq!(query.get("client_id").unwrap(), "sailplane");
        assert_eq!(query.get("state").unwrap(), &transaction.state);
        assert_eq!(query.get("nonce").unwrap(), &transaction.nonce);
        assert_eq!(
            query.get("redirect_uri").unwrap(),
            "https://app.example/admin/oidc/callback"
        );
        assert!(!query.contains_key("code_challenge"), "PKCE is off by default");
    }

    #[tokio::test]
    async fn pkce_adds_challenge() {
        let mut provider = provider();
        let mut config = provider.config.clone();
        config.use_pkce = true;
        provider = OidcProvider::new(config).unwrap();

        let (url, transaction) = provider.begin_flow("https://app.example/cb").await.unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();

        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        assert!(transaction.verifier.is_some());
    }

    #[test]
    fn role_claim_accepts_strings_and_arrays() {
        assert_eq!(resolve_role(&Value::String("admin".into())), Some(super::super::Role::Admin));
        assert_eq!(
            resolve_role(&serde_json::json!(["member", "viewer"])),
            Some(super::super::Role::Viewer)
        );
        // Owner is never granted through a claim.
        assert_eq!(resolve_role(&Value::String("owner".into())), None);
        assert_eq!(resolve_role(&serde_json::json!(42)), None);
    }

    #[test]
    fn auto_method_prefers_basic() {
        assert_eq!(
            pick_auto_method(&[], None),
            TokenEndpointAuthMethod::ClientSecretBasic
        );
        assert_eq!(
            pick_auto_method(&["client_secret_post".into()], None),
            TokenEndpointAuthMethod::ClientSecretPost
        );
        assert_eq!(
            pick_auto_method(&["client_secret_basic".into(), "client_secret_post".into()], None),
            TokenEndpointAuthMethod::ClientSecretBasic
        );
    }

    #[test]
    fn discovery_url_inserts_well_known_before_path() {
        // Mirrors RFC 8414 insertion for issuers with a path component.
        let issuer = "https://idp.example/tenant/1";
        let (scheme, rest) = issuer.split_once("://").unwrap();
        let index = rest.find('/').unwrap();
        let url = format!(
            "{scheme}://{}/.well-known/openid-configuration{}",
            &rest[..index],
            &rest[index..]
        );
        assert_eq!(
            url,
            "https://idp.example/.well-known/openid-configuration/tenant/1"
        );
    }
}
