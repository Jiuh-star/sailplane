//! HTTP client for the Headscale v1 API.
//!
//! Upstream reports every non-2xx as HTTP 502 and puts the real status in the
//! payload. This client keeps the original status. Callers can then tell a
//! read-only policy from a missing one without unwrapping a nested error.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::types::*;
use super::version::{Capabilities, ServerVersion};

pub const USER_AGENT: &str = concat!("Sailplane/", env!("CARGO_PKG_VERSION"));

/// Delay between retries while Headscale is unreachable.
const VERSION_RETRY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum HeadscaleError {
    /// Headscale answered with a non-2xx status.
    #[error("headscale API error {status} for {request_url}")]
    Api {
        request_url: String,
        status: u16,
        raw: String,
        data: Option<Value>,
    },

    #[error("failed to reach headscale at {request_url}: {message}")]
    Connection {
        request_url: String,
        message: String,
    },
}

impl HeadscaleError {
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            Self::Connection { .. } => None,
        }
    }

    pub fn raw_body(&self) -> &str {
        match self {
            Self::Api { raw, .. } => raw,
            Self::Connection { .. } => "",
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(401)
    }

    /// True when Headscale reports policy updates are disabled in file mode.
    pub fn is_policy_read_only(&self) -> bool {
        self.raw_body().contains("update is disabled")
    }

    /// True when Headscale has no ACL policy configured.
    pub fn is_policy_missing(&self) -> bool {
        self.raw_body().contains("acl policy not found")
    }
}

/// New key lifetime when expiry is re-enabled on a node.
const RENEWED_KEY_EXPIRY_DAYS: i64 = 180;

/// Shared handle to a Headscale server. Cheap to clone.
#[derive(Clone)]
pub struct Headscale {
    inner: Arc<Inner>,
}

struct Inner {
    /// Swappable so a change to `headscale.url` takes effect without a restart.
    base_url: RwLock<String>,
    http: reqwest::Client,
    version: RwLock<ServerVersion>,
}

impl Headscale {
    /// Builds a client. `cert_path` optionally pins a PEM certificate for TLS.
    pub fn new(url: &str, cert_path: Option<&std::path::Path>) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            // Headscale is on an internal network and must never go through an
            // ambient `HTTP_PROXY`. This also keeps the bearer API key off any
            // intermediary; upstream's undici client ignores proxy variables
            // for the same reason.
            .no_proxy();

        if let Some(path) = cert_path {
            match std::fs::read(path) {
                Ok(pem) => match reqwest::Certificate::from_pem(&pem) {
                    Ok(cert) => builder = builder.add_root_certificate(cert),
                    Err(err) => tracing::warn!(
                        "headscale.tls_cert_path {} is not a valid PEM certificate: {err}",
                        path.display()
                    ),
                },
                Err(err) => tracing::warn!(
                    "failed to read headscale.tls_cert_path {}: {err}",
                    path.display()
                ),
            }
        }

        let http = builder.build().context("failed to build HTTP client")?;

