//! Uniform JSON error responses.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::headscale::HeadscaleError;

/// An error rendered to the SPA as `{"error": {"message": ..., "code": ...}}`.
#[derive(Debug)]
pub enum ApiError {
    /// No session, or the session is no longer valid.
    Unauthorized(String),
    /// Authenticated but missing a capability.
    Forbidden(String),
    NotFound(String),
    BadRequest(String),
    /// Headscale rejected the request; the status is preserved.
    ///
    /// Boxed because the variant carries the response body, and every handler
    /// returns `ApiResult`; keeping the success path small matters.
    Upstream(Box<HeadscaleError>),
    Internal(anyhow::Error),
}

impl ApiError {
    /// Wraps a message as a 500 internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(anyhow::anyhow!(message.into()))
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(err) => err
                .status()
                .and_then(|status| StatusCode::from_u16(status).ok())
                .unwrap_or(StatusCode::BAD_GATEWAY),
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// A stable machine-readable code for the client to branch on.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::Upstream(err) if err.is_policy_read_only() => "policy_read_only",
            Self::Upstream(err) if err.is_policy_missing() => "policy_missing",
            Self::Upstream(_) => "headscale_error",
            Self::Internal(_) => "internal_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unauthorized(message)
            | Self::Forbidden(message)
            | Self::NotFound(message)
            | Self::BadRequest(message) => message.clone(),
            Self::Upstream(err) => upstream_message(err),
            Self::Internal(err) => {
                tracing::error!("internal error: {err:#}");
                "An internal error occurred. Check the server logs for details.".into()
            }
        }
    }
}

/// Turns a Headscale error into something a user can act on.
fn upstream_message(err: &HeadscaleError) -> String {
    if err.is_policy_read_only() {
        return "The ACL policy is not writable because Headscale is using file mode. Set \
                `policy.mode: database` in the Headscale configuration to enable editing."
            .into();
    }
    if err.is_policy_missing() {
        return "Headscale has no ACL policy configured yet.".into();
    }

    match err {
        HeadscaleError::Api { status, data, raw, .. } => {
            // Headscale returns `{"message": "..."}` for most failures.
            if let Some(message) = data
                .as_ref()
                .and_then(|value| value.get("message"))
                .and_then(|value| value.as_str())
            {
                return message.to_string();
            }
            if let Some(message) = data
                .as_ref()
                .and_then(|value| value.get("error"))
                .and_then(|value| value.as_str())
            {
                return message.to_string();
            }
            if *status == 401 {
                return "Headscale rejected the API key. It may have expired or been deleted."
                    .into();
            }
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                format!("Headscale returned HTTP {status}")
            } else {
                crate::util::truncate(trimmed, 300)
            }
        }
        HeadscaleError::Connection { message, .. } => {
            format!("Could not reach Headscale: {message}")
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(err)
    }
}

impl From<HeadscaleError> for ApiError {
    fn from(err: HeadscaleError) -> Self {
        Self::Upstream(Box::new(err))
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Internal(err.into())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code(),
                message: self.message(),
            },
        };
        (status, Json(body)).into_response()
    }
}

/// Convenience alias used throughout the route modules.
pub type ApiResult<T> = Result<T, ApiError>;
