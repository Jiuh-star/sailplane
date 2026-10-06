//! Browser SSH: server-side SSH sessions to tailnet nodes.
//!
//! Upstream joins the browser to the tailnet with a Go/WASM node. Sailplane
//! instead opens a real SSH connection from the server and bridges the terminal
//! over a WebSocket. The connection goes through the SOCKS5 proxy
//! (`TS_SOCKS5_SERVER`) of the userspace `tailscaled` sidecar, the same process
//! the host-info agent reads.

pub mod session;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::config::SshConfig;

pub use session::SessionHandle;

/// How long to wait for the SSH handshake before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Shared SSH client state.
#[derive(Clone)]
pub struct SshService {
    inner: Arc<Inner>,
}

struct Inner {
    config: SshConfig,
    /// `None` when the feature is disabled, with the reason why.
    disabled_reason: Option<String>,
}

impl SshService {
    pub fn new(config: SshConfig, disabled_reason: Option<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                disabled_reason,
            }),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.config.enabled && self.inner.disabled_reason.is_none()
    }

    pub fn disabled_reason(&self) -> Option<&str> {
        self.inner.disabled_reason.as_deref()
    }
    /// Returns the configured SSH port, defaulting to 22.
    pub fn port(&self) -> u16 {
        self.inner.config.port.unwrap_or(22)
    }

    /// The user to sign in as when the request does not name one.
    pub fn default_username(&self) -> Option<&str> {
        self.inner.config.username.as_deref()
    }

    /// Opens a session to `host`, optionally through the configured proxy.
    pub async fn connect(
        &self,
        host: &str,
        port: u16,
        username: &str,
        cols: u32,
        rows: u32,
    ) -> Result<SessionHandle> {
        if !self.is_enabled() {
            bail!(
                "{}",
                self.inner
                    .disabled_reason
                    .clone()
                    .unwrap_or_else(|| "browser SSH is not enabled".into())
            );
        }

        let stream = self.dial(host, port).await?;
        session::open(
            stream,
            username,
            &self.inner.config,
            cols,
            rows,
            CONNECT_TIMEOUT,
        )
        .await
        .with_context(|| format!("could not open an SSH session to {host}"))
    }

    /// Opens a TCP connection to the target, through SOCKS5 when configured.
    async fn dial(&self, host: &str, port: u16) -> Result<session::Stream> {
        match self.inner.config.proxy.as_deref() {
            Some(proxy) => {
                let proxy = parse_proxy(proxy)?;
                let stream = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    tokio_socks::tcp::Socks5Stream::connect(proxy.as_str(), (host, port)),
                )
                .await
                .context("the SOCKS5 proxy did not answer in time")?
                .with_context(|| {
                    format!("the SOCKS5 proxy at {proxy} could not reach {host}:{port}")
                })?;
                Ok(Box::pin(stream))
            }
            None => {
                let stream = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    tokio::net::TcpStream::connect((host, port)),
                )
                .await
                .context("the connection attempt timed out")?
                .with_context(|| format!("could not reach {host}:{port}"))?;
                Ok(Box::pin(stream))
            }
        }
    }
}

/// Accepts `host:port`, `socks5://host:port` or `socks5h://host:port`.
fn parse_proxy(input: &str) -> Result<String> {
    let stripped = input
        .strip_prefix("socks5h://")
        .or_else(|| input.strip_prefix("socks5://"))
        .or_else(|| input.strip_prefix("socks://"))
        .unwrap_or(input);

    if stripped.is_empty() {
        bail!("the configured SSH proxy is empty");
    }
    Ok(stripped.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_schemes_are_stripped() {
        assert_eq!(
            parse_proxy("socks5://127.0.0.1:1080").unwrap(),
            "127.0.0.1:1080"
        );
        assert_eq!(
            parse_proxy("socks5h://127.0.0.1:1080").unwrap(),
            "127.0.0.1:1080"
        );
        assert_eq!(parse_proxy("127.0.0.1:1080").unwrap(), "127.0.0.1:1080");
        assert!(parse_proxy("socks5://").is_err());
    }

    #[test]
    fn disabled_service_reports_a_reason() {
        let service = SshService::new(SshConfig::default(), Some("no ssh proxy".into()));
        assert!(!service.is_enabled());
        assert_eq!(service.disabled_reason(), Some("no ssh proxy"));
        assert_eq!(service.port(), 22);
    }

    #[test]
    fn port_defaults_and_overrides() {
        let mut config = SshConfig {
            enabled: true,
            ..SshConfig::default()
        };
        assert_eq!(SshService::new(config.clone(), None).port(), 22);

        config.port = Some(2222);
        assert_eq!(SshService::new(config, None).port(), 2222);
    }
}
