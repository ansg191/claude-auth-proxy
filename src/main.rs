use http_body_util::BodyExt;
use axum::{Json, Router, body::Body, extract::Request};
use axum::response::Response;
use http::HeaderValue;
use claude_auth_transform::transform_request;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let v1 = Router::new().route("/messages", axum::routing::post(messages_handler));

    let app = Router::new().nest("/v1", v1);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap()
}

async fn messages_handler(req: Request) -> Response {
    let (mut parts, body) = req.into_parts();

    // Change host header to api.anthropic.com
    parts.headers.insert("host", HeaderValue::from_static("api.anthropic.com"));

    // Change uri to https://api.anthropic.com/v1/messages
    parts.uri = http::uri::Builder::new()
        .scheme("https")
        .authority("api.anthropic.com")
        .path_and_query("/v1/messages")
        .build()
        .unwrap();

    parts.headers.remove("host");
    parts.headers.remove("content-length");
    parts.headers.remove("transfer-encoding");
    parts.headers.remove("connection");
    parts.headers.remove("accept-encoding");

    let collected = body.collect().await.unwrap().to_bytes();
    let req = http::Request::from_parts(parts, collected);

    let req = transform_request(req).unwrap();

    let (parts, body) = req.into_parts();
    dbg!(&parts);
    dbg!(String::from_utf8_lossy(&body));
    let req = http::Request::from_parts(parts, body);

    let req = reqwest::Request::try_from(req).unwrap();
    let client = reqwest::Client::new();

    let res = client.execute(req).await.unwrap();

    // Convert response to Axum Response
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.bytes().await.unwrap();
    let body = Body::from(body);

    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;

    response
}
