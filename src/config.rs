use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    time::Duration,
};

use clap::Args;
use claude_auth_transform::TransformConfig;

/// Runtime configuration for the proxy server.
///
/// Values are resolved by clap with the precedence
/// `CLI flag > environment variable > compiled default`.
#[derive(Debug, Clone, Args)]
pub struct ServerConfig {
    #[command(flatten)]
    pub transform: TransformArgs,

    /// Host address to bind the HTTP listener on.
    #[arg(
        long,
        env = "CLAUDE_PROXY_HOST",
        default_value_t = IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    )]
    pub host: IpAddr,

    /// Port to bind the HTTP listener on.
    #[arg(long, env = "CLAUDE_PROXY_PORT", default_value_t = 3000)]
    pub port: u16,

    /// Connect timeout for upstream requests, in seconds.
    #[arg(
        long = "connect-timeout",
        env = "PROXY_CONNECT_TIMEOUT_SECS",
        value_parser = parse_duration_secs,
        default_value = "10",
    )]
    pub connect_timeout: Duration,

    /// Read timeout for upstream requests, in seconds.
    #[arg(
        long = "read-timeout",
        env = "PROXY_READ_TIMEOUT_SECS",
        value_parser = parse_duration_secs,
        default_value = "600",
    )]
    pub read_timeout: Duration,

    /// Maximum number of attempts for 429/529 responses and transient network
    /// failures (including the initial attempt).
    #[arg(long, env = "PROXY_MAX_RETRIES", default_value_t = 3)]
    pub max_retries: u32,

    /// Retry generic 5xx responses (other than 529) up to `max_5xx_retries`.
    ///
    /// On the CLI this behaves both as a bare flag (`--retry-on-5xx`) and as
    /// an explicit value (`--retry-on-5xx=true` / `--retry-on-5xx=false`).
    /// As an environment variable it accepts the same values the previous
    /// hand-rolled helper did: `1`/`0`/`true`/`false`/`yes`/`no`.
    #[arg(
        long,
        env = "PROXY_RETRY_ON_5XX",
        value_parser = parse_bool_flag,
        num_args = 0..=1,
        require_equals = true,
        default_value_t = false,
        default_missing_value = "true",
    )]
    pub retry_on_5xx: bool,

    /// Maximum number of attempts for generic 5xx responses when
    /// `retry_on_5xx` is enabled. Typically shorter than `max_retries`.
    #[arg(long, env = "PROXY_5XX_MAX_RETRIES", default_value_t = 1)]
    pub max_5xx_retries: u32,

    /// Dump upstream requests to this directory, if set.
    #[arg(long, env = "PROXY_DUMP_REQ_DIR")]
    pub dump_req_dir: Option<PathBuf>,
}

/// Transform-layer configuration parsed from environment variables and CLI
/// flags via clap. Converted into [`TransformConfig`] for the transform crate.
#[derive(Debug, Clone, Args)]
pub struct TransformArgs {
    /// CLI version string for billing headers.
    #[arg(
        long = "cli-version",
        env = "ANTHROPIC_CLI_VERSION",
        default_value = claude_auth_transform::DEFAULT_CC_VERSION,
    )]
    pub cc_version: String,

    /// Entrypoint identifier for billing headers.
    #[arg(
        long = "entrypoint",
        env = "CLAUDE_CODE_ENTRYPOINT",
        default_value = "cli"
    )]
    pub entrypoint: String,

    /// Override the user-agent header sent to upstream.
    #[arg(long = "user-agent", env = "ANTHROPIC_USER_AGENT")]
    pub user_agent_override: Option<String>,

    /// Comma-separated beta flags (replaces defaults when set).
    #[arg(
        long = "beta-flags",
        env = "ANTHROPIC_BETA_FLAGS",
        value_parser = parse_beta_flags,
    )]
    pub beta_flags_override: Option<Vec<String>>,
}

impl TransformArgs {
    /// Convert into a [`TransformConfig`] suitable for the transform crate.
    pub fn into_transform_config(self) -> TransformConfig {
        let default = TransformConfig::default();
        TransformConfig {
            cc_version: self.cc_version,
            entrypoint: self.entrypoint,
            user_agent_override: self.user_agent_override,
            base_betas: self.beta_flags_override.unwrap_or(default.base_betas),
            ..default
        }
    }
}

/// Clap `value_parser` requires `Result`; this never fails.
#[allow(clippy::unnecessary_wraps)]
fn parse_beta_flags(s: &str) -> Result<Vec<String>, String> {
    Ok(s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect())
}

fn parse_duration_secs(s: &str) -> Result<Duration, String> {
    s.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|e| format!("invalid seconds value '{s}': {e}"))
}

fn parse_bool_flag(s: &str) -> Result<bool, String> {
    match s.trim() {
        "1" | "true" | "TRUE" | "True" | "yes" | "YES" => Ok(true),
        "0" | "false" | "FALSE" | "False" | "no" | "NO" => Ok(false),
        other => Err(format!("invalid boolean value: {other}")),
    }
}
