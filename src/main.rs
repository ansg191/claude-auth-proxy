mod config;

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use claude_auth_providers::{AnyAuthProvider, ClaudeAuthProvider};
use claude_auth_transform::{transform_request, transform_response};
use http_body_util::BodyExt;
use reqwest::Client;
use tokio::signal;
use tracing::{debug, info};

use crate::config::ServerConfig;

/// Upper bound on honoring a `Retry-After` response header. Protects against a
/// misbehaving upstream asking the proxy to stall for hours.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct ServerState {
    auth: AnyAuthProvider,
    client: Client,
    config: ServerConfig,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = ServerConfig::from_env();

    tracing::info!(
        host = %config.host,
        port = config.port,
        connect_timeout = ?config.connect_timeout,
        read_timeout = ?config.read_timeout,
        max_retries = config.max_retries,
        retry_on_5xx = config.retry_on_5xx,
        max_5xx_retries = config.max_5xx_retries,
        "Proxy configuration"
    );

    let host = config.host;
    let port = config.port;
    let state = Arc::new(ServerState {
        auth: AnyAuthProvider::from_env(),
        client: Client::builder()
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            .build()
            .expect("failed to build HTTP client"),
        config,
    });

    let app = Router::new()
        .route("/health", axum::routing::get(health_handler))
        .route("/ready", axum::routing::get(ready_handler))
        .route("/v1/{*rest}", axum::routing::any(messages_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind((host, port)).await.unwrap();
    info!("Listening on {host}:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
    info!("Server shutdown complete");
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("Failed to listen for Ctrl+C") };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    info!("Shutdown signal received, draining in-flight requests...");
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn ready_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    if state.auth.has_credentials() {
        (StatusCode::OK, Json(serde_json::json!({"status": "ready"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "not_ready"})),
        )
    }
}

async fn messages_handler(State(state): State<Arc<ServerState>>, req: Request) -> Response {
    let (mut parts, body) = req.into_parts();
    debug!("Received request: {} {}", parts.method, parts.uri);

    let path_and_query = parts
        .uri
        .path_and_query()
        .cloned()
        .unwrap_or_else(|| http::uri::PathAndQuery::from_static("/"));

    parts.uri = http::uri::Builder::new()
        .scheme("https")
        .authority("api.anthropic.com")
        .path_and_query(path_and_query)
        .build()
        .unwrap();

    parts.headers.remove("host");
    parts.headers.remove("content-length");
    parts.headers.remove("transfer-encoding");
    parts.headers.remove("connection");
    parts.headers.remove("accept-encoding");

    let collected = body.collect().await.unwrap().to_bytes();
    let req = http::Request::from_parts(parts, collected);

    let token = state.auth.get_access_token().await.unwrap();

    let req = transform_request(req, &token).unwrap();

    let (parts, body) = req.into_parts();
    debug!("Forwarding request: {} {}", parts.method, parts.uri);

    // Convert the body to `Bytes` so that cloning for retries is cheap
    // (reference-counted) instead of deep-copying the Vec on each attempt.
    let body = Bytes::from(body);
    let res = execute_with_retry(&state, parts, body).await;

    // Convert reqwest::Response -> http::Response with a streaming body
    let status = res.status();
    let headers = res.headers().clone();
    let stream = res.bytes_stream();
    let stream_body = Body::from_stream(stream);

    let mut response = Response::new(stream_body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;

    // Wrap through ClaudeBody to strip tool prefixes from SSE events
    transform_response(response).map(Body::new)
}

/// Execute the upstream request with retries for transient failures.
///
/// Each retry category has an independent budget so that, for example, a
/// single connect failure does not eat into the budget available for 429s:
/// - 429 (rate limited) and 529 (overloaded): up to `config.max_retries` attempts
/// - Other 5xx: up to `config.max_5xx_retries` attempts, if `config.retry_on_5xx`
/// - Network timeouts / connect errors: up to `config.max_retries` attempts
///
/// On HTTP status exhaustion the final response is returned to the caller so
/// the upstream error is propagated downstream. This function only panics on
/// non-retryable network errors or when the network retry budget is exhausted,
/// matching the existing `.unwrap()` behavior at this call site.
async fn execute_with_retry(
    state: &ServerState,
    parts: http::request::Parts,
    body: Bytes,
) -> reqwest::Response {
    // Each retry category has an independent budget: a failure in one
    // category must not consume retries reserved for another.
    let mut rate_limit_attempts: u32 = 0;
    let mut other_5xx_attempts: u32 = 0;
    let mut network_attempts: u32 = 0;

    loop {
        // Cloning `Bytes` is a cheap Arc bump, not a deep copy of the body.
        let http_req = http::Request::from_parts(parts.clone(), body.clone());
        let reqwest_req = reqwest::Request::try_from(http_req).unwrap();

        match state.client.execute(reqwest_req).await {
            Ok(mut res) => {
                let status = res.status();
                let code = status.as_u16();
                let is_rate_limited = code == 429 || code == 529;
                let is_other_5xx = !is_rate_limited && status.is_server_error();

                // Determine which counter/budget applies, or return the
                // response as-is for non-retryable statuses.
                let (attempt, budget) = if is_rate_limited {
                    (&mut rate_limit_attempts, state.config.max_retries)
                } else if is_other_5xx && state.config.retry_on_5xx {
                    (&mut other_5xx_attempts, state.config.max_5xx_retries)
                } else {
                    return res;
                };

                // `*attempt` is 0-indexed and counts the attempt just made.
                // A budget of N means up to N total attempts, so once the
                // initial call + prior retries reach the budget we stop.
                if *attempt + 1 >= budget {
                    return res;
                }

                let delay = retry_delay(*attempt, res.headers());
                debug!(
                    status = code,
                    attempt = *attempt,
                    delay_secs = delay.as_secs(),
                    "Retryable upstream status, retrying after delay"
                );
                // Drain the response body in a streaming fashion so the
                // connection can be returned to the pool without buffering a
                // potentially large error body in memory.
                while let Ok(Some(_)) = res.chunk().await {}
                tokio::time::sleep(delay).await;
                *attempt += 1;
            }
            Err(e)
                if (e.is_timeout() || e.is_connect())
                    && network_attempts + 1 < state.config.max_retries =>
            {
                let delay = Duration::from_secs(u64::from(network_attempts + 1) * 2);
                debug!(
                    error = %e,
                    attempt = network_attempts,
                    delay_secs = delay.as_secs(),
                    "Transient network error, retrying after delay"
                );
                tokio::time::sleep(delay).await;
                network_attempts += 1;
            }
            Err(e) => panic!("upstream request failed: {e}"),
        }
    }
}

/// Compute how long to wait before the next retry attempt.
///
/// - When the `Retry-After` header is present (integer seconds), it is
///   honored, capped at [`MAX_RETRY_AFTER`].
/// - Otherwise falls back to `(attempt + 1) * 2` seconds of linear backoff.
fn retry_delay(attempt: u32, headers: &reqwest::header::HeaderMap) -> Duration {
    let header_delay = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .map(|d| d.min(MAX_RETRY_AFTER));

    header_delay.unwrap_or_else(|| Duration::from_secs(u64::from(attempt + 1) * 2))
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

    use super::*;

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    fn headers_with_retry_after(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, HeaderValue::from_str(value).unwrap());
        h
    }

    #[test]
    fn retry_delay_first_attempt_uses_backoff() {
        let delay = retry_delay(0, &empty_headers());
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn retry_delay_respects_retry_after_header() {
        let headers = headers_with_retry_after("5");
        let delay = retry_delay(0, &headers);
        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn retry_delay_caps_retry_after_at_max() {
        let headers = headers_with_retry_after("9999");
        let delay = retry_delay(0, &headers);
        assert_eq!(delay, MAX_RETRY_AFTER);
    }

    #[test]
    fn retry_delay_ignores_unparseable_retry_after() {
        let headers = headers_with_retry_after("Wed, 21 Oct 2015 07:28:00 GMT");
        let delay = retry_delay(0, &headers);
        // Falls back to linear backoff
        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn retry_delay_trims_whitespace_in_retry_after() {
        let headers = headers_with_retry_after("  7  ");
        let delay = retry_delay(0, &headers);
        assert_eq!(delay, Duration::from_secs(7));
    }

    #[test]
    fn retry_delay_linear_backoff_progression() {
        let headers = empty_headers();
        assert_eq!(retry_delay(0, &headers), Duration::from_secs(2));
        assert_eq!(retry_delay(1, &headers), Duration::from_secs(4));
        assert_eq!(retry_delay(2, &headers), Duration::from_secs(6));
    }
}
