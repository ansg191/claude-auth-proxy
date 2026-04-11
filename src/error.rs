//! Application error type for the request handler.
//!
//! Request-handler errors are rendered as HTTP responses that mirror
//! Anthropic's public API error envelope, so existing Anthropic SDK clients
//! that hit this proxy can parse and surface the error through their normal
//! error-handling paths. See <https://docs.anthropic.com/en/api/errors>.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("failed to build upstream URI: {0}")]
    BuildUri(#[source] http::Error),

    #[error("failed to read request body: {0}")]
    BodyCollect(#[source] axum::Error),

    #[error("failed to acquire upstream access token: {0}")]
    Auth(#[from] claude_auth_providers::Error),

    #[error("failed to transform request: {0}")]
    Transform(#[from] claude_auth_transform::Error),

    #[error("failed to build upstream request: {0}")]
    BuildRequest(#[source] reqwest::Error),

    #[error("upstream request failed: {0}")]
    Upstream(#[source] reqwest::Error),
}

impl AppError {
    /// HTTP status code returned to the client for this error.
    const fn status_code(&self) -> StatusCode {
        match self {
            Self::BodyCollect(_) | Self::Transform(_) => StatusCode::BAD_REQUEST,
            Self::BuildUri(_) | Self::BuildRequest(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Auth(_) | Self::Upstream(_) => StatusCode::BAD_GATEWAY,
        }
    }

    /// Anthropic-compatible `error.type` discriminant.
    ///
    /// Anthropic's taxonomy has no `bad_gateway` variant, so upstream /
    /// auth failures map onto `api_error` (the general server-side failure
    /// type). Client-payload failures map onto `invalid_request_error`.
    const fn anthropic_error_type(&self) -> &'static str {
        match self {
            Self::BodyCollect(_) | Self::Transform(_) => "invalid_request_error",
            Self::BuildUri(_) | Self::BuildRequest(_) | Self::Auth(_) | Self::Upstream(_) => {
                "api_error"
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let error_type = self.anthropic_error_type();
        tracing::error!(error = %self, status = %status, "request failed");
        let body = Json(serde_json::json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": self.to_string(),
            },
        }));
        (status, body).into_response()
    }
}
