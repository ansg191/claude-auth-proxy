mod config;
mod error;
mod install;
use std::{io::Write, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use claude_auth_providers::{AnyAuthProvider, ClaudeAuthProvider};
use claude_auth_transform::{TransformContext, transform_request, transform_response};
use http_body_util::BodyExt;
use reqwest::Client;
use tokio::signal;
use tracing::{debug, info, warn};

use crate::{config::ServerConfig, error::AppError};

/// Upper bound on honoring a `Retry-After` response header. Protects against a
/// misbehaving upstream asking the proxy to stall for hours.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct ServerState {
    auth: AnyAuthProvider,
    client: Client,
    config: ServerConfig,
    transform: TransformContext,
}

#[derive(Parser, Debug)]
#[command(
    name = "claude-auth-proxy",
    version,
    about,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the proxy server.
    Run(ServerConfig),
    /// Install the proxy as a macOS launchd user agent.
    Install(install::InstallArgs),
    /// Uninstall the macOS launchd user agent.
    Uninstall,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run(config) => run(config).await?,
        Command::Install(args) => install::install(args)?,
        Command::Uninstall => install::uninstall()?,
    }
    Ok(())
}

async fn run(config: ServerConfig) -> anyhow::Result<()> {
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

    let transform_config = config.transform.clone().into_transform_config();

    tracing::info!(
        cc_version = %transform_config.cc_version,
        entrypoint = %transform_config.entrypoint,
        user_agent_override = ?transform_config.user_agent_override,
        base_betas = ?transform_config.base_betas,
        "Transform configuration"
    );
    tracing::debug!(
        session_id = %transform_config.session_id,
        "Transform session"
    );

    let host = config.host;
    let port = config.port;
    let transform = TransformContext::new(transform_config)?;
    let state = Arc::new(ServerState {
        auth: AnyAuthProvider::from_env(),
        client: Client::builder()
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            .build()
            .expect("failed to build HTTP client"),
        config,
        transform,
    });

    let app = Router::new()
        .route("/health", axum::routing::get(health_handler))
        .route("/ready", axum::routing::get(ready_handler))
        .route("/v1/{*rest}", axum::routing::any(messages_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    info!("Listening on {host}:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    info!("Server shutdown complete");
    Ok(())
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

async fn messages_handler(
    State(state): State<Arc<ServerState>>,
    req: Request,
) -> Result<Response, AppError> {
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
        .map_err(AppError::BuildUri)?;

    parts.headers.remove("host");
    parts.headers.remove("content-length");
    parts.headers.remove("transfer-encoding");
    parts.headers.remove("connection");
    parts.headers.remove("accept-encoding");

    let collected = body
        .collect()
        .await
        .map_err(AppError::BodyCollect)?
        .to_bytes();

    let token = state.auth.get_access_token().await?;

    let req = transform_request(
        http::Request::from_parts(parts.clone(), collected.clone()),
        &token,
        &state.transform,
    )?;

    if let Err(e) = dump_request(&req, &state).await {
        warn!(error = %e, "Failed to dump request");
    }

    let (tx_parts, tx_body) = req.into_parts();
    debug!("Forwarding request: {} {}", tx_parts.method, tx_parts.uri);

    // Convert the body to `Bytes` so that cloning for retries is cheap
    // (reference-counted) instead of deep-copying the Vec on each attempt.
    let tx_body = Bytes::from(tx_body);
    let res = execute_with_retry(&state, tx_parts, tx_body).await?;

    // On 401, attempt a forced credential refresh and retry once.
    let res = if res.status() == StatusCode::UNAUTHORIZED {
        handle_401_retry(&state, &token, parts, collected, res).await?
    } else {
        res
    };

    // Convert reqwest::Response -> http::Response with a streaming body
    let status = res.status();
    let headers = res.headers().clone();
    let stream = res.bytes_stream();
    let stream_body = Body::from_stream(stream);

    let mut response = Response::new(stream_body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;

    // Wrap through ClaudeBody to strip tool prefixes from SSE events
    Ok(transform_response(response).map(Body::new))
}

async fn dump_request(
    req: &http::Request<Vec<u8>>,
    state: &ServerState,
) -> Result<(), std::io::Error> {
    let Some(dump_path) = state.config.dump_req_dir.clone() else {
        return Ok(());
    };

    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let body = req.body().clone();

    tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        let id = headers
            .get("x-client-request-id")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Missing x-client-request-id header for request dump filename",
                )
            })?;

        let filename = format!("req_{id}.txt");
        let path = dump_path.join(filename);

        let mut file = std::fs::File::create(path)?;

        write!(file, "{method} {uri}\r\n")?;
        for (name, value) in &headers {
            write!(
                file,
                "{}: {}\r\n",
                name,
                value.to_str().unwrap_or("<binary>")
            )?;
        }
        write!(file, "\r\n")?;
        file.write_all(&body)?;

        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(format!("request dump task failed: {e}")))?
    .map_err(|e| std::io::Error::new(e.kind(), format!("request dump I/O failed: {e}")))
}

/// When upstream returns 401 Unauthorized, attempt a forced credential
/// refresh and retry the request exactly once with the new token.
///
/// If the refreshed token is identical to the original (meaning the
/// credentials haven't actually changed), or if the refresh itself fails,
/// the original 401 response is returned to the caller.
async fn handle_401_retry(
    state: &ServerState,
    original_token: &str,
    parts: http::request::Parts,
    body: Bytes,
    original_response: reqwest::Response,
) -> Result<reqwest::Response, AppError> {
    debug!("Received 401 from upstream, attempting credential refresh");

    let new_token = match state.auth.force_refresh_token().await {
        Ok(token) => token,
        Err(e) => {
            warn!(error = %e, "Credential refresh failed, returning original 401");
            return Ok(original_response);
        }
    };

    if new_token == original_token {
        debug!("Refreshed token is identical to original, returning 401 to caller");
        return Ok(original_response);
    }

    info!("Credentials refreshed after 401, retrying request with new token");

    let req = transform_request(
        http::Request::from_parts(parts, body),
        &new_token,
        &state.transform,
    )?;
    let (tx_parts, tx_body) = req.into_parts();
    let tx_body = Bytes::from(tx_body);

    match execute_with_retry(state, tx_parts, tx_body).await {
        Ok(retry_response) => {
            // The retry succeeded; drain the original 401 so the connection
            // can return to the pool.
            drain_response(original_response).await;
            Ok(retry_response)
        }
        Err(e) => {
            warn!(error = %e, "Retry after credential refresh failed, returning original 401");
            Ok(original_response)
        }
    }
}

/// Drain a response body in a streaming fashion so the underlying connection
/// can be returned to the pool without buffering in memory.
async fn drain_response(mut response: reqwest::Response) {
    loop {
        match response.chunk().await {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) => {
                debug!(error = %e, "Failed to drain response body, connection may not be reused");
                break;
            }
        }
    }
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
/// the upstream error is propagated downstream. Non-retryable network failures
/// and exhaustion of the network retry budget surface as
/// [`AppError::Upstream`], which the handler renders as a 502 response.
async fn execute_with_retry(
    state: &ServerState,
    parts: http::request::Parts,
    body: Bytes,
) -> Result<reqwest::Response, AppError> {
    // Each retry category has an independent budget: a failure in one
    // category must not consume retries reserved for another.
    let mut rate_limit_attempts: u32 = 0;
    let mut other_5xx_attempts: u32 = 0;
    let mut network_attempts: u32 = 0;

    loop {
        // Cloning `Bytes` is a cheap Arc bump, not a deep copy of the body.
        let http_req = http::Request::from_parts(parts.clone(), body.clone());
        let reqwest_req = reqwest::Request::try_from(http_req).map_err(AppError::BuildRequest)?;

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
                    return Ok(res);
                };

                // `*attempt` is 0-indexed and counts the attempt just made.
                // A budget of N means up to N total attempts, so once the
                // initial call + prior retries reach the budget we stop.
                if *attempt + 1 >= budget {
                    return Ok(res);
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
            Err(e) => return Err(AppError::Upstream(e)),
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
