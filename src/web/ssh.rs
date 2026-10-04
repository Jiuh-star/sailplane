//! Browser SSH endpoints.
//!
//! The browser connects a terminal over a WebSocket; the server opens a real
//! SSH session to the machine and pumps bytes between the two. See `crate::ssh`
//! for why the client lives on the server rather than in the tab.

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{Capability, Principal};
use crate::headscale::Machine;

use super::error::{ApiError, ApiResult};
use super::state::{Auth, SharedState};

#[derive(Deserialize, Default)]
pub struct ConnectQuery {
    user: Option<String>,
    cols: Option<u32>,
    rows: Option<u32>,
}

/// The bridge reaches the target with the *server's* tailnet credentials, so
/// the connecting user is never authenticated there. Access is therefore gated
/// like any other machine-scoped action, not on read access alone.
fn require_access(principal: &Principal, node: &Machine) -> Result<(), ApiError> {
    if !principal.has(Capability::ReadMachines) {
        return Err(ApiError::forbidden(
            "Your account does not have access to machines",
        ));
    }
    if !principal.can_manage_node(node) {
        return Err(ApiError::forbidden(
            "Browser SSH connects with the server's own tailnet identity, so it is \
             limited to machines your account manages",
        ));
    }
    Ok(())
}

/// Returns what the terminal page needs before connecting. `GET /api/ssh/{id}`
pub async fn info(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let nodes = state.live.nodes().await;
    let node = nodes
        .data
        .iter()
        .find(|node| node.id == id || node.given_name == id)
        .ok_or_else(|| ApiError::not_found("That machine does not exist"))?;

    require_access(&principal, node)?;

    let host_info = state.agent.host_info().await.unwrap_or_default();
    let view = super::presentation::MachineView::build(node, host_info.get(&node.node_key));

    let service = &state.ssh;
    let available = service.is_enabled() && view.ipv4.is_some();

    Ok(Json(json!({
        "machine": {
            "id": node.id,
            "name": node.given_name,
            "online": node.online,
            "ipv4": view.ipv4,
            "os": view.os,
            "sshHostKeys": view.ssh_host_keys,
        },
        "available": available,
        "reason": service
            .disabled_reason()
            .map(str::to_string)
            .or_else(|| view.ipv4.is_none().then(|| "This machine has no IPv4 address to reach.".into())),
        "defaultUser": service.default_username(),
        "port": service.port(),
        // Tailscale SSH has no host keys, and an ordinary target was chosen by
        // the operator, so the bridge accepts whatever key the machine presents.
        "hostKeyVerified": false,
    })))
}

/// Upgrades the connection to the terminal bridge. `GET /api/ssh/{id}/ws`
pub async fn connect(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Path(id): Path<String>,
    Query(query): Query<ConnectQuery>,
    headers: axum::http::HeaderMap,
) -> ApiResult<Response> {
    // A WebSocket is not covered by CORS, and the handshake is a GET, so the
    // origin middleware never sees it. Require a same-origin `Origin` here
    // rather than relying on `SameSite` alone.
    if super::middleware::origin_matches_host(&headers) != Some(true) {
        return Err(ApiError::forbidden(
            "The terminal only accepts same-origin connections",
        ));
    }

    if !state.ssh.is_enabled() {
        return Err(ApiError::bad_request(
            state
                .ssh
                .disabled_reason()
                .unwrap_or("Browser SSH is not enabled"),
        ));
    }

    let nodes = state.live.nodes().await;
    let node = nodes
        .data
        .iter()
        .find(|node| node.id == id || node.given_name == id)
        .ok_or_else(|| ApiError::not_found("That machine does not exist"))?;

    require_access(&principal, node)?;

    let host = node
        .ipv4()
        .map(str::to_string)
        .ok_or_else(|| ApiError::bad_request("This machine has no IPv4 address to reach"))?;

    let username = query
        .user
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| state.ssh.default_username().map(str::to_string))
        .ok_or_else(|| {
            ApiError::bad_request("No SSH user was given and no default is configured")
        })?;

    let cols = query.cols.unwrap_or(80).clamp(20, 500);
    let rows = query.rows.unwrap_or(24).clamp(5, 300);
    let port = state.ssh.port();
    let service = state.ssh.clone();
    let label = node.given_name.clone();

    let shutdown = state.shutdown.clone();
    Ok(ws
        .on_upgrade(move |socket| {
            bridge(socket, service, host, port, username, cols, rows, label, shutdown)
        })
        .into_response())
}

