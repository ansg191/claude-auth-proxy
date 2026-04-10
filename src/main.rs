use std::{env, sync::{Arc, LazyLock}, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    response::Response,
};
use claude_auth_providers::{ClaudeAuthProvider, claude_code::ClaudeCodeAuthProvider};
use claude_auth_transform::{transform_request, transform_response};
use http_body_util::BodyExt;
use reqwest::Client;
use tracing::debug;

static CONNECT_TIMEOUT: LazyLock<Duration> = LazyLock::new(|| {
    env::var("PROXY_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(Duration::from_secs(10), Duration::from_secs)
});

static READ_TIMEOUT: LazyLock<Duration> = LazyLock::new(|| {
    env::var("PROXY_READ_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(Duration::from_secs(600), Duration::from_secs)
});

#[derive(Debug)]
struct ServerState {
    auth: ClaudeCodeAuthProvider,
    client: Client,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    tracing::info!(
        connect_timeout = ?*CONNECT_TIMEOUT,
        read_timeout = ?*READ_TIMEOUT,
        "Proxy timeout configuration"
    );

    let state = Arc::new(ServerState {
        auth: ClaudeCodeAuthProvider::new(),
        client: Client::builder()
            .connect_timeout(*CONNECT_TIMEOUT)
            .read_timeout(*READ_TIMEOUT)
            .build()
            .expect("failed to build HTTP client"),
    });

    let app = Router::new()
        .route("/v1/{*rest}", axum::routing::any(messages_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
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

    debug!("Forwarding request: {} {}", req.method(), req.uri());
    let req = reqwest::Request::try_from(req).unwrap();

    let res = state.client.execute(req).await.unwrap();

    // Convert reqwest::Response → http::Response with a streaming body
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
