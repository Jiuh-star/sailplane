//! Minimal HTTP/1.1 client for unix domain sockets.
//!
//! Docker's Engine API and Tailscale's LocalAPI both speak HTTP over a unix socket.
//! Both are trusted internal endpoints, so one request per connection is enough.

use std::path::Path;

use anyhow::{Context, Result, bail};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

/// A response from a unix-socket HTTP endpoint.
pub struct SocketResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl SocketResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).context("failed to parse the JSON response")
    }

    /// Returns the body as text for error messages.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).trim().to_string()
    }
}
/// Sends one HTTP request over `socket`.
///
/// `host` overrides the `Host` header. Tailscale's LocalAPI rejects any other
/// value; hyperlocal would otherwise send the URI authority.
pub async fn request_from(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    host: Option<&str>,
) -> Result<SocketResponse> {
    let socket = socket.to_path_buf();
    if !socket.exists() {
        bail!("socket {} does not exist", socket.display());
    }

    let client: Client<hyperlocal::UnixConnector, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build(hyperlocal::UnixConnector);

    let uri: hyper::Uri = hyperlocal::Uri::new(&socket, path).into();

    let mut builder = hyper::Request::builder().method(method).uri(uri);
    if let Some(host) = host {
        builder = builder.header(hyper::header::HOST, host);
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }
    let request = builder
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .context("failed to build the request")?;

    let response = client
        .request(request)
        .await
        .with_context(|| format!("failed to reach {} over {}", path, socket.display()))?;

    let status = response.status().as_u16();
    let bytes = response
        .into_body()
        .collect()
        .await
        .context("failed to read the response body")?
        .to_bytes();

    Ok(SocketResponse {
        status,
        body: bytes.to_vec(),
    })
}
