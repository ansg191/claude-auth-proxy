use std::{env, net::IpAddr, time::Duration};

/// Runtime configuration for the proxy server, populated from environment
/// variables on startup.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Host to bind the HTTP listener on.
    pub host: IpAddr,
    /// Port to bind the HTTP listener on.
    pub port: u16,
    /// Connect timeout for upstream requests.
    pub connect_timeout: Duration,
    /// Read timeout for upstream requests.
    pub read_timeout: Duration,
    /// Maximum number of attempts for 429/529 responses (including the
    /// initial attempt). A value of `3` means 1 initial attempt + 2 retries.
    pub max_retries: u32,
    /// When `true`, 5xx server errors (other than 529) are also retried.
    pub retry_on_5xx: bool,
    /// Maximum number of attempts for 5xx responses when `retry_on_5xx` is
    /// enabled. Typically shorter than `max_retries`.
    pub max_5xx_retries: u32,
}

impl ServerConfig {
    /// Build a `ServerConfig` from environment variables, falling back to
    /// sensible defaults when variables are unset or unparseable.
    pub fn from_env() -> Result<Self, ConfigError> {
        let host = env::var("CLAUDE_PROXY_HOST").unwrap_or_else(|_| "0.0.0.0".to_owned());
        let port = env::var("CLAUDE_PROXY_PORT").unwrap_or_else(|_| "3000".to_owned());
        let host = host
            .parse()
            .map_err(|_| ConfigError::InvalidHost(host.clone()))?;
        let port = port
            .parse()
            .map_err(|_| ConfigError::InvalidPort(port.clone()))?;
        Ok(Self {
            host,
            port,
            connect_timeout: parse_duration_secs("PROXY_CONNECT_TIMEOUT_SECS", 10),
            read_timeout: parse_duration_secs("PROXY_READ_TIMEOUT_SECS", 600),
            max_retries: parse_u32("PROXY_MAX_RETRIES", 3),
            retry_on_5xx: parse_bool("PROXY_RETRY_ON_5XX", false),
            max_5xx_retries: parse_u32("PROXY_5XX_MAX_RETRIES", 1),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid host: {0}")]
    InvalidHost(String),
    #[error("Invalid port: {0}")]
    InvalidPort(String),
}

fn parse_duration_secs(var: &str, default_secs: u64) -> Duration {
    let secs = env::var(var)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default_secs);
    Duration::from_secs(secs)
}

fn parse_u32(var: &str, default: u32) -> u32 {
    env::var(var)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

fn parse_bool(var: &str, default: bool) -> bool {
    env::var(var).ok().map_or(default, |v| match v.trim() {
        "1" | "true" | "TRUE" | "True" | "yes" | "YES" => true,
        "0" | "false" | "FALSE" | "False" | "no" | "NO" => false,
        _ => default,
    })
}
