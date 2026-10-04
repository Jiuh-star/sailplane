//! Server-sent events for live snapshot updates.
//!
//! The client keeps a copy of every resource and refetches one only when its
//! version changes, so an idle dashboard costs one heartbeat every 15 s.

use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::state::{Auth, SharedState};

/// How often an idle stream emits a comment.
///
/// This also bounds how long an abandoned stream lingers: a task parked on the
/// broadcast receiver only notices a vanished client when the next write
/// fails.
const HEARTBEAT: Duration = Duration::from_secs(15);

/// Streams live snapshot changes over SSE. `GET /events/live`
pub async fn stream(axum::extract::State(state): axum::extract::State<SharedState>, Auth(_principal): Auth) -> Response {
    let live = state.live.clone();

    // Snapshot versions at subscribe time, then stream every change.
    let mut receiver = live.subscribe();

    let hello = json!({
        "nodes": live.nodes_version_blocking(),
        "users": live.users_version_blocking(),
    });

    // An event feed never ends on its own, and graceful shutdown waits for it.
    let mut shutdown = state.shutdown.clone();

    let stream = async_stream::stream! {
        yield Ok::<Event, std::convert::Infallible>(Event::default().event("hello").data(hello.to_string()));

        loop {
            // Closing the stream lets the server drain and exit.
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                result = receiver.recv() => match result {
                Ok(change) => {
                    let data = serde_json::to_string(&change).unwrap_or_else(|_| "{}".into());
                    yield Ok(Event::default().event("changed").data(data));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    // The client fell behind; tell it to refetch everything
                    // rather than replaying a partial history.
                    tracing::debug!("live stream lagged by {skipped} events; forcing resync");
                    let resync = json!({
                        "nodes": live.nodes_version_blocking(),
                        "users": live.users_version_blocking(),
                    });
                    yield Ok(Event::default().event("resync").data(resync.to_string()));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            }
        }
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(HEARTBEAT)
                .text("heartbeat"),
        )
        .into_response()
}
