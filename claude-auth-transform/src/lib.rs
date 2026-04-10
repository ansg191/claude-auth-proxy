mod betas;
mod bodies;
mod config;
mod error;
mod response;
mod signing;
mod transforms;

use std::{env, sync::LazyLock};

pub use error::Error;
use http::{HeaderMap, HeaderValue};
pub use response::{ClaudeBody, transform_response};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::{betas::BETA_MANAGER, config::CONFIG, transforms::transform_body};

/// Stable per-process session ID, matching Claude Code's X-Claude-Code-Session-Id
static SESSION_ID: LazyLock<String> = LazyLock::new(|| Uuid::new_v4().to_string());

static ENV_VERSION: LazyLock<Option<String>> =
    LazyLock::new(|| env::var("ANTHROPIC_CLI_VERSION").ok());

pub fn transform_request<B>(request: http::Request<B>) -> Result<http::Request<Vec<u8>>, Error>
where
    B: AsRef<[u8]>,
{
    let (mut parts, body) = request.into_parts();

    // TODO: Process Credentials
    let access_token =
        env::var("ANTHROPIC_ACCESS_TOKEN").expect("ANTHROPIC_ACCESS_TOKEN must be set");

    build_request_headers(&mut parts.headers, &access_token, "claude-opus-4-6");

    let body = transform_body(body.as_ref())?;

    trace!(headers = ?parts.headers, "Transformed Headers");
    trace!(body = %String::from_utf8_lossy(&body).as_ref(), "Transformed Body");

    Ok(http::Request::from_parts(parts, body))
}

fn build_request_headers(headers: &mut HeaderMap, access_token: &str, model_id: &str) {
    let model_betas = BETA_MANAGER.get_model_betas(model_id);
    trace!(?model_betas, model_id, "Model betas");
    let incoming_beta = headers
        .get("anthropic-beta")
        .map(|v| v.to_str().unwrap_or(""))
        .unwrap_or("");
    trace!(?incoming_beta, model_id, "Incoming betas");
    let merged_betas = model_betas
        .into_iter()
        .chain(
            incoming_beta
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        )
        .collect::<Vec<_>>();

    debug!(?merged_betas, model_id, "Computed betas");
    let merged_betas =
        HeaderValue::from_str(&merged_betas.join(",")).expect("Betas should all be valid ascii");

    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {}", access_token)).unwrap(),
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert("anthropic-beta", merged_betas);
    headers.insert("x-app", HeaderValue::from_static("cli"));
    headers.insert("user-agent", get_user_agent());
    headers.insert(
        "x-client-request-id",
        HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap(),
    );
    headers.insert(
        "X-Claude-Code-Session-Id",
        HeaderValue::from_str(&SESSION_ID).unwrap(),
    );
    headers.remove("x-api-key");
}

fn get_user_agent() -> HeaderValue {
    HeaderValue::from_str(&env::var("ANTHROPIC_USER_AGENT").unwrap_or_else(|_| {
        format!(
            "claude-cli/{} (external, cli)",
            ENV_VERSION.as_deref().unwrap_or(CONFIG.cc_version)
        )
    }))
    .expect("User agent must be valid ascii")
}
