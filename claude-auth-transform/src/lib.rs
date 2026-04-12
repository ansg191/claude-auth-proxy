mod betas;
mod bodies;
mod config;
mod error;
mod response;
mod signing;
mod transforms;

pub use error::Error;
use http::{HeaderMap, HeaderValue};
pub use response::{ClaudeBody, transform_response};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::{betas::BetaManager, transforms::transform_body};

/// Runtime configuration for the transform layer.
///
/// Constructed once at startup from environment variables and defaults.
/// The transform crate itself never reads `env::var` -- the caller is
/// responsible for resolving values and passing them in.
#[derive(Debug, Clone)]
pub struct TransformConfig {
    /// CLI version string (from `ANTHROPIC_CLI_VERSION`, default `ModelConfig.cc_version`).
    pub cc_version: String,
    /// Entrypoint identifier (from `CLAUDE_CODE_ENTRYPOINT`, default `"cli"`).
    pub entrypoint: String,
    /// Full user-agent override (from `ANTHROPIC_USER_AGENT`). When `None`,
    /// computed as `"claude-cli/{cc_version} (external, cli)"`.
    pub user_agent_override: Option<String>,
    /// Beta flag override (from `ANTHROPIC_BETA_FLAGS`). When `None`, uses
    /// `ModelConfig.base_betas`. When `Some`, entirely replaces base betas.
    pub beta_flags_override: Option<Vec<String>>,
    /// Stable per-process session ID.
    pub session_id: String,
}

impl Default for TransformConfig {
    fn default() -> Self {
        Self {
            cc_version: config::CONFIG.cc_version.to_owned(),
            entrypoint: "cli".to_owned(),
            user_agent_override: None,
            beta_flags_override: None,
            session_id: Uuid::new_v4().to_string(),
        }
    }
}

/// Bundles [`TransformConfig`] with stateful components ([`BetaManager`]).
///
/// Created once at startup and shared across requests via `Arc`.
#[derive(Debug)]
pub struct TransformContext {
    pub config: TransformConfig,
    beta_manager: BetaManager,
}

impl TransformContext {
    #[must_use]
    pub fn new(config: TransformConfig) -> Self {
        Self {
            beta_manager: BetaManager::new(),
            config,
        }
    }
}

/// Transform an anthropic API request into a authenticated Claude API request.
///
/// # Arguments
///
/// * `request`: The HTTP request to transform
/// * `access_token`: The access token to use for authentication
/// * `ctx`: Transform context holding runtime config and beta state
///
/// # Errors
///
/// * `Error`: If the request cannot be transformed, e.g., due to invalid body encoding
pub fn transform_request<B>(
    request: http::Request<B>,
    access_token: &str,
    ctx: &TransformContext,
) -> Result<http::Request<Vec<u8>>, Error>
where
    B: AsRef<[u8]>,
{
    let (mut parts, body) = request.into_parts();

    let model_id = serde_json::from_slice::<serde_json::Value>(body.as_ref())
        .ok()
        .and_then(|v| v.get("model")?.as_str().map(String::from));
    build_request_headers(
        &mut parts.headers,
        access_token,
        model_id.as_deref().unwrap_or(""),
        ctx,
    );

    let body = transform_body(body.as_ref(), &ctx.config)?;

    trace!(headers = ?parts.headers, "Transformed Headers");
    trace!(body = %String::from_utf8_lossy(&body).as_ref(), "Transformed Body");

    Ok(http::Request::from_parts(parts, body))
}

fn build_request_headers(
    headers: &mut HeaderMap,
    access_token: &str,
    model_id: &str,
    ctx: &TransformContext,
) {
    let model_betas = ctx.beta_manager.get_model_betas(model_id, &ctx.config);
    trace!(?model_betas, model_id, "Model betas");
    let incoming_beta = headers
        .get("anthropic-beta")
        .map_or("", |v| v.to_str().unwrap_or(""));
    trace!(?incoming_beta, model_id, "Incoming betas");
    let merged_betas: Vec<String> = model_betas
        .into_iter()
        .chain(
            incoming_beta
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
        )
        .collect();

    debug!(?merged_betas, model_id, "Computed betas");
    let merged_betas =
        HeaderValue::from_str(&merged_betas.join(",")).expect("Betas should all be valid ascii");

    let user_agent = ctx.config.user_agent_override.as_ref().map_or_else(
        || {
            HeaderValue::from_str(&format!(
                "claude-cli/{} (external, cli)",
                ctx.config.cc_version
            ))
            .expect("User agent must be valid ascii")
        },
        |ua| HeaderValue::from_str(ua).expect("User agent must be valid ascii"),
    );

    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {access_token}")).unwrap(),
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert("anthropic-beta", merged_betas);
    headers.insert("x-app", HeaderValue::from_static("cli"));
    headers.insert("user-agent", user_agent);
    headers.insert(
        "x-client-request-id",
        HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap(),
    );
    headers.insert(
        "X-Claude-Code-Session-Id",
        HeaderValue::from_str(&ctx.config.session_id).unwrap(),
    );
    headers.remove("x-api-key");
}
