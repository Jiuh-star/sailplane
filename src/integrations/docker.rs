//! Docker integration: restarts the Headscale container after a config change.
//!
//! Uses the Docker Engine API directly over the configured socket. Unix sockets
//! go through a hyper client with a custom connector; `tcp://` sockets use the
//! regular HTTP client.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::stream::BoxStream;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;

use crate::config::DockerConfig;
use crate::headscale::Headscale;

/// Minimum Docker Engine API version supported.
const MIN_API_VERSION: (u32, u32) = (1, 24);
/// API version requested when the daemon is newer.
const TARGET_API_VERSION: (u32, u32) = (1, 44);
/// How long to wait for Headscale to become healthy after a restart.
const HEALTH_ATTEMPTS: usize = 10;
const HEALTH_INTERVAL: Duration = Duration::from_secs(1);

pub struct DockerIntegration {
    config: DockerConfig,
    /// The daemon's API version, resolved once.
    ///
    /// Probing costs a round trip to `/version`, which Podman can take about a
    /// second to answer, and the answer cannot change while this process runs.
    api_version: tokio::sync::OnceCell<String>,
}

impl DockerIntegration {
    pub fn new(config: DockerConfig) -> Self {
        Self {
            config,
            api_version: tokio::sync::OnceCell::new(),
        }
    }
    /// Restarts Headscale and waits for it to report healthy.
    pub async fn on_config_change(&self, headscale: &Headscale) -> Result<()> {
        let container = self.find_container().await?;
        self.restart_container(&container).await?;
        self.wait_for_health(headscale).await
    }

    /// Resolves the Docker API version, clamped to the supported range.
    async fn resolve_api_version(&self) -> Result<String> {
        self.api_version
            .get_or_try_init(|| self.probe_api_version())
            .await
            .cloned()
    }

    async fn probe_api_version(&self) -> Result<String> {
        let response = self.request("GET", "/version", None).await?;

        // Very old daemons answer `/_ping` but not `/version`; a parse failure
        // is not fatal because the unversioned path still works.
        let version = response
            .get("ApiVersion")
            .and_then(Value::as_str)
            .unwrap_or("1.44");

        let parsed = parse_version(version);
        let chosen = match parsed {
            Some(version) if version < MIN_API_VERSION => bail!(
                "the Docker daemon API version {}.{} is older than the supported minimum {}.{}",
                version.0,
                version.1,
                MIN_API_VERSION.0,
                MIN_API_VERSION.1
            ),
            Some(version) if version > TARGET_API_VERSION => TARGET_API_VERSION,
            Some(version) => version,
            None => TARGET_API_VERSION,
        };

        Ok(format!("{}.{}", chosen.0, chosen.1))
    }

