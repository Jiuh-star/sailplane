//! The Headscale log stream.
//!
//! Reading the logs needs the container runtime, which only the Docker
//! integration has: Headscale's own API exposes no log endpoint, and the
//! process integration can send a signal but not read stdout.

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::auth::Capability;

use super::super::error::{ApiError, ApiResult};
use super::super::state::{Auth, PrincipalExt, SharedState};

#[derive(Deserialize)]
pub struct LogQuery {
    #[serde(default)]
    tail: Option<usize>,
    /// `true`/`false` and `1`/`0` both work: a query string is not JSON.
    #[serde(default, deserialize_with = "yes_or_no")]
    follow: bool,
}

fn yes_or_no<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" | "" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "`{other}` is not a yes or no value"
        ))),
    }
}

/// Streams the Headscale container logs as plain text. `GET /api/logs`
pub async fn stream(
    State(state): State<SharedState>,
    Auth(principal): Auth,
    Query(query): Query<LogQuery>,
) -> ApiResult<Response> {
    principal.require(&[Capability::ReadFeature])?;

    let tail = query.tail.unwrap_or(200).clamp(10, 5000);
    let follow = query.follow;

    let mut shutdown = state.shutdown.clone();
    let logs = state
        .integration
        .log_stream(tail, follow)
        .await
        .map_err(|err| ApiError::bad_request(format!("{err:#}")))?;

    // A followed log never ends on its own, and graceful shutdown waits for
    // it; end it here so the process can exit.
    use futures_util::StreamExt;
    let mut logs = Box::pin(logs);
    let stream = async_stream::stream! {
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                next = logs.next() => match next {
                    Some(chunk) => yield chunk,
                    None => break,
                },
            }
        }
    };

    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            // The body arrives over minutes; nothing may buffer or compress it.
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::HeaderName::from_static("x-accel-buffering"),
                HeaderValue::from_static("no"),
            ),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}
