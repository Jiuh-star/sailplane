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

/// How long to wait for the SSH handshake before the attempt stops.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Shared SSH client state.
#[derive(Clone)]
pub struct SshService {
    inner: Arc<Inner>,
}

struct Inner {
    /// [`SshService::reconfigure`] swaps it when a setting changes, so the
    /// service tracks the current configuration without a restart.
    config: std::sync::RwLock<SshConfig>,
    /// `None` when the feature is disabled, with the reason why.
    disabled_reason: std::sync::RwLock<Option<String>>,
}

impl SshService {
    pub fn new(config: SshConfig, disabled_reason: Option<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                config: std::sync::RwLock::new(config),
                disabled_reason: std::sync::RwLock::new(disabled_reason),
            }),
        }
    }

    /// Swaps in a fresh configuration so a saved setting takes effect without a
    /// restart.
    pub fn reconfigure(&self, config: SshConfig, disabled_reason: Option<String>) {
        *self.inner.config.write().expect("config lock") = config;
        *self.inner.disabled_reason.write().expect("reason lock") = disabled_reason;
    }

    fn config(&self) -> SshConfig {
        self.inner.config.read().expect("config lock").clone()
    }

    fn reason(&self) -> Option<String> {
        self.inner.disabled_reason.read().expect("reason lock").clone()
    }

    pub fn is_enabled(&self) -> bool {
        self.config().enabled && self.reason().is_none()
    }

    /// Whether the feature is turned on in configuration, regardless of whether
    /// it can currently connect. Distinguishes "off" from "on but broken" so
    /// the UI can explain the right way to enable it.
    pub fn configured(&self) -> bool {
        self.config().enabled
    }

    pub fn disabled_reason(&self) -> Option<String> {
        self.reason()
    }

    /// Returns the configured SSH port, with a default of 22.
    pub fn port(&self) -> u16 {
        self.config().port.unwrap_or(22)
    }

    /// The user to sign in as when the request does not name one.
    pub fn default_username(&self) -> Option<String> {
        self.config().username
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
        let config = self.config();
        if !(config.enabled && self.reason().is_none()) {
            bail!(
                "{}",
                self.reason()
                    .unwrap_or_else(|| "browser SSH is not enabled".into())
            );
        }

        let stream = self.dial(&config, host, port).await?;
        session::open(stream, username, &config, cols, rows, CONNECT_TIMEOUT)
            .await
            .with_context(|| format!("could not open an SSH session to {host}"))
    }

    /// Opens a TCP connection to the target, through SOCKS5 when configured.
    async fn dial(&self, config: &SshConfig, host: &str, port: u16) -> Result<session::Stream> {
        match config.proxy.as_deref() {
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

/// Explains why browser SSH cannot run, or `None` when it can.
///
/// Recomputed whenever a setting is saved, so the reason tracks the current
/// configuration instead of being frozen at startup.
pub fn disabled_reason(config: &SshConfig) -> Option<String> {
    if !config.enabled {
        return None;
    }
    // A proxy is how Sailplane reaches a tailnet it is not itself a member of.
    // Without one, direct connectivity is assumed.
    if let Some(proxy) = config.proxy.as_deref() {
        let address = proxy
            .trim_start_matches("socks5h://")
            .trim_start_matches("socks5://")
            .trim_start_matches("socks://");
        if std::net::TcpStream::connect_timeout(
            &address
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:0".parse().expect("valid literal")),
            std::time::Duration::from_millis(500),
        )
        .is_err()
        {
            return Some(format!(
                "The configured SSH proxy at {address} is not reachable. If you rely on the \
                 sidecar tailscaled, set `TS_SOCKS5_SERVER` so it exposes one."
            ));
        }
    }
    None
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
        assert_eq!(service.disabled_reason(), Some("no ssh proxy".to_string()));
        assert_eq!(service.port(), 22);
    }

    #[test]
    fn reconfiguring_takes_effect_without_a_restart() {
        let service = SshService::new(SshConfig::default(), None);
        assert!(!service.is_enabled());

        service.reconfigure(
            SshConfig {
                enabled: true,
                username: Some("root".into()),
                ..SshConfig::default()
            },
            None,
        );
        assert!(service.is_enabled());
        assert_eq!(service.default_username().as_deref(), Some("root"));

        // Turning it off again is just another swap.
        service.reconfigure(SshConfig::default(), None);
        assert!(!service.is_enabled());
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
