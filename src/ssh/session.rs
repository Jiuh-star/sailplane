//! One SSH session: handshake, PTY, and the byte pumps that feed the terminal.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use russh::client::{self, Config, Handle};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelWriteHalf};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::config::SshConfig;

/// Anything the SSH transport can run over: a plain TCP stream or a SOCKS5
/// stream into the tailnet.
pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

pub type Stream = Pin<Box<dyn AsyncReadWrite>>;

/// A message travelling from the browser to the session.
#[derive(Debug, Clone)]
pub enum SessionRequest {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

/// The live pieces of a session, handed to the WebSocket bridge.
pub struct SessionHandle {
    /// Input and resize requests for this session.
    pub requests: mpsc::Sender<SessionRequest>,
    /// Terminal output; ends when the session closes. Taken once by the
    /// bridge, which is the only consumer.
    pub output: Option<mpsc::Receiver<Vec<u8>>>,
    /// The SSH connection. Held for its side effect: dropping it closes the
    /// session, so whoever owns the handle owns the session's lifetime.
    #[allow(dead_code)]
    pub handle: Handle<ClientHandler>,
}

impl SessionHandle {
    /// Takes the output stream. Returns `None` after the first call.
    pub fn take_output(&mut self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.output.take()
    }

    /// Sends a resize to the remote PTY.
    pub async fn resize(&self, cols: u32, rows: u32) -> bool {
        self.requests
            .send(SessionRequest::Resize { cols, rows })
            .await
            .is_ok()
    }

    /// Sends terminal input.
    pub async fn write(&self, data: Vec<u8>) -> bool {
        self.requests.send(SessionRequest::Data(data)).await.is_ok()
    }
}

/// Accepts every host key.
///
/// Tailscale SSH authenticates the peer with WireGuard, not host keys. For a
/// plain sshd, the operator trusts the target by configuring it. The UI states
/// this before the session opens.
#[derive(Clone)]
pub struct ClientHandler;

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Runs the handshake, requests a PTY and a shell, and returns the live halves.
pub async fn open(
    stream: Stream,
    username: &str,
    config: &SshConfig,
    cols: u32,
    rows: u32,
    timeout: Duration,
) -> Result<SessionHandle> {
    let client_config = Arc::new(Config {
        // Keep every algorithm the server is willing to negotiate; tailnet
        // appliances and embedded sshd builds vary widely.
        preferred: russh::Preferred::default(),
        ..Config::default()
    });

    let mut session = tokio::time::timeout(
        timeout,
        client::connect_stream(client_config, stream, ClientHandler),
    )
    .await
    .context("the SSH handshake timed out")?
    .context("the SSH handshake failed")?;

    authenticate(&mut session, username, config).await?;

    let channel = session
        .channel_open_session()
        .await
        .context("the server refused to open a session channel")?;

    channel
        .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
        .await
        .context("the server refused a PTY")?;
    channel
        .request_shell(true)
        .await
        .context("the server refused a shell")?;

    let (mut reader, writer) = channel.split();

    let (request_tx, mut request_rx) = mpsc::channel::<SessionRequest>(64);
    let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(256);

    // Input pump: browser -> SSH. It owns the write half, which is why the
    // handle exposes a channel rather than the writer itself.
    let writer: ChannelWriteHalf<russh::client::Msg> = writer;
    tokio::spawn(async move {
        while let Some(request) = request_rx.recv().await {
            let result = match request {
                SessionRequest::Data(bytes) => writer.data_bytes(bytes).await,
                SessionRequest::Resize { cols, rows } => {
                    writer.window_change(cols, rows, 0, 0).await
                }
            };
            if let Err(err) = result {
                tracing::warn!("SSH channel write failed: {err}");
                break;
            }
        }
    });

    // Output pump: SSH -> browser.
    tokio::spawn(async move {
        loop {
            match reader.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    if output_tx.send(data.to_vec()).await.is_err() {
                        break;
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    if output_tx.send(data.to_vec()).await.is_err() {
                        break;
                    }
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                _ => {}
            }
        }
    });

    Ok(SessionHandle {
        requests: request_tx,
        output: Some(output_rx),
        handle: session,
    })
}

/// Tries each configured credential in turn.
async fn authenticate(
    session: &mut Handle<ClientHandler>,
    username: &str,
    config: &SshConfig,
) -> Result<()> {
    // Tailscale SSH authenticates the *node*, not the user: it accepts the
    // "none" method and derives the identity from the WireGuard peer.
    match session.authenticate_none(username).await {
        Ok(result) if result.success() => return Ok(()),
        Ok(_) => {}
        Err(err) => tracing::debug!("SSH none-auth was rejected: {err}"),
    }

    if let Some(path) = config.private_key_path.as_ref()
        && try_public_key(session, username, path).await?
    {
        return Ok(());
    }

    if let Some(password) = config.password.as_deref() {
        let result = session
            .authenticate_password(username, password)
            .await
            .context("password authentication failed")?;
        if result.success() {
            return Ok(());
        }
    }

    bail!(
        "the server rejected every configured SSH credential for `{username}`. For Tailscale \
         SSH, check the ACL `ssh` rules; for a plain sshd, set `integration.ssh.private_key_path`."
    )
}

/// Attempts public-key auth. Returns whether it succeeded.
async fn try_public_key(
    session: &mut Handle<ClientHandler>,
    username: &str,
    path: &std::path::Path,
) -> Result<bool> {
    let key = match russh::keys::load_secret_key(path, None) {
        Ok(key) => key,
        Err(err) => {
            tracing::warn!(
                "could not load the SSH key at {}: {err}; continuing without it",
                path.display()
            );
            return Ok(false);
        }
    };

    // RSA keys need an explicit hash algorithm; let the server advertise one.
    let hash_alg = session
        .best_supported_rsa_hash()
        .await
        .ok()
        .flatten()
        .flatten();

    let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
    let result = session
        .authenticate_publickey(username, key)
        .await
        .context("public key authentication failed")?;

    Ok(result.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_requests_clone() {
        let request = SessionRequest::Resize { cols: 80, rows: 24 };
        match request.clone() {
            SessionRequest::Resize { cols, rows } => assert_eq!((cols, rows), (80, 24)),
            _ => panic!("wrong variant"),
        }
    }
}