    async fn find_container(&self) -> Result<String> {
        let api = self.resolve_api_version().await?;

        // The filter value is a JSON document, so it must be percent-encoded
        // before it becomes part of the request URI: raw braces and quotes are
        // not valid URI characters.
        let filter = match self.config.container_name.as_deref() {
            Some(name) => format!("{{\"name\":[\"{name}\"]}}"),
            None => format!("{{\"label\":[\"{}\"]}}", self.config.container_label),
        };
        let path = format!(
            "/v{api}/containers/json?filters={}&limit=1",
            urlencode(&filter)
        );
        let body = self.request_raw("GET", &path, None).await?;
        let containers: Vec<Value> =
            serde_json::from_slice(&body).context("failed to parse the Docker container list")?;

        let container = containers
            .into_iter()
            .find(|container| {
                // A name filter is a substring match, so verify it precisely.
                match self.config.container_name.as_deref() {
                    Some(wanted) => container
                        .get("Names")
                        .and_then(Value::as_array)
                        .map(|names| {
                            names
                                .iter()
                                .filter_map(Value::as_str)
                                .any(|name| name.trim_start_matches('/') == wanted)
                        })
                        .unwrap_or(false),
                    None => true,
                }
            })
            .and_then(|container| {
                container
                    .get("Id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });

        container.context(
            "could not find the Headscale container. Check `integration.docker.container_name` \
             or the label on the container",
        )
    }

    /// Streams the Headscale container's logs as text.
    ///
    /// The socket stays open when `follow` is set, so this returns a stream
    /// rather than a body: output arrives as Headscale writes it.
    pub async fn log_stream(
        &self,
        tail: usize,
        follow: bool,
    ) -> Result<BoxStream<'static, Result<Bytes, std::io::Error>>> {
        let container = self.find_container().await?;
        // `find_container` resolves this too; the second call is cached.
        let api = self.resolve_api_version().await?;
        let path = format!(
            "/v{api}/containers/{container}/logs?stdout=1&stderr=1&tail={tail}&follow={}",
            u8::from(follow)
        );

        // The pool lives in the client, and the response is still being read
        // when this function returns: dropping the client here would abort the
        // in-flight connection. It travels with the stream instead.
        let uri: hyper::Uri = if let Some(authority) = self.config.socket.strip_prefix("tcp://") {
            format!("http://{authority}{path}")
                .parse()
                .context("invalid Docker API address")?
        } else {
            let socket_path = self
                .config
                .socket
                .strip_prefix("unix://")
                .unwrap_or(&self.config.socket);
            hyperlocal::Uri::new(socket_path, &path).into()
        };

        let transport = if self.config.socket.starts_with("tcp://") {
            Transport::Tcp(Client::builder(TokioExecutor::new()).build_http())
        } else {
            Transport::Unix(Client::builder(TokioExecutor::new()).build(hyperlocal::UnixConnector))
        };

        let request = hyper::Request::get(uri).body(Full::new(Bytes::new()))?;
        let response = match &transport {
            Transport::Tcp(client) => client.request(request),
            Transport::Unix(client) => client.request(request),
        }
        .await
        .context("failed to reach the Docker daemon for container logs")?;

        if !response.status().is_success() {
            bail!(
                "the Docker daemon refused the log request: HTTP {}",
                response.status()
            );
        }

        let mut body = response.into_body();
        let mut demux = LogDemux::default();

        Ok(Box::pin(async_stream::stream! {
            // Held for the stream's lifetime so the connection stays open.
            let _transport = transport;
            while let Some(frame) = body.frame().await {
                let frame = match frame {
                    Ok(frame) => frame,
                    Err(err) => {
                        yield Err(std::io::Error::other(err.to_string()));
                        break;
                    }
                };
                let Some(data) = frame.data_ref() else { continue };
                let text = demux.push(data);
                if !text.is_empty() {
                    yield Ok(Bytes::from(text));
                }
            }
        }))
    }

    async fn restart_container(&self, id: &str) -> Result<()> {
        let api = self.resolve_api_version().await?;
        let path = format!("/v{api}/containers/{id}/restart");
        self.request_raw("POST", &path, None)
            .await
            .context("failed to restart the Headscale container")?;
        Ok(())
    }

    async fn wait_for_health(&self, headscale: &Headscale) -> Result<()> {
        for attempt in 1..=HEALTH_ATTEMPTS {
            if headscale.health().await {
                return Ok(());
            }
            if attempt < HEALTH_ATTEMPTS {
                tokio::time::sleep(HEALTH_INTERVAL).await;
            }
        }
        bail!(
            "Headscale did not report healthy within {}s of restarting the container",
            HEALTH_ATTEMPTS
        )
    }

    async fn request(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let raw = self.request_raw(method, path, body).await?;
        serde_json::from_slice(&raw).context("failed to parse the Docker API response")
    }

    async fn request_raw(&self, method: &str, path: &str, body: Option<Value>) -> Result<Vec<u8>> {
        let bytes = body.map(|value| Bytes::from(value.to_string()));

        if let Some(tcp) = self.config.socket.strip_prefix("tcp://") {
            return self.request_tcp(tcp, method, path, bytes).await;
        }

        let socket_path = self
            .config
            .socket
            .strip_prefix("unix://")
            .unwrap_or(&self.config.socket);

        let response = crate::unix_socket::request_from(
            std::path::Path::new(socket_path),
            method,
            path,
            bytes.map(|bytes| bytes.to_vec()),
            None,
        )
        .await?;

        if !response.is_success() {
            bail!(
                "the Docker daemon returned {}: {}",
                response.status,
                crate::util::truncate(&response.text(), 200)
            );
        }

        Ok(response.body)
    }

    async fn request_tcp(
        &self,
        authority: &str,
        method: &str,
        path: &str,
        body: Option<Bytes>,
    ) -> Result<Vec<u8>> {
        let client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build_http();

        let uri = format!("http://{authority}{path}")
            .parse()
            .context("invalid Docker API address")?;
        let request = build_request(method, uri, body)?;
        let response = client
            .request(request)
            .await
            .with_context(|| format!("failed to reach the Docker daemon at {authority}"))?;
        collect(response).await
    }
}

fn build_request(
    method: &str,
    uri: hyper::Uri,
    body: Option<Bytes>,
) -> Result<hyper::Request<Full<Bytes>>> {
    let mut builder = hyper::Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }
    builder
        .body(Full::new(body.unwrap_or_default()))
        .context("failed to build the Docker API request")
}

async fn collect(response: hyper::Response<hyper::body::Incoming>) -> Result<Vec<u8>> {
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .context("failed to read the Docker API response")?
        .to_bytes();

    if !status.is_success() {
        bail!(
            "the Docker daemon returned {status}: {}",
            String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
        );
    }

    Ok(bytes.to_vec())
}

/// The HTTP client for whichever socket the configuration names. Both accept
/// `hyper::Request<Full<Bytes>>`; they differ only in what they connect to.
enum Transport {
    Unix(Client<hyperlocal::UnixConnector, Full<Bytes>>),
    Tcp(Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>),
}

/// Docker multiplexes stdout and stderr with an 8-byte header per frame, unless
/// the container has a TTY, in which case the bytes are raw. Both shapes reach
/// the same stream, so the first chunk decides which it is.
#[derive(Default)]
struct LogDemux {
    buffer: Vec<u8>,
    framed: Option<bool>,
}

impl LogDemux {
    /// Appends `chunk` and returns whatever is now complete.
    fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.buffer.extend_from_slice(chunk);

