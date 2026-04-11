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
            // `NoCredentials` is local misconfiguration, not an upstream
            // gateway failure; 503 mirrors the `/ready` handler's response
            // for the same condition.
            Self::Auth(claude_auth_providers::Error::NoCredentials) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
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

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
    use serde_json::Value;

    use super::AppError;

    /// Render an `AppError` through `IntoResponse` and return
    /// `(status, json_body)` for assertions.
    async fn render(err: AppError) -> (StatusCode, Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let json: Value =
            serde_json::from_slice(&bytes).expect("response body should be valid JSON");
        (status, json)
    }

    /// Assert the Anthropic-compatible error envelope shape.
    ///
    /// Matches on the stable message prefix (from our own `#[error("...")]`
    /// attribute) rather than the full string, so upstream error-crate display
    /// tweaks don't break the test.
    fn assert_envelope(json: &Value, expected_type: &str, expected_prefix: &str) {
        assert_eq!(json["type"], "error", "envelope type");
        assert_eq!(json["error"]["type"], expected_type, "error.type");
        let message = json["error"]["message"]
            .as_str()
            .expect("error.message should be a string");
        assert!(
            message.starts_with(expected_prefix),
            "expected message to start with {expected_prefix:?}, got {message:?}",
        );
    }

    #[tokio::test]
    async fn body_collect_maps_to_400_invalid_request_error() {
        let source = std::io::Error::new(std::io::ErrorKind::InvalidData, "bad body");
        let (status, json) = render(AppError::BodyCollect(axum::Error::new(source))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_envelope(
            &json,
            "invalid_request_error",
            "failed to read request body: ",
        );
    }

    #[tokio::test]
    async fn transform_maps_to_400_invalid_request_error() {
        let json_err = serde_json::from_str::<i32>("not-a-number").unwrap_err();
        let err = AppError::Transform(claude_auth_transform::Error::Json(json_err));
        let (status, json) = render(err).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_envelope(
            &json,
            "invalid_request_error",
            "failed to transform request: ",
        );
    }

    #[tokio::test]
    async fn build_uri_maps_to_500_api_error() {
        let source = http::Request::builder()
            .method("not a method")
            .body(())
            .expect_err("invalid HTTP method should produce an http::Error");
        let (status, json) = render(AppError::BuildUri(source)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_envelope(&json, "api_error", "failed to build upstream URI: ");
    }

    #[tokio::test]
    async fn auth_no_credentials_maps_to_503_api_error() {
        let err = AppError::Auth(claude_auth_providers::Error::NoCredentials);
        let (status, json) = render(err).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_envelope(
            &json,
            "api_error",
            "failed to acquire upstream access token: ",
        );
    }

    #[tokio::test]
    async fn auth_other_error_maps_to_502_api_error() {
        let err = AppError::Auth(claude_auth_providers::Error::Refresh(
            "network failure".to_string(),
        ));
        let (status, json) = render(err).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_envelope(
            &json,
            "api_error",
            "failed to acquire upstream access token: ",
        );
    }
}