        Ok(Self {
            inner: Arc::new(Inner {
                base_url: RwLock::new(url.trim_end_matches('/').to_string()),
                http,
                version: RwLock::new(ServerVersion::default()),
            }),
        })
    }

    /// The current Headscale base URL.
    pub fn base_url(&self) -> String {
        self.inner
            .base_url
            .read()
            .expect("base url lock poisoned")
            .clone()
    }

    /// Points new requests at a different Headscale URL. A no-op when the URL
    /// is unchanged, so a per-tick caller does not take the write lock.
    pub fn set_base_url(&self, url: &str) {
        let url = url.trim_end_matches('/');
        let mut current = self.inner.base_url.write().expect("base url lock poisoned");
        if current.as_str() == url {
            return;
        }
        *current = url.to_string();
    }

    pub fn version(&self) -> ServerVersion {
        self.inner
            .version
            .read()
            .expect("version lock poisoned")
            .clone()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.version().capabilities()
    }

    pub fn set_version(&self, version: ServerVersion) {
        *self.inner.version.write().expect("version lock poisoned") = version;
    }

    /// Fetches `/version` once. `Ok(None)` means the endpoint is absent, and
    /// the server is older than 0.27.0.
    pub async fn probe_version(&self) -> Result<Option<ServerVersion>> {
        let url = format!("{}/version", self.base_url());
        let response = self
            .inner
            .http
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await;

        match response {
            Ok(response) if response.status() == StatusCode::NOT_FOUND => Ok(None),
            Ok(response) if response.status().is_success() => {
                let body: VersionResponse = response
                    .json()
                    .await
                    .context("failed to parse /version response")?;
                let version = ServerVersion::parse(&body.version);
                self.set_version(version.clone());
                Ok(Some(version))
            }
            Ok(response) => {
                let status = response.status();
                let raw = response.text().await.unwrap_or_default();
                anyhow::bail!(
                    "GET /version returned {status}: {}",
                    crate::util::truncate(&raw, 200)
                )
            }
            Err(err) => Err(err).context("failed to reach headscale /version"),
        }
    }

    /// Spawns the version poller. It retries every 30 s until a version is
    /// known, so an in-place Headscale upgrade is picked up without a restart.
    pub fn spawn_version_poller(&self) {
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                match this.probe_version().await {
                    Ok(Some(version)) => {
                        if version.below_minimum() {
                            tracing::error!(
                                "headscale {} is older than the minimum supported version 0.27.0",
                                version.raw
                            );
                        } else {
                            tracing::info!("connected to headscale {}", version.raw);
                        }
                        return;
                    }
                    Ok(None) => {
                        tracing::error!(
                            "headscale /version returned 404; sailplane requires Headscale 0.27.0 \
                             or newer. Retrying in {}s",
                            VERSION_RETRY_INTERVAL.as_secs()
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            "could not determine headscale version ({err:#}); \
                             assuming newest capabilities and retrying in {}s",
                            VERSION_RETRY_INTERVAL.as_secs()
                        );
                    }
                }
                tokio::time::sleep(VERSION_RETRY_INTERVAL).await;
            }
        });
    }

    /// Checks server health. `GET /health`. Never fails; a transport error
    /// means unhealthy.
    pub async fn health(&self) -> bool {
        let url = format!("{}/health", self.base_url());
        match self.inner.http.get(&url).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }

    /// Returns an API view authenticated with one key.
    pub fn client(&self, api_key: impl Into<String>) -> ApiClient {
        ApiClient {
            headscale: self.clone(),
            api_key: Arc::new(api_key.into()),
        }
    }

    async fn send(
        &self,
        api_key: Option<&str>,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<Value, HeadscaleError> {
        let request_url = format!("{} {}", method, path);
        let url = format!("{}/api/{}", self.base_url(), path.trim_start_matches('/'));

        let mut request = self.inner.http.request(method.clone(), &url);

        if let Some(key) = api_key {
            request = request.bearer_auth(key);
        }
        request = request.header("Accept", "application/json");

        // Headscale binds request fields to the query string unless the route
        // declares a body, so every method carries its query. Node registration
        // sends the parameters in both places because upstream does.
        if !query.is_empty() {
            request = request.query(query);
        }
        let carries_body = matches!(method, Method::POST | Method::PUT | Method::PATCH);
        if carries_body && let Some(payload) = body {
            request = request.json(&payload);
        }

        let response = request
            .send()
            .await
            .map_err(|err| HeadscaleError::Connection {
                request_url: request_url.clone(),
                message: format!("{err}"),
            })?;

        let status = response.status();
        let raw = response.text().await.unwrap_or_default();

        if !status.is_success() {
            let data = serde_json::from_str(&raw).ok();
            return Err(HeadscaleError::Api {
                request_url,
                status: status.as_u16(),
                raw,
                data,
            });
        }

        if raw.trim().is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_str(&raw).map_err(|err| HeadscaleError::Api {
            request_url,
            status: status.as_u16(),
            raw: format!("invalid JSON response: {err}"),
            data: None,
        })
    }
}

/// A Headscale API view authenticated with one API key.
#[derive(Clone)]
pub struct ApiClient {
    headscale: Headscale,
    api_key: Arc<String>,
}