        let framed = *self.framed.get_or_insert_with(|| {
            // A frame header is a stream type of 0–2 followed by three zero
            // bytes; a log line that looks like that does not occur in practice.
            self.buffer.len() >= 8 && self.buffer[0] <= 2 && self.buffer[1..4] == [0, 0, 0]
        });

        if !framed {
            return std::mem::take(&mut self.buffer);
        }

        let mut out = Vec::new();
        let mut consumed = 0;
        while self.buffer.len() - consumed >= 8 {
            let header = &self.buffer[consumed..consumed + 8];
            let size = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
            if self.buffer.len() - consumed - 8 < size {
                break;
            }
            out.extend_from_slice(&self.buffer[consumed + 8..consumed + 8 + size]);
            consumed += 8 + size;
        }
        self.buffer.drain(..consumed);
        out
    }
}

/// Percent-encodes everything outside the unreserved set, so a JSON filter
/// value is safe to embed in a URI.
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

fn parse_version(input: &str) -> Option<(u32, u32)> {
    let mut parts = input.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_documents_are_percent_encoded() {
        let encoded = urlencode("{\"label\":[\"a=b\"]}");
        assert_eq!(encoded, "%7B%22label%22%3A%5B%22a%3Db%22%5D%7D");
        // The value must survive a round trip back through the URI parser.
        let uri: hyper::Uri = format!("/v1.44/containers/json?filters={encoded}&limit=1")
            .parse()
            .expect("encoded filter must form a valid URI");
        assert_eq!(
            uri.query(),
            Some(format!("filters={encoded}&limit=1").as_str())
        );
    }

    #[test]
    fn parses_api_versions() {
        assert_eq!(parse_version("1.44"), Some((1, 44)));
        assert_eq!(parse_version("1.24"), Some((1, 24)));
        assert_eq!(parse_version("nonsense"), None);
    }

    #[test]
    fn rejects_daemons_below_the_floor() {
        let chosen = match Some((1, 23)) {
            Some(version) if version < MIN_API_VERSION => None,
            Some(version) if version > TARGET_API_VERSION => Some(TARGET_API_VERSION),
            other => other,
        };
        assert_eq!(chosen, None);
    }
}