/// Pumps bytes between the WebSocket and the SSH session.
///
/// Conventions:
///   * browser -> server: binary frames are terminal input; text frames are
///     JSON control messages (`resize`).
///   * server -> browser: binary frames are terminal output; text frames are
///     JSON status (`ready`, `exit`, `error`).
#[allow(clippy::too_many_arguments)]
async fn bridge(
    socket: WebSocket,
    service: crate::ssh::SshService,
    host: String,
    port: u16,
    username: String,
    cols: u32,
    rows: u32,
    label: String,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let (mut sink, mut stream) = socket.split();

    tracing::info!("opening an SSH session to {label} ({host}:{port}) as {username}");

    let mut session = match service.connect(&host, port, &username, cols, rows).await {
        Ok(session) => session,
        Err(err) => {
            let message = format!("{err:#}");
            tracing::warn!("SSH session to {label} failed: {message}");
            let _ = sink
                .send(Message::Text(
                    json!({ "type": "error", "message": message })
                        .to_string()
                        .into(),
                ))
                .await;
            let _ = sink.close().await;
            return;
        }
    };

    if sink
        .send(Message::Text(
            json!({ "type": "ready", "user": username, "host": host })
                .to_string()
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    let Some(mut output) = session.take_output() else {
        return;
    };
    let input = std::sync::Arc::new(session);

    // Terminal output -> browser.
    let outbound = tokio::spawn(async move {
        while let Some(chunk) = output.recv().await {
            if sink.send(Message::Binary(chunk.into())).await.is_err() {
                break;
            }
        }
        let _ = sink
            .send(Message::Text(json!({ "type": "exit" }).to_string().into()))
            .await;
        let _ = sink.close().await;
    });

    // Browser input -> terminal.
    let inbound_input = std::sync::Arc::clone(&input);
    let inbound = tokio::spawn(async move {
        let input = inbound_input;
        loop {
            let Some(message) = stream.next().await else {
                break;
            };
            match message {
                Ok(Message::Binary(data)) => {
                    if !input.write(data.to_vec()).await {
                        break;
                    }
                }
                Ok(Message::Text(text)) => {
                    // A text frame is either a JSON control message, or
                    // terminal input when it is not valid JSON. `ws.send("…")`
                    // in the browser produces text.
                    let Ok(control) = serde_json::from_str::<Value>(&text) else {
                        if !input.write(text.as_bytes().to_vec()).await {
                            break;
                        }
                        continue;
                    };
                    if control.get("type").and_then(Value::as_str) == Some("resize") {
                        let cols = control
                            .get("cols")
                            .and_then(Value::as_u64)
                            .unwrap_or(80)
                            .clamp(20, 500) as u32;
                        let rows = control
                            .get("rows")
                            .and_then(Value::as_u64)
                            .unwrap_or(24)
                            .clamp(5, 300) as u32;
                        if !input.resize(cols, rows).await {
                            break;
                        }
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    // Whichever half finishes first ends the session. A shutdown does the
    // same, because it would otherwise wait on a terminal nobody will close.
    tokio::select! {
        _ = outbound => {}
        _ = inbound => {}
        _ = shutdown.changed() => {}
    }

    // Dropping the last handle closes the connection and the remote shell.
    drop(input);
    tracing::info!("SSH session to {label} closed");
}
