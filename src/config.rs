use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use clap::Args;

/// Runtime configuration for the proxy server.
///
/// Values are resolved by clap with the precedence
/// `CLI flag > environment variable > compiled default`.
#[derive(Debug, Clone, Args)]
pub struct ServerConfig {
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
