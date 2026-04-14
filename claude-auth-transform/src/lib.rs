mod betas;
mod bodies;
mod config;
mod error;
mod response;
mod signing;
mod tool_names;
mod transforms;

use std::sync::Arc;

pub use error::Error;
use http::{HeaderMap, HeaderValue};
pub use response::{ClaudeBody, transform_response};
use tool_names::ToolNameMapper;
use tracing::{debug, trace};
use uuid::Uuid;

use crate::{betas::BetaManager, transforms::transform_body};

/// Default CLI version string (from `ModelConfig`). Exposed as a constant
/// so the main crate can use it for clap default values.
pub const DEFAULT_CC_VERSION: &str = config::CONFIG.cc_version;

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
    /// Resolved base beta flags. When `ANTHROPIC_BETA_FLAGS` is set, this
    /// holds the parsed override; otherwise it holds `ModelConfig.base_betas`.
    /// Cached at startup to avoid per-request allocations.
    pub base_betas: Vec<String>,
    /// Stable per-process session ID.
    pub session_id: String,
    /// Minimum number of hex characters to use when obfuscating tool names.
    pub tool_name_hash_len: usize,
    /// Maximum number of hex characters to use when obfuscating tool names.
    pub tool_name_max_hash_len: usize,
}

impl Default for TransformConfig {
    fn default() -> Self {
        Self {
            cc_version: config::CONFIG.cc_version.to_owned(),
            entrypoint: "cli".to_owned(),
            user_agent_override: None,
            base_betas: config::CONFIG
                .base_betas
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            session_id: Uuid::new_v4().to_string(),
            tool_name_hash_len: 8,
            tool_name_max_hash_len: 16,
        }
    }
}

/// Bundles [`TransformConfig`] with stateful components ([`BetaManager`])
/// and pre-computed header values.
///
/// Created once at startup and shared across requests via `Arc`.
pub struct TransformContext {
    pub config: TransformConfig,
    beta_manager: BetaManager,
    tool_name_mapper: Arc<ToolNameMapper>,
    /// Pre-computed user-agent header value (avoids per-request formatting).
    user_agent: HeaderValue,
}

impl std::fmt::Debug for TransformContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformContext")
            .field("config", &self.config)
            .field("beta_manager", &self.beta_manager)
            .field("tool_name_mapper", &self.tool_name_mapper)
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

impl TransformContext {
    /// Build a new context, pre-computing the user-agent header value.
    ///
    /// # Errors
    ///
    /// Errors if the resolved user-agent string is not valid ASCII.
    pub fn new(config: TransformConfig) -> Result<Self, Error> {
        if config.tool_name_hash_len > config.tool_name_max_hash_len {
            return Err(Error::InvalidToolNameHashBounds(format!(
                "min {} exceeds max {}",
                config.tool_name_hash_len, config.tool_name_max_hash_len
            )));
        }

        let user_agent = config
            .user_agent_override
            .as_ref()
            .map_or_else(
                || {
                    HeaderValue::from_str(&format!(
                        "claude-cli/{} (external, cli)",
                        config.cc_version
                    ))
                },
                |ua| HeaderValue::from_str(ua),
            )
            .map_err(Error::InvalidUserAgent)?;
        Ok(Self {
            beta_manager: BetaManager::new(),
            tool_name_mapper: Arc::new(ToolNameMapper::new(
                config.tool_name_hash_len,
                config.tool_name_max_hash_len,
            )),
            config,
            user_agent,
        })
    }

    pub fn tool_name_mapper(&self) -> Arc<ToolNameMapper> {
        Arc::clone(&self.tool_name_mapper)
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
    )?;

    let body = transform_body(body.as_ref(), &ctx.config, &ctx.tool_name_mapper)?;

    trace!(headers = ?parts.headers, "Transformed Headers");
    trace!(body = %String::from_utf8_lossy(&body).as_ref(), "Transformed Body");

    Ok(http::Request::from_parts(parts, body))
}

fn build_request_headers(
    headers: &mut HeaderMap,
    access_token: &str,
    model_id: &str,
    ctx: &TransformContext,
) -> Result<(), Error> {
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
        HeaderValue::from_str(&merged_betas.join(",")).map_err(Error::InvalidBetas)?;

    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {access_token}"))
            .map_err(Error::InvalidAccessToken)?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert("anthropic-beta", merged_betas);
    headers.insert("x-app", HeaderValue::from_static("cli"));
    headers.insert("user-agent", ctx.user_agent.clone());
    headers.insert(
        "x-client-request-id",
        HeaderValue::from_str(&Uuid::new_v4().to_string()).expect("UUID should be valid ascii"),
    );
    headers.insert(
        "X-Claude-Code-Session-Id",
        HeaderValue::from_str(&ctx.config.session_id).map_err(Error::InvalidSessionId)?,
    );
    headers.remove("x-api-key");
    Ok(())
}