impl ApiClient {
    pub fn capabilities(&self) -> Capabilities {
        self.headscale.capabilities()
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<T, HeadscaleError> {
        let value = self
            .headscale
            .send(Some(&self.api_key), Method::GET, path, query, None)
            .await?;
        decode(value)
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<T, HeadscaleError> {
        let value = self
            .headscale
            .send(Some(&self.api_key), Method::POST, path, query, body)
            .await?;
        decode(value)
    }

    async fn put<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T, HeadscaleError> {
        let value = self
            .headscale
            .send(Some(&self.api_key), Method::PUT, path, &[], Some(body))
            .await?;
        decode(value)
    }

    async fn delete(&self, path: &str) -> Result<(), HeadscaleError> {
        self.headscale
            .send(Some(&self.api_key), Method::DELETE, path, &[], None)
            .await
            .map(|_| ())
    }

    async fn delete_with_query(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<(), HeadscaleError> {
        self.headscale
            .send(Some(&self.api_key), Method::DELETE, path, query, None)
            .await
            .map(|_| ())
    }

    // --- Nodes ---

    pub async fn list_nodes(&self) -> Result<Vec<Machine>, HeadscaleError> {
        let response: NodesResponse = self.get("v1/node", &[]).await?;
        Ok(response.nodes)
    }
    pub async fn delete_node(&self, id: &str) -> Result<(), HeadscaleError> {
        self.delete(&format!("v1/node/{id}")).await
    }

    /// Registers a node from a registration key. It sends the parameters as
    /// both query string and JSON body, as upstream does.
    pub async fn register_node(&self, user: &str, key: &str) -> Result<Machine, HeadscaleError> {
        let query = vec![
            ("user".to_string(), user.to_string()),
            ("key".to_string(), key.to_string()),
        ];
        let body = serde_json::json!({ "user": user, "key": key });
        let response: NodeResponse = self.post("v1/node/register", &query, Some(body)).await?;
        response.node.ok_or_else(|| HeadscaleError::Api {
            request_url: "POST v1/node/register".into(),
            status: 500,
            raw: "registration returned no node".into(),
            data: None,
        })
    }

    pub async fn rename_node(&self, id: &str, name: &str) -> Result<(), HeadscaleError> {
        let encoded = urlencode(name);
        self.post::<Value>(&format!("v1/node/{id}/rename/{encoded}"), &[], None)
            .await
            .map(|_| ())
    }

    pub async fn expire_node(&self, id: &str) -> Result<(), HeadscaleError> {
        self.post::<Value>(&format!("v1/node/{id}/expire"), &[], None)
            .await
            .map(|_| ())
    }

    /// Enables or disables key expiry. Requires Headscale 0.29+.
    ///
    /// Headscale reads a missing `expiry` as "now". Re-enabling expiry must
    /// therefore name a date, because `disableExpiry=false` alone logs the node
    /// out. The date is 180 days out, matching Tailscale's default.
    pub async fn toggle_node_expiry(
        &self,
        id: &str,
        disable_expiry: bool,
    ) -> Result<(), HeadscaleError> {
        let mut query = vec![("disableExpiry".to_string(), disable_expiry.to_string())];
        if !disable_expiry {
            let expiry = chrono::Utc::now() + chrono::Duration::days(RENEWED_KEY_EXPIRY_DAYS);
            query.push(("expiry".to_string(), crate::util::format_rfc3339(expiry)));
        }
        self.post::<Value>(&format!("v1/node/{id}/expire"), &query, None)
            .await
            .map(|_| ())
    }

    pub async fn set_node_tags(&self, id: &str, tags: &[String]) -> Result<(), HeadscaleError> {
        let body = serde_json::json!({ "tags": tags });
        self.post::<Value>(&format!("v1/node/{id}/tags"), &[], Some(body))
            .await
            .map(|_| ())
    }

    pub async fn approve_routes(&self, id: &str, routes: &[String]) -> Result<(), HeadscaleError> {
        let body = serde_json::json!({ "routes": routes });
        self.post::<Value>(&format!("v1/node/{id}/approve_routes"), &[], Some(body))
            .await
            .map(|_| ())
    }

    /// Reassigns node ownership. Unsupported on Headscale 0.28+.
    pub async fn reassign_node(&self, id: &str, user: &str) -> Result<(), HeadscaleError> {
        let body = serde_json::json!({ "user": user });
        self.post::<Value>(&format!("v1/node/{id}/user"), &[], Some(body))
            .await
            .map(|_| ())
    }

    // --- Users ---

    pub async fn list_users(&self) -> Result<Vec<User>, HeadscaleError> {
        let response: UsersResponse = self.get("v1/user", &[]).await?;
        Ok(response.users)
    }

    pub async fn create_user(
        &self,
        name: &str,
        email: Option<&str>,
        display_name: Option<&str>,
        picture_url: Option<&str>,
    ) -> Result<User, HeadscaleError> {
        let mut body = serde_json::Map::new();
        body.insert("name".into(), Value::String(name.to_string()));
        if let Some(email) = email {
            body.insert("email".into(), Value::String(email.to_string()));
        }
        if let Some(display_name) = display_name {
            body.insert(
                "displayName".into(),
                Value::String(display_name.to_string()),
            );
        }
        if let Some(picture_url) = picture_url {
            body.insert("pictureUrl".into(), Value::String(picture_url.to_string()));
        }
        let response: UserResponse = self.post("v1/user", &[], Some(Value::Object(body))).await?;
        response.user.ok_or_else(|| HeadscaleError::Api {
            request_url: "POST v1/user".into(),
            status: 500,
            raw: "user creation returned no user".into(),
            data: None,
        })
    }

    pub async fn delete_user(&self, id: &str) -> Result<(), HeadscaleError> {
        self.delete(&format!("v1/user/{id}")).await
    }

    pub async fn rename_user(&self, id: &str, name: &str) -> Result<(), HeadscaleError> {
        let encoded = urlencode(name);
        self.post::<Value>(&format!("v1/user/{id}/rename/{encoded}"), &[], None)
            .await
            .map(|_| ())
    }

    // --- Pre-auth keys ---

    pub async fn list_pre_auth_keys(&self) -> Result<Vec<PreAuthKey>, HeadscaleError> {
        let response: PreAuthKeysResponse = self.get("v1/preauthkey", &[]).await?;
        Ok(response.pre_auth_keys)
    }

    pub async fn list_pre_auth_keys_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<PreAuthKey>, HeadscaleError> {
        let response: PreAuthKeysResponse = self
            .get("v1/preauthkey", &[("user".into(), user_id.into())])
            .await?;
        Ok(response.pre_auth_keys)
    }

    pub async fn create_pre_auth_key(
        &self,
        user_id: Option<&str>,
        acl_tags: &[String],
        ephemeral: bool,
        reusable: bool,
        expiration: Option<&str>,
    ) -> Result<PreAuthKey, HeadscaleError> {
        let mut body = serde_json::Map::new();
        body.insert("ephemeral".into(), Value::Bool(ephemeral));
        body.insert("reusable".into(), Value::Bool(reusable));
        body.insert(
            "expiration".into(),
            match expiration {
                Some(value) => Value::String(value.to_string()),
                None => Value::Null,
            },
        );
        if let Some(user_id) = user_id {
            body.insert("user".into(), Value::String(user_id.to_string()));
        }
        if !acl_tags.is_empty() {
            body.insert(
                "aclTags".into(),
                Value::Array(acl_tags.iter().map(|t| Value::String(t.clone())).collect()),
            );
        }

        let response: PreAuthKeyResponse = self
            .post("v1/preauthkey", &[], Some(Value::Object(body)))
            .await?;
        response.pre_auth_key.ok_or_else(|| HeadscaleError::Api {
            request_url: "POST v1/preauthkey".into(),
            status: 500,
            raw: "key creation returned no key".into(),
            data: None,
        })
    }

    /// Expires a pre-auth key. Headscale 0.28+ identifies keys by id; earlier
    /// versions need the owning user's numeric id plus the key itself.
    pub async fn expire_pre_auth_key(
        &self,
        key_id: &str,
        user_id: Option<&str>,
        key: &str,
    ) -> Result<(), HeadscaleError> {
        let body = if self.capabilities().pre_auth_keys_have_stable_ids {
            serde_json::json!({ "id": key_id })
        } else {
            serde_json::json!({ "user": user_id.unwrap_or_default(), "key": key })
        };
        self.post::<Value>("v1/preauthkey/expire", &[], Some(body))
            .await
            .map(|_| ())
    }

    /// Deletes a pre-auth key. This removes the row, rather than only marking
    /// it expired. Key ids exist on Headscale 0.28+.
    pub async fn delete_pre_auth_key(&self, id: &str) -> Result<(), HeadscaleError> {
        let query = vec![("id".to_string(), id.to_string())];
        self.delete_with_query("v1/preauthkey", &query).await
    }

    // --- API keys ---

    pub async fn list_api_keys(&self) -> Result<Vec<ApiKey>, HeadscaleError> {
        let response: ApiKeysResponse = self.get("v1/apikey", &[]).await?;
        Ok(response.api_keys)
    }

    /// Revokes an API key by id, which stops it authenticating immediately.
    ///
    /// The body carries the id, not the path prefix. Headscale masks the
    /// listed prefix with `*`, so the two lookups are not interchangeable.
    pub async fn expire_api_key(&self, id: u64) -> Result<(), HeadscaleError> {
        self.post::<Value>(
            "v1/apikey/expire",
            &[],
            Some(serde_json::json!({ "id": id })),
        )
        .await
        .map(|_| ())
    }

    // --- Policy ---

    pub async fn get_policy(&self) -> Result<Policy, HeadscaleError> {
        self.get("v1/policy", &[]).await
    }

    pub async fn set_policy(&self, policy: &str) -> Result<Policy, HeadscaleError> {
        let body = serde_json::json!({ "policy": policy });
        self.put("v1/policy", body).await
    }

    // --- Auth ---

    pub async fn approve_auth(&self, auth_id: &str) -> Result<(), HeadscaleError> {
        let body = serde_json::json!({ "authId": auth_id });
        self.post::<Value>("v1/auth/approve", &[], Some(body))
            .await
            .map(|_| ())
    }
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, HeadscaleError> {
    serde_json::from_value(value).map_err(|err| HeadscaleError::Api {
        request_url: "decode".into(),
        status: 500,
        raw: format!("failed to decode response: {err}"),
        data: None,
    })
}

/// Percent-encodes a path segment, leaving unreserved characters intact.
fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_escapes_reserved_characters() {
        assert_eq!(urlencode("my machine"), "my%20machine");
        assert_eq!(urlencode("safe-name_1"), "safe-name_1");
        assert_eq!(urlencode("a/b"), "a%2Fb");
    }

    /// Changing `headscale.url` in the UI must repoint the existing client
    /// without a restart.
    #[test]
    fn base_url_can_be_swapped() {
        let headscale = Headscale::new("http://one.example/", None).unwrap();
        assert_eq!(headscale.base_url(), "http://one.example");
        headscale.set_base_url("http://two.example/");
        assert_eq!(headscale.base_url(), "http://two.example");
    }

    /// Serves the node-expire route, echoing the query back and recording it.
    async fn echo_query_server() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = seen.clone();

        let app = axum::Router::new().route(
            "/api/v1/node/{id}/expire",
            axum::routing::post(move |uri: axum::http::Uri| {
                let recorder = recorder.clone();
                async move {
                    let query = uri.query().unwrap_or_default().to_string();
                    recorder.lock().unwrap().push(query.clone());
                    axum::Json(serde_json::json!({ "query": query }))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), seen)
    }

    /// Headscale binds request fields to the query string by default, so a
    /// POST that drops them silently does nothing.
    #[tokio::test]
    async fn post_requests_carry_their_query_parameters() {
        let (base_url, _) = echo_query_server().await;
        let client = Headscale::new(&base_url, None)
            .unwrap()
            .client("test-key".to_string());

        let query = vec![("disableExpiry".to_string(), "true".to_string())];
        let value = client
            .post::<Value>("v1/node/1/expire", &query, None)
            .await
            .unwrap();

        assert_eq!(value["query"], "disableExpiry=true");
    }

    /// Headscale reads a missing `expiry` as "now", so re-enabling expiry must
    /// carry a date or the node is logged out on the spot.
    #[tokio::test]
    async fn re_enabling_key_expiry_sends_a_future_date() {
        let (base_url, seen) = echo_query_server().await;
        let client = Headscale::new(&base_url, None)
            .unwrap()
            .client("test-key".to_string());

        client.toggle_node_expiry("1", false).await.unwrap();

        let query = seen.lock().unwrap().last().cloned().unwrap();
        assert!(query.contains("disableExpiry=false"), "{query}");

        let expiry = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("expiry="))
            .map(|value| value.replace("%3A", ":").replace("%2B", "+"))
            .expect("the request must carry an expiry");

        let parsed = crate::util::parse_rfc3339(&expiry).expect("a valid RFC 3339 timestamp");
        assert!(
            parsed > chrono::Utc::now() + chrono::Duration::days(30),
            "expiry {expiry} is not a renewed lease"
        );
    }

    /// Disabling expiry must not also send a date: Headscale rejects both.
    #[tokio::test]
    async fn disabling_key_expiry_sends_no_date() {
        let (base_url, seen) = echo_query_server().await;
        let client = Headscale::new(&base_url, None)
            .unwrap()
            .client("test-key".to_string());

        client.toggle_node_expiry("1", true).await.unwrap();

        let query = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(query, "disableExpiry=true");
    }
}
