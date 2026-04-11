use std::{
    env,
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
    time::Duration,
};

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
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            host: parse_or_default("CLAUDE_PROXY_HOST", IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            port: parse_or_default("CLAUDE_PROXY_PORT", 3000),
            connect_timeout: parse_duration_secs("PROXY_CONNECT_TIMEOUT_SECS", 10),
            read_timeout: parse_duration_secs("PROXY_READ_TIMEOUT_SECS", 600),
            max_retries: parse_or_default("PROXY_MAX_RETRIES", 3),
            retry_on_5xx: parse_bool("PROXY_RETRY_ON_5XX", false),
            max_5xx_retries: parse_or_default("PROXY_5XX_MAX_RETRIES", 1),
        }
    }
}

fn parse_duration_secs(var: &str, default_secs: u64) -> Duration {
    Duration::from_secs(parse_or_default(var, default_secs))
}

/// Read the environment variable `var`, parse it as `T`, and return the
/// parsed value. If the variable is unset or cannot be parsed, return
/// `default`.
fn parse_or_default<T: FromStr>(var: &str, default: T) -> T {
    env::var(var)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(default)
}

fn parse_bool(var: &str, default: bool) -> bool {
    env::var(var).ok().map_or(default, |v| match v.trim() {
        "1" | "true" | "TRUE" | "True" | "yes" | "YES" => true,
        "0" | "false" | "FALSE" | "False" | "no" | "NO" => false,
        _ => default,
    })
}
