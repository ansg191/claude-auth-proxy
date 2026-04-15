use http::header::InvalidHeaderValue;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid user agent: {0}")]
    InvalidUserAgent(#[source] InvalidHeaderValue),
    #[error("invalid session id: {0}")]
    InvalidSessionId(#[source] InvalidHeaderValue),
    #[error("invalid access token: {0}")]
    InvalidAccessToken(#[source] InvalidHeaderValue),
    #[error("invalid betas: {0}")]
    InvalidBetas(#[source] InvalidHeaderValue),
    #[error("failed to serialize JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("invalid tool-name hash bounds: {0}")]
    InvalidToolNameHashBounds(String),
}
